//! Builder for tree transducers.

use std::hash::Hash;

use super::{
    StateId, TreeChild, TreePattern, TreeRule, VectorTreeTransducer, WeightedTreeTransducer,
};
use crate::semiring::Semiring;

/// Builder for constructing tree transducers.
#[derive(Debug, Clone)]
pub struct TreeTransducerBuilder<L, W: Semiring> {
    /// The transducer being built.
    transducer: VectorTreeTransducer<L, W>,
    /// Next state ID to allocate.
    next_state: StateId,
}

impl<L: Clone + Eq + Hash + Send + Sync, W: Semiring + Clone> TreeTransducerBuilder<L, W> {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            transducer: VectorTreeTransducer::new(),
            next_state: 0,
        }
    }

    /// Add a new state and return its ID.
    pub fn add_state(&mut self) -> StateId {
        let id = self.transducer.add_state();
        self.next_state = id + 1;
        id
    }

    /// Add a new final state with the given weight.
    pub fn add_final_state(&mut self, weight: W) -> StateId {
        let id = self.transducer.add_final_state(weight);
        self.next_state = id + 1;
        id
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) -> &mut Self {
        self.transducer.set_start(state);
        self
    }

    /// Make a state final with the given weight.
    pub fn set_final(&mut self, state: StateId, weight: W) -> &mut Self {
        self.transducer.set_final(state, weight);
        self
    }

    /// Add a rule to the transducer.
    pub fn add_rule(&mut self, rule: TreeRule<L, W>) -> &mut Self {
        self.transducer.add_rule(rule);
        self
    }

    /// Add a rule with explicit components.
    pub fn add_rule_parts(
        &mut self,
        state: StateId,
        input_symbol: L,
        input_arity: usize,
        output_pattern: TreePattern<L>,
        weight: W,
    ) -> &mut Self {
        self.add_rule(TreeRule::new(
            state,
            input_symbol,
            input_arity,
            output_pattern,
            weight,
        ))
    }

    /// Add an identity rule (copy input to output unchanged).
    pub fn add_identity_rule(
        &mut self,
        state: StateId,
        symbol: L,
        arity: usize,
        weight: W,
    ) -> &mut Self
    where
        L: Clone,
    {
        let children: Vec<TreeChild<L>> =
            (0..arity).map(|i| TreeChild::variable(state, i)).collect();
        let pattern = TreePattern::new(symbol.clone(), children);
        self.add_rule(TreeRule::new(state, symbol, arity, pattern, weight))
    }

    /// Add a relabeling rule (change symbol but keep structure).
    pub fn add_relabel_rule(
        &mut self,
        state: StateId,
        input_symbol: L,
        output_symbol: L,
        arity: usize,
        weight: W,
    ) -> &mut Self
    where
        L: Clone,
    {
        let children: Vec<TreeChild<L>> =
            (0..arity).map(|i| TreeChild::variable(state, i)).collect();
        let pattern = TreePattern::new(output_symbol, children);
        self.add_rule(TreeRule::new(state, input_symbol, arity, pattern, weight))
    }

    /// Add a deletion rule (map to a fixed output, ignoring input children).
    pub fn add_deletion_rule(
        &mut self,
        state: StateId,
        input_symbol: L,
        input_arity: usize,
        output_symbol: L,
        weight: W,
    ) -> &mut Self {
        let pattern = TreePattern::leaf(output_symbol);
        self.add_rule(TreeRule::new(
            state,
            input_symbol,
            input_arity,
            pattern,
            weight,
        ))
    }

    /// Add a copying rule (use same input variable multiple times).
    pub fn add_copy_rule(
        &mut self,
        state: StateId,
        input_symbol: L,
        input_arity: usize,
        output_symbol: L,
        output_var_pattern: &[usize],
        weight: W,
    ) -> &mut Self
    where
        L: Clone,
    {
        let children: Vec<TreeChild<L>> = output_var_pattern
            .iter()
            .map(|&i| TreeChild::variable(state, i))
            .collect();
        let pattern = TreePattern::new(output_symbol, children);
        self.add_rule(TreeRule::new(
            state,
            input_symbol,
            input_arity,
            pattern,
            weight,
        ))
    }

    /// Add a swap rule (reorder children).
    pub fn add_swap_rule(
        &mut self,
        state: StateId,
        input_symbol: L,
        output_symbol: L,
        permutation: &[usize],
        weight: W,
    ) -> &mut Self
    where
        L: Clone,
    {
        let arity = permutation.len();
        let children: Vec<TreeChild<L>> = permutation
            .iter()
            .map(|&i| TreeChild::variable(state, i))
            .collect();
        let pattern = TreePattern::new(output_symbol, children);
        self.add_rule(TreeRule::new(state, input_symbol, arity, pattern, weight))
    }

    /// Add a flattening rule (remove one level of nesting).
    ///
    /// E.g., S(S(x, y), z) -> S(x, y, z) (not supported with fixed arity)
    /// But we can do: S(S(x, y)) -> S(x, y) where outer S has arity 1
    pub fn add_flatten_rule(
        &mut self,
        state: StateId,
        symbol: L,
        inner_state: StateId,
        weight: W,
    ) -> &mut Self
    where
        L: Clone,
    {
        // This creates a rule that expects symbol(symbol(x, y)) -> symbol(x, y)
        // The outer symbol has arity 1, inner symbol has arity 2
        let children = vec![TreeChild::variable(inner_state, 0)];
        let pattern = TreePattern::new(symbol.clone(), children);
        self.add_rule(TreeRule::new(state, symbol, 1, pattern, weight))
    }

    /// Get the number of states added so far.
    pub fn num_states(&self) -> usize {
        self.transducer.num_states()
    }

    /// Get the number of rules added so far.
    pub fn num_rules(&self) -> usize {
        self.transducer.num_rules()
    }

    /// Build the transducer.
    pub fn build(self) -> VectorTreeTransducer<L, W> {
        self.transducer
    }
}

impl<L: Clone + Eq + Hash + Send + Sync, W: Semiring + Clone> Default
    for TreeTransducerBuilder<L, W>
{
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating tree patterns.
#[derive(Debug, Clone)]
pub struct TreePatternBuilder<L> {
    symbol: L,
    children: Vec<TreeChild<L>>,
}

impl<L> TreePatternBuilder<L> {
    /// Create a new pattern builder with the given root symbol.
    pub fn new(symbol: L) -> Self {
        Self {
            symbol,
            children: Vec::new(),
        }
    }

    /// Add a variable child.
    pub fn variable(mut self, state: StateId, var_index: usize) -> Self {
        self.children.push(TreeChild::variable(state, var_index));
        self
    }

    /// Add a fixed subtree child.
    pub fn subtree(mut self, pattern: TreePattern<L>) -> Self {
        self.children.push(TreeChild::subtree(pattern));
        self
    }

    /// Build the pattern.
    pub fn build(self) -> TreePattern<L> {
        TreePattern::new(self.symbol, self.children)
    }
}

/// Create a leaf pattern.
pub fn leaf<L>(symbol: L) -> TreePattern<L> {
    TreePattern::leaf(symbol)
}

/// Create a pattern builder.
pub fn pattern<L>(symbol: L) -> TreePatternBuilder<L> {
    TreePatternBuilder::new(symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use crate::tree_transducers::{Tree, TreeTransducerOps, WeightedTreeTransducer};

    #[test]
    fn test_builder_creation() {
        let builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();
        assert_eq!(builder.num_states(), 0);
        assert_eq!(builder.num_rules(), 0);
    }

    #[test]
    fn test_builder_add_states() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        let s1 = builder.add_final_state(TropicalWeight::one());

        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(builder.num_states(), 2);
    }

    #[test]
    fn test_builder_identity_rule() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        // Identity rule for "S" with arity 2
        builder.add_identity_rule(s0, "S", 2, TropicalWeight::one());
        builder.add_identity_rule(s0, "a", 0, TropicalWeight::one());
        builder.add_identity_rule(s0, "b", 0, TropicalWeight::one());

        let tt = builder.build();

        // Test transduction
        let input = Tree::node("S", vec![Tree::leaf("a"), Tree::leaf("b")]);
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0.label(), &"S");
        assert_eq!(outputs[0].0.children()[0].label(), &"a");
        assert_eq!(outputs[0].0.children()[1].label(), &"b");
    }

    #[test]
    fn test_builder_relabel_rule() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        // Relabel "S" to "T"
        builder.add_relabel_rule(s0, "S", "T", 2, TropicalWeight::one());
        builder.add_identity_rule(s0, "a", 0, TropicalWeight::one());
        builder.add_identity_rule(s0, "b", 0, TropicalWeight::one());

        let tt = builder.build();

        let input = Tree::node("S", vec![Tree::leaf("a"), Tree::leaf("b")]);
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0.label(), &"T");
    }

    #[test]
    fn test_builder_swap_rule() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        // Swap children: S(x0, x1) -> T(x1, x0)
        builder.add_swap_rule(s0, "S", "T", &[1, 0], TropicalWeight::one());
        builder.add_identity_rule(s0, "a", 0, TropicalWeight::one());
        builder.add_identity_rule(s0, "b", 0, TropicalWeight::one());

        let tt = builder.build();

        let input = Tree::node("S", vec![Tree::leaf("a"), Tree::leaf("b")]);
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0.label(), &"T");
        assert_eq!(outputs[0].0.children()[0].label(), &"b");
        assert_eq!(outputs[0].0.children()[1].label(), &"a");
    }

    #[test]
    fn test_builder_deletion_rule() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        // Delete children: S(x0, x1) -> DELETED
        builder.add_deletion_rule(s0, "S", 2, "DELETED", TropicalWeight::one());

        let tt = builder.build();

        let input = Tree::node("S", vec![Tree::leaf("a"), Tree::leaf("b")]);
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0.label(), &"DELETED");
        assert!(outputs[0].0.is_leaf());
    }

    #[test]
    fn test_builder_copy_rule() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        // Copy: S(x0) -> T(x0, x0)
        builder.add_copy_rule(s0, "S", 1, "T", &[0, 0], TropicalWeight::one());
        builder.add_identity_rule(s0, "a", 0, TropicalWeight::one());

        let tt = builder.build();

        let input = Tree::node("S", vec![Tree::leaf("a")]);
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0.label(), &"T");
        assert_eq!(outputs[0].0.arity(), 2);
        assert_eq!(outputs[0].0.children()[0].label(), &"a");
        assert_eq!(outputs[0].0.children()[1].label(), &"a");
    }

    #[test]
    fn test_pattern_builder() {
        let pattern: TreePattern<&str> = pattern("S").variable(0, 0).variable(0, 1).build();

        assert_eq!(pattern.symbol, "S");
        assert_eq!(pattern.arity(), 2);
        assert!(pattern.children[0].is_variable());
        assert!(pattern.children[1].is_variable());
    }

    #[test]
    fn test_pattern_with_subtree() {
        let inner = leaf("NP");
        let pattern: TreePattern<&str> = pattern("S").subtree(inner).variable(0, 0).build();

        assert_eq!(pattern.symbol, "S");
        assert_eq!(pattern.arity(), 2);
        assert!(!pattern.children[0].is_variable());
        assert!(pattern.children[1].is_variable());
    }

    #[test]
    fn test_complex_transducer() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        // Complex rule: S(NP(x), VP(y)) -> Sentence(Subject(x), Predicate(y))
        let output = pattern("Sentence")
            .subtree(pattern("Subject").variable(s0, 0).build())
            .subtree(pattern("Predicate").variable(s0, 1).build())
            .build();

        builder.add_rule_parts(s0, "S", 2, output, TropicalWeight::one());

        // Rules for NP and VP
        builder.add_identity_rule(s0, "NP", 1, TropicalWeight::one());
        builder.add_identity_rule(s0, "VP", 1, TropicalWeight::one());

        // Terminal rules
        builder.add_identity_rule(s0, "the", 0, TropicalWeight::one());
        builder.add_identity_rule(s0, "cat", 0, TropicalWeight::one());
        builder.add_identity_rule(s0, "runs", 0, TropicalWeight::one());

        let tt = builder.build();

        assert!(tt.num_states() > 0);
        assert!(tt.num_rules() > 0);
    }

    #[test]
    fn test_weighted_rules() {
        let mut builder: TreeTransducerBuilder<&str, TropicalWeight> = TreeTransducerBuilder::new();

        let s0 = builder.add_state();
        builder.set_start(s0);
        builder.set_final(s0, TropicalWeight::one());

        // Add two rules for same input with different weights
        builder.add_relabel_rule(s0, "S", "T1", 0, TropicalWeight::new(1.0));
        builder.add_relabel_rule(s0, "S", "T2", 0, TropicalWeight::new(2.0));

        let tt = builder.build();

        let input = Tree::leaf("S");
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 2);
        // Results should have different weights
        let weights: Vec<_> = outputs.iter().map(|(_, w)| w.value()).collect();
        assert!(weights.contains(&1.0));
        assert!(weights.contains(&2.0));
    }
}
