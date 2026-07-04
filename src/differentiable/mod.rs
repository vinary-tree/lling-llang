//! Differentiable WFST operations for end-to-end training.
//!
//! This module provides automatic differentiation through WFST operations,
//! enabling gradient-based training with WFST-based loss functions.
//!
//! ## Core Concepts
//!
//! 1. **Gradient graphs**: Every WFST operation returns a graph where gradients
//!    can be computed with respect to arc weights.
//!
//! 2. **Forward/backward passes**: Forward computes scores, backward propagates
//!    gradients through the graph structure.
//!
//! 3. **Semiring selection**: Log semiring for forward score (sum over paths),
//!    tropical semiring for Viterbi (max over paths).
//!
//! ## Supported Operations
//!
//! | Operation | Description | Differentiable |
//! |-----------|-------------|----------------|
//! | Forward Score | log-sum-exp over all paths | ✓ |
//! | Viterbi Score | max over all paths | ✓ |
//! | Viterbi Path | argmax path extraction | ✓ |
//! | Intersection | A₁ ∩ A₂ (acceptors) | ✓ |
//! | Composition | T₁ ∘ T₂ (transducers) | ✓ |
//! | WFST Convolution | Apply kernel WFSTs to receptive fields | ✓ |
//! | Token Graphs | CTC variants (Spike, Duration-Limited) | ✓ |
//! | Marginalization | Word piece decomposition marginalization | ✓ |
//! | Second-Order | Hessian and Fisher information | ✓ |
//!
//! ## Deep Learning Integration
//!
//! This module includes components for integrating WFSTs into deep learning:
//!
//! - **WFST Convolutional Layers**: Apply kernel WFSTs to hidden unit sequences
//!   with far fewer parameters than a dense convolution (≈38× in Hannun et al. (2020)).
//!
//! - **Token Graph Variants**: Encode different prior beliefs about alignments
//!   (Spike CTC, Duration-Limited CTC, Equally Spaced CTC).
//!
//! - **Marginalized Word Pieces**: Learn task-salient segmentations by
//!   marginalizing over all valid decompositions via a lexicon transducer.
//!
//! - **N-gram Pruning**: Efficient transition graphs with back-off for large
//!   vocabularies via pruning and back-off.
//!
//! - **Second-Order Differentiation**: Compute Hessian matrices and Fisher
//!   information for natural gradient optimization.
//!
//! ## Example
//!
//! ```rust
//! use lling_llang::differentiable::{forward_score, backward, GradientWfst};
//! use lling_llang::wfst::{VectorWfst, MutableWfst};
//! use lling_llang::semiring::{LogWeight, Semiring};
//!
//! // Create a simple WFST
//! let mut fst = VectorWfst::<char, LogWeight>::new();
//! let s0 = fst.add_state();
//! let s1 = fst.add_state();
//! fst.set_start(s0);
//! fst.set_final(s1, LogWeight::one());
//! fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
//!
//! // Compute forward score with gradients
//! let grad_fst = GradientWfst::from_wfst(&fst);
//! let score = forward_score(&grad_fst);
//!
//! // Backward pass to compute gradients
//! let gradients = backward(&grad_fst);
//! ```
//!
//! ## References
//!
//! - Hannun et al., "Differentiable Weighted Finite-State Transducers" (ICML 2020, arXiv:2010.01003)

mod forward_score;
mod gradient;
mod layers;
mod marginalization;
mod ngram_pruning;
mod second_order;
mod token_graphs;
mod topdown;
mod viterbi;

// Core differentiable operations
pub use forward_score::{forward_score, log_sum_exp_paths};
pub use gradient::{backward, ArcGradient, GradientAccumulator, GradientWfst};
pub use viterbi::{viterbi_path_with_grad, viterbi_score, ViterbiGradResult};

// WFST convolutional layers
pub use layers::{
    wfst_conv_backward, wfst_conv_forward, wfst_conv_forward_with_gradients, PaddingMode,
    ReceptiveField, WfstConvConfig, WfstConvLabel, WfstConvLayer, WfstConvOutput, WfstKernel,
};

// Token graph variants for CTC-like training
pub use token_graphs::{
    build_blank_graph, build_token_graph, build_vocabulary_graph, TokenGraphConfig,
    TokenGraphStats, TokenGraphType, TokenId, BLANK_TOKEN,
};

// Marginalized word piece decompositions
pub use marginalization::{
    build_character_lexicon, build_identity_lexicon, build_lexicon_transducer, build_target_graph,
    marginalized_loss, GraphemeId, LexiconConfig, LexiconEntry, MarginalizationContext,
    MarginalizationResult, MarginalizationStats, WordPieceId,
};

// N-gram transitions with pruning
pub use ngram_pruning::{
    build_pruned_bigram_graph, build_pruned_trigram_graph, NgramCounts, PrunedNgramConfig,
    PrunedNgramStats,
};

// Second-order differentiation
pub use second_order::{
    compute_diagonal_hessian, compute_fisher_information, gradient_and_hessian,
    hessian_vector_product, natural_gradient, HessianMatrix, SecondOrderConfig, SecondOrderResult,
    SecondOrderWfst,
};

// k2-style top-down differentiation
pub use topdown::{
    composed_backward, count_arcs, forward_backward, pruned_search_backward, topdown_backward,
    BackwardStats, ComposedArcMap, ComposedBackwardResult, ComposedState, ForwardBackwardScores,
    PrunedBackwardConfig, PrunedSearchResult, SparseGradient,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::{LogWeight, Semiring};
    use crate::wfst::{MutableWfst, VectorWfst};

    #[test]
    fn test_forward_score_single_path() {
        // Single path: 0 --a/-1.0--> 1 (final)
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);

        // Single path weight = -1.0 + 0.0 (final) = -1.0
        assert!((score.value() - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_forward_score_two_paths() {
        // Two paths from 0 to 1: weight 1.0 and 2.0
        // LogWeight stores negative log probabilities (positive values = valid probs < 1)
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0)); // prob e^-1
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(2.0)); // prob e^-2

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = forward_score(&grad_fst);

        // Forward score = -log(e^-1 + e^-2) ≈ 0.687
        let expected = -((-1.0_f64).exp() + (-2.0_f64).exp()).ln();
        assert!((score.value() - expected).abs() < 1e-6);
    }

    #[test]
    fn test_viterbi_score() {
        // Two paths: weight -1.0 and -2.0
        // Viterbi = min(-1.0, -2.0) = -2.0
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));
        fst.add_arc(s0, Some('b'), Some('b'), s1, LogWeight::new(-2.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let score = viterbi_score(&grad_fst);

        // Best path = -2.0
        assert!((score.value() - (-2.0)).abs() < 1e-6);
    }

    #[test]
    fn test_backward_gradients() {
        // Single path: gradient should be 1.0
        let mut fst = VectorWfst::<char, LogWeight>::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s1, LogWeight::one());
        fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(-1.0));

        let grad_fst = GradientWfst::from_wfst(&fst);
        let _ = forward_score(&grad_fst);
        let gradients = backward(&grad_fst);

        // Single arc should have gradient 1.0
        assert_eq!(gradients.arc_gradients.len(), 1);
        let grad = gradients.arc_gradients[0].gradient;
        assert!((grad - 1.0).abs() < 1e-6);
    }
}
