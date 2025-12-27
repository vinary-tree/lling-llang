//! Weighted transition type for WFSTs.

use std::hash::Hash;

use crate::semiring::Semiring;
use super::StateId;

/// A weighted transition in a WFST.
///
/// Transitions connect states with input/output labels and a weight.
/// Epsilon (empty) transitions use `None` for the label.
///
/// # Type Parameters
///
/// - `L`: Label type (typically `char`, `u8`, or vocabulary ID)
/// - `W`: Weight type (must implement [`Semiring`])
///
/// # Example
///
/// ```
/// use lling_llang::wfst::WeightedTransition;
/// use lling_llang::semiring::TropicalWeight;
///
/// // Transition from state 0 to state 1 with label 'a' and weight 1.5
/// let t = WeightedTransition {
///     from: 0,
///     input: Some('a'),
///     output: Some('a'),
///     to: 1,
///     weight: TropicalWeight::new(1.5),
/// };
/// ```
#[derive(Clone, Debug)]
pub struct WeightedTransition<L, W: Semiring> {
    /// Source state ID.
    pub from: StateId,
    /// Input label (`None` for epsilon transition).
    pub input: Option<L>,
    /// Output label (`None` for epsilon transition).
    pub output: Option<L>,
    /// Target state ID.
    pub to: StateId,
    /// Weight of the transition.
    pub weight: W,
}

impl<L, W: Semiring> WeightedTransition<L, W> {
    /// Create a new weighted transition.
    #[inline]
    pub fn new(from: StateId, input: Option<L>, output: Option<L>, to: StateId, weight: W) -> Self {
        Self { from, input, output, to, weight }
    }

    /// Create an epsilon transition (no input or output label).
    #[inline]
    pub fn epsilon(from: StateId, to: StateId, weight: W) -> Self {
        Self { from, input: None, output: None, to, weight }
    }

    /// Check if this is an epsilon transition (no input label).
    #[inline]
    pub fn is_epsilon_input(&self) -> bool {
        self.input.is_none()
    }

    /// Check if this is an epsilon transition (no output label).
    #[inline]
    pub fn is_epsilon_output(&self) -> bool {
        self.output.is_none()
    }

    /// Check if this is a full epsilon transition (no input or output).
    #[inline]
    pub fn is_epsilon(&self) -> bool {
        self.input.is_none() && self.output.is_none()
    }
}

impl<L: Clone, W: Semiring> WeightedTransition<L, W> {
    /// Create a copy with a different weight.
    #[inline]
    pub fn with_weight(&self, weight: W) -> Self {
        Self {
            from: self.from,
            input: self.input.clone(),
            output: self.output.clone(),
            to: self.to,
            weight,
        }
    }
}

impl<L: PartialEq, W: Semiring> PartialEq for WeightedTransition<L, W> {
    fn eq(&self, other: &Self) -> bool {
        self.from == other.from
            && self.input == other.input
            && self.output == other.output
            && self.to == other.to
            && self.weight == other.weight
    }
}

impl<L: Eq, W: Semiring> Eq for WeightedTransition<L, W> {}

impl<L: Hash, W: Semiring> Hash for WeightedTransition<L, W> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.from.hash(state);
        self.input.hash(state);
        self.output.hash(state);
        self.to.hash(state);
        // Note: weight is not hashed as it's often floating-point
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_transition_creation() {
        let t: WeightedTransition<char, TropicalWeight> = WeightedTransition::new(
            0,
            Some('a'),
            Some('b'),
            1,
            TropicalWeight::new(1.5),
        );

        assert_eq!(t.from, 0);
        assert_eq!(t.input, Some('a'));
        assert_eq!(t.output, Some('b'));
        assert_eq!(t.to, 1);
        assert_eq!(t.weight.value(), 1.5);
    }

    #[test]
    fn test_epsilon_transition() {
        let t: WeightedTransition<char, TropicalWeight> = WeightedTransition::epsilon(
            0,
            1,
            TropicalWeight::one(),
        );

        assert!(t.is_epsilon());
        assert!(t.is_epsilon_input());
        assert!(t.is_epsilon_output());
    }

    #[test]
    fn test_partial_epsilon() {
        let t: WeightedTransition<char, TropicalWeight> = WeightedTransition::new(
            0,
            Some('a'),
            None,
            1,
            TropicalWeight::one(),
        );

        assert!(!t.is_epsilon());
        assert!(!t.is_epsilon_input());
        assert!(t.is_epsilon_output());
    }
}
