//! Core benchmarks for lling-llang WFST framework.
//!
//! These benchmarks measure performance of critical operations:
//! - Semiring operations (tropical, log)
//! - Lattice algorithms (topological sort, path counting)
//! - Path extraction (Viterbi, N-best, beam search)
//! - CFG parsing (Earley on lattices)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use lling_llang::algorithms::{
    all_pairs_shortest_distance, single_source_shortest_distance, ShortestDistanceConfig,
};
use lling_llang::backend::{HashMapBackend, LatticeBackend};
use lling_llang::cfg::{GrammarBuilder, EarleyParser};
use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
use lling_llang::path::{viterbi, nbest, beam_search};
use lling_llang::semiring::{TropicalWeight, LogWeight, Semiring};
use lling_llang::wfst::{VectorWfst, MutableWfst, StateId};

// ============================================================================
// Helper Functions for Building Test Data
// ============================================================================

/// Build a linear lattice: 0 -> 1 -> 2 -> ... -> n
fn build_linear_lattice(size: usize) -> lling_llang::lattice::Lattice<TropicalWeight, HashMapBackend> {
    let mut backend = HashMapBackend::new();
    let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend.clone());

    for i in 0..size {
        let word = format!("word{}", i);
        let id = backend.intern(&word);
        builder.add_correction_by_id(
            i,
            i + 1,
            id,
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
    }

    builder.build(size)
}

/// Build a diamond lattice with branching factor at each position
fn build_diamond_lattice(
    positions: usize,
    branching: usize,
) -> lling_llang::lattice::Lattice<TropicalWeight, HashMapBackend> {
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

/// Build a simple grammar: S -> NP VP, NP -> Det N, VP -> V | V NP
fn build_simple_grammar() -> lling_llang::cfg::Grammar {
    GrammarBuilder::new()
        .start("S")
        .rule("S", &["NP", "VP"])
        .rule("NP", &["Det", "N"])
        .rule("VP", &["V", "NP"])
        .rule("VP", &["V"])
        .rule("Det", &["the"])
        .rule("Det", &["a"])
        .rule("N", &["dog"])
        .rule("N", &["cat"])
        .rule("V", &["saw"])
        .rule("V", &["chased"])
        .build()
        .expect("valid grammar")
}

/// Build a lattice for a sentence compatible with the simple grammar
fn build_sentence_lattice(
    words: &[&str],
    grammar: &lling_llang::cfg::Grammar,
) -> lling_llang::lattice::Lattice<TropicalWeight, HashMapBackend> {
    let mut backend = HashMapBackend::new();
    let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend.clone());

    for (i, word) in words.iter().enumerate() {
        // Get terminal from grammar
        if let Some(terminal) = grammar.terminal_by_name(word) {
            let _id = backend.intern(word);
            builder.add_correction_by_id(
                i,
                i + 1,
                terminal.vocab_id(),
                TropicalWeight::one(),
                EdgeMetadata::default(),
            );
        }
    }

    builder.build(words.len())
}

/// Build a linear chain WFST: 0 -> 1 -> 2 -> ... -> n
fn build_linear_wfst(n: usize) -> VectorWfst<char, TropicalWeight> {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(n + 1);

    for _ in 0..=n {
        fst.add_state();
    }
    fst.set_start(0);
    fst.set_final(n as StateId, TropicalWeight::one());

    for i in 0..n {
        fst.add_arc(
            i as StateId,
            Some('a'),
            Some('a'),
            (i + 1) as StateId,
            TropicalWeight::new(1.0),
        );
    }

    fst
}

/// Build a diamond WFST with multiple parallel paths
fn build_diamond_wfst(n: usize, branching: usize) -> VectorWfst<char, TropicalWeight> {
    // n positions with branching alternatives each
    // States: 0, 1, ..., n
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(n + 1);

    for _ in 0..=n {
        fst.add_state();
    }
    fst.set_start(0);
    fst.set_final(n as StateId, TropicalWeight::one());

    for pos in 0..n {
        for branch in 0..branching {
            let label = (b'a' + (branch as u8 % 26)) as char;
            fst.add_arc(
                pos as StateId,
                Some(label),
                Some(label),
                (pos + 1) as StateId,
                TropicalWeight::new(1.0 + branch as f64 * 0.1),
            );
        }
    }

    fst
}

/// Build a WFST with cycles for testing non-acyclic algorithms
#[allow(dead_code)]
fn build_cyclic_wfst(n: usize) -> VectorWfst<char, TropicalWeight> {
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(n);

    for _ in 0..n {
        fst.add_state();
    }
    fst.set_start(0);
    fst.set_final((n - 1) as StateId, TropicalWeight::one());

    // Forward chain
    for i in 0..(n - 1) {
        fst.add_arc(
            i as StateId,
            Some('f'),
            Some('f'),
            (i + 1) as StateId,
            TropicalWeight::new(1.0),
        );
    }

    // Add back edge creating a cycle (if n > 2)
    if n > 2 {
        fst.add_arc(
            (n - 2) as StateId,
            Some('b'),
            Some('b'),
            1,
            TropicalWeight::new(0.5), // Lower weight to prefer cycle
        );
    }

    fst
}

// ============================================================================
// Semiring Benchmarks
// ============================================================================

fn semiring_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("semiring");

    // TropicalWeight operations
    group.bench_function("tropical_plus", |b| {
        let a = TropicalWeight::new(1.5);
        let c = TropicalWeight::new(2.5);
        b.iter(|| black_box(a.plus(&c)))
    });

    group.bench_function("tropical_times", |b| {
        let a = TropicalWeight::new(1.5);
        let c = TropicalWeight::new(2.5);
        b.iter(|| black_box(a.times(&c)))
    });

    group.bench_function("tropical_is_zero", |b| {
        let a = TropicalWeight::new(1.5);
        b.iter(|| black_box(a.is_zero()))
    });

    // LogWeight operations (more complex due to log-add)
    group.bench_function("log_plus", |b| {
        let a = LogWeight::new(-1.5);
        let c = LogWeight::new(-2.5);
        b.iter(|| black_box(a.plus(&c)))
    });

    group.bench_function("log_times", |b| {
        let a = LogWeight::new(-1.5);
        let c = LogWeight::new(-2.5);
        b.iter(|| black_box(a.times(&c)))
    });

    group.bench_function("log_from_probability", |b| {
        b.iter(|| black_box(LogWeight::from_probability(0.5)))
    });

    group.bench_function("log_to_probability", |b| {
        let w = LogWeight::new(-0.693); // ln(0.5)
        b.iter(|| black_box(w.to_probability()))
    });

    group.finish();
}

// ============================================================================
// Lattice Algorithm Benchmarks
// ============================================================================

fn lattice_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("lattice");

    // Vary lattice size for scaling analysis (reduced from 1000 to 200 to avoid OOM)
    for size in [10, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("topological_sort_linear", size),
            size,
            |b, &size| {
                let lattice = build_linear_lattice(size);
                b.iter(|| {
                    let mut l = lattice.clone();
                    let order = l.topological_order();
                    black_box(order.map(|o| o.len()))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("topological_sort_diamond", size),
            size,
            |b, &size| {
                let lattice = build_diamond_lattice(size, 3);
                b.iter(|| {
                    let mut l = lattice.clone();
                    let order = l.topological_order();
                    black_box(order.map(|o| o.len()))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("path_count_linear", size),
            size,
            |b, &size| {
                let mut lattice = build_linear_lattice(size);
                // Ensure topological order is computed
                lattice.topological_order();
                b.iter(|| {
                    let mut l = lattice.clone();
                    black_box(l.path_count())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("path_count_diamond", size),
            size,
            |b, &size| {
                let mut lattice = build_diamond_lattice(size, 3);
                lattice.topological_order();
                b.iter(|| {
                    let mut l = lattice.clone();
                    black_box(l.path_count())
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Path Extraction Benchmarks
// ============================================================================

fn path_extraction_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("path");

    // Viterbi on different lattice sizes (reduced from 500 to 200 to avoid OOM)
    for size in [10, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("viterbi_linear", size),
            size,
            |b, &size| {
                let mut lattice = build_linear_lattice(size);
                lattice.topological_order();
                b.iter(|| {
                    let mut l = lattice.clone();
                    black_box(viterbi(&mut l))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("viterbi_diamond", size),
            size,
            |b, &size| {
                let mut lattice = build_diamond_lattice(size, 3);
                lattice.topological_order();
                b.iter(|| {
                    let mut l = lattice.clone();
                    black_box(viterbi(&mut l))
                })
            },
        );
    }

    // N-best extraction - use small lattice (branching^positions paths exist)
    // For 10 positions with 2 alternatives = 2^10 = 1024 paths max
    for n in [1, 5, 10].iter() {
        group.bench_with_input(
            BenchmarkId::new("nbest_diamond_10", n),
            n,
            |b, &n| {
                let mut lattice = build_diamond_lattice(10, 2);
                lattice.topological_order();
                b.iter(|| {
                    let mut l = lattice.clone();
                    black_box(nbest(&mut l, n))
                })
            },
        );
    }

    // Beam search with different widths - use small lattice
    for width in [1, 5, 10].iter() {
        group.bench_with_input(
            BenchmarkId::new("beam_search_diamond_10", width),
            width,
            |b, &width| {
                let mut lattice = build_diamond_lattice(10, 2);
                lattice.topological_order();
                b.iter(|| {
                    let mut l = lattice.clone();
                    black_box(beam_search(&mut l, width))
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// CFG Parsing Benchmarks
// ============================================================================

fn cfg_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("cfg");

    let grammar = build_simple_grammar();

    // Simple valid sentences
    group.bench_function("earley_3_word_sentence", |b| {
        let lattice = build_sentence_lattice(&["the", "dog", "saw"], &grammar);
        let parser = EarleyParser::new(&grammar);
        b.iter(|| black_box(parser.parse_lattice(&lattice)))
    });

    group.bench_function("earley_5_word_sentence", |b| {
        let lattice = build_sentence_lattice(&["the", "dog", "saw", "a", "cat"], &grammar);
        let parser = EarleyParser::new(&grammar);
        b.iter(|| black_box(parser.parse_lattice(&lattice)))
    });

    // Lattice with ambiguity
    group.bench_function("earley_lattice_with_alternatives", |b| {
        let mut backend = HashMapBackend::new();

        // "the dog/cat saw" - alternative at position 1
        let the_id = grammar.terminal_by_name("the").expect("the").vocab_id();
        let dog_id = grammar.terminal_by_name("dog").expect("dog").vocab_id();
        let cat_id = grammar.terminal_by_name("cat").expect("cat").vocab_id();
        let saw_id = grammar.terminal_by_name("saw").expect("saw").vocab_id();

        let _the = backend.intern("the");
        let _dog = backend.intern("dog");
        let _cat = backend.intern("cat");
        let _saw = backend.intern("saw");

        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, the_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, dog_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, cat_id, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(2, 3, saw_id, TropicalWeight::one(), EdgeMetadata::default());
        let lattice = builder.build(3);

        let parser = EarleyParser::new(&grammar);
        b.iter(|| black_box(parser.parse_lattice(&lattice)))
    });

    group.finish();
}

// ============================================================================
// Shortest-Distance Algorithm Benchmarks
// ============================================================================

fn shortest_distance_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("shortest_distance");

    // Single-source shortest distance with different queue disciplines
    for size in [10, 50, 100, 200].iter() {
        // Linear WFST - acyclic, should benefit from TopologicalQueue
        group.bench_with_input(
            BenchmarkId::new("single_source_linear_auto", size),
            size,
            |b, &size| {
                let fst = build_linear_wfst(size);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::default(),
                    ))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single_source_linear_topological", size),
            size,
            |b, &size| {
                let fst = build_linear_wfst(size);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::acyclic(),
                    ))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single_source_linear_tropical", size),
            size,
            |b, &size| {
                let fst = build_linear_wfst(size);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::tropical(),
                    ))
                })
            },
        );

        // Diamond WFST - multiple paths, tests semiring combination
        group.bench_with_input(
            BenchmarkId::new("single_source_diamond_auto", size),
            size,
            |b, &size| {
                let fst = build_diamond_wfst(size, 3);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::default(),
                    ))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single_source_diamond_topological", size),
            size,
            |b, &size| {
                let fst = build_diamond_wfst(size, 3);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::acyclic(),
                    ))
                })
            },
        );
    }

    // All-pairs shortest distance - O(n³) so use smaller sizes
    for size in [5, 10, 20, 30].iter() {
        group.bench_with_input(
            BenchmarkId::new("all_pairs_linear", size),
            size,
            |b, &size| {
                let fst = build_linear_wfst(size);
                b.iter(|| black_box(all_pairs_shortest_distance(&fst)))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("all_pairs_diamond", size),
            size,
            |b, &size| {
                let fst = build_diamond_wfst(size, 3);
                b.iter(|| black_box(all_pairs_shortest_distance(&fst)))
            },
        );
    }

    // Queue discipline comparison - varying edge density
    for branching in [2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("queue_fifo_diamond_50", branching),
            branching,
            |b, &branching| {
                let fst = build_diamond_wfst(50, branching);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::default(),
                    ))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("queue_topological_diamond_50", branching),
            branching,
            |b, &branching| {
                let fst = build_diamond_wfst(50, branching);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::acyclic(),
                    ))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("queue_shortest_first_diamond_50", branching),
            branching,
            |b, &branching| {
                let fst = build_diamond_wfst(50, branching);
                b.iter(|| {
                    black_box(single_source_shortest_distance(
                        &fst,
                        ShortestDistanceConfig::tropical(),
                    ))
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Main Benchmark Groups
// ============================================================================

criterion_group!(
    benches,
    semiring_benchmarks,
    lattice_benchmarks,
    path_extraction_benchmarks,
    cfg_benchmarks,
    shortest_distance_benchmarks
);
criterion_main!(benches);
