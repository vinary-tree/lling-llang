//! Edit semiring for tracking explicit edit operations.
//!
//! The edit semiring combines cost tracking with explicit edit operation recording,
//! enabling explainable corrections. Each weight carries both a cost and the
//! sequence(s) of edit operations that achieve that cost.
//!
//! **Definition:**
//! ```text
//! S_edit = (EditSeq × ℝ₊, ⊕, ⊗, (∅, ∞), ([], 0))
//!
//! EditSeq = sequences of {Copy, Insert, Delete, Substitute, Transpose}
//!
//! (S₁, c₁) ⊕ (S₂, c₂):
//!   if c₁ < c₂: return (S₁, c₁)
//!   if c₂ < c₁: return (S₂, c₂)
//!   if c₁ = c₂: return (S₁ ∪ S₂, c₁)  // Merge alternatives at same cost
//!
//! (S₁, c₁) ⊗ (S₂, c₂) = (S₁ × S₂, c₁ + c₂)
//!   where S₁ × S₂ = {s₁ ++ s₂ | s₁ ∈ S₁, s₂ ∈ S₂}  // Cartesian product concat
//! ```
//!
//! # Why Not `Semiring` Trait?
//!
//! Edit weights contain `SmallVec` which cannot be `Copy`. The [`Semiring`](super::Semiring)
//! trait requires `Copy` for efficiency in numeric weights. Edit weights provide the
//! same API through inherent methods instead of trait implementations.
//!
//! # Use Cases
//!
//! - **Explainable spelling correction**: Show what edits were made
//! - **Alignment visualization**: Display character-level transformations
//! - **Error analysis**: Categorize and count edit types
//! - **Correction confidence**: Compare alternative edit paths
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::{EditWeight, EditOp};
//!
//! // Weight for substituting 'a' with 'e'
//! let subst = EditWeight::single(EditOp::Substitute { from: 'a', to: 'e' }, 1.0);
//!
//! // Weight for deleting 'x'
//! let del = EditWeight::single(EditOp::Delete('x'), 1.0);
//!
//! // Sequential operations: substitute then delete (cost = 2.0)
//! let combined = subst.times(&del);
//! assert_eq!(combined.cost(), 2.0);
//! assert_eq!(combined.num_alternatives(), 1);
//!
//! // The edit sequence is: [Substitute(a→e), Delete(x)]
//! let edits: Vec<_> = combined.sequences().next().expect("semiring/edit.rs: required value was None/Err").collect();
//! assert_eq!(edits.len(), 2);
//! ```
//!
//! # Memory Management
//!
//! The edit semiring can accumulate many alternative sequences. Use `prune()`
//! to limit alternatives when memory is a concern:
//!
//! ```
//! use lling_llang::semiring::{EditWeight, EditOp};
//!
//! let mut weight = EditWeight::one();
//! // ... accumulate many alternatives ...
//! weight.prune(10);  // Keep at most 10 alternatives
//! ```

use ordered_float::OrderedFloat;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

/// An atomic edit operation on a character.
///
/// These operations form the alphabet of edit sequences tracked by [`EditWeight`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EditOp {
    /// Copy a character unchanged (identity operation).
    Copy(char),

    /// Insert a character that wasn't in the source.
    Insert(char),

    /// Delete a character from the source.
    Delete(char),

    /// Substitute one character for another.
    Substitute {
        /// The original character.
        from: char,
        /// The replacement character.
        to: char,
    },

    /// Transpose two adjacent characters (Damerau-Levenshtein extension).
    Transpose {
        /// First character (moves right).
        a: char,
        /// Second character (moves left).
        b: char,
    },
}

impl EditOp {
    /// Returns the default cost for this operation type.
    ///
    /// - Copy: 0.0
    /// - Insert: 1.0
    /// - Delete: 1.0
    /// - Substitute: 1.0
    /// - Transpose: 1.0
    #[inline]
    pub fn default_cost(&self) -> f64 {
        match self {
            EditOp::Copy(_) => 0.0,
            EditOp::Insert(_) => 1.0,
            EditOp::Delete(_) => 1.0,
            EditOp::Substitute { .. } => 1.0,
            EditOp::Transpose { .. } => 1.0,
        }
    }

    /// Returns true if this is a Copy operation (zero cost by default).
    #[inline]
    pub fn is_copy(&self) -> bool {
        matches!(self, EditOp::Copy(_))
    }

    /// Returns the output character produced by this operation, if any.
    #[inline]
    pub fn output_char(&self) -> Option<char> {
        match self {
            EditOp::Copy(c) => Some(*c),
            EditOp::Insert(c) => Some(*c),
            EditOp::Delete(_) => None,
            EditOp::Substitute { to, .. } => Some(*to),
            EditOp::Transpose { a: _, b } => Some(*b), // b comes first after transpose
        }
    }

    /// Returns the input character consumed by this operation, if any.
    #[inline]
    pub fn input_char(&self) -> Option<char> {
        match self {
            EditOp::Copy(c) => Some(*c),
            EditOp::Insert(_) => None,
            EditOp::Delete(c) => Some(*c),
            EditOp::Substitute { from, .. } => Some(*from),
            EditOp::Transpose { a, .. } => Some(*a), // consumes a first
        }
    }
}

impl std::fmt::Display for EditOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditOp::Copy(c) => write!(f, "={}", c),
            EditOp::Insert(c) => write!(f, "+{}", c),
            EditOp::Delete(c) => write!(f, "-{}", c),
            EditOp::Substitute { from, to } => write!(f, "{}>{}", from, to),
            EditOp::Transpose { a, b } => write!(f, "~{}{}", a, b),
        }
    }
}

/// A single sequence of edit operations.
///
/// Stored inline for small sequences (up to 8 operations) to avoid heap allocation.
pub type EditSequence = SmallVec<[EditOp; 8]>;

const MAX_EDIT_ALTERNATIVES: usize = 100;

/// Edit semiring weight tracking both cost and edit operation sequences.
///
/// This weight type enables explainable string transformations by recording
/// the exact sequence of edit operations that achieve a given cost. Multiple
/// alternative sequences with the same cost are preserved.
///
/// # Note
///
/// This type does not implement the [`Semiring`](super::Semiring) trait because
/// it contains `SmallVec` which cannot be `Copy`. It provides semiring-like
/// operations through inherent methods instead.
///
/// # Algebraic Properties
///
/// - **Idempotent ⊕**: duplicate equal-cost alternatives are retained once
/// - **Zero-sum-free**: Only zero (∅, ∞) produces zero sums
/// - **NOT K-closed**: Star may not converge
/// - **Generally non-commutative ⊗**: edit sequence order is semantically meaningful
#[derive(Clone, Debug)]
pub struct EditWeight {
    /// Alternative edit sequences (all with the same cost).
    ///
    /// Invariant: All sequences in this set achieve `cost`.
    /// Empty set represents the zero element (unreachable).
    sequences: SmallVec<[EditSequence; 4]>,

    /// The cost achieved by all sequences.
    ///
    /// Infinity represents the zero element (unreachable).
    cost: OrderedFloat<f64>,
}

impl EditWeight {
    /// Create a new edit weight with a single sequence and its cost.
    #[inline]
    pub fn new(sequence: EditSequence, cost: f64) -> Self {
        let mut sequences = SmallVec::new();
        sequences.push(sequence);
        EditWeight {
            sequences,
            cost: OrderedFloat(cost),
        }
    }

    /// Create an edit weight from a single edit operation.
    #[inline]
    pub fn single(op: EditOp, cost: f64) -> Self {
        let mut seq = EditSequence::new();
        seq.push(op);
        Self::new(seq, cost)
    }

    /// Create an edit weight with a single operation using its default cost.
    #[inline]
    pub fn from_op(op: EditOp) -> Self {
        Self::single(op, op.default_cost())
    }

    /// Additive identity: unreachable (no sequences, infinite cost).
    #[inline]
    pub fn zero() -> Self {
        EditWeight {
            sequences: SmallVec::new(),
            cost: OrderedFloat(f64::INFINITY),
        }
    }

    /// Multiplicative identity: identity transformation (empty sequence, zero cost).
    #[inline]
    pub fn one() -> Self {
        Self::new(EditSequence::new(), 0.0)
    }

    /// Get the cost of this weight.
    #[inline]
    pub fn cost(&self) -> f64 {
        self.cost.into_inner()
    }

    /// Get the number of alternative edit sequences.
    #[inline]
    pub fn num_alternatives(&self) -> usize {
        self.sequences.len()
    }

    /// Iterate over the alternative edit sequences.
    #[inline]
    pub fn sequences(&self) -> impl Iterator<Item = impl Iterator<Item = &EditOp>> {
        self.sequences.iter().map(|seq| seq.iter())
    }

    /// Get the sequences as a slice.
    #[inline]
    pub fn sequences_slice(&self) -> &[EditSequence] {
        &self.sequences
    }

    /// Check if this is the zero element (unreachable).
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.cost.is_infinite() || self.sequences.is_empty()
    }

    /// Check if this is the one element (identity).
    #[inline]
    pub fn is_one(&self) -> bool {
        self.cost.into_inner() == 0.0 && self.sequences.len() == 1 && self.sequences[0].is_empty()
    }

    /// Addition: select lower cost, merge sequences if costs equal.
    pub fn plus(&self, other: &Self) -> Self {
        match self.cost.cmp(&other.cost) {
            Ordering::Less => self.clone(),
            Ordering::Greater => other.clone(),
            Ordering::Equal => {
                // Merge alternatives at same cost
                if self.is_zero() {
                    return other.clone();
                }
                if other.is_zero() {
                    return self.clone();
                }

                let mut merged = self.sequences.clone();
                merged.reserve(other.sequences.len());
                for seq in &other.sequences {
                    // Only add if not already present (avoid duplicates)
                    if !merged.contains(seq) {
                        merged.push(seq.clone());
                    }
                }
                EditWeight {
                    sequences: merged,
                    cost: self.cost,
                }
            }
        }
    }

    /// Multiplication: concatenate sequences, add costs.
    pub fn times(&self, other: &Self) -> Self {
        // Zero annihilates
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }

        let product_bound = self
            .sequences
            .len()
            .saturating_mul(other.sequences.len())
            .min(MAX_EDIT_ALTERNATIVES);
        let mut sequences = SmallVec::with_capacity(product_bound);

        'outer: for seq1 in &self.sequences {
            for seq2 in &other.sequences {
                let mut combined = seq1.clone();
                combined.reserve(seq2.len());
                combined.extend(seq2.iter().cloned());
                sequences.push(combined);

                if sequences.len() == MAX_EDIT_ALTERNATIVES {
                    break 'outer;
                }
            }
        }

        EditWeight {
            sequences,
            cost: OrderedFloat(self.cost.into_inner() + other.cost.into_inner()),
        }
    }

    /// Division: subtract costs.
    ///
    /// Since edit sequences don't have a natural inverse, division only
    /// affects the cost component. The sequences are taken from self.
    pub fn divide(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            return None;
        }

        let new_cost = self.cost.into_inner() - other.cost.into_inner();
        if new_cost.is_nan() {
            return None;
        }

        Some(EditWeight {
            sequences: self.sequences.clone(),
            cost: OrderedFloat(new_cost),
        })
    }

    /// Kleene closure for edit semiring.
    ///
    /// For edit weights, star converges only for zero-cost weights (identity).
    /// Otherwise, the infinite sum of repetitions diverges.
    pub fn star(&self) -> Option<Self> {
        if self.is_zero() {
            // 0* = 1 (identity)
            return Some(Self::one());
        }

        if self.cost.into_inner() >= 0.0 {
            // For non-negative costs, the minimum is achieved at 0 repetitions
            // a* = 1 ⊕ a ⊕ a² ⊕ ... and min is achieved by 1 (0 copies)
            Some(Self::one())
        } else {
            // Negative costs diverge to -∞
            None
        }
    }

    /// Check approximate equality based on cost.
    pub fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        (self.cost.into_inner() - other.cost.into_inner()).abs() <= epsilon
    }

    /// Natural ordering: lower cost is better.
    pub fn natural_less(&self, other: &Self) -> Option<bool> {
        Some(self.cost < other.cost)
    }

    /// Convert to bytes for serialization.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Serialize cost as bytes (primary identifier)
        self.cost.into_inner().to_le_bytes().to_vec()
    }

    /// Prune to keep at most `max_alternatives` sequences.
    ///
    /// This is useful to limit memory usage when many alternatives accumulate.
    /// Pruning is arbitrary (keeps first `max_alternatives`).
    pub fn prune(&mut self, max_alternatives: usize) {
        if self.sequences.len() > max_alternatives {
            self.sequences.truncate(max_alternatives);
        }
    }

    /// Deduplicate sequences, keeping only unique ones.
    ///
    /// This can reduce memory usage when the same sequence appears multiple times.
    pub fn deduplicate(&mut self) {
        if self.sequences.len() <= 1 {
            return;
        }

        // Sort and deduplicate
        self.sequences.sort();
        self.sequences.dedup();
    }

    /// Apply this edit sequence to transform a string.
    ///
    /// Returns the result of applying the first alternative sequence.
    /// Returns `None` if there are no sequences (zero weight).
    pub fn apply(&self, _input: &str) -> Option<String> {
        let seq = self.sequences.first()?;
        let mut output = String::new();

        for op in seq {
            match op {
                EditOp::Copy(c) => output.push(*c),
                EditOp::Insert(c) => output.push(*c),
                EditOp::Delete(_) => {} // Skip deleted char
                EditOp::Substitute { to, .. } => output.push(*to),
                EditOp::Transpose { a, b } => {
                    output.push(*b);
                    output.push(*a);
                }
            }
        }

        Some(output)
    }

    /// Describe the edit sequence as a human-readable string.
    ///
    /// Returns the description of the first alternative.
    pub fn describe(&self) -> String {
        match self.sequences.first() {
            None => "unreachable".to_string(),
            Some(seq) if seq.is_empty() => "identity".to_string(),
            Some(seq) => seq
                .iter()
                .map(|op| op.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    /// Count operations by type in the first alternative.
    pub fn operation_counts(&self) -> EditOpCounts {
        let mut counts = EditOpCounts::default();

        if let Some(seq) = self.sequences.first() {
            for op in seq {
                match op {
                    EditOp::Copy(_) => counts.copies += 1,
                    EditOp::Insert(_) => counts.insertions += 1,
                    EditOp::Delete(_) => counts.deletions += 1,
                    EditOp::Substitute { .. } => counts.substitutions += 1,
                    EditOp::Transpose { .. } => counts.transpositions += 1,
                }
            }
        }

        counts
    }

    /// Quantize the cost for hashing.
    pub fn quantize(&self, epsilon: f64) -> i64 {
        let v = self.cost.into_inner();
        if v.is_nan() {
            i64::MIN
        } else if v.is_infinite() {
            if v > 0.0 {
                i64::MAX
            } else {
                i64::MIN + 1
            }
        } else {
            (v / epsilon).round() as i64
        }
    }
}

/// Counts of edit operations by type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditOpCounts {
    /// Number of copy operations.
    pub copies: usize,
    /// Number of insertion operations.
    pub insertions: usize,
    /// Number of deletion operations.
    pub deletions: usize,
    /// Number of substitution operations.
    pub substitutions: usize,
    /// Number of transposition operations.
    pub transpositions: usize,
}

impl EditOpCounts {
    /// Total number of non-copy operations (edit distance).
    #[inline]
    pub fn edit_distance(&self) -> usize {
        self.insertions + self.deletions + self.substitutions + self.transpositions
    }

    /// Total number of all operations.
    #[inline]
    pub fn total(&self) -> usize {
        self.copies + self.insertions + self.deletions + self.substitutions + self.transpositions
    }
}

impl PartialEq for EditWeight {
    fn eq(&self, other: &Self) -> bool {
        // Two weights are equal if they have the same cost
        // (sequence equality is too expensive and not always meaningful)
        self.cost == other.cost
    }
}

impl Eq for EditWeight {}

impl Hash for EditWeight {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash based on cost (quantized)
        self.cost.hash(state);
    }
}

impl PartialOrd for EditWeight {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EditWeight {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cost.cmp(&other.cost)
    }
}

impl Default for EditWeight {
    /// Default is identity (empty sequence, zero cost).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl std::ops::Add for EditWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Add<&EditWeight> for EditWeight {
    type Output = Self;

    #[inline]
    fn add(self, other: &Self) -> Self {
        self.plus(other)
    }
}

impl std::ops::Mul for EditWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::Mul<&EditWeight> for EditWeight {
    type Output = Self;

    #[inline]
    fn mul(self, other: &Self) -> Self {
        self.times(other)
    }
}

impl std::ops::AddAssign for EditWeight {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl std::ops::MulAssign for EditWeight {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for EditWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("EditWeight", 2)?;
        state.serialize_field("cost", &self.cost.into_inner())?;
        // Serialize only the number of alternatives, not the full sequences
        state.serialize_field("alternatives", &self.sequences.len())?;
        state.end()
    }
}

// ============================================================================
// Builder for Edit Weights
// ============================================================================

/// Builder for constructing edit weights from source/target string pairs.
#[derive(Clone, Debug)]
pub struct EditWeightBuilder {
    /// Cost for insertion operations.
    pub insert_cost: f64,
    /// Cost for deletion operations.
    pub delete_cost: f64,
    /// Cost for substitution operations.
    pub substitute_cost: f64,
    /// Cost for transposition operations.
    pub transpose_cost: f64,
}

impl Default for EditWeightBuilder {
    fn default() -> Self {
        EditWeightBuilder {
            insert_cost: 1.0,
            delete_cost: 1.0,
            substitute_cost: 1.0,
            transpose_cost: 1.0,
        }
    }
}

impl EditWeightBuilder {
    /// Create a new builder with default costs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the insertion cost.
    pub fn insert_cost(mut self, cost: f64) -> Self {
        self.insert_cost = cost;
        self
    }

    /// Set the deletion cost.
    pub fn delete_cost(mut self, cost: f64) -> Self {
        self.delete_cost = cost;
        self
    }

    /// Set the substitution cost.
    pub fn substitute_cost(mut self, cost: f64) -> Self {
        self.substitute_cost = cost;
        self
    }

    /// Set the transposition cost.
    pub fn transpose_cost(mut self, cost: f64) -> Self {
        self.transpose_cost = cost;
        self
    }

    /// Create an edit weight for a single operation.
    pub fn weight_for(&self, op: EditOp) -> EditWeight {
        let cost = match op {
            EditOp::Copy(_) => 0.0,
            EditOp::Insert(_) => self.insert_cost,
            EditOp::Delete(_) => self.delete_cost,
            EditOp::Substitute { .. } => self.substitute_cost,
            EditOp::Transpose { .. } => self.transpose_cost,
        };
        EditWeight::single(op, cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_copy_weight(c: char, cost: f64) -> EditWeight {
        EditWeight::single(EditOp::Copy(c), cost)
    }

    fn make_insert_weight(c: char, cost: f64) -> EditWeight {
        EditWeight::single(EditOp::Insert(c), cost)
    }

    fn make_delete_weight(c: char, cost: f64) -> EditWeight {
        EditWeight::single(EditOp::Delete(c), cost)
    }

    fn make_subst_weight(from: char, to: char, cost: f64) -> EditWeight {
        EditWeight::single(EditOp::Substitute { from, to }, cost)
    }

    #[test]
    fn test_basic_operations() {
        let a = make_subst_weight('a', 'b', 1.0);
        let b = make_insert_weight('c', 2.0);

        // Plus: lower cost wins
        let sum = a.plus(&b);
        assert_eq!(sum.cost(), 1.0);

        // Times: costs add, sequences concatenate
        let prod = a.times(&b);
        assert_eq!(prod.cost(), 3.0);
        assert_eq!(prod.num_alternatives(), 1);
    }

    #[test]
    fn test_identity() {
        let a = make_subst_weight('a', 'b', 1.0);

        // Zero is additive identity
        let sum = a.plus(&EditWeight::zero());
        assert!(a.approx_eq(&sum, 1e-10));

        // One is multiplicative identity
        let prod = a.times(&EditWeight::one());
        assert!(a.approx_eq(&prod, 1e-10));
        assert_eq!(prod.num_alternatives(), a.num_alternatives());
    }

    #[test]
    fn test_annihilation() {
        let a = make_subst_weight('a', 'b', 1.0);

        // Zero annihilates
        let prod = a.times(&EditWeight::zero());
        assert!(prod.is_zero());
    }

    #[test]
    fn test_sequence_merge() {
        // Two different operations with same cost should merge
        let a = make_subst_weight('a', 'b', 1.0);
        let b = make_insert_weight('c', 1.0);

        let sum = a.plus(&b);
        assert_eq!(sum.cost(), 1.0);
        assert_eq!(sum.num_alternatives(), 2); // Both alternatives preserved
    }

    #[test]
    fn test_sequence_concatenation() {
        let copy_a = make_copy_weight('a', 0.0);
        let subst_bc = make_subst_weight('b', 'c', 1.0);
        let del_x = make_delete_weight('x', 1.0);

        // Chain: copy 'a', substitute b->c, delete 'x'
        let chain = copy_a.times(&subst_bc).times(&del_x);

        assert_eq!(chain.cost(), 2.0);
        assert_eq!(chain.num_alternatives(), 1);

        // Check the sequence
        let seq: Vec<_> = chain
            .sequences()
            .next()
            .expect("semiring/edit.rs: required value was None/Err")
            .collect();
        assert_eq!(seq.len(), 3);
        assert!(matches!(seq[0], EditOp::Copy('a')));
        assert!(matches!(seq[1], EditOp::Substitute { from: 'b', to: 'c' }));
        assert!(matches!(seq[2], EditOp::Delete('x')));
    }

    #[test]
    fn test_times_truncates_alternatives_by_prefix_order() {
        let mut left = EditWeight::zero();
        let mut right = EditWeight::zero();

        for offset in 0..12 {
            left = left.plus(&make_insert_weight((b'a' + offset) as char, 1.0));
            right = right.plus(&make_delete_weight((b'A' + offset) as char, 1.0));
        }

        let product = left.times(&right);

        assert_eq!(product.num_alternatives(), MAX_EDIT_ALTERNATIVES);

        let first = &product.sequences_slice()[0];
        assert_eq!(
            first.as_slice(),
            &[EditOp::Insert('a'), EditOp::Delete('A')]
        );

        let last = &product.sequences_slice()[MAX_EDIT_ALTERNATIVES - 1];
        assert_eq!(last.as_slice(), &[EditOp::Insert('i'), EditOp::Delete('D')]);
    }

    #[test]
    fn test_star() {
        // Identity star = identity
        let one = EditWeight::one();
        let star_one = one.star().expect("One star should converge");
        assert!(star_one.is_one());

        // Positive cost star = one (minimum at 0 repetitions)
        let positive = make_subst_weight('a', 'b', 1.0);
        let star_pos = positive.star().expect("Positive cost star should converge");
        assert!(star_pos.is_one());

        // Negative cost should not converge
        let negative = EditWeight::new(EditSequence::new(), -1.0);
        assert!(negative.star().is_none());
    }

    #[test]
    fn test_division() {
        let a = make_subst_weight('a', 'b', 5.0);
        let b = make_insert_weight('c', 3.0);

        // Division subtracts costs
        let quotient = a.divide(&b).expect("Division should succeed");
        assert_eq!(quotient.cost(), 2.0);

        // Division by zero fails
        assert!(a.divide(&EditWeight::zero()).is_none());
    }

    #[test]
    fn test_describe() {
        let seq = make_subst_weight('a', 'b', 1.0)
            .times(&make_insert_weight('c', 1.0))
            .times(&make_delete_weight('x', 1.0));

        let desc = seq.describe();
        assert!(desc.contains("a>b"));
        assert!(desc.contains("+c"));
        assert!(desc.contains("-x"));
    }

    #[test]
    fn test_operation_counts() {
        let seq = make_copy_weight('a', 0.0)
            .times(&make_subst_weight('b', 'c', 1.0))
            .times(&make_insert_weight('d', 1.0))
            .times(&make_delete_weight('e', 1.0));

        let counts = seq.operation_counts();
        assert_eq!(counts.copies, 1);
        assert_eq!(counts.substitutions, 1);
        assert_eq!(counts.insertions, 1);
        assert_eq!(counts.deletions, 1);
        assert_eq!(counts.transpositions, 0);
        assert_eq!(counts.edit_distance(), 3);
    }

    #[test]
    fn test_prune() {
        // Create weight with multiple alternatives
        let a = make_subst_weight('a', 'b', 1.0);
        let b = make_subst_weight('c', 'd', 1.0);
        let c = make_subst_weight('e', 'f', 1.0);

        let merged = a.plus(&b).plus(&c);
        assert_eq!(merged.num_alternatives(), 3);

        let mut pruned = merged.clone();
        pruned.prune(2);
        assert_eq!(pruned.num_alternatives(), 2);
    }

    #[test]
    fn test_builder() {
        let builder = EditWeightBuilder::new()
            .insert_cost(0.5)
            .delete_cost(0.7)
            .substitute_cost(0.9);

        let ins = builder.weight_for(EditOp::Insert('a'));
        assert_eq!(ins.cost(), 0.5);

        let del = builder.weight_for(EditOp::Delete('b'));
        assert_eq!(del.cost(), 0.7);

        let sub = builder.weight_for(EditOp::Substitute { from: 'c', to: 'd' });
        assert_eq!(sub.cost(), 0.9);
    }

    #[test]
    fn test_semiring_axioms() {
        let a = make_subst_weight('a', 'b', 2.0);
        let b = make_insert_weight('c', 3.0);
        let c = make_delete_weight('x', 1.0);

        // Additive identity: a + 0 = a
        assert!(a.plus(&EditWeight::zero()).approx_eq(&a, 1e-10));
        assert!(EditWeight::zero().plus(&a).approx_eq(&a, 1e-10));

        // Multiplicative identity: a * 1 = a
        assert!(a.times(&EditWeight::one()).approx_eq(&a, 1e-10));
        assert!(EditWeight::one().times(&a).approx_eq(&a, 1e-10));

        // Additive commutativity: a + b = b + a
        assert!(a.plus(&b).approx_eq(&b.plus(&a), 1e-10));

        // Additive associativity: (a + b) + c = a + (b + c)
        assert!(a.plus(&b).plus(&c).approx_eq(&a.plus(&b.plus(&c)), 1e-10));

        // Multiplicative associativity: (a * b) * c = a * (b * c)
        assert!(a
            .times(&b)
            .times(&c)
            .approx_eq(&a.times(&b.times(&c)), 1e-10));

        // Zero annihilation: 0 * a = 0 and a * 0 = 0
        assert!(EditWeight::zero().times(&a).is_zero());
        assert!(a.times(&EditWeight::zero()).is_zero());
    }

    #[test]
    fn test_spelling_correction_example() {
        // Example: correct "teh" -> "the"
        // This demonstrates tracking the edit sequence

        // Step 1: Copy 't'
        let step1 = EditWeight::single(EditOp::Copy('t'), 0.0);

        // Step 2: Transpose 'e' and 'h'
        let step2 = EditWeight::single(EditOp::Transpose { a: 'e', b: 'h' }, 1.0);

        // Combined transformation
        let correction = step1.times(&step2);

        assert_eq!(correction.cost(), 1.0);
        let desc = correction.describe();
        assert!(desc.contains("=t"));
        assert!(desc.contains("~eh"));
    }

    #[test]
    fn test_operator_overloading() {
        let a = make_subst_weight('a', 'b', 2.0);
        let b = make_insert_weight('c', 3.0);

        // Addition via +
        let sum = a.clone() + b.clone();
        assert_eq!(sum.cost(), 2.0);

        // Multiplication via *
        let prod = a.clone() * b.clone();
        assert_eq!(prod.cost(), 5.0);

        // With references
        let sum_ref = a.clone() + &b;
        assert_eq!(sum_ref.cost(), 2.0);

        let prod_ref = a.clone() * &b;
        assert_eq!(prod_ref.cost(), 5.0);
    }

    use proptest::prelude::*;

    fn arb_edit_op() -> impl Strategy<Value = EditOp> {
        prop_oneof![
            any::<char>().prop_map(EditOp::Copy),
            any::<char>().prop_map(EditOp::Insert),
            any::<char>().prop_map(EditOp::Delete),
            (any::<char>(), any::<char>()).prop_map(|(f, t)| EditOp::Substitute { from: f, to: t }),
            (any::<char>(), any::<char>()).prop_map(|(a, b)| EditOp::Transpose { a, b }),
        ]
    }

    fn arb_edit_weight() -> impl Strategy<Value = EditWeight> {
        (arb_edit_op(), 0.0f64..100.0).prop_map(|(op, cost)| EditWeight::single(op, cost))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn proptest_plus_associative(
            a in arb_edit_weight(),
            b in arb_edit_weight(),
            c in arb_edit_weight()
        ) {
            // Plus associativity (comparing costs): (a ⊕ b) ⊕ c ≈ a ⊕ (b ⊕ c)
            let left = a.plus(&b).plus(&c);
            let right = a.plus(&b.plus(&c));
            prop_assert!((left.cost() - right.cost()).abs() < 1e-10);
        }

        #[test]
        fn proptest_plus_commutative(
            a in arb_edit_weight(),
            b in arb_edit_weight()
        ) {
            // Plus commutativity (comparing costs): a ⊕ b ≈ b ⊕ a
            let ab = a.plus(&b);
            let ba = b.plus(&a);
            prop_assert!((ab.cost() - ba.cost()).abs() < 1e-10);
        }

        #[test]
        fn proptest_plus_identity(a in arb_edit_weight()) {
            // Plus identity: a ⊕ 0 ≈ a
            let zero = EditWeight::zero();
            let sum = a.plus(&zero);
            prop_assert!(a.approx_eq(&sum, 1e-10));
        }

        #[test]
        fn proptest_times_associative(
            a in arb_edit_weight(),
            b in arb_edit_weight(),
            c in arb_edit_weight()
        ) {
            // Times associativity: (a ⊗ b) ⊗ c ≈ a ⊗ (b ⊗ c)
            let left = a.times(&b).times(&c);
            let right = a.times(&b.times(&c));
            prop_assert!((left.cost() - right.cost()).abs() < 1e-10);
        }

        #[test]
        fn proptest_times_identity(a in arb_edit_weight()) {
            // Times identity: a ⊗ 1 ≈ a and 1 ⊗ a ≈ a
            let one = EditWeight::one();
            prop_assert!(a.times(&one).approx_eq(&a, 1e-10));
            prop_assert!(one.times(&a).approx_eq(&a, 1e-10));
        }

        #[test]
        fn proptest_zero_annihilation(a in arb_edit_weight()) {
            // Zero annihilation: a ⊗ 0 = 0 and 0 ⊗ a = 0
            let zero = EditWeight::zero();
            prop_assert!(a.times(&zero).is_zero());
            prop_assert!(zero.times(&a).is_zero());
        }

        #[test]
        fn proptest_left_distributivity(
            a in arb_edit_weight(),
            b in arb_edit_weight(),
            c in arb_edit_weight()
        ) {
            // Left distributivity (comparing costs): a ⊗ (b ⊕ c) has cost ≤ (a ⊗ b) ⊕ (a ⊗ c)
            // Note: Due to min semantics, distributivity holds for costs
            let left = a.times(&b.plus(&c));
            let right = a.times(&b).plus(&a.times(&c));
            // Both should have same minimum cost
            prop_assert!((left.cost() - right.cost()).abs() < 1e-10);
        }

        #[test]
        fn proptest_cost_non_negative(a in arb_edit_weight(), b in arb_edit_weight()) {
            // Costs should remain non-negative after operations
            let sum = a.plus(&b);
            let prod = a.times(&b);
            prop_assert!(sum.cost() >= 0.0);
            prop_assert!(prod.cost() >= 0.0);
        }

        #[test]
        fn proptest_times_adds_costs(
            op1 in arb_edit_op(),
            cost1 in 0.0f64..100.0,
            op2 in arb_edit_op(),
            cost2 in 0.0f64..100.0
        ) {
            // Times should add costs
            let a = EditWeight::single(op1, cost1);
            let b = EditWeight::single(op2, cost2);
            let prod = a.times(&b);
            prop_assert!((prod.cost() - (cost1 + cost2)).abs() < 1e-10);
        }

        #[test]
        fn proptest_plus_takes_minimum_cost(
            op1 in arb_edit_op(),
            cost1 in 0.0f64..100.0,
            op2 in arb_edit_op(),
            cost2 in 0.0f64..100.0
        ) {
            // Plus should take minimum cost
            let a = EditWeight::single(op1, cost1);
            let b = EditWeight::single(op2, cost2);
            let sum = a.plus(&b);
            prop_assert!((sum.cost() - cost1.min(cost2)).abs() < 1e-10);
        }
    }
}
