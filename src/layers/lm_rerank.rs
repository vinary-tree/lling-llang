//! Language model reranking layer.
//!
//! This layer reranks lattice paths using language model scores.
//! It requires an external language model implementation.
//!
//! # Feature Gate
//!
//! This module is only available when the `lm-rerank` feature is enabled.

use crate::backend::LatticeBackend;
use crate::lattice::Lattice;
use crate::semiring::Semiring;

use super::traits::{CorrectionLayer, LayerError, LayerResult};

/// Trait for language models.
///
/// Implement this trait to provide language model scoring.
pub trait LanguageModel: Send + Sync {
    /// Score a complete token sequence.
    ///
    /// Returns log probability (higher = more likely).
    fn score_sequence(&self, tokens: &[&str]) -> f64;

    /// Score a continuation given a prefix.
    ///
    /// Returns log probability of `next` given `prefix`.
    fn score_continuation(&self, prefix: &[&str], next: &str) -> f64;

    /// Get the vocabulary size (for perplexity calculations).
    fn vocab_size(&self) -> usize {
        0 // Unknown by default
    }
}

/// Language model reranking layer.
///
/// This layer adjusts lattice edge weights based on language model scores,
/// helping to select more fluent corrections.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::LanguageModelLayer;
///
/// let layer = LanguageModelLayer::new(Box::new(my_lm))
///     .with_weight(0.5);  // Interpolate 50% LM, 50% edit distance
/// let reranked = layer.apply(&lattice)?;
/// ```
pub struct LanguageModelLayer {
    model: Box<dyn LanguageModel>,
    /// Interpolation weight for LM scores (0.0 = ignore LM, 1.0 = only LM).
    weight: f64,
    /// Whether to normalize LM scores by sequence length.
    normalize_by_length: bool,
}

impl LanguageModelLayer {
    /// Create a new language model layer.
    pub fn new(model: Box<dyn LanguageModel>) -> Self {
        Self {
            model,
            weight: 0.5,
            normalize_by_length: true,
        }
    }

    /// Set the interpolation weight for LM scores.
    ///
    /// - 0.0: Ignore LM scores entirely
    /// - 0.5: Equal weight to LM and original scores (default)
    /// - 1.0: Use only LM scores
    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set whether to normalize LM scores by sequence length.
    ///
    /// Default is true, which prevents bias toward shorter sequences.
    pub fn with_length_normalization(mut self, normalize: bool) -> Self {
        self.normalize_by_length = normalize;
        self
    }

    /// Get the language model.
    pub fn model(&self) -> &dyn LanguageModel {
        self.model.as_ref()
    }

    /// Get the interpolation weight.
    pub fn weight(&self) -> f64 {
        self.weight
    }
}

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for LanguageModelLayer {
    fn name(&self) -> &str {
        "lm-rerank"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // TODO: Implement LM-based reranking
        // 1. Extract paths from the lattice
        // 2. Score each path with the language model
        // 3. Interpolate LM scores with original edge weights
        // 4. Build a new lattice with adjusted weights

        Err(LayerError::Other(
            "Language model layer not yet implemented - this is a stub".to_string()
        ))
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true
    }

    fn estimated_reduction(&self) -> f64 {
        // LM reranking doesn't reduce paths, it reweights them
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::semiring::TropicalWeight;

    struct MockLanguageModel;

    impl LanguageModel for MockLanguageModel {
        fn score_sequence(&self, tokens: &[&str]) -> f64 {
            // Simple mock: -1.0 per token
            -(tokens.len() as f64)
        }

        fn score_continuation(&self, _prefix: &[&str], _next: &str) -> f64 {
            -1.0
        }
    }

    #[test]
    fn test_mock_lm() {
        let lm = MockLanguageModel;
        assert_eq!(lm.score_sequence(&["the", "dog"]), -2.0);
        assert_eq!(lm.score_continuation(&["the"], "dog"), -1.0);
    }

    #[test]
    fn test_layer_name() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel));
        // Use explicit trait method call with concrete types
        let name = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer);
        assert_eq!(name, "lm-rerank");
    }

    #[test]
    fn test_layer_builder() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel))
            .with_weight(0.7)
            .with_length_normalization(false);

        assert!((layer.weight - 0.7).abs() < 0.001);
        assert!(!layer.normalize_by_length);
    }

    #[test]
    fn test_weight_clamping() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel))
            .with_weight(1.5);  // Should clamp to 1.0

        assert!((layer.weight - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel));
        // Use explicit trait method call with concrete types
        let reduction = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::estimated_reduction(&layer);
        assert!((reduction - 1.0).abs() < 0.001);
    }
}
