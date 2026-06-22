//! Feature-gated tests for lling-llang semiring integration with UnionZipper.
//!
//! These tests verify that IdempotentSemiring types (TropicalWeight, BoolWeight)
//! work correctly with the Lattice trait and UnionZipper.
//!
//! Run with: `cargo test --features lling-llang`
//!
//! **Note**: These tests are disabled when `persistent-artrie` is enabled because
//! lling-llang semiring types (TropicalWeight, BoolWeight) do not implement serde,
//! which is required by `DictionaryValue` when `persistent-artrie` is enabled.

#![cfg(all(feature = "lattice", not(feature = "lattice-persistent")))]

use libdictenstein::double_array_trie::zipper::DoubleArrayTrieZipper;
use libdictenstein::double_array_trie::DoubleArrayTrie;
use libdictenstein::union_zipper::{LatticeJoin, UnionZipper, ValuedUnionIterator};
use libdictenstein::zipper::{DictZipper, ValuedDictZipper};
use lling_llang::lattice_bridge::SemiringLatticeWrapper;
use lling_llang::prelude::{BoolWeight, Semiring, TropicalWeight};
use ordered_float::OrderedFloat;

/// Helper to create TropicalWeight from f64
fn tropical(v: f64) -> TropicalWeight {
    TropicalWeight(OrderedFloat(v))
}

// ============================================================================
// TropicalWeight Tests
// ============================================================================

#[test]
fn test_tropical_weight_lattice_join() {
    // TropicalWeight: plus = min, so join = min
    // This is the semiring "sum" which picks the better (lower cost) path

    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("path_a", SemiringLatticeWrapper(tropical(5.0))),
            ("path_b", SemiringLatticeWrapper(tropical(10.0))),
        ]
        .into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("path_a", SemiringLatticeWrapper(tropical(3.0))),
            ("path_c", SemiringLatticeWrapper(tropical(7.0))),
        ]
        .into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);

    let union = UnionZipper::with_strategy(vec![z1, z2], LatticeJoin);

    // Check "path_a" - should be min(5.0, 3.0) = 3.0
    let path_a = union
        .descend(b'p')
        .and_then(|z| z.descend(b'a'))
        .and_then(|z| z.descend(b't'))
        .and_then(|z| z.descend(b'h'))
        .and_then(|z| z.descend(b'_'))
        .and_then(|z| z.descend(b'a'))
        .expect("Should find 'path_a'");

    let value = path_a.value().expect("Should have value");
    assert_eq!(value.0 .0 .0, 3.0);
}

#[test]
fn test_tropical_weight_iteration() {
    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("cost", SemiringLatticeWrapper(tropical(100.0))),
            ("dist", SemiringLatticeWrapper(tropical(50.0))),
        ]
        .into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("cost", SemiringLatticeWrapper(tropical(80.0))),
            ("time", SemiringLatticeWrapper(tropical(30.0))),
        ]
        .into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);

    let union = UnionZipper::with_strategy(vec![z1, z2], LatticeJoin);
    let valued_iter = ValuedUnionIterator::new(union);

    let mut results: Vec<(String, f64)> = valued_iter
        .map(|(path, val)| (String::from_utf8(path).unwrap(), val.0 .0 .0))
        .collect();
    results.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        results,
        vec![
            ("cost".to_string(), 80.0), // min(100, 80)
            ("dist".to_string(), 50.0),
            ("time".to_string(), 30.0),
        ]
    );
}

#[test]
fn test_tropical_weight_infinity() {
    // Test with infinity (tropical zero)
    let tropical_zero = TropicalWeight::zero();
    assert!(tropical_zero.0 .0.is_infinite());

    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![("inf", SemiringLatticeWrapper(tropical_zero))].into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![("inf", SemiringLatticeWrapper(tropical(42.0)))].into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);

    let union = UnionZipper::with_strategy(vec![z1, z2], LatticeJoin);

    let inf = union
        .descend(b'i')
        .and_then(|z| z.descend(b'n'))
        .and_then(|z| z.descend(b'f'))
        .expect("Should find 'inf'");

    // min(∞, 42) = 42
    let value = inf.value().expect("Should have value");
    assert_eq!(value.0 .0 .0, 42.0);
}

// ============================================================================
// BoolWeight Tests
// ============================================================================

#[test]
fn test_bool_weight_lattice_join() {
    // BoolWeight: plus = OR, so join = OR
    // This is the semiring "sum" which is disjunction

    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("flag_a", SemiringLatticeWrapper(BoolWeight(true))),
            ("flag_b", SemiringLatticeWrapper(BoolWeight(false))),
        ]
        .into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("flag_a", SemiringLatticeWrapper(BoolWeight(false))),
            ("flag_b", SemiringLatticeWrapper(BoolWeight(true))),
        ]
        .into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);

    let union = UnionZipper::with_strategy(vec![z1, z2], LatticeJoin);

    // Check "flag_a" - should be true OR false = true
    let flag_a = union
        .descend(b'f')
        .and_then(|z| z.descend(b'l'))
        .and_then(|z| z.descend(b'a'))
        .and_then(|z| z.descend(b'g'))
        .and_then(|z| z.descend(b'_'))
        .and_then(|z| z.descend(b'a'))
        .expect("Should find 'flag_a'");

    let value = flag_a.value().expect("Should have value");
    assert!(value.0 .0);

    // Check "flag_b" - should be false OR true = true
    let flag_b = union
        .descend(b'f')
        .and_then(|z| z.descend(b'l'))
        .and_then(|z| z.descend(b'a'))
        .and_then(|z| z.descend(b'g'))
        .and_then(|z| z.descend(b'_'))
        .and_then(|z| z.descend(b'b'))
        .expect("Should find 'flag_b'");

    let value = flag_b.value().expect("Should have value");
    assert!(value.0 .0);
}

#[test]
fn test_bool_weight_all_false() {
    // Test that false OR false = false
    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![("disabled", SemiringLatticeWrapper(BoolWeight(false)))].into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![("disabled", SemiringLatticeWrapper(BoolWeight(false)))].into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);

    let union = UnionZipper::with_strategy(vec![z1, z2], LatticeJoin);

    let disabled = union
        .descend(b'd')
        .and_then(|z| z.descend(b'i'))
        .and_then(|z| z.descend(b's'))
        .and_then(|z| z.descend(b'a'))
        .and_then(|z| z.descend(b'b'))
        .and_then(|z| z.descend(b'l'))
        .and_then(|z| z.descend(b'e'))
        .and_then(|z| z.descend(b'd'))
        .expect("Should find 'disabled'");

    // false OR false = false
    let value = disabled.value().expect("Should have value");
    assert!(!value.0 .0);
}

#[test]
fn test_bool_weight_iteration() {
    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("a", SemiringLatticeWrapper(BoolWeight(true))),
            ("b", SemiringLatticeWrapper(BoolWeight(false))),
            ("c", SemiringLatticeWrapper(BoolWeight(false))),
        ]
        .into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("a", SemiringLatticeWrapper(BoolWeight(false))),
            ("b", SemiringLatticeWrapper(BoolWeight(false))),
            ("d", SemiringLatticeWrapper(BoolWeight(true))),
        ]
        .into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);

    let union = UnionZipper::with_strategy(vec![z1, z2], LatticeJoin);
    let valued_iter = ValuedUnionIterator::new(union);

    let mut results: Vec<(String, bool)> = valued_iter
        .map(|(path, val)| (String::from_utf8(path).unwrap(), val.0 .0))
        .collect();
    results.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        results,
        vec![
            ("a".to_string(), true),  // true OR false
            ("b".to_string(), false), // false OR false
            ("c".to_string(), false), // only in dict1
            ("d".to_string(), true),  // only in dict2
        ]
    );
}

// ============================================================================
// SemiringLatticeWrapper Tests
// ============================================================================

#[test]
fn test_semiring_lattice_wrapper_join_is_plus() {
    use libdictenstein::union_zipper::Lattice;

    // Verify that join delegates to semiring plus
    let a = SemiringLatticeWrapper(tropical(10.0));
    let b = SemiringLatticeWrapper(tropical(5.0));

    let joined = a.join(&b);
    let plusd = SemiringLatticeWrapper(a.0.plus(&b.0));

    assert_eq!(joined.0 .0 .0, plusd.0 .0 .0);
    assert_eq!(joined.0 .0 .0, 5.0); // min(10, 5) = 5
}

#[test]
fn test_semiring_lattice_wrapper_meet_is_times() {
    use libdictenstein::union_zipper::Lattice;

    // Verify that meet delegates to semiring times
    // Note: For tropical, times = + (addition), which may not be ideal for "meet"
    // but this is the documented behavior of the wrapper
    let a = SemiringLatticeWrapper(tropical(10.0));
    let b = SemiringLatticeWrapper(tropical(5.0));

    let met = a.meet(&b);
    let timed = SemiringLatticeWrapper(a.0.times(&b.0));

    assert_eq!(met.0 .0 .0, timed.0 .0 .0);
    assert_eq!(met.0 .0 .0, 15.0); // 10 + 5 = 15 (path composition, not lattice meet)
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_tropical_weight_three_dictionaries() {
    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![("key", SemiringLatticeWrapper(tropical(100.0)))].into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![("key", SemiringLatticeWrapper(tropical(50.0)))].into_iter(),
    );
    let dict3 = DoubleArrayTrie::from_terms_with_values(
        vec![("key", SemiringLatticeWrapper(tropical(75.0)))].into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);
    let z3 = DoubleArrayTrieZipper::new_from_dict(&dict3);

    let union = UnionZipper::with_strategy(vec![z1, z2, z3], LatticeJoin);

    let key = union
        .descend(b'k')
        .and_then(|z| z.descend(b'e'))
        .and_then(|z| z.descend(b'y'))
        .expect("Should find 'key'");

    // min(100, 50, 75) = 50
    let value = key.value().expect("Should have value");
    assert_eq!(value.0 .0 .0, 50.0);
}

#[test]
fn test_mixed_overlap_tropical() {
    // Some terms overlap, some don't
    let dict1 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("shared", SemiringLatticeWrapper(tropical(10.0))),
            ("only1", SemiringLatticeWrapper(tropical(20.0))),
        ]
        .into_iter(),
    );
    let dict2 = DoubleArrayTrie::from_terms_with_values(
        vec![
            ("shared", SemiringLatticeWrapper(tropical(5.0))),
            ("only2", SemiringLatticeWrapper(tropical(15.0))),
        ]
        .into_iter(),
    );

    let z1 = DoubleArrayTrieZipper::new_from_dict(&dict1);
    let z2 = DoubleArrayTrieZipper::new_from_dict(&dict2);

    let union = UnionZipper::with_strategy(vec![z1, z2], LatticeJoin);

    // Count all terms
    let count = union.iter().count();
    assert_eq!(count, 3); // shared, only1, only2

    // Check merged value
    let shared = union
        .descend(b's')
        .and_then(|z| z.descend(b'h'))
        .and_then(|z| z.descend(b'a'))
        .and_then(|z| z.descend(b'r'))
        .and_then(|z| z.descend(b'e'))
        .and_then(|z| z.descend(b'd'))
        .expect("Should find 'shared'");

    let value = shared.value().expect("Should have value");
    assert_eq!(value.0 .0 .0, 5.0); // min(10, 5)
}
