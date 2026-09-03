//! SMT-backed [`ConstraintTheory`](crate::symbolic::logict::ConstraintTheory)
//! backend (Z3 library, in-process) — Task #22 §4-B.
//!
//! Implementing [`ConstraintTheory`](crate::symbolic::logict::ConstraintTheory)
//! for [`Z3Theory`](crate::symbolic::logict_smt::Z3Theory) makes
//! [`TheoryAlgebra<Z3Theory>`](crate::symbolic::logict::TheoryAlgebra) a
//! [`BooleanAlgebra`](crate::symbolic::BooleanAlgebra) *for free* (see
//! [`crate::symbolic::logict`]): every symbolic-automaton algorithm (emptiness,
//! intersection, complement, determinization, language inclusion) then works over
//! **SMT-theory guards** — booleans, linear integer arithmetic, and fixed-width
//! bitvectors — without changing a single automaton algorithm.
//!
//! # Soundness: the `Sat3` channel for SMT `Unknown`
//!
//! [`ConstraintTheory::propagate`](crate::symbolic::logict::ConstraintTheory::propagate)
//! is *two-valued* — `Some(store)` (consistent) or
//! `None` (inconsistent) — but an SMT solver may return **`Unknown`** (timeout,
//! undecidable fragments, non-linear arithmetic). Collapsing `Unknown` to either side is
//! unsound: as "consistent" it lets an unsatisfiable guard through; as "inconsistent"
//! it rejects a satisfiable one. So [`SmtStore`](crate::symbolic::logict_smt::SmtStore)
//! carries [`Sat3`](crate::symbolic::algebra_tower::Sat3):
//!
//! - `propagate` returns `None` **only** on a proven `Unsat`; both `Sat` and `Unknown`
//!   yield `Some(store)`, recording `Sat3::Sat` / `Sat3::DontKnow`.
//! - [`ConstraintTheory::witness`](crate::symbolic::logict::ConstraintTheory::witness)
//!   returns a model **only** on `Sat3::Sat` — never on
//!   `DontKnow`, so an undecided guard never fabricates a witness.
//!
//! Thus `Unknown` is treated as *possibly satisfiable* — the conservative
//! over-approximation that keeps emptiness / language-inclusion checks sound — and
//! [`Sat3::into_safe_bool`](crate::symbolic::algebra_tower::Sat3::into_safe_bool)
//! forces callers to handle the undecided case rather than
//! silently treat it as `false`. This is exactly why the `algebra_tower`'s
//! three-valued logic is load-bearing here.
//!
//! # Boundary
//!
//! The Z3 **library** (the `z3` crate, dynamically linked against the system libz3) is
//! in-process — in-boundary for `lling-llang`/`pgmcp`. The cvc5 / Z3 **CLI**
//! certificate path (`--produce-proofs` → Alethe/LFSC) is a *subprocess* and lives in
//! the WFST sidecar, never here. A fresh Z3 `Context`/`Solver` is built per check, so
//! no Z3 AST (which borrows its `Context`) is ever stored in a `Store` — keeping
//! [`SmtStore`](crate::symbolic::logict_smt::SmtStore)
//! `Clone + Send + Sync` and lifetime-free.

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use z3::ast::Ast; // brings `_eq` into scope for Int/BV

use super::algebra_tower::Sat3;
use super::logict::{ConstraintTheory, LogicStream};

// ══════════════════════════════════════════════════════════════════════════════
// Constraint AST (self-contained: Clone + Eq + Hash, no Z3 Context lifetime)
// ══════════════════════════════════════════════════════════════════════════════

/// A numeric term: linear integer arithmetic or a fixed-width bitvector.
///
/// Kept independent of any Z3 `Context` so [`SmtConstraint`] satisfies
/// `ConstraintTheory::Constraint: Clone + Eq + Hash`; translated to a fresh Z3 AST at
/// solve time by the private `Z3Env` translator.
pub enum SmtTerm {
    /// Integer literal.
    IntLit(i64),
    /// Integer variable (by name).
    IntVar(String),
    /// Bitvector literal `(value, width)`.
    BvLit(u64, u32),
    /// Bitvector variable `(name, width)`.
    BvVar(String, u32),
    /// `a + b`.
    Add(Box<SmtTerm>, Box<SmtTerm>),
    /// `a - b`.
    Sub(Box<SmtTerm>, Box<SmtTerm>),
    /// `k · a` (linear: integer/bitvector coefficient).
    Scale(i64, Box<SmtTerm>),
}

impl Clone for SmtTerm {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a SmtTerm),
            Add,
            Sub,
            Scale(i64),
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(term) => match term {
                    SmtTerm::IntLit(value) => values.push(SmtTerm::IntLit(*value)),
                    SmtTerm::IntVar(name) => values.push(SmtTerm::IntVar(name.clone())),
                    SmtTerm::BvLit(value, width) => values.push(SmtTerm::BvLit(*value, *width)),
                    SmtTerm::BvVar(name, width) => {
                        values.push(SmtTerm::BvVar(name.clone(), *width));
                    }
                    SmtTerm::Add(left, right) => {
                        tasks.push(Task::Add);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    SmtTerm::Sub(left, right) => {
                        tasks.push(Task::Sub);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    SmtTerm::Scale(coefficient, inner) => {
                        tasks.push(Task::Scale(*coefficient));
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::Add | Task::Sub => {
                    let right = values.pop().expect("right term clone is present");
                    let left = values.pop().expect("left term clone is present");
                    values.push(if matches!(task, Task::Add) {
                        SmtTerm::Add(Box::new(left), Box::new(right))
                    } else {
                        SmtTerm::Sub(Box::new(left), Box::new(right))
                    });
                }
                Task::Scale(coefficient) => {
                    let inner = values.pop().expect("scaled term clone is present");
                    values.push(SmtTerm::Scale(coefficient, Box::new(inner)));
                }
            }
        }
        values.pop().expect("the root term produces one clone")
    }
}

impl PartialEq for SmtTerm {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (SmtTerm::IntLit(left), SmtTerm::IntLit(right)) if left == right => {}
                (SmtTerm::IntVar(left), SmtTerm::IntVar(right)) if left == right => {}
                (SmtTerm::BvLit(lv, lw), SmtTerm::BvLit(rv, rw)) if lv == rv && lw == rw => {}
                (SmtTerm::BvVar(ln, lw), SmtTerm::BvVar(rn, rw)) if ln == rn && lw == rw => {}
                (SmtTerm::Add(la, lb), SmtTerm::Add(ra, rb))
                | (SmtTerm::Sub(la, lb), SmtTerm::Sub(ra, rb)) => {
                    pending.push((lb, rb));
                    pending.push((la, ra));
                }
                (SmtTerm::Scale(lk, left), SmtTerm::Scale(rk, right)) if lk == rk => {
                    pending.push((left, right));
                }
                _ => return false,
            }
        }
        true
    }
}

impl Eq for SmtTerm {}

impl Hash for SmtTerm {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(term) = pending.pop() {
            std::mem::discriminant(term).hash(state);
            match term {
                SmtTerm::IntLit(value) => value.hash(state),
                SmtTerm::IntVar(name) => name.hash(state),
                SmtTerm::BvLit(value, width) => {
                    value.hash(state);
                    width.hash(state);
                }
                SmtTerm::BvVar(name, width) => {
                    name.hash(state);
                    width.hash(state);
                }
                SmtTerm::Add(left, right) | SmtTerm::Sub(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                SmtTerm::Scale(coefficient, inner) => {
                    coefficient.hash(state);
                    pending.push(inner);
                }
            }
        }
    }
}

impl fmt::Debug for SmtTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Term(&'a SmtTerm),
            Text(&'static str),
        }
        let mut events = vec![Event::Term(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Term(term) => match term {
                    SmtTerm::IntLit(value) => write!(f, "IntLit({value:?})")?,
                    SmtTerm::IntVar(name) => write!(f, "IntVar({name:?})")?,
                    SmtTerm::BvLit(value, width) => write!(f, "BvLit({value:?}, {width:?})")?,
                    SmtTerm::BvVar(name, width) => write!(f, "BvVar({name:?}, {width:?})")?,
                    SmtTerm::Add(left, right) | SmtTerm::Sub(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(term, SmtTerm::Add(_, _)) {
                                "Add"
                            } else {
                                "Sub"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Term(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Term(left));
                    }
                    SmtTerm::Scale(coefficient, inner) => {
                        write!(f, "Scale({coefficient:?}, ")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Term(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for SmtTerm {
    fn drop(&mut self) {
        fn drain(term: &mut SmtTerm, pending: &mut Vec<SmtTerm>) {
            match term {
                SmtTerm::Add(left, right) | SmtTerm::Sub(left, right) => {
                    pending.push(std::mem::replace(&mut **right, SmtTerm::IntLit(0)));
                    pending.push(std::mem::replace(&mut **left, SmtTerm::IntLit(0)));
                }
                SmtTerm::Scale(_, inner) => {
                    pending.push(std::mem::replace(&mut **inner, SmtTerm::IntLit(0)));
                }
                SmtTerm::IntLit(_)
                | SmtTerm::IntVar(_)
                | SmtTerm::BvLit(_, _)
                | SmtTerm::BvVar(_, _) => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut term) = pending.pop() {
            drain(&mut term, &mut pending);
        }
    }
}

/// A guard constraint over [`SmtTerm`]s: booleans + (in)equalities. Boolean
/// connectives compose constraints; comparisons relate two terms **of the same sort**
/// (both integer or both bitvector of equal width).
pub enum SmtConstraint {
    /// Constant truth.
    True,
    /// Constant falsity.
    False,
    /// Boolean variable (by name).
    BoolVar(String),
    /// `a = b`.
    Eq(SmtTerm, SmtTerm),
    /// $`a\le b`$ (signed for integers, unsigned for bitvectors).
    Le(SmtTerm, SmtTerm),
    /// `a < b`.
    Lt(SmtTerm, SmtTerm),
    /// $`a\ge b`$.
    Ge(SmtTerm, SmtTerm),
    /// `a > b`.
    Gt(SmtTerm, SmtTerm),
    /// $`\lnot a`$.
    Not(Box<SmtConstraint>),
    /// $`a\land b`$.
    And(Box<SmtConstraint>, Box<SmtConstraint>),
    /// $`a\lor b`$.
    Or(Box<SmtConstraint>, Box<SmtConstraint>),
}

impl Clone for SmtConstraint {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a SmtConstraint),
            Not,
            And,
            Or,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(constraint) => match constraint {
                    SmtConstraint::True => values.push(SmtConstraint::True),
                    SmtConstraint::False => values.push(SmtConstraint::False),
                    SmtConstraint::BoolVar(name) => {
                        values.push(SmtConstraint::BoolVar(name.clone()));
                    }
                    SmtConstraint::Eq(left, right) => {
                        values.push(SmtConstraint::Eq(left.clone(), right.clone()));
                    }
                    SmtConstraint::Le(left, right) => {
                        values.push(SmtConstraint::Le(left.clone(), right.clone()));
                    }
                    SmtConstraint::Lt(left, right) => {
                        values.push(SmtConstraint::Lt(left.clone(), right.clone()));
                    }
                    SmtConstraint::Ge(left, right) => {
                        values.push(SmtConstraint::Ge(left.clone(), right.clone()));
                    }
                    SmtConstraint::Gt(left, right) => {
                        values.push(SmtConstraint::Gt(left.clone(), right.clone()));
                    }
                    SmtConstraint::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                    SmtConstraint::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    SmtConstraint::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                },
                Task::Not => {
                    let inner = values.pop().expect("negated constraint clone is present");
                    values.push(SmtConstraint::Not(Box::new(inner)));
                }
                Task::And | Task::Or => {
                    let right = values.pop().expect("right constraint clone is present");
                    let left = values.pop().expect("left constraint clone is present");
                    values.push(if matches!(task, Task::And) {
                        SmtConstraint::And(Box::new(left), Box::new(right))
                    } else {
                        SmtConstraint::Or(Box::new(left), Box::new(right))
                    });
                }
            }
        }
        values
            .pop()
            .expect("the root constraint produces one clone")
    }
}

impl PartialEq for SmtConstraint {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (SmtConstraint::True, SmtConstraint::True)
                | (SmtConstraint::False, SmtConstraint::False) => {}
                (SmtConstraint::BoolVar(left), SmtConstraint::BoolVar(right)) if left == right => {}
                (SmtConstraint::Eq(la, lb), SmtConstraint::Eq(ra, rb))
                | (SmtConstraint::Le(la, lb), SmtConstraint::Le(ra, rb))
                | (SmtConstraint::Lt(la, lb), SmtConstraint::Lt(ra, rb))
                | (SmtConstraint::Ge(la, lb), SmtConstraint::Ge(ra, rb))
                | (SmtConstraint::Gt(la, lb), SmtConstraint::Gt(ra, rb))
                    if la == ra && lb == rb => {}
                (SmtConstraint::Not(left), SmtConstraint::Not(right)) => {
                    pending.push((left, right));
                }
                (SmtConstraint::And(la, lb), SmtConstraint::And(ra, rb))
                | (SmtConstraint::Or(la, lb), SmtConstraint::Or(ra, rb)) => {
                    pending.push((lb, rb));
                    pending.push((la, ra));
                }
                _ => return false,
            }
        }
        true
    }
}

impl Eq for SmtConstraint {}

impl Hash for SmtConstraint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(constraint) = pending.pop() {
            std::mem::discriminant(constraint).hash(state);
            match constraint {
                SmtConstraint::True | SmtConstraint::False => {}
                SmtConstraint::BoolVar(name) => name.hash(state),
                SmtConstraint::Eq(left, right)
                | SmtConstraint::Le(left, right)
                | SmtConstraint::Lt(left, right)
                | SmtConstraint::Ge(left, right)
                | SmtConstraint::Gt(left, right) => {
                    left.hash(state);
                    right.hash(state);
                }
                SmtConstraint::Not(inner) => pending.push(inner),
                SmtConstraint::And(left, right) | SmtConstraint::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
            }
        }
    }
}

impl fmt::Debug for SmtConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Constraint(&'a SmtConstraint),
            Text(&'static str),
        }
        let mut events = vec![Event::Constraint(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Constraint(constraint) => match constraint {
                    SmtConstraint::True => write!(f, "True")?,
                    SmtConstraint::False => write!(f, "False")?,
                    SmtConstraint::BoolVar(name) => write!(f, "BoolVar({name:?})")?,
                    SmtConstraint::Eq(left, right)
                    | SmtConstraint::Le(left, right)
                    | SmtConstraint::Lt(left, right)
                    | SmtConstraint::Ge(left, right)
                    | SmtConstraint::Gt(left, right) => {
                        let name = match constraint {
                            SmtConstraint::Eq(_, _) => "Eq",
                            SmtConstraint::Le(_, _) => "Le",
                            SmtConstraint::Lt(_, _) => "Lt",
                            SmtConstraint::Ge(_, _) => "Ge",
                            SmtConstraint::Gt(_, _) => "Gt",
                            _ => unreachable!(),
                        };
                        write!(f, "{name}({left:?}, {right:?})")?;
                    }
                    SmtConstraint::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Constraint(inner));
                    }
                    SmtConstraint::And(left, right) | SmtConstraint::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(constraint, SmtConstraint::And(_, _)) {
                                "And"
                            } else {
                                "Or"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Constraint(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Constraint(left));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for SmtConstraint {
    fn drop(&mut self) {
        fn drain(constraint: &mut SmtConstraint, pending: &mut Vec<SmtConstraint>) {
            match constraint {
                SmtConstraint::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, SmtConstraint::False));
                }
                SmtConstraint::And(left, right) | SmtConstraint::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, SmtConstraint::False));
                    pending.push(std::mem::replace(&mut **left, SmtConstraint::False));
                }
                SmtConstraint::True
                | SmtConstraint::False
                | SmtConstraint::BoolVar(_)
                | SmtConstraint::Eq(_, _)
                | SmtConstraint::Le(_, _)
                | SmtConstraint::Lt(_, _)
                | SmtConstraint::Ge(_, _)
                | SmtConstraint::Gt(_, _) => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut constraint) = pending.pop() {
            drain(&mut constraint, &mut pending);
        }
    }
}

/// A satisfying assignment extracted from a [`Sat3::Sat`] store.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SmtModel {
    /// Integer variable assignments.
    pub ints: HashMap<String, i64>,
    /// Bitvector variable assignments (value masked to width).
    pub bvs: HashMap<String, u64>,
    /// Boolean variable assignments.
    pub bools: HashMap<String, bool>,
}

/// Accumulated assertions plus the tri-state of the last check.
#[derive(Clone, Debug)]
pub struct SmtStore {
    /// The asserted guard constraints (conjoined).
    pub asserts: Vec<SmtConstraint>,
    /// Tri-state result of the most recent solve over `asserts`.
    pub status: Sat3,
}

// ══════════════════════════════════════════════════════════════════════════════
// Z3Theory
// ══════════════════════════════════════════════════════════════════════════════

/// A [`ConstraintTheory`] backed by the in-process Z3 library.
#[derive(Clone, Debug)]
pub struct Z3Theory {
    /// Per-check solver timeout in milliseconds (`0` = no timeout).
    pub timeout_ms: u32,
}

impl Default for Z3Theory {
    fn default() -> Self {
        Z3Theory { timeout_ms: 5_000 }
    }
}

/// Runtime probe: can a Z3 `Context` be constructed? Cached after the first call;
/// never panics (a missing/incompatible libz3 yields `false` rather than aborting).
pub fn z3_available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        std::panic::catch_unwind(|| {
            let cfg = z3::Config::new();
            let _ctx = z3::Context::new(&cfg);
            true
        })
        .unwrap_or(false)
    })
}

impl Z3Theory {
    /// Construct a theory iff Z3 is available at runtime; otherwise `None`.
    pub fn new() -> Option<Self> {
        z3_available().then(Z3Theory::default)
    }

    /// Solve `asserts` for satisfiability, optionally extracting a model on `Sat`.
    fn solve(&self, asserts: &[SmtConstraint], want_model: bool) -> (Sat3, Option<SmtModel>) {
        let mut cfg = z3::Config::new();
        if self.timeout_ms > 0 {
            cfg.set_timeout_msec(self.timeout_ms as u64);
        }
        let ctx = z3::Context::new(&cfg);
        let solver = z3::Solver::new(&ctx);
        let mut env = Z3Env::new(&ctx);
        for c in asserts {
            let b = env.constraint(c);
            solver.assert(&b);
        }
        match solver.check() {
            z3::SatResult::Unsat => (Sat3::Unsat, None),
            z3::SatResult::Unknown => (Sat3::DontKnow, None),
            z3::SatResult::Sat => {
                let model = if want_model {
                    solver.get_model().map(|m| env.extract_model(&m))
                } else {
                    None
                };
                (Sat3::Sat, model)
            }
        }
    }
}

impl ConstraintTheory for Z3Theory {
    type Constraint = SmtConstraint;
    type Assignment = SmtModel;
    type Store = SmtStore;

    fn empty_store(&self) -> Self::Store {
        // The empty conjunction is trivially satisfiable.
        SmtStore {
            asserts: Vec::new(),
            status: Sat3::Sat,
        }
    }

    fn propagate(&self, store: &Self::Store, c: &Self::Constraint) -> Option<Self::Store> {
        let mut asserts = store.asserts.clone();
        asserts.push(c.clone());
        let (status, _) = self.solve(&asserts, false);
        match status {
            // A proven Unsat is the ONLY inconsistency. `Unknown` (DontKnow) is kept
            // as "possibly satisfiable" — sound for the over-approximating emptiness /
            // inclusion checks the automata layer performs.
            Sat3::Unsat => None,
            Sat3::Sat | Sat3::DontKnow => Some(SmtStore { asserts, status }),
        }
    }

    fn is_consistent(&self, store: &Self::Store) -> bool {
        store.status != Sat3::Unsat
    }

    fn witness(&self, store: &Self::Store) -> Option<Self::Assignment> {
        // A witness is produced ONLY from a definitely-`Sat` store — never from
        // `DontKnow` (an undecided guard must not fabricate a model).
        match store.status {
            Sat3::Sat => self.solve(&store.asserts, true).1,
            Sat3::Unsat | Sat3::DontKnow => None,
        }
    }

    fn label(&self, _store: &Self::Store) -> LogicStream<Self::Constraint> {
        // Z3 decides ground guards by `check-sat`; propagation is the oracle, so no
        // explicit labeling search is generated (cf. the decidable-theory convention).
        LogicStream::empty()
    }

    fn evaluate(&self, c: &Self::Constraint, assignment: &Self::Assignment) -> bool {
        eval_constraint(c, assignment)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Pure evaluator (model checking a constraint under an assignment)
// ══════════════════════════════════════════════════════════════════════════════

/// Evaluate an [`SmtTerm`] under an assignment. Unbound variables default to `0`
/// (a satisfying model from Z3 binds every relevant variable, so this only affects
/// terms over variables outside the witness).
fn eval_term(t: &SmtTerm, m: &SmtModel) -> i64 {
    enum Task<'a> {
        Eval(&'a SmtTerm),
        Add,
        Sub,
        Scale(i64),
    }

    let mut tasks = vec![Task::Eval(t)];
    let mut values = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Eval(term) => match term {
                SmtTerm::IntLit(value) => values.push(*value),
                SmtTerm::IntVar(name) => {
                    values.push(m.ints.get(name).copied().unwrap_or(0));
                }
                SmtTerm::BvLit(value, _) => values.push(*value as i64),
                SmtTerm::BvVar(name, _) => {
                    values.push(m.bvs.get(name).copied().unwrap_or(0) as i64);
                }
                SmtTerm::Add(left, right) => {
                    tasks.push(Task::Add);
                    tasks.push(Task::Eval(right));
                    tasks.push(Task::Eval(left));
                }
                SmtTerm::Sub(left, right) => {
                    tasks.push(Task::Sub);
                    tasks.push(Task::Eval(right));
                    tasks.push(Task::Eval(left));
                }
                SmtTerm::Scale(coefficient, inner) => {
                    tasks.push(Task::Scale(*coefficient));
                    tasks.push(Task::Eval(inner));
                }
            },
            Task::Add => {
                let right = values.pop().expect("right addition operand is present");
                let left = values.pop().expect("left addition operand is present");
                values.push(left.wrapping_add(right));
            }
            Task::Sub => {
                let right = values.pop().expect("right subtraction operand is present");
                let left = values.pop().expect("left subtraction operand is present");
                values.push(left.wrapping_sub(right));
            }
            Task::Scale(coefficient) => {
                let value = values.pop().expect("scaled operand is present");
                values.push(coefficient.wrapping_mul(value));
            }
        }
    }
    values.pop().expect("the root term produces one value")
}

/// Evaluate an [`SmtConstraint`] under an assignment.
fn eval_constraint(c: &SmtConstraint, m: &SmtModel) -> bool {
    enum Frame<'a> {
        Eval(&'a SmtConstraint),
        Not,
        AndRight(&'a SmtConstraint),
        OrRight(&'a SmtConstraint),
    }

    let mut frames = vec![Frame::Eval(c)];
    let mut result = None;
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Eval(constraint) => match constraint {
                SmtConstraint::True => result = Some(true),
                SmtConstraint::False => result = Some(false),
                SmtConstraint::BoolVar(name) => {
                    result = Some(m.bools.get(name).copied().unwrap_or(false));
                }
                SmtConstraint::Eq(left, right) => {
                    result = Some(eval_term(left, m) == eval_term(right, m));
                }
                SmtConstraint::Le(left, right) => {
                    result = Some(eval_term(left, m) <= eval_term(right, m));
                }
                SmtConstraint::Lt(left, right) => {
                    result = Some(eval_term(left, m) < eval_term(right, m));
                }
                SmtConstraint::Ge(left, right) => {
                    result = Some(eval_term(left, m) >= eval_term(right, m));
                }
                SmtConstraint::Gt(left, right) => {
                    result = Some(eval_term(left, m) > eval_term(right, m));
                }
                SmtConstraint::Not(inner) => {
                    frames.push(Frame::Not);
                    frames.push(Frame::Eval(inner));
                }
                SmtConstraint::And(left, right) => {
                    frames.push(Frame::AndRight(right));
                    frames.push(Frame::Eval(left));
                }
                SmtConstraint::Or(left, right) => {
                    frames.push(Frame::OrRight(right));
                    frames.push(Frame::Eval(left));
                }
            },
            Frame::Not => {
                result = Some(!result.take().expect("negation operand was evaluated"));
            }
            Frame::AndRight(right) => {
                if result
                    .take()
                    .expect("left conjunction operand was evaluated")
                {
                    frames.push(Frame::Eval(right));
                } else {
                    result = Some(false);
                }
            }
            Frame::OrRight(right) => {
                if result
                    .take()
                    .expect("left disjunction operand was evaluated")
                {
                    result = Some(true);
                } else {
                    frames.push(Frame::Eval(right));
                }
            }
        }
    }
    result.expect("the root constraint produces one Boolean result")
}

// ══════════════════════════════════════════════════════════════════════════════
// Z3 translation environment
// ══════════════════════════════════════════════════════════════════════════════

/// A translated numeric term — either an integer or a fixed-width bitvector AST.
enum Z3Num<'ctx> {
    Int(z3::ast::Int<'ctx>),
    Bv(z3::ast::BV<'ctx>),
}

/// Builds Z3 ASTs from the self-contained constraint AST, caching declared variables
/// so repeated occurrences share one Z3 constant.
struct Z3Env<'ctx> {
    ctx: &'ctx z3::Context,
    ints: HashMap<String, z3::ast::Int<'ctx>>,
    bvs: HashMap<String, (z3::ast::BV<'ctx>, u32)>,
    bools: HashMap<String, z3::ast::Bool<'ctx>>,
}

impl<'ctx> Z3Env<'ctx> {
    fn new(ctx: &'ctx z3::Context) -> Self {
        Z3Env {
            ctx,
            ints: HashMap::new(),
            bvs: HashMap::new(),
            bools: HashMap::new(),
        }
    }

    fn int_var(&mut self, name: &str) -> z3::ast::Int<'ctx> {
        self.ints
            .entry(name.to_string())
            .or_insert_with(|| z3::ast::Int::new_const(self.ctx, name))
            .clone()
    }

    fn bv_var(&mut self, name: &str, width: u32) -> z3::ast::BV<'ctx> {
        self.bvs
            .entry(name.to_string())
            .or_insert_with(|| (z3::ast::BV::new_const(self.ctx, name, width), width))
            .0
            .clone()
    }

    fn bool_var(&mut self, name: &str) -> z3::ast::Bool<'ctx> {
        self.bools
            .entry(name.to_string())
            .or_insert_with(|| z3::ast::Bool::new_const(self.ctx, name))
            .clone()
    }

    fn term(&mut self, t: &SmtTerm) -> Z3Num<'ctx> {
        enum Task<'a> {
            Eval(&'a SmtTerm),
            Add,
            Sub,
            Scale(i64),
        }

        let mut tasks = vec![Task::Eval(t)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Eval(term) => match term {
                    SmtTerm::IntLit(value) => {
                        values.push(Z3Num::Int(z3::ast::Int::from_i64(self.ctx, *value)));
                    }
                    SmtTerm::IntVar(name) => values.push(Z3Num::Int(self.int_var(name))),
                    SmtTerm::BvLit(value, width) => {
                        values.push(Z3Num::Bv(z3::ast::BV::from_u64(self.ctx, *value, *width)))
                    }
                    SmtTerm::BvVar(name, width) => {
                        values.push(Z3Num::Bv(self.bv_var(name, *width)));
                    }
                    SmtTerm::Add(left, right) => {
                        tasks.push(Task::Add);
                        tasks.push(Task::Eval(right));
                        tasks.push(Task::Eval(left));
                    }
                    SmtTerm::Sub(left, right) => {
                        tasks.push(Task::Sub);
                        tasks.push(Task::Eval(right));
                        tasks.push(Task::Eval(left));
                    }
                    SmtTerm::Scale(coefficient, inner) => {
                        tasks.push(Task::Scale(*coefficient));
                        tasks.push(Task::Eval(inner));
                    }
                },
                Task::Add | Task::Sub => {
                    let right = values.pop().expect("right numeric operand is present");
                    let left = values.pop().expect("left numeric operand is present");
                    values.push(match (left, right, matches!(task, Task::Add)) {
                        (Z3Num::Int(x), Z3Num::Int(y), true) => Z3Num::Int(x + y),
                        (Z3Num::Int(x), Z3Num::Int(y), false) => Z3Num::Int(x - y),
                        (Z3Num::Bv(x), Z3Num::Bv(y), true) => Z3Num::Bv(x.bvadd(&y)),
                        (Z3Num::Bv(x), Z3Num::Bv(y), false) => Z3Num::Bv(x.bvsub(&y)),
                        // Preserve the historical mixed-sort behavior: both
                        // operands are translated, then the left reading wins.
                        (Z3Num::Int(x), _, _) => Z3Num::Int(x),
                        (Z3Num::Bv(x), _, _) => Z3Num::Bv(x),
                    });
                }
                Task::Scale(coefficient) => {
                    let value = values.pop().expect("scaled numeric operand is present");
                    values.push(match value {
                        Z3Num::Int(x) => {
                            Z3Num::Int(z3::ast::Int::from_i64(self.ctx, coefficient) * x)
                        }
                        Z3Num::Bv(x) => {
                            let width = x.get_size();
                            Z3Num::Bv(
                                z3::ast::BV::from_u64(self.ctx, coefficient as u64, width)
                                    .bvmul(&x),
                            )
                        }
                    });
                }
            }
        }
        values.pop().expect("the root term produces one Z3 AST")
    }

    fn constraint(&mut self, c: &SmtConstraint) -> z3::ast::Bool<'ctx> {
        enum Task<'a> {
            Eval(&'a SmtConstraint),
            Not,
            And,
            Or,
        }

        let mut tasks = vec![Task::Eval(c)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Eval(constraint) => match constraint {
                    SmtConstraint::True => {
                        values.push(z3::ast::Bool::from_bool(self.ctx, true));
                    }
                    SmtConstraint::False => {
                        values.push(z3::ast::Bool::from_bool(self.ctx, false));
                    }
                    SmtConstraint::BoolVar(name) => values.push(self.bool_var(name)),
                    SmtConstraint::Eq(left, right) => {
                        values.push(self.compare(left, right, Cmp::Eq));
                    }
                    SmtConstraint::Le(left, right) => {
                        values.push(self.compare(left, right, Cmp::Le));
                    }
                    SmtConstraint::Lt(left, right) => {
                        values.push(self.compare(left, right, Cmp::Lt));
                    }
                    SmtConstraint::Ge(left, right) => {
                        values.push(self.compare(left, right, Cmp::Ge));
                    }
                    SmtConstraint::Gt(left, right) => {
                        values.push(self.compare(left, right, Cmp::Gt));
                    }
                    SmtConstraint::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Eval(inner));
                    }
                    SmtConstraint::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Eval(right));
                        tasks.push(Task::Eval(left));
                    }
                    SmtConstraint::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Eval(right));
                        tasks.push(Task::Eval(left));
                    }
                },
                Task::Not => {
                    let value = values.pop().expect("negated Z3 AST is present");
                    values.push(value.not());
                }
                Task::And | Task::Or => {
                    let right = values.pop().expect("right Boolean Z3 AST is present");
                    let left = values.pop().expect("left Boolean Z3 AST is present");
                    values.push(if matches!(task, Task::And) {
                        z3::ast::Bool::and(self.ctx, &[&left, &right])
                    } else {
                        z3::ast::Bool::or(self.ctx, &[&left, &right])
                    });
                }
            }
        }
        values
            .pop()
            .expect("the root constraint produces one Boolean Z3 AST")
    }

    fn compare(&mut self, a: &SmtTerm, b: &SmtTerm, cmp: Cmp) -> z3::ast::Bool<'ctx> {
        match (self.term(a), self.term(b)) {
            (Z3Num::Int(x), Z3Num::Int(y)) => match cmp {
                Cmp::Eq => x._eq(&y),
                Cmp::Le => x.le(&y),
                Cmp::Lt => x.lt(&y),
                Cmp::Ge => x.ge(&y),
                Cmp::Gt => x.gt(&y),
            },
            (Z3Num::Bv(x), Z3Num::Bv(y)) => match cmp {
                Cmp::Eq => x._eq(&y),
                Cmp::Le => x.bvule(&y),
                Cmp::Lt => x.bvult(&y),
                Cmp::Ge => x.bvuge(&y),
                Cmp::Gt => x.bvugt(&y),
            },
            // Mismatched sorts: an ill-typed guard — treat as unconstrained `true`
            // rather than abort. (The constraint builder upstream keeps sorts aligned.)
            _ => z3::ast::Bool::from_bool(self.ctx, true),
        }
    }

    fn extract_model(&self, model: &z3::Model<'ctx>) -> SmtModel {
        let mut out = SmtModel::default();
        for (name, ast) in &self.ints {
            if let Some(v) = model.eval(ast, true).and_then(|a| a.as_i64()) {
                out.ints.insert(name.clone(), v);
            }
        }
        for (name, (ast, _w)) in &self.bvs {
            if let Some(v) = model.eval(ast, true).and_then(|a| a.as_u64()) {
                out.bvs.insert(name.clone(), v);
            }
        }
        for (name, ast) in &self.bools {
            if let Some(v) = model.eval(ast, true).and_then(|a| a.as_bool()) {
                out.bools.insert(name.clone(), v);
            }
        }
        out
    }
}

/// Comparison operator selector for [`Z3Env::compare`].
#[derive(Clone, Copy)]
enum Cmp {
    Eq,
    Le,
    Lt,
    Ge,
    Gt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const DEEP_Z3_AST_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    fn ivar(s: &str) -> SmtTerm {
        SmtTerm::IntVar(s.to_string())
    }
    fn ilit(n: i64) -> SmtTerm {
        SmtTerm::IntLit(n)
    }

    fn integer_term_strategy() -> BoxedStrategy<SmtTerm> {
        (-8i64..=8)
            .prop_map(SmtTerm::IntLit)
            .prop_recursive(5, 96, 3, |inner| {
                prop_oneof![
                    (inner.clone(), inner.clone()).prop_map(|(left, right)| {
                        SmtTerm::Add(Box::new(left), Box::new(right))
                    }),
                    (inner.clone(), inner.clone()).prop_map(|(left, right)| {
                        SmtTerm::Sub(Box::new(left), Box::new(right))
                    }),
                    (-2i64..=2, inner).prop_map(|(coefficient, term)| {
                        SmtTerm::Scale(coefficient, Box::new(term))
                    }),
                ]
            })
            .boxed()
    }

    fn ground_constraint_strategy() -> BoxedStrategy<SmtConstraint> {
        let terms = integer_term_strategy();
        prop_oneof![
            Just(SmtConstraint::True),
            Just(SmtConstraint::False),
            (terms.clone(), terms.clone())
                .prop_map(|(left, right)| { SmtConstraint::Eq(left, right) }),
            (terms.clone(), terms.clone())
                .prop_map(|(left, right)| { SmtConstraint::Le(left, right) }),
            (terms.clone(), terms).prop_map(|(left, right)| { SmtConstraint::Gt(left, right) }),
        ]
        .prop_recursive(5, 96, 3, |inner| {
            prop_oneof![
                inner
                    .clone()
                    .prop_map(|value| SmtConstraint::Not(Box::new(value))),
                (inner.clone(), inner.clone()).prop_map(|(left, right)| {
                    SmtConstraint::And(Box::new(left), Box::new(right))
                }),
                (inner.clone(), inner).prop_map(|(left, right)| {
                    SmtConstraint::Or(Box::new(left), Box::new(right))
                }),
            ]
        })
        .boxed()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn z3_integer_translation_refines_ground_evaluation(term in integer_term_strategy()) {
            let context = z3::Context::new(&z3::Config::new());
            let mut environment = Z3Env::new(&context);
            let expected = eval_term(&term, &SmtModel::default());
            let Z3Num::Int(ast) = environment.term(&term) else {
                prop_assert!(false, "integer syntax must translate to an integer Z3 AST");
                return Ok(());
            };
            prop_assert_eq!(ast.simplify().as_i64(), Some(expected));
        }

        #[test]
        fn z3_constraint_translation_refines_ground_evaluation(
            constraint in ground_constraint_strategy(),
        ) {
            let context = z3::Context::new(&z3::Config::new());
            let mut environment = Z3Env::new(&context);
            let expected = eval_constraint(&constraint, &SmtModel::default());
            prop_assert_eq!(
                environment.constraint(&constraint).simplify().as_bool(),
                Some(expected),
            );
        }
    }

    #[test]
    fn z3_is_available() {
        // System libz3 is present in this environment.
        assert!(z3_available());
    }

    #[test]
    fn satisfiable_linear_arithmetic_yields_witness() {
        let th = Z3Theory::new().expect("z3 available");
        // x > 3 ∧ x < 7
        let s = th.empty_store();
        let s = th
            .propagate(&s, &SmtConstraint::Gt(ivar("x"), ilit(3)))
            .expect("consistent");
        let s = th
            .propagate(&s, &SmtConstraint::Lt(ivar("x"), ilit(7)))
            .expect("consistent");
        assert_eq!(s.status, Sat3::Sat);
        assert!(th.is_consistent(&s));
        let m = th.witness(&s).expect("witness on Sat");
        let x = m.ints.get("x").copied().unwrap_or_default();
        assert!((4..=6).contains(&x), "x = {x} not in (3,7)");
        // The witness re-satisfies the guard under the pure evaluator.
        assert!(th.evaluate(&SmtConstraint::Gt(ivar("x"), ilit(3)), &m));
        assert!(th.evaluate(&SmtConstraint::Lt(ivar("x"), ilit(7)), &m));
    }

    #[test]
    fn contradiction_is_inconsistent_no_witness() {
        let th = Z3Theory::new().expect("z3 available");
        // x ≥ 5 ∧ x ≤ 2  →  Unsat
        let s = th.empty_store();
        let s = th
            .propagate(&s, &SmtConstraint::Ge(ivar("x"), ilit(5)))
            .expect("consistent so far");
        let r = th.propagate(&s, &SmtConstraint::Le(ivar("x"), ilit(2)));
        assert!(r.is_none(), "contradiction must propagate to None");
    }

    #[test]
    fn bitvector_overflow_wraps() {
        let th = Z3Theory::new().expect("z3 available");
        // (bv8 a) + 1 = 0  is satisfiable at a = 255 (wraparound).
        let a = SmtTerm::BvVar("a".to_string(), 8);
        let sum = SmtTerm::Add(Box::new(a), Box::new(SmtTerm::BvLit(1, 8)));
        let s = th.empty_store();
        let s = th
            .propagate(&s, &SmtConstraint::Eq(sum, SmtTerm::BvLit(0, 8)))
            .expect("wraparound is sat");
        assert_eq!(s.status, Sat3::Sat);
        let m = th.witness(&s).expect("witness");
        assert_eq!(m.bvs.get("a").copied(), Some(255));
    }

    #[test]
    fn theory_algebra_is_boolean_algebra() {
        use super::super::logict::{TheoryAlgebra, TheoryPred};
        use super::super::BooleanAlgebra;
        // The whole point of §4-B: TheoryAlgebra<Z3Theory> is a BooleanAlgebra, so the
        // SFA machinery decides SMT guards. Smoke-check is_satisfiable on a guard.
        let alg = TheoryAlgebra::new(Z3Theory::default(), 16);
        let atom = |c| TheoryPred::Atom(c);
        let p = alg.and(
            &atom(SmtConstraint::Gt(ivar("y"), ilit(0))),
            &atom(SmtConstraint::Lt(ivar("y"), ilit(10))),
        );
        assert!(alg.is_satisfiable(&p));
        let bad = alg.and(
            &atom(SmtConstraint::Gt(ivar("y"), ilit(10))),
            &atom(SmtConstraint::Lt(ivar("y"), ilit(0))),
        );
        assert!(!alg.is_satisfiable(&bad));
    }

    #[test]
    fn deep_z3_translation_uses_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-z3-translation".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let mut term = SmtTerm::IntLit(1);
                let mut constraint = SmtConstraint::True;
                for _ in 0..DEEP_Z3_AST_DEPTH {
                    term = SmtTerm::Scale(1, Box::new(term));
                    constraint = SmtConstraint::Not(Box::new(constraint));
                }

                let context = z3::Context::new(&z3::Config::new());
                let mut environment = Z3Env::new(&context);
                let translated_term = environment.term(&term);
                let translated_constraint = environment.constraint(&constraint);
                match translated_term {
                    Z3Num::Int(ast) => assert!(!ast.get_z3_ast().is_null()),
                    Z3Num::Bv(_) => panic!("integer syntax produced a bitvector AST"),
                }
                assert!(!translated_constraint.get_z3_ast().is_null());
            })
            .expect("the bounded-stack Z3 worker must spawn")
            .join()
            .expect("Z3 translation and lifecycle must not overflow the native stack");
    }
}
