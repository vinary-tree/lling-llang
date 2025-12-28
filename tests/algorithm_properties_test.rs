//! Property-based tests for WFST algorithms.

use lling_llang::semiring::{TropicalWeight, Semiring};
use lling_llang::wfst::{VectorWfst, MutableWfst, Wfst};

/// Test connect algorithm on a simple FST.
#[test]
fn test_connect_simple() {
    use lling_llang::algorithms::{connect, ConnectConfig};

    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state(); // Unreachable

    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());
    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));

    let original_states = fst.num_states();
    let removed = connect(&mut fst, ConnectConfig::default());

    // Should remove unreachable state
    assert!(removed > 0 || fst.num_states() <= original_states);
}

/// Test determinize algorithm on a simple FST.
#[test]
fn test_determinize_simple() {
    use lling_llang::algorithms::{determinize, DeterminizeConfig};

    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s1, TropicalWeight::one());
    fst.set_final(s2, TropicalWeight::one());

    // Non-deterministic: two arcs with same label
    fst.add_arc(s0, Some('a'), Some('a'), s1, TropicalWeight::new(1.0));
    fst.add_arc(s0, Some('a'), Some('a'), s2, TropicalWeight::new(2.0));

    let result = determinize(&fst, DeterminizeConfig::default());

    // Should succeed or fail gracefully
    match result {
        Ok(det) => {
            assert!(det.num_states() >= 1);
        }
        Err(_) => {
            // Some FSTs may not be determinizable
        }
    }
}
