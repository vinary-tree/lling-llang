//! Core benchmarks for lling-llang WFST framework.
//!
//! These benchmarks measure performance of critical operations:
//! - Semiring operations (tropical, log)
//! - Lattice algorithms (topological sort, path counting)
//! - Path extraction (Viterbi, N-best, beam search)
//! - CFG parsing (Earley on lattices)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use lling_llang::algorithms::{
    all_pairs_shortest_distance, connect, determinize, is_connected, is_deterministic, minimize,
    push_weights, remove_epsilon, single_source_shortest_distance, ConnectConfig,
    DeterminizeConfig, EpsilonRemovalConfig, MinimizeConfig, PushConfig, ShortestDistanceConfig,
};
use lling_llang::backend::{HashMapBackend, LatticeBackend};
use lling_llang::cfg::{EarleyParser, GrammarBuilder};
use lling_llang::composition::{compose, materialize};
use lling_llang::ctc::{
    compact_ctc, correct_ctc, minimal_ctc, selfless_compact_ctc, selfless_correct_ctc,
};
use lling_llang::differentiable::{backward, forward_score, viterbi_score, GradientWfst};
use lling_llang::lattice::{EdgeMetadata, LatticeBuilder};
use lling_llang::optimization::{
    build_lookahead_table, compute_size_reduction, prepare_for_beam_search, BigramLm, BucketQueue,
    LogPushConfig, LookaheadConfig, NgramLmBuilder, NgramLmConfig, Token, TokenGroup,
    TokenGroupConfig, TokenGroupManager, TokenGroupPool,
};
use lling_llang::path::{beam_search, nbest, viterbi};
use lling_llang::semiring::{
    ExpectationWeight, LeftStringWeight, LogWeight, ProbabilityWeight, RightStringWeight, Semiring,
    TropicalWeight,
};
use lling_llang::wfst::{MutableWfst, StateId, VectorWfst, VectorWfstBuilder};

// ============================================================================
// Helper Functions for Building Test Data
// ============================================================================

/// Build a linear lattice: 0 -> 1 -> 2 -> ... -> n
fn build_linear_lattice(
    size: usize,
) -> lling_llang::lattice::Lattice<TropicalWeight, HashMapBackend> {
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
            n as StateId, // Point to final state
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

    // ProbabilityWeight operations
    group.bench_function("probability_plus", |b| {
        let a = ProbabilityWeight::new(0.3);
        let c = ProbabilityWeight::new(0.5);
        b.iter(|| black_box(a.plus(&c)))
    });

    group.bench_function("probability_times", |b| {
        let a = ProbabilityWeight::new(0.3);
        let c = ProbabilityWeight::new(0.5);
        b.iter(|| black_box(a.times(&c)))
    });

    group.bench_function("probability_divide", |b| {
        use lling_llang::semiring::DivisibleSemiring;
        let a = ProbabilityWeight::new(0.6);
        let c = ProbabilityWeight::new(0.3);
        b.iter(|| black_box(a.divide(&c)))
    });

    group.bench_function("probability_star", |b| {
        use lling_llang::semiring::StarSemiring;
        let a = ProbabilityWeight::new(0.3); // |a| < 1 for convergence
        b.iter(|| black_box(a.star()))
    });

    group.bench_function("probability_from_log", |b| {
        b.iter(|| black_box(ProbabilityWeight::from_log(0.693))) // e^(-0.693) ≈ 0.5
    });

    group.bench_function("probability_to_log", |b| {
        let w = ProbabilityWeight::new(0.5);
        b.iter(|| black_box(w.to_log()))
    });

    // LeftStringWeight operations
    group.bench_function("string_left_plus_short", |b| {
        let a = LeftStringWeight::from_str("hello");
        let c = LeftStringWeight::from_str("helicopter");
        b.iter(|| black_box(a.plus(&c))) // lcp = "hel"
    });

    group.bench_function("string_left_plus_long", |b| {
        let a = LeftStringWeight::from_str("abcdefghijklmnopqrstuvwxyz");
        let c = LeftStringWeight::from_str("abcdefghijklmnop"); // prefix of a
        b.iter(|| black_box(a.plus(&c))) // lcp = c
    });

    group.bench_function("string_left_times", |b| {
        let a = LeftStringWeight::from_str("hello");
        let c = LeftStringWeight::from_str("world");
        b.iter(|| black_box(a.times(&c))) // concat = "helloworld"
    });

    group.bench_function("string_left_times_long", |b| {
        let a = LeftStringWeight::from_str("abcdefghijklmnopqrstuvwxyz");
        let c = LeftStringWeight::from_str("0123456789");
        b.iter(|| black_box(a.times(&c)))
    });

    // RightStringWeight operations
    group.bench_function("string_right_plus_short", |b| {
        let a = RightStringWeight::from_str("testing");
        let c = RightStringWeight::from_str("ing");
        b.iter(|| black_box(a.plus(&c))) // lcs = "ing"
    });

    group.bench_function("string_right_plus_long", |b| {
        let a = RightStringWeight::from_str("abcdefghijklmnopqrstuvwxyz");
        let c = RightStringWeight::from_str("pqrstuvwxyz"); // suffix of a
        b.iter(|| black_box(a.plus(&c))) // lcs = c
    });

    group.bench_function("string_right_times", |b| {
        let a = RightStringWeight::from_str("hello");
        let c = RightStringWeight::from_str("world");
        b.iter(|| black_box(a.times(&c))) // concat = "helloworld"
    });

    // ExpectationWeight operations
    group.bench_function("expectation_plus", |b| {
        let a = ExpectationWeight::new(0.3, 1.0);
        let c = ExpectationWeight::new(0.5, 2.0);
        b.iter(|| black_box(a.plus(&c))) // (0.8, 3.0)
    });

    group.bench_function("expectation_times", |b| {
        let a = ExpectationWeight::new(0.3, 1.0);
        let c = ExpectationWeight::new(0.5, 2.0);
        b.iter(|| black_box(a.times(&c))) // (0.15, 0.3*2.0 + 0.5*1.0) = (0.15, 1.1)
    });

    group.bench_function("expectation_divide", |b| {
        use lling_llang::semiring::DivisibleSemiring;
        let a = ExpectationWeight::new(0.6, 1.2);
        let c = ExpectationWeight::new(0.3, 0.6);
        b.iter(|| black_box(a.divide(&c)))
    });

    group.bench_function("expectation_star", |b| {
        use lling_llang::semiring::StarSemiring;
        let a = ExpectationWeight::new(0.3, 0.1); // |value| < 1 for convergence
        b.iter(|| black_box(a.star()))
    });

    group.bench_function("expectation_from_probability", |b| {
        b.iter(|| black_box(ExpectationWeight::from_probability(0.5)))
    });

    group.bench_function("expectation_from_probability_and_cost", |b| {
        b.iter(|| black_box(ExpectationWeight::from_probability_and_cost(0.5, 3.0)))
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
        group.bench_with_input(BenchmarkId::new("nbest_diamond_10", n), n, |b, &n| {
            let mut lattice = build_diamond_lattice(10, 2);
            lattice.topological_order();
            b.iter(|| {
                let mut l = lattice.clone();
                black_box(nbest(&mut l, n))
            })
        });
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
                    |mut fst| black_box(push_weights(&mut fst, PushConfig::backward()).ok()),
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("forward_linear", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_linear_wfst(size),
                    |mut fst| black_box(push_weights(&mut fst, PushConfig::forward()).ok()),
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("backward_diamond", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_diamond_wfst(size, 3),
                    |mut fst| black_box(push_weights(&mut fst, PushConfig::backward()).ok()),
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("forward_diamond", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || build_diamond_wfst(size, 3),
                    |mut fst| black_box(push_weights(&mut fst, PushConfig::forward()).ok()),
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
        group.bench_with_input(BenchmarkId::new("epsilon_chain", size), size, |b, &size| {
            b.iter_with_setup(
                || build_epsilon_chain_wfst(size),
                |mut fst| black_box(remove_epsilon(&mut fst, EpsilonRemovalConfig::default()).ok()),
            )
        });

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
                    |mut fst| black_box(connect(&mut fst, ConnectConfig::trim())),
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
                        |fst| black_box(determinize(&fst, DeterminizeConfig::standard())),
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
        group.bench_with_input(BenchmarkId::new("redundant", size), &size, |b, &size| {
            b.iter_with_setup(
                || build_redundant_deterministic_wfst(size),
                |fst| black_box(minimize(&fst, MinimizeConfig::standard())),
            )
        });
    }

    // Benchmark minimization on already-minimal WFSTs (should be fast)
    for size in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("already_minimal_linear", size),
            &size,
            |b, &size| {
                b.iter_with_setup(
                    || build_linear_wfst(size),
                    |fst| black_box(minimize(&fst, MinimizeConfig::standard())),
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
            |b, &vocab_size| b.iter(|| black_box(correct_ctc::<LogWeight>(vocab_size))),
        );

        // Compact-CTC: N states, 3N-2 arcs
        group.bench_with_input(
            BenchmarkId::new("compact_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| b.iter(|| black_box(compact_ctc::<LogWeight>(vocab_size))),
        );

        // Minimal-CTC: 1 state, N arcs
        group.bench_with_input(
            BenchmarkId::new("minimal_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| b.iter(|| black_box(minimal_ctc::<LogWeight>(vocab_size))),
        );

        // Selfless Correct-CTC
        group.bench_with_input(
            BenchmarkId::new("selfless_correct_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| b.iter(|| black_box(selfless_correct_ctc::<LogWeight>(vocab_size))),
        );

        // Selfless Compact-CTC
        group.bench_with_input(
            BenchmarkId::new("selfless_compact_ctc_construct", vocab_size),
            vocab_size,
            |b, &vocab_size| b.iter(|| black_box(selfless_compact_ctc::<LogWeight>(vocab_size))),
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
// Phase 6: Differentiable Operations Benchmarks
// ============================================================================

/// Build a WFST with LogWeight for differentiable operations
fn build_log_linear_wfst(size: usize) -> VectorWfst<char, LogWeight> {
    let mut fst = VectorWfst::new();
    fst.add_states(size + 1);
    fst.set_start(0);
    fst.set_final(size as StateId, LogWeight::one());

    for i in 0..size {
        let label = (b'a' + (i % 26) as u8) as char;
        fst.add_arc(
            i as StateId,
            Some(label),
            Some(label),
            (i + 1) as StateId,
            LogWeight::new(1.0 + (i % 10) as f64 * 0.1),
        );
    }
    fst
}

/// Build a WFST with multiple parallel paths for differentiable operations
fn build_log_parallel_wfst(size: usize, num_paths: usize) -> VectorWfst<char, LogWeight> {
    let mut fst = VectorWfst::new();
    fst.add_states(2);
    fst.set_start(0);
    fst.set_final(1, LogWeight::one());

    // Add multiple parallel arcs from start to final
    for i in 0..num_paths {
        let label = (b'a' + (i % 26) as u8) as char;
        fst.add_arc(
            0,
            Some(label),
            Some(label),
            1,
            LogWeight::new(1.0 + (i % 10) as f64 * 0.5),
        );
    }

    // Now extend with size - 1 states (already have 2)
    for i in 2..=size {
        let s = fst.add_state();
        let prev = s - 1;
        let label = (b'x' + ((i - 2) % 3) as u8) as char;
        fst.add_arc(prev, Some(label), Some(label), s, LogWeight::new(0.5));
        if i == size {
            fst.set_final(s, LogWeight::one());
        }
    }

    fst
}

/// Build a diamond WFST with LogWeight for forward score benchmarks
fn build_log_diamond_wfst(layers: usize, width: usize) -> VectorWfst<char, LogWeight> {
    let mut fst = VectorWfst::new();
    // Total states = 1 (start) + layers * width + 1 (final)
    let total_states = 1 + layers * width + 1;
    fst.add_states(total_states);
    fst.set_start(0);
    let final_state = (total_states - 1) as StateId;
    fst.set_final(final_state, LogWeight::one());

    // Connect start to first layer
    for w in 0..width {
        let target = 1 + w;
        let label = (b'a' + (w % 26) as u8) as char;
        fst.add_arc(
            0,
            Some(label),
            Some(label),
            target as StateId,
            LogWeight::new(1.0 + w as f64 * 0.1),
        );
    }

    // Connect intermediate layers
    for layer in 0..(layers - 1) {
        for w_from in 0..width {
            let from = 1 + layer * width + w_from;
            for w_to in 0..width {
                let to = 1 + (layer + 1) * width + w_to;
                let label = (b'm' + ((w_from + w_to) % 10) as u8) as char;
                fst.add_arc(
                    from as StateId,
                    Some(label),
                    Some(label),
                    to as StateId,
                    LogWeight::new(0.5 + (w_from * w_to % 5) as f64 * 0.1),
                );
            }
        }
    }

    // Connect last layer to final
    for w in 0..width {
        let from = 1 + (layers - 1) * width + w;
        let label = (b'z' - (w % 10) as u8) as char;
        fst.add_arc(
            from as StateId,
            Some(label),
            Some(label),
            final_state,
            LogWeight::new(0.5 + w as f64 * 0.05),
        );
    }

    fst
}

fn differentiable_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("differentiable");

    // Forward score benchmarks
    for size in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("forward_score_linear", size),
            size,
            |b, &size| {
                let fst = build_log_linear_wfst(size);
                let grad_fst = GradientWfst::from_wfst(&fst);
                b.iter(|| {
                    grad_fst.reset();
                    black_box(forward_score(&grad_fst))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("viterbi_score_linear", size),
            size,
            |b, &size| {
                let fst = build_log_linear_wfst(size);
                let grad_fst = GradientWfst::from_wfst(&fst);
                b.iter(|| black_box(viterbi_score(&grad_fst)))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("backward_linear", size),
            size,
            |b, &size| {
                let fst = build_log_linear_wfst(size);
                let grad_fst = GradientWfst::from_wfst(&fst);
                b.iter(|| {
                    grad_fst.reset();
                    forward_score(&grad_fst);
                    black_box(backward(&grad_fst))
                })
            },
        );
    }

    // Parallel paths benchmarks (tests log-sum-exp)
    for num_paths in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("forward_score_parallel", num_paths),
            num_paths,
            |b, &num_paths| {
                let fst = build_log_parallel_wfst(5, num_paths);
                let grad_fst = GradientWfst::from_wfst(&fst);
                b.iter(|| {
                    grad_fst.reset();
                    black_box(forward_score(&grad_fst))
                })
            },
        );
    }

    // Diamond (many paths) benchmarks
    for (layers, width) in [(3, 5), (5, 5), (5, 10), (8, 8)].iter() {
        group.bench_with_input(
            BenchmarkId::new("forward_score_diamond", format!("{}x{}", layers, width)),
            &(*layers, *width),
            |b, &(layers, width)| {
                let fst = build_log_diamond_wfst(layers, width);
                let grad_fst = GradientWfst::from_wfst(&fst);
                b.iter(|| {
                    grad_fst.reset();
                    black_box(forward_score(&grad_fst))
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("backward_diamond", format!("{}x{}", layers, width)),
            &(*layers, *width),
            |b, &(layers, width)| {
                let fst = build_log_diamond_wfst(layers, width);
                let grad_fst = GradientWfst::from_wfst(&fst);
                b.iter(|| {
                    grad_fst.reset();
                    forward_score(&grad_fst);
                    black_box(backward(&grad_fst))
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Phase 7: Optimization Benchmarks
// ============================================================================

fn optimization_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimization");

    // Log-semiring weight pushing for beam search
    for size in [10, 50, 100, 200].iter() {
        // Log-push on linear WFST
        group.bench_with_input(
            BenchmarkId::new("log_push_linear", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || {
                        // Build LogWeight WFST for log pushing
                        let mut fst: VectorWfst<char, LogWeight> =
                            VectorWfst::with_capacity(size + 1);
                        for _ in 0..=size {
                            fst.add_state();
                        }
                        fst.set_start(0);
                        fst.set_final(size as StateId, LogWeight::one());
                        for i in 0..size {
                            fst.add_arc(
                                i as StateId,
                                Some('a'),
                                Some('a'),
                                (i + 1) as StateId,
                                LogWeight::new(1.0),
                            );
                        }
                        fst
                    },
                    |mut fst| {
                        black_box(prepare_for_beam_search(&mut fst, LogPushConfig::default()).ok())
                    },
                )
            },
        );

        // Log-push on diamond WFST (multiple parallel paths)
        group.bench_with_input(
            BenchmarkId::new("log_push_diamond", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || {
                        let branching = 3;
                        let mut fst: VectorWfst<char, LogWeight> =
                            VectorWfst::with_capacity(size + 1);
                        for _ in 0..=size {
                            fst.add_state();
                        }
                        fst.set_start(0);
                        fst.set_final(size as StateId, LogWeight::one());
                        for pos in 0..size {
                            for branch in 0..branching {
                                let label = (b'a' + (branch as u8 % 26)) as char;
                                fst.add_arc(
                                    pos as StateId,
                                    Some(label),
                                    Some(label),
                                    (pos + 1) as StateId,
                                    LogWeight::new(1.0 + branch as f64 * 0.1),
                                );
                            }
                        }
                        fst
                    },
                    |mut fst| {
                        black_box(prepare_for_beam_search(&mut fst, LogPushConfig::default()).ok())
                    },
                )
            },
        );
    }

    // Lookahead table construction
    for size in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("lookahead_table_linear", size),
            size,
            |b, &size| {
                // Build LogWeight WFST
                let mut fst: VectorWfst<char, LogWeight> = VectorWfst::with_capacity(size + 1);
                for _ in 0..=size {
                    fst.add_state();
                }
                fst.set_start(0);
                fst.set_final(size as StateId, LogWeight::one());
                for i in 0..size {
                    fst.add_arc(
                        i as StateId,
                        Some('a'),
                        Some('a'),
                        (i + 1) as StateId,
                        LogWeight::new(1.0),
                    );
                }
                b.iter(|| black_box(build_lookahead_table(&fst, LookaheadConfig::default()).ok()))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("lookahead_table_diamond", size),
            size,
            |b, &size| {
                let branching = 3;
                let mut fst: VectorWfst<char, LogWeight> = VectorWfst::with_capacity(size + 1);
                for _ in 0..=size {
                    fst.add_state();
                }
                fst.set_start(0);
                fst.set_final(size as StateId, LogWeight::one());
                for pos in 0..size {
                    for branch in 0..branching {
                        let label = (b'a' + (branch as u8 % 26)) as char;
                        fst.add_arc(
                            pos as StateId,
                            Some(label),
                            Some(label),
                            (pos + 1) as StateId,
                            LogWeight::new(1.0 + branch as f64 * 0.1),
                        );
                    }
                }
                b.iter(|| black_box(build_lookahead_table(&fst, LookaheadConfig::default()).ok()))
            },
        );
    }

    // Lookahead query performance
    group.bench_function("lookahead_query_100_states", |b| {
        let size = 100;
        let mut fst: VectorWfst<char, LogWeight> = VectorWfst::with_capacity(size + 1);
        for _ in 0..=size {
            fst.add_state();
        }
        fst.set_start(0);
        fst.set_final(size as StateId, LogWeight::one());
        for i in 0..size {
            fst.add_arc(
                i as StateId,
                Some('a'),
                Some('a'),
                (i + 1) as StateId,
                LogWeight::new(1.0),
            );
        }
        let table = build_lookahead_table(&fst, LookaheadConfig::default())
            .expect("should build lookahead table");
        b.iter(|| {
            // Query all states
            let mut total = LogWeight::zero();
            for s in 0..size {
                total = total.plus(&table.get(s as StateId));
            }
            black_box(total)
        })
    });

    // Normalize score performance
    group.bench_function("normalize_score_100", |b| {
        let size = 100;
        let mut fst: VectorWfst<char, LogWeight> = VectorWfst::with_capacity(size + 1);
        for _ in 0..=size {
            fst.add_state();
        }
        fst.set_start(0);
        fst.set_final(size as StateId, LogWeight::one());
        for i in 0..size {
            fst.add_arc(
                i as StateId,
                Some('a'),
                Some('a'),
                (i + 1) as StateId,
                LogWeight::new(1.0),
            );
        }
        let table = build_lookahead_table(&fst, LookaheadConfig::default())
            .expect("should build lookahead table");
        let accumulated = LogWeight::new(5.0);
        b.iter(|| {
            // Normalize scores for all states
            let mut total = LogWeight::zero();
            for s in 0..size {
                total = total.plus(&table.normalize_score(s as StateId, &accumulated));
            }
            black_box(total)
        })
    });

    // ========================================================================
    // Token Grouping (LET-Decoder) Benchmarks
    // ========================================================================

    // BucketQueue insert/pop throughput
    for size in [100, 500, 1000, 5000].iter() {
        group.bench_with_input(
            BenchmarkId::new("bucket_queue_insert_pop", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || {
                        // Create queue with reasonable range
                        BucketQueue::<usize>::new(100, 1.0, 0.0)
                    },
                    |mut queue| {
                        // Insert items with varying priorities
                        for i in 0..size {
                            queue.insert((i % 50) as f64, i);
                        }
                        // Pop all items
                        let mut count = 0usize;
                        while queue.pop().is_some() {
                            count += 1;
                        }
                        black_box(count)
                    },
                )
            },
        );
    }

    // TokenGroup add_token throughput
    for size in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("token_group_add_tokens", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || {
                        let base_token = Token {
                            base_state: 0,
                            grammar_state: 0,
                            forward_prob: LogWeight::new(0.0),
                            prev_token: None,
                            prev_arc: None,
                        };
                        (TokenGroup::with_token(0, base_token, 0), size)
                    },
                    |(mut group, size)| {
                        for i in 1..size {
                            let token = Token {
                                base_state: 0,
                                grammar_state: i as StateId,
                                forward_prob: LogWeight::new(i as f64 * 0.1),
                                prev_token: None,
                                prev_arc: None,
                            };
                            group.add_token(token);
                        }
                        black_box(group.num_tokens())
                    },
                )
            },
        );
    }

    // TokenGroupPool get_or_create throughput
    for size in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("token_group_pool_get_or_create", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || TokenGroupPool::with_capacity(size),
                    |mut pool| {
                        // Create groups for different base states
                        for base_state in 0..size {
                            let _ = pool.get_or_create(base_state as StateId);
                        }
                        black_box(pool.len())
                    },
                )
            },
        );
    }

    // TokenGroupPool lookup performance (after creation)
    group.bench_function("token_group_pool_lookup_1000", |b| {
        let mut pool = TokenGroupPool::with_capacity(1000);
        // Pre-populate pool
        for base_state in 0..1000usize {
            let _ = pool.get_or_create(base_state as StateId);
        }
        b.iter(|| {
            // Lookup all groups by ID (0-999)
            let mut found = 0usize;
            for group_id in 0..1000u32 {
                if pool.get(group_id).is_some() {
                    found += 1;
                }
            }
            black_box(found)
        })
    });

    // TokenGroupManager process_token throughput
    for size in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("token_group_manager_process", size),
            size,
            |b, &size| {
                b.iter_with_setup(
                    || TokenGroupManager::new(TokenGroupConfig::default()),
                    |mut manager| {
                        // Process tokens for different base states
                        for i in 0..size {
                            let token = Token {
                                base_state: (i % 100) as StateId, // 100 unique base states
                                grammar_state: (i / 100) as StateId,
                                forward_prob: LogWeight::new(i as f64 * 0.01),
                                prev_token: None,
                                prev_arc: None,
                            };
                            let _ = manager.process_token(token, false);
                        }
                        black_box(manager.stats().tokens_processed)
                    },
                )
            },
        );
    }

    // TokenGroupManager with word arcs (triggers expansion)
    group.bench_function("token_group_manager_word_arcs_500", |b| {
        b.iter_with_setup(
            || TokenGroupManager::new(TokenGroupConfig::default()),
            |mut manager| {
                // Process mix of word and non-word arcs
                for i in 0..500usize {
                    let token = Token {
                        base_state: (i % 50) as StateId,
                        grammar_state: (i / 50) as StateId,
                        forward_prob: LogWeight::new(i as f64 * 0.01),
                        prev_token: None,
                        prev_arc: None,
                    };
                    // Every 5th token is a word arc
                    let is_word = i % 5 == 0;
                    let _ = manager.process_token(token, is_word);
                }
                black_box(manager.stats().expansions)
            },
        )
    });

    // TokenGroupManager advance_frame
    group.bench_function("token_group_manager_advance_frame", |b| {
        b.iter_with_setup(
            || {
                let mut manager = TokenGroupManager::new(TokenGroupConfig::default());
                // Populate with tokens
                for i in 0..200usize {
                    let token = Token {
                        base_state: (i % 20) as StateId,
                        grammar_state: (i / 20) as StateId,
                        forward_prob: LogWeight::new(i as f64 * 0.01),
                        prev_token: None,
                        prev_arc: None,
                    };
                    let _ = manager.process_token(token, false);
                }
                manager
            },
            |mut manager| {
                // Advance through multiple frames
                let mut frame_count = 0u32;
                for _ in 0..10 {
                    let _ = manager.advance_frame();
                    frame_count += 1;
                }
                black_box(frame_count)
            },
        )
    });

    // ========================================================================
    // N-gram Back-off Benchmarks
    // ========================================================================

    // BigramLm creation and probability lookup
    for vocab_size in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("bigram_lm_create", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                b.iter(|| {
                    let mut lm = BigramLm::new(vocab_size);
                    // Add unigrams
                    for w in 0..vocab_size {
                        lm.set_unigram(w as u32, 1.0 + (w as f64 * 0.001));
                    }
                    // Add sparse bigrams (1% density)
                    for i in 0..(vocab_size / 10) {
                        let w1 = (i * 7) % vocab_size;
                        let w2 = (i * 11 + 1) % vocab_size;
                        lm.set_bigram(w1 as u32, w2 as u32, 0.5);
                    }
                    black_box(lm)
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("bigram_lm_lookup", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                let mut lm = BigramLm::new(vocab_size);
                for w in 0..vocab_size {
                    lm.set_unigram(w as u32, 1.0 + (w as f64 * 0.001));
                }
                for i in 0..(vocab_size / 10) {
                    let w1 = (i * 7) % vocab_size;
                    let w2 = (i * 11 + 1) % vocab_size;
                    lm.set_bigram(w1 as u32, w2 as u32, 0.5);
                }
                b.iter(|| {
                    // Random lookups
                    let mut sum = 0.0f64;
                    for i in 0..100 {
                        let w1 = ((i * 17) % vocab_size) as u32;
                        let w2 = ((i * 23 + 5) % vocab_size) as u32;
                        sum += lm.prob(w1, w2);
                    }
                    black_box(sum)
                })
            },
        );
    }

    // BigramLm to WFST conversion
    for vocab_size in [50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("bigram_lm_to_wfst", vocab_size),
            vocab_size,
            |b, &vocab_size| {
                let mut lm = BigramLm::new(vocab_size);
                for w in 0..vocab_size {
                    lm.set_unigram(w as u32, 1.0 + (w as f64 * 0.001));
                    lm.set_backoff(w as u32, 0.1);
                }
                for i in 0..(vocab_size / 5) {
                    let w1 = (i * 7) % vocab_size;
                    let w2 = (i * 11 + 1) % vocab_size;
                    lm.set_bigram(w1 as u32, w2 as u32, 0.5);
                }
                b.iter(|| black_box(lm.to_wfst()))
            },
        );
    }

    // NgramLmBuilder for trigrams
    for num_ngrams in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("trigram_lm_build", num_ngrams),
            num_ngrams,
            |b, &num_ngrams| {
                b.iter_with_setup(
                    || {
                        let config = NgramLmConfig {
                            order: 3,
                            use_backoff_symbol: true,
                            vocab_size: 100,
                            prune_threshold: None,
                        };
                        let mut builder = NgramLmBuilder::new(config);
                        // Add unigrams
                        for w in 0..50u32 {
                            builder.add_ngram(&[], w, 1.0 + w as f64 * 0.01);
                        }
                        // Add bigrams
                        for i in 0..(num_ngrams / 3) {
                            let w1 = ((i * 7) % 50) as u32;
                            let w2 = ((i * 11 + 1) % 50) as u32;
                            builder.add_ngram(&[w1], w2, 0.5 + i as f64 * 0.001);
                        }
                        // Add trigrams
                        for i in 0..(num_ngrams / 3) {
                            let w1 = ((i * 7) % 50) as u32;
                            let w2 = ((i * 11 + 1) % 50) as u32;
                            let w3 = ((i * 13 + 2) % 50) as u32;
                            builder.add_ngram(&[w1, w2], w3, 0.3 + i as f64 * 0.001);
                            builder.add_backoff(&[w1, w2], 0.1);
                        }
                        builder
                    },
                    |builder| black_box(builder.build()),
                )
            },
        );
    }

    // Size reduction calculation
    group.bench_function("size_reduction_calc", |b| {
        b.iter(|| {
            let mut total = 0.0f64;
            for vocab in [100, 500, 1000, 5000] {
                for observed in [1000, 10000, 50000] {
                    let reduction = compute_size_reduction(vocab, observed, 2);
                    total += reduction.arc_reduction;
                }
            }
            black_box(total)
        })
    });

    group.finish();
}

// ============================================================================
// Composition Benchmarks
// ============================================================================

/// Build a linear "relabel" transducer chain of `len` arcs, each mapping
/// `in_label` to `out_label`, for use as a composition operand.
fn build_relabel_chain(
    len: usize,
    in_label: char,
    out_label: char,
) -> VectorWfst<char, TropicalWeight> {
    let mut builder = VectorWfstBuilder::<char, TropicalWeight>::new()
        .add_states(len + 1)
        .start(0)
        .final_state(len as StateId, TropicalWeight::one());
    for i in 0..len {
        builder = builder.arc(
            i as StateId,
            Some(in_label),
            Some(out_label),
            (i + 1) as StateId,
            TropicalWeight::new(1.0),
        );
    }
    builder.build()
}

/// Build a two-arc-per-state transducer of `depth` steps mapping `inputs` to
/// `outputs`; composing a chain of these exercises two matched transitions per
/// product state.
fn build_branch_transducer(
    depth: usize,
    inputs: (char, char),
    outputs: (char, char),
) -> VectorWfst<char, TropicalWeight> {
    let (i0, i1) = inputs;
    let (o0, o1) = outputs;
    let mut builder = VectorWfstBuilder::<char, TropicalWeight>::new()
        .add_states(depth + 1)
        .start(0)
        .final_state(depth as StateId, TropicalWeight::one());
    for i in 0..depth {
        builder = builder
            .arc(
                i as StateId,
                Some(i0),
                Some(o0),
                (i + 1) as StateId,
                TropicalWeight::new(1.0),
            )
            .arc(
                i as StateId,
                Some(i1),
                Some(o1),
                (i + 1) as StateId,
                TropicalWeight::new(1.0),
            );
    }
    builder.build()
}

fn composition_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("composition");

    // Linear chain composition: fst1 maps a->x, fst2 maps x->b; the product is
    // a single diagonal chain. Measures the compose+materialize pipeline cost
    // (the per-decode CTC obs ∘ ctc ∘ lm path shape).
    for size in [10, 50, 100] {
        group.bench_with_input(BenchmarkId::new("chain", size), &size, |b, &size| {
            b.iter_with_setup(
                || {
                    (
                        build_relabel_chain(size, 'a', 'x'),
                        build_relabel_chain(size, 'x', 'b'),
                    )
                },
                |(fst1, fst2)| black_box(materialize(compose(fst1, fst2))),
            )
        });
    }

    // Branching composition: two matched transitions per product state.
    for size in [10, 25, 50] {
        group.bench_with_input(BenchmarkId::new("branching", size), &size, |b, &size| {
            b.iter_with_setup(
                || {
                    (
                        build_branch_transducer(size, ('a', 'b'), ('x', 'y')),
                        build_branch_transducer(size, ('x', 'y'), ('p', 'q')),
                    )
                },
                |(fst1, fst2)| black_box(materialize(compose(fst1, fst2))),
            )
        });
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
    shortest_distance_benchmarks,
    weight_push_benchmarks,
    epsilon_removal_benchmarks,
    connect_benchmarks,
    determinize_benchmarks,
    minimize_benchmarks,
    ctc_topology_benchmarks,
    differentiable_benchmarks,
    optimization_benchmarks,
    composition_benchmarks
);
criterion_main!(benches);
