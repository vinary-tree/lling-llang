//! Weighted Tree Transducers for syntax tree transformations.
//!
//! This module implements weighted tree transducers (WTTs) for transforming
//! weighted tree languages. Tree transducers generalize string transducers
//! to handle hierarchical structures like syntax trees.
//!
//! # Mathematical Definition
//!
//! A weighted tree transducer is a tuple T = (Q, Σ, Δ, q₀, F, R, ρ) where:
//! - Q: Finite set of states
//! - Σ: Input ranked alphabet (symbols with arities)
//! - Δ: Output ranked alphabet
//! - q₀: Initial state
//! - F ⊆ Q: Final states
//! - R: Set of weighted rules
//! - ρ: Final weight function
//!
//! # Rule Format
//!
//! Rules have the form:
//! ```text
//! q(σ(x₁,...,xₙ)) → δ(q₁(xπ(1)),...,qₘ(xπ(m))), w
//! ```
//!
//! Where variables can be reordered, copied, or deleted.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::tree_transducers::{Tree, TreeTransducerBuilder, VectorTreeTransducer};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Build a tree transducer for syntax normalization
//! let mut builder = TreeTransducerBuilder::<String, TropicalWeight>::new();
//!
//! // Add rule: q(S(NP(x), VP(y))) → S(NP(x), VP(y)), 1.0
//! builder.add_rule(
//!     0,  // state
//!     "S".to_string(),  // input symbol
//!     vec![0, 1],  // input children states
//!     "S".to_string(),  // output symbol
//!     vec![TreeChild::Variable(0, 0), TreeChild::Variable(1, 1)],  // output children
//!     TropicalWeight::one(),
//! );
//!
//! let transducer = builder.build();
//! let output = transducer.transduce(&input_tree);
//! ```
//!
//! # Applications
//!
//! - Syntax-based machine translation
//! - Parse tree normalization
//! - AST-to-AST program transformation
//! - Tree-to-string linearization

mod alphabet;
mod builder;
mod rule;
mod transducer;
mod tree;
mod types;

pub use alphabet::{RankedAlphabet, SimpleAlphabet, Symbol};
pub use builder::{leaf, pattern, TreePatternBuilder, TreeTransducerBuilder};
pub use rule::{TreeChild, TreePattern, TreeRule};
pub use transducer::{
    TransducerState, TreeTransducerError, TreeTransducerOps, VectorTreeTransducer,
    WeightedTreeTransducer,
};
pub use tree::{Tree, TreeNode};
pub use types::StateId;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_module_imports() {
        // Verify module structure compiles
        let _tree: Tree<String> = Tree::leaf("test".to_string());
        let _builder: TreeTransducerBuilder<String, TropicalWeight> = TreeTransducerBuilder::new();
    }
}
