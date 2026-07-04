//! Marginalized word piece decompositions for differentiable training.
//!
//! This module provides functionality for marginalizing over multiple
//! word piece decompositions during training, allowing the model to
//! learn task-salient segmentations.
//!
//! ## Motivation
//!
//! Standard word piece tokenization (e.g., SentencePiece) learns decompositions
//! independently of the downstream task. This can be suboptimal because:
//!
//! 1. Fixed decomposition may not align with acoustic/visual features
//! 2. Different inputs may benefit from different segmentation granularities
//! 3. Task-specific patterns may not be captured
//!
//! ## Solution
//!
//! Instead of using a fixed decomposition, we marginalize over all valid
//! decompositions via a lexicon transducer:
//!
//! ```text
//! A = E ∘ (B ∘ ((T₁ + ... + T_C)* ∘ (L ∘ Y)))
//! ```
//!
//! Where:
//! - E = emissions from neural network
//! - B = bigram transition graph
//! - T_i = token graph for token i
//! - L = lexicon transducer (word pieces → graphemes)
//! - Y = target label sequence
//!
//! ## Benefits
//!
//! - Model learns task-salient decompositions
//! - Adapts dynamically based on input characteristics
//! - Recovers accuracy lost with larger token vocabularies
//!
//! ## References
//!
//! - Hannun et al., "Differentiable Weighted Finite-State Transducers" (ICML 2020, arXiv:2010.01003)

use std::collections::HashMap;

use crate::composition::{compose, materialize};
use crate::semiring::{LogWeight, Semiring};
#[cfg(test)]
use crate::wfst::Wfst;
use crate::wfst::{MutableWfst, StateId, VectorWfst};

/// Word piece identifier type.
pub type WordPieceId = u32;

/// Grapheme identifier type (character-level).
pub type GraphemeId = u32;

/// A lexicon entry mapping a word piece to its grapheme sequence.
#[derive(Clone, Debug)]
pub struct LexiconEntry {
    /// Word piece ID.
    pub word_piece: WordPieceId,
    /// Grapheme sequence (character-level representation).
    pub graphemes: Vec<GraphemeId>,
    /// Weight (log probability) for this decomposition.
    pub weight: f64,
}

impl LexiconEntry {
    /// Create a new lexicon entry.
    pub fn new(word_piece: WordPieceId, graphemes: Vec<GraphemeId>) -> Self {
        Self {
            word_piece,
            graphemes,
            weight: 0.0,
        }
    }

    /// Create with a specific weight.
    pub fn with_weight(word_piece: WordPieceId, graphemes: Vec<GraphemeId>, weight: f64) -> Self {
        Self {
            word_piece,
            graphemes,
            weight,
        }
    }
}

/// Configuration for lexicon construction.
#[derive(Clone, Debug)]
pub struct LexiconConfig {
    /// Whether to allow multiple decompositions per grapheme sequence.
    pub allow_multiple_decompositions: bool,
    /// Initial weight for lexicon transitions.
    pub init_weight: f64,
    /// Word boundary marker (if any).
    pub word_boundary: Option<GraphemeId>,
}

impl Default for LexiconConfig {
    fn default() -> Self {
        Self {
            allow_multiple_decompositions: true,
            init_weight: 0.0,
            word_boundary: None,
        }
    }
}

/// Build a lexicon transducer from word piece to grapheme mappings.
///
/// The lexicon transducer maps word piece sequences to grapheme sequences,
/// enabling marginalization over all valid decompositions.
///
/// # Arguments
///
/// * `entries` - Lexicon entries mapping word pieces to graphemes
/// * `config` - Configuration options
///
/// # Returns
///
/// A WFST where input labels are word pieces and output labels are graphemes.
pub fn build_lexicon_transducer(
    entries: &[LexiconEntry],
    config: &LexiconConfig,
) -> VectorWfst<WordPieceId, LogWeight> {
    let mut fst = VectorWfst::new();

    // Create start state (also serves as loop-back point)
    let start = fst.add_state();
    fst.set_start(start);
    fst.set_final(start, LogWeight::one());

    // Add each word piece entry
    for entry in entries {
        add_lexicon_entry(&mut fst, start, entry, config);
    }

    fst
}

/// Add a single lexicon entry to the transducer.
fn add_lexicon_entry(
    fst: &mut VectorWfst<WordPieceId, LogWeight>,
    start: StateId,
    entry: &LexiconEntry,
    _config: &LexiconConfig,
) {
    if entry.graphemes.is_empty() {
        // Empty grapheme sequence: direct epsilon transition
        fst.add_arc(
            start,
            Some(entry.word_piece),
            None,
            start,
            LogWeight::new(entry.weight),
        );
        return;
    }

    // Create states for grapheme sequence
    let num_graphemes = entry.graphemes.len();
    let mut current = start;

    // First transition: input = word_piece, output = first grapheme
    let next = fst.add_state();
    fst.add_arc(
        current,
        Some(entry.word_piece),
        Some(entry.graphemes[0]),
        next,
        LogWeight::new(entry.weight),
    );
    current = next;

    // Middle transitions: input = epsilon, output = grapheme
    for i in 1..num_graphemes - 1 {
        let next = fst.add_state();
        fst.add_arc(
            current,
            None,
            Some(entry.graphemes[i]),
            next,
            LogWeight::one(),
        );
        current = next;
    }

    // Last transition: back to start
    if num_graphemes > 1 {
        fst.add_arc(
            current,
            None,
            Some(entry.graphemes[num_graphemes - 1]),
            start,
            LogWeight::one(),
        );
    } else {
        // Single grapheme: already handled in first transition
        // Need to add loop-back epsilon
        fst.add_arc(current, None, None, start, LogWeight::one());
    }
}

/// Build a target sequence graph from grapheme labels.
///
/// Creates a linear WFST accepting exactly the given grapheme sequence.
///
/// # Arguments
///
/// * `graphemes` - Target grapheme sequence
///
/// # Returns
///
/// A WFST accepting exactly the input sequence.
pub fn build_target_graph(graphemes: &[GraphemeId]) -> VectorWfst<GraphemeId, LogWeight> {
    let mut fst = VectorWfst::new();

    if graphemes.is_empty() {
        let s = fst.add_state();
        fst.set_start(s);
        fst.set_final(s, LogWeight::one());
        return fst;
    }

    // Create linear chain
    let mut states = Vec::with_capacity(graphemes.len() + 1);
    for _ in 0..=graphemes.len() {
        states.push(fst.add_state());
    }

    fst.set_start(states[0]);
    fst.set_final(states[graphemes.len()], LogWeight::one());

    // Add transitions for each grapheme
    for (i, &grapheme) in graphemes.iter().enumerate() {
        fst.add_arc(
            states[i],
            Some(grapheme),
            Some(grapheme),
            states[i + 1],
            LogWeight::one(),
        );
    }

    fst
}

/// Marginalization context for word piece decompositions.
///
/// Maintains the composed graph structure for computing marginal
/// probabilities over all valid decompositions.
#[derive(Clone, Debug)]
pub struct MarginalizationContext {
    /// Vocabulary size (number of word pieces).
    pub vocab_size: usize,
    /// Grapheme vocabulary size.
    pub grapheme_vocab_size: usize,
    /// Whether the context has been initialized.
    pub initialized: bool,
}

impl MarginalizationContext {
    /// Create a new marginalization context.
    pub fn new(vocab_size: usize, grapheme_vocab_size: usize) -> Self {
        Self {
            vocab_size,
            grapheme_vocab_size,
            initialized: false,
        }
    }

    /// Initialize the context with lexicon entries.
    pub fn initialize(&mut self, _entries: &[LexiconEntry]) {
        self.initialized = true;
    }
}

/// Compute marginalized loss over word piece decompositions.
///
/// This function computes the loss by marginalizing over all valid
/// decompositions of the target sequence into word pieces.
///
/// # Arguments
///
/// * `emissions` - Neural network emissions (log probabilities)
/// * `lexicon` - Lexicon transducer
/// * `target` - Target grapheme sequence
///
/// # Returns
///
/// The marginalized log probability.
pub fn marginalized_loss(
    emissions: &VectorWfst<WordPieceId, LogWeight>,
    lexicon: &VectorWfst<WordPieceId, LogWeight>,
    target: &[GraphemeId],
) -> f64 {
    let target_graph = build_target_graph(target);
    let emission_lexicon = materialize(compose(emissions.clone(), lexicon.clone()));
    let constrained = materialize(compose(emission_lexicon, target_graph));

    compute_forward_score(&constrained)
}

/// Compute the log-domain forward score of a constrained graph.
fn compute_forward_score(fst: &VectorWfst<WordPieceId, LogWeight>) -> f64 {
    use super::forward_score::forward_score;
    use super::gradient::GradientWfst;

    let grad_fst = GradientWfst::from_wfst(fst);
    let score = forward_score(&grad_fst);
    score.value()
}

/// Result of marginalized training step.
#[derive(Clone, Debug)]
pub struct MarginalizationResult {
    /// The marginalized loss value.
    pub loss: f64,
    /// Gradients for emission weights.
    pub emission_gradients: Vec<f64>,
    /// Decomposition statistics.
    pub stats: MarginalizationStats,
}

/// Statistics about marginalization.
#[derive(Clone, Debug, Default)]
pub struct MarginalizationStats {
    /// Number of valid decompositions found.
    pub num_decompositions: usize,
    /// Average decomposition length.
    pub avg_decomposition_length: f64,
    /// Most probable decomposition.
    pub best_decomposition: Vec<WordPieceId>,
}

/// Build a simple vocabulary from word pieces.
///
/// Creates lexicon entries where each word piece maps to itself as a grapheme.
pub fn build_identity_lexicon(vocab_size: usize) -> Vec<LexiconEntry> {
    (0..vocab_size as WordPieceId)
        .map(|wp| LexiconEntry::new(wp, vec![wp]))
        .collect()
}

/// Build a character-level lexicon from word piece definitions.
///
/// # Arguments
///
/// * `word_pieces` - Map from word piece ID to string representation
///
/// # Returns
///
/// Lexicon entries with character-level grapheme sequences.
pub fn build_character_lexicon(word_pieces: &HashMap<WordPieceId, String>) -> Vec<LexiconEntry> {
    word_pieces
        .iter()
        .map(|(&wp_id, wp_str)| {
            let graphemes: Vec<GraphemeId> = wp_str.chars().map(|c| c as GraphemeId).collect();
            LexiconEntry::new(wp_id, graphemes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfst::NO_STATE;

    #[test]
    fn test_lexicon_entry_creation() {
        let entry = LexiconEntry::new(1, vec![10, 11, 12]);
        assert_eq!(entry.word_piece, 1);
        assert_eq!(entry.graphemes, vec![10, 11, 12]);
        assert_eq!(entry.weight, 0.0);
    }

    #[test]
    fn test_lexicon_entry_with_weight() {
        let entry = LexiconEntry::with_weight(2, vec![20, 21], -0.5);
        assert_eq!(entry.word_piece, 2);
        assert_eq!(entry.weight, -0.5);
    }

    #[test]
    fn test_lexicon_config_default() {
        let config = LexiconConfig::default();
        assert!(config.allow_multiple_decompositions);
        assert_eq!(config.init_weight, 0.0);
        assert!(config.word_boundary.is_none());
    }

    #[test]
    fn test_build_lexicon_transducer() {
        let entries = vec![
            LexiconEntry::new(1, vec![10, 11]),
            LexiconEntry::new(2, vec![20]),
        ];
        let config = LexiconConfig::default();

        let fst = build_lexicon_transducer(&entries, &config);

        assert!(fst.start() != NO_STATE);
        assert!(fst.num_states() > 1);
    }

    #[test]
    fn test_build_lexicon_empty_entry() {
        let entries = vec![
            LexiconEntry::new(1, vec![]), // Empty grapheme sequence
        ];
        let config = LexiconConfig::default();

        let fst = build_lexicon_transducer(&entries, &config);

        // Should still be valid
        assert!(fst.start() != NO_STATE);
    }

    #[test]
    fn test_build_target_graph() {
        let graphemes = vec![10, 11, 12];
        let fst = build_target_graph(&graphemes);

        assert_eq!(fst.num_states(), 4); // 3 transitions + 1 = 4 states
        assert!(fst.start() != NO_STATE);
        assert!(fst.is_final(3));
    }

    #[test]
    fn test_build_target_graph_empty() {
        let graphemes: Vec<GraphemeId> = vec![];
        let fst = build_target_graph(&graphemes);

        assert_eq!(fst.num_states(), 1);
        assert!(fst.is_final(0));
    }

    #[test]
    fn test_marginalization_context() {
        let mut ctx = MarginalizationContext::new(100, 256);
        assert!(!ctx.initialized);

        let entries = vec![LexiconEntry::new(0, vec![0])];
        ctx.initialize(&entries);
        assert!(ctx.initialized);
    }

    #[test]
    fn test_build_identity_lexicon() {
        let lexicon = build_identity_lexicon(10);
        assert_eq!(lexicon.len(), 10);

        for (i, entry) in lexicon.iter().enumerate() {
            assert_eq!(entry.word_piece, i as WordPieceId);
            assert_eq!(entry.graphemes, vec![i as GraphemeId]);
        }
    }

    #[test]
    fn test_build_character_lexicon() {
        let mut word_pieces = HashMap::new();
        word_pieces.insert(0, "a".to_string());
        word_pieces.insert(1, "bc".to_string());

        let lexicon = build_character_lexicon(&word_pieces);
        assert_eq!(lexicon.len(), 2);

        // Find the "bc" entry
        let bc_entry = lexicon
            .iter()
            .find(|e| e.word_piece == 1)
            .expect("differentiable/marginalization.rs: required value was None/Err");
        assert_eq!(
            bc_entry.graphemes,
            vec!['b' as GraphemeId, 'c' as GraphemeId]
        );
    }

    #[test]
    fn test_marginalization_stats_default() {
        let stats = MarginalizationStats::default();
        assert_eq!(stats.num_decompositions, 0);
        assert!(stats.best_decomposition.is_empty());
    }

    #[test]
    fn test_marginalized_loss_filters_by_target() {
        let mut emissions = VectorWfst::new();
        let start = emissions.add_state();
        let final_state = emissions.add_state();
        emissions.set_start(start);
        emissions.set_final(final_state, LogWeight::one());
        emissions.add_arc(start, Some(1), Some(1), final_state, LogWeight::new(0.3));
        emissions.add_arc(start, Some(2), Some(2), final_state, LogWeight::new(0.7));

        let entries = vec![
            LexiconEntry::new(1, vec![10]),
            LexiconEntry::new(2, vec![20]),
        ];
        let lexicon = build_lexicon_transducer(&entries, &LexiconConfig::default());

        let loss = marginalized_loss(&emissions, &lexicon, &[10]);
        assert!((loss - 0.3).abs() < 1e-9);

        let impossible = marginalized_loss(&emissions, &lexicon, &[30]);
        assert!(impossible.is_infinite());
    }

    #[test]
    fn test_marginalized_loss_sums_decompositions() {
        let mut emissions = VectorWfst::new();
        let s0 = emissions.add_state();
        let s1 = emissions.add_state();
        let s2 = emissions.add_state();
        let sf = emissions.add_state();
        emissions.set_start(s0);
        emissions.set_final(sf, LogWeight::one());
        emissions.add_arc(s0, Some(1), Some(1), sf, LogWeight::new(1.0));
        emissions.add_arc(s0, Some(2), Some(2), s1, LogWeight::new(2.0));
        emissions.add_arc(s1, Some(3), Some(3), sf, LogWeight::new(3.0));
        emissions.add_arc(s0, Some(4), Some(4), s2, LogWeight::new(0.1));
        emissions.add_arc(s2, Some(5), Some(5), sf, LogWeight::new(0.1));

        let entries = vec![
            LexiconEntry::new(1, vec![10, 11]),
            LexiconEntry::new(2, vec![10]),
            LexiconEntry::new(3, vec![11]),
            LexiconEntry::new(4, vec![10]),
            LexiconEntry::new(5, vec![12]),
        ];
        let lexicon = build_lexicon_transducer(&entries, &LexiconConfig::default());

        let expected = LogWeight::new(1.0).plus(&LogWeight::new(5.0)).value();
        let loss = marginalized_loss(&emissions, &lexicon, &[10, 11]);

        assert!((loss - expected).abs() < 1e-9);
    }
}
