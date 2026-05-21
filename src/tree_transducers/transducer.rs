//! Tree transducer trait and implementations.

use std::collections::HashMap;
use std::hash::Hash;

use super::{StateId, Tree, TreeChild, TreePattern, TreeRule};
use crate::semiring::Semiring;

/// State information for a tree transducer.
#[derive(Debug, Clone)]
pub struct TransducerState<W: Semiring> {
    /// Whether this is a final state.
    pub is_final: bool,
    /// Final weight (only meaningful if is_final).
    pub final_weight: W,
}

impl<W: Semiring> TransducerState<W> {
    /// Create a non-final state.
    pub fn non_final() -> Self {
        Self {
            is_final: false,
            final_weight: W::zero(),
        }
    }

    /// Create a final state with the given weight.
    pub fn final_with_weight(weight: W) -> Self {
        Self {
            is_final: true,
            final_weight: weight,
        }
    }
}

impl<W: Semiring> Default for TransducerState<W> {
    fn default() -> Self {
        Self::non_final()
    }
}

/// Trait for weighted tree transducers.
pub trait WeightedTreeTransducer<L, W>: Clone + Send + Sync
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring,
{
    /// Get the initial state.
    fn start(&self) -> StateId;

    /// Check if a state is final.
    fn is_final(&self, state: StateId) -> bool;

    /// Get the final weight for a state.
    fn final_weight(&self, state: StateId) -> W;

    /// Get rules for a given state and input symbol.
    fn rules(&self, state: StateId, symbol: &L) -> Vec<&TreeRule<L, W>>;

    /// Get all rules in the transducer.
    fn all_rules(&self) -> Vec<&TreeRule<L, W>>;

    /// Get the number of states.
    fn num_states(&self) -> usize;

    /// Get the number of rules.
    fn num_rules(&self) -> usize;
}

/// Extension trait for tree transducer operations.
pub trait TreeTransducerOps<L, W>: WeightedTreeTransducer<L, W>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    /// Apply the transducer to an input tree.
    ///
    /// Returns all possible output trees with their weights.
    fn transduce(&self, input: &Tree<L>) -> Vec<(Tree<L>, W)> {
        self.transduce_from_state(self.start(), input)
    }

    /// Apply the transducer starting from a specific state.
    fn transduce_from_state(&self, state: StateId, input: &Tree<L>) -> Vec<(Tree<L>, W)> {
        let mut results = Vec::new();

        // Find applicable rules
        for rule in self.rules(state, input.label()) {
            // Check arity matches
            if rule.input_arity != input.arity() {
                continue;
            }

            // Try to apply this rule
            if let Some(outputs) = self.apply_rule(rule, input) {
                results.extend(outputs);
            }
        }

        // If state is final and input is a leaf, we might be done
        if self.is_final(state) && input.is_leaf() && results.is_empty() {
            results.push((input.clone(), self.final_weight(state)));
        }

        results
    }

    /// Apply a single rule to an input tree.
    fn apply_rule(&self, rule: &TreeRule<L, W>, input: &Tree<L>) -> Option<Vec<(Tree<L>, W)>> {
        // Process input children to get their transduced outputs
        let mut child_outputs: Vec<Vec<(Tree<L>, W)>> = Vec::new();

        for child in &rule.output_pattern.children {
            match child {
                TreeChild::Variable { state, var_index } => {
                    if *var_index >= input.arity() {
                        return None;
                    }
                    let child_input = &input.children()[*var_index];
                    let outputs = self.transduce_from_state(*state, child_input);
                    if outputs.is_empty() {
                        return None;
                    }
                    child_outputs.push(outputs);
                }
                TreeChild::Subtree(pattern) => {
                    // Fixed subtree - instantiate it
                    let tree = instantiate_pattern(pattern);
                    child_outputs.push(vec![(tree, W::one())]);
                }
            }
        }

        // Compute cartesian product of child outputs
        let combinations = cartesian_product(child_outputs);

        let mut results = Vec::new();
        for (children, child_weight) in combinations {
            let output = Tree::node(rule.output_pattern.symbol.clone(), children);
            let weight = rule.weight.clone().times(&child_weight);
            results.push((output, weight));
        }

        Some(results)
    }
}

// Implement TreeTransducerOps for all types implementing WeightedTreeTransducer
impl<T, L, W> TreeTransducerOps<L, W> for T
where
    T: WeightedTreeTransducer<L, W>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
}

/// Convert a TreePattern to a Tree (for fixed subtrees).
fn instantiate_pattern<L: Clone>(pattern: &TreePattern<L>) -> Tree<L> {
    let children: Vec<Tree<L>> = pattern
        .children
        .iter()
        .filter_map(|child| {
            match child {
                TreeChild::Subtree(sub) => Some(instantiate_pattern(sub)),
                TreeChild::Variable { .. } => None, // Variables shouldn't appear in fixed patterns
            }
        })
        .collect();

    Tree::node(pattern.symbol.clone(), children)
}

/// Compute cartesian product of vectors of weighted items.
fn cartesian_product<L: Clone, W: Semiring + Clone>(
    items: Vec<Vec<(Tree<L>, W)>>,
) -> Vec<(Vec<Tree<L>>, W)> {
    if items.is_empty() {
        return vec![(Vec::new(), W::one())];
    }

    let mut result = vec![(Vec::new(), W::one())];

    for item_vec in items {
        let mut new_result = Vec::new();

        for (prefix, prefix_weight) in &result {
            for (item, item_weight) in &item_vec {
                let mut new_prefix = prefix.clone();
                new_prefix.push(item.clone());
                let new_weight = prefix_weight.clone().times(item_weight);
                new_result.push((new_prefix, new_weight));
            }
        }

        result = new_result;
    }

    result
}

/// Vector-based weighted tree transducer implementation.
#[derive(Debug, Clone)]
pub struct VectorTreeTransducer<L, W: Semiring> {
    /// States indexed by ID.
    states: Vec<TransducerState<W>>,
    /// Rules indexed by (state, input_symbol).
    rules: Vec<TreeRule<L, W>>,
    /// Index from (state, symbol) to rule indices.
    rule_index: HashMap<(StateId, L), Vec<usize>>,
    /// Initial state.
    start: StateId,
}

impl<L: Clone + Eq + Hash, W: Semiring + Clone> VectorTreeTransducer<L, W> {
    /// Create a new empty transducer.
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            rules: Vec::new(),
            rule_index: HashMap::new(),
            start: 0,
        }
    }

    /// Add a state.
    pub fn add_state(&mut self) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(TransducerState::non_final());
        id
    }

    /// Add a state with final weight.
    pub fn add_final_state(&mut self, weight: W) -> StateId {
        let id = self.states.len() as StateId;
        self.states.push(TransducerState::final_with_weight(weight));
        id
    }

    /// Set the initial state.
    pub fn set_start(&mut self, state: StateId) {
        self.start = state;
    }

    /// Make a state final.
    pub fn set_final(&mut self, state: StateId, weight: W) {
        if let Some(s) = self.states.get_mut(state as usize) {
            s.is_final = true;
            s.final_weight = weight;
        }
    }

    /// Add a rule.
    pub fn add_rule(&mut self, rule: TreeRule<L, W>) {
        let key = (rule.state, rule.input_symbol.clone());
        let idx = self.rules.len();
        self.rule_index.entry(key).or_default().push(idx);
        self.rules.push(rule);
    }
}

impl<L: Clone + Eq + Hash, W: Semiring> Default for VectorTreeTransducer<L, W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L, W> WeightedTreeTransducer<L, W> for VectorTreeTransducer<L, W>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    fn start(&self) -> StateId {
        self.start
    }

    fn is_final(&self, state: StateId) -> bool {
        self.states
            .get(state as usize)
            .map(|s| s.is_final)
            .unwrap_or(false)
    }

    fn final_weight(&self, state: StateId) -> W {
        self.states
            .get(state as usize)
            .map(|s| s.final_weight.clone())
            .unwrap_or_else(W::zero)
    }

    fn rules(&self, state: StateId, symbol: &L) -> Vec<&TreeRule<L, W>> {
        self.rule_index
            .get(&(state, symbol.clone()))
            .map(|indices| indices.iter().map(|&i| &self.rules[i]).collect())
            .unwrap_or_default()
    }

    fn all_rules(&self) -> Vec<&TreeRule<L, W>> {
        self.rules.iter().collect()
    }

    fn num_states(&self) -> usize {
        self.states.len()
    }

    fn num_rules(&self) -> usize {
        self.rules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    fn make_simple_transducer() -> VectorTreeTransducer<&'static str, TropicalWeight> {
        let mut tt = VectorTreeTransducer::new();

        // State 0: initial
        let s0 = tt.add_state();
        tt.set_start(s0);
        tt.set_final(s0, TropicalWeight::one());

        // Rule: q0(S(x0, x1)) -> T(x1, x0)  (swap children)
        let pattern = TreePattern::new(
            "T",
            vec![TreeChild::variable(0, 1), TreeChild::variable(0, 0)],
        );

        tt.add_rule(TreeRule::new(0, "S", 2, pattern, TropicalWeight::one()));

        // Rule for leaves
        tt.add_rule(TreeRule::new(
            0,
            "a",
            0,
            TreePattern::leaf("a"),
            TropicalWeight::one(),
        ));
        tt.add_rule(TreeRule::new(
            0,
            "b",
            0,
            TreePattern::leaf("b"),
            TropicalWeight::one(),
        ));

        tt
    }

    #[test]
    fn test_transducer_creation() {
        let tt = make_simple_transducer();

        assert_eq!(tt.num_states(), 1);
        assert_eq!(tt.num_rules(), 3);
        assert_eq!(tt.start(), 0);
        assert!(tt.is_final(0));
    }

    #[test]
    fn test_rules_lookup() {
        let tt = make_simple_transducer();

        let s_rules = tt.rules(0, &"S");
        assert_eq!(s_rules.len(), 1);

        let a_rules = tt.rules(0, &"a");
        assert_eq!(a_rules.len(), 1);

        let unknown_rules = tt.rules(0, &"unknown");
        assert!(unknown_rules.is_empty());
    }

    #[test]
    fn test_transduce_leaf() {
        let tt = make_simple_transducer();

        let input = Tree::leaf("a");
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0.label(), &"a");
    }

    #[test]
    fn test_transduce_swap() {
        let tt = make_simple_transducer();

        // Input: S(a, b)
        let input = Tree::node("S", vec![Tree::leaf("a"), Tree::leaf("b")]);

        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        // Output should be: T(b, a)
        assert_eq!(outputs[0].0.label(), &"T");
        assert_eq!(outputs[0].0.children()[0].label(), &"b");
        assert_eq!(outputs[0].0.children()[1].label(), &"a");
    }

    #[test]
    fn test_cartesian_product() {
        let items: Vec<Vec<(Tree<&str>, TropicalWeight)>> = vec![
            vec![
                (Tree::leaf("a"), TropicalWeight::one()),
                (Tree::leaf("b"), TropicalWeight::new(1.0)),
            ],
            vec![(Tree::leaf("x"), TropicalWeight::one())],
        ];

        let product = cartesian_product(items);

        assert_eq!(product.len(), 2);
        assert_eq!(product[0].0.len(), 2);
        assert_eq!(product[1].0.len(), 2);
    }

    #[test]
    fn test_instantiate_pattern() {
        let pattern: TreePattern<&str> = TreePattern::new(
            "S",
            vec![
                TreeChild::subtree(TreePattern::leaf("NP")),
                TreeChild::subtree(TreePattern::new(
                    "VP",
                    vec![TreeChild::subtree(TreePattern::leaf("V"))],
                )),
            ],
        );

        let tree = instantiate_pattern(&pattern);

        assert_eq!(tree.label(), &"S");
        assert_eq!(tree.arity(), 2);
        assert_eq!(tree.children()[0].label(), &"NP");
        assert_eq!(tree.children()[1].label(), &"VP");
    }
}
