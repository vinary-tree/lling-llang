//! Language model reranking layer.
//!
//! This layer reranks lattice paths using language model scores.
//! It requires an external language model implementation.
//!
//! # Feature Gate
//!
//! This module is only available when the `lm-rerank` feature is enabled.

use std::collections::HashMap;

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder, NodeId};
use crate::semiring::{NumericalWeight, Semiring};

use crate::layers::traits::{CorrectionLayer, LayerError, LayerResult};

/// Maximum number of words to keep in LM context.
/// This is typically set to the LM order minus 1 (e.g., 4 for a 5-gram LM).
const MAX_CONTEXT_LEN: usize = 10;

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

    /// Compute forward contexts using dynamic programming.
    ///
    /// For each node, computes all possible word contexts (histories) that can
    /// reach that node along with their accumulated weights.
    ///
    /// Returns a map from node ID to list of (context_words, accumulated_weight) pairs.
    fn compute_forward_contexts<W, B>(
        &self,
        lattice: &mut Lattice<W, B>,
    ) -> LayerResult<HashMap<NodeId, Vec<Vec<String>>>>
    where
        W: Semiring,
        B: LatticeBackend,
    {
        let mut context_map: HashMap<NodeId, Vec<Vec<String>>> = HashMap::new();

        // Initialize start node with empty context
        context_map.insert(lattice.start(), vec![vec![]]);

        // Get topological order for forward processing
        let topo_order = lattice
            .topological_order()
            .ok_or_else(|| LayerError::Other("Lattice contains a cycle".to_string()))?
            .to_vec();

        // Process nodes in topological order
        for node_id in topo_order {
            // Get contexts at current node (clone to avoid borrow issues)
            let current_contexts: Vec<Vec<String>> =
                context_map.get(&node_id).cloned().unwrap_or_default();

            if current_contexts.is_empty() {
                continue;
            }

            // Collect outgoing edge info
            let outgoing_info: Vec<(NodeId, Option<String>)> = lattice
                .outgoing_edges(node_id)
                .map(|edge| {
                    let word = lattice.edge_word(edge).map(|s| s.to_string());
                    (edge.target, word)
                })
                .collect();

            // Process each outgoing edge
            for (target, word) in outgoing_info {
                for ctx in &current_contexts {
                    let mut new_ctx = ctx.clone();

                    // Append word if present
                    if let Some(ref w) = word {
                        new_ctx.push(w.clone());

                        // Trim context to max length (keep most recent words)
                        while new_ctx.len() > MAX_CONTEXT_LEN {
                            new_ctx.remove(0);
                        }
                    }

                    context_map.entry(target).or_default().push(new_ctx);
                }
            }
        }

        Ok(context_map)
    }

    /// Interpolate original weight with LM score.
    ///
    /// Combines the original edge weight with the language model score using
    /// linear interpolation in the cost/log-prob space.
    ///
    /// Formula: w_new = (1 - λ) * w_orig + λ * (-lm_score)
    ///
    /// Where lm_score is a log probability (negative values for probabilities < 1).
    #[inline]
    fn interpolate_weight<W>(&self, orig_weight: W, lm_log_prob: f64) -> W
    where
        W: NumericalWeight + From<f64>,
    {
        let orig_val = orig_weight.numerical_value();

        // Convert LM log prob to cost space (negate it)
        // LM returns log(p) which is negative, we want -log(p) which is positive cost
        let lm_cost = -lm_log_prob;

        // Linear interpolation in cost space
        let interpolated = (1.0 - self.weight) * orig_val + self.weight * lm_cost;

        W::from(interpolated)
    }
}

impl<W, B> CorrectionLayer<W, B> for LanguageModelLayer
where
    W: Semiring + NumericalWeight + From<f64>,
    B: LatticeBackend,
{
    fn name(&self) -> &str {
        "lm-rerank"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Step 1: Compute forward contexts using DP
        // We need a mutable borrow for topological_order caching, so clone
        let mut lattice_copy = lattice.clone();
        let context_map = self.compute_forward_contexts(&mut lattice_copy)?;

        // Step 2: Build new lattice with LM-adjusted weights
        let mut builder = LatticeBuilder::with_capacity(
            lattice.backend().clone(),
            lattice.num_nodes(),
            lattice.num_edges() / lattice.num_nodes().max(1) + 1,
        );

        // Step 3: Process each edge and compute LM-adjusted weights
        for edge in lattice.edges() {
            // Get the word for this edge
            let word = match lattice.edge_word(edge) {
                Some(w) => w,
                None => {
                    // Edge without a word (epsilon transition) - keep original weight
                    let source_pos = lattice
                        .node(edge.source)
                        .and_then(|n| n.position)
                        .unwrap_or(edge.source.0 as usize);
                    let target_pos = lattice
                        .node(edge.target)
                        .and_then(|n| n.position)
                        .unwrap_or(edge.target.0 as usize);

                    builder.add_correction_by_id(
                        source_pos,
                        target_pos,
                        edge.label,
                        edge.weight,
                        edge.metadata.clone(),
                    );
                    continue;
                }
            };

            // Get contexts reaching the source node
            let contexts: &[Vec<String>] = context_map
                .get(&edge.source)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            // Compute LM score based on contexts
            let lm_log_prob = if contexts.is_empty() {
                // Start node - use unigram probability (no context)
                self.model.score_continuation(&[], word)
            } else {
                // Average LM score over all contexts reaching this node
                // This handles lattice ambiguity by marginalizing over contexts
                let mut total_score = 0.0;
                let mut count = 0;

                for ctx in contexts {
                    let ctx_refs: Vec<&str> = ctx.iter().map(|s| s.as_str()).collect();
                    total_score += self.model.score_continuation(&ctx_refs, word);
                    count += 1;
                }

                if count > 0 {
                    total_score / count as f64
                } else {
                    self.model.score_continuation(&[], word)
                }
            };

            // Interpolate the original weight with the LM score
            let adjusted_weight = self.interpolate_weight(edge.weight, lm_log_prob);

            // Get node positions for the builder
            let source_pos = lattice
                .node(edge.source)
                .and_then(|n| n.position)
                .unwrap_or(edge.source.0 as usize);
            let target_pos = lattice
                .node(edge.target)
                .and_then(|n| n.position)
                .unwrap_or(edge.target.0 as usize);

            // Add the edge with adjusted weight
            builder.add_correction_by_id(
                source_pos,
                target_pos,
                edge.label,
                adjusted_weight,
                edge.metadata.clone(),
            );
        }

        // Build and return the rescored lattice
        let end_pos = lattice
            .node(lattice.end())
            .and_then(|n| n.position)
            .unwrap_or(lattice.end().0 as usize);

        Ok(builder.build(end_pos))
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
    use crate::lattice::{EdgeMetadata, LatticeBuilder};
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

    /// LM that favors specific words with lower (better) costs
    struct BiasedLanguageModel {
        favored: String,
    }

    impl BiasedLanguageModel {
        fn new(favored: &str) -> Self {
            Self {
                favored: favored.to_string(),
            }
        }
    }

    impl LanguageModel for BiasedLanguageModel {
        fn score_sequence(&self, tokens: &[&str]) -> f64 {
            tokens.iter().map(|t| self.score_continuation(&[], t)).sum()
        }

        fn score_continuation(&self, _prefix: &[&str], next: &str) -> f64 {
            // Favored word gets better (higher/less negative) score
            if next == self.favored {
                -0.1 // High probability
            } else {
                -2.0 // Low probability
            }
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
        let name =
            <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer);
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
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel)).with_weight(1.5); // Should clamp to 1.0

        assert!((layer.weight - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel));
        // Use explicit trait method call with concrete types
        let reduction = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::estimated_reduction(&layer);
        assert!((reduction - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_apply_empty_lattice() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel));
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        let result = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        assert!(result.is_ok());
        let rescored = result.expect("layers/lm_rerank.rs: required value was None/Err");
        assert!(rescored.is_empty());
    }

    #[test]
    fn test_apply_single_edge() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel)).with_weight(0.5);

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        // Add single edge: "hello" with weight 2.0
        builder.add_correction(
            0,
            1,
            "hello",
            TropicalWeight::new(2.0),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(1);

        let result = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        assert!(result.is_ok());
        let rescored = result.expect("layers/lm_rerank.rs: required value was None/Err");

        // Should have same number of edges
        assert_eq!(rescored.num_edges(), 1);

        // Check the weight was adjusted
        // Original: 2.0, LM score: -1.0 (cost = 1.0)
        // Interpolated: 0.5 * 2.0 + 0.5 * 1.0 = 1.5
        for edge in rescored.edges() {
            let expected = 1.5;
            assert!(
                (edge.weight.value() - expected).abs() < 0.001,
                "Expected weight {}, got {}",
                expected,
                edge.weight.value()
            );
        }
    }

    #[test]
    fn test_apply_preserves_structure() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel));

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(
            0,
            1,
            "the",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(0, 1, "a", TropicalWeight::new(2.0), EdgeMetadata::default());
        builder.add_correction(
            1,
            2,
            "dog",
            TropicalWeight::new(1.5),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(2);

        let result = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        assert!(result.is_ok());
        let rescored = result.expect("layers/lm_rerank.rs: required value was None/Err");

        // Structure should be preserved
        assert_eq!(rescored.num_edges(), 3);
        assert_eq!(rescored.num_nodes(), lattice.num_nodes());
    }

    #[test]
    fn test_weight_interpolation_formula() {
        // Test the interpolation formula directly
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel)).with_weight(0.3); // 30% LM, 70% original

        let orig_weight = TropicalWeight::new(4.0);
        let lm_log_prob = -2.0; // Cost = 2.0

        let result = layer.interpolate_weight(orig_weight, lm_log_prob);

        // Expected: 0.7 * 4.0 + 0.3 * 2.0 = 2.8 + 0.6 = 3.4
        assert!(
            (result.value() - 3.4).abs() < 0.001,
            "Expected 3.4, got {}",
            result.value()
        );
    }

    #[test]
    fn test_lambda_zero_ignores_lm() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel)).with_weight(0.0); // Ignore LM completely

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(
            0,
            1,
            "word",
            TropicalWeight::new(5.0),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(1);

        let result = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        let rescored = result.expect("layers/lm_rerank.rs: required value was None/Err");

        // With lambda=0, weights should be unchanged
        for edge in rescored.edges() {
            assert!(
                (edge.weight.value() - 5.0).abs() < 0.001,
                "Expected weight 5.0, got {}",
                edge.weight.value()
            );
        }
    }

    #[test]
    fn test_lambda_one_uses_only_lm() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel)).with_weight(1.0); // Use only LM scores

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction(
            0,
            1,
            "word",
            TropicalWeight::new(5.0),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(1);

        let result = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        let rescored = result.expect("layers/lm_rerank.rs: required value was None/Err");

        // With lambda=1, weight should be purely LM cost
        // LM returns -1.0 (log prob), cost = 1.0
        for edge in rescored.edges() {
            assert!(
                (edge.weight.value() - 1.0).abs() < 0.001,
                "Expected weight 1.0, got {}",
                edge.weight.value()
            );
        }
    }

    #[test]
    fn test_biased_lm_adjusts_weights() {
        // LM that favors "good" over "bad"
        let layer =
            LanguageModelLayer::new(Box::new(BiasedLanguageModel::new("good"))).with_weight(0.5);

        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);
        // Both start with same original weight
        builder.add_correction(
            0,
            1,
            "good",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            0,
            1,
            "bad",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(1);

        let result = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        let rescored = result.expect("layers/lm_rerank.rs: required value was None/Err");

        let mut good_weight = None;
        let mut bad_weight = None;

        for edge in rescored.edges() {
            let word = rescored.edge_word(edge).unwrap_or("");
            if word == "good" {
                good_weight = Some(edge.weight.value());
            } else if word == "bad" {
                bad_weight = Some(edge.weight.value());
            }
        }

        // "good" should have lower (better) weight than "bad"
        // good: 0.5 * 1.0 + 0.5 * 0.1 = 0.55
        // bad:  0.5 * 1.0 + 0.5 * 2.0 = 1.5
        assert!(good_weight.is_some(), "good edge not found");
        assert!(bad_weight.is_some(), "bad edge not found");
        assert!(
            good_weight.expect("layers/lm_rerank.rs: required value was None/Err")
                < bad_weight.expect("layers/lm_rerank.rs: required value was None/Err"),
            "Expected good ({}) < bad ({})",
            good_weight.expect("layers/lm_rerank.rs: required value was None/Err"),
            bad_weight.expect("layers/lm_rerank.rs: required value was None/Err")
        );
    }

    #[test]
    fn test_can_apply_always_true() {
        let layer = LanguageModelLayer::new(Box::new(MockLanguageModel));
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        let can_apply =
            <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::can_apply(
                &layer, &lattice,
            );
        assert!(can_apply);
    }
}
