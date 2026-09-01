//! Tree transducer trait and implementations.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use super::rule::{TreeChild, TreePattern, TreeRule};
use super::tree::Tree;
use super::types::StateId;
use crate::semiring::Semiring;

/// Error returned by checked tree-transducer mutation methods.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeTransducerError {
    /// The transducer cannot represent another state with the `u32` state ID type.
    StateIdOverflow {
        /// Number of states already present in the transducer.
        num_states: usize,
    },
    /// A start state ID is not present in the transducer.
    InvalidStartState {
        /// Invalid state ID supplied by the caller.
        state: StateId,
        /// Number of states present in the transducer.
        num_states: usize,
    },
    /// A final state ID is not present in the transducer.
    InvalidFinalState {
        /// Invalid state ID supplied by the caller.
        state: StateId,
        /// Number of states present in the transducer.
        num_states: usize,
    },
    /// A rule belongs to a state that is not present in the transducer.
    InvalidRuleState {
        /// Invalid rule state.
        state: StateId,
        /// Number of states present in the transducer.
        num_states: usize,
    },
    /// A rule variable references a state that is not present in the transducer.
    InvalidVariableState {
        /// Invalid variable state.
        state: StateId,
        /// Number of states present in the transducer.
        num_states: usize,
    },
    /// A rule variable references an input child outside the rule's input arity.
    InvalidVariableIndex {
        /// Invalid variable index.
        var_index: usize,
        /// Rule input arity.
        input_arity: usize,
    },
}

impl fmt::Display for TreeTransducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StateIdOverflow { num_states } => write!(
                f,
                "tree transducer cannot represent {} states with u32 state IDs",
                num_states
            ),
            Self::InvalidStartState { state, num_states } => write!(
                f,
                "start state {} is invalid for tree transducer with {} states",
                state, num_states
            ),
            Self::InvalidFinalState { state, num_states } => write!(
                f,
                "final state {} is invalid for tree transducer with {} states",
                state, num_states
            ),
            Self::InvalidRuleState { state, num_states } => write!(
                f,
                "rule state {} is invalid for tree transducer with {} states",
                state, num_states
            ),
            Self::InvalidVariableState { state, num_states } => write!(
                f,
                "rule variable state {} is invalid for tree transducer with {} states",
                state, num_states
            ),
            Self::InvalidVariableIndex {
                var_index,
                input_arity,
            } => write!(
                f,
                "rule variable index {} is invalid for input arity {}",
                var_index, input_arity
            ),
        }
    }
}

impl std::error::Error for TreeTransducerError {}

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
        run_transducer_machine(
            self,
            MachineTask::State {
                state: self.start(),
                input,
            },
        )
        .expect("a state task always produces a result vector")
    }

    /// Apply the transducer starting from a specific state.
    fn transduce_from_state(&self, state: StateId, input: &Tree<L>) -> Vec<(Tree<L>, W)> {
        run_transducer_machine(self, MachineTask::State { state, input })
            .expect("a state task always produces a result vector")
    }

    /// Apply a single rule to an input tree.
    fn apply_rule(&self, rule: &TreeRule<L, W>, input: &Tree<L>) -> Option<Vec<(Tree<L>, W)>> {
        let mut outputs = run_transducer_machine(
            self,
            MachineTask::Pattern {
                pattern: &rule.output_pattern,
                input,
            },
        )?;
        for (_, child_weight) in &mut outputs {
            *child_weight = rule.weight.clone().times(child_weight);
        }
        Some(outputs)
    }

    /// Instantiate an output pattern against an input tree.
    fn instantiate_output_pattern(
        &self,
        pattern: &TreePattern<L>,
        input: &Tree<L>,
    ) -> Option<Vec<(Tree<L>, W)>> {
        run_transducer_machine(self, MachineTask::Pattern { pattern, input })
    }
}

type WeightedTrees<L, W> = Vec<(Tree<L>, W)>;

struct StateFrame<'a, L, W: Semiring> {
    state: StateId,
    input: &'a Tree<L>,
    rules: Vec<&'a TreeRule<L, W>>,
    next_rule: usize,
    results: WeightedTrees<L, W>,
}

struct PatternFrame<'a, L, W: Semiring> {
    pattern: &'a TreePattern<L>,
    input: &'a Tree<L>,
    next_child: usize,
    child_outputs: Vec<WeightedTrees<L, W>>,
}

enum MachineTask<'a, L, W: Semiring> {
    State {
        state: StateId,
        input: &'a Tree<L>,
    },
    Pattern {
        pattern: &'a TreePattern<L>,
        input: &'a Tree<L>,
    },
    ContinueState(StateFrame<'a, L, W>),
    ContinuePattern(PatternFrame<'a, L, W>),
}

enum Continuation<'a, L, W: Semiring> {
    StateRule {
        frame: StateFrame<'a, L, W>,
        rule_weight: W,
    },
    PatternChild(PatternFrame<'a, L, W>),
}

/// Execute the mutually recursive transducer equations as a specialized
/// defunctionalized pushdown automaton.  Each transition either consumes a rule,
/// consumes a pattern child, or returns across one explicit continuation frame.
fn run_transducer_machine<'a, T, L, W>(
    transducer: &'a T,
    root: MachineTask<'a, L, W>,
) -> Option<WeightedTrees<L, W>>
where
    T: WeightedTreeTransducer<L, W>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    let mut task = Some(root);
    let mut returned = None;
    let mut continuations = Vec::new();

    loop {
        if let Some(current) = task.take() {
            match current {
                MachineTask::State { state, input } => {
                    task = Some(MachineTask::ContinueState(StateFrame {
                        state,
                        input,
                        rules: transducer.rules(state, input.label()),
                        next_rule: 0,
                        results: Vec::new(),
                    }));
                }
                MachineTask::Pattern { pattern, input } => {
                    task = Some(MachineTask::ContinuePattern(PatternFrame {
                        pattern,
                        input,
                        next_child: 0,
                        child_outputs: Vec::with_capacity(pattern.children.len()),
                    }));
                }
                MachineTask::ContinueState(mut frame) => {
                    let mut applicable = None;
                    while let Some(rule) = frame.rules.get(frame.next_rule).copied() {
                        frame.next_rule += 1;
                        if rule.input_arity == frame.input.arity() {
                            applicable = Some(rule);
                            break;
                        }
                    }

                    if let Some(rule) = applicable {
                        let input = frame.input;
                        continuations.push(Continuation::StateRule {
                            frame,
                            rule_weight: rule.weight.clone(),
                        });
                        task = Some(MachineTask::Pattern {
                            pattern: &rule.output_pattern,
                            input,
                        });
                    } else {
                        if transducer.is_final(frame.state)
                            && frame.input.is_leaf()
                            && frame.results.is_empty()
                        {
                            frame
                                .results
                                .push((frame.input.clone(), transducer.final_weight(frame.state)));
                        }
                        returned = Some(Some(frame.results));
                    }
                }
                MachineTask::ContinuePattern(mut frame) => {
                    if let Some(child) = frame.pattern.children.get(frame.next_child) {
                        frame.next_child += 1;
                        match child {
                            TreeChild::Variable { state, var_index } => {
                                if let Some(child_input) = frame.input.children().get(*var_index) {
                                    continuations.push(Continuation::PatternChild(frame));
                                    task = Some(MachineTask::State {
                                        state: *state,
                                        input: child_input,
                                    });
                                } else {
                                    returned = Some(None);
                                }
                            }
                            TreeChild::Subtree(subpattern) => {
                                let input = frame.input;
                                continuations.push(Continuation::PatternChild(frame));
                                task = Some(MachineTask::Pattern {
                                    pattern: subpattern,
                                    input,
                                });
                            }
                        }
                    } else {
                        let combinations = cartesian_product(frame.child_outputs);
                        let mut results = Vec::with_capacity(combinations.len());
                        for (children, child_weight) in combinations {
                            results.push((
                                Tree::node(frame.pattern.symbol.clone(), children),
                                child_weight,
                            ));
                        }
                        returned = Some(Some(results));
                    }
                }
            }
            continue;
        }

        let value = returned
            .take()
            .expect("a completed machine task always returns one value");
        let Some(continuation) = continuations.pop() else {
            return value;
        };
        match continuation {
            Continuation::StateRule {
                mut frame,
                rule_weight,
            } => {
                if let Some(outputs) = value {
                    frame.results.reserve(outputs.len());
                    frame
                        .results
                        .extend(outputs.into_iter().map(|(tree, child_weight)| {
                            (tree, rule_weight.clone().times(&child_weight))
                        }));
                }
                task = Some(MachineTask::ContinueState(frame));
            }
            Continuation::PatternChild(mut frame) => match value {
                Some(outputs) if !outputs.is_empty() => {
                    frame.child_outputs.push(outputs);
                    task = Some(MachineTask::ContinuePattern(frame));
                }
                Some(_) | None => returned = Some(None),
            },
        }
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

/// Compute cartesian product of vectors of weighted items.
fn cartesian_product<L: Clone, W: Semiring + Clone>(
    items: Vec<Vec<(Tree<L>, W)>>,
) -> Vec<(Vec<Tree<L>>, W)> {
    if items.is_empty() {
        return vec![(Vec::new(), W::one())];
    }

    let mut result = vec![(Vec::new(), W::one())];

    for mut item_vec in items {
        // The overwhelmingly common deterministic case has one accumulated
        // prefix and one choice.  Move both trees instead of cloning the entire
        // already-built output subtree at every input level.
        if result.len() == 1 && item_vec.len() == 1 {
            let (mut prefix, prefix_weight) = result
                .pop()
                .expect("the singleton prefix was length-checked");
            let (item, item_weight) = item_vec
                .pop()
                .expect("the singleton choice was length-checked");
            prefix.push(item);
            result.push((prefix, prefix_weight.times(&item_weight)));
            continue;
        }

        // The output of this round is exactly `result × item_vec`; preallocate it.
        let mut new_result = Vec::with_capacity(result.len().saturating_mul(item_vec.len()));

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
        self.try_add_state().unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to add a state.
    pub fn try_add_state(&mut self) -> Result<StateId, TreeTransducerError> {
        let id = usize_to_state_id(self.states.len())?;
        self.states.push(TransducerState::non_final());
        Ok(id)
    }

    /// Add a state with final weight.
    pub fn add_final_state(&mut self, weight: W) -> StateId {
        self.try_add_final_state(weight)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to add a state with final weight.
    pub fn try_add_final_state(&mut self, weight: W) -> Result<StateId, TreeTransducerError> {
        let id = usize_to_state_id(self.states.len())?;
        self.states.push(TransducerState::final_with_weight(weight));
        Ok(id)
    }

    /// Set the initial state.
    pub fn set_start(&mut self, state: StateId) {
        let _ = self.try_set_start(state);
    }

    /// Try to set the initial state.
    pub fn try_set_start(&mut self, state: StateId) -> Result<(), TreeTransducerError> {
        if state as usize >= self.states.len() {
            return Err(TreeTransducerError::InvalidStartState {
                state,
                num_states: self.states.len(),
            });
        }

        self.start = state;
        Ok(())
    }

    /// Make a state final.
    pub fn set_final(&mut self, state: StateId, weight: W) {
        let _ = self.try_set_final(state, weight);
    }

    /// Try to make a state final.
    pub fn try_set_final(&mut self, state: StateId, weight: W) -> Result<(), TreeTransducerError> {
        let num_states = self.states.len();
        let Some(s) = self.states.get_mut(state as usize) else {
            return Err(TreeTransducerError::InvalidFinalState { state, num_states });
        };

        s.is_final = true;
        s.final_weight = weight;
        Ok(())
    }

    /// Add a rule.
    pub fn add_rule(&mut self, rule: TreeRule<L, W>) {
        self.push_rule(rule);
    }

    /// Try to add a rule.
    pub fn try_add_rule(&mut self, rule: TreeRule<L, W>) -> Result<(), TreeTransducerError> {
        self.validate_rule(&rule)?;
        self.push_rule(rule);
        Ok(())
    }

    fn push_rule(&mut self, rule: TreeRule<L, W>) {
        let key = (rule.state, rule.input_symbol.clone());
        let idx = self.rules.len();
        self.rule_index.entry(key).or_default().push(idx);
        self.rules.push(rule);
    }

    fn validate_rule(&self, rule: &TreeRule<L, W>) -> Result<(), TreeTransducerError> {
        if rule.state as usize >= self.states.len() {
            return Err(TreeTransducerError::InvalidRuleState {
                state: rule.state,
                num_states: self.states.len(),
            });
        }

        validate_pattern_states(&rule.output_pattern, rule.input_arity, self.states.len())
    }
}

fn validate_pattern_states<L>(
    pattern: &TreePattern<L>,
    input_arity: usize,
    num_states: usize,
) -> Result<(), TreeTransducerError> {
    let mut pending: Vec<&TreeChild<L>> = pattern.children.iter().rev().collect();
    while let Some(child) = pending.pop() {
        match child {
            TreeChild::Variable { state, var_index } => {
                if *state as usize >= num_states {
                    return Err(TreeTransducerError::InvalidVariableState {
                        state: *state,
                        num_states,
                    });
                }
                if *var_index >= input_arity {
                    return Err(TreeTransducerError::InvalidVariableIndex {
                        var_index: *var_index,
                        input_arity,
                    });
                }
            }
            TreeChild::Subtree(subtree) => {
                pending.extend(subtree.children.iter().rev());
            }
        }
    }

    Ok(())
}

fn usize_to_state_id(value: usize) -> Result<StateId, TreeTransducerError> {
    if value <= StateId::MAX as usize {
        Ok(value as StateId)
    } else {
        Err(TreeTransducerError::StateIdOverflow { num_states: value })
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
    use proptest::prelude::*;

    const DEEP_PATTERN_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    fn nested_variable_pattern(
        depth: usize,
        state: StateId,
        var_index: usize,
    ) -> TreePattern<&'static str> {
        let mut pattern = TreePattern::new("Leaf", vec![TreeChild::variable(state, var_index)]);
        for _ in 0..depth {
            pattern = TreePattern::new("Node", vec![TreeChild::subtree(pattern)]);
        }
        pattern
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn checked_rule_validation_refines_recursive_state_and_index_rules(
            depth in 0usize..128,
            variable_state in 0u32..4,
            var_index in 0usize..4,
        ) {
            let mut transducer: VectorTreeTransducer<&str, TropicalWeight> =
                VectorTreeTransducer::new();
            transducer.add_state();
            transducer.add_state();
            let result = transducer.try_add_rule(TreeRule::new(
                0,
                "Input",
                2,
                nested_variable_pattern(depth, variable_state, var_index),
                TropicalWeight::one(),
            ));
            let expected = if variable_state >= 2 {
                Err(TreeTransducerError::InvalidVariableState {
                    state: variable_state,
                    num_states: 2,
                })
            } else if var_index >= 2 {
                Err(TreeTransducerError::InvalidVariableIndex {
                    var_index,
                    input_arity: 2,
                })
            } else {
                Ok(())
            };
            prop_assert_eq!(result, expected);
        }
    }

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
    fn test_checked_state_mutation_rejects_invalid_states() {
        let mut tt: VectorTreeTransducer<&str, TropicalWeight> = VectorTreeTransducer::new();

        assert_eq!(
            tt.try_set_start(0),
            Err(TreeTransducerError::InvalidStartState {
                state: 0,
                num_states: 0,
            })
        );
        assert_eq!(
            tt.try_set_final(0, TropicalWeight::one()),
            Err(TreeTransducerError::InvalidFinalState {
                state: 0,
                num_states: 0,
            })
        );

        tt.set_start(0);
        tt.set_final(0, TropicalWeight::one());
        assert_eq!(tt.start(), 0);
        assert!(!tt.is_final(0));
    }

    #[test]
    fn test_try_add_rule_rejects_invalid_rule_state() {
        let mut tt: VectorTreeTransducer<&str, TropicalWeight> = VectorTreeTransducer::new();
        let rule = TreeRule::new(0, "S", 0, TreePattern::leaf("T"), TropicalWeight::one());

        assert_eq!(
            tt.try_add_rule(rule),
            Err(TreeTransducerError::InvalidRuleState {
                state: 0,
                num_states: 0,
            })
        );
        assert_eq!(tt.num_rules(), 0);
    }

    #[test]
    fn test_try_add_rule_rejects_invalid_variable_state() {
        let mut tt: VectorTreeTransducer<&str, TropicalWeight> = VectorTreeTransducer::new();
        tt.add_state();
        let rule = TreeRule::new(
            0,
            "S",
            1,
            TreePattern::new("T", vec![TreeChild::variable(9, 0)]),
            TropicalWeight::one(),
        );

        assert_eq!(
            tt.try_add_rule(rule),
            Err(TreeTransducerError::InvalidVariableState {
                state: 9,
                num_states: 1,
            })
        );
        assert_eq!(tt.num_rules(), 0);
    }

    #[test]
    fn test_try_add_rule_rejects_invalid_variable_index() {
        let mut tt: VectorTreeTransducer<&str, TropicalWeight> = VectorTreeTransducer::new();
        tt.add_state();
        let rule = TreeRule::new(
            0,
            "S",
            1,
            TreePattern::new("T", vec![TreeChild::variable(0, 1)]),
            TropicalWeight::one(),
        );

        assert_eq!(
            tt.try_add_rule(rule),
            Err(TreeTransducerError::InvalidVariableIndex {
                var_index: 1,
                input_arity: 1,
            })
        );
        assert_eq!(tt.num_rules(), 0);
    }

    #[test]
    fn deep_checked_rule_validation_uses_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-tree-pattern-validation".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let mut transducer: VectorTreeTransducer<&str, TropicalWeight> =
                    VectorTreeTransducer::new();
                transducer.add_state();
                let rule = TreeRule::new(
                    0,
                    "Input",
                    1,
                    nested_variable_pattern(DEEP_PATTERN_DEPTH, 0, 0),
                    TropicalWeight::one(),
                );
                transducer
                    .try_add_rule(rule)
                    .expect("the deep pattern contains only a valid variable");
                assert_eq!(transducer.num_rules(), 1);
                drop(transducer);
            })
            .expect("the bounded-stack pattern-validation worker must spawn")
            .join()
            .expect("pattern validation and lifecycle must not overflow the native stack");
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
    fn test_fixed_subtree_pattern() {
        let tt = make_simple_transducer();
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

        let outputs = tt
            .instantiate_output_pattern(&pattern, &Tree::leaf("unused"))
            .expect("fixed pattern should instantiate");

        assert_eq!(outputs.len(), 1);
        let tree = &outputs[0].0;
        assert_eq!(tree.label(), &"S");
        assert_eq!(tree.arity(), 2);
        assert_eq!(tree.children()[0].label(), &"NP");
        assert_eq!(tree.children()[1].label(), &"VP");
    }

    #[test]
    fn test_nested_variable_in_subtree_pattern_is_preserved() {
        let mut tt: VectorTreeTransducer<&str, TropicalWeight> = VectorTreeTransducer::new();

        let s0 = tt.add_state();
        tt.set_start(s0);
        tt.set_final(s0, TropicalWeight::one());

        tt.add_rule(TreeRule::new(
            s0,
            "S",
            1,
            TreePattern::new(
                "T",
                vec![TreeChild::subtree(TreePattern::new(
                    "U",
                    vec![TreeChild::variable(s0, 0)],
                ))],
            ),
            TropicalWeight::one(),
        ));
        tt.add_rule(TreeRule::new(
            s0,
            "a",
            0,
            TreePattern::leaf("a"),
            TropicalWeight::one(),
        ));

        let input = Tree::node("S", vec![Tree::leaf("a")]);
        let outputs = tt.transduce(&input);

        assert_eq!(outputs.len(), 1);
        assert_eq!(format!("{}", outputs[0].0), "T(U(a))");
    }
}
