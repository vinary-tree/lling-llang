//! Token graph variants for CTC-like training.
//!
//! This module provides various token graph constructions that encode
//! different prior assumptions about label alignments in sequence-to-sequence
//! training.
//!
//! ## Token Graph Variants
//!
//! 1. **Standard CTC**: Allows any number of blank/non-blank repetitions
//! 2. **Spike CTC**: Single emission per non-blank token
//! 3. **Duration-Limited CTC**: Limits token duration to 1-2 frames
//! 4. **Equally Spaced CTC**: Fixed distance between non-blank tokens
//!
//! ## Prior Encoding
//!
//! The token graph encodes prior beliefs about:
//! - Token duration distribution
//! - Alignment sparsity
//! - Temporal spacing of emissions
//!
//! Different priors suit different data characteristics:
//! - Tight handwriting → shorter token durations → Spike CTC
//! - Loose handwriting → longer durations → Standard CTC
//!
//! ## References
//!
//! - Hannun et al., "Differentiable Weighted Finite-State Transducers" (ICLR 2021)
//! - Collobert et al., "Wav2Letter" (2016)

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{StateId, VectorWfst, MutableWfst, Wfst};

/// Token identifier type.
pub type TokenId = u32;

/// Blank token constant (typically 0).
pub const BLANK_TOKEN: TokenId = 0;

/// Token graph type for different CTC variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenGraphType {
    /// Standard CTC with unlimited repetitions.
    Standard,
    /// Spike CTC: single emission per token.
    Spike,
    /// Duration-limited CTC with max duration.
    DurationLimited { max_duration: usize },
    /// Equally spaced CTC with fixed blank count between tokens.
    EquallySpaced { blank_count: usize },
}

/// Configuration for token graph construction.
#[derive(Clone, Debug)]
pub struct TokenGraphConfig {
    /// Type of token graph to construct.
    pub graph_type: TokenGraphType,
    /// Whether to include blank token.
    pub include_blank: bool,
    /// Blank token ID.
    pub blank_id: TokenId,
    /// Initial weight for transitions.
    pub init_weight: f64,
}

impl Default for TokenGraphConfig {
    fn default() -> Self {
        Self {
            graph_type: TokenGraphType::Standard,
            include_blank: true,
            blank_id: BLANK_TOKEN,
            init_weight: 0.0,
        }
    }
}

/// Build a token graph for a single token.
///
/// # Arguments
///
/// * `token` - The token ID
/// * `config` - Configuration for the token graph
///
/// # Returns
///
/// A WFST representing the token graph.
pub fn build_token_graph(token: TokenId, config: &TokenGraphConfig) -> VectorWfst<TokenId, LogWeight> {
    match config.graph_type {
        TokenGraphType::Standard => build_standard_token_graph(token, config),
        TokenGraphType::Spike => build_spike_token_graph(token, config),
        TokenGraphType::DurationLimited { max_duration } => {
            build_duration_limited_token_graph(token, max_duration, config)
        }
        TokenGraphType::EquallySpaced { blank_count } => {
            build_equally_spaced_token_graph(token, blank_count, config)
        }
    }
}

/// Build standard CTC token graph.
///
/// Structure:
/// ```text
///     ε:ε (self-loop)
///        ↓
/// 0 --a:a--> 1
///        ↑
///     a:ε (self-loop)
/// ```
///
/// Allows any number of repetitions.
fn build_standard_token_graph(token: TokenId, config: &TokenGraphConfig) -> VectorWfst<TokenId, LogWeight> {
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s1, LogWeight::one());

    // Main transition: token:token
    fst.add_arc(s0, Some(token), Some(token), s1, LogWeight::new(config.init_weight));

    // Self-loop on s1 for repetitions: token:ε
    fst.add_arc(s1, Some(token), None, s1, LogWeight::new(config.init_weight));

    // Optional blank handling
    if config.include_blank {
        // Self-loop on s0 for leading blanks
        fst.add_arc(s0, Some(config.blank_id), None, s0, LogWeight::new(config.init_weight));
        // Self-loop on s1 for trailing blanks
        fst.add_arc(s1, Some(config.blank_id), None, s1, LogWeight::new(config.init_weight));
    }

    fst
}

/// Build spike CTC token graph.
///
/// Structure:
/// ```text
/// 0 --a:a--> 1
/// ```
///
/// Only allows single emission per token.
fn build_spike_token_graph(token: TokenId, config: &TokenGraphConfig) -> VectorWfst<TokenId, LogWeight> {
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    let s1 = fst.add_state();

    fst.set_start(s0);
    fst.set_final(s1, LogWeight::one());

    // Single transition: token:token
    fst.add_arc(s0, Some(token), Some(token), s1, LogWeight::new(config.init_weight));

    // Optional blank handling - only at boundaries
    if config.include_blank {
        // Blanks before token
        fst.add_arc(s0, Some(config.blank_id), None, s0, LogWeight::new(config.init_weight));
        // Blanks after token
        fst.add_arc(s1, Some(config.blank_id), None, s1, LogWeight::new(config.init_weight));
    }

    fst
}

/// Build duration-limited CTC token graph.
///
/// Structure for max_duration=2:
/// ```text
/// 0 --a:a--> 1 --a:ε--> 2
/// ```
///
/// Limits token duration to specified maximum.
fn build_duration_limited_token_graph(
    token: TokenId,
    max_duration: usize,
    config: &TokenGraphConfig,
) -> VectorWfst<TokenId, LogWeight> {
    let mut fst = VectorWfst::new();

    // Create states: 0 (start), 1 to max_duration (emissions)
    let mut states = Vec::with_capacity(max_duration + 1);
    for _ in 0..=max_duration {
        states.push(fst.add_state());
    }

    fst.set_start(states[0]);
    fst.set_final(states[max_duration], LogWeight::one());

    // First transition emits the token
    fst.add_arc(
        states[0],
        Some(token),
        Some(token),
        states[1],
        LogWeight::new(config.init_weight),
    );

    // Subsequent transitions are repetitions (token:ε)
    for i in 1..max_duration {
        fst.add_arc(
            states[i],
            Some(token),
            None,
            states[i + 1],
            LogWeight::new(config.init_weight),
        );

        // Also make intermediate states final
        fst.set_final(states[i], LogWeight::one());
    }

    // Optional blank handling
    if config.include_blank {
        // Blanks at start
        fst.add_arc(
            states[0],
            Some(config.blank_id),
            None,
            states[0],
            LogWeight::new(config.init_weight),
        );
        // Blanks at end
        fst.add_arc(
            states[max_duration],
            Some(config.blank_id),
            None,
            states[max_duration],
            LogWeight::new(config.init_weight),
        );
    }

    fst
}

/// Build equally spaced CTC token graph.
///
/// Structure for blank_count=2:
/// ```text
/// 0 --a:a--> 1 --<blank>:ε--> 2 --<blank>:ε--> 3
/// ```
///
/// Requires fixed number of blanks between tokens.
fn build_equally_spaced_token_graph(
    token: TokenId,
    blank_count: usize,
    config: &TokenGraphConfig,
) -> VectorWfst<TokenId, LogWeight> {
    let mut fst = VectorWfst::new();

    // States: 0 (start), 1 (after token), 2..blank_count+1 (blanks)
    let num_states = blank_count + 2;
    let mut states = Vec::with_capacity(num_states);
    for _ in 0..num_states {
        states.push(fst.add_state());
    }

    fst.set_start(states[0]);
    fst.set_final(states[num_states - 1], LogWeight::one());

    // Token emission
    fst.add_arc(
        states[0],
        Some(token),
        Some(token),
        states[1],
        LogWeight::new(config.init_weight),
    );

    // Required blanks
    for i in 1..=blank_count {
        fst.add_arc(
            states[i],
            Some(config.blank_id),
            None,
            states[i + 1],
            LogWeight::new(config.init_weight),
        );
    }

    // Also allow empty (for end of sequence)
    fst.set_final(states[1], LogWeight::one());

    fst
}

/// Build a complete token vocabulary graph.
///
/// Creates the union of token graphs for all tokens in the vocabulary.
///
/// # Arguments
///
/// * `vocab_size` - Number of tokens (excluding blank if separate)
/// * `config` - Configuration for token graphs
///
/// # Returns
///
/// A WFST representing (T₁ + T₂ + ... + T_n)* where T_i is the token graph for token i.
pub fn build_vocabulary_graph(
    vocab_size: usize,
    config: &TokenGraphConfig,
) -> VectorWfst<TokenId, LogWeight> {
    let mut fst = VectorWfst::new();

    // Create start state (also final for empty sequence)
    let start = fst.add_state();
    fst.set_start(start);
    fst.set_final(start, LogWeight::one());

    // Start token ID (skip blank if it's ID 0)
    let start_token = if config.include_blank { 1 } else { 0 };

    // Add each token's graph
    for token_id in start_token..(start_token + vocab_size as TokenId) {
        let token_graph = build_token_graph(token_id, config);

        // Embed token graph into main FST
        // Map states: token_graph.start -> new state, etc.
        let state_offset = fst.num_states() as StateId;

        // Add states for this token graph
        for _ in 0..token_graph.num_states() {
            fst.add_state();
        }

        // Copy arcs with state remapping
        for s in 0..token_graph.num_states() as StateId {
            for arc in token_graph.transitions(s) {
                fst.add_arc(
                    s + state_offset,
                    arc.input.clone(),
                    arc.output.clone(),
                    arc.to + state_offset,
                    arc.weight,
                );
            }
        }

        // Connect main start to token graph start
        let token_start = token_graph.start() + state_offset;
        fst.add_arc(start, None, None, token_start, LogWeight::one());

        // Connect token graph final states back to main start (for closure)
        for s in 0..token_graph.num_states() as StateId {
            if token_graph.is_final(s) {
                let mapped_state = s + state_offset;
                fst.add_arc(mapped_state, None, None, start, token_graph.final_weight(s));
            }
        }
    }

    fst
}

/// Build a blank graph for CTC.
///
/// Creates a graph that accepts only blank tokens.
pub fn build_blank_graph(config: &TokenGraphConfig) -> VectorWfst<TokenId, LogWeight> {
    let mut fst = VectorWfst::new();

    let s0 = fst.add_state();
    fst.set_start(s0);
    fst.set_final(s0, LogWeight::one());

    // Self-loop for blanks
    fst.add_arc(
        s0,
        Some(config.blank_id),
        None,
        s0,
        LogWeight::new(config.init_weight),
    );

    fst
}

/// Statistics about token graphs.
#[derive(Clone, Debug, Default)]
pub struct TokenGraphStats {
    /// Number of states in the graph.
    pub num_states: usize,
    /// Number of arcs in the graph.
    pub num_arcs: usize,
    /// Graph type used.
    pub graph_type: Option<TokenGraphType>,
}

impl TokenGraphStats {
    /// Compute statistics for a token graph.
    pub fn from_wfst<L: Clone + Send + Sync>(fst: &VectorWfst<L, LogWeight>) -> Self {
        let num_states = fst.num_states();
        let num_arcs: usize = (0..num_states as StateId)
            .map(|s| fst.transitions(s).len())
            .sum();

        Self {
            num_states,
            num_arcs,
            graph_type: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::NO_STATE;

    #[test]
    fn test_token_graph_config_default() {
        let config = TokenGraphConfig::default();
        assert_eq!(config.graph_type, TokenGraphType::Standard);
        assert!(config.include_blank);
        assert_eq!(config.blank_id, BLANK_TOKEN);
    }

    #[test]
    fn test_standard_token_graph() {
        let config = TokenGraphConfig::default();
        let graph = build_token_graph(1, &config);

        assert!(graph.start() != NO_STATE);
        assert!(graph.num_states() >= 2);

        // Should have self-loops for repetitions
        let stats = TokenGraphStats::from_wfst(&graph);
        assert!(stats.num_arcs >= 2);
    }

    #[test]
    fn test_spike_token_graph() {
        let config = TokenGraphConfig {
            graph_type: TokenGraphType::Spike,
            ..Default::default()
        };
        let graph = build_token_graph(1, &config);

        // Spike has no repetition self-loop on the token itself
        assert_eq!(graph.num_states(), 2);
    }

    #[test]
    fn test_duration_limited_token_graph() {
        let config = TokenGraphConfig {
            graph_type: TokenGraphType::DurationLimited { max_duration: 3 },
            include_blank: false,
            ..Default::default()
        };
        let graph = build_token_graph(1, &config);

        // Should have max_duration + 1 states
        assert_eq!(graph.num_states(), 4);
    }

    #[test]
    fn test_equally_spaced_token_graph() {
        let config = TokenGraphConfig {
            graph_type: TokenGraphType::EquallySpaced { blank_count: 2 },
            ..Default::default()
        };
        let graph = build_token_graph(1, &config);

        // Should have blank_count + 2 states
        assert_eq!(graph.num_states(), 4);
    }

    #[test]
    fn test_vocabulary_graph() {
        let config = TokenGraphConfig {
            graph_type: TokenGraphType::Spike,
            include_blank: true,
            blank_id: 0,
            init_weight: 0.0,
        };

        let graph = build_vocabulary_graph(3, &config);

        // Should have start state plus states for each token graph
        assert!(graph.num_states() > 1);
        assert!(graph.start() != NO_STATE);
    }

    #[test]
    fn test_blank_graph() {
        let config = TokenGraphConfig::default();
        let graph = build_blank_graph(&config);

        assert_eq!(graph.num_states(), 1);
        assert!(graph.is_final(0));
    }

    #[test]
    fn test_token_graph_stats() {
        let config = TokenGraphConfig::default();
        let graph = build_token_graph(1, &config);
        let stats = TokenGraphStats::from_wfst(&graph);

        assert!(stats.num_states > 0);
        assert!(stats.num_arcs > 0);
    }

    #[test]
    fn test_duration_limited_all_states_reachable() {
        let config = TokenGraphConfig {
            graph_type: TokenGraphType::DurationLimited { max_duration: 2 },
            include_blank: false,
            ..Default::default()
        };
        let graph = build_token_graph(1, &config);

        // All intermediate states should be final
        assert!(graph.is_final(1)); // After 1 emission
        assert!(graph.is_final(2)); // After 2 emissions
    }

    #[test]
    fn test_equally_spaced_requires_blanks() {
        let config = TokenGraphConfig {
            graph_type: TokenGraphType::EquallySpaced { blank_count: 2 },
            include_blank: true,
            blank_id: 0,
            ..Default::default()
        };
        let graph = build_token_graph(5, &config);

        // Should have transitions for blanks
        let stats = TokenGraphStats::from_wfst(&graph);
        assert!(stats.num_arcs >= 3); // 1 token + 2 blanks
    }
}
