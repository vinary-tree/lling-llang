//! SMT-backed [`ConstraintTheory`] backend (Z3 library, in-process) — Task #22 §4-B.
//!
//! Implementing [`ConstraintTheory`] for [`Z3Theory`] makes
//! [`TheoryAlgebra<Z3Theory>`](crate::symbolic::logict::TheoryAlgebra) a
//! [`BooleanAlgebra`](crate::symbolic::BooleanAlgebra) *for free* (see
//! [`crate::symbolic::logict`]): every symbolic-automaton algorithm (emptiness,
//! intersection, complement, determinization, language inclusion) then works over
//! **SMT-theory guards** — booleans, linear integer arithmetic, and fixed-width
//! bitvectors — without changing a single automaton algorithm.
//!
//! # Soundness: the `Sat3` channel for SMT `Unknown`
//!
//! [`ConstraintTheory::propagate`] is *two-valued* — `Some(store)` (consistent) or
//! `None` (inconsistent) — but an SMT solver may return **`Unknown`** (timeout,
//! undecidable fragments, non-linear arithmetic). Collapsing `Unknown` to either side is
//! unsound: as "consistent" it lets an unsatisfiable guard through; as "inconsistent"
//! it rejects a satisfiable one. So the [`SmtStore`] carries a [`Sat3`]:
//!
//! - `propagate` returns `None` **only** on a proven `Unsat`; both `Sat` and `Unknown`
//!   yield `Some(store)`, recording `Sat3::Sat` / `Sat3::DontKnow`.
//! - [`ConstraintTheory::witness`] returns a model **only** on `Sat3::Sat` — never on
//!   `DontKnow`, so an undecided guard never fabricates a witness.
//!
//! Thus `Unknown` is treated as *possibly satisfiable* — the conservative
//! over-approximation that keeps emptiness / language-inclusion checks sound — and
//! [`Sat3::into_safe_bool`] forces callers to handle the undecided case rather than
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
//! [`SmtStore`] `Clone + Send + Sync` and lifetime-free.

use std::collections::HashMap;
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
/// solve time by [`Z3Env`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

/// A guard constraint over [`SmtTerm`]s: booleans + (in)equalities. Boolean
/// connectives compose constraints; comparisons relate two terms **of the same sort**
/// (both integer or both bitvector of equal width).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SmtConstraint {
    /// Constant truth.
    True,
    /// Constant falsity.
    False,
    /// Boolean variable (by name).
    BoolVar(String),
    /// `a = b`.
    Eq(SmtTerm, SmtTerm),
    /// `a ≤ b` (signed for integers, unsigned for bitvectors).
    Le(SmtTerm, SmtTerm),
    /// `a < b`.
    Lt(SmtTerm, SmtTerm),
    /// `a ≥ b`.
    Ge(SmtTerm, SmtTerm),
    /// `a > b`.
    Gt(SmtTerm, SmtTerm),
    /// `¬a`.
    Not(Box<SmtConstraint>),
    /// `a ∧ b`.
    And(Box<SmtConstraint>, Box<SmtConstraint>),
    /// `a ∨ b`.
    Or(Box<SmtConstraint>, Box<SmtConstraint>),
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
    match t {
        SmtTerm::IntLit(n) => *n,
        SmtTerm::IntVar(name) => m.ints.get(name).copied().unwrap_or(0),
        SmtTerm::BvLit(v, _) => *v as i64,
        SmtTerm::BvVar(name, _) => m.bvs.get(name).copied().unwrap_or(0) as i64,
        SmtTerm::Add(a, b) => eval_term(a, m).wrapping_add(eval_term(b, m)),
        SmtTerm::Sub(a, b) => eval_term(a, m).wrapping_sub(eval_term(b, m)),
        SmtTerm::Scale(k, a) => k.wrapping_mul(eval_term(a, m)),
    }
}

/// Evaluate an [`SmtConstraint`] under an assignment.
fn eval_constraint(c: &SmtConstraint, m: &SmtModel) -> bool {
    match c {
        SmtConstraint::True => true,
        SmtConstraint::False => false,
        SmtConstraint::BoolVar(name) => m.bools.get(name).copied().unwrap_or(false),
        SmtConstraint::Eq(a, b) => eval_term(a, m) == eval_term(b, m),
        SmtConstraint::Le(a, b) => eval_term(a, m) <= eval_term(b, m),
        SmtConstraint::Lt(a, b) => eval_term(a, m) < eval_term(b, m),
        SmtConstraint::Ge(a, b) => eval_term(a, m) >= eval_term(b, m),
        SmtConstraint::Gt(a, b) => eval_term(a, m) > eval_term(b, m),
        SmtConstraint::Not(a) => !eval_constraint(a, m),
        SmtConstraint::And(a, b) => eval_constraint(a, m) && eval_constraint(b, m),
        SmtConstraint::Or(a, b) => eval_constraint(a, m) || eval_constraint(b, m),
    }
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
        match t {
            SmtTerm::IntLit(n) => Z3Num::Int(z3::ast::Int::from_i64(self.ctx, *n)),
            SmtTerm::IntVar(name) => Z3Num::Int(self.int_var(name)),
            SmtTerm::BvLit(v, w) => Z3Num::Bv(z3::ast::BV::from_u64(self.ctx, *v, *w)),
            SmtTerm::BvVar(name, w) => Z3Num::Bv(self.bv_var(name, *w)),
            SmtTerm::Add(a, b) => self.num_binop(a, b, |x, y| x + y, |x, y| x.bvadd(y)),
            SmtTerm::Sub(a, b) => self.num_binop(a, b, |x, y| x - y, |x, y| x.bvsub(y)),
            SmtTerm::Scale(k, a) => match self.term(a) {
                Z3Num::Int(x) => Z3Num::Int(z3::ast::Int::from_i64(self.ctx, *k) * x),
                Z3Num::Bv(x) => {
                    let w = x.get_size();
                    Z3Num::Bv(z3::ast::BV::from_u64(self.ctx, *k as u64, w).bvmul(&x))
                }
            },
        }
    }

    fn num_binop(
        &mut self,
        a: &SmtTerm,
        b: &SmtTerm,
        int_op: impl Fn(z3::ast::Int<'ctx>, z3::ast::Int<'ctx>) -> z3::ast::Int<'ctx>,
        bv_op: impl Fn(&z3::ast::BV<'ctx>, &z3::ast::BV<'ctx>) -> z3::ast::BV<'ctx>,
    ) -> Z3Num<'ctx> {
        match (self.term(a), self.term(b)) {
            (Z3Num::Int(x), Z3Num::Int(y)) => Z3Num::Int(int_op(x, y)),
            (Z3Num::Bv(x), Z3Num::Bv(y)) => Z3Num::Bv(bv_op(&x, &y)),
            // Mixed sorts are ill-typed guards; default to the integer reading so the
            // solver sees a well-formed (if unintended) constraint rather than panicking.
            (Z3Num::Int(x), _) => Z3Num::Int(x),
            (Z3Num::Bv(x), _) => Z3Num::Bv(x),
        }
    }

    fn constraint(&mut self, c: &SmtConstraint) -> z3::ast::Bool<'ctx> {
        match c {
            SmtConstraint::True => z3::ast::Bool::from_bool(self.ctx, true),
            SmtConstraint::False => z3::ast::Bool::from_bool(self.ctx, false),
            SmtConstraint::BoolVar(name) => self.bool_var(name),
            SmtConstraint::Eq(a, b) => self.compare(a, b, Cmp::Eq),
            SmtConstraint::Le(a, b) => self.compare(a, b, Cmp::Le),
            SmtConstraint::Lt(a, b) => self.compare(a, b, Cmp::Lt),
            SmtConstraint::Ge(a, b) => self.compare(a, b, Cmp::Ge),
            SmtConstraint::Gt(a, b) => self.compare(a, b, Cmp::Gt),
            SmtConstraint::Not(a) => self.constraint(a).not(),
            SmtConstraint::And(a, b) => {
                let x = self.constraint(a);
                let y = self.constraint(b);
                z3::ast::Bool::and(self.ctx, &[&x, &y])
            }
            SmtConstraint::Or(a, b) => {
                let x = self.constraint(a);
                let y = self.constraint(b);
                z3::ast::Bool::or(self.ctx, &[&x, &y])
            }
        }
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

    fn ivar(s: &str) -> SmtTerm {
        SmtTerm::IntVar(s.to_string())
    }
    fn ilit(n: i64) -> SmtTerm {
        SmtTerm::IntLit(n)
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
}
