//! # lling-llang
//!
//! A Weighted Finite State Transducer (WFST) framework for text normalization
//! and grammar correction.
//!
//! ## Overview
//!
//! lling-llang provides:
//! - **Semirings**: Algebraic weight structures (Tropical, Log, Boolean, Product)
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
    };
    pub use crate::wfst::{
        StateId, WeightedTransition, WfstState,
        Wfst, MutableWfst, LazyWfst, CachePolicy,
        VectorWfst, VectorWfstBuilder, LazyState, StateSource, LazyWfstWrapper,
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
}
