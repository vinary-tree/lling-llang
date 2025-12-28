//! Optimization algorithms for WFSTs.
//!
//! This module provides specialized optimization techniques identified from
//! research on WFST-based systems, particularly for speech recognition.
//!
//! ## Overview
//!
//! | Optimization | Description | Speedup |
//! |--------------|-------------|---------|
//! | Log-Semiring Pushing | Stochastic normalization for beam search | Up to 18× |
//! | Token Grouping | Lazy evaluation for on-the-fly composition | 10-20× fewer ops |
//! | N-gram Back-off | Compact LM representation | Avoids O(|V|²) |
//!
//! ## Log-Semiring Weight Pushing
//!
//! Weight pushing in the **log semiring** (not tropical!) has a significant impact
//! on beam search pruning efficacy. Unlike tropical pushing which uses min-weight
//! potentials, log pushing uses the sum of all path probabilities, creating a
//! stochastic automaton where weights at each state sum to 1.
//!
//! This "synchronizes" acoustic likelihoods with transducer probabilities, providing
//! optimal likelihood ratio decisions for pruning.
//!
//! ### Reference
//!
//! Mohri, Pereira, Riley (2002): "Weight pushing in the log semiring has a very
//! large beneficial impact on the pruning efficacy of a standard Viterbi beam search"
//!
//! ## Token Grouping (LET-Decoder)
//!
//! For on-the-fly composition scenarios, tokens with the same HCLG-state but different
//! grammar states can be grouped together. Expansion is deferred until word boundaries,
//! avoiding redundant operations for tokens that will be pruned anyway.
//!
//! ### Reference
//!
//! Lv et al. (2023): "LET-Decoder: Lazy-evaluation Token-group Decoder"
//!
//! ## N-gram Back-off Structure
//!
//! For large vocabulary language models, directly representing all n-grams creates
//! O(|V|²) transitions. Using back-off states with ε-transitions to lower-order
//! n-grams keeps the graph compact while preserving the language model distribution.

pub mod log_push;
pub mod lookahead;
pub mod token_group;
pub mod ngram_backoff;

pub use log_push::{
    prepare_for_beam_search, LogPushConfig, BeamSearchPrepResult,
    compute_log_potentials, apply_log_push,
};
pub use lookahead::{
    LookaheadTable, build_lookahead_table, LookaheadConfig,
};
pub use token_group::{
    Token, TokenId, ArcId, GroupLink, TokenGroupId,
    TokenGroup, TokenGroupPool, BucketQueue,
    TokenGroupConfig, TokenGroupStats, GroupedFrame,
    TokenGroupManager,
};
pub use ngram_backoff::{
    VocabId, UNK_ID, BOS_ID, EOS_ID,
    NgramEntry, BackoffWeight, NgramLmConfig, NgramLmBuilder, NgramStats,
    BigramLm, BigramStats, PruningStrategy,
    compute_size_reduction, SizeReduction,
};
