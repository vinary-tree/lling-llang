//! Tree transducer rules and patterns.

use std::fmt;
use std::hash::{Hash, Hasher};

use super::types::StateId;
use crate::semiring::Semiring;

/// A child element in a tree rule's right-hand side.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TreeChild<L> {
    /// A variable referring to a child subtree.
    ///
    /// Contains: (state to apply, variable index from input)
    Variable {
        /// State to process this subtree with.
        state: StateId,
        /// Index of the input variable (0-indexed).
        var_index: usize,
    },
    /// A fixed subtree pattern.
    Subtree(Box<TreePattern<L>>),
}

impl<L> TreeChild<L> {
    /// Create a variable child.
    pub fn variable(state: StateId, var_index: usize) -> Self {
        TreeChild::Variable { state, var_index }
    }

    /// Create a subtree child.
    pub fn subtree(pattern: TreePattern<L>) -> Self {
        TreeChild::Subtree(Box::new(pattern))
    }

    /// Check if this is a variable.
    pub fn is_variable(&self) -> bool {
        matches!(self, TreeChild::Variable { .. })
    }

    /// Get the variable index if this is a variable.
    pub fn var_index(&self) -> Option<usize> {
        match self {
            TreeChild::Variable { var_index, .. } => Some(*var_index),
            TreeChild::Subtree(_) => None,
        }
    }

    /// Get the state if this is a variable.
    pub fn state(&self) -> Option<StateId> {
        match self {
            TreeChild::Variable { state, .. } => Some(*state),
            TreeChild::Subtree(_) => None,
        }
    }
}

/// A tree pattern for matching or producing trees.
pub struct TreePattern<L> {
    /// The root symbol of the pattern.
    pub symbol: L,
    /// Children of the pattern.
    pub children: Vec<TreeChild<L>>,
}

impl<L: Clone> Clone for TreePattern<L> {
    fn clone(&self) -> Self {
        struct Frame<'a, L> {
            source: &'a TreePattern<L>,
            next_child: usize,
            children: Vec<TreeChild<L>>,
        }
        let mut frames = vec![Frame {
            source: self,
            next_child: 0,
            children: Vec::with_capacity(self.children.len()),
        }];
        loop {
            let frame = frames
                .last_mut()
                .expect("the root pattern clone frame remains until completion");
            if let Some(child) = frame.source.children.get(frame.next_child) {
                frame.next_child += 1;
                match child {
                    TreeChild::Variable { state, var_index } => {
                        frame.children.push(TreeChild::Variable {
                            state: *state,
                            var_index: *var_index,
                        });
                    }
                    TreeChild::Subtree(subtree) => frames.push(Frame {
                        source: subtree,
                        next_child: 0,
                        children: Vec::with_capacity(subtree.children.len()),
                    }),
                }
                continue;
            }
            let completed = frames
                .pop()
                .expect("a completed pattern clone frame is present");
            let pattern = TreePattern {
                symbol: completed.source.symbol.clone(),
                children: completed.children,
            };
            if let Some(parent) = frames.last_mut() {
                parent.children.push(TreeChild::Subtree(Box::new(pattern)));
            } else {
                return pattern;
            }
        }
    }
}

impl<L: PartialEq> PartialEq for TreePattern<L> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            if left.symbol != right.symbol || left.children.len() != right.children.len() {
                return false;
            }
            for (left, right) in left.children.iter().zip(&right.children) {
                match (left, right) {
                    (
                        TreeChild::Variable {
                            state: ls,
                            var_index: lv,
                        },
                        TreeChild::Variable {
                            state: rs,
                            var_index: rv,
                        },
                    ) if ls == rs && lv == rv => {}
                    (TreeChild::Subtree(left), TreeChild::Subtree(right)) => {
                        pending.push((left, right));
                    }
                    _ => return false,
                }
            }
        }
        true
    }
}

impl<L: Eq> Eq for TreePattern<L> {}

impl<L: Hash> Hash for TreePattern<L> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        enum Item<'a, L> {
            Pattern(&'a TreePattern<L>),
            Child(&'a TreeChild<L>),
        }
        let mut pending = vec![Item::Pattern(self)];
        while let Some(item) = pending.pop() {
            match item {
                Item::Pattern(pattern) => {
                    pattern.symbol.hash(state);
                    pattern.children.len().hash(state);
                    pending.extend(pattern.children.iter().rev().map(Item::Child));
                }
                Item::Child(TreeChild::Variable {
                    state: child_state,
                    var_index,
                }) => {
                    std::mem::discriminant(&TreeChild::<L>::Variable {
                        state: *child_state,
                        var_index: *var_index,
                    })
                    .hash(state);
                    child_state.hash(state);
                    var_index.hash(state);
                }
                Item::Child(child @ TreeChild::Subtree(subtree)) => {
                    std::mem::discriminant(child).hash(state);
                    pending.push(Item::Pattern(subtree));
                }
            }
        }
    }
}

impl<L: fmt::Debug> fmt::Debug for TreePattern<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, L> {
            Pattern(&'a TreePattern<L>),
            Children(&'a [TreeChild<L>], usize),
            Text(&'static str),
        }
        let mut events = vec![Event::Pattern(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pattern(pattern) => {
                    write!(
                        f,
                        "TreePattern {{ symbol: {:?}, children: [",
                        pattern.symbol
                    )?;
                    events.push(Event::Children(&pattern.children, 0));
                }
                Event::Children(children, index) => {
                    if index == children.len() {
                        write!(f, "] }}")?;
                    } else {
                        if index > 0 {
                            write!(f, ", ")?;
                        }
                        events.push(Event::Children(children, index + 1));
                        match &children[index] {
                            TreeChild::Variable { state, var_index } => write!(
                                f,
                                "Variable {{ state: {state:?}, var_index: {var_index:?} }}"
                            )?,
                            TreeChild::Subtree(subtree) => {
                                write!(f, "Subtree(")?;
                                events.push(Event::Text(")"));
                                events.push(Event::Pattern(subtree));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl<L> Drop for TreePattern<L> {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        for child in std::mem::take(&mut self.children) {
            if let TreeChild::Subtree(subtree) = child {
                pending.push(subtree);
            }
        }
        while let Some(mut pattern) = pending.pop() {
            for child in std::mem::take(&mut pattern.children) {
                if let TreeChild::Subtree(subtree) = child {
                    pending.push(subtree);
                }
            }
        }
    }
}

impl<L> TreePattern<L> {
    /// Create a new pattern.
    pub fn new(symbol: L, children: Vec<TreeChild<L>>) -> Self {
        Self { symbol, children }
    }

    /// Create a leaf pattern (no children).
    pub fn leaf(symbol: L) -> Self {
        Self {
            symbol,
            children: Vec::new(),
        }
    }

    /// Get the arity of this pattern.
    pub fn arity(&self) -> usize {
        self.children.len()
    }

    /// Check if this is a leaf pattern.
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

impl<L: Clone> TreePattern<L> {
    /// Get all variable indices used in this pattern.
    pub fn variable_indices(&self) -> Vec<usize> {
        let mut indices = Vec::new();
        self.collect_var_indices(&mut indices);
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    fn collect_var_indices(&self, indices: &mut Vec<usize>) {
        let mut pending: Vec<&TreeChild<L>> = self.children.iter().rev().collect();
        while let Some(child) = pending.pop() {
            match child {
                TreeChild::Variable { var_index, .. } => {
                    indices.push(*var_index);
                }
                TreeChild::Subtree(pattern) => {
                    pending.extend(pattern.children.iter().rev());
                }
            }
        }
    }
}

/// A weighted tree transducer rule.
///
/// Format:
/// $`q(\sigma(x_1,\ldots,x_n))\to\mathrm{pattern},w`$.
///
/// Where:
/// - q is the current state
/// - $`\sigma`$ is the input symbol with arity $`n`$.
/// - $`x_1,\ldots,x_n`$ are the input variable children.
/// - pattern is the output tree pattern
/// - w is the weight
#[derive(Debug, Clone)]
pub struct TreeRule<L, W: Semiring> {
    /// The state this rule applies in.
    pub state: StateId,
    /// The input symbol to match.
    pub input_symbol: L,
    /// Number of input children (arity of input symbol).
    pub input_arity: usize,
    /// The output pattern.
    pub output_pattern: TreePattern<L>,
    /// The rule weight.
    pub weight: W,
}

impl<L, W: Semiring> TreeRule<L, W> {
    /// Create a new rule.
    pub fn new(
        state: StateId,
        input_symbol: L,
        input_arity: usize,
        output_pattern: TreePattern<L>,
        weight: W,
    ) -> Self {
        Self {
            state,
            input_symbol,
            input_arity,
            output_pattern,
            weight,
        }
    }
}

impl<L: Clone, W: Semiring + Clone> TreeRule<L, W> {
    /// Check if this rule is deleting (some input variables not used).
    pub fn is_deleting(&self) -> bool {
        let used = self.output_pattern.variable_indices();
        used.len() < self.input_arity
    }

    /// Check if this rule is copying (some input variables used multiple times).
    pub fn is_copying(&self) -> bool {
        let mut all_indices = Vec::new();
        self.output_pattern.collect_var_indices(&mut all_indices);
        let unique_count = {
            let mut sorted = all_indices.clone();
            sorted.sort_unstable();
            sorted.dedup();
            sorted.len()
        };
        all_indices.len() > unique_count
    }

    /// Check if this rule is linear (each input variable used exactly once).
    pub fn is_linear(&self) -> bool {
        !self.is_deleting() && !self.is_copying()
    }
}

impl<L: Clone + PartialEq, W: Semiring> TreeRule<L, W> {
    /// Check if this rule preserves the input structure (identity-like).
    pub fn is_identity_like(&self) -> bool {
        if self.input_symbol != self.output_pattern.symbol {
            return false;
        }
        if self.input_arity != self.output_pattern.arity() {
            return false;
        }

        // Check if variables are used in order
        for (i, child) in self.output_pattern.children.iter().enumerate() {
            match child {
                TreeChild::Variable { var_index, .. } if *var_index == i => continue,
                _ => return false,
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_tree_child_variable() {
        let child: TreeChild<&str> = TreeChild::variable(0, 1);
        assert!(child.is_variable());
        assert_eq!(child.var_index(), Some(1));
        assert_eq!(child.state(), Some(0));
    }

    #[test]
    fn test_tree_child_subtree() {
        let pattern = TreePattern::leaf("x");
        let child: TreeChild<&str> = TreeChild::subtree(pattern);
        assert!(!child.is_variable());
        assert_eq!(child.var_index(), None);
    }

    #[test]
    fn test_tree_pattern() {
        let pattern: TreePattern<&str> = TreePattern::new(
            "S",
            vec![TreeChild::variable(0, 0), TreeChild::variable(0, 1)],
        );

        assert_eq!(pattern.symbol, "S");
        assert_eq!(pattern.arity(), 2);
        assert!(!pattern.is_leaf());
    }

    #[test]
    fn test_tree_pattern_leaf() {
        let leaf: TreePattern<&str> = TreePattern::leaf("x");
        assert!(leaf.is_leaf());
        assert_eq!(leaf.arity(), 0);
    }

    #[test]
    fn test_variable_indices() {
        let pattern: TreePattern<&str> = TreePattern::new(
            "S",
            vec![
                TreeChild::variable(0, 2),
                TreeChild::variable(0, 0),
                TreeChild::variable(0, 2), // duplicate
            ],
        );

        let indices = pattern.variable_indices();
        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn test_rule_creation() {
        let output = TreePattern::new("T", vec![TreeChild::variable(0, 0)]);

        let rule: TreeRule<&str, TropicalWeight> =
            TreeRule::new(0, "S", 2, output, TropicalWeight::one());

        assert_eq!(rule.state, 0);
        assert_eq!(rule.input_symbol, "S");
        assert_eq!(rule.input_arity, 2);
    }

    #[test]
    fn test_rule_is_deleting() {
        // Rule that uses only var 0 but input has arity 2
        let output = TreePattern::new("T", vec![TreeChild::variable(0, 0)]);

        let rule: TreeRule<&str, TropicalWeight> =
            TreeRule::new(0, "S", 2, output, TropicalWeight::one());

        assert!(rule.is_deleting());
    }

    #[test]
    fn test_rule_is_copying() {
        // Rule that uses var 0 twice
        let output = TreePattern::new(
            "T",
            vec![TreeChild::variable(0, 0), TreeChild::variable(0, 0)],
        );

        let rule: TreeRule<&str, TropicalWeight> =
            TreeRule::new(0, "S", 1, output, TropicalWeight::one());

        assert!(rule.is_copying());
    }

    #[test]
    fn test_rule_is_linear() {
        // Rule that uses each var exactly once
        let output = TreePattern::new(
            "T",
            vec![TreeChild::variable(0, 0), TreeChild::variable(0, 1)],
        );

        let rule: TreeRule<&str, TropicalWeight> =
            TreeRule::new(0, "S", 2, output, TropicalWeight::one());

        assert!(rule.is_linear());
    }

    #[test]
    fn test_rule_is_identity_like() {
        // Identity rule: S(x0, x1) -> S(x0, x1)
        let output = TreePattern::new(
            "S",
            vec![TreeChild::variable(0, 0), TreeChild::variable(0, 1)],
        );

        let rule: TreeRule<&str, TropicalWeight> =
            TreeRule::new(0, "S", 2, output, TropicalWeight::one());

        assert!(rule.is_identity_like());

        // Non-identity: S(x0, x1) -> T(x0, x1)
        let output2 = TreePattern::new(
            "T",
            vec![TreeChild::variable(0, 0), TreeChild::variable(0, 1)],
        );

        let rule2: TreeRule<&str, TropicalWeight> =
            TreeRule::new(0, "S", 2, output2, TropicalWeight::one());

        assert!(!rule2.is_identity_like());
    }
}
