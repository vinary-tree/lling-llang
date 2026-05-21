//! Set semiring for ambiguity tracking and feature propagation.
//!
//! The set semiring (2^T, ∪, ∩, ∅, U) operates on finite sets with:
//!
//! - **⊕ = ∪**: Set union for combining alternatives from parallel paths
//! - **⊗ = ∩**: Set intersection for combining features from sequential transitions
//! - **0̄ = ∅**: Empty set (annihilator for intersection)
//! - **1̄ = U**: Universe marker (identity for intersection)
//!
//! # Use Cases
//!
//! - **Ambiguity tracking**: Track which annotations are possible at each position
//! - **Feature propagation**: Propagate linguistic features through the transducer
//! - **Constraint satisfaction**: Find paths that satisfy all required features
//!
//! # Universe Representation
//!
//! Since representing the actual universal set is impractical, we use a special
//! "universe" flag. When the universe flag is set:
//! - `universe ∩ A = A` (identity for intersection)
//! - `universe ∪ A = universe` (universe absorbs everything)
//!
//! # Note on Semiring Trait
//!
//! `SetWeight` does NOT implement the [`Semiring`] trait because the trait requires
//! `Copy`, and sets cannot be `Copy` due to their variable-size nature.
//! Instead, `SetWeight` provides the same API via inherent methods (`zero()`,
//! `one()`, `plus()`, `times()`), enabling manual semiring-style operations.
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::SetWeight;
//! use std::collections::BTreeSet;
//!
//! let mut a_set = BTreeSet::new();
//! a_set.insert("noun");
//! a_set.insert("verb");
//! let a = SetWeight::from_set(a_set);
//!
//! let mut b_set = BTreeSet::new();
//! b_set.insert("verb");
//! b_set.insert("adj");
//! let b = SetWeight::from_set(b_set);
//!
//! // Union (⊕): {"noun", "verb"} ∪ {"verb", "adj"} = {"noun", "verb", "adj"}
//! let union = a.plus(&b);
//! assert_eq!(union.len(), 3);
//!
//! // Intersection (⊗): {"noun", "verb"} ∩ {"verb", "adj"} = {"verb"}
//! let intersection = a.times(&b);
//! assert_eq!(intersection.len(), 1);
//! assert!(intersection.contains(&"verb"));
//! ```
//!
//! # Mathematical Properties
//!
//! - Idempotent: A ∪ A = A and A ∩ A = A
//! - Commutative: Both ⊕ and ⊗ are commutative
//! - Zero-sum-free: A ∪ B = ∅ only when both A = ∅ and B = ∅
//!
//! # Small Set Optimization
//!
//! Uses `SmallVec` for inline storage of small sets to avoid heap allocation
//! for typical use cases with few elements.

use std::collections::BTreeSet;
use std::fmt;
use std::hash::Hash;

use smallvec::SmallVec;

/// Maximum number of elements stored inline (before spilling to heap).
const INLINE_CAPACITY: usize = 8;

/// Set semiring weight for tracking sets of elements.
///
/// Stores a finite set of elements or a special "universe" marker.
/// Uses `SmallVec` for inline storage of small sets.
#[derive(Clone)]
pub struct SetWeight<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> {
    /// The set elements, stored in sorted order for deterministic behavior.
    /// If `is_universe` is true, this field is ignored.
    elements: SmallVec<[T; INLINE_CAPACITY]>,
    /// Flag indicating this represents the universal set.
    is_universe: bool,
}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> SetWeight<T> {
    /// Create an empty set (the additive identity, zero).
    #[inline]
    pub fn empty() -> Self {
        SetWeight {
            elements: SmallVec::new(),
            is_universe: false,
        }
    }

    /// Create the universal set (the multiplicative identity, one).
    #[inline]
    pub fn universe() -> Self {
        SetWeight {
            elements: SmallVec::new(),
            is_universe: true,
        }
    }

    /// Create a set from a single element.
    #[inline]
    pub fn singleton(element: T) -> Self {
        let mut elements = SmallVec::new();
        elements.push(element);
        SetWeight {
            elements,
            is_universe: false,
        }
    }

    /// Create a set from an iterator of elements.
    pub fn from_iter(iter: impl IntoIterator<Item = T>) -> Self {
        let mut elements: SmallVec<[T; INLINE_CAPACITY]> = iter.into_iter().collect();
        elements.sort();
        elements.dedup();
        SetWeight {
            elements,
            is_universe: false,
        }
    }

    /// Create a set from a BTreeSet (already sorted and deduplicated).
    pub fn from_set(set: BTreeSet<T>) -> Self {
        SetWeight {
            elements: set.into_iter().collect(),
            is_universe: false,
        }
    }

    /// Check if this is the empty set.
    #[inline]
    pub fn is_empty(&self) -> bool {
        !self.is_universe && self.elements.is_empty()
    }

    /// Check if this is the universal set.
    #[inline]
    pub fn is_universal(&self) -> bool {
        self.is_universe
    }

    /// Get the number of elements (0 for universe, actual count otherwise).
    #[inline]
    pub fn len(&self) -> usize {
        if self.is_universe {
            // Universe has "infinite" elements, but we return 0 for practical purposes
            0
        } else {
            self.elements.len()
        }
    }

    /// Check if the set contains an element.
    #[inline]
    pub fn contains(&self, element: &T) -> bool {
        if self.is_universe {
            true // Universe contains everything
        } else {
            self.elements.binary_search(element).is_ok()
        }
    }

    /// Get an iterator over the elements.
    ///
    /// Returns an empty iterator for the universe (since we can't enumerate all elements).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements.iter()
    }

    /// Convert to a BTreeSet.
    ///
    /// Panics if this is the universe (cannot enumerate infinite set).
    pub fn to_set(&self) -> BTreeSet<T> {
        if self.is_universe {
            panic!("Cannot convert universe to finite set");
        }
        self.elements.iter().cloned().collect()
    }

    /// Union of two sets (⊕ operation).
    fn set_union(&self, other: &Self) -> Self {
        // Universe absorbs everything in union
        if self.is_universe || other.is_universe {
            return SetWeight::universe();
        }

        // Merge sorted arrays
        let mut result = SmallVec::with_capacity(self.elements.len() + other.elements.len());
        let mut i = 0;
        let mut j = 0;

        while i < self.elements.len() && j < other.elements.len() {
            match self.elements[i].cmp(&other.elements[j]) {
                std::cmp::Ordering::Less => {
                    result.push(self.elements[i].clone());
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    result.push(other.elements[j].clone());
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    result.push(self.elements[i].clone());
                    i += 1;
                    j += 1;
                }
            }
        }

        // Add remaining elements
        while i < self.elements.len() {
            result.push(self.elements[i].clone());
            i += 1;
        }
        while j < other.elements.len() {
            result.push(other.elements[j].clone());
            j += 1;
        }

        SetWeight {
            elements: result,
            is_universe: false,
        }
    }

    /// Intersection of two sets (⊗ operation).
    fn set_intersection(&self, other: &Self) -> Self {
        // Empty set annihilates in intersection
        if self.is_empty() || other.is_empty() {
            return SetWeight::empty();
        }

        // Universe is identity for intersection
        if self.is_universe {
            return other.clone();
        }
        if other.is_universe {
            return self.clone();
        }

        // Intersect sorted arrays
        let mut result = SmallVec::new();
        let mut i = 0;
        let mut j = 0;

        while i < self.elements.len() && j < other.elements.len() {
            match self.elements[i].cmp(&other.elements[j]) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    result.push(self.elements[i].clone());
                    i += 1;
                    j += 1;
                }
            }
        }

        SetWeight {
            elements: result,
            is_universe: false,
        }
    }
}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> PartialEq for SetWeight<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.is_universe != other.is_universe {
            return false;
        }
        if self.is_universe {
            return true; // Both are universe
        }
        self.elements == other.elements
    }
}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> Eq for SetWeight<T> {}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> Hash for SetWeight<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.is_universe.hash(state);
        if !self.is_universe {
            for elem in &self.elements {
                elem.hash(state);
            }
        }
    }
}

impl<T: Clone + Eq + Ord + Hash + fmt::Debug + Send + Sync + 'static> fmt::Debug for SetWeight<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_universe {
            write!(f, "SetWeight(Universe)")
        } else {
            write!(f, "SetWeight({:?})", self.elements.as_slice())
        }
    }
}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> Default for SetWeight<T> {
    /// Default is the multiplicative identity (universe).
    fn default() -> Self {
        Self::one()
    }
}

// ============================================================================
// Semiring-like API (inherent methods)
// ============================================================================
//
// Note: SetWeight cannot implement the Semiring trait because the trait requires
// Copy, and SetWeight contains a SmallVec which is not Copy.
// Instead, we provide the same API via inherent methods.

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> SetWeight<T> {
    /// Additive identity: empty set
    ///
    /// This is the identity for `plus()` (union).
    #[inline]
    pub fn zero() -> Self {
        SetWeight::empty()
    }

    /// Multiplicative identity: universe
    ///
    /// This is the identity for `times()` (intersection).
    #[inline]
    pub fn one() -> Self {
        SetWeight::universe()
    }

    /// Addition (⊕): set union
    ///
    /// Combines elements from both sets.
    pub fn plus(&self, other: &Self) -> Self {
        self.set_union(other)
    }

    /// Multiplication (⊗): set intersection
    ///
    /// Returns elements present in both sets.
    pub fn times(&self, other: &Self) -> Self {
        self.set_intersection(other)
    }

    /// Check if this is the additive identity (empty set).
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.is_empty()
    }

    /// Check if this is the multiplicative identity (universe).
    #[inline]
    pub fn is_one(&self) -> bool {
        self.is_universal()
    }

    /// Sets are exactly equal (no floating point concerns).
    pub fn approx_eq(&self, other: &Self, _epsilon: f64) -> bool {
        self == other
    }

    /// Natural ordering: smaller sets are "better" (more specific).
    pub fn natural_less(&self, other: &Self) -> Option<bool> {
        // Universe is the "worst" (least specific)
        match (self.is_universe, other.is_universe) {
            (true, true) => Some(false),
            (true, false) => Some(false), // Universe is worse
            (false, true) => Some(true),  // Finite is better than universe
            (false, false) => Some(self.elements.len() < other.elements.len()),
        }
    }

    /// Serialize to bytes (simple encoding).
    pub fn to_bytes(&self) -> Vec<u8> {
        // Simple encoding: 1 byte for universe flag, then length-prefixed elements
        // This is a placeholder - real implementation would need proper serialization
        let mut bytes = Vec::new();
        bytes.push(if self.is_universe { 1 } else { 0 });
        if !self.is_universe {
            bytes.extend((self.elements.len() as u64).to_le_bytes());
        }
        bytes
    }
}

// ============================================================================
// Algebraic Properties (documented but not trait-based)
// ============================================================================
//
// SetWeight satisfies these algebraic properties:
// - Idempotent: A ∪ A = A and A ∩ A = A
// - Commutative: Both ⊕ and ⊗ are commutative
// - Zero-sum-free: A ∪ B = ∅ only when both A = ∅ and B = ∅
// - K-closed with k = 1: Star operation converges immediately (A* = A ∪ U = U)

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> std::ops::Add for SetWeight<T> {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> std::ops::Mul for SetWeight<T> {
    type Output = Self;

    #[inline]
    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> std::ops::AddAssign for SetWeight<T> {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = self.plus(&other);
    }
}

impl<T: Clone + Eq + Ord + Hash + Send + Sync + 'static> std::ops::MulAssign for SetWeight<T> {
    #[inline]
    fn mul_assign(&mut self, other: Self) {
        *self = self.times(&other);
    }
}

/// Type alias for set weights over strings (common use case).
pub type StringSetWeight = SetWeight<String>;

/// Type alias for set weights over static string slices.
pub type StrSetWeight = SetWeight<&'static str>;

/// Type alias for set weights over u32 (feature IDs).
pub type FeatureSetWeight = SetWeight<u32>;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    #[test]
    fn test_basic_operations() {
        let a = SetWeight::from_iter(vec![1, 2, 3]);
        let b = SetWeight::from_iter(vec![2, 3, 4]);

        // Union: {1, 2, 3} ∪ {2, 3, 4} = {1, 2, 3, 4}
        let union = a.plus(&b);
        assert_eq!(union.len(), 4);
        assert!(union.contains(&1));
        assert!(union.contains(&2));
        assert!(union.contains(&3));
        assert!(union.contains(&4));

        // Intersection: {1, 2, 3} ∩ {2, 3, 4} = {2, 3}
        let intersection = a.times(&b);
        assert_eq!(intersection.len(), 2);
        assert!(!intersection.contains(&1));
        assert!(intersection.contains(&2));
        assert!(intersection.contains(&3));
        assert!(!intersection.contains(&4));
    }

    #[test]
    fn test_identities() {
        let a = SetWeight::from_iter(vec![1, 2, 3]);
        let empty: SetWeight<i32> = SetWeight::empty();
        let universe: SetWeight<i32> = SetWeight::universe();

        // Empty is additive identity: A ∪ ∅ = A
        assert_eq!(a.plus(&empty), a);
        assert_eq!(empty.plus(&a), a);

        // Universe is multiplicative identity: A ∩ U = A
        assert_eq!(a.times(&universe), a);
        assert_eq!(universe.times(&a), a);
    }

    #[test]
    fn test_annihilation() {
        let a = SetWeight::from_iter(vec![1, 2, 3]);
        let empty: SetWeight<i32> = SetWeight::empty();

        // Empty annihilates in intersection: A ∩ ∅ = ∅
        assert!(a.times(&empty).is_zero());
        assert!(empty.times(&a).is_zero());
    }

    #[test]
    fn test_universe_absorption() {
        let a = SetWeight::from_iter(vec![1, 2, 3]);
        let universe: SetWeight<i32> = SetWeight::universe();

        // Universe absorbs in union: A ∪ U = U
        assert!(a.plus(&universe).is_one());
        assert!(universe.plus(&a).is_one());
    }

    #[test]
    fn test_idempotence() {
        let a = SetWeight::from_iter(vec![1, 2, 3]);

        // A ∪ A = A
        assert_eq!(a.plus(&a), a);

        // A ∩ A = A
        assert_eq!(a.times(&a), a);
    }

    #[test]
    fn test_commutativity() {
        let a = SetWeight::from_iter(vec![1, 2]);
        let b = SetWeight::from_iter(vec![2, 3]);

        // A ∪ B = B ∪ A
        assert_eq!(a.plus(&b), b.plus(&a));

        // A ∩ B = B ∩ A
        assert_eq!(a.times(&b), b.times(&a));
    }

    #[test]
    fn test_distributivity() {
        let a = SetWeight::from_iter(vec![1, 2]);
        let b = SetWeight::from_iter(vec![2, 3]);
        let c = SetWeight::from_iter(vec![3, 4]);

        // A ∩ (B ∪ C) = (A ∩ B) ∪ (A ∩ C)
        let left = a.times(&b.plus(&c));
        let right = a.times(&b).plus(&a.times(&c));
        assert_eq!(left, right);
    }

    #[test]
    fn test_singleton() {
        let a = SetWeight::singleton(42i32);
        assert_eq!(a.len(), 1);
        assert!(a.contains(&42));
        assert!(!a.contains(&0));
    }

    #[test]
    fn test_from_set() {
        let mut set = BTreeSet::new();
        set.insert(1);
        set.insert(2);
        set.insert(3);

        let a = SetWeight::from_set(set.clone());
        assert_eq!(a.to_set(), set);
    }

    #[test]
    fn test_natural_ordering() {
        let small = SetWeight::from_iter(vec![1]);
        let medium = SetWeight::from_iter(vec![1, 2, 3]);
        let universe: SetWeight<i32> = SetWeight::universe();

        // Smaller sets are "better"
        assert_eq!(small.natural_less(&medium), Some(true));
        assert_eq!(medium.natural_less(&small), Some(false));

        // Finite sets are better than universe
        assert_eq!(small.natural_less(&universe), Some(true));
        assert_eq!(universe.natural_less(&small), Some(false));
    }

    #[test]
    fn test_string_sets() {
        let a = SetWeight::from_iter(vec!["noun".to_string(), "verb".to_string()]);
        let b = SetWeight::from_iter(vec!["verb".to_string(), "adj".to_string()]);

        let union = a.plus(&b);
        assert_eq!(union.len(), 3);

        let intersection = a.times(&b);
        assert_eq!(intersection.len(), 1);
        assert!(intersection.contains(&"verb".to_string()));
    }

    proptest! {
        #[test]
        fn proptest_semiring_axioms(
            a_elems in prop::collection::vec(0u32..50, 0..5),
            b_elems in prop::collection::vec(0u32..50, 0..5),
            c_elems in prop::collection::vec(0u32..50, 0..5)
        ) {
            let a = SetWeight::from_iter(a_elems);
            let b = SetWeight::from_iter(b_elems);
            let c = SetWeight::from_iter(c_elems);
            let zero: SetWeight<u32> = SetWeight::zero();
            let one: SetWeight<u32> = SetWeight::one();

            // Plus associativity: (a ⊕ b) ⊕ c = a ⊕ (b ⊕ c)
            prop_assert_eq!(a.plus(&b).plus(&c), a.plus(&b.plus(&c)));

            // Plus commutativity: a ⊕ b = b ⊕ a
            prop_assert_eq!(a.plus(&b), b.plus(&a));

            // Plus identity: a ⊕ 0 = a
            prop_assert_eq!(a.plus(&zero), a.clone());

            // Times associativity: (a ⊗ b) ⊗ c = a ⊗ (b ⊗ c)
            prop_assert_eq!(a.times(&b).times(&c), a.times(&b.times(&c)));

            // Times identity: a ⊗ 1 = a
            prop_assert_eq!(a.times(&one), a.clone());

            // Zero annihilation: a ⊗ 0 = 0
            prop_assert!(a.times(&zero).is_zero());

            // Left distributivity: a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)
            prop_assert_eq!(a.times(&b.plus(&c)), a.times(&b).plus(&a.times(&c)));

            // Right distributivity: (a ⊕ b) ⊗ c = (a ⊗ c) ⊕ (b ⊗ c)
            prop_assert_eq!(a.plus(&b).times(&c), a.times(&c).plus(&b.times(&c)));
        }

        #[test]
        fn proptest_idempotent(a_elems in prop::collection::vec(0u32..50, 0..5)) {
            let a = SetWeight::from_iter(a_elems);

            // Plus idempotent: a ⊕ a = a
            prop_assert_eq!(a.plus(&a), a.clone());

            // Times idempotent: a ⊗ a = a
            prop_assert_eq!(a.times(&a), a);
        }

        #[test]
        fn proptest_commutative_times(
            a_elems in prop::collection::vec(0u32..50, 0..5),
            b_elems in prop::collection::vec(0u32..50, 0..5)
        ) {
            let a = SetWeight::from_iter(a_elems);
            let b = SetWeight::from_iter(b_elems);

            // Times commutativity: a ⊗ b = b ⊗ a
            prop_assert_eq!(a.times(&b), b.times(&a));
        }

        #[test]
        fn proptest_zero_sum_free(
            a_elems in prop::collection::vec(0u32..50, 0..5),
            b_elems in prop::collection::vec(0u32..50, 0..5)
        ) {
            let a = SetWeight::from_iter(a_elems);
            let b = SetWeight::from_iter(b_elems);

            // Zero-sum-free: a ⊕ b = 0 implies a = 0 and b = 0
            let sum = a.plus(&b);
            if sum.is_zero() {
                prop_assert!(a.is_zero(), "a should be zero when a ⊕ b = 0");
                prop_assert!(b.is_zero(), "b should be zero when a ⊕ b = 0");
            }
        }

        #[test]
        fn proptest_union_correct(
            a_elems in prop::collection::vec(0u32..100, 0..10),
            b_elems in prop::collection::vec(0u32..100, 0..10)
        ) {
            let a = SetWeight::from_iter(a_elems.clone());
            let b = SetWeight::from_iter(b_elems.clone());
            let union = a.plus(&b);

            // Check all elements from both sets are in union
            let a_hs: HashSet<_> = a_elems.iter().collect();
            let b_hs: HashSet<_> = b_elems.iter().collect();

            for elem in a_hs.union(&b_hs) {
                prop_assert!(union.contains(elem));
            }
        }

        #[test]
        fn proptest_intersection_correct(
            a_elems in prop::collection::vec(0u32..100, 0..10),
            b_elems in prop::collection::vec(0u32..100, 0..10)
        ) {
            let a = SetWeight::from_iter(a_elems.clone());
            let b = SetWeight::from_iter(b_elems.clone());
            let intersection = a.times(&b);

            // Check intersection contains exactly the common elements
            let a_hs: HashSet<_> = a_elems.iter().collect();
            let b_hs: HashSet<_> = b_elems.iter().collect();

            for elem in a_hs.intersection(&b_hs) {
                prop_assert!(intersection.contains(elem));
            }

            // Check no extra elements
            for elem in intersection.iter() {
                prop_assert!(a.contains(elem) && b.contains(elem));
            }
        }

        #[test]
        fn proptest_universe_identity(a_elems in prop::collection::vec(0u32..100, 0..10)) {
            let a = SetWeight::from_iter(a_elems);
            let universe: SetWeight<u32> = SetWeight::universe();

            // Universe is identity for intersection
            prop_assert_eq!(a.times(&universe), a.clone());
            prop_assert_eq!(universe.times(&a), a);
        }
    }
}
