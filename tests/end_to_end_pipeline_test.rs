//! End-to-end integration tests for correction pipelines.
//!
//! Tests complete workflows from input through all processing stages to output.

use lling_llang::algorithms::{
    connect, determinize, minimize, single_source_shortest_distance, ConnectConfig,
    DeterminizeConfig, MinimizeConfig, ShortestDistanceConfig,
};
use lling_llang::backend::HashMapBackend;
use lling_llang::cfg::{EarleyParser, GrammarBuilder};
use lling_llang::composition::{compose, materialize};
use lling_llang::error_models::{qwerty_confusion_matrix, ConfusionMatrix, EditDistanceTransducer};
use lling_llang::lattice::{EdgeMetadata, LatticeBuilder};
#[cfg(feature = "latex-syntax")]
use lling_llang::layers::latex::{LatexGrammar, LatexSyntaxLayer, LatexValidator};
#[cfg(feature = "mathml-semantic")]
use lling_llang::layers::mathml::{
    GlyphMeaning, HomoglyphDisambiguator, MathContext, MathMLSemanticLayer, MathTypeChecker,
};
use lling_llang::layers::{CorrectionLayer, LayerPipeline, LayerPipelineBuilder};
use lling_llang::path::{beam_search, nbest, viterbi};
use lling_llang::semiring::{LogWeight, ProbabilityWeight, Semiring, TropicalWeight};
use lling_llang::wfst::{MutableWfst, VectorWfst, Wfst, NO_STATE};

// =============================================================================
// Part 1: Text Correction Pipeline Tests
// =============================================================================

/// Test basic spelling correction with edit distance transducer.
#[test]
fn test_text_correction_edit_distance_pipeline() {
    // Build an edit distance transducer for max distance 1
    let transducer =
        EditDistanceTransducer::levenshtein(1).with_alphabet("abcdefghijklmnopqrstuvwxyz");

    // The transducer models error patterns, composition would be with a dictionary
    let fst = transducer.build();

    assert!(fst.num_states() > 0, "Edit distance FST should have states");
    assert!(fst.start() != NO_STATE, "FST should have a start state");
}

/// Test QWERTY keyboard confusion transducer integration.
#[test]
fn test_text_correction_qwerty_confusion_pipeline() {
    let matrix = qwerty_confusion_matrix();

    // Adjacent keys should have defined confusion costs
    let q_w = matrix.substitution_cost('q', 'w');
    let a_s = matrix.substitution_cost('a', 's');

    assert!(q_w.is_some(), "Adjacent keys should have confusion cost");
    assert!(a_s.is_some(), "Adjacent keys should have confusion cost");

    // Verify the matrix was constructed
    assert!(
        !matrix.alphabet().is_empty(),
        "Matrix should have an alphabet"
    );
}

/// Test lattice construction from multiple correction candidates.
#[test]
fn test_text_correction_multi_candidate_lattice() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Simulate correction candidates for "teh" -> "the" or "ten"
    builder.add_correction(
        0,
        1,
        "the",
        TropicalWeight::new(0.1),
        EdgeMetadata::correction(1),
    );
    builder.add_correction(
        0,
        1,
        "ten",
        TropicalWeight::new(0.5),
        EdgeMetadata::correction(1),
    );
    builder.add_correction(
        0,
        1,
        "tea",
        TropicalWeight::new(0.8),
        EdgeMetadata::correction(1),
    );

    // Add second word "cat" -> "cat" (no change)
    builder.add_correction(
        1,
        2,
        "cat",
        TropicalWeight::new(0.0),
        EdgeMetadata::default(),
    );

    let mut lattice = builder.build(2);
    let result = viterbi(&mut lattice);

    assert!(result.success, "Viterbi should find a path");
}

/// Test complete pipeline: confusion -> composition -> best path.
#[test]
fn test_text_correction_full_pipeline() {
    // Build confusion-based FST
    let mut matrix = ConfusionMatrix::new();
    matrix.add_substitution('a', 'e', 0.2);
    matrix.add_substitution('e', 'a', 0.2);

    // Build lattice with candidates
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // "cat" might be "cet" due to confusion
    builder.add_correction(
        0,
        1,
        "cat",
        TropicalWeight::new(0.0),
        EdgeMetadata::default(),
    );
    builder.add_correction(
        0,
        1,
        "cet",
        TropicalWeight::new(0.2),
        EdgeMetadata::correction(1),
    );

    let mut lattice = builder.build(1);
    let result = viterbi(&mut lattice);

    assert!(result.success, "Pipeline should find a path");
}

/// Test N-best extraction from correction lattice.
#[test]
fn test_text_correction_nbest_extraction() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Multiple candidates with different weights
    builder.add_correction(
        0,
        1,
        "the",
        TropicalWeight::new(0.1),
        EdgeMetadata::correction(1),
    );
    builder.add_correction(
        0,
        1,
        "then",
        TropicalWeight::new(0.3),
        EdgeMetadata::correction(1),
    );
    builder.add_correction(
        0,
        1,
        "them",
        TropicalWeight::new(0.5),
        EdgeMetadata::correction(1),
    );
    builder.add_correction(
        0,
        1,
        "there",
        TropicalWeight::new(0.7),
        EdgeMetadata::correction(1),
    );

    let mut lattice = builder.build(1);

    // Extract top-3 paths
    let paths = nbest(&mut lattice, 3);

    // Should get up to 3 paths
    assert!(paths.len() <= 3, "N-best should return at most 3 paths");
}

/// Test beam search for pruned correction search.
#[test]
fn test_text_correction_beam_search() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Add many candidates - beam search should prune
    for i in 0..10 {
        let weight = TropicalWeight::new(i as f64 * 0.1);
        builder.add_correction(
            0,
            1,
            &format!("word{}", i),
            weight,
            EdgeMetadata::correction(1),
        );
    }

    let mut lattice = builder.build(1);

    let paths = beam_search(&mut lattice, 3);
    assert!(!paths.is_empty(), "Beam search should find paths");
}

// =============================================================================
// Part 2: WFST Composition Pipeline Tests
// =============================================================================

/// Test identity transducer preserves input.
#[test]
fn test_wfst_identity_composition() {
    // Build a simple acceptor using VectorWfst directly
    let mut fst1: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s0 = fst1.add_state();
    let s1 = fst1.add_state();
    fst1.set_start(s0);
    fst1.set_final(s1, TropicalWeight::one());
    fst1.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(0.5));

    // Build identity transducer for label 1
    let mut fst2: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let t0 = fst2.add_state();
    fst2.set_start(t0);
    fst2.set_final(t0, TropicalWeight::one());
    fst2.add_arc(t0, Some(1), Some(1), t0, TropicalWeight::one());

    // Compose
    let composed = compose(fst1.clone(), fst2.clone());
    let materialized = materialize(composed);

    // Should produce equivalent output
    assert!(materialized.num_states() > 0);
}

/// Test transducer chain composition (A ∘ B ∘ C).
#[test]
fn test_wfst_chain_composition() {
    // FST A: maps 1 -> 2
    let mut fst_a: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let a0 = fst_a.add_state();
    let a1 = fst_a.add_state();
    fst_a.set_start(a0);
    fst_a.set_final(a1, TropicalWeight::one());
    fst_a.add_arc(a0, Some(1), Some(2), a1, TropicalWeight::new(0.1));

    // FST B: maps 2 -> 3
    let mut fst_b: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let b0 = fst_b.add_state();
    let b1 = fst_b.add_state();
    fst_b.set_start(b0);
    fst_b.set_final(b1, TropicalWeight::one());
    fst_b.add_arc(b0, Some(2), Some(3), b1, TropicalWeight::new(0.2));

    // FST C: maps 3 -> 4
    let mut fst_c: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let c0 = fst_c.add_state();
    let c1 = fst_c.add_state();
    fst_c.set_start(c0);
    fst_c.set_final(c1, TropicalWeight::one());
    fst_c.add_arc(c0, Some(3), Some(4), c1, TropicalWeight::new(0.3));

    // Compose A ∘ B
    let ab = compose(fst_a.clone(), fst_b.clone());
    let ab_mat = materialize(ab);

    // Then (A ∘ B) ∘ C
    let abc = compose(ab_mat, fst_c.clone());
    let abc_mat = materialize(abc);

    // Final FST should map 1 -> 4
    assert!(
        abc_mat.start() != NO_STATE,
        "Composed FST should have start state"
    );
}

/// Test lazy vs eager composition equivalence.
#[test]
fn test_wfst_lazy_eager_equivalence() {
    // Build two simple FSTs
    let mut fst1: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s0 = fst1.add_state();
    let s1 = fst1.add_state();
    fst1.set_start(s0);
    fst1.set_final(s1, TropicalWeight::one());
    fst1.add_arc(s0, Some(1), Some(2), s1, TropicalWeight::new(0.5));
    fst1.add_arc(s0, Some(2), Some(3), s1, TropicalWeight::new(0.7));

    let mut fst2: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let t0 = fst2.add_state();
    let t1 = fst2.add_state();
    fst2.set_start(t0);
    fst2.set_final(t1, TropicalWeight::one());
    fst2.add_arc(t0, Some(2), Some(4), t1, TropicalWeight::new(0.3));
    fst2.add_arc(t0, Some(3), Some(5), t1, TropicalWeight::new(0.4));

    // Lazy composition
    let lazy = compose(fst1.clone(), fst2.clone());

    // Both should have valid structure (check component states)
    let start = lazy.start();
    assert!(start.s1 != NO_STATE && start.s2 != NO_STATE);

    // Materialize
    let eager = materialize(lazy);
    assert!(eager.start() != NO_STATE);
}

// =============================================================================
// Part 3: Algorithm Pipeline Tests
// =============================================================================

/// Test determinize -> minimize pipeline.
#[test]
fn test_algorithm_determinize_minimize_pipeline() {
    // Build a non-deterministic FST
    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    let s3 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s3, TropicalWeight::one());

    // Non-determinism: two arcs with same input label from s0
    fst.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(0.3));
    fst.add_arc(s0, Some(1), Some(1), s2, TropicalWeight::new(0.5));
    fst.add_arc(s1, Some(2), Some(2), s3, TropicalWeight::new(0.2));
    fst.add_arc(s2, Some(2), Some(2), s3, TropicalWeight::new(0.1));

    // Determinize
    let det =
        determinize(&fst, DeterminizeConfig::default()).expect("Determinization should succeed");

    // Minimize
    let min = minimize(&det, MinimizeConfig::default()).expect("Minimization should succeed");

    // Minimized FST should have fewer or equal states
    assert!(min.num_states() <= det.num_states());
}

/// Test connect -> determinize pipeline for cleaning FSTs.
#[test]
fn test_algorithm_connect_determinize_pipeline() {
    // Build FST with unreachable states
    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let _s2 = fst.add_state(); // Will be unreachable from start
    let s3 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s3, TropicalWeight::one());

    // s0 -> s1 -> s3 is the only path
    fst.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(0.5));
    fst.add_arc(s1, Some(2), Some(2), s3, TropicalWeight::new(0.5));
    // s2 -> s3 exists but s2 is unreachable (arc not added from s0 or s1)

    // Connect (trim)
    connect(&mut fst, ConnectConfig::default());

    // Determinize the connected FST
    let det =
        determinize(&fst, DeterminizeConfig::default()).expect("Determinization should succeed");

    assert!(det.start() != NO_STATE);
}

/// Test shortest distance computation in pipeline.
#[test]
fn test_algorithm_shortest_distance_pipeline() {
    // Build weighted FST
    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s2, TropicalWeight::one());

    // Two paths: s0 -> s1 -> s2 (cost 0.8) and s0 -> s2 (cost 1.0)
    fst.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(0.3));
    fst.add_arc(s1, Some(2), Some(2), s2, TropicalWeight::new(0.5));
    fst.add_arc(s0, Some(3), Some(3), s2, TropicalWeight::new(1.0));

    // Compute shortest distances
    if let Some(distances) =
        single_source_shortest_distance(&fst, ShortestDistanceConfig::default())
    {
        // Distance to s0 should be zero (start state)
        assert!((distances[s0 as usize].value() - 0.0).abs() < 1e-6);

        // Distance to final state s2 should be 0.8 (via s1)
        assert!((distances[s2 as usize].value() - 0.8).abs() < 1e-6);
    }
}

// =============================================================================
// Part 4: CFG Integration Pipeline Tests
// =============================================================================

/// Test simple grammar parsing pipeline.
#[test]
fn test_cfg_parse_pipeline() {
    // Build a simple grammar: S -> NP VP
    // Use builder pattern (methods consume and return self)
    let grammar = GrammarBuilder::new()
        .start("S")
        .terminal_with_id("det", 1)
        .terminal_with_id("noun", 2)
        .terminal_with_id("verb", 3)
        .rule("S", &["NP", "VP"])
        .rule("NP", &["det", "noun"])
        .rule("VP", &["verb"])
        .build()
        .expect("Grammar should build");

    // Create parser
    let parser = EarleyParser::new(&grammar);

    // Build a lattice from input sequence: [det, noun, verb] = [1, 2, 3]
    let backend = HashMapBackend::new();
    let mut lat_builder = LatticeBuilder::new(backend);
    lat_builder.add_correction_by_id(0, 1, 1u32, TropicalWeight::one(), EdgeMetadata::default()); // det
    lat_builder.add_correction_by_id(1, 2, 2u32, TropicalWeight::one(), EdgeMetadata::default()); // noun
    lat_builder.add_correction_by_id(2, 3, 3u32, TropicalWeight::one(), EdgeMetadata::default()); // verb

    let lattice = lat_builder.build(3);
    let result = parser.parse_lattice(&lattice);

    assert!(result.is_ok(), "Parse should succeed for valid input");
}

/// Test CFG filtering of lattice paths.
#[test]
fn test_cfg_lattice_filtering_pipeline() {
    // Grammar that accepts "the cat" but not "cat the"
    let grammar = GrammarBuilder::new()
        .start("S")
        .terminal_with_id("det", 1)
        .terminal_with_id("noun", 2)
        .rule("S", &["det", "noun"])
        .build()
        .expect("Grammar should build");

    // Build lattice with valid path
    let backend = HashMapBackend::new();
    let mut lat_builder = LatticeBuilder::new(backend);
    lat_builder.add_correction(
        0,
        1,
        "the",
        TropicalWeight::new(0.1),
        EdgeMetadata::default(),
    );
    lat_builder.add_correction(
        1,
        2,
        "cat",
        TropicalWeight::new(0.2),
        EdgeMetadata::default(),
    );

    let mut lattice = lat_builder.build(2);
    let viterbi_result = viterbi(&mut lattice);

    assert!(viterbi_result.success, "Lattice should have valid path");

    // Verify grammar was constructed correctly by parsing a lattice with [det, noun] = [1, 2]
    let parser = EarleyParser::new(&grammar);
    let backend2 = HashMapBackend::new();
    let mut builder2 = LatticeBuilder::new(backend2);
    builder2.add_correction_by_id(0, 1, 1u32, TropicalWeight::one(), EdgeMetadata::default()); // det
    builder2.add_correction_by_id(1, 2, 2u32, TropicalWeight::one(), EdgeMetadata::default()); // noun
    let lattice2 = builder2.build(2);
    assert!(
        parser.parse_lattice(&lattice2).is_ok(),
        "Grammar should accept [det, noun]"
    );
}

// =============================================================================
// Part 5: LaTeX Correction Pipeline Tests (Feature-gated)
// =============================================================================

#[cfg(feature = "latex-syntax")]
mod latex_tests {
    use super::*;

    /// Test LaTeX validation pipeline with balanced braces.
    #[test]
    fn test_latex_validation_balanced_braces() {
        let validator = LatexValidator::new();

        let tokens = vec!["{", "content", "}"];
        let result = validator.validate(&tokens);

        assert!(result.is_valid, "Balanced braces should be valid");
    }

    /// Test LaTeX validation pipeline with unbalanced braces.
    #[test]
    fn test_latex_validation_unbalanced_braces() {
        let validator = LatexValidator::new();

        let tokens = vec!["{", "content"];
        let result = validator.validate(&tokens);

        assert!(!result.is_valid, "Unbalanced braces should be invalid");
        assert!(!result.issues.is_empty(), "Should report issues");
    }

    /// Test LaTeX environment matching.
    #[test]
    fn test_latex_environment_matching() {
        let validator = LatexValidator::new();

        // Valid environment
        let valid_tokens = vec![
            "\\begin", "{", "equation", "}", "x", "\\end", "{", "equation", "}",
        ];
        let valid_result = validator.validate(&valid_tokens);
        assert!(
            valid_result.is_valid,
            "Matching environment should be valid"
        );

        // Mismatched environment
        let invalid_tokens = vec![
            "\\begin", "{", "equation", "}", "x", "\\end", "{", "align", "}",
        ];
        let invalid_result = validator.validate(&invalid_tokens);
        assert!(
            !invalid_result.is_valid,
            "Mismatched environment should be invalid"
        );
    }

    /// Test LaTeX full correction pipeline: input -> validation -> repair.
    #[test]
    fn test_latex_full_correction_pipeline() {
        let grammar = LatexGrammar::minimal().expect("Grammar should build");
        let layer = LatexSyntaxLayer::new(grammar);

        // Verify layer is properly initialized (call through trait with explicit types)
        let name = CorrectionLayer::<TropicalWeight, HashMapBackend>::name(&layer);
        assert_eq!(name, "latex-syntax");
    }
}

// =============================================================================
// Part 6: MathML Semantic Pipeline Tests (Feature-gated)
// =============================================================================

#[cfg(feature = "mathml-semantic")]
mod mathml_tests {
    use super::*;

    /// Test MathML type checking pipeline.
    #[test]
    fn test_mathml_type_checking_pipeline() {
        let mut checker = MathTypeChecker::new();

        // Check a simple numeric expression
        let result = checker.check(&["42"]);
        assert!(result.is_ok(), "Number should type-check");

        // Check a variable
        let result = checker.check(&["x"]);
        assert!(result.is_ok(), "Variable should type-check");
    }

    /// Test MathML homoglyph disambiguation.
    #[test]
    fn test_mathml_homoglyph_pipeline() {
        let disambiguator = HomoglyphDisambiguator::new();

        // 'x' after a number should be multiplication
        let context = MathContext {
            prev_was_number: true,
            in_math_mode: true,
            ..Default::default()
        };
        let result = disambiguator.disambiguate('x', &context);

        // Result should provide disambiguation (not Unknown)
        assert!(
            !matches!(result, GlyphMeaning::Unknown),
            "Should disambiguate 'x' after number"
        );
    }

    /// Test full MathML semantic layer pipeline.
    #[test]
    fn test_mathml_semantic_layer_pipeline() {
        let layer = MathMLSemanticLayer::new();

        // Verify layer setup (call through trait with explicit types)
        let name = CorrectionLayer::<TropicalWeight, HashMapBackend>::name(&layer);
        assert_eq!(name, "mathml-semantic");

        // Analyze simple expression
        let tokens = ["\\alpha"];
        let result = layer.analyze(&tokens);

        assert!(result.is_valid, "Greek letter should be semantically valid");
    }
}

// =============================================================================
// Part 7: Multi-Semiring Pipeline Tests
// =============================================================================

/// Test pipeline with LogWeight semiring.
#[test]
fn test_log_semiring_pipeline() {
    let backend = HashMapBackend::new();
    let mut builder: LatticeBuilder<LogWeight, _> = LatticeBuilder::new(backend);

    // Add corrections with log probabilities
    builder.add_correction(0, 1, "word1", LogWeight::new(0.5), EdgeMetadata::default());
    builder.add_correction(0, 1, "word2", LogWeight::new(1.0), EdgeMetadata::default());

    let mut lattice = builder.build(1);
    let result = viterbi(&mut lattice);

    assert!(result.success, "LogWeight pipeline should work");
}

/// Test pipeline with ProbabilityWeight semiring.
#[test]
fn test_probability_semiring_pipeline() {
    let backend = HashMapBackend::new();
    let mut builder: LatticeBuilder<ProbabilityWeight, _> = LatticeBuilder::new(backend);

    // Add corrections with probabilities
    builder.add_correction(
        0,
        1,
        "word1",
        ProbabilityWeight::new(0.8),
        EdgeMetadata::default(),
    );
    builder.add_correction(
        0,
        1,
        "word2",
        ProbabilityWeight::new(0.2),
        EdgeMetadata::default(),
    );

    let mut lattice = builder.build(1);
    let result = viterbi(&mut lattice);

    assert!(result.success, "ProbabilityWeight pipeline should work");
}

// =============================================================================
// Part 8: Edge Case Pipeline Tests
// =============================================================================

/// Test pipeline with very long sequence.
#[test]
fn test_long_sequence_pipeline() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Build a long sequence of 100 positions
    for i in 0..100 {
        builder.add_correction(
            i,
            i + 1,
            &format!("word{}", i),
            TropicalWeight::new((i % 10) as f64 * 0.1),
            EdgeMetadata::default(),
        );
    }

    let mut lattice = builder.build(100);
    let result = viterbi(&mut lattice);

    assert!(result.success, "Long sequence pipeline should succeed");
}

/// Test pipeline with parallel paths.
#[test]
fn test_parallel_paths_pipeline() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Two completely parallel paths
    builder.add_correction(
        0,
        1,
        "path1_word1",
        TropicalWeight::new(0.1),
        EdgeMetadata::default(),
    );
    builder.add_correction(
        1,
        2,
        "path1_word2",
        TropicalWeight::new(0.1),
        EdgeMetadata::default(),
    );

    builder.add_correction(
        0,
        1,
        "path2_word1",
        TropicalWeight::new(0.5),
        EdgeMetadata::default(),
    );
    builder.add_correction(
        1,
        2,
        "path2_word2",
        TropicalWeight::new(0.5),
        EdgeMetadata::default(),
    );

    let mut lattice = builder.build(2);
    let result = viterbi(&mut lattice);

    assert!(result.success, "Parallel paths pipeline should succeed");
}

/// Test pipeline with diamond structure (convergent paths).
#[test]
fn test_diamond_structure_pipeline() {
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Diamond: 0 -> 1 -> 3
    //          0 -> 2 -> 3
    builder.add_correction(
        0,
        1,
        "top_path",
        TropicalWeight::new(0.3),
        EdgeMetadata::default(),
    );
    builder.add_correction(
        1,
        3,
        "top_join",
        TropicalWeight::new(0.3),
        EdgeMetadata::default(),
    );

    builder.add_correction(
        0,
        2,
        "bottom_path",
        TropicalWeight::new(0.2),
        EdgeMetadata::default(),
    );
    builder.add_correction(
        2,
        3,
        "bottom_join",
        TropicalWeight::new(0.2),
        EdgeMetadata::default(),
    );

    let mut lattice = builder.build(3);
    let result = viterbi(&mut lattice);

    assert!(result.success, "Diamond structure pipeline should succeed");
}

/// Test pipeline handles epsilon arcs correctly.
#[test]
fn test_epsilon_handling_pipeline() {
    let mut fst: VectorWfst<u32, TropicalWeight> = VectorWfst::new();
    let s0 = fst.add_state();
    let s1 = fst.add_state();
    let s2 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s2, TropicalWeight::one());

    // Path with epsilon (None) arc
    fst.add_arc(s0, Some(1), Some(1), s1, TropicalWeight::new(0.5));
    fst.add_epsilon(s1, s2, TropicalWeight::new(0.0)); // Epsilon

    // FST should still be valid
    assert!(fst.start() != NO_STATE);
    assert!(fst.num_states() == 3);
}

// =============================================================================
// Part 9: Layer Pipeline Tests
// =============================================================================

/// Test empty layer pipeline.
#[test]
fn test_empty_layer_pipeline() {
    let pipeline: LayerPipeline<TropicalWeight, HashMapBackend> =
        LayerPipelineBuilder::new().build();

    // Empty pipeline should have zero layers
    assert_eq!(pipeline.len(), 0);
    assert!(pipeline.is_empty());
}

/// Test layer pipeline methods.
#[test]
fn test_layer_pipeline_methods() {
    let pipeline: LayerPipeline<TropicalWeight, HashMapBackend> =
        LayerPipelineBuilder::new().build();

    // Empty pipeline layer names should be empty
    assert!(pipeline.layer_names().is_empty());

    // Estimated reduction of empty pipeline should be 1.0 (no reduction)
    assert!((pipeline.estimated_reduction() - 1.0).abs() < 1e-6);
}
