//! # lling-llang
//!
//! A Weighted Finite State Transducer (WFST) framework for text normalization
//! and grammar correction.
//!
//! ## Overview
//!
//! lling-llang provides:
//! - **Semirings**: Algebraic weight structures (Tropical, Log, Probability, Boolean, Product, String, Expectation)
//! - **WFSTs**: Weighted finite state transducers with composition operators
//! - **Lattices**: Weighted DAGs for representing correction alternatives
//! - **CFG Parsing**: Earley parser modified for lattice input
//! - **Extensible Layers**: Plugin architecture for correction pipelines
//!
//! ## Feature Flags
//!
//! Core:
//! - `default`: Standalone WFST framework with no external dependencies
//! - `levenshtein`: Integration with liblevenshtein for lexical correction
//! - `lattice`: Semiring↔lattice bridge — `lling-llang` semirings as
//!   `libdictenstein` dictionary values (via `llattice`)
//! - `lattice-persistent`: serde-bounded dictionary values for disk-backed
//!   (`persistent-artrie`) dictionaries
//! - `pcfg`: Probabilistic context-free grammar support
//! - `error-grammar`: Predefined error grammars
//!
//! Extension layers:
//! - `pos-tagging`: POS-tagging correction layer
//! - `lm-rerank`: Language-model reranking layer
//! - `phonetic-rescore`: Phonetic rescoring layer (requires `levenshtein`)
//! - `code-correction`: Pattern-aware code syntax-recovery layer
//! - `latex-syntax`: LaTeX syntax-correction layer
//! - `mathml-semantic`: MathML semantic / homoglyph layer
//!
//! F1R3FLY.io integration:
//! - `f1r3fly`: Full F1R3FLY.io stack (PathMap, MORK, MeTTaTron, MeTTaIL)
//! - `sexpr`: S-expression path format for MORK compatibility
//! - `pathmap-backend`: PathMap-optimized lattice backend
//!
//! Serialization & testing:
//! - `serde`: Serialization support
//! - `bincode-ser`: Bincode serialization (implies `serde`)
//! - `test-utils`: Expose the `test_utils` module (proptest strategies, fixtures)
//!   to downstream crates
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Correction Layer Stack                           │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Layer N: [User-Defined]           ← Implement CorrectionLayer trait    │
//! │     ↑                                                                   │
//! │  Layer 3: CFG Grammar              ← Syntactic filtering                │
//! │     ↑                                                                   │
//! │  Layer 1: Lexical Correction       ← Levenshtein + phonetic candidates  │
//! │     ↑                                                                   │
//! │  [Input Lattice]                                                        │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```
//! use lling_llang::prelude::*;
//!
//! // Build a correction lattice over a vocabulary-interning backend.
//! let backend = HashMapBackend::new();
//! let mut builder = LatticeBuilder::<TropicalWeight, _>::new(backend);
//! builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::original());
//! builder.add_correction(0, 1, "teh", TropicalWeight::new(0.0), EdgeMetadata::correction(1));
//! let mut lattice = builder.build(1);
//!
//! // Extract the best (Viterbi) path: ⊕ = min over paths, ⊗ = + along a path.
//! let result = viterbi(&mut lattice);
//! assert!(result.success);
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
// === Clippy policy allows ===
// Generic-semiring code uses explicit `.clone()` to document intent and to
// stay correct when a future Semiring impl is Clone-but-not-Copy. Concrete
// semirings (LogWeight, TropicalWeight, ...) all happen to be Copy today.
#![allow(clippy::clone_on_copy)]
// `&*x` and `&x` patterns are common in this crate's iterator-heavy code
// where adding/removing borrows would require touching many call sites.
#![allow(clippy::needless_borrow)]
// Range-indexed loops are preferred over `iter().enumerate()` in numeric
// algorithm code where the index is the primary value (alpha[s], distances[i]).
#![allow(clippy::needless_range_loop)]
// Very-complex types appear in lazy-WFST plumbing where the type alias would
// obscure rather than clarify.
#![allow(clippy::type_complexity)]
// `if let Some(x) = ...` nested in `match` arms is often the clearest way to
// express weighted-transition decisions in algorithm code; collapsing them
// makes the algebra harder to read.
#![allow(clippy::collapsible_if, clippy::collapsible_match)]
// `(x + n - 1) / n` and `x % n == 0` are the textbook idioms in this crate's
// algorithmic code, often appearing inside comments referencing the formula.
#![allow(clippy::manual_div_ceil, clippy::manual_is_multiple_of)]
// `s[1..]` / `&s[..s.len()-1]` patterns appear in tight tokenization loops
// where the manual form matches the surrounding indexing arithmetic.
#![allow(clippy::manual_strip)]
// `x as u32` on values already typed as `u32` survives generic refactors
// (e.g. `StateId` aliasing) and documents the intent at the call site.
#![allow(clippy::unnecessary_cast)]
// `Foo { ..Default::default() }` is more readable than full struct init for
// many config types in this crate.
#![allow(clippy::field_reassign_with_default)]
// `0..=255u8` and similar appear as `b'\0'..=b'\xFF'` deliberately to spell
// out the full byte range.
#![allow(clippy::almost_complete_range)]
// `iter().enumerate().map(|(_, x)| ...)` survives index-related refactors.
#![allow(clippy::unused_enumerate_index)]
// `or_insert_with(Vec::new)` is identical to `or_default()` but reads as
// "insert an empty Vec", which matches the surrounding code in this crate.
#![allow(clippy::unwrap_or_default)]
// `>= n + 1` patterns appear in inequality chains where keeping the symmetric
// form aids legibility.
#![allow(clippy::int_plus_one)]
// Boolean-comparison-to-true patterns occur in proptest predicates where the
// explicit form documents that the value is a bool, not e.g. an Option<bool>.
#![allow(clippy::bool_comparison, clippy::nonminimal_bool)]
// `.contains_key + .insert` is structurally a `.entry().or_insert()` pair but
// the explicit form makes the absence path observable to the reader.
#![allow(clippy::map_entry)]
// `.expect(format!(...))` in tests is legible; the lazy_format dance is noise.
#![allow(clippy::expect_fun_call)]
// `manual RangeInclusive::contains` matches the surrounding comparison style.
#![allow(clippy::manual_range_contains)]
// `Default::default` redundant closures are fine; explicit constructor calls
// document the type being defaulted.
#![allow(clippy::redundant_closure)]
// `from_str` on inherent impls is a deliberate API choice (some types support
// fallible parsing via Result and a separate non-FromStr signature).
#![allow(clippy::should_implement_trait)]
// Remaining stylistic lints that are deliberate codebase patterns:
#![allow(
    // `for k in map.iter()` over `for k in map.keys()` documents that the value is intentionally ignored.
    clippy::for_kv_map,
    // `.iter().any(|x| x == &needle)` documents the comparator; `contains` hides it for non-Copy types.
    clippy::manual_contains,
    // `* 1.0` and similar appear in benchmark fixtures as load-bearing scaffolding.
    clippy::no_effect,
    // `map_or` chains stay because the call sites compose with other map_or chains.
    clippy::unnecessary_map_or,
    // Wide-arity builder/decoder functions are part of public API; renaming/grouping would break callers.
    clippy::too_many_arguments,
    // `vec![x].clone()` in tests reads more naturally than `std::slice::from_ref`.
    clippy::single_element_loop, clippy::redundant_slicing,
    // Reference-of-both-operands patterns appear in proptest predicates where keeping both sides borrowed avoids move warnings.
    clippy::op_ref,
    // Internal `module/module.rs` layouts (e.g. lattice/lattice.rs) are intentional for the public type sharing the module name.
    clippy::module_inception,
    // `match Option { Some(x) => x, None => Default::default() }` documents intent better than `unwrap_or_default`.
    clippy::manual_unwrap_or_default,
    // `(x as char) == ...` casts appear in tokenization for documentation purposes.
    clippy::single_char_pattern,
    // sort_by closure pattern is fine when the key extraction has side-effect-free arithmetic.
    clippy::unnecessary_sort_by,
    // `.max(lo).min(hi)` vs `.clamp(lo, hi)` is a wash; both are legible.
    clippy::manual_clamp,
    // `text.len() == 1` reads more naturally than `text.chars().count() == 1` for ASCII inputs.
    clippy::comparison_to_empty,
)]
// Doc-format lints triggered by intentional README-style markdown in module docs.
#![allow(
    rustdoc::redundant_explicit_links,
    clippy::doc_overindented_list_items,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments
)]

pub mod acoustic;
pub mod algorithms;
pub mod asr;
pub mod backend;
pub mod cfg;
pub mod composition;
pub mod ctc;
pub mod differentiable;
pub mod error_models;
pub mod gpu;
pub mod lattice;
#[cfg(feature = "lattice")]
pub mod lattice_bridge;
pub mod layers;
pub mod llm;
pub mod multilingual;
pub mod multitape;
pub mod optimization;
pub mod path;
pub mod programming;
pub mod pushdown;
pub mod semiring;
pub mod simd;
pub mod subsequential;
/// Symbolic-automata + algebra-tower core (SFA/SFT, Boolean/Heyting/RejectSafe
/// algebra tower, `ConstraintTheory`/`TheoryAlgebra`, behavioral algebra, Presburger),
/// hoisted from prattail (Task #21 / ADR-018) as the shared foundational home so
/// prattail, pgmcp, the constrained decoder, and the WFST sidecar all depend on it here.
pub mod symbolic;
pub mod text_processing;
pub mod training;
pub mod transducer;
pub mod tree_transducers;
pub mod wfst;

/// Test utilities for property-based testing and assertions.
///
/// This module provides `proptest` strategies, custom assertions, and
/// common fixtures for testing WFSTs and semirings.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

// #[cfg(feature = "error-grammar")]
// pub mod error_grammar;

// #[cfg(feature = "sexpr")]
// pub mod sexpr;

// #[cfg(feature = "f1r3fly")]
// pub mod storage;

#[cfg(feature = "levenshtein")]
pub mod integration;

/// Prelude for convenient imports.
pub mod prelude {
    pub use crate::acoustic::{
        // Score fusion
        AcousticLanguageModel,
        // Core trait
        AcousticModel,
        // Posteriors
        FramePosterior,
        FusionConfig,
        HmmStateId,
        PosteriorSequence,
        TransitionLogProb,
        // HMM topology
        TransitionMatrix,
        UnitId,
    };
    pub use crate::algorithms::{
        all_pairs_shortest_distance, single_source_shortest_distance,
        single_source_shortest_distance_with_queue, AutoQueue, FifoQueue, QueueType,
        ShortestDistanceConfig, ShortestDistanceQueue, ShortestFirstQueue, TopologicalQueue,
    };
    pub use crate::asr::{
        chain_factor,
        rescore_lattice,
        AsrCascade,
        AuxiliarySymbol,
        BackoffState,
        CascadeBuilder,
        CascadeConfig,
        Chain,
        ChainFactorConfig,
        ChainFactorResult,
        ChainId,
        ContextDependencyBuilder,
        ContextDependencyConfig,
        ContextDependencyError,
        ContextState,
        DysfluencyConfig,
        DysfluencyDetector,
        // Dysfluency detection
        DysfluencyPattern,
        DysfluencySpan,
        LatticeGrammar,
        LexiconEntry,
        NgramBuilder,
        NgramConfig,
        NgramOrder,
        NgramTransducer,
        NgramWeight,
        PhoneId,
        RescoreConfig,
        RescorePass,
        RescoreResult,
        SyllableRepetitionBuilder,
        TetraploneBuilder,
        TriphoneBuilder,
        WordRepetitionBuilder,
    };
    pub use crate::backend::{HashMapBackend, LatticeBackend, VocabId};
    pub use crate::cfg::{
        EarleyChart, EarleyParser, EarleyState, ForestNode, ForestNodeId, Grammar, GrammarBuilder,
        GrammarError, NonTerminal, ParseError, ParseForest, ParseTree, Production, RuleId, Symbol,
        SymbolKind, Terminal,
    };
    pub use crate::composition::{
        compose, ComposedPath, CompositionStats, EpsilonFilter, EpsilonFilterType, FilterState,
        FilteredLattice, LazyCfgComposition, LazyComposition, ParseState, ValidPathIterator,
    };
    pub use crate::ctc::{
        compact_ctc,
        correct_ctc,
        minimal_ctc,
        selfless_compact_ctc,
        selfless_correct_ctc,
        // CTC decoding
        CtcDecoder,
        CtcDecoderConfig,
        CtcLabel,
        CtcTopology,
        CtcTopologyInfo,
        DecodingError,
        DecodingResult,
        DecodingStats,
        ObservationFst,
        StreamingCtcDecoder,
        BLANK,
    };
    pub use crate::differentiable::{
        backward, forward_score, log_sum_exp_paths, viterbi_path_with_grad, viterbi_score,
        ArcGradient, GradientAccumulator, GradientWfst, ViterbiGradResult,
    };
    pub use crate::gpu::{
        csr_from_vector_wfst,
        csr_memory_size,
        pack_cost_arc,
        reduce_with_k_vectors,
        unpack_cost_arc,
        AdaptiveBeam,
        BatchedDecoder,
        // Channels/Lanes
        Channel,
        ChannelState,
        CsrArc,
        CsrBuilder,
        CsrState,
        // CSR representation
        CsrWfst,
        DecoderConfig,
        // K-vector reduction
        KVector,
        KVectorConfig,
        KVectorStats,
        Lane,
        LaneState,
        LoadBalancer,
        // Token recombination
        PackedToken,
        RecombinationBuffer,
        SoftPruneBuffer,
        SoftPruneConfig,
        SoftPruneManager,
        SoftPruneStats,
        // Soft pruning
        SoftToken,
        TokenPacker,
        WorkDispatcher,
        // Load balancing
        WorkGroup,
        WorkItem,
        WorkQueue,
    };
    pub use crate::lattice::{
        Edge, EdgeId, EdgeMetadata, Lattice, LatticeBuilder, LatticePath, Node, NodeId,
        PathIterator,
    };
    pub use crate::layers::{
        CfgFilterLayer,
        // Confusion layer
        ConfusionLayer,
        ConfusionLayerConfig,
        ConfusionMatrix,
        CorrectionLayer,
        LayerError,
        LayerPipeline,
        LayerPipelineBuilder,
        LayerResult,
        LayerStats,
    };
    pub use crate::multilingual::{
        CodeSwitchBuilder,
        CodeSwitchConfig,
        CodeSwitchPath,
        // Code-switching transducer
        CodeSwitchTransducer,
        DetectionResult,
        LanguageConfig,
        LanguageDetector,
        // Language types
        LanguageId,
        LanguageModel,
        LanguageSpan,
        SimpleLanguageModel,
        SwitchPoint,
        WordProbability,
    };
    pub use crate::multitape::{
        // Labels and transitions
        MultiTapeLabel,
        MultiTapeState,
        MultiTapeTransition,
        // Traits
        MultiTapeWfst,
        // Builder
        MultiTapeWfstBuilder,
        // Projection and synchronization
        ProjectedWfst,
        SyncConfig,
        SynchronizedMultiTape,
        TapeDelay,
        // Implementations
        VectorMultiTapeWfst,
    };
    pub use crate::optimization::{
        apply_log_push, build_lookahead_table, compute_log_potentials, prepare_for_beam_search,
        BeamSearchPrepResult, LogPushConfig, LookaheadConfig, LookaheadTable,
    };
    pub use crate::path::{
        beam_search, nbest, viterbi, BeamSearchConfig, NBestIterator, ViterbiResult,
    };
    pub use crate::programming::{
        ApiMigrationBuilder,
        ApiMigrationRule,
        ApiMigrationTransducer,
        MigrationResult,
        MigrationStats,
        MigrationType,
        NodeKind,
        ParseResult,
        // Parser backend traits
        ParserBackend,
        ParserError,
        PatternMatcher,
        Position,
        Range,
        RepairAction,
        RepairCandidate,
        ReplacementAction,
        SyntaxNode,
        SyntaxNodeRef,
        SyntaxRepairBuilder,
        SyntaxRepairCosts,
        // Syntax repair
        SyntaxRepairRule,
        SyntaxRepairTransducer,
        // Token patterns
        Token,
        TokenKind,
        TokenPattern,
        TokenPredicate,
        TokenReplacement,
        // API migration
        Version,
        VersionRange,
    };
    pub use crate::pushdown::{
        PdaAcceptMode,
        // Builder
        PdaBuilder,
        PdaConfiguration,
        PdaState,
        // Transitions
        PdaTransition,
        StackAction,
        // Stack operations
        StackSymbol,
        // Implementations
        VectorPda,
        // Traits
        WeightedPda,
    };
    pub use crate::semiring::{
        BoolWeight, DivisibleSemiring, ExpectationWeight, FallibleStarSemiring, GodelWeight,
        LeftStringWeight, LogWeight, ProbabilityWeight, ProductWeight, RightStringWeight, Semiring,
        SignedTropicalWeight, StarDivergenceError, StarSemiring, TropicalWeight,
    };
    pub use crate::subsequential::{
        DecompositionStats,
        PiecewiseBuilder,
        PiecewiseSubsequential,
        // Subsequential transducers
        SubsequentialTransducer,
    };
    pub use crate::tree_transducers::{
        // Ranked alphabet
        RankedAlphabet,
        SimpleAlphabet,
        Symbol as RankedSymbol,
        TransducerState,
        // Tree data structure
        Tree,
        TreeChild,
        TreeNode,
        TreePattern,
        // Rules and patterns
        TreeRule,
        // Builder
        TreeTransducerBuilder,
        TreeTransducerOps,
        VectorTreeTransducer,
        // Transducer trait and implementations
        WeightedTreeTransducer,
    };
    pub use crate::wfst::{
        closure,
        closure_plus,
        compute_max_delay,
        concat,
        has_bounded_delay,
        invert,
        project_input,
        project_output,
        reverse,
        synchronize,
        synchronize_bounded,
        union,
        CachePolicy,
        ClosureSource,
        ClosureWfst,
        ConcatSource,
        ConcatWfst,
        // Unary operations
        InvertSource,
        InvertWfst,
        LazyState,
        LazyWfst,
        LazyWfstWrapper,
        MutableSyncSource,
        MutableWfst,
        ProjectInputWfst,
        ProjectOutputWfst,
        ProjectSource,
        StateId,
        StateSource,
        // Synchronization
        StringDelay,
        SyncSource,
        SyncState,
        SyncWfst,
        // Rational operations
        UnionSource,
        UnionWfst,
        VectorWfst,
        VectorWfstBuilder,
        WeightedTransition,
        Wfst,
        WfstState,
    };
}
