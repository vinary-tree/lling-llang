//! Presburger Arithmetic: Automata-Based Decision Procedure
//!
//! ## Theory
//!
//! Presburger arithmetic is the first-order theory of the natural numbers with
//! addition (no multiplication). Despite being decidable, its decision problem
//! has non-elementary lower bounds for general formulas. Büchi (1960) showed that
//! Presburger-definable sets are exactly the sets recognizable by finite automata
//! over binary-encoded integers, providing an elegant automata-theoretic decision
//! procedure.
//!
//! ## Approach: Remainder-Based NFA Construction (Bartzis-Bultan)
//!
//! For an atomic constraint `a₁x₁ + a₂x₂ + ... + aₖxₖ ≤ b` over k variables:
//!
//! 1. **Alphabet**: `{0,1}^k` — one bit per variable per position, read LSB-first.
//!    Each symbol is a `u32` bitmask with bit `i` representing variable `xᵢ`.
//!
//! 2. **States**: remainder values tracking `r = floor((b - S_j) / 2^j)` where
//!    `S_j` is the partial sum from the first `j` bit positions.
//!    - Initial remainder: `r₀ = b`
//!    - At position `j`, reading bits `(d₁, ..., dₖ)`:
//!      - ordinary low bit: `r' = floor((r - Σᵢ aᵢ · dᵢ) / 2)`
//!      - final two's-complement sign bit: `r' = r + Σᵢ aᵢ · dᵢ`
//!    - After `w` bits: accept iff `r ≥ 0`
//!
//! 3. **Remainder bound**: reachable remainders are bounded by the coefficients,
//!    so the state space is finite and typically small.
//!
//! 4. **Boolean operations**: intersection (product), union (product + accept union),
//!    complement (depth-tracked determinization + terminal-state flip), existential
//!    projection (drop one bit dimension, merge transitions).
//!
//! 5. **Negation**: handled algebraically via NNF conversion. `NOT(Σ aᵢxᵢ ≤ b)`
//!    becomes `Σ (-aᵢ)xᵢ ≤ -(b+1)`. De Morgan's laws push negation inward.
//!    Complement construction is only needed for `NOT(EXISTS ...)` (FORALL).
//!
//! ## References
//!
//! - Büchi, J. R. (1960). "Weak second-order arithmetic and finite automata."
//!   Zeitschrift für mathematische Logik und Grundlagen der Mathematik, 6, 66–92.
//! - Wolper, P. & Boigelot, B. (1995). "An automata-theoretic approach to
//!   Presburger arithmetic constraints." SAS 1995. LNCS 983, 21–32.
//! - Bartzis, C. & Bultan, T. (2003). "Efficient symbolic representations for
//!   arithmetic constraints in verification." International Journal of Foundations
//!   of Computer Science, 14(4), 605–624.
//!
//! ## Feature Gates
//!
//! - This module: `#[cfg(feature = "presburger")]`
//! - `PresburgerAlgebra` (BooleanAlgebra impl): additionally requires `symbolic-automata`
//! - `PresburgerTheory` (ConstraintTheory impl): `logict` is a dependency of `presburger`

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};

use super::logict::{ConstraintTheory, LogicStream};
use super::BooleanAlgebra;

// ══════════════════════════════════════════════════════════════════════════════
// Types — Sprint 4
// ══════════════════════════════════════════════════════════════════════════════

/// Default bit width for bounded integer representations.
///
/// Integers are represented in `BIT_WIDTH`-bit two's complement, giving a range
/// of `[-2^(BIT_WIDTH-1), 2^(BIT_WIDTH-1) - 1]`. 16 bits covers `[-32768, 32767]`,
/// sufficient for most grammar-level analysis while keeping NFA construction fast.
pub const DEFAULT_BIT_WIDTH: usize = 16;

/// An atomic linear inequality: `Σ aᵢ·xᵢ ≤ b`.
///
/// Each term `(var_index, coefficient)` pairs a variable index with its integer
/// coefficient. Variable indices are 0-based and must be consistent across all
/// constraints in a formula.
///
/// # Examples
///
/// - `x₀ + x₁ ≤ 5` → `LinearConstraint { terms: vec![(0, 1), (1, 1)], rhs: 5 }`
/// - `2x₀ - 3x₁ ≤ -1` → `LinearConstraint { terms: vec![(0, 2), (1, -3)], rhs: -1 }`
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LinearConstraint {
    /// Coefficient terms: `(variable_index, coefficient)`.
    pub terms: Vec<(usize, i64)>,
    /// Right-hand side constant.
    pub rhs: i64,
}

impl LinearConstraint {
    /// Create a new linear constraint `Σ aᵢ·xᵢ ≤ b`.
    pub fn new(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        LinearConstraint { terms, rhs }
    }

    /// Number of distinct variables referenced by this constraint.
    pub fn num_vars(&self) -> usize {
        self.terms.iter().map(|&(v, _)| v + 1).max().unwrap_or(0)
    }

    /// Evaluate this constraint on a concrete assignment.
    ///
    /// Returns `true` iff `Σ aᵢ·assignment[xᵢ] ≤ b`.
    pub fn evaluate(&self, assignment: &IntAssignment) -> bool {
        let sum: i64 = self
            .terms
            .iter()
            .map(|&(var, coeff)| coeff * assignment.0.get(var).copied().unwrap_or(0))
            .sum();
        sum <= self.rhs
    }

    // ── Normal form conversions ─────────────────────────────────────────

    /// Convert `Σ aᵢ·xᵢ > b` to normal form: `-(Σ aᵢ·xᵢ) ≤ -(b+1)`,
    /// i.e., `Σ (-aᵢ)·xᵢ ≤ -(b+1)`.
    ///
    /// `a > b` ↔ `-(a) ≤ -(b+1)` ↔ `-(a) ≤ -b - 1`
    pub fn from_gt(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        let negated_terms: Vec<(usize, i64)> = terms.into_iter().map(|(v, c)| (v, -c)).collect();
        LinearConstraint {
            terms: negated_terms,
            rhs: -rhs - 1,
        }
    }

    /// Convert `Σ aᵢ·xᵢ ≥ b` to normal form: `Σ (-aᵢ)·xᵢ ≤ -b`.
    ///
    /// `a ≥ b` ↔ `-(a) ≤ -(b)` ↔ `Σ (-aᵢ)·xᵢ ≤ -b`
    pub fn from_gte(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        let negated_terms: Vec<(usize, i64)> = terms.into_iter().map(|(v, c)| (v, -c)).collect();
        LinearConstraint {
            terms: negated_terms,
            rhs: -rhs,
        }
    }

    /// Convert `Σ aᵢ·xᵢ < b` to normal form: `Σ aᵢ·xᵢ ≤ b - 1`.
    ///
    /// `a < b` ↔ `a ≤ b - 1` (for integers)
    pub fn from_lt(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        LinearConstraint {
            terms,
            rhs: rhs - 1,
        }
    }

    /// Convert `Σ aᵢ·xᵢ = b` to a conjunction of two ≤ constraints.
    ///
    /// `a = b` ↔ `a ≤ b ∧ a ≥ b` ↔ `a ≤ b ∧ -a ≤ -b`
    pub fn from_eq(terms: Vec<(usize, i64)>, rhs: i64) -> (Self, Self) {
        let leq = LinearConstraint {
            terms: terms.clone(),
            rhs,
        };
        let geq = LinearConstraint::from_gte(terms, rhs);
        (leq, geq)
    }

    /// Convert `Σ aᵢ·xᵢ ≠ b` to a disjunction of two ≤ constraints.
    ///
    /// `a ≠ b` ↔ `a < b ∨ a > b` ↔ `a ≤ b-1 ∨ -a ≤ -b-1`
    pub fn from_neq(terms: Vec<(usize, i64)>, rhs: i64) -> (Self, Self) {
        let lt = LinearConstraint::from_lt(terms.clone(), rhs);
        let gt = LinearConstraint::from_gt(terms, rhs);
        (lt, gt)
    }
}

impl fmt::Display for LinearConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.terms.is_empty() {
            return write!(f, "0 <= {}", self.rhs);
        }
        for (i, &(var, coeff)) in self.terms.iter().enumerate() {
            if i > 0 {
                if coeff >= 0 {
                    write!(f, " + ")?;
                } else {
                    write!(f, " - ")?;
                }
                if coeff.abs() != 1 {
                    write!(f, "{}*", coeff.abs())?;
                }
            } else if coeff == -1 {
                write!(f, "-")?;
            } else if coeff != 1 {
                write!(f, "{}*", coeff)?;
            }
            write!(f, "x{}", var)?;
        }
        write!(f, " <= {}", self.rhs)
    }
}

/// A Presburger arithmetic predicate (Boolean combination of linear constraints).
///
/// Supports the full first-order fragment over linear integer arithmetic with
/// bounded quantification (existential projection via NFA variable elimination).
pub enum PresburgerPred {
    /// Always true.
    True,
    /// Always false.
    False,
    /// Atomic constraint: `Σ aᵢ·xᵢ ≤ b`.
    Atom(LinearConstraint),
    /// Conjunction: `φ ∧ ψ`.
    And(Box<PresburgerPred>, Box<PresburgerPred>),
    /// Disjunction: `φ ∨ ψ`.
    Or(Box<PresburgerPred>, Box<PresburgerPred>),
    /// Negation: `¬φ`.
    Not(Box<PresburgerPred>),
    /// Existential quantification: `∃ xᵥ. φ`.
    ///
    /// Implemented via NFA projection (drop the bit dimension for variable `var`).
    Exists {
        var: usize,
        body: Box<PresburgerPred>,
    },
}

impl PresburgerPred {
    /// Convenience: `a ≤ b` atom.
    pub fn leq(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        PresburgerPred::Atom(LinearConstraint::new(terms, rhs))
    }

    /// Convenience: `a ≥ b` atom (normalized to ≤ form).
    pub fn geq(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        PresburgerPred::Atom(LinearConstraint::from_gte(terms, rhs))
    }

    /// Convenience: `a < b` atom (normalized to ≤ form).
    pub fn lt(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        PresburgerPred::Atom(LinearConstraint::from_lt(terms, rhs))
    }

    /// Convenience: `a > b` atom (normalized to ≤ form).
    pub fn gt(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        PresburgerPred::Atom(LinearConstraint::from_gt(terms, rhs))
    }

    /// Convenience: `a = b` (conjunction of ≤ and ≥).
    pub fn eq(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        let (leq, geq) = LinearConstraint::from_eq(terms, rhs);
        PresburgerPred::And(
            Box::new(PresburgerPred::Atom(leq)),
            Box::new(PresburgerPred::Atom(geq)),
        )
    }

    /// Convenience: `a ≠ b` (disjunction of < and >).
    pub fn neq(terms: Vec<(usize, i64)>, rhs: i64) -> Self {
        let (lt, gt) = LinearConstraint::from_neq(terms, rhs);
        PresburgerPred::Or(
            Box::new(PresburgerPred::Atom(lt)),
            Box::new(PresburgerPred::Atom(gt)),
        )
    }

    /// Number of distinct variables referenced in the entire formula.
    pub fn num_vars(&self) -> usize {
        let mut maximum = 0usize;
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            match predicate {
                PresburgerPred::True | PresburgerPred::False => {}
                PresburgerPred::Atom(constraint) => maximum = maximum.max(constraint.num_vars()),
                PresburgerPred::And(left, right) | PresburgerPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                PresburgerPred::Not(inner) => pending.push(inner),
                PresburgerPred::Exists { var, body } => {
                    maximum = maximum.max(*var + 1);
                    pending.push(body);
                }
            }
        }
        maximum
    }
}

impl Clone for PresburgerPred {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a PresburgerPred),
            And,
            Or,
            Not,
            Exists(usize),
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    PresburgerPred::True => values.push(PresburgerPred::True),
                    PresburgerPred::False => values.push(PresburgerPred::False),
                    PresburgerPred::Atom(constraint) => {
                        values.push(PresburgerPred::Atom(constraint.clone()));
                    }
                    PresburgerPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    PresburgerPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    PresburgerPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                    PresburgerPred::Exists { var, body } => {
                        tasks.push(Task::Exists(*var));
                        tasks.push(Task::Clone(body));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right predicate clone is present");
                    let left = values.pop().expect("left predicate clone is present");
                    values.push(if matches!(task, Task::And) {
                        PresburgerPred::And(Box::new(left), Box::new(right))
                    } else {
                        PresburgerPred::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated predicate clone is present");
                    values.push(PresburgerPred::Not(Box::new(inner)));
                }
                Task::Exists(var) => {
                    let body = values.pop().expect("quantifier body clone is present");
                    values.push(PresburgerPred::Exists {
                        var,
                        body: Box::new(body),
                    });
                }
            }
        }
        values.pop().expect("the root predicate produces one clone")
    }
}

impl PartialEq for PresburgerPred {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (PresburgerPred::True, PresburgerPred::True)
                | (PresburgerPred::False, PresburgerPred::False) => {}
                (PresburgerPred::Atom(left), PresburgerPred::Atom(right)) if left == right => {}
                (PresburgerPred::And(la, lb), PresburgerPred::And(ra, rb))
                | (PresburgerPred::Or(la, lb), PresburgerPred::Or(ra, rb)) => {
                    pending.push((lb, rb));
                    pending.push((la, ra));
                }
                (PresburgerPred::Not(left), PresburgerPred::Not(right)) => {
                    pending.push((left, right));
                }
                (
                    PresburgerPred::Exists { var: lv, body: lb },
                    PresburgerPred::Exists { var: rv, body: rb },
                ) if lv == rv => pending.push((lb, rb)),
                _ => return false,
            }
        }
        true
    }
}

impl Eq for PresburgerPred {}

impl Hash for PresburgerPred {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                PresburgerPred::True | PresburgerPred::False => {}
                PresburgerPred::Atom(constraint) => constraint.hash(state),
                PresburgerPred::And(left, right) | PresburgerPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                PresburgerPred::Not(inner) => pending.push(inner),
                PresburgerPred::Exists { var, body } => {
                    var.hash(state);
                    pending.push(body);
                }
            }
        }
    }
}

impl fmt::Debug for PresburgerPred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Predicate(&'a PresburgerPred),
            Text(&'static str),
        }
        let mut events = vec![Event::Predicate(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Predicate(predicate) => match predicate {
                    PresburgerPred::True => write!(f, "True")?,
                    PresburgerPred::False => write!(f, "False")?,
                    PresburgerPred::Atom(constraint) => write!(f, "Atom({constraint:?})")?,
                    PresburgerPred::And(left, right) | PresburgerPred::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, PresburgerPred::And(_, _)) {
                                "And"
                            } else {
                                "Or"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Predicate(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Predicate(left));
                    }
                    PresburgerPred::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Predicate(inner));
                    }
                    PresburgerPred::Exists { var, body } => {
                        write!(f, "Exists {{ var: {var:?}, body: ")?;
                        events.push(Event::Text(" }"));
                        events.push(Event::Predicate(body));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for PresburgerPred {
    fn drop(&mut self) {
        fn drain_children(predicate: &mut PresburgerPred, pending: &mut Vec<PresburgerPred>) {
            match predicate {
                PresburgerPred::And(left, right) | PresburgerPred::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, PresburgerPred::False));
                    pending.push(std::mem::replace(&mut **left, PresburgerPred::False));
                }
                PresburgerPred::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, PresburgerPred::False));
                }
                PresburgerPred::Exists { body, .. } => {
                    pending.push(std::mem::replace(&mut **body, PresburgerPred::False));
                }
                PresburgerPred::True | PresburgerPred::False | PresburgerPred::Atom(_) => {}
            }
        }

        let mut pending = Vec::new();
        drain_children(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain_children(&mut predicate, &mut pending);
        }
    }
}

impl fmt::Display for PresburgerPred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Predicate(&'a PresburgerPred),
            Text(&'static str),
        }
        let mut events = vec![Event::Predicate(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Predicate(predicate) => match predicate {
                    PresburgerPred::True => write!(f, "true")?,
                    PresburgerPred::False => write!(f, "false")?,
                    PresburgerPred::Atom(constraint) => write!(f, "{constraint}")?,
                    PresburgerPred::And(left, right) | PresburgerPred::Or(left, right) => {
                        write!(f, "(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Predicate(right));
                        events.push(Event::Text(
                            if matches!(predicate, PresburgerPred::And(_, _)) {
                                " /\\ "
                            } else {
                                " \\/ "
                            },
                        ));
                        events.push(Event::Predicate(left));
                    }
                    PresburgerPred::Not(inner) => {
                        write!(f, "~(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Predicate(inner));
                    }
                    PresburgerPred::Exists { var, body } => {
                        write!(f, "(exists x{var}. ")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Predicate(body));
                    }
                },
            }
        }
        Ok(())
    }
}

/// A concrete integer assignment for Presburger arithmetic.
///
/// Maps variable indices (0-based) to integer values. Variables beyond the
/// length of the vector are treated as 0.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IntAssignment(pub Vec<i64>);

impl IntAssignment {
    /// Create an assignment from a slice of values.
    pub fn new(values: Vec<i64>) -> Self {
        IntAssignment(values)
    }

    /// Get the value of variable `var`, defaulting to 0 if out of bounds.
    pub fn get(&self, var: usize) -> i64 {
        self.0.get(var).copied().unwrap_or(0)
    }
}

impl fmt::Display for IntAssignment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for (i, v) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "x{}={}", i, v)?;
        }
        write!(f, "]")
    }
}

/// Evaluate a Presburger predicate on a concrete assignment.
///
/// Recursively evaluates the Boolean formula. Quantified variables
/// are evaluated by bounded search over the bit-width range.
pub fn evaluate_presburger(
    pred: &PresburgerPred,
    assignment: &IntAssignment,
    bit_width: usize,
) -> bool {
    enum Frame<'a> {
        Eval(&'a PresburgerPred),
        Not,
        AndRight(&'a PresburgerPred),
        OrRight(&'a PresburgerPred),
        ExistsResume {
            var: usize,
            body: &'a PresburgerPred,
            next: i64,
            hi: i64,
            old_len: usize,
            old_value: Option<i64>,
        },
    }

    fn restore_binding(values: &mut Vec<i64>, var: usize, old_len: usize, old: Option<i64>) {
        values.truncate(old_len);
        if let Some(value) = old {
            values[var] = value;
        }
    }

    let mut values = assignment.0.clone();
    let mut frames = vec![Frame::Eval(pred)];
    let mut result = None;

    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Eval(current) => match current {
                PresburgerPred::True => result = Some(true),
                PresburgerPred::False => result = Some(false),
                PresburgerPred::Atom(constraint) => {
                    let sum: i64 = constraint
                        .terms
                        .iter()
                        .map(|&(var, coefficient)| {
                            coefficient * values.get(var).copied().unwrap_or(0)
                        })
                        .sum();
                    result = Some(sum <= constraint.rhs);
                }
                PresburgerPred::And(left, right) => {
                    frames.push(Frame::AndRight(right));
                    frames.push(Frame::Eval(left));
                }
                PresburgerPred::Or(left, right) => {
                    frames.push(Frame::OrRight(right));
                    frames.push(Frame::Eval(left));
                }
                PresburgerPred::Not(inner) => {
                    frames.push(Frame::Not);
                    frames.push(Frame::Eval(inner));
                }
                PresburgerPred::Exists { var, body } => {
                    // Bounded search: try values in ascending order, exactly as the
                    // recursive `Iterator::any` formulation did.  A single mutable
                    // environment plus an explicit restoration frame avoids cloning
                    // it for every Boolean node and quantified candidate.
                    let half = 1i64 << (bit_width - 1);
                    let lo = -half;
                    let hi = half;
                    let old_len = values.len();
                    let old_value = values.get(*var).copied();
                    values.resize(values.len().max(*var + 1), 0);
                    values[*var] = lo;
                    frames.push(Frame::ExistsResume {
                        var: *var,
                        body,
                        next: lo + 1,
                        hi,
                        old_len,
                        old_value,
                    });
                    frames.push(Frame::Eval(body));
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
            Frame::ExistsResume {
                var,
                body,
                next,
                hi,
                old_len,
                old_value,
            } => {
                if result.take().expect("quantifier body was evaluated") {
                    restore_binding(&mut values, var, old_len, old_value);
                    result = Some(true);
                } else if next < hi {
                    values[var] = next;
                    frames.push(Frame::ExistsResume {
                        var,
                        body,
                        next: next + 1,
                        hi,
                        old_len,
                        old_value,
                    });
                    frames.push(Frame::Eval(body));
                } else {
                    restore_binding(&mut values, var, old_len, old_value);
                    result = Some(false);
                }
            }
        }
    }

    result.expect("the root predicate always produces one Boolean result")
}

// ══════════════════════════════════════════════════════════════════════════════
// Presburger NFA — Sprint 5 (Büchi's automata-theoretic construction)
// ══════════════════════════════════════════════════════════════════════════════

/// An NFA over the alphabet `{0,1}^k` for Presburger arithmetic decision.
///
/// States are indexed by `usize`. The NFA reads one bit per variable per step,
/// processing integers LSB-first. After `bit_width` steps, the NFA accepts iff
/// the encoded integer tuple satisfies the original Presburger formula.
///
/// Push negation inward to obtain Negation Normal Form (NNF).
///
/// In NNF, `Not` only appears directly on atoms. For Presburger arithmetic,
/// negating an atom `Σ aᵢxᵢ ≤ b` yields `Σ aᵢxᵢ > b`, which normalizes to
/// `Σ (-aᵢ)xᵢ ≤ -(b+1)`.
///
/// De Morgan's laws are applied recursively:
/// - `NOT(A AND B)` → `NOT(A) OR NOT(B)`
/// - `NOT(A OR B)` → `NOT(A) AND NOT(B)`
/// - `NOT(NOT(A))` → `A`
/// - `NOT(TRUE)` → `FALSE`
/// - `NOT(FALSE)` → `TRUE`
/// - `NOT(EXISTS x. A)` → `FORALL x. NOT(A)` — but we don't have FORALL,
///   so we handle this by noting: `NOT(EXISTS x. A) ≡ FORALL x. NOT(A)`.
///   For bounded integers, `FORALL x ∈ D. P(x) ≡ NOT(EXISTS x ∈ D. NOT(P(x)))`.
///   So `NOT(EXISTS x. A)` becomes `NOT(EXISTS x. A)` — we keep it as-is
///   and handle it in the NFA compilation by complementing the projection.
///   Actually: for the NFA approach, `NOT(EXISTS x. A)` is handled by
///   building NFA(EXISTS x. A) then complementing. We use a specialized
///   complement that works on the already-projected NFA.
fn push_negation_inward(pred: &PresburgerPred) -> PresburgerPred {
    normalize_with_polarity(pred, false)
}

/// Defunctionalized polarity machine shared by positive NNF conversion and
/// negation.  The Boolean flag is the pending negation parity, so the former
/// mutual recursion becomes a single linear pass over heap-resident frames.
fn normalize_with_polarity(pred: &PresburgerPred, negated: bool) -> PresburgerPred {
    enum Task<'a> {
        Visit(&'a PresburgerPred, bool),
        Binary { conjunction: bool },
        Exists { var: usize, residual_not: bool },
    }

    let mut tasks = vec![Task::Visit(pred, negated)];
    let mut values = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(current, polarity) => match current {
                PresburgerPred::True => values.push(if polarity {
                    PresburgerPred::False
                } else {
                    PresburgerPred::True
                }),
                PresburgerPred::False => values.push(if polarity {
                    PresburgerPred::True
                } else {
                    PresburgerPred::False
                }),
                PresburgerPred::Atom(constraint) => {
                    values.push(PresburgerPred::Atom(if polarity {
                        LinearConstraint::from_gt(constraint.terms.clone(), constraint.rhs)
                    } else {
                        constraint.clone()
                    }));
                }
                PresburgerPred::And(left, right) => {
                    tasks.push(Task::Binary {
                        conjunction: !polarity,
                    });
                    tasks.push(Task::Visit(right, polarity));
                    tasks.push(Task::Visit(left, polarity));
                }
                PresburgerPred::Or(left, right) => {
                    tasks.push(Task::Binary {
                        conjunction: polarity,
                    });
                    tasks.push(Task::Visit(right, polarity));
                    tasks.push(Task::Visit(left, polarity));
                }
                PresburgerPred::Not(inner) => tasks.push(Task::Visit(inner, !polarity)),
                PresburgerPred::Exists { var, body } => {
                    // A negative existential remains the documented residual
                    // complement form; its body is normalized positively.
                    tasks.push(Task::Exists {
                        var: *var,
                        residual_not: polarity,
                    });
                    tasks.push(Task::Visit(body, false));
                }
            },
            Task::Binary { conjunction } => {
                let right = values.pop().expect("right normalized operand is present");
                let left = values.pop().expect("left normalized operand is present");
                values.push(if conjunction {
                    PresburgerPred::And(Box::new(left), Box::new(right))
                } else {
                    PresburgerPred::Or(Box::new(left), Box::new(right))
                });
            }
            Task::Exists { var, residual_not } => {
                let body = values.pop().expect("normalized quantifier body is present");
                let quantified = PresburgerPred::Exists {
                    var,
                    body: Box::new(body),
                };
                values.push(if residual_not {
                    PresburgerPred::Not(Box::new(quantified))
                } else {
                    quantified
                });
            }
        }
    }
    values
        .pop()
        .expect("the root normalization task produces one predicate")
}

/// # Representation
///
/// Each alphabet symbol is a `u32` bitmask encoding `k` bits, one per variable.
/// Bit `i` of the symbol is the current bit of variable `xᵢ`.
#[derive(Clone, Debug)]
pub struct PresburgerNfa {
    /// Number of states (states are indexed `0..num_states`).
    pub num_states: usize,
    /// Accepting states (indexed by state id, `true` = accepting).
    pub accepting: Vec<bool>,
    /// Transition function: `(state, alphabet_symbol) -> set of successor states`.
    pub transitions: HashMap<(usize, u32), HashSet<usize>>,
    /// Set of initial states.
    pub initial: HashSet<usize>,
    /// Number of variables (determines alphabet size `2^num_vars`).
    pub num_vars: usize,
    /// Bit width for bounded integers.
    pub bit_width: usize,
}

impl PresburgerNfa {
    /// Compile an atomic `LinearConstraint` into an NFA (Büchi/Bartzis-Bultan construction).
    ///
    /// For `Σ aᵢxᵢ ≤ b` with `k` variables and bit width `w`:
    ///
    /// Uses the remainder-based construction (Bartzis & Bultan 2003):
    /// - States represent the "remainder" `r = floor((b - Σ aᵢ·low_j(xᵢ)) / 2^j)`.
    /// - Initial remainder: `r₀ = b`
    /// - At an ordinary low-bit position `j`, reading bits `(d₁,...,dₖ)`:
    ///   - `r' = floor((r - Σᵢ aᵢ·dᵢ) / 2)`
    /// - At the final two's-complement sign bit:
    ///   - `r' = r + Σᵢ aᵢ·dᵢ`
    /// - After `w` positions, accept iff `r ≥ 0`.
    ///
    /// The construction unfolds the remainder automaton for exactly `w` time steps,
    /// creating at most `w * |reachable_remainders|` states.
    pub fn from_constraint(
        constraint: &LinearConstraint,
        num_vars: usize,
        bit_width: usize,
    ) -> Self {
        let alpha_size: u32 = 1 << num_vars;

        // Build coefficient vector indexed by variable (0..num_vars).
        let mut coeffs = vec![0i64; num_vars];
        for &(var, coeff) in &constraint.terms {
            if var < num_vars {
                coeffs[var] = coeff;
            }
        }
        let b = constraint.rhs;

        // Precompute all possible bit_sum values for the 2^k alphabet symbols.
        let mut bit_sums = Vec::with_capacity(alpha_size as usize);
        for sym in 0..alpha_size {
            let mut s: i64 = 0;
            for v in 0..num_vars {
                if (sym >> v) & 1 == 1 {
                    s += coeffs[v];
                }
            }
            bit_sums.push(s);
        }

        // Map (position, remainder) -> state_id.
        let mut state_map: HashMap<(usize, i64), usize> = HashMap::new();
        let mut num_states = 0usize;
        let mut transitions: HashMap<(usize, u32), HashSet<usize>> = HashMap::new();

        // BFS queue of (position, remainder).
        let mut queue: VecDeque<(usize, i64)> = VecDeque::new();

        // Initial state: position 0, remainder = b.
        let initial_id = num_states;
        state_map.insert((0, b), initial_id);
        num_states += 1;
        queue.push_back((0, b));

        while let Some((pos, remainder)) = queue.pop_front() {
            if pos >= bit_width {
                // No transitions from terminal states.
                continue;
            }

            let src = state_map[&(pos, remainder)];

            // Enumerate all possible input bit vectors.
            for sym in 0..alpha_size {
                let bit_sum = bit_sums[sym as usize];
                let next_remainder = if pos + 1 == bit_width {
                    // The most-significant bit has weight -2^(w-1) in a
                    // signed two's-complement value, so its coefficient sum is
                    // added to the residual and is not divided again.
                    remainder + bit_sum
                } else {
                    div_floor(remainder - bit_sum, 2)
                };
                let next_pos = pos + 1;

                // Get or create the destination state.
                let dst = *state_map
                    .entry((next_pos, next_remainder))
                    .or_insert_with(|| {
                        let id = num_states;
                        num_states += 1;
                        queue.push_back((next_pos, next_remainder));
                        id
                    });

                transitions
                    .entry((src, sym))
                    .or_insert_with(|| HashSet::with_capacity(1))
                    .insert(dst);
            }
        }

        // Accepting states: position == bit_width AND remainder >= 0.
        let mut accepting = vec![false; num_states];
        for (&(pos, remainder), &state_id) in &state_map {
            if pos == bit_width && remainder >= 0 {
                accepting[state_id] = true;
            }
        }

        // Add self-loops at terminal states so that complement construction
        // works correctly. Without self-loops, the complement's dead state
        // (empty NFA set) absorbs inputs beyond bit_width, making intermediate
        // states incorrectly accepting in the complement.
        for (&(pos, _), &state_id) in &state_map {
            if pos == bit_width {
                for sym in 0..alpha_size {
                    transitions
                        .entry((state_id, sym))
                        .or_insert_with(|| HashSet::with_capacity(1))
                        .insert(state_id);
                }
            }
        }

        let mut initial_set = HashSet::with_capacity(1);
        initial_set.insert(initial_id);

        PresburgerNfa {
            num_states,
            accepting,
            transitions,
            initial: initial_set,
            num_vars,
            bit_width,
        }
    }

    /// Compile a full `PresburgerPred` to an NFA.
    ///
    /// Recursively compiles sub-formulas and combines using NFA operations:
    /// - `And` → `intersect_nfa`
    /// - `Or` → `union_nfa`
    /// - `Not` → `complement_nfa`
    /// - `Exists` → `project_nfa`
    pub fn from_pred(pred: &PresburgerPred, bit_width: usize) -> Self {
        let num_vars = pred.num_vars().max(1);
        Self::from_pred_with_vars(pred, num_vars, bit_width)
    }

    fn from_pred_with_vars(pred: &PresburgerPred, num_vars: usize, bit_width: usize) -> Self {
        // First, push negation inward to eliminate Not nodes. This converts
        // every formula to an equivalent negation-normal form (NNF) where Not
        // only appears on atoms. Then atomic negations are handled by
        // converting `NOT(Σ aᵢxᵢ ≤ b)` to `Σ aᵢxᵢ > b` (= `Σ (-aᵢ)xᵢ ≤ -(b+1)`).
        //
        // This avoids the general complement_nfa construction, which is problematic
        // for position-unfolded NFAs (fixed-length language complement requires
        // careful handling of intermediate states).
        let nnf = push_negation_inward(pred);
        Self::compile_nnf(&nnf, num_vars, bit_width)
    }

    /// Compile a negation-normal form predicate to an NFA.
    ///
    /// Precondition: `pred` is in NNF (Not only appears on atoms, and
    /// atomic Not has been resolved to a positive constraint).
    fn compile_nnf(pred: &PresburgerPred, num_vars: usize, bit_width: usize) -> Self {
        enum Task<'a> {
            Compile(&'a PresburgerPred),
            Intersect,
            Union,
            Complement,
            Project(usize),
        }
        let mut tasks = vec![Task::Compile(pred)];
        let mut automata = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Compile(predicate) => match predicate {
                    PresburgerPred::True => automata.push(Self::universal(num_vars, bit_width)),
                    PresburgerPred::False => {
                        automata.push(Self::empty_language(num_vars, bit_width));
                    }
                    PresburgerPred::Atom(constraint) => {
                        automata.push(Self::from_constraint(constraint, num_vars, bit_width));
                    }
                    PresburgerPred::And(left, right) => {
                        tasks.push(Task::Intersect);
                        tasks.push(Task::Compile(right));
                        tasks.push(Task::Compile(left));
                    }
                    PresburgerPred::Or(left, right) => {
                        tasks.push(Task::Union);
                        tasks.push(Task::Compile(right));
                        tasks.push(Task::Compile(left));
                    }
                    PresburgerPred::Not(inner) => {
                        tasks.push(Task::Complement);
                        tasks.push(Task::Compile(inner));
                    }
                    PresburgerPred::Exists { var, body } => {
                        tasks.push(Task::Project(*var));
                        tasks.push(Task::Compile(body));
                    }
                },
                Task::Intersect | Task::Union => {
                    let right = automata.pop().expect("right predicate NFA is present");
                    let left = automata.pop().expect("left predicate NFA is present");
                    automata.push(if matches!(task, Task::Intersect) {
                        intersect_nfa(&left, &right)
                    } else {
                        union_nfa(&left, &right)
                    });
                }
                Task::Complement => {
                    let inner = automata.pop().expect("complemented NFA is present");
                    automata.push(complement_fixed_length(&inner));
                }
                Task::Project(var) => {
                    let body = automata.pop().expect("projected NFA is present");
                    automata.push(project_nfa(&body, var));
                }
            }
        }
        automata
            .pop()
            .expect("the root predicate produces one compiled NFA")
    }

    /// Build a universal NFA (accepts all fixed-length inputs).
    ///
    /// Creates a position-unfolded chain of `bit_width + 1` states where every
    /// state transitions to the next position on all symbols. The terminal state
    /// (position `bit_width`) is accepting and has self-loops. This structure is
    /// compatible with the fixed-length complement construction.
    pub fn universal(num_vars: usize, bit_width: usize) -> Self {
        let alpha_size = 1u32 << num_vars;
        let total_states = bit_width + 1;
        let mut transitions = HashMap::new();

        // Chain: state j -> state j+1 on all symbols (positions 0..bit_width-1).
        for j in 0..bit_width {
            for sym in 0..alpha_size {
                let mut set = HashSet::with_capacity(1);
                set.insert(j + 1);
                transitions.insert((j, sym), set);
            }
        }
        // Terminal state (position bit_width): self-loops on all symbols.
        for sym in 0..alpha_size {
            let mut set = HashSet::with_capacity(1);
            set.insert(bit_width);
            transitions.insert((bit_width, sym), set);
        }

        let mut initial = HashSet::with_capacity(1);
        initial.insert(0);

        // Only the terminal state is accepting.
        let mut accepting = vec![false; total_states];
        accepting[bit_width] = true;

        PresburgerNfa {
            num_states: total_states,
            accepting,
            transitions,
            initial,
            num_vars,
            bit_width,
        }
    }

    /// Build an NFA accepting no fixed-length inputs (empty language).
    ///
    /// Creates the same position-unfolded chain as `universal()`, but the
    /// terminal state is non-accepting. Compatible with complement construction.
    pub fn empty_language(num_vars: usize, bit_width: usize) -> Self {
        let alpha_size = 1u32 << num_vars;
        let total_states = bit_width + 1;
        let mut transitions = HashMap::new();

        // Chain: state j -> state j+1 on all symbols.
        for j in 0..bit_width {
            for sym in 0..alpha_size {
                let mut set = HashSet::with_capacity(1);
                set.insert(j + 1);
                transitions.insert((j, sym), set);
            }
        }
        // Terminal state: self-loops (needed for complement to work).
        for sym in 0..alpha_size {
            let mut set = HashSet::with_capacity(1);
            set.insert(bit_width);
            transitions.insert((bit_width, sym), set);
        }

        let mut initial = HashSet::with_capacity(1);
        initial.insert(0);

        // No state is accepting.
        let accepting = vec![false; total_states];

        PresburgerNfa {
            num_states: total_states,
            accepting,
            transitions,
            initial,
            num_vars,
            bit_width,
        }
    }

    /// Check whether the NFA accepts any input (language is non-empty).
    pub fn is_nonempty(&self) -> bool {
        !self.is_empty_language()
    }

    /// Check whether the NFA accepts no input (language is empty).
    pub fn is_empty_language(&self) -> bool {
        // BFS from initial states to find any accepting state.
        let mut visited = vec![false; self.num_states];
        let mut queue: VecDeque<usize> = VecDeque::new();

        for &init in &self.initial {
            if init < self.num_states {
                if self.accepting[init] {
                    return false;
                }
                visited[init] = true;
                queue.push_back(init);
            }
        }

        let alpha_size = 1u32 << self.num_vars;
        while let Some(state) = queue.pop_front() {
            for sym in 0..alpha_size {
                if let Some(succs) = self.transitions.get(&(state, sym)) {
                    for &s in succs {
                        if s < self.num_states && !visited[s] {
                            if self.accepting[s] {
                                return false;
                            }
                            visited[s] = true;
                            queue.push_back(s);
                        }
                    }
                }
            }
        }

        true
    }

    /// Find a shortest accepting path and decode it to an integer assignment.
    ///
    /// Returns `None` if the language is empty. Uses BFS for shortest path.
    pub fn witness(&self) -> Option<IntAssignment> {
        let alpha_size = 1u32 << self.num_vars;

        // BFS: state -> (parent_state, symbol_used)
        let mut parent: HashMap<usize, (usize, u32)> = HashMap::new();
        let mut visited = vec![false; self.num_states];
        let mut queue: VecDeque<usize> = VecDeque::new();
        // Sentinel: initial states have no parent.
        let sentinel = usize::MAX;

        for &init in &self.initial {
            if init < self.num_states {
                visited[init] = true;
                parent.insert(init, (sentinel, 0));
                if self.accepting[init] {
                    // Accepting initial state: path length 0.
                    return Some(self.decode_path(&parent, init));
                }
                queue.push_back(init);
            }
        }

        while let Some(state) = queue.pop_front() {
            for sym in 0..alpha_size {
                if let Some(succs) = self.transitions.get(&(state, sym)) {
                    for &s in succs {
                        if s < self.num_states && !visited[s] {
                            visited[s] = true;
                            parent.insert(s, (state, sym));
                            if self.accepting[s] {
                                return Some(self.decode_path(&parent, s));
                            }
                            queue.push_back(s);
                        }
                    }
                }
            }
        }

        None
    }

    /// Decode a BFS path into an integer assignment.
    ///
    /// The path encodes bits LSB-first. Each symbol is a `u32` bitmask with
    /// bit `i` being the bit of variable `xᵢ` at that position.
    fn decode_path(&self, parent: &HashMap<usize, (usize, u32)>, end: usize) -> IntAssignment {
        let sentinel = usize::MAX;

        // Collect symbols from end to start (reversed).
        let mut symbols = Vec::new();
        let mut cur = end;
        loop {
            let &(prev, sym) = parent.get(&cur).expect("BFS parent should exist");
            if prev == sentinel {
                break;
            }
            symbols.push(sym);
            cur = prev;
        }
        symbols.reverse();

        // Decode the fixed-width words as signed two's-complement integers.
        // Bits are read LSB-first; the final bit carries negative weight.
        let mut words = vec![0u64; self.num_vars];
        for (j, &sym) in symbols.iter().enumerate() {
            for v in 0..self.num_vars {
                if (sym >> v) & 1 == 1 {
                    if let Some(bit) = 1u64.checked_shl(j as u32) {
                        words[v] |= bit;
                    }
                }
            }
        }

        let width = self.bit_width.min(64);
        let values = words
            .into_iter()
            .map(|mut word| {
                if width > 0 && width < 64 {
                    let sign = 1u64 << (width - 1);
                    let mask = (1u64 << width) - 1;
                    word &= mask;
                    if word & sign != 0 {
                        word |= !mask;
                    }
                }
                word as i64
            })
            .collect();

        IntAssignment(values)
    }
}

// ── NFA Boolean Operations ──────────────────────────────────────────────────

/// Product construction for NFA intersection.
///
/// The resulting NFA accepts the intersection of the languages of `a` and `b`.
/// A state `(s1, s2)` in the product is accepting iff both `s1` and `s2` are
/// accepting in their respective NFAs.
pub fn intersect_nfa(a: &PresburgerNfa, b: &PresburgerNfa) -> PresburgerNfa {
    assert_eq!(
        a.num_vars, b.num_vars,
        "NFAs must have same number of variables"
    );
    let num_vars = a.num_vars;
    let bit_width = a.bit_width.max(b.bit_width);
    let alpha_size = 1u32 << num_vars;

    // State map: (a_state, b_state) -> product_state_id.
    let mut state_map: HashMap<(usize, usize), usize> = HashMap::new();
    let mut num_states = 0usize;
    let mut transitions: HashMap<(usize, u32), HashSet<usize>> = HashMap::new();
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();

    // Initial states: all pairs of initial states.
    let mut initial = HashSet::new();
    for &a_init in &a.initial {
        for &b_init in &b.initial {
            let id = num_states;
            state_map.insert((a_init, b_init), id);
            num_states += 1;
            initial.insert(id);
            queue.push_back((a_init, b_init));
        }
    }

    while let Some((as_, bs)) = queue.pop_front() {
        let src = state_map[&(as_, bs)];
        for sym in 0..alpha_size {
            let a_succs = a.transitions.get(&(as_, sym));
            let b_succs = b.transitions.get(&(bs, sym));
            if let (Some(a_s), Some(b_s)) = (a_succs, b_succs) {
                for &a_next in a_s {
                    for &b_next in b_s {
                        let dst = *state_map.entry((a_next, b_next)).or_insert_with(|| {
                            let id = num_states;
                            num_states += 1;
                            queue.push_back((a_next, b_next));
                            id
                        });
                        transitions
                            .entry((src, sym))
                            .or_insert_with(|| HashSet::with_capacity(1))
                            .insert(dst);
                    }
                }
            }
        }
    }

    let mut accepting = vec![false; num_states];
    for (&(as_, bs), &id) in &state_map {
        let a_acc = as_ < a.accepting.len() && a.accepting[as_];
        let b_acc = bs < b.accepting.len() && b.accepting[bs];
        accepting[id] = a_acc && b_acc;
    }

    PresburgerNfa {
        num_states,
        accepting,
        transitions,
        initial,
        num_vars,
        bit_width,
    }
}

/// Product construction for NFA union.
///
/// The resulting NFA accepts the union of the languages of `a` and `b`.
/// A state in the product is accepting iff either component state is accepting.
pub fn union_nfa(a: &PresburgerNfa, b: &PresburgerNfa) -> PresburgerNfa {
    assert_eq!(
        a.num_vars, b.num_vars,
        "NFAs must have same number of variables"
    );
    let num_vars = a.num_vars;
    let bit_width = a.bit_width.max(b.bit_width);
    let alpha_size = 1u32 << num_vars;

    let mut state_map: HashMap<(usize, usize), usize> = HashMap::new();
    let mut num_states = 0usize;
    let mut transitions: HashMap<(usize, u32), HashSet<usize>> = HashMap::new();
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();

    let mut initial = HashSet::new();
    for &a_init in &a.initial {
        for &b_init in &b.initial {
            let id = num_states;
            state_map.insert((a_init, b_init), id);
            num_states += 1;
            initial.insert(id);
            queue.push_back((a_init, b_init));
        }
    }

    while let Some((as_, bs)) = queue.pop_front() {
        let src = state_map[&(as_, bs)];
        for sym in 0..alpha_size {
            let a_succs = a.transitions.get(&(as_, sym));
            let b_succs = b.transitions.get(&(bs, sym));
            if let (Some(a_s), Some(b_s)) = (a_succs, b_succs) {
                for &a_next in a_s {
                    for &b_next in b_s {
                        let dst = *state_map.entry((a_next, b_next)).or_insert_with(|| {
                            let id = num_states;
                            num_states += 1;
                            queue.push_back((a_next, b_next));
                            id
                        });
                        transitions
                            .entry((src, sym))
                            .or_insert_with(|| HashSet::with_capacity(1))
                            .insert(dst);
                    }
                }
            }
        }
    }

    let mut accepting = vec![false; num_states];
    for (&(as_, bs), &id) in &state_map {
        let a_acc = as_ < a.accepting.len() && a.accepting[as_];
        let b_acc = bs < b.accepting.len() && b.accepting[bs];
        accepting[id] = a_acc || b_acc;
    }

    PresburgerNfa {
        num_states,
        accepting,
        transitions,
        initial,
        num_vars,
        bit_width,
    }
}

/// Complement an NFA for fixed-length Presburger languages.
///
/// For position-unfolded NFAs (where valid inputs have exactly `bit_width` symbols),
/// standard complement (determinize + flip all states) is incorrect because
/// intermediate-position states that are non-accepting in the original would become
/// accepting in the complement, allowing the complement to accept partial inputs.
///
/// Instead, this function:
/// 1. Determinizes via subset construction (without the dead-state trap).
/// 2. Tracks the BFS depth of each DFA state from the initial state.
/// 3. Only flips acceptance for states at depth == `bit_width` (terminal states).
/// 4. All other states remain non-accepting.
///
/// The result is technically a DFA stored in the NFA representation for uniformity.
pub fn complement_nfa(nfa: &PresburgerNfa) -> PresburgerNfa {
    complement_fixed_length(nfa)
}

/// Fixed-length complement: determinize + flip only terminal-depth states.
fn complement_fixed_length(nfa: &PresburgerNfa) -> PresburgerNfa {
    let alpha_size = 1u32 << nfa.num_vars;
    let bit_width = nfa.bit_width;

    // Subset construction with depth tracking.
    // We use BTreeSet<usize> as DFA states (sets of NFA states).
    // The empty set is a valid DFA state (dead/sink state).
    let mut state_map: HashMap<BTreeSet<usize>, usize> = HashMap::new();
    let mut state_depth: Vec<usize> = Vec::new();
    let mut num_states = 0usize;
    let mut transitions: HashMap<(usize, u32), HashSet<usize>> = HashMap::new();
    let mut queue: VecDeque<(BTreeSet<usize>, usize)> = VecDeque::new();

    let init_set: BTreeSet<usize> = nfa.initial.iter().copied().collect();
    let init_id = num_states;
    state_map.insert(init_set.clone(), init_id);
    state_depth.push(0);
    num_states += 1;
    queue.push_back((init_set, 0));

    let mut initial = HashSet::with_capacity(1);
    initial.insert(init_id);

    while let Some((cur_set, depth)) = queue.pop_front() {
        let src = state_map[&cur_set];

        // Don't explore beyond bit_width depth.
        if depth >= bit_width {
            continue;
        }

        for sym in 0..alpha_size {
            let mut next_set = BTreeSet::new();
            for &s in &cur_set {
                if let Some(succs) = nfa.transitions.get(&(s, sym)) {
                    for &ns in succs {
                        next_set.insert(ns);
                    }
                }
            }

            // The empty set is a valid DFA state (dead/sink).
            // In the complement, dead states at terminal depth become accepting.
            let dst = *state_map.entry(next_set.clone()).or_insert_with(|| {
                let id = num_states;
                num_states += 1;
                state_depth.push(depth + 1);
                queue.push_back((next_set, depth + 1));
                id
            });

            let mut set = HashSet::with_capacity(1);
            set.insert(dst);
            transitions.insert((src, sym), set);
        }
    }

    // Flip accepting ONLY for terminal-depth states (depth == bit_width).
    // Intermediate states remain non-accepting in the complement.
    let mut accepting = vec![false; num_states];
    for (nfa_set, &dfa_id) in &state_map {
        if state_depth[dfa_id] == bit_width {
            let any_accept = nfa_set
                .iter()
                .any(|&s| s < nfa.accepting.len() && nfa.accepting[s]);
            accepting[dfa_id] = !any_accept;
        }
    }

    PresburgerNfa {
        num_states,
        accepting,
        transitions,
        initial,
        num_vars: nfa.num_vars,
        bit_width: nfa.bit_width,
    }
}

/// Existential projection: eliminate one variable from the NFA.
///
/// For variable `var`, the resulting NFA accepts an input over `k-1` variables
/// iff there exists a value for `xᵥ` such that the original NFA accepts the
/// extended input.
///
/// Implementation: for each transition on symbol `s`, add transitions for both
/// possible values of bit `var` (0 and 1) in `s`, projecting away that dimension.
/// Then epsilon-close to handle the nondeterminism.
///
/// The projected NFA keeps `num_vars` unchanged but the `var`-th bit dimension
/// becomes "free" (both 0 and 1 are tried at each position).
pub fn project_nfa(nfa: &PresburgerNfa, var: usize) -> PresburgerNfa {
    assert!(var < nfa.num_vars, "variable index out of range");

    let mut transitions: HashMap<(usize, u32), HashSet<usize>> = HashMap::new();

    // For each original transition (state, sym) -> succs,
    // create transitions for the projected symbol (with bit `var` masked out).
    // The projected NFA tries both bit values for `var`.
    for (&(state, sym), succs) in &nfa.transitions {
        // For each projected input (ignoring var's bit), we allow transitions
        // from any original symbol that matches on the non-var bits.
        // The key insight: when reading projected symbol `ps`, we try both
        // `ps` (var=0) and `ps | (1 << var)` (var=1).
        //
        // So for each original sym, its successors contribute to the
        // projected symbol that matches on non-var bits.
        let ps = sym & !(1u32 << var); // non-var bits of this symbol
        transitions
            .entry((state, ps))
            .or_insert_with(HashSet::new)
            .extend(succs.iter().copied());
        // Also contribute to ps | (1 << var) if it matches.
        // Actually, the above already handles it: we iterate over all original
        // transitions, and for each one, we add its successors under the
        // projected symbol (non-var bits only).
    }

    PresburgerNfa {
        num_states: nfa.num_states,
        accepting: nfa.accepting.clone(),
        transitions,
        initial: nfa.initial.clone(),
        num_vars: nfa.num_vars,
        bit_width: nfa.bit_width,
    }
}

/// Check if the language of the NFA is empty (no accepting run exists).
pub fn is_empty_nfa(nfa: &PresburgerNfa) -> bool {
    nfa.is_empty_language()
}

/// Find a shortest witness (accepting path) in the NFA.
///
/// Returns `None` if the language is empty.
pub fn witness_nfa(nfa: &PresburgerNfa) -> Option<IntAssignment> {
    nfa.witness()
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Floor division: `floor(a / b)` for integers.
///
/// Rust's integer division truncates toward zero; this function always floors
/// (rounds toward negative infinity), which is required for the carry computation.
#[inline]
pub fn div_floor(a: i64, b: i64) -> i64 {
    let d = a / b;
    let r = a % b;
    // If remainder is non-zero and signs of a and b differ, floor is d-1.
    if (r != 0) && ((r ^ b) < 0) {
        d - 1
    } else {
        d
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// PresburgerAlgebra — BooleanAlgebra impl (Sprint 6)
// ══════════════════════════════════════════════════════════════════════════════

/// Direct BooleanAlgebra implementation for Presburger arithmetic.
///
/// Uses NFA-based satisfiability (Büchi construction + emptiness check) for
/// the `is_satisfiable` and `witness` operations. This is the "fast path"
/// that avoids the LogicT search overhead of `TheoryAlgebra<PresburgerTheory>`.
///
/// # Correctness
///
/// The NFA-based approach and the `TheoryAlgebra` validation path must produce
/// identical results for all predicates. This is verified by cross-validation
/// tests.
#[derive(Clone, Debug)]
pub struct PresburgerAlgebra {
    /// Bit width for bounded integer representation.
    pub bit_width: usize,
}

impl PresburgerAlgebra {
    /// Create a new `PresburgerAlgebra` with the given bit width.
    pub fn new(bit_width: usize) -> Self {
        PresburgerAlgebra { bit_width }
    }

    /// Create a new `PresburgerAlgebra` with the default bit width (16).
    pub fn default_width() -> Self {
        PresburgerAlgebra {
            bit_width: DEFAULT_BIT_WIDTH,
        }
    }
}

impl BooleanAlgebra for PresburgerAlgebra {
    type Predicate = PresburgerPred;
    type Domain = IntAssignment;

    fn true_pred(&self) -> Self::Predicate {
        PresburgerPred::True
    }

    fn false_pred(&self) -> Self::Predicate {
        PresburgerPred::False
    }

    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (PresburgerPred::True, _) => b.clone(),
            (_, PresburgerPred::True) => a.clone(),
            (PresburgerPred::False, _) | (_, PresburgerPred::False) => PresburgerPred::False,
            _ => PresburgerPred::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (PresburgerPred::True, _) | (_, PresburgerPred::True) => PresburgerPred::True,
            (PresburgerPred::False, _) => b.clone(),
            (_, PresburgerPred::False) => a.clone(),
            _ => PresburgerPred::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn not(&self, a: &Self::Predicate) -> Self::Predicate {
        match a {
            PresburgerPred::True => PresburgerPred::False,
            PresburgerPred::False => PresburgerPred::True,
            PresburgerPred::Not(inner) => (**inner).clone(),
            _ => PresburgerPred::Not(Box::new(a.clone())),
        }
    }

    fn is_satisfiable(&self, pred: &Self::Predicate) -> bool {
        // Fast path: structural shortcuts.
        match pred {
            PresburgerPred::True => return true,
            PresburgerPred::False => return false,
            _ => {}
        }

        let nfa = PresburgerNfa::from_pred(pred, self.bit_width);
        nfa.is_nonempty()
    }

    fn witness(&self, pred: &Self::Predicate) -> Option<Self::Domain> {
        match pred {
            PresburgerPred::True => return Some(IntAssignment(vec![0])),
            PresburgerPred::False => return None,
            _ => {}
        }

        let nfa = PresburgerNfa::from_pred(pred, self.bit_width);
        nfa.witness()
    }

    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool {
        evaluate_presburger(pred, elem, self.bit_width)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// PresburgerTheory — ConstraintTheory impl (Sprint 6)
// ══════════════════════════════════════════════════════════════════════════════

/// Presburger constraint store: accumulated conjunction of linear constraints.
///
/// Each constraint is propagated by adding it to the store. The store is
/// consistent iff the conjunction of all constraints is satisfiable (checked
/// via NFA emptiness).
#[derive(Clone, Debug)]
pub struct PresburgerStore {
    /// Accumulated constraints (conjunction).
    constraints: Vec<LinearConstraint>,
    /// Cached NFA for the conjunction (recomputed on each propagation).
    /// `None` if the store is known to be inconsistent.
    nfa: Option<PresburgerNfa>,
    /// Bit width for NFA construction.
    bit_width: usize,
    /// Number of variables across all constraints.
    num_vars: usize,
}

impl PresburgerStore {
    fn new(bit_width: usize) -> Self {
        PresburgerStore {
            constraints: Vec::new(),
            nfa: Some(PresburgerNfa::universal(1, bit_width)),
            bit_width,
            num_vars: 1,
        }
    }

    fn rebuild_nfa(&mut self) {
        if self.constraints.is_empty() {
            self.nfa = Some(PresburgerNfa::universal(
                self.num_vars.max(1),
                self.bit_width,
            ));
            return;
        }

        // Build conjunction NFA from all constraints.
        let nv = self.num_vars.max(1);
        let mut nfa = PresburgerNfa::from_constraint(&self.constraints[0], nv, self.bit_width);
        for c in &self.constraints[1..] {
            let c_nfa = PresburgerNfa::from_constraint(c, nv, self.bit_width);
            nfa = intersect_nfa(&nfa, &c_nfa);
        }

        if nfa.is_empty_language() {
            self.nfa = None;
        } else {
            self.nfa = Some(nfa);
        }
    }
}

/// Presburger arithmetic as a `ConstraintTheory`.
///
/// This is the validation path: constraints are accumulated in a store and
/// satisfiability is checked via NFA construction. The `label()` method returns
/// `LogicStream::empty()` because Presburger arithmetic is decidable — propagation
/// alone determines satisfiability.
///
/// When the `symbolic-automata` feature is also enabled, `TheoryAlgebra<PresburgerTheory>`
/// provides a BooleanAlgebra implementation that can be cross-validated against
/// the direct `PresburgerAlgebra` fast path.
#[derive(Clone, Debug)]
pub struct PresburgerTheory {
    /// Bit width for bounded integer representation.
    pub bit_width: usize,
}

impl PresburgerTheory {
    /// Create a new `PresburgerTheory` with the given bit width.
    pub fn new(bit_width: usize) -> Self {
        PresburgerTheory { bit_width }
    }

    /// Create a new `PresburgerTheory` with the default bit width (16).
    pub fn default_width() -> Self {
        PresburgerTheory {
            bit_width: DEFAULT_BIT_WIDTH,
        }
    }
}

impl ConstraintTheory for PresburgerTheory {
    type Constraint = LinearConstraint;
    type Assignment = IntAssignment;
    type Store = PresburgerStore;

    fn empty_store(&self) -> Self::Store {
        PresburgerStore::new(self.bit_width)
    }

    fn propagate(&self, store: &Self::Store, c: &Self::Constraint) -> Option<Self::Store> {
        let mut new_store = store.clone();
        let c_num_vars = c.num_vars();
        new_store.num_vars = new_store.num_vars.max(c_num_vars);
        new_store.constraints.push(c.clone());
        new_store.rebuild_nfa();
        if new_store.nfa.is_some() {
            Some(new_store)
        } else {
            None
        }
    }

    fn is_consistent(&self, store: &Self::Store) -> bool {
        store.nfa.is_some()
    }

    fn witness(&self, store: &Self::Store) -> Option<Self::Assignment> {
        store.nfa.as_ref().and_then(|nfa| nfa.witness())
    }

    fn label(&self, _store: &Self::Store) -> LogicStream<Self::Constraint> {
        // Presburger arithmetic is decidable — no search labeling needed.
        LogicStream::empty()
    }

    fn evaluate(&self, c: &Self::Constraint, assignment: &Self::Assignment) -> bool {
        c.evaluate(assignment)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Analysis result
// ══════════════════════════════════════════════════════════════════════════════

/// Analysis result from Presburger arithmetic guard checking.
#[derive(Debug, Clone, Default)]
pub struct PresburgerAnalysis {
    /// Guards found to be unsatisfiable (dead code). (guard_desc, rule_label)
    pub unsatisfiable_guards: Vec<(String, String)>,
    /// Guards found to be tautological (always-true). (guard_desc, rule_label)
    pub tautological_guards: Vec<(String, String)>,
    /// Guard pairs where one subsumes the other. (subsuming_desc, subsumed_desc, subsumed_rule)
    pub subsumed_guards: Vec<(String, String, String)>,
}

// ══════════════════════════════════════════════════════════════════════════════
// Bundle analysis
// ══════════════════════════════════════════════════════════════════════════════

/// Check satisfiability of a `PresburgerPred` using the NFA-based decision procedure.
///
/// Builds an NFA for `pred` at the given bit width and checks if the language is
/// non-empty (i.e., there exists at least one integer tuple satisfying `pred`).
pub fn is_satisfiable_nfa(pred: &PresburgerPred, bit_width: usize) -> bool {
    match pred {
        PresburgerPred::True => true,
        PresburgerPred::False => false,
        _ => {
            let nfa = PresburgerNfa::from_pred(pred, bit_width);
            nfa.is_nonempty()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const DEEP_PRESBURGER_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    fn predicate_strategy() -> BoxedStrategy<PresburgerPred> {
        prop_oneof![
            Just(PresburgerPred::True),
            Just(PresburgerPred::False),
            (-2i64..=2, -3i64..=3).prop_map(|(coefficient, rhs)| {
                PresburgerPred::Atom(LinearConstraint::new(vec![(0, coefficient)], rhs))
            }),
        ]
        .prop_recursive(5, 128, 3, |inner| {
            prop_oneof![
                inner
                    .clone()
                    .prop_map(|value| PresburgerPred::Not(Box::new(value))),
                (inner.clone(), inner.clone()).prop_map(|(left, right)| {
                    PresburgerPred::And(Box::new(left), Box::new(right))
                }),
                (inner.clone(), inner).prop_map(|(left, right)| {
                    PresburgerPred::Or(Box::new(left), Box::new(right))
                }),
            ]
        })
        .boxed()
    }

    fn recursive_evaluate(predicate: &PresburgerPred, value: i64) -> bool {
        match predicate {
            PresburgerPred::True => true,
            PresburgerPred::False => false,
            PresburgerPred::Atom(constraint) => constraint.evaluate(&IntAssignment(vec![value])),
            PresburgerPred::And(left, right) => {
                recursive_evaluate(left, value) && recursive_evaluate(right, value)
            }
            PresburgerPred::Or(left, right) => {
                recursive_evaluate(left, value) || recursive_evaluate(right, value)
            }
            PresburgerPred::Not(inner) => !recursive_evaluate(inner, value),
            PresburgerPred::Exists { .. } => {
                unreachable!("the bounded property strategy has no quantifiers")
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn polarity_machine_and_nfa_refine_bounded_recursive_semantics(
            predicate in predicate_strategy(),
        ) {
            let normalized = push_negation_inward(&predicate);
            let expected = (-2i64..=1)
                .map(|value| recursive_evaluate(&predicate, value))
                .collect::<Vec<_>>();
            let actual = (-2i64..=1)
                .map(|value| recursive_evaluate(&normalized, value))
                .collect::<Vec<_>>();
            prop_assert_eq!(&actual, &expected);

            let nfa = PresburgerNfa::from_pred(&predicate, 2);
            prop_assert_eq!(nfa.is_nonempty(), expected.into_iter().any(|value| value));
            if let Some(witness) = nfa.witness() {
                prop_assert!(recursive_evaluate(&predicate, witness.get(0)));
            }
        }
    }

    #[test]
    fn deep_polarity_and_nfa_compilation_use_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-presburger-machine".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let mut predicate = PresburgerPred::True;
                for _ in 0..DEEP_PRESBURGER_DEPTH {
                    predicate = PresburgerPred::Not(Box::new(predicate));
                }

                assert!(matches!(
                    push_negation_inward(&predicate),
                    PresburgerPred::True
                ));
                assert!(matches!(
                    normalize_with_polarity(&predicate, true),
                    PresburgerPred::False
                ));
                let nfa = PresburgerNfa::compile_nnf(&predicate, 1, 1);
                assert!(nfa.is_nonempty());
                drop(nfa);
                drop(predicate);
            })
            .expect("the bounded-stack Presburger worker must spawn")
            .join()
            .expect("Presburger polarity, compilation, and lifecycle must not overflow the native stack");
    }
}
