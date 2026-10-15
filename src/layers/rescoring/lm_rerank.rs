//! Language model reranking layer.
//!
//! This layer reranks lattice paths using language model scores.
//! It requires an external language model implementation.
//!
//! # Feature Gate
//!
//! This module is only available when the `lm-rerank` feature is enabled.

use std::collections::{BTreeMap, HashMap};

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder, NodeId};
use crate::semiring::{NumericalWeight, Semiring};

use super::super::traits::{CorrectionLayer, LayerError, LayerResult};

/// Maximum number of words to keep in LM context.
/// This is typically set to the LM order minus 1 (e.g., 4 for a 5-gram LM).
const MAX_CONTEXT_LEN: usize = 10;
const DEFAULT_MAX_CONTEXTS_PER_NODE: usize = 4096;
const DEFAULT_MAX_CONTEXT_EXPANSIONS: usize = 1_048_576;

type ContextCounts = BTreeMap<Vec<String>, u64>;

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
    /// Whether to normalize continuation scores by the observed context-window length.
    normalize_by_length: bool,
    /// Bound distinct LM histories retained at each lattice node.
    max_contexts_per_node: usize,
    /// Bound context propagation across lattice edges.
    max_context_expansions: usize,
}

impl LanguageModelLayer {
    /// Create a new language model layer.
    pub fn new(model: Box<dyn LanguageModel>) -> Self {
        Self {
            model,
            weight: 0.5,
            normalize_by_length: true,
            max_contexts_per_node: DEFAULT_MAX_CONTEXTS_PER_NODE,
            max_context_expansions: DEFAULT_MAX_CONTEXT_EXPANSIONS,
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

    /// Set whether to normalize each continuation score by its observed
    /// context-window length plus the next word.
    ///
    /// The context window holds at most ten preceding words. Default is true.
    pub fn with_length_normalization(mut self, normalize: bool) -> Self {
        self.normalize_by_length = normalize;
        self
    }

    /// Set hard limits on distinct contexts per node and total propagated
    /// context-edge pairs. Exceeding either limit returns a layer error.
    pub fn with_context_limits(mut self, per_node: usize, expansions: usize) -> Self {
        self.max_contexts_per_node = per_node;
        self.max_context_expansions = expansions;
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
    /// For each node, computes distinct word histories and the number of
    /// paths that reach each history. Collapsing duplicate histories retains
    /// the previous path-multiplicity weighting without storing every path.
    ///
    /// Returns a map from node ID to bounded context-multiplicity maps.
    fn compute_forward_contexts<W, B>(
        &self,
        lattice: &mut Lattice<W, B>,
    ) -> LayerResult<HashMap<NodeId, ContextCounts>>
    where
        W: Semiring,
        B: LatticeBackend,
    {
        if self.max_contexts_per_node == 0 || self.max_context_expansions == 0 {
            return Err(LayerError::ConfigError(
                "LM context limits must be positive".to_string(),
            ));
        }
        let mut context_map: HashMap<NodeId, ContextCounts> = HashMap::new();
        let mut expansions = 0usize;

        // Initialize start node with empty context
        context_map.insert(lattice.start(), BTreeMap::from([(vec![], 1)]));

        // Get topological order for forward processing
        let topo_order = lattice
            .topological_order()
            .ok_or_else(|| LayerError::Other("Lattice contains a cycle".to_string()))?
            .to_vec();

        // Process nodes in topological order
        for node_id in topo_order {
            // Get contexts at current node (clone to avoid borrow issues)
            let current_contexts: ContextCounts =
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
                for (ctx, count) in &current_contexts {
                    expansions = expansions.checked_add(1).ok_or_else(|| {
                        LayerError::Other("LM context expansion counter overflowed".to_string())
                    })?;
                    if expansions > self.max_context_expansions {
                        return Err(LayerError::Other(
                            "LM context expansion limit exceeded".to_string(),
                        ));
                    }
                    let mut new_ctx = ctx.clone();

                    // Append word if present
                    if let Some(ref w) = word {
                        new_ctx.push(w.clone());

                        // Trim context to max length (keep most recent words)
                        while new_ctx.len() > MAX_CONTEXT_LEN {
                            new_ctx.remove(0);
                        }
                    }

                    let target_contexts = context_map.entry(target).or_default();
                    if !target_contexts.contains_key(&new_ctx)
                        && target_contexts.len() >= self.max_contexts_per_node
                    {
                        return Err(LayerError::Other(
                            "LM contexts-per-node limit exceeded".to_string(),
                        ));
                    }
                    let multiplicity = target_contexts.entry(new_ctx).or_default();
                    *multiplicity = multiplicity.checked_add(*count).ok_or_else(|| {
                        LayerError::Other("LM path multiplicity overflowed".to_string())
                    })?;
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
    /// Formula:
    ///
    /// ```math
    /// w_{\mathrm{new}}=(1-\lambda)w_{\mathrm{orig}}
    /// +\lambda(-s_{\mathrm{LM}})
    /// ```
    ///
    /// Where lm_score is a log probability (negative values for probabilities < 1).
    #[inline]
    fn interpolate_weight<W>(&self, orig_weight: W, lm_log_prob: f64) -> W
    where
        W: NumericalWeight + From<f64>,
    {
        if self.weight == 0.0 {
            return orig_weight;
        }
        let orig_val = orig_weight.numerical_value();

        // Convert LM log prob to cost space (negate it)
        // LM returns log(p) which is negative, we want -log(p) which is positive cost
        let lm_cost = -lm_log_prob;

        // Linear interpolation in cost space
        let interpolated = if self.weight == 1.0 {
            lm_cost
        } else {
            (1.0 - self.weight) * orig_val + self.weight * lm_cost
        };

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
        if !self.weight.is_finite() {
            return Err(LayerError::ConfigError(
                "LM interpolation weight must be finite".to_string(),
            ));
        }
        if self.weight == 0.0 {
            return Ok(lattice.clone());
        }
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
            let contexts = context_map.get(&edge.source);

            // Compute LM score based on contexts
            let lm_log_prob = if contexts.is_none_or(BTreeMap::is_empty) {
                // Start node - use unigram probability (no context)
                self.model.score_continuation(&[], word)
            } else {
                // Average over path histories, preserving multiplicities after
                // identical contexts were collapsed during propagation.
                let mut total_score = 0.0;
                let mut count = 0u128;

                for (ctx, multiplicity) in contexts.expect("nonempty contexts checked above") {
                    let ctx_refs: Vec<&str> = ctx.iter().map(|s| s.as_str()).collect();
                    let mut score = self.model.score_continuation(&ctx_refs, word);
                    if self.normalize_by_length {
                        score /= (ctx.len() + 1) as f64;
                    }
                    if score.is_nan() || score == f64::INFINITY {
                        return Err(LayerError::Other(
                            "LM returned an invalid continuation score".to_string(),
                        ));
                    }
                    total_score += score * *multiplicity as f64;
                    count = count
                        .checked_add(u128::from(*multiplicity))
                        .ok_or_else(|| LayerError::Other("LM path count overflowed".to_string()))?;
                }

                total_score / count as f64
            };
            if lm_log_prob.is_nan() || lm_log_prob == f64::INFINITY {
                return Err(LayerError::Other(
                    "LM returned an invalid continuation score".to_string(),
                ));
            }

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

    struct UniformPenaltyLanguageModel;

    impl LanguageModel for UniformPenaltyLanguageModel {
        fn score_sequence(&self, tokens: &[&str]) -> f64 {
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
    fn test_uniform_penalty_lm() {
        let lm = UniformPenaltyLanguageModel;
        assert_eq!(lm.score_sequence(&["the", "dog"]), -2.0);
        assert_eq!(lm.score_continuation(&["the"], "dog"), -1.0);
    }

    #[test]
    fn test_layer_name() {
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel));
        // Use explicit trait method call with concrete types
        let name =
            <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer);
        assert_eq!(name, "lm-rerank");
    }

    #[test]
    fn test_layer_builder() {
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel))
            .with_weight(0.7)
            .with_length_normalization(false)
            .with_context_limits(8, 64);

        assert!((layer.weight - 0.7).abs() < 0.001);
        assert!(!layer.normalize_by_length);
        assert_eq!(layer.max_contexts_per_node, 8);
        assert_eq!(layer.max_context_expansions, 64);
    }

    #[test]
    fn test_weight_clamping() {
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel)).with_weight(1.5); // Should clamp to 1.0

        assert!((layer.weight - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_invalid_weight_is_rejected_before_scoring() {
        let layer =
            LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel)).with_weight(f64::NAN);
        let lattice: Lattice<TropicalWeight, _> =
            LatticeBuilder::new(HashMapBackend::new()).build(0);
        assert!(layer.apply(&lattice).is_err());
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel));
        // Use explicit trait method call with concrete types
        let reduction = <LanguageModelLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::estimated_reduction(&layer);
        assert!((reduction - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_apply_empty_lattice() {
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel));
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
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel)).with_weight(0.5);

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
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel));

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
    fn test_context_length_normalization_changes_following_edge() {
        let mut builder = LatticeBuilder::new(HashMapBackend::new());
        builder.add_correction(
            0,
            1,
            "first",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "second",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(2);
        let normalized = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel))
            .with_weight(1.0)
            .apply(&lattice)
            .expect("normalized LM applies");
        let raw = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel))
            .with_weight(1.0)
            .with_length_normalization(false)
            .apply(&lattice)
            .expect("raw LM applies");
        assert_eq!(normalized.edges()[0].weight.value(), 1.0);
        assert_eq!(normalized.edges()[1].weight.value(), 0.5);
        assert_eq!(raw.edges()[1].weight.value(), 1.0);
    }

    #[test]
    fn test_context_limits_reject_branch_explosion() {
        let mut builder = LatticeBuilder::new(HashMapBackend::new());
        builder.add_correction(0, 1, "a", TropicalWeight::new(0.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(0.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "c", TropicalWeight::new(0.0), EdgeMetadata::default());
        let lattice = builder.build(2);

        let context_limited = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel))
            .with_context_limits(1, 100);
        assert!(context_limited.apply(&lattice).is_err());

        let work_limited = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel))
            .with_context_limits(8, 1);
        assert!(work_limited.apply(&lattice).is_err());
    }

    #[test]
    fn test_duplicate_contexts_do_not_exhaust_distinct_context_limit() {
        let mut builder = LatticeBuilder::new(HashMapBackend::new());
        builder.add_correction(
            0,
            1,
            "same",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            0,
            1,
            "same",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "next",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(2);
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel))
            .with_context_limits(1, 100);
        assert!(layer.apply(&lattice).is_ok());
    }

    #[test]
    fn test_weight_interpolation_formula() {
        // Test the interpolation formula directly
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel)).with_weight(0.3); // 30% LM, 70% original

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
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel))
            .with_weight(0.0)
            .with_context_limits(0, 0); // No LM contexts are needed.
        assert_eq!(
            layer
                .interpolate_weight(TropicalWeight::new(5.0), f64::NEG_INFINITY)
                .value(),
            5.0
        );

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
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel)).with_weight(1.0); // Use only LM scores

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
        let layer = LanguageModelLayer::new(Box::new(UniformPenaltyLanguageModel));
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
