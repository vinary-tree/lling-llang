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
    push_weights, PushConfig, remove_epsilon, EpsilonRemovalConfig,
    connect, ConnectConfig, is_connected,
    determinize, DeterminizeConfig, is_deterministic,
    minimize, MinimizeConfig,
};
use lling_llang::ctc::{
    correct_ctc, compact_ctc, minimal_ctc,
    selfless_correct_ctc, selfless_compact_ctc,
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

/// Build a WFST with epsilon transitions for epsilon removal benchmarks
fn build_epsilon_chain_wfst(n: usize) -> VectorWfst<char, TropicalWeight> {
    // Alternating epsilon and label transitions: 0 --ε--> 1 --a--> 2 --ε--> 3 --b--> ...
    let states = n * 2 + 1;
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(states);

    for _ in 0..states {
        fst.add_state();
    }
    fst.set_start(0);
    fst.set_final((states - 1) as StateId, TropicalWeight::one());

    for i in 0..n {
        let from = (i * 2) as StateId;
        let mid = (i * 2 + 1) as StateId;
        let to = (i * 2 + 2) as StateId;
        let label = (b'a' + (i as u8 % 26)) as char;

        // Epsilon transition
        fst.add_epsilon(from, mid, TropicalWeight::new(0.5));
        // Label transition
        fst.add_arc(mid, Some(label), Some(label), to, TropicalWeight::new(1.0));
    }

    fst
}

/// Build a WFST with unreachable and dead-end states for connect benchmarks
fn build_disconnected_wfst(n: usize) -> VectorWfst<char, TropicalWeight> {
    // Main path: 0 -> 1 -> ... -> n (useful states)
    // Plus n dead-end states and n unreachable states
    let total_states = n * 3 + 1;
    let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(total_states);

    for _ in 0..total_states {
        fst.add_state();
    }
    fst.set_start(0);
    fst.set_final(n as StateId, TropicalWeight::one());

    // Main path (useful)
    for i in 0..n {
        fst.add_arc(
            i as StateId,
            Some('a'),
            Some('a'),
            (i + 1) as StateId,
            TropicalWeight::new(1.0),
        );
    }

    // Dead-end states (accessible but not coaccessible)
    for i in 0..n {
        let dead_end = (n + 1 + i) as StateId;
        fst.add_arc(
            (i % n) as StateId,
            Some('d'),
            Some('d'),
            dead_end,
            TropicalWeight::new(2.0),
        );
    }

    // Unreachable states (coaccessible but not accessible)
    for i in 0..n {
        let unreachable = (2 * n + 1 + i) as StateId;
        fst.add_arc(
            unreachable,
            Some('u'),
            Some('u'),
            n as StateId,  // Point to final state
            TropicalWeight::new(2.0),
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
// Core WFST Operations Benchmarks (Phase 2)
// ============================================================================

fn weight_push_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("weight_push");

    // Weight pushing on linear and diamond WFSTs
    for size in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("backward_linear", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_linear_wfst(size),
                    |mut fst| {
                        black_box(push_weights(&mut fst, PushConfig::backward()).ok())
                    },
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("forward_linear", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_linear_wfst(size),
                    |mut fst| {
                        black_box(push_weights(&mut fst, PushConfig::forward()).ok())
                    },
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("backward_diamond", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_diamond_wfst(size, 3),
                    |mut fst| {
                        black_box(push_weights(&mut fst, PushConfig::backward()).ok())
                    },
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("forward_diamond", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_diamond_wfst(size, 3),
                    |mut fst| {
                        black_box(push_weights(&mut fst, PushConfig::forward()).ok())
                    },
                )
            },
        );
    }

    group.finish();
}

fn epsilon_removal_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("epsilon_removal");

    // Epsilon removal on different sizes
    for size in [5, 10, 25, 50].iter() {
        group.bench_with_input(
            BenchmarkId::new("epsilon_chain", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_epsilon_chain_wfst(size),
                    |mut fst| {
                        black_box(remove_epsilon(&mut fst, EpsilonRemovalConfig::default()).ok())
                    },
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("epsilon_chain_acyclic", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_epsilon_chain_wfst(size),
                    |mut fst| {
                        black_box(remove_epsilon(&mut fst, EpsilonRemovalConfig::acyclic()).ok())
                    },
                )
            },
        );
    }

    group.finish();
}

fn connect_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("connect");

    // Connect on already-connected FSTs (should be fast)
    for size in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("already_connected_linear", size),
            size,
            |b, &size| {
                let fst = build_linear_wfst(size);
                b.iter(|| black_box(is_connected(&fst)))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("already_connected_diamond", size),
            size,
            |b, &size| {
                let fst = build_diamond_wfst(size, 3);
                b.iter(|| black_box(is_connected(&fst)))
            },
        );
    }

    // Connect on disconnected FSTs
    for size in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("trim_disconnected", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_disconnected_wfst(size),
                    |mut fst| {
                        black_box(connect(&mut fst, ConnectConfig::trim()))
                    },
                )
            },
        );
    }

    group.finish();
}

// ============================================================================
// Phase 3: Determinization & Minimization Benchmarks
// ============================================================================

/// Build a non-deterministic WFST for benchmarking determinization.
/// Creates a graph where each state has multiple outgoing transitions with the same label.
fn build_non_deterministic_wfst(size: usize, branching: usize) -> VectorWfst<char, TropicalWeight> {
    let mut fst = VectorWfst::new();
    // Create size + 1 states (0 through size)
    fst.add_states(size * branching + 2);
    fst.set_start(0);

    // Create non-deterministic structure:
    // From state 0, we have 'branching' paths with same label 'a'
    // Each path goes through different intermediate states
    for b in 0..branching {
        let intermediate_start = 1 + b * size;
        // Add transition with same label from start
        fst.add_arc(
            0,
            Some('a'),
            Some('a'),
            intermediate_start as StateId,
            TropicalWeight::new(1.0 + b as f64 * 0.1),
        );

        // Build a chain from this intermediate
        for i in 0..(size - 1) {
            let from = intermediate_start + i;
            let to = intermediate_start + i + 1;
            let label = (b'b' + (i % 26) as u8) as char;
            fst.add_arc(
                from as StateId,
                Some(label),
                Some(label),
                to as StateId,
                TropicalWeight::new(1.0),
            );
        }

        // Final state
        let final_state = (intermediate_start + size - 1) as StateId;
        fst.set_final(final_state, TropicalWeight::one());
    }

    fst
}

/// Build a redundant (but deterministic) WFST for benchmarking minimization.
/// Creates equivalent states that should be merged.
fn build_redundant_deterministic_wfst(size: usize) -> VectorWfst<char, TropicalWeight> {
    let mut fst = VectorWfst::new();
    // Create duplicate branches that are equivalent
    let num_duplicates = 2;
    fst.add_states(size * num_duplicates + 1);
    fst.set_start(0);

    // Create num_duplicates identical branches from start
    for dup in 0..num_duplicates {
        let chain_start = 1 + dup * size;
        let label = (b'a' + dup as u8) as char; // Different first label
        fst.add_arc(
            0,
            Some(label),
            Some(label),
            chain_start as StateId,
            TropicalWeight::new(1.0),
        );

        // Build chain - all branches have same structure after first transition
        for i in 0..(size - 1) {
            let from = chain_start + i;
            let to = chain_start + i + 1;
            let arc_label = (b'x' + (i % 3) as u8) as char; // Same labels across branches
            fst.add_arc(
                from as StateId,
                Some(arc_label),
                Some(arc_label),
                to as StateId,
                TropicalWeight::new(1.0),
            );
        }

        // Same final weight - makes states equivalent
        let final_state = (chain_start + size - 1) as StateId;
        fst.set_final(final_state, TropicalWeight::one());
    }

    fst
}

fn determinize_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("determinize");

    // Benchmark determinization on non-deterministic WFSTs
    for size in [10, 25, 50] {
        for branching in [2, 3] {
            group.bench_with_input(
                BenchmarkId::new(format!("nondet_b{}", branching), size),
                &(size, branching),
                |b, &(size, branching)| {
                    b.iter_with_setup(
                        || build_non_deterministic_wfst(size, branching),
                        |fst| {
                            black_box(determinize(&fst, DeterminizeConfig::standard()))
                        },
                    )
                },
            );
        }
    }

    // Benchmark is_deterministic check
    for size in [10, 50, 100, 200] {
        group.bench_with_input(
            BenchmarkId::new("is_deterministic_linear", size),
            &size,
            |b, &size| {
                let fst = build_linear_wfst(size);
                b.iter(|| black_box(is_deterministic(&fst)))
            },
        );
    }

    group.finish();
}

fn minimize_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("minimize");

    // Benchmark minimization on redundant WFSTs
    for size in [10, 25, 50] {
        group.bench_with_input(
            BenchmarkId::new("redundant", size),
            &size,
            |b, &size| {
                b.iter_with_setup(
                    || build_redundant_deterministic_wfst(size),
                    |fst| {
                        black_box(minimize(&fst, MinimizeConfig::standard()))
                    },
                )
            },
        );
    }

    // Benchmark minimization on already-minimal WFSTs (should be fast)
    for size in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("already_minimal_linear", size),
            &size,
            |b, &size| {
                b.iter_with_setup(
                    || build_linear_wfst(size),
                    |fst| {
                        black_box(minimize(&fst, MinimizeConfig::standard()))
                    },
                )
            },
        );
    }

    group.finish();
}

// ============================================================================
// Phase 5: CTC Topology Benchmarks
// ============================================================================

fn ctc_topology_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("ctc");

    // Benchmark topology construction for various vocabulary sizes
    for vocab_size in [10, 100, 500, 1000].iter() {
        // Correct-CTC: N states, N² arcs
        group.bench_with_input(
            BenchmarkId::new("correct_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                b.iter(|| black_box(correct_ctc::<LogWeight>(vocab_size)))
            },
        );

        // Compact-CTC: N states, 3N-2 arcs
        group.bench_with_input(
            BenchmarkId::new("compact_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                b.iter(|| black_box(compact_ctc::<LogWeight>(vocab_size)))
            },
        );

        // Minimal-CTC: 1 state, N arcs
        group.bench_with_input(
            BenchmarkId::new("minimal_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                b.iter(|| black_box(minimal_ctc::<LogWeight>(vocab_size)))
            },
        );

        // Selfless Correct-CTC
        group.bench_with_input(
            BenchmarkId::new("selfless_correct_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                b.iter(|| black_box(selfless_correct_ctc::<LogWeight>(vocab_size)))
            },
        );

        // Selfless Compact-CTC
        group.bench_with_input(
            BenchmarkId::new("selfless_compact_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                b.iter(|| black_box(selfless_compact_ctc::<LogWeight>(vocab_size)))
            },
        );
    }

    // Memory usage comparison (measure arc count as proxy)
    group.bench_function("arc_count_comparison_1000", |b| {
        b.iter(|| {
            let correct = correct_ctc::<LogWeight>(1000);
            let compact = compact_ctc::<LogWeight>(1000);
            let minimal = minimal_ctc::<LogWeight>(1000);
            black_box((
                correct.info().num_arcs,
                compact.info().num_arcs,
                minimal.info().num_arcs,
            ))
        })
    });

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
    shortest_distance_benchmarks,
    weight_push_benchmarks,
    epsilon_removal_benchmarks,
    connect_benchmarks,
    determinize_benchmarks,
    minimize_benchmarks,
    ctc_topology_benchmarks
);
criterion_main!(benches);
