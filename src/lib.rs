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
//! - `default`: Standalone WFST framework with no external dependencies
//! - `levenshtein`: Integration with liblevenshtein for lexical correction
//! - `pos-tagging`: POS tagging layer support
//! - `lm-rerank`: Language model reranking layer support
//! - `f1r3fly`: Full F1R3FLY.io integration (PathMap, MORK, MeTTaTron, MeTTaIL)
//! - `sexpr`: S-expression path format for MORK compatibility
//! - `serde`: Serialization support
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
//! ```rust,ignore
//! use lling_llang::prelude::*;
//!
//! // Build a correction lattice
//! let mut builder = LatticeBuilder::<TropicalWeight, _>::new(backend);
//! builder.add_correction(0, 1, "the", TropicalWeight(1.0), EdgeMetadata::default());
//! builder.add_correction(0, 1, "teh", TropicalWeight(0.0), EdgeMetadata::default());
//! let lattice = builder.build(2);
//!
//! // Extract best path
//! let best = lattice.best_path();
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

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
