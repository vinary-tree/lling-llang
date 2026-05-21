//! Multi-tape labels for multi-tape WFSTs.

use std::fmt::{self, Debug, Display};
use std::hash::Hash;

/// A label for multi-tape transitions.
///
/// Each tape can have `Some(label)` or `None` (epsilon).
/// The const generic `N` specifies the number of tapes.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MultiTapeLabel<L, const N: usize> {
    /// Labels for each tape. `None` represents epsilon.
    pub labels: [Option<L>; N],
}

impl<L, const N: usize> MultiTapeLabel<L, N> {
    /// Create a new multi-tape label with all epsilon.
    pub fn epsilon() -> Self
    where
        L: Clone,
    {
        Self {
            labels: std::array::from_fn(|_| None),
        }
    }

    /// Create a new multi-tape label from an array.
    pub fn new(labels: [Option<L>; N]) -> Self {
        Self { labels }
    }

    /// Create a label from concrete values (all non-epsilon).
    pub fn from_values(values: [L; N]) -> Self
    where
        L: Clone,
    {
        Self {
            labels: values.map(Some),
        }
    }

    /// Check if all tapes are epsilon.
    pub fn is_epsilon(&self) -> bool {
        self.labels.iter().all(|l| l.is_none())
    }

    /// Check if a specific tape is epsilon.
    pub fn is_tape_epsilon(&self, tape: usize) -> bool {
        tape < N && self.labels[tape].is_none()
    }

    /// Get the label on a specific tape.
    pub fn tape(&self, index: usize) -> Option<&L> {
        self.labels.get(index).and_then(|l| l.as_ref())
    }

    /// Get a mutable reference to a tape label.
    pub fn tape_mut(&mut self, index: usize) -> Option<&mut Option<L>> {
        self.labels.get_mut(index)
    }

    /// Set the label on a specific tape.
    pub fn set_tape(&mut self, index: usize, label: Option<L>) {
        if index < N {
            self.labels[index] = label;
        }
    }

    /// Get the number of tapes.
    pub const fn num_tapes(&self) -> usize {
        N
    }

    /// Count non-epsilon tapes.
    pub fn non_epsilon_count(&self) -> usize {
        self.labels.iter().filter(|l| l.is_some()).count()
    }

    /// Map a function over all labels.
    pub fn map<F, M>(&self, f: F) -> MultiTapeLabel<M, N>
    where
        F: Fn(&L) -> M,
        L: Clone,
    {
        MultiTapeLabel {
            labels: std::array::from_fn(|i| self.labels[i].as_ref().map(&f)),
        }
    }

    /// Check if two labels match on non-epsilon positions.
    ///
    /// Returns true if for every tape where both labels are non-epsilon,
    /// the labels are equal.
    pub fn matches(&self, other: &Self) -> bool
    where
        L: PartialEq,
    {
        self.labels
            .iter()
            .zip(other.labels.iter())
            .all(|(a, b)| match (a, b) {
                (Some(x), Some(y)) => x == y,
                _ => true,
            })
    }
}

impl<L: Clone, const N: usize> MultiTapeLabel<L, N> {
    /// Create a label with a single non-epsilon tape.
    pub fn single(tape: usize, label: L) -> Self {
        let mut result = Self::epsilon();
        if tape < N {
            result.labels[tape] = Some(label);
        }
        result
    }

    /// Create a label with two non-epsilon tapes.
    pub fn pair(tape1: usize, label1: L, tape2: usize, label2: L) -> Self {
        let mut result = Self::epsilon();
        if tape1 < N {
            result.labels[tape1] = Some(label1);
        }
        if tape2 < N {
            result.labels[tape2] = Some(label2);
        }
        result
    }
}

impl<L: Debug, const N: usize> Debug for MultiTapeLabel<L, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for (i, label) in self.labels.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            match label {
                Some(l) => write!(f, "{:?}", l)?,
                None => write!(f, "ε")?,
            }
        }
        write!(f, "]")
    }
}

impl<L: Display, const N: usize> Display for MultiTapeLabel<L, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for (i, label) in self.labels.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            match label {
                Some(l) => write!(f, "{}", l)?,
                None => write!(f, "ε")?,
            }
        }
        write!(f, "]")
    }
}

impl<L, const N: usize> Default for MultiTapeLabel<L, N>
where
    L: Clone,
{
    fn default() -> Self {
        Self::epsilon()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epsilon_label() {
        let label: MultiTapeLabel<char, 3> = MultiTapeLabel::epsilon();
        assert!(label.is_epsilon());
        assert_eq!(label.non_epsilon_count(), 0);
    }

    #[test]
    fn test_from_values() {
        let label = MultiTapeLabel::from_values(['a', 'b', 'c']);
        assert!(!label.is_epsilon());
        assert_eq!(label.non_epsilon_count(), 3);
        assert_eq!(label.tape(0), Some(&'a'));
        assert_eq!(label.tape(1), Some(&'b'));
        assert_eq!(label.tape(2), Some(&'c'));
    }

    #[test]
    fn test_single_tape() {
        let label: MultiTapeLabel<char, 3> = MultiTapeLabel::single(1, 'x');
        assert!(!label.is_epsilon());
        assert!(label.is_tape_epsilon(0));
        assert!(!label.is_tape_epsilon(1));
        assert!(label.is_tape_epsilon(2));
        assert_eq!(label.tape(1), Some(&'x'));
    }

    #[test]
    fn test_pair_tapes() {
        let label: MultiTapeLabel<char, 4> = MultiTapeLabel::pair(0, 'a', 2, 'c');
        assert_eq!(label.tape(0), Some(&'a'));
        assert_eq!(label.tape(1), None);
        assert_eq!(label.tape(2), Some(&'c'));
        assert_eq!(label.tape(3), None);
    }

    #[test]
    fn test_new() {
        let label = MultiTapeLabel::new([Some('a'), None, Some('c')]);
        assert_eq!(label.tape(0), Some(&'a'));
        assert_eq!(label.tape(1), None);
        assert_eq!(label.tape(2), Some(&'c'));
    }

    #[test]
    fn test_set_tape() {
        let mut label: MultiTapeLabel<char, 3> = MultiTapeLabel::epsilon();
        label.set_tape(1, Some('x'));
        assert_eq!(label.tape(1), Some(&'x'));
        label.set_tape(1, None);
        assert_eq!(label.tape(1), None);
    }

    #[test]
    fn test_num_tapes() {
        let label: MultiTapeLabel<char, 5> = MultiTapeLabel::epsilon();
        assert_eq!(label.num_tapes(), 5);
    }

    #[test]
    fn test_map() {
        let label = MultiTapeLabel::from_values([1, 2, 3]);
        let mapped = label.map(|&x| x * 2);
        assert_eq!(mapped.tape(0), Some(&2));
        assert_eq!(mapped.tape(1), Some(&4));
        assert_eq!(mapped.tape(2), Some(&6));
    }

    #[test]
    fn test_matches() {
        let label1 = MultiTapeLabel::new([Some('a'), None, Some('c')]);
        let label2 = MultiTapeLabel::new([Some('a'), Some('b'), Some('c')]);
        let label3 = MultiTapeLabel::new([Some('x'), None, Some('c')]);

        // Matches because non-epsilon positions are equal
        assert!(label1.matches(&label2));

        // Doesn't match because tape 0 differs
        assert!(!label1.matches(&label3));
    }

    #[test]
    fn test_display() {
        let label = MultiTapeLabel::new([Some('a'), None, Some('c')]);
        let s = format!("{}", label);
        assert_eq!(s, "[a, ε, c]");
    }

    #[test]
    fn test_debug() {
        let label = MultiTapeLabel::new([Some('a'), None, Some('c')]);
        let s = format!("{:?}", label);
        assert_eq!(s, "['a', ε, 'c']");
    }

    #[test]
    fn test_equality() {
        let label1 = MultiTapeLabel::from_values(['a', 'b']);
        let label2 = MultiTapeLabel::from_values(['a', 'b']);
        let label3 = MultiTapeLabel::from_values(['a', 'c']);

        assert_eq!(label1, label2);
        assert_ne!(label1, label3);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;

        let label1 = MultiTapeLabel::from_values(['a', 'b']);
        let label2 = MultiTapeLabel::from_values(['a', 'b']);

        let mut set = HashSet::new();
        set.insert(label1.clone());
        assert!(set.contains(&label2));
    }
}
