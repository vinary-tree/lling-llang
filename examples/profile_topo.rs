//! Profiling helper for topological sort algorithm.
//! Run with: perf record -g ./target/release/examples/profile_topo

use lling_llang::backend::HashMapBackend;
use lling_llang::backend::LatticeBackend;
use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
use lling_llang::semiring::TropicalWeight;

fn build_diamond_lattice(positions: usize, branching: usize)
    -> lling_llang::lattice::Lattice<TropicalWeight, HashMapBackend>
{
    let mut backend = HashMapBackend::new();
    let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend.clone());

    for pos in 0..positions {
        for branch in 0..branching {
            let word = format!("word{}_{}", pos, branch);
            let id = backend.intern(&word);
            builder.add_correction_by_id(
                pos,
                pos + 1,
                id,
                TropicalWeight::new(1.0 + branch as f64 * 0.1),
                EdgeMetadata::default(),
            );
        }
    }

    builder.build(positions)
}

fn main() {
    let lattice = build_diamond_lattice(200, 3);

    // Run many iterations for profiling
    for _ in 0..50000 {
        let mut l = lattice.clone();
        let _ = l.topological_order();
    }
}
