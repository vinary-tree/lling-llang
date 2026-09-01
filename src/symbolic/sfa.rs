//! Symbolic Automata (SFA) with predicate-labeled transitions over infinite domains.
//!
//! ## Theory
//!
//! Symbolic Finite Automata (SFA) generalize classical finite automata by replacing
//! finite alphabets with transitions guarded by predicates from an effective Boolean
//! algebra (D'Antoni & Veanes, 2017). Where a classical DFA has transitions labeled
//! with individual symbols from a finite set Sigma, an SFA has transitions labeled
//! with predicates over a potentially infinite domain D. A transition fires when the
//! current input element satisfies the guard predicate.
//!
//! The key abstraction is the **`BooleanAlgebra`** trait, which provides:
//! - Predicate constructors: `true_pred`, `false_pred`, `and`, `or`, `not`
//! - Decision procedures: `is_satisfiable`, `witness`
//! - Evaluation: `evaluate(predicate, element) -> bool`
//!
//! These operations suffice for all standard automata algorithms (emptiness, intersection,
//! complement, determinization, equivalence) to work symbolically, without enumerating
//! the potentially infinite domain.
//!
//! ### Minterm-Based Determinization
//!
//! The determinization algorithm uses minterms — maximal satisfiable conjunctions of
//! predicates and their negations. For a set of predicates {phi_1, ..., phi_k} appearing
//! on outgoing transitions, the minterms partition the domain into equivalence classes
//! where all elements are treated identically by every guard. This reduces the problem
//! to classical subset construction over a finite (though potentially exponential)
//! effective alphabet.
//!
//! ### References
//!
//! - D'Antoni, L. & Veanes, M. (2017). "The power of symbolic automata and transducers."
//!   CAV 2017. The foundational paper on effective Boolean algebras for symbolic automata.
//! - Veanes, M., de Halleux, P., & Tillmann, N. (2010). "Rex: Symbolic regular expression
//!   explorer." ICST 2010. Symbolic execution of regular expressions.
//! - D'Antoni, L. & Veanes, M. (2014). "Minimization of symbolic automata." POPL 2014.
//!   Efficient minimization using predicates rather than explicit alphabets.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                PraTTaIL Pipeline                         │
//! │                                                         │
//! │  Grammar rules                                          │
//! │       │                                                 │
//! │       ▼                                                 │
//! │  WFST + Decision Tree (finite-alphabet dispatch)        │
//! │       │                                                 │
//! │       │    ┌─────────────────────────────────────┐      │
//! │       └───▶│  Symbolic Automata Module            │      │
//! │            │                                     │      │
//! │            │  BooleanAlgebra (trait)              │      │
//! │            │    ├── IntervalAlgebra (numeric)     │      │
//! │            │    ├── CharClassAlgebra (characters)  │      │
//! │            │    └── KatBooleanAlgebra (propositional)│   │
//! │            │                                     │      │
//! │            │  SymbolicAutomaton<A>                │      │
//! │            │    ├── is_empty()                    │      │
//! │            │    ├── accepts()                     │      │
//! │            │    ├── intersect()                   │      │
//! │            │    ├── complement()                  │      │
//! │            │    ├── determinize()                 │      │
//! │            │    └── is_equivalent()               │      │
//! │            │                                     │      │
//! │            │  DecidabilityClassifier              │      │
//! │            │    └── classify_decidability()       │      │
//! │            │                                     │      │
//! │            │  SymbolicAnalysis (pipeline bridge)  │      │
//! │            └─────────────────────────────────────┘      │
//! │                                                         │
//! │  KAT module ◄──── KatBooleanAlgebra adapter             │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Integration Points
//!
//! - **KAT module** (`kat.rs`): The `KatBooleanAlgebra` adapter wraps `BooleanTest`
//!   predicates from the KAT module, enabling symbolic automata operations over
//!   propositional predicates used in Hoare logic verification.
//! - **Pipeline analysis** (`pipeline.rs`): `SymbolicAnalysis` provides guard
//!   satisfiability, overlap, and subsumption data for lint diagnostics.
//! - **Decision tree** (`decision_tree.rs`): Symbolic automata can model dispatch
//!   decisions over predicate-guarded transitions (e.g., character class checks),
//!   complementing the PathMap-based trie dispatch.
//! - **WFST** (`wfst.rs`): Symbolic guards generalize WFST input labels from
//!   finite tokens to predicate-guarded transitions, enabling analysis over
//!   infinite token domains (e.g., all integers, all identifiers matching a pattern).

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};

// ══════════════════════════════════════════════════════════════════════════════
// BooleanAlgebra trait
// ══════════════════════════════════════════════════════════════════════════════

/// Effective Boolean algebra over predicates — the core abstraction for symbolic automata.
///
/// An effective Boolean algebra provides decidable operations over a potentially
/// infinite domain via predicates. The algebra must support:
/// - Construction of true/false/and/or/not predicates
/// - A satisfiability decision procedure
/// - Witness generation (finding a concrete domain element satisfying a predicate)
/// - Evaluation of predicates on concrete domain elements
///
/// These operations enable all standard automata algorithms (emptiness, intersection,
/// complement, determinization, equivalence) to work symbolically.
///
/// # Type Parameters
///
/// - `Predicate`: The type of guard predicates (e.g., `IntervalPred`, `CharClassPred`).
/// - `Domain`: The type of concrete elements the predicates range over (e.g., `i64`, `char`).
pub trait BooleanAlgebra: Clone + std::fmt::Debug + Send + Sync + 'static {
    /// The type of guard predicates in this algebra.
    type Predicate: Clone + std::fmt::Debug + Eq + std::hash::Hash + Send + Sync + 'static;

    /// The type of concrete domain elements that predicates evaluate over.
    type Domain: Clone + std::fmt::Debug + Send + Sync + 'static;

    /// The predicate that is always satisfied (accepts all domain elements).
    fn true_pred(&self) -> Self::Predicate;

    /// The predicate that is never satisfied (rejects all domain elements).
    fn false_pred(&self) -> Self::Predicate;

    /// Conjunction: `a AND b`. Satisfied when both `a` and `b` are satisfied.
    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate;

    /// Disjunction: `a OR b`. Satisfied when either `a` or `b` is satisfied.
    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate;

    /// Negation: `NOT a`. Satisfied when `a` is not satisfied.
    fn not(&self, a: &Self::Predicate) -> Self::Predicate;

    /// Satisfiability check: does there exist a domain element satisfying `a`?
    ///
    /// This is the core decision procedure. All derived methods (implies,
    /// equivalent, is_tautology, overlaps) reduce to satisfiability queries.
    fn is_satisfiable(&self, a: &Self::Predicate) -> bool;

    /// Witness generation: find a concrete domain element satisfying `a`, if one exists.
    ///
    /// Returns `None` iff `a` is unsatisfiable.
    fn witness(&self, a: &Self::Predicate) -> Option<Self::Domain>;

    /// Evaluate a predicate on a concrete domain element.
    ///
    /// Returns `true` iff `elem` satisfies `pred`.
    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool;

    /// Implication: does `a` imply `b`? Equivalently, is `a AND NOT b` unsatisfiable?
    fn implies(&self, a: &Self::Predicate, b: &Self::Predicate) -> bool {
        !self.is_satisfiable(&self.and(a, &self.not(b)))
    }

    /// Semantic equivalence: are `a` and `b` satisfied by exactly the same elements?
    fn equivalent(&self, a: &Self::Predicate, b: &Self::Predicate) -> bool {
        self.implies(a, b) && self.implies(b, a)
    }

    /// Tautology check: is `a` satisfied by all domain elements?
    fn is_tautology(&self, a: &Self::Predicate) -> bool {
        !self.is_satisfiable(&self.not(a))
    }

    /// Overlap check: can `a` and `b` be simultaneously satisfied?
    fn overlaps(&self, a: &Self::Predicate, b: &Self::Predicate) -> bool {
        self.is_satisfiable(&self.and(a, b))
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// IntervalAlgebra — numeric range predicates
// ══════════════════════════════════════════════════════════════════════════════

/// A predicate over integer intervals.
///
/// Represents sets of integers via half-open ranges `[lo, hi)`, their unions,
/// and their complements. The algebra domain is `i64` values within a
/// configured `[min_val, max_val)` universe.
pub enum IntervalPred {
    /// The universal predicate: satisfied by all integers in `[min_val, max_val)`.
    True,
    /// The empty predicate: satisfied by no integer.
    False,
    /// A single half-open range `[lo, hi)`.
    Range(i64, i64),
    /// A union of sorted, non-overlapping half-open ranges.
    Union(Vec<(i64, i64)>),
    /// Complement of a predicate (relative to the universe `[min_val, max_val)`).
    Not(Box<IntervalPred>),
}

impl Clone for IntervalPred {
    fn clone(&self) -> Self {
        let mut negations = 0usize;
        let mut current = self;
        while let IntervalPred::Not(inner) = current {
            negations += 1;
            current = inner;
        }
        let mut cloned = match current {
            IntervalPred::True => IntervalPred::True,
            IntervalPred::False => IntervalPred::False,
            IntervalPred::Range(lo, hi) => IntervalPred::Range(*lo, *hi),
            IntervalPred::Union(ranges) => IntervalPred::Union(ranges.clone()),
            IntervalPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        };
        for _ in 0..negations {
            cloned = IntervalPred::Not(Box::new(cloned));
        }
        cloned
    }
}

impl PartialEq for IntervalPred {
    fn eq(&self, other: &Self) -> bool {
        let mut left = self;
        let mut right = other;
        loop {
            match (left, right) {
                (IntervalPred::True, IntervalPred::True)
                | (IntervalPred::False, IntervalPred::False) => return true,
                (IntervalPred::Range(ll, lh), IntervalPred::Range(rl, rh)) => {
                    return ll == rl && lh == rh;
                }
                (IntervalPred::Union(left), IntervalPred::Union(right)) => {
                    return left == right;
                }
                (IntervalPred::Not(li), IntervalPred::Not(ri)) => {
                    left = li;
                    right = ri;
                }
                _ => return false,
            }
        }
    }
}

impl Eq for IntervalPred {}

impl Hash for IntervalPred {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut current = self;
        loop {
            std::mem::discriminant(current).hash(state);
            match current {
                IntervalPred::True | IntervalPred::False => return,
                IntervalPred::Range(lo, hi) => {
                    lo.hash(state);
                    hi.hash(state);
                    return;
                }
                IntervalPred::Union(ranges) => {
                    ranges.hash(state);
                    return;
                }
                IntervalPred::Not(inner) => current = inner,
            }
        }
    }
}

impl fmt::Debug for IntervalPred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut negations = 0usize;
        let mut current = self;
        while let IntervalPred::Not(inner) = current {
            write!(f, "Not(")?;
            negations += 1;
            current = inner;
        }
        match current {
            IntervalPred::True => write!(f, "True")?,
            IntervalPred::False => write!(f, "False")?,
            IntervalPred::Range(lo, hi) => write!(f, "Range({lo:?}, {hi:?})")?,
            IntervalPred::Union(ranges) => write!(f, "Union({ranges:?})")?,
            IntervalPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        }
        for _ in 0..negations {
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl Drop for IntervalPred {
    fn drop(&mut self) {
        fn take_child(predicate: &mut IntervalPred) -> Option<IntervalPred> {
            match predicate {
                IntervalPred::Not(inner) => {
                    Some(std::mem::replace(&mut **inner, IntervalPred::True))
                }
                IntervalPred::True
                | IntervalPred::False
                | IntervalPred::Range(_, _)
                | IntervalPred::Union(_) => None,
            }
        }
        let mut current = take_child(self);
        while let Some(mut predicate) = current {
            current = take_child(&mut predicate);
        }
    }
}

impl fmt::Display for IntervalPred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut current = self;
        while let IntervalPred::Not(inner) = current {
            write!(f, "~")?;
            current = inner;
        }
        match current {
            IntervalPred::True => write!(f, "TRUE"),
            IntervalPred::False => write!(f, "FALSE"),
            IntervalPred::Range(lo, hi) => write!(f, "[{}, {})", lo, hi),
            IntervalPred::Union(ranges) => {
                write!(f, "(")?;
                for (i, (lo, hi)) in ranges.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "[{}, {})", lo, hi)?;
                }
                write!(f, ")")
            }
            IntervalPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        }
    }
}

/// Boolean algebra over integer intervals within a bounded universe.
///
/// The domain is `i64` values in `[min_val, max_val)`. Predicates are
/// expressed as unions of half-open ranges.
#[derive(Clone, Debug)]
pub struct IntervalAlgebra {
    /// Minimum value in the universe (inclusive).
    pub min_val: i64,
    /// Maximum value in the universe (exclusive).
    pub max_val: i64,
}

impl IntervalAlgebra {
    /// Create a new interval algebra with the given universe bounds.
    pub fn new(min_val: i64, max_val: i64) -> Self {
        assert!(
            min_val < max_val,
            "IntervalAlgebra requires min_val ({}) < max_val ({})",
            min_val,
            max_val,
        );
        IntervalAlgebra { min_val, max_val }
    }

    /// Normalize a predicate to a canonical union-of-ranges form.
    ///
    /// Returns a sorted, non-overlapping list of half-open ranges `[lo, hi)`
    /// representing exactly the set of integers satisfying the predicate
    /// within the universe `[min_val, max_val)`.
    fn normalize(&self, pred: &IntervalPred) -> Vec<(i64, i64)> {
        let mut current = pred;
        let mut negated = false;
        while let IntervalPred::Not(inner) = current {
            negated = !negated;
            current = inner;
        }
        let ranges = match current {
            IntervalPred::True => vec![(self.min_val, self.max_val)],
            IntervalPred::False => vec![],
            IntervalPred::Range(lo, hi) => {
                let lo = (*lo).max(self.min_val);
                let hi = (*hi).min(self.max_val);
                if lo < hi {
                    vec![(lo, hi)]
                } else {
                    vec![]
                }
            }
            IntervalPred::Union(ranges) => {
                // Clip and merge ranges into canonical form.
                let mut clipped: Vec<(i64, i64)> = ranges
                    .iter()
                    .filter_map(|&(lo, hi)| {
                        let lo = lo.max(self.min_val);
                        let hi = hi.min(self.max_val);
                        if lo < hi {
                            Some((lo, hi))
                        } else {
                            None
                        }
                    })
                    .collect();
                clipped.sort_unstable();
                merge_ranges(&clipped)
            }
            IntervalPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        };
        if negated {
            complement_ranges(&ranges, self.min_val, self.max_val)
        } else {
            ranges
        }
    }

    /// Build a predicate from a normalized list of ranges.
    fn from_ranges(ranges: &[(i64, i64)]) -> IntervalPred {
        match ranges.len() {
            0 => IntervalPred::False,
            1 => IntervalPred::Range(ranges[0].0, ranges[0].1),
            _ => IntervalPred::Union(ranges.to_vec()),
        }
    }
}

/// Merge a sorted list of ranges into a non-overlapping union.
fn merge_ranges(sorted: &[(i64, i64)]) -> Vec<(i64, i64)> {
    if sorted.is_empty() {
        return vec![];
    }
    let mut result = Vec::with_capacity(sorted.len());
    let (mut cur_lo, mut cur_hi) = sorted[0];
    for &(lo, hi) in &sorted[1..] {
        if lo <= cur_hi {
            // Overlapping or adjacent — extend.
            cur_hi = cur_hi.max(hi);
        } else {
            result.push((cur_lo, cur_hi));
            cur_lo = lo;
            cur_hi = hi;
        }
    }
    result.push((cur_lo, cur_hi));
    result
}

/// Complement a sorted, non-overlapping list of ranges within `[min_val, max_val)`.
fn complement_ranges(ranges: &[(i64, i64)], min_val: i64, max_val: i64) -> Vec<(i64, i64)> {
    let mut result = Vec::with_capacity(ranges.len() + 1);
    let mut cursor = min_val;
    for &(lo, hi) in ranges {
        if cursor < lo {
            result.push((cursor, lo));
        }
        cursor = hi;
    }
    if cursor < max_val {
        result.push((cursor, max_val));
    }
    result
}

/// Intersect two sorted, non-overlapping range lists.
fn intersect_ranges(a: &[(i64, i64)], b: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut result = Vec::with_capacity(a.len().min(b.len()));
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if lo < hi {
            result.push((lo, hi));
        }
        // Advance the range that ends first.
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

/// Union two sorted, non-overlapping range lists.
fn union_ranges(a: &[(i64, i64)], b: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut combined = Vec::with_capacity(a.len() + b.len());
    combined.extend_from_slice(a);
    combined.extend_from_slice(b);
    combined.sort_unstable();
    merge_ranges(&combined)
}

impl BooleanAlgebra for IntervalAlgebra {
    type Predicate = IntervalPred;
    type Domain = i64;

    fn true_pred(&self) -> IntervalPred {
        IntervalPred::True
    }

    fn false_pred(&self) -> IntervalPred {
        IntervalPred::False
    }

    fn and(&self, a: &IntervalPred, b: &IntervalPred) -> IntervalPred {
        let ra = self.normalize(a);
        let rb = self.normalize(b);
        let result = intersect_ranges(&ra, &rb);
        IntervalAlgebra::from_ranges(&result)
    }

    fn or(&self, a: &IntervalPred, b: &IntervalPred) -> IntervalPred {
        let ra = self.normalize(a);
        let rb = self.normalize(b);
        let result = union_ranges(&ra, &rb);
        IntervalAlgebra::from_ranges(&result)
    }

    fn not(&self, a: &IntervalPred) -> IntervalPred {
        let ra = self.normalize(a);
        let result = complement_ranges(&ra, self.min_val, self.max_val);
        IntervalAlgebra::from_ranges(&result)
    }

    fn is_satisfiable(&self, a: &IntervalPred) -> bool {
        !self.normalize(a).is_empty()
    }

    fn witness(&self, a: &IntervalPred) -> Option<i64> {
        let ranges = self.normalize(a);
        ranges.first().map(|&(lo, _)| lo)
    }

    fn evaluate(&self, pred: &IntervalPred, elem: &i64) -> bool {
        let val = *elem;
        if val < self.min_val || val >= self.max_val {
            return false;
        }
        let ranges = self.normalize(pred);
        ranges.iter().any(|&(lo, hi)| val >= lo && val < hi)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// CharClassAlgebra — character class predicates
// ══════════════════════════════════════════════════════════════════════════════

/// A predicate over Unicode character classes.
///
/// Represents sets of characters via inclusive ranges `[lo, hi]`, their unions,
/// and their complements. The domain is the full Unicode scalar value range
/// `['\0', char::MAX]`.
pub enum CharClassPred {
    /// The universal predicate: satisfied by all characters.
    True,
    /// The empty predicate: satisfied by no character.
    False,
    /// A single inclusive character range `[lo, hi]`.
    Range(char, char),
    /// A union of sorted, non-overlapping inclusive character ranges.
    Union(Vec<(char, char)>),
    /// Complement of a predicate (relative to the full Unicode range).
    Not(Box<CharClassPred>),
}

impl Clone for CharClassPred {
    fn clone(&self) -> Self {
        let mut negations = 0usize;
        let mut current = self;
        while let CharClassPred::Not(inner) = current {
            negations += 1;
            current = inner;
        }
        let mut cloned = match current {
            CharClassPred::True => CharClassPred::True,
            CharClassPred::False => CharClassPred::False,
            CharClassPred::Range(lo, hi) => CharClassPred::Range(*lo, *hi),
            CharClassPred::Union(ranges) => CharClassPred::Union(ranges.clone()),
            CharClassPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        };
        for _ in 0..negations {
            cloned = CharClassPred::Not(Box::new(cloned));
        }
        cloned
    }
}

impl PartialEq for CharClassPred {
    fn eq(&self, other: &Self) -> bool {
        let mut left = self;
        let mut right = other;
        loop {
            match (left, right) {
                (CharClassPred::True, CharClassPred::True)
                | (CharClassPred::False, CharClassPred::False) => return true,
                (CharClassPred::Range(ll, lh), CharClassPred::Range(rl, rh)) => {
                    return ll == rl && lh == rh;
                }
                (CharClassPred::Union(left), CharClassPred::Union(right)) => {
                    return left == right;
                }
                (CharClassPred::Not(li), CharClassPred::Not(ri)) => {
                    left = li;
                    right = ri;
                }
                _ => return false,
            }
        }
    }
}

impl Eq for CharClassPred {}

impl Hash for CharClassPred {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut current = self;
        loop {
            std::mem::discriminant(current).hash(state);
            match current {
                CharClassPred::True | CharClassPred::False => return,
                CharClassPred::Range(lo, hi) => {
                    lo.hash(state);
                    hi.hash(state);
                    return;
                }
                CharClassPred::Union(ranges) => {
                    ranges.hash(state);
                    return;
                }
                CharClassPred::Not(inner) => current = inner,
            }
        }
    }
}

impl fmt::Debug for CharClassPred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut negations = 0usize;
        let mut current = self;
        while let CharClassPred::Not(inner) = current {
            write!(f, "Not(")?;
            negations += 1;
            current = inner;
        }
        match current {
            CharClassPred::True => write!(f, "True")?,
            CharClassPred::False => write!(f, "False")?,
            CharClassPred::Range(lo, hi) => write!(f, "Range({lo:?}, {hi:?})")?,
            CharClassPred::Union(ranges) => write!(f, "Union({ranges:?})")?,
            CharClassPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        }
        for _ in 0..negations {
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl Drop for CharClassPred {
    fn drop(&mut self) {
        fn take_child(predicate: &mut CharClassPred) -> Option<CharClassPred> {
            match predicate {
                CharClassPred::Not(inner) => {
                    Some(std::mem::replace(&mut **inner, CharClassPred::True))
                }
                CharClassPred::True
                | CharClassPred::False
                | CharClassPred::Range(_, _)
                | CharClassPred::Union(_) => None,
            }
        }
        let mut current = take_child(self);
        while let Some(mut predicate) = current {
            current = take_child(&mut predicate);
        }
    }
}

impl fmt::Display for CharClassPred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut current = self;
        while let CharClassPred::Not(inner) = current {
            write!(f, "~")?;
            current = inner;
        }
        match current {
            CharClassPred::True => write!(f, "TRUE"),
            CharClassPred::False => write!(f, "FALSE"),
            CharClassPred::Range(lo, hi) => {
                if lo == hi {
                    write!(f, "[{}]", lo.escape_debug())
                } else {
                    write!(f, "[{}-{}]", lo.escape_debug(), hi.escape_debug())
                }
            }
            CharClassPred::Union(ranges) => {
                write!(f, "[")?;
                for (i, (lo, hi)) in ranges.iter().enumerate() {
                    if i > 0 {
                        write!(f, "|")?;
                    }
                    if lo == hi {
                        write!(f, "{}", lo.escape_debug())?;
                    } else {
                        write!(f, "{}-{}", lo.escape_debug(), hi.escape_debug())?;
                    }
                }
                write!(f, "]")
            }
            CharClassPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        }
    }
}

/// Boolean algebra over Unicode character classes.
///
/// The domain is all Unicode scalar values. Predicates are expressed as
/// unions of inclusive character ranges. Internally, ranges are converted
/// to `u32` half-open intervals `[lo, hi+1)` for uniform manipulation,
/// then converted back to `(char, char)` for the public API.
#[derive(Clone, Debug)]
pub struct CharClassAlgebra;

impl CharClassAlgebra {
    /// Create a new character class algebra.
    pub fn new() -> Self {
        CharClassAlgebra
    }

    /// Normalize a predicate to a sorted, non-overlapping list of
    /// half-open `u32` ranges `[lo, hi)`.
    fn normalize_u32(pred: &CharClassPred) -> Vec<(u32, u32)> {
        let mut current = pred;
        let mut negated = false;
        while let CharClassPred::Not(inner) = current {
            negated = !negated;
            current = inner;
        }
        let ranges = match current {
            CharClassPred::True => vec![(0, (char::MAX as u32) + 1)],
            CharClassPred::False => vec![],
            CharClassPred::Range(lo, hi) => {
                if *lo <= *hi {
                    vec![(*lo as u32, (*hi as u32) + 1)]
                } else {
                    vec![]
                }
            }
            CharClassPred::Union(ranges) => {
                let mut u32_ranges: Vec<(u32, u32)> = ranges
                    .iter()
                    .filter_map(|&(lo, hi)| {
                        if lo <= hi {
                            Some((lo as u32, (hi as u32) + 1))
                        } else {
                            None
                        }
                    })
                    .collect();
                u32_ranges.sort_unstable();
                merge_u32_ranges(&u32_ranges)
            }
            CharClassPred::Not(_) => unreachable!("the unary spine was fully traversed"),
        };
        if negated {
            complement_u32_ranges(&ranges, 0, (char::MAX as u32) + 1)
        } else {
            ranges
        }
    }

    /// Build a `CharClassPred` from a list of half-open `u32` ranges.
    fn from_u32_ranges(ranges: &[(u32, u32)]) -> CharClassPred {
        let char_ranges: Vec<(char, char)> = ranges
            .iter()
            .filter_map(|&(lo, hi)| {
                if lo < hi {
                    let lo_char = char::from_u32(lo)?;
                    let hi_char = char::from_u32(hi - 1)?;
                    Some((lo_char, hi_char))
                } else {
                    None
                }
            })
            .collect();

        match char_ranges.len() {
            0 => CharClassPred::False,
            1 => CharClassPred::Range(char_ranges[0].0, char_ranges[0].1),
            _ => CharClassPred::Union(char_ranges),
        }
    }
}

impl Default for CharClassAlgebra {
    fn default() -> Self {
        Self::new()
    }
}

/// Merge sorted half-open `u32` ranges.
fn merge_u32_ranges(sorted: &[(u32, u32)]) -> Vec<(u32, u32)> {
    if sorted.is_empty() {
        return vec![];
    }
    let mut result = Vec::with_capacity(sorted.len());
    let (mut cur_lo, mut cur_hi) = sorted[0];
    for &(lo, hi) in &sorted[1..] {
        if lo <= cur_hi {
            cur_hi = cur_hi.max(hi);
        } else {
            result.push((cur_lo, cur_hi));
            cur_lo = lo;
            cur_hi = hi;
        }
    }
    result.push((cur_lo, cur_hi));
    result
}

/// Complement sorted, non-overlapping half-open `u32` ranges within `[min, max)`.
fn complement_u32_ranges(ranges: &[(u32, u32)], min: u32, max: u32) -> Vec<(u32, u32)> {
    let mut result = Vec::with_capacity(ranges.len() + 1);
    let mut cursor = min;
    for &(lo, hi) in ranges {
        if cursor < lo {
            result.push((cursor, lo));
        }
        cursor = hi;
    }
    if cursor < max {
        result.push((cursor, max));
    }
    result
}

/// Intersect two sorted, non-overlapping half-open `u32` range lists.
fn intersect_u32_ranges(a: &[(u32, u32)], b: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut result = Vec::with_capacity(a.len().min(b.len()));
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if lo < hi {
            result.push((lo, hi));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

/// Union two sorted, non-overlapping half-open `u32` range lists.
fn union_u32_ranges(a: &[(u32, u32)], b: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut combined = Vec::with_capacity(a.len() + b.len());
    combined.extend_from_slice(a);
    combined.extend_from_slice(b);
    combined.sort_unstable();
    merge_u32_ranges(&combined)
}

impl BooleanAlgebra for CharClassAlgebra {
    type Predicate = CharClassPred;
    type Domain = char;

    fn true_pred(&self) -> CharClassPred {
        CharClassPred::True
    }

    fn false_pred(&self) -> CharClassPred {
        CharClassPred::False
    }

    fn and(&self, a: &CharClassPred, b: &CharClassPred) -> CharClassPred {
        let ra = CharClassAlgebra::normalize_u32(a);
        let rb = CharClassAlgebra::normalize_u32(b);
        let result = intersect_u32_ranges(&ra, &rb);
        CharClassAlgebra::from_u32_ranges(&result)
    }

    fn or(&self, a: &CharClassPred, b: &CharClassPred) -> CharClassPred {
        let ra = CharClassAlgebra::normalize_u32(a);
        let rb = CharClassAlgebra::normalize_u32(b);
        let result = union_u32_ranges(&ra, &rb);
        CharClassAlgebra::from_u32_ranges(&result)
    }

    fn not(&self, a: &CharClassPred) -> CharClassPred {
        let ra = CharClassAlgebra::normalize_u32(a);
        let result = complement_u32_ranges(&ra, 0, (char::MAX as u32) + 1);
        CharClassAlgebra::from_u32_ranges(&result)
    }

    fn is_satisfiable(&self, a: &CharClassPred) -> bool {
        !CharClassAlgebra::normalize_u32(a).is_empty()
    }

    fn witness(&self, a: &CharClassPred) -> Option<char> {
        let ranges = CharClassAlgebra::normalize_u32(a);
        ranges.first().and_then(|&(lo, _)| char::from_u32(lo))
    }

    fn evaluate(&self, pred: &CharClassPred, elem: &char) -> bool {
        let val = *elem as u32;
        let ranges = CharClassAlgebra::normalize_u32(pred);
        ranges.iter().any(|&(lo, hi)| val >= lo && val < hi)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Symbolic Automaton
// ══════════════════════════════════════════════════════════════════════════════

/// A state in a symbolic automaton.
#[derive(Debug, Clone)]
pub struct SymbolicState {
    /// Unique state identifier.
    pub id: usize,
    /// Whether this is an accepting (final) state.
    pub is_accepting: bool,
    /// Optional human-readable label for diagnostics.
    pub label: Option<String>,
}

/// A transition in a symbolic automaton, guarded by a predicate.
///
/// The transition `from --[guard]--> to` fires on input element `e`
/// iff `algebra.evaluate(guard, e)` returns true.
#[derive(Debug, Clone)]
pub struct SymbolicTransition<A: BooleanAlgebra> {
    /// Source state ID.
    pub from: usize,
    /// Target state ID.
    pub to: usize,
    /// Guard predicate: the transition fires when this predicate is satisfied.
    pub guard: A::Predicate,
}

/// A Symbolic Finite Automaton (SFA) parameterized by a Boolean algebra.
///
/// Unlike classical NFAs/DFAs where transitions are labeled with individual
/// symbols, SFA transitions are labeled with predicates from the algebra.
/// This enables modeling automata over infinite alphabets (e.g., all integers,
/// all Unicode characters) compactly.
///
/// # Type Parameter
///
/// - `A`: The Boolean algebra providing predicate operations. Determines
///   both the predicate type (used as transition guards) and the domain
///   type (used for concrete simulation).
#[derive(Debug, Clone)]
pub struct SymbolicAutomaton<A: BooleanAlgebra> {
    /// The Boolean algebra used for guard operations.
    pub algebra: A,
    /// All states in the automaton.
    pub states: Vec<SymbolicState>,
    /// All transitions, each guarded by a predicate.
    pub transitions: Vec<SymbolicTransition<A>>,
    /// Set of initial state IDs.
    pub initial_states: HashSet<usize>,
    /// Set of accepting (final) state IDs.
    pub accepting_states: HashSet<usize>,
}

impl<A: BooleanAlgebra> SymbolicAutomaton<A> {
    /// Create a new empty symbolic automaton.
    pub fn new(algebra: A) -> Self {
        SymbolicAutomaton {
            algebra,
            states: Vec::new(),
            transitions: Vec::new(),
            initial_states: HashSet::new(),
            accepting_states: HashSet::new(),
        }
    }

    /// Add a state and return its ID.
    pub fn add_state(&mut self, is_accepting: bool, label: Option<String>) -> usize {
        let id = self.states.len();
        self.states.push(SymbolicState {
            id,
            is_accepting,
            label,
        });
        if is_accepting {
            self.accepting_states.insert(id);
        }
        id
    }

    /// Mark a state as initial.
    pub fn set_initial(&mut self, state_id: usize) {
        assert!(
            state_id < self.states.len(),
            "State ID {} out of range (have {} states)",
            state_id,
            self.states.len(),
        );
        self.initial_states.insert(state_id);
    }

    /// Add a guarded transition.
    pub fn add_transition(&mut self, from: usize, to: usize, guard: A::Predicate) {
        assert!(
            from < self.states.len() && to < self.states.len(),
            "Transition endpoints ({} -> {}) out of range (have {} states)",
            from,
            to,
            self.states.len(),
        );
        self.transitions
            .push(SymbolicTransition { from, to, guard });
    }

    fn is_valid_state_id(&self, state_id: usize) -> bool {
        state_id < self.states.len()
    }

    fn is_valid_transition(&self, trans: &SymbolicTransition<A>) -> bool {
        self.is_valid_state_id(trans.from) && self.is_valid_state_id(trans.to)
    }

    fn valid_initial_states(&self) -> impl Iterator<Item = usize> + '_ {
        self.initial_states
            .iter()
            .copied()
            .filter(|&state_id| self.is_valid_state_id(state_id))
    }

    fn has_valid_accepting_state(&self) -> bool {
        self.accepting_states
            .iter()
            .any(|&state_id| self.is_valid_state_id(state_id))
    }

    fn valid_outgoing_transitions(&self) -> Vec<Vec<&SymbolicTransition<A>>> {
        let mut outgoing = vec![Vec::new(); self.states.len()];
        for trans in &self.transitions {
            if self.is_valid_transition(trans) {
                outgoing[trans.from].push(trans);
            }
        }
        outgoing
    }

    /// Get the number of states.
    pub fn num_states(&self) -> usize {
        self.states.len()
    }

    /// Get the number of transitions.
    pub fn num_transitions(&self) -> usize {
        self.transitions.len()
    }

    // ── Core algorithms ──────────────────────────────────────────────────

    /// Emptiness check: does the automaton accept any word?
    ///
    /// Uses BFS from initial states, following only transitions whose guards
    /// are satisfiable. The automaton is non-empty iff some accepting state
    /// is reachable via satisfiable transitions.
    ///
    /// # Complexity
    ///
    /// O(|Q| + |delta| * SAT), where SAT is the cost of one satisfiability
    /// check on the algebra.
    pub fn is_empty(&self) -> bool {
        let valid_initials: Vec<usize> = self.valid_initial_states().collect();
        if valid_initials.is_empty() {
            return true;
        }
        if !self.has_valid_accepting_state() {
            return true;
        }
        if valid_initials
            .iter()
            .any(|state| self.accepting_states.contains(state))
        {
            return false;
        }

        // BFS from initial states.
        let outgoing = self.valid_outgoing_transitions();
        let mut visited = vec![false; self.states.len()];
        let mut queue = VecDeque::with_capacity(valid_initials.len());
        for init in valid_initials {
            if !visited[init] {
                visited[init] = true;
                queue.push_back(init);
            }
        }

        while let Some(state) = queue.pop_front() {
            if self.accepting_states.contains(&state) {
                return false; // Found reachable accepting state → non-empty.
            }
            for trans in &outgoing[state] {
                if self.algebra.is_satisfiable(&trans.guard) && !visited[trans.to] {
                    visited[trans.to] = true;
                    queue.push_back(trans.to);
                }
            }
        }

        true // No reachable accepting state → empty.
    }

    /// A shortest concrete word accepted by the automaton, or `None` if the
    /// language is empty.
    ///
    /// BFS from the initial states, materializing one concrete domain element
    /// per edge via `algebra.witness` (which yields `Some` exactly when the
    /// guard is satisfiable). The first accepting state reached gives a
    /// length-minimal accepted word. Used as the `witness` for derived algebras
    /// (e.g. the string algebra) whose predicates compile to an SFA.
    pub fn shortest_accepted(&self) -> Option<Vec<A::Domain>> {
        let valid_initials: Vec<usize> = self.valid_initial_states().collect();
        if valid_initials.is_empty() || !self.has_valid_accepting_state() {
            return None;
        }
        // The empty word is accepted iff some initial state is accepting.
        if valid_initials
            .iter()
            .any(|s| self.accepting_states.contains(s))
        {
            return Some(Vec::new());
        }
        let outgoing = self.valid_outgoing_transitions();
        let mut visited = vec![false; self.states.len()];
        // pred[state] = (predecessor state, the element consumed on the edge)
        let mut pred: Vec<Option<(usize, A::Domain)>> =
            (0..self.states.len()).map(|_| None).collect();
        let mut queue = VecDeque::with_capacity(valid_initials.len());
        for init in valid_initials {
            if !visited[init] {
                visited[init] = true;
                queue.push_back(init);
            }
        }
        while let Some(state) = queue.pop_front() {
            for trans in &outgoing[state] {
                if visited[trans.to] {
                    continue;
                }
                if let Some(elem) = self.algebra.witness(&trans.guard) {
                    visited[trans.to] = true;
                    pred[trans.to] = Some((state, elem));
                    if self.accepting_states.contains(&trans.to) {
                        // Reconstruct the path back to an initial state.
                        let mut word = Vec::new();
                        let mut cur = trans.to;
                        while let Some((prev, elem)) = pred[cur].take() {
                            word.push(elem);
                            cur = prev;
                        }
                        word.reverse();
                        return Some(word);
                    }
                    queue.push_back(trans.to);
                }
            }
        }
        None
    }

    /// Simulate the automaton on a concrete word.
    ///
    /// Returns `true` iff the word is accepted (i.e., after consuming all
    /// elements, at least one current state is accepting).
    ///
    /// This performs NFA-style simulation: it maintains a set of current
    /// states and, for each input element, computes successor states by
    /// evaluating guards.
    ///
    /// # Complexity
    ///
    /// O(|delta| + |w| * |delta_reachable|), where |w| is word length.
    pub fn accepts(&self, word: &[A::Domain]) -> bool {
        let valid_initials: Vec<usize> = self.valid_initial_states().collect();
        if valid_initials.is_empty() {
            return false;
        }

        let mut current: HashSet<usize> = valid_initials.into_iter().collect();
        if word.is_empty() {
            return current
                .iter()
                .any(|state| self.accepting_states.contains(state));
        }

        let outgoing = self.valid_outgoing_transitions();
        for elem in word {
            let mut next = HashSet::new();
            for &state in &current {
                for trans in &outgoing[state] {
                    if self.algebra.evaluate(&trans.guard, elem) {
                        next.insert(trans.to);
                    }
                }
            }
            if next.is_empty() {
                return false;
            }
            current = next;
        }

        current.iter().any(|s| self.accepting_states.contains(s))
    }

    /// Product construction: intersection of two SFAs over the same algebra.
    ///
    /// States are pairs `(q1, q2)` from the two automata. Transitions are
    /// guarded by the conjunction of the corresponding guards. The resulting
    /// automaton accepts exactly the intersection of the two languages.
    ///
    /// # Complexity
    ///
    /// O(|Q1| * |Q2| * |delta1| * |delta2| * AND), where AND is the cost
    /// of one conjunction + satisfiability check on the algebra.
    pub fn intersect(&self, other: &Self) -> Self {
        let mut result = SymbolicAutomaton::new(self.algebra.clone());

        // State mapping: (self_state, other_state) -> result_state_id.
        let mut state_map: HashMap<(usize, usize), usize> = HashMap::new();

        // Create product states.
        for s1 in &self.states {
            for s2 in &other.states {
                let is_accepting = s1.is_accepting && s2.is_accepting;
                let label = match (&s1.label, &s2.label) {
                    (Some(a), Some(b)) => Some(format!("({},{})", a, b)),
                    (Some(a), None) => Some(format!("({},q{})", a, s2.id)),
                    (None, Some(b)) => Some(format!("(q{},{})", s1.id, b)),
                    (None, None) => Some(format!("(q{},q{})", s1.id, s2.id)),
                };
                let id = result.add_state(is_accepting, label);
                state_map.insert((s1.id, s2.id), id);
            }
        }

        // Set initial states.
        for i1 in self.valid_initial_states() {
            for i2 in other.valid_initial_states() {
                if let Some(&pid) = state_map.get(&(i1, i2)) {
                    result.set_initial(pid);
                }
            }
        }

        // Create product transitions with conjunctive guards.
        for t1 in self
            .transitions
            .iter()
            .filter(|t| self.is_valid_transition(t))
        {
            for t2 in other
                .transitions
                .iter()
                .filter(|t| other.is_valid_transition(t))
            {
                let guard = self.algebra.and(&t1.guard, &t2.guard);
                if self.algebra.is_satisfiable(&guard) {
                    if let (Some(&from), Some(&to)) = (
                        state_map.get(&(t1.from, t2.from)),
                        state_map.get(&(t1.to, t2.to)),
                    ) {
                        result.add_transition(from, to, guard);
                    }
                }
            }
        }

        result
    }

    /// Union of two SFAs over the same algebra.
    ///
    /// Builds a single NFA whose state space is the disjoint union of
    /// `self`'s and `other`'s state spaces. Initial states from both
    /// automata are marked initial in the result; accepting states are
    /// preserved. Transitions are renumbered into the combined index
    /// space. The resulting NFA accepts exactly the union of the two
    /// languages.
    ///
    /// # Complexity
    ///
    /// O(|Q1| + |Q2| + |delta1| + |delta2|). Unlike intersection, no
    /// product construction is performed; the union is purely
    /// structural.
    pub fn union(&self, other: &Self) -> Self {
        let mut result = SymbolicAutomaton::new(self.algebra.clone());

        // Copy `self`'s states; record the offset for renumbering.
        let self_offset = 0;
        for state in &self.states {
            let new_label = state
                .label
                .clone()
                .map(|l| format!("L:{}", l))
                .or_else(|| Some(format!("L:q{}", state.id)));
            result.add_state(state.is_accepting, new_label);
        }

        // Copy `other`'s states with an offset.
        let other_offset = self.states.len();
        for state in &other.states {
            let new_label = state
                .label
                .clone()
                .map(|l| format!("R:{}", l))
                .or_else(|| Some(format!("R:q{}", state.id)));
            result.add_state(state.is_accepting, new_label);
        }

        // Mark initial states from both sides.
        for init in self.valid_initial_states() {
            result.set_initial(init + self_offset);
        }
        for init in other.valid_initial_states() {
            result.set_initial(init + other_offset);
        }

        // Copy `self`'s transitions.
        for trans in self
            .transitions
            .iter()
            .filter(|t| self.is_valid_transition(t))
        {
            result.add_transition(
                trans.from + self_offset,
                trans.to + self_offset,
                trans.guard.clone(),
            );
        }

        // Copy `other`'s transitions with renumbering.
        for trans in other
            .transitions
            .iter()
            .filter(|t| other.is_valid_transition(t))
        {
            result.add_transition(
                trans.from + other_offset,
                trans.to + other_offset,
                trans.guard.clone(),
            );
        }

        result
    }

    /// Complement the automaton.
    ///
    /// Determinizes the automaton first (if it is nondeterministic), then
    /// flips accepting and non-accepting states in the deterministic version.
    ///
    /// # Complexity
    ///
    /// Dominated by determinization: worst case O(2^|Q|) states.
    pub fn complement(&self) -> Self {
        let det = self.determinize();

        let mut result = SymbolicAutomaton::new(det.algebra.clone());

        // Copy states with flipped acceptance.
        for state in &det.states {
            result.add_state(!state.is_accepting, state.label.clone());
        }

        // Copy initial states.
        for &init in &det.initial_states {
            result.set_initial(init);
        }

        // Copy transitions.
        for trans in &det.transitions {
            result.add_transition(trans.from, trans.to, trans.guard.clone());
        }

        // Add a dead/sink state for completeness — any input not matched
        // by existing transitions goes to the sink (which is accepting in
        // the complement).
        let sink = result.add_state(true, Some("sink".to_string()));

        // For each state, compute the disjunction of all outgoing guards.
        // The complement of that disjunction covers inputs with no transition.
        let num_states = det.states.len();
        for state_id in 0..num_states {
            let outgoing_guards: Vec<&A::Predicate> = det
                .transitions
                .iter()
                .filter(|t| t.from == state_id)
                .map(|t| &t.guard)
                .collect();

            let covered = if outgoing_guards.is_empty() {
                self.algebra.false_pred()
            } else {
                let mut disj = outgoing_guards[0].clone();
                for g in &outgoing_guards[1..] {
                    disj = self.algebra.or(&disj, g);
                }
                disj
            };

            let uncovered = self.algebra.not(&covered);
            if self.algebra.is_satisfiable(&uncovered) {
                result.add_transition(state_id, sink, uncovered);
            }
        }

        // Sink state loops to itself on all inputs.
        result.add_transition(sink, sink, self.algebra.true_pred());

        result
    }

    /// Determinize the automaton using minterm-based subset construction.
    ///
    /// ## Algorithm
    ///
    /// 1. For each subset state `S` (set of NFA states), collect all guard
    ///    predicates on outgoing transitions.
    /// 2. Compute minterms: maximal satisfiable conjunctions of predicates
    ///    and their negations. Minterms partition the domain into equivalence
    ///    classes where all elements trigger exactly the same transitions.
    /// 3. For each minterm, compute the successor subset state by collecting
    ///    all targets of transitions whose guards overlap with the minterm.
    /// 4. Continue until no new subset states are discovered.
    ///
    /// ## Complexity
    ///
    /// Worst case O(2^|Q|) subset states, each with up to 2^k minterms
    /// (k = number of distinct predicates on outgoing transitions).
    /// In practice, far fewer states and minterms are generated.
    pub fn determinize(&self) -> Self {
        let mut result = SymbolicAutomaton::new(self.algebra.clone());

        // Subset state mapping: sorted set of NFA state IDs -> DFA state ID.
        let mut state_map: HashMap<BTreeSet<usize>, usize> = HashMap::new();
        let mut worklist: VecDeque<BTreeSet<usize>> = VecDeque::new();

        // Initial subset state.
        let initial_set: BTreeSet<usize> = self.valid_initial_states().collect();
        if initial_set.is_empty() {
            // No initial states → empty automaton.
            let q0 = result.add_state(false, Some("empty".to_string()));
            result.set_initial(q0);
            return result;
        }

        let is_accepting = initial_set
            .iter()
            .any(|s| self.accepting_states.contains(s));
        let dfa_id = result.add_state(is_accepting, Some(format!("{:?}", initial_set)));
        result.set_initial(dfa_id);
        state_map.insert(initial_set.clone(), dfa_id);
        worklist.push_back(initial_set);

        // Pre-build outgoing transition index by valid source state.
        let outgoing = self.valid_outgoing_transitions();

        while let Some(current_set) = worklist.pop_front() {
            // Collect all guard predicates on outgoing transitions from this subset.
            let mut all_guards: Vec<A::Predicate> = Vec::new();
            for &nfa_state in &current_set {
                for trans in &outgoing[nfa_state] {
                    all_guards.push(trans.guard.clone());
                }
            }

            if all_guards.is_empty() {
                continue; // Dead end — no outgoing transitions.
            }

            // Compute minterms from the guard predicates.
            let minterms = compute_minterms(&self.algebra, &all_guards);

            // For each satisfiable minterm, compute the successor subset state.
            for minterm in &minterms {
                let mut successor_set = BTreeSet::new();

                for &nfa_state in &current_set {
                    for trans in &outgoing[nfa_state] {
                        // Does this minterm overlap with the guard?
                        if self.algebra.overlaps(minterm, &trans.guard) {
                            successor_set.insert(trans.to);
                        }
                    }
                }

                if successor_set.is_empty() {
                    continue;
                }

                let succ_id = if let Some(&existing) = state_map.get(&successor_set) {
                    existing
                } else {
                    let is_acc = successor_set
                        .iter()
                        .any(|s| self.accepting_states.contains(s));
                    let new_id = result.add_state(is_acc, Some(format!("{:?}", successor_set)));
                    state_map.insert(successor_set.clone(), new_id);
                    worklist.push_back(successor_set);
                    new_id
                };

                let from_id = state_map[&current_set];
                result.add_transition(from_id, succ_id, minterm.clone());
            }
        }

        result
    }

    /// Equivalence check: do two SFAs accept the same language?
    ///
    /// Reduces to emptiness of the symmetric difference:
    /// `L(A) = L(B)` iff `(L(A) ∩ L(B)^c) ∪ (L(A)^c ∩ L(B))` is empty.
    ///
    /// # Complexity
    ///
    /// Dominated by complement (determinization) and intersection.
    pub fn is_equivalent(&self, other: &Self) -> bool {
        // A \ B = A ∩ B^c
        let b_complement = other.complement();
        let a_minus_b = self.intersect(&b_complement);

        if !a_minus_b.is_empty() {
            return false;
        }

        // B \ A = B ∩ A^c
        let a_complement = self.complement();
        let b_minus_a = other.intersect(&a_complement);

        b_minus_a.is_empty()
    }

    /// Analyze the automaton for pipeline diagnostics.
    ///
    /// Produces a `SymbolicAnalysis` summarizing:
    /// - State and transition counts
    /// - Guard satisfiability for each transition
    /// - Pairs of guards that overlap (non-disjoint)
    /// - Pairs where one guard subsumes (implies) another
    pub fn analyze(&self) -> SymbolicAnalysis {
        let mut guard_satisfiability = Vec::with_capacity(self.transitions.len());
        let mut overlapping_guards = Vec::new();
        let mut subsumed_guards = Vec::new();

        // Check satisfiability of each guard.
        for (i, trans) in self.transitions.iter().enumerate() {
            let desc = format!("q{} --[{:?}]--> q{}", trans.from, trans.guard, trans.to,);
            let sat = self.algebra.is_satisfiable(&trans.guard);
            guard_satisfiability.push((desc.clone(), sat));

            // Check overlap and subsumption against all subsequent transitions.
            for (_j, other_trans) in self.transitions.iter().enumerate().skip(i + 1) {
                let desc_j = format!(
                    "q{} --[{:?}]--> q{}",
                    other_trans.from, other_trans.guard, other_trans.to,
                );

                // Overlap check.
                if self.algebra.overlaps(&trans.guard, &other_trans.guard) {
                    overlapping_guards.push((desc.clone(), desc_j.clone()));
                }

                // Subsumption checks (both directions).
                if self.algebra.implies(&trans.guard, &other_trans.guard) {
                    subsumed_guards.push((desc.clone(), desc_j.clone()));
                }
                if self.algebra.implies(&other_trans.guard, &trans.guard) {
                    subsumed_guards.push((desc_j, desc.clone()));
                }
            }
        }

        let unsatisfiable_rule_labels: Vec<String> = guard_satisfiability
            .iter()
            .filter(|(_, sat)| !sat)
            .map(|(desc, _)| desc.clone())
            .collect();
        SymbolicAnalysis {
            num_states: self.states.len(),
            num_transitions: self.transitions.len(),
            guard_satisfiability,
            overlapping_guards,
            subsumed_guards,
            unsatisfiable_rule_labels,
        }
    }
}

impl<A: BooleanAlgebra> fmt::Display for SymbolicAutomaton<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "SymbolicAutomaton ({} states, {} transitions)",
            self.states.len(),
            self.transitions.len()
        )?;
        writeln!(f, "  Initial: {:?}", self.initial_states)?;
        writeln!(f, "  Accepting: {:?}", self.accepting_states)?;
        for trans in &self.transitions {
            writeln!(
                f,
                "  q{} --[{:?}]--> q{}",
                trans.from, trans.guard, trans.to,
            )?;
        }
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Minterm computation
// ══════════════════════════════════════════════════════════════════════════════

/// Compute minterms from a set of predicates.
///
/// A minterm is a maximal satisfiable conjunction of predicates and their
/// negations. Given predicates {phi_1, ..., phi_k}, a minterm is:
///   (+/-)phi_1 AND (+/-)phi_2 AND ... AND (+/-)phi_k
/// where (+) means the predicate itself and (-) means its negation, and the
/// resulting conjunction is satisfiable.
///
/// Minterms partition the domain into equivalence classes: all elements in
/// the same minterm are treated identically by every predicate.
///
/// # Algorithm
///
/// Iteratively refine a set of satisfiable predicates by splitting each
/// against each guard predicate. Start with {TRUE}, then for each guard phi:
/// - Replace each current predicate psi with psi AND phi and psi AND NOT phi
/// - Keep only satisfiable results
///
/// # Complexity
///
/// Worst case O(2^k) minterms for k predicates, but in practice many
/// conjunctions are unsatisfiable and are pruned.
pub fn compute_minterms<A: BooleanAlgebra>(
    algebra: &A,
    predicates: &[A::Predicate],
) -> Vec<A::Predicate> {
    // Deduplicate predicates.
    let unique_preds: Vec<&A::Predicate> = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for p in predicates {
            if seen.insert(p.clone()) {
                result.push(p);
            }
        }
        result
    };

    let mut minterms = vec![algebra.true_pred()];

    for pred in &unique_preds {
        let mut new_minterms = Vec::with_capacity(minterms.len() * 2);
        let neg = algebra.not(pred);

        for minterm in &minterms {
            // Split: minterm AND pred
            let pos = algebra.and(minterm, pred);
            if algebra.is_satisfiable(&pos) {
                new_minterms.push(pos);
            }

            // Split: minterm AND NOT pred
            let neg_part = algebra.and(minterm, &neg);
            if algebra.is_satisfiable(&neg_part) {
                new_minterms.push(neg_part);
            }
        }

        minterms = new_minterms;
    }

    minterms
}

// ══════════════════════════════════════════════════════════════════════════════
// Decidability Classifier
// ══════════════════════════════════════════════════════════════════════════════

/// Classification of decidability for predicate expressions.
///
/// Following the standard computability hierarchy:
/// - T1 (compile-time decidable): purely propositional, finite-domain quantification
/// - T2 (runtime decidable): involves Ascent relation lookups (finite but dynamic)
/// - T3 (semi-decidable): bounded infinite-domain quantification
/// - T4 (undecidable): unbounded infinite-domain quantification
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DecidabilityTier {
    /// T1: Compile-time decidable (structural, finite-domain).
    ///
    /// Expressions containing only True/False/Atom/Not/And/Or and
    /// ForallFinite/ExistsFinite with finite domains. All quantification
    /// is bounded over enumerable sets; all atoms are ground-decidable.
    CompileTimeDecidable,

    /// T2: Runtime decidable (Ascent queries, finite-state checks).
    ///
    /// Expressions containing `Relation` atoms that reference Ascent
    /// database relations. Decidable at runtime when the Ascent database
    /// is populated, but not at compile time.
    RuntimeDecidable,

    /// T3: Semi-decidable (bounded checking with depth limit).
    ///
    /// Expressions containing `ForallInfinite`/`ExistsInfinite` quantifiers,
    /// but wrapped in a `Bounded` node with an explicit depth limit. The
    /// checker explores up to the bound; correctness is guaranteed only
    /// within the bound.
    SemiDecidable,

    /// T4: Undecidable (requires user proof/assertion).
    ///
    /// Expressions containing unbounded `ForallInfinite`/`ExistsInfinite`
    /// quantifiers without a `Bounded` wrapper. No algorithm can decide
    /// these in general; they require manual proof or axiom assertion.
    Undecidable,
}

impl fmt::Display for DecidabilityTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecidabilityTier::CompileTimeDecidable => write!(f, "T1 (compile-time decidable)"),
            DecidabilityTier::RuntimeDecidable => write!(f, "T2 (runtime decidable)"),
            DecidabilityTier::SemiDecidable => write!(f, "T3 (semi-decidable)"),
            DecidabilityTier::Undecidable => write!(f, "T4 (undecidable)"),
        }
    }
}

/// A predicate expression for decidability classification.
///
/// This is a richer predicate language than `BooleanTest`, supporting
/// quantification (both finite and infinite-domain), relational atoms
/// (database lookups), and bounded checking.
pub enum PredicateExpr {
    /// Boolean true.
    True,
    /// Boolean false.
    False,
    /// Atomic proposition (ground-decidable at compile time).
    Atom(String),
    /// Logical negation.
    Not(Box<PredicateExpr>),
    /// Logical conjunction.
    And(Box<PredicateExpr>, Box<PredicateExpr>),
    /// Logical disjunction.
    Or(Box<PredicateExpr>, Box<PredicateExpr>),
    /// Universal quantification over a finite domain.
    /// Decidable by enumeration: check body for each element.
    ForallFinite {
        /// Bound variable name.
        var: String,
        /// Finite domain to quantify over.
        domain: Vec<String>,
        /// Body predicate (may reference `var`).
        body: Box<PredicateExpr>,
    },
    /// Existential quantification over a finite domain.
    /// Decidable by enumeration: find an element satisfying body.
    ExistsFinite {
        /// Bound variable name.
        var: String,
        /// Finite domain to quantify over.
        domain: Vec<String>,
        /// Body predicate (may reference `var`).
        body: Box<PredicateExpr>,
    },
    /// Universal quantification over an infinite domain.
    /// Undecidable in general; semi-decidable when wrapped in `Bounded`.
    ForallInfinite {
        /// Bound variable name.
        var: String,
        /// Body predicate (may reference `var`).
        body: Box<PredicateExpr>,
    },
    /// Existential quantification over an infinite domain.
    /// Undecidable in general; semi-decidable when wrapped in `Bounded`.
    ExistsInfinite {
        /// Bound variable name.
        var: String,
        /// Body predicate (may reference `var`).
        body: Box<PredicateExpr>,
    },
    /// Relational atom: references an Ascent database relation.
    /// Decidable at runtime (T2) when the relation is populated.
    Relation {
        /// Relation name in the Ascent database.
        name: String,
        /// Arguments (column names or values).
        args: Vec<String>,
    },
    /// Bounded checking wrapper: limits exploration depth for
    /// infinite-domain quantification, making it semi-decidable (T3).
    Bounded {
        /// The body expression being bounded.
        body: Box<PredicateExpr>,
        /// Maximum exploration depth/count.
        bound: u64,
    },
}

impl Clone for PredicateExpr {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a PredicateExpr),
            Not,
            And,
            Or,
            ForallFinite(&'a str, &'a [String]),
            ExistsFinite(&'a str, &'a [String]),
            ForallInfinite(&'a str),
            ExistsInfinite(&'a str),
            Bounded(u64),
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(expression) => match expression {
                    PredicateExpr::True => values.push(PredicateExpr::True),
                    PredicateExpr::False => values.push(PredicateExpr::False),
                    PredicateExpr::Atom(name) => values.push(PredicateExpr::Atom(name.clone())),
                    PredicateExpr::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                    PredicateExpr::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    PredicateExpr::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    PredicateExpr::ForallFinite { var, domain, body } => {
                        tasks.push(Task::ForallFinite(var, domain));
                        tasks.push(Task::Clone(body));
                    }
                    PredicateExpr::ExistsFinite { var, domain, body } => {
                        tasks.push(Task::ExistsFinite(var, domain));
                        tasks.push(Task::Clone(body));
                    }
                    PredicateExpr::ForallInfinite { var, body } => {
                        tasks.push(Task::ForallInfinite(var));
                        tasks.push(Task::Clone(body));
                    }
                    PredicateExpr::ExistsInfinite { var, body } => {
                        tasks.push(Task::ExistsInfinite(var));
                        tasks.push(Task::Clone(body));
                    }
                    PredicateExpr::Relation { name, args } => {
                        values.push(PredicateExpr::Relation {
                            name: name.clone(),
                            args: args.clone(),
                        });
                    }
                    PredicateExpr::Bounded { body, bound } => {
                        tasks.push(Task::Bounded(*bound));
                        tasks.push(Task::Clone(body));
                    }
                },
                Task::Not => {
                    let inner = values.pop().expect("negated expression clone is present");
                    values.push(PredicateExpr::Not(Box::new(inner)));
                }
                Task::And | Task::Or => {
                    let right = values.pop().expect("right expression clone is present");
                    let left = values.pop().expect("left expression clone is present");
                    values.push(if matches!(task, Task::And) {
                        PredicateExpr::And(Box::new(left), Box::new(right))
                    } else {
                        PredicateExpr::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::ForallFinite(var, domain) | Task::ExistsFinite(var, domain) => {
                    let body = values
                        .pop()
                        .expect("finite quantifier body clone is present");
                    values.push(if matches!(task, Task::ForallFinite(_, _)) {
                        PredicateExpr::ForallFinite {
                            var: var.to_string(),
                            domain: domain.to_vec(),
                            body: Box::new(body),
                        }
                    } else {
                        PredicateExpr::ExistsFinite {
                            var: var.to_string(),
                            domain: domain.to_vec(),
                            body: Box::new(body),
                        }
                    });
                }
                Task::ForallInfinite(var) | Task::ExistsInfinite(var) => {
                    let body = values
                        .pop()
                        .expect("infinite quantifier body clone is present");
                    values.push(if matches!(task, Task::ForallInfinite(_)) {
                        PredicateExpr::ForallInfinite {
                            var: var.to_string(),
                            body: Box::new(body),
                        }
                    } else {
                        PredicateExpr::ExistsInfinite {
                            var: var.to_string(),
                            body: Box::new(body),
                        }
                    });
                }
                Task::Bounded(bound) => {
                    let body = values.pop().expect("bounded body clone is present");
                    values.push(PredicateExpr::Bounded {
                        body: Box::new(body),
                        bound,
                    });
                }
            }
        }
        values
            .pop()
            .expect("the root expression produces one clone")
    }
}

impl PartialEq for PredicateExpr {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (PredicateExpr::True, PredicateExpr::True)
                | (PredicateExpr::False, PredicateExpr::False) => {}
                (PredicateExpr::Atom(left), PredicateExpr::Atom(right)) if left == right => {}
                (PredicateExpr::Not(left), PredicateExpr::Not(right)) => {
                    pending.push((left, right));
                }
                (PredicateExpr::And(ll, lr), PredicateExpr::And(rl, rr))
                | (PredicateExpr::Or(ll, lr), PredicateExpr::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (
                    PredicateExpr::ForallFinite {
                        var: lv,
                        domain: ld,
                        body: lb,
                    },
                    PredicateExpr::ForallFinite {
                        var: rv,
                        domain: rd,
                        body: rb,
                    },
                )
                | (
                    PredicateExpr::ExistsFinite {
                        var: lv,
                        domain: ld,
                        body: lb,
                    },
                    PredicateExpr::ExistsFinite {
                        var: rv,
                        domain: rd,
                        body: rb,
                    },
                ) if lv == rv && ld == rd => pending.push((lb, rb)),
                (
                    PredicateExpr::ForallInfinite { var: lv, body: lb },
                    PredicateExpr::ForallInfinite { var: rv, body: rb },
                )
                | (
                    PredicateExpr::ExistsInfinite { var: lv, body: lb },
                    PredicateExpr::ExistsInfinite { var: rv, body: rb },
                ) if lv == rv => pending.push((lb, rb)),
                (
                    PredicateExpr::Relation { name: ln, args: la },
                    PredicateExpr::Relation { name: rn, args: ra },
                ) if ln == rn && la == ra => {}
                (
                    PredicateExpr::Bounded {
                        body: lb,
                        bound: ll,
                    },
                    PredicateExpr::Bounded {
                        body: rb,
                        bound: rl,
                    },
                ) if ll == rl => pending.push((lb, rb)),
                _ => return false,
            }
        }
        true
    }
}

impl Eq for PredicateExpr {}

impl Hash for PredicateExpr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        enum Event<'a> {
            Expr(&'a PredicateExpr),
            Bound(u64),
        }
        let mut pending = vec![Event::Expr(self)];
        while let Some(event) = pending.pop() {
            match event {
                Event::Bound(bound) => bound.hash(state),
                Event::Expr(expression) => {
                    std::mem::discriminant(expression).hash(state);
                    match expression {
                        PredicateExpr::True | PredicateExpr::False => {}
                        PredicateExpr::Atom(name) => name.hash(state),
                        PredicateExpr::Not(inner) => pending.push(Event::Expr(inner)),
                        PredicateExpr::And(left, right) | PredicateExpr::Or(left, right) => {
                            pending.push(Event::Expr(right));
                            pending.push(Event::Expr(left));
                        }
                        PredicateExpr::ForallFinite { var, domain, body }
                        | PredicateExpr::ExistsFinite { var, domain, body } => {
                            var.hash(state);
                            domain.hash(state);
                            pending.push(Event::Expr(body));
                        }
                        PredicateExpr::ForallInfinite { var, body }
                        | PredicateExpr::ExistsInfinite { var, body } => {
                            var.hash(state);
                            pending.push(Event::Expr(body));
                        }
                        PredicateExpr::Relation { name, args } => {
                            name.hash(state);
                            args.hash(state);
                        }
                        PredicateExpr::Bounded { body, bound } => {
                            pending.push(Event::Bound(*bound));
                            pending.push(Event::Expr(body));
                        }
                    }
                }
            }
        }
    }
}

impl fmt::Debug for PredicateExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Expr(&'a PredicateExpr),
            Text(&'static str),
            Bound(u64),
        }
        let mut events = vec![Event::Expr(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Bound(bound) => write!(f, "{bound:?}")?,
                Event::Expr(expression) => match expression {
                    PredicateExpr::True => write!(f, "True")?,
                    PredicateExpr::False => write!(f, "False")?,
                    PredicateExpr::Atom(name) => write!(f, "Atom({name:?})")?,
                    PredicateExpr::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Expr(inner));
                    }
                    PredicateExpr::And(left, right) | PredicateExpr::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(expression, PredicateExpr::And(_, _)) {
                                "And"
                            } else {
                                "Or"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Expr(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Expr(left));
                    }
                    PredicateExpr::ForallFinite { var, domain, body }
                    | PredicateExpr::ExistsFinite { var, domain, body } => {
                        write!(
                            f,
                            "{} {{ var: {var:?}, domain: {domain:?}, body: ",
                            if matches!(expression, PredicateExpr::ForallFinite { .. }) {
                                "ForallFinite"
                            } else {
                                "ExistsFinite"
                            }
                        )?;
                        events.push(Event::Text(" }"));
                        events.push(Event::Expr(body));
                    }
                    PredicateExpr::ForallInfinite { var, body }
                    | PredicateExpr::ExistsInfinite { var, body } => {
                        write!(
                            f,
                            "{} {{ var: {var:?}, body: ",
                            if matches!(expression, PredicateExpr::ForallInfinite { .. }) {
                                "ForallInfinite"
                            } else {
                                "ExistsInfinite"
                            }
                        )?;
                        events.push(Event::Text(" }"));
                        events.push(Event::Expr(body));
                    }
                    PredicateExpr::Relation { name, args } => {
                        write!(f, "Relation {{ name: {name:?}, args: {args:?} }}")?;
                    }
                    PredicateExpr::Bounded { body, bound } => {
                        write!(f, "Bounded {{ body: ")?;
                        events.push(Event::Text(" }"));
                        events.push(Event::Bound(*bound));
                        events.push(Event::Text(", bound: "));
                        events.push(Event::Expr(body));
                    }
                },
            }
        }
        Ok(())
    }
}

impl fmt::Display for PredicateExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Expr(&'a PredicateExpr),
            Text(&'static str),
            Bound(u64),
        }
        let mut events = vec![Event::Expr(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Bound(bound) => write!(f, ", {bound}")?,
                Event::Expr(expression) => match expression {
                    PredicateExpr::True => write!(f, "true")?,
                    PredicateExpr::False => write!(f, "false")?,
                    PredicateExpr::Atom(name) => write!(f, "{name}")?,
                    PredicateExpr::Not(inner) => {
                        write!(f, "~(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Expr(inner));
                    }
                    PredicateExpr::And(left, right) | PredicateExpr::Or(left, right) => {
                        write!(f, "(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Expr(right));
                        events.push(Event::Text(
                            if matches!(expression, PredicateExpr::And(_, _)) {
                                " /\\ "
                            } else {
                                " \\/ "
                            },
                        ));
                        events.push(Event::Expr(left));
                    }
                    PredicateExpr::ForallFinite { var, domain, body }
                    | PredicateExpr::ExistsFinite { var, domain, body } => {
                        write!(
                            f,
                            "{} {var} in {domain:?}. ",
                            if matches!(expression, PredicateExpr::ForallFinite { .. }) {
                                "forall"
                            } else {
                                "exists"
                            }
                        )?;
                        events.push(Event::Expr(body));
                    }
                    PredicateExpr::ForallInfinite { var, body }
                    | PredicateExpr::ExistsInfinite { var, body } => {
                        write!(
                            f,
                            "{} {var}. ",
                            if matches!(expression, PredicateExpr::ForallInfinite { .. }) {
                                "forall"
                            } else {
                                "exists"
                            }
                        )?;
                        events.push(Event::Expr(body));
                    }
                    PredicateExpr::Relation { name, args } => {
                        write!(f, "{}({})", name, args.join(", "))?;
                    }
                    PredicateExpr::Bounded { body, bound } => {
                        write!(f, "bounded(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Bound(*bound));
                        events.push(Event::Expr(body));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for PredicateExpr {
    fn drop(&mut self) {
        fn drain(expression: &mut PredicateExpr, pending: &mut Vec<PredicateExpr>) {
            match expression {
                PredicateExpr::Not(inner)
                | PredicateExpr::ForallFinite { body: inner, .. }
                | PredicateExpr::ExistsFinite { body: inner, .. }
                | PredicateExpr::ForallInfinite { body: inner, .. }
                | PredicateExpr::ExistsInfinite { body: inner, .. }
                | PredicateExpr::Bounded { body: inner, .. } => {
                    pending.push(std::mem::replace(&mut **inner, PredicateExpr::True));
                }
                PredicateExpr::And(left, right) | PredicateExpr::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, PredicateExpr::True));
                    pending.push(std::mem::replace(&mut **left, PredicateExpr::True));
                }
                PredicateExpr::True
                | PredicateExpr::False
                | PredicateExpr::Atom(_)
                | PredicateExpr::Relation { .. } => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut expression) = pending.pop() {
            drain(&mut expression, &mut pending);
        }
    }
}

/// Classify the decidability tier of a predicate expression.
///
/// The classification follows the computability hierarchy:
///
/// - **T1 (CompileTimeDecidable)**: Only propositional connectives (True, False,
///   Atom, Not, And, Or) and finite-domain quantifiers (ForallFinite, ExistsFinite).
///   All atoms are ground-decidable and all domains are enumerable.
///
/// - **T2 (RuntimeDecidable)**: Contains `Relation` atoms referencing Ascent
///   database relations. Decidable when the database is populated, but not
///   at compile time.
///
/// - **T3 (SemiDecidable)**: Contains `ForallInfinite`/`ExistsInfinite` quantifiers,
///   but all such quantifiers are wrapped in `Bounded` nodes. The checker
///   explores up to the bound, making the check semi-decidable.
///
/// - **T4 (Undecidable)**: Contains unbounded `ForallInfinite`/`ExistsInfinite`
///   quantifiers. No algorithm can decide these in general.
///
/// The function returns the highest (least decidable) tier found anywhere
/// in the expression tree. A sub-expression of higher tier dominates.
pub fn classify_decidability(expr: &PredicateExpr) -> DecidabilityTier {
    classify_decidability_inner(expr, false)
}

/// Internal recursive classifier.
///
/// `in_bounded` tracks whether we are inside a `Bounded` wrapper,
/// which downgrades infinite quantifiers from T4 to T3.
fn classify_decidability_inner(expr: &PredicateExpr, in_bounded: bool) -> DecidabilityTier {
    enum Frame<'a> {
        Visit(&'a PredicateExpr, bool),
        BinaryRight(&'a PredicateExpr, bool),
        BinaryFinish(DecidabilityTier),
        Floor(DecidabilityTier),
    }
    let mut frames = vec![Frame::Visit(expr, in_bounded)];
    let mut result = None;
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Visit(expression, bounded) => match expression {
                PredicateExpr::True | PredicateExpr::False | PredicateExpr::Atom(_) => {
                    result = Some(DecidabilityTier::CompileTimeDecidable);
                }
                PredicateExpr::Relation { .. } => {
                    result = Some(DecidabilityTier::RuntimeDecidable);
                }
                PredicateExpr::Not(inner)
                | PredicateExpr::ForallFinite { body: inner, .. }
                | PredicateExpr::ExistsFinite { body: inner, .. } => {
                    frames.push(Frame::Visit(inner, bounded));
                }
                PredicateExpr::And(left, right) | PredicateExpr::Or(left, right) => {
                    frames.push(Frame::BinaryRight(right, bounded));
                    frames.push(Frame::Visit(left, bounded));
                }
                PredicateExpr::ForallInfinite { body, .. }
                | PredicateExpr::ExistsInfinite { body, .. } => {
                    if bounded {
                        frames.push(Frame::Floor(DecidabilityTier::SemiDecidable));
                        frames.push(Frame::Visit(body, true));
                    } else {
                        result = Some(DecidabilityTier::Undecidable);
                    }
                }
                PredicateExpr::Bounded { body, .. } => {
                    frames.push(Frame::Visit(body, true));
                }
            },
            Frame::BinaryRight(right, bounded) => {
                let left = result.take().expect("left decidability tier is available");
                frames.push(Frame::BinaryFinish(left));
                frames.push(Frame::Visit(right, bounded));
            }
            Frame::BinaryFinish(left) => {
                let right = result.take().expect("right decidability tier is available");
                result = Some(left.max(right));
            }
            Frame::Floor(floor) => {
                let body = result
                    .take()
                    .expect("bounded infinite body tier is available");
                result = Some(body.max(floor));
            }
        }
    }
    result.expect("the root expression produces one decidability tier")
}

// ══════════════════════════════════════════════════════════════════════════════
// Pipeline Analysis Result
// ══════════════════════════════════════════════════════════════════════════════

/// Pipeline-level symbolic automaton analysis results.
///
/// Captures guard analysis data for lint diagnostics:
/// - Which guards are satisfiable (unsatisfiable guards indicate dead transitions)
/// - Which guard pairs overlap (potential ambiguity)
/// - Which guards subsume others (redundancy opportunities)
#[derive(Debug, Clone)]
pub struct SymbolicAnalysis {
    /// Number of states in the analyzed automaton.
    pub num_states: usize,
    /// Number of transitions in the analyzed automaton.
    pub num_transitions: usize,
    /// Per-transition guard satisfiability: `(guard_description, is_satisfiable)`.
    pub guard_satisfiability: Vec<(String, bool)>,
    /// Pairs of transitions with overlapping (non-disjoint) guards.
    /// Each entry is `(guard_desc_1, guard_desc_2)`.
    pub overlapping_guards: Vec<(String, String)>,
    /// Pairs where one guard subsumes (implies) another.
    /// Each entry is `(subsumed_guard_desc, subsumer_guard_desc)`.
    pub subsumed_guards: Vec<(String, String)>,
    /// Rule labels whose guards are provably unsatisfiable (dead rules).
    /// Populated from `guard_satisfiability` entries where `is_satisfiable == false`.
    /// Used by codegen to extend dead-code elimination (SYM01-DCE).
    pub unsatisfiable_rule_labels: Vec<String>,
}

impl fmt::Display for SymbolicAnalysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "SymbolicAnalysis: {} states, {} transitions",
            self.num_states, self.num_transitions
        )?;
        writeln!(f, "  Guard satisfiability:")?;
        for (desc, sat) in &self.guard_satisfiability {
            writeln!(f, "    {} : {}", desc, if *sat { "SAT" } else { "UNSAT" })?;
        }
        if !self.overlapping_guards.is_empty() {
            writeln!(f, "  Overlapping guards:")?;
            for (a, b) in &self.overlapping_guards {
                writeln!(f, "    {} <-> {}", a, b)?;
            }
        }
        if !self.subsumed_guards.is_empty() {
            writeln!(f, "  Subsumed guards:")?;
            for (sub, sup) in &self.subsumed_guards {
                writeln!(f, "    {} <= {}", sub, sup)?;
            }
        }
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// ProductAlgebra — Cartesian product of two Boolean algebras
// ══════════════════════════════════════════════════════════════════════════════

/// A predicate over the Cartesian product of two Boolean algebras.
///
/// Represents Boolean combinations of predicates from algebras `A` and `B`.
/// The domain is the pair `(A::Domain, B::Domain)`, and satisfiability requires
/// both components to be satisfiable (independent domains).
pub enum ProductPred<A: BooleanAlgebra, B: BooleanAlgebra> {
    /// Always true.
    True,
    /// Always false.
    False,
    /// Both left and right predicates must be satisfied.
    Both(A::Predicate, B::Predicate),
    /// Only left predicate constrained (right is implicitly True).
    LeftOnly(A::Predicate),
    /// Only right predicate constrained (left is implicitly True).
    RightOnly(B::Predicate),
    /// Conjunction of two product predicates.
    And(Box<ProductPred<A, B>>, Box<ProductPred<A, B>>),
    /// Disjunction of two product predicates.
    Or(Box<ProductPred<A, B>>, Box<ProductPred<A, B>>),
    /// Negation of a product predicate.
    Not(Box<ProductPred<A, B>>),
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> Clone for ProductPred<A, B> {
    fn clone(&self) -> Self {
        enum Task<'a, A: BooleanAlgebra, B: BooleanAlgebra> {
            Clone(&'a ProductPred<A, B>),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    ProductPred::True => values.push(ProductPred::True),
                    ProductPred::False => values.push(ProductPred::False),
                    ProductPred::Both(left, right) => {
                        values.push(ProductPred::Both(left.clone(), right.clone()));
                    }
                    ProductPred::LeftOnly(left) => {
                        values.push(ProductPred::LeftOnly(left.clone()));
                    }
                    ProductPred::RightOnly(right) => {
                        values.push(ProductPred::RightOnly(right.clone()));
                    }
                    ProductPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    ProductPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    ProductPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right product clone is present");
                    let left = values.pop().expect("left product clone is present");
                    values.push(if matches!(task, Task::And) {
                        ProductPred::And(Box::new(left), Box::new(right))
                    } else {
                        ProductPred::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated product clone is present");
                    values.push(ProductPred::Not(Box::new(inner)));
                }
            }
        }
        values.pop().expect("the root product produces one clone")
    }
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> PartialEq for ProductPred<A, B> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (ProductPred::True, ProductPred::True)
                | (ProductPred::False, ProductPred::False) => {}
                (ProductPred::Both(la, lb), ProductPred::Both(ra, rb)) if la == ra && lb == rb => {}
                (ProductPred::LeftOnly(left), ProductPred::LeftOnly(right)) if left == right => {}
                (ProductPred::RightOnly(left), ProductPred::RightOnly(right)) if left == right => {}
                (ProductPred::And(ll, lr), ProductPred::And(rl, rr))
                | (ProductPred::Or(ll, lr), ProductPred::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (ProductPred::Not(left), ProductPred::Not(right)) => {
                    pending.push((left, right));
                }
                _ => return false,
            }
        }
        true
    }
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> Eq for ProductPred<A, B> {}

impl<A: BooleanAlgebra, B: BooleanAlgebra> std::hash::Hash for ProductPred<A, B> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                ProductPred::True | ProductPred::False => {}
                ProductPred::Both(left, right) => {
                    left.hash(state);
                    right.hash(state);
                }
                ProductPred::LeftOnly(left) => left.hash(state),
                ProductPred::RightOnly(right) => right.hash(state),
                ProductPred::And(left, right) | ProductPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                ProductPred::Not(inner) => pending.push(inner),
            }
        }
    }
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> fmt::Debug for ProductPred<A, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, A: BooleanAlgebra, B: BooleanAlgebra> {
            Pred(&'a ProductPred<A, B>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    ProductPred::True => write!(f, "True")?,
                    ProductPred::False => write!(f, "False")?,
                    ProductPred::Both(left, right) => write!(f, "Both({left:?}, {right:?})")?,
                    ProductPred::LeftOnly(left) => write!(f, "LeftOnly({left:?})")?,
                    ProductPred::RightOnly(right) => write!(f, "RightOnly({right:?})")?,
                    ProductPred::And(left, right) | ProductPred::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, ProductPred::And(_, _)) {
                                "And"
                            } else {
                                "Or"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Pred(left));
                    }
                    ProductPred::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> Drop for ProductPred<A, B> {
    fn drop(&mut self) {
        fn drain<A: BooleanAlgebra, B: BooleanAlgebra>(
            predicate: &mut ProductPred<A, B>,
            pending: &mut Vec<ProductPred<A, B>>,
        ) {
            match predicate {
                ProductPred::And(left, right) | ProductPred::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, ProductPred::True));
                    pending.push(std::mem::replace(&mut **left, ProductPred::True));
                }
                ProductPred::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, ProductPred::True));
                }
                ProductPred::True
                | ProductPred::False
                | ProductPred::Both(_, _)
                | ProductPred::LeftOnly(_)
                | ProductPred::RightOnly(_) => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain(&mut predicate, &mut pending);
        }
    }
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> fmt::Display for ProductPred<A, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, A: BooleanAlgebra, B: BooleanAlgebra> {
            Pred(&'a ProductPred<A, B>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    ProductPred::True => write!(f, "TRUE")?,
                    ProductPred::False => write!(f, "FALSE")?,
                    ProductPred::Both(left, right) => write!(f, "({left:?} × {right:?})")?,
                    ProductPred::LeftOnly(left) => write!(f, "({left:?} × TRUE)")?,
                    ProductPred::RightOnly(right) => write!(f, "(TRUE × {right:?})")?,
                    ProductPred::And(left, right) | ProductPred::Or(left, right) => {
                        write!(f, "(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(right));
                        events.push(Event::Text(
                            if matches!(predicate, ProductPred::And(_, _)) {
                                " ∧ "
                            } else {
                                " ∨ "
                            },
                        ));
                        events.push(Event::Pred(left));
                    }
                    ProductPred::Not(inner) => {
                        write!(f, "¬")?;
                        events.push(Event::Pred(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

/// Domain element for the product algebra: a pair of domain elements.
#[derive(Clone, Debug)]
pub struct ProductDomain<A: BooleanAlgebra, B: BooleanAlgebra>(pub A::Domain, pub B::Domain);

/// A Boolean algebra combining two independent Boolean algebras.
///
/// The product algebra `A × B` has:
/// - **Predicates**: Boolean combinations of `A::Predicate` and `B::Predicate`
/// - **Domain**: `(A::Domain, B::Domain)` pairs
/// - **Satisfiability**: `Both(a, b)` requires both `a` and `b` satisfiable
///   (independent domains, so satisfiability factors)
///
/// This enables mixing constraint domains: e.g.,
/// `ProductAlgebra<PresburgerAlgebra, CharClassAlgebra>` for guards combining
/// numeric and character constraints.
#[derive(Clone, Debug)]
pub struct ProductAlgebra<A: BooleanAlgebra, B: BooleanAlgebra> {
    /// Left component algebra.
    pub left: A,
    /// Right component algebra.
    pub right: B,
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> ProductAlgebra<A, B> {
    /// Create a new product algebra from two component algebras.
    pub fn new(left: A, right: B) -> Self {
        ProductAlgebra { left, right }
    }

    /// Collect all atomic (left, right) predicate pairs in DNF.
    ///
    /// Converts the product predicate to disjunctive normal form over
    /// the atomic predicates. Each disjunct is a pair `(left_pred, right_pred)`.
    /// The overall predicate is satisfiable iff at least one disjunct has
    /// both components satisfiable (independent domains factor per-disjunct).
    fn to_dnf(&self, pred: &ProductPred<A, B>) -> Vec<(A::Predicate, B::Predicate)> {
        enum Task<'a, A: BooleanAlgebra, B: BooleanAlgebra> {
            Convert(&'a ProductPred<A, B>, bool),
            Combine { conjunction: bool },
        }
        let mut tasks = vec![Task::Convert(pred, false)];
        let mut values: Vec<Vec<(A::Predicate, B::Predicate)>> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Convert(predicate, negated) => match predicate {
                    ProductPred::True => values.push(if negated {
                        Vec::new()
                    } else {
                        vec![(self.left.true_pred(), self.right.true_pred())]
                    }),
                    ProductPred::False => values.push(if negated {
                        vec![(self.left.true_pred(), self.right.true_pred())]
                    } else {
                        Vec::new()
                    }),
                    ProductPred::Both(left, right) => values.push(if negated {
                        vec![
                            (self.left.not(left), self.right.true_pred()),
                            (self.left.true_pred(), self.right.not(right)),
                        ]
                    } else {
                        vec![(left.clone(), right.clone())]
                    }),
                    ProductPred::LeftOnly(left) => values.push(vec![(
                        if negated {
                            self.left.not(left)
                        } else {
                            left.clone()
                        },
                        self.right.true_pred(),
                    )]),
                    ProductPred::RightOnly(right) => values.push(vec![(
                        self.left.true_pred(),
                        if negated {
                            self.right.not(right)
                        } else {
                            right.clone()
                        },
                    )]),
                    ProductPred::And(left, right) | ProductPred::Or(left, right) => {
                        let source_is_and = matches!(predicate, ProductPred::And(_, _));
                        tasks.push(Task::Combine {
                            conjunction: source_is_and != negated,
                        });
                        tasks.push(Task::Convert(right, negated));
                        tasks.push(Task::Convert(left, negated));
                    }
                    ProductPred::Not(inner) => tasks.push(Task::Convert(inner, !negated)),
                },
                Task::Combine { conjunction } => {
                    let right = values.pop().expect("right product DNF is present");
                    let mut left = values.pop().expect("left product DNF is present");
                    if conjunction {
                        let capacity = left.len().saturating_mul(right.len());
                        let mut product = Vec::with_capacity(capacity);
                        for (left_a, left_b) in &left {
                            for (right_a, right_b) in &right {
                                product.push((
                                    self.left.and(left_a, right_a),
                                    self.right.and(left_b, right_b),
                                ));
                            }
                        }
                        values.push(product);
                    } else {
                        left.reserve(right.len());
                        left.extend(right);
                        values.push(left);
                    }
                }
            }
        }
        values.pop().expect("the root product produces one DNF")
    }

    // 2026-05-12: `extract_left` and `extract_right` methods DELETED —
    // authored speculatively but never wired up; the documented
    // ProductAlgebra surface (to_dnf, is_satisfiable, witness, evaluate,
    // and/or/not) routes through `to_dnf` instead, which produces both
    // left and right predicates directly via the `(A::Predicate,
    // B::Predicate)` pair at its DNF leaves. See
    // `prattail/docs/design/constraint-theories/product-algebra.md`.
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> BooleanAlgebra for ProductAlgebra<A, B> {
    type Predicate = ProductPred<A, B>;
    type Domain = ProductDomain<A, B>;

    fn true_pred(&self) -> ProductPred<A, B> {
        ProductPred::True
    }

    fn false_pred(&self) -> ProductPred<A, B> {
        ProductPred::False
    }

    fn and(&self, a: &ProductPred<A, B>, b: &ProductPred<A, B>) -> ProductPred<A, B> {
        match (a, b) {
            (ProductPred::True, _) => b.clone(),
            (_, ProductPred::True) => a.clone(),
            (ProductPred::False, _) | (_, ProductPred::False) => ProductPred::False,
            _ => ProductPred::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &ProductPred<A, B>, b: &ProductPred<A, B>) -> ProductPred<A, B> {
        match (a, b) {
            (ProductPred::True, _) | (_, ProductPred::True) => ProductPred::True,
            (ProductPred::False, _) => b.clone(),
            (_, ProductPred::False) => a.clone(),
            _ => ProductPred::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn not(&self, a: &ProductPred<A, B>) -> ProductPred<A, B> {
        match a {
            ProductPred::True => ProductPred::False,
            ProductPred::False => ProductPred::True,
            ProductPred::Not(inner) => (**inner).clone(),
            _ => ProductPred::Not(Box::new(a.clone())),
        }
    }

    fn is_satisfiable(&self, pred: &ProductPred<A, B>) -> bool {
        // Convert to DNF and check each disjunct independently.
        // A disjunct (left, right) is satisfiable iff both components are.
        // The overall predicate is satisfiable iff any disjunct is.
        let dnf = self.to_dnf(pred);
        dnf.iter()
            .any(|(l, r)| self.left.is_satisfiable(l) && self.right.is_satisfiable(r))
    }

    fn witness(&self, pred: &ProductPred<A, B>) -> Option<ProductDomain<A, B>> {
        let dnf = self.to_dnf(pred);
        for (l, r) in &dnf {
            if let (Some(lw), Some(rw)) = (self.left.witness(l), self.right.witness(r)) {
                return Some(ProductDomain(lw, rw));
            }
        }
        None
    }

    fn evaluate(&self, pred: &ProductPred<A, B>, elem: &ProductDomain<A, B>) -> bool {
        enum Frame<'a, A: BooleanAlgebra, B: BooleanAlgebra> {
            Eval(&'a ProductPred<A, B>),
            Not,
            AndRight(&'a ProductPred<A, B>),
            OrRight(&'a ProductPred<A, B>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    ProductPred::True => result = Some(true),
                    ProductPred::False => result = Some(false),
                    ProductPred::Both(left, right) => {
                        result = Some(
                            self.left.evaluate(left, &elem.0)
                                && self.right.evaluate(right, &elem.1),
                        );
                    }
                    ProductPred::LeftOnly(left) => {
                        result = Some(self.left.evaluate(left, &elem.0));
                    }
                    ProductPred::RightOnly(right) => {
                        result = Some(self.right.evaluate(right, &elem.1));
                    }
                    ProductPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    ProductPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    ProductPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => result = Some(!result.take().expect("negated product is evaluated")),
                Frame::AndRight(right) => {
                    if result
                        .take()
                        .expect("left product conjunction is evaluated")
                    {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result
                        .take()
                        .expect("left product disjunction is evaluated")
                    {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root product predicate produces one result")
    }
}

// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn malformed_interval_automaton() -> SymbolicAutomaton<IntervalAlgebra> {
        let algebra = IntervalAlgebra::new(0, 10);
        let mut automaton = SymbolicAutomaton::new(algebra);
        let start = automaton.add_state(false, Some("start".to_string()));
        let accept = automaton.add_state(true, Some("accept".to_string()));
        automaton.set_initial(start);
        automaton.add_transition(start, accept, IntervalPred::Range(1, 2));

        automaton.initial_states.insert(99);
        automaton.accepting_states.insert(99);
        automaton.transitions.push(SymbolicTransition {
            from: 99,
            to: accept,
            guard: IntervalPred::True,
        });
        automaton.transitions.push(SymbolicTransition {
            from: start,
            to: 99,
            guard: IntervalPred::True,
        });
        automaton
    }

    fn transition_endpoints_are_valid<A: BooleanAlgebra>(automaton: &SymbolicAutomaton<A>) -> bool {
        automaton
            .transitions
            .iter()
            .all(|trans| trans.from < automaton.num_states() && trans.to < automaton.num_states())
    }

    #[test]
    fn malformed_state_ids_are_ignored_by_symbolic_traversals() {
        let automaton = malformed_interval_automaton();

        assert!(!automaton.is_empty());
        assert_eq!(automaton.shortest_accepted(), Some(vec![1]));
        assert!(automaton.accepts(&[1]));
        assert!(!automaton.accepts(&[0]));
        assert!(!automaton.accepts(&[]));
    }

    #[test]
    fn malformed_state_ids_are_not_copied_into_derived_automata() {
        let automaton = malformed_interval_automaton();

        let determinized = automaton.determinize();
        assert!(transition_endpoints_are_valid(&determinized));
        assert!(determinized.accepts(&[1]));
        assert!(!determinized.accepts(&[0]));

        let unioned = automaton.union(&automaton);
        assert!(transition_endpoints_are_valid(&unioned));
        assert!(unioned.accepts(&[1]));
        assert!(!unioned.accepts(&[0]));
    }

    #[test]
    fn invalid_only_symbolic_automaton_is_empty() {
        let mut automaton = SymbolicAutomaton::new(IntervalAlgebra::new(0, 10));
        automaton.add_state(false, Some("valid-but-unreachable".to_string()));
        automaton.initial_states.insert(99);
        automaton.accepting_states.insert(99);
        automaton.transitions.push(SymbolicTransition {
            from: 99,
            to: 99,
            guard: IntervalPred::True,
        });

        assert!(automaton.is_empty());
        assert_eq!(automaton.shortest_accepted(), None);
        assert!(!automaton.accepts(&[]));
        assert!(!automaton.accepts(&[1]));

        let determinized = automaton.determinize();
        assert!(determinized.is_empty());
        assert!(transition_endpoints_are_valid(&determinized));
    }
}
