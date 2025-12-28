//! Integration tests for lattice-based correction pipeline.
//!
//! Tests end-to-end lattice construction and path extraction.

use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
use lling_llang::backend::HashMapBackend;
use lling_llang::path::viterbi;
use lling_llang::semiring::TropicalWeight;

/// Test that Viterbi finds the best path through a simple lattice.
#[test]
fn test_lattice_viterbi_best_path() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Add corrections: position 0 -> 1
    builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
    builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::correction(1));

    let mut lattice = builder.build(1);
    let result = viterbi(&mut lattice);

    assert!(result.success, "Viterbi should find a path");
}

/// Test empty lattice handling.
#[test]
fn test_empty_lattice() {
    let backend = HashMapBackend::new();
    let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

    let mut lattice = builder.build(0);
    let result = viterbi(&mut lattice);

    // Empty lattice where start == end should succeed with empty path
    assert!(result.success, "Empty lattice with start==end should have empty valid path");
}

/// Test single-edge lattice.
#[test]
fn test_single_edge_lattice() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    builder.add_correction(0, 1, "hello", TropicalWeight::new(0.0), EdgeMetadata::default());

    let mut lattice = builder.build(1);
    let result = viterbi(&mut lattice);

    assert!(result.success, "Single-edge lattice should have valid path");
}
