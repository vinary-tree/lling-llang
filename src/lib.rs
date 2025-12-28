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

pub mod semiring;
pub mod wfst;
pub mod backend;
pub mod lattice;
pub mod path;
pub mod composition;
pub mod cfg;
pub mod layers;
pub mod algorithms;
pub mod ctc;
pub mod differentiable;
pub mod optimization;
pub mod asr;
pub mod gpu;

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

// #[cfg(feature = "levenshtein")]
// pub mod integration;

/// Prelude for convenient imports.
pub mod prelude {
    pub use crate::semiring::{
        Semiring, DivisibleSemiring, StarSemiring,
        TropicalWeight, LogWeight, BoolWeight, ProductWeight,
        ProbabilityWeight, LeftStringWeight, RightStringWeight, ExpectationWeight,
    };
    pub use crate::wfst::{
        StateId, WeightedTransition, WfstState,
        Wfst, MutableWfst, LazyWfst, CachePolicy,
        VectorWfst, VectorWfstBuilder, LazyState, StateSource, LazyWfstWrapper,
        // Rational operations
        UnionSource, ConcatSource, ClosureSource,
        UnionWfst, ConcatWfst, ClosureWfst,
        union, concat, closure, closure_plus,
        // Unary operations
        InvertSource, ProjectSource,
        InvertWfst, ProjectInputWfst, ProjectOutputWfst,
        invert, project_input, project_output, reverse,
        // Synchronization
        StringDelay, SyncState, SyncSource, MutableSyncSource, SyncWfst,
        synchronize, synchronize_bounded, has_bounded_delay, compute_max_delay,
    };
    pub use crate::backend::{
        LatticeBackend, VocabId, HashMapBackend,
    };
    pub use crate::lattice::{
        NodeId, EdgeId, Node, Edge, EdgeMetadata,
        Lattice, LatticeBuilder, LatticePath, PathIterator,
    };
    pub use crate::path::{
        viterbi, nbest, beam_search,
        ViterbiResult, NBestIterator, BeamSearchConfig,
    };
    pub use crate::composition::{
        compose, LazyComposition, ComposedPath,
        EpsilonFilter, EpsilonFilterType, FilterState,
        LazyCfgComposition, FilteredLattice, ValidPathIterator,
        ParseState, CompositionStats,
    };
    pub use crate::cfg::{
        NonTerminal, Terminal, RuleId, Symbol, SymbolKind,
        Production, Grammar, GrammarError, GrammarBuilder,
        EarleyParser, EarleyState, EarleyChart, ParseError,
        ParseForest, ParseTree, ForestNodeId, ForestNode,
    };
    pub use crate::layers::{
        CorrectionLayer, LayerPipeline, LayerPipelineBuilder,
        LayerError, LayerResult, LayerStats,
        CfgFilterLayer,
    };
    pub use crate::algorithms::{
        ShortestDistanceQueue, FifoQueue, TopologicalQueue, ShortestFirstQueue,
        AutoQueue, QueueType, single_source_shortest_distance,
        single_source_shortest_distance_with_queue, all_pairs_shortest_distance,
        ShortestDistanceConfig,
    };
    pub use crate::ctc::{
        CtcTopology, CtcTopologyInfo, CtcLabel,
        correct_ctc, compact_ctc, minimal_ctc,
        selfless_correct_ctc, selfless_compact_ctc,
    };
    pub use crate::differentiable::{
        GradientWfst, ArcGradient, GradientAccumulator,
        forward_score, log_sum_exp_paths,
        viterbi_score, viterbi_path_with_grad, ViterbiGradResult,
        backward,
    };
    pub use crate::optimization::{
        prepare_for_beam_search, LogPushConfig, BeamSearchPrepResult,
        compute_log_potentials, apply_log_push,
        LookaheadTable, build_lookahead_table, LookaheadConfig,
    };
    pub use crate::asr::{
        ContextDependencyBuilder, TriphoneBuilder, TetraploneBuilder,
        ContextDependencyConfig, ContextState, PhoneId,
        NgramBuilder, NgramTransducer, NgramConfig,
        BackoffState, NgramOrder, NgramWeight,
        CascadeBuilder, AsrCascade, CascadeConfig,
        LexiconEntry, AuxiliarySymbol,
        chain_factor, ChainFactorConfig, ChainFactorResult,
        Chain, ChainId,
        rescore_lattice, RescoreConfig, RescoreResult,
        LatticeGrammar, RescorePass,
    };
    pub use crate::gpu::{
        // CSR representation
        CsrWfst, CsrBuilder, CsrArc, CsrState,
        csr_from_vector_wfst, csr_memory_size,
        // Token recombination
        PackedToken, TokenPacker, RecombinationBuffer,
        pack_cost_arc, unpack_cost_arc,
        // Load balancing
        WorkGroup, WorkDispatcher, LoadBalancer,
        WorkItem, WorkQueue,
        // K-vector reduction
        KVector, KVectorConfig, KVectorStats,
        reduce_with_k_vectors,
        // Channels/Lanes
        Channel, Lane, BatchedDecoder, DecoderConfig,
        ChannelState, LaneState,
        // Soft pruning
        SoftToken, SoftPruneConfig, SoftPruneBuffer,
        SoftPruneStats, AdaptiveBeam, SoftPruneManager,
    };
}
