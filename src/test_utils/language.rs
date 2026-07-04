//! Language equivalence checking for WFSTs.
//!
//! This module provides utilities for checking whether two WFSTs accept
//! the same language (set of string pairs with weights).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::semiring::Semiring;
use crate::wfst::{StateId, VectorWfst, Wfst};

// =============================================================================
// Path Representation
// =============================================================================

/// A path through a WFST.
#[derive(Clone, Debug, PartialEq)]
pub struct Path<L, W> {
    /// Input string (sequence of labels).
    pub input: Vec<L>,
    /// Output string (sequence of labels).
    pub output: Vec<L>,
    /// Total weight of the path.
    pub weight: W,
}

impl<L, W: Semiring> Path<L, W> {
    /// Create a new path.
    pub fn new(input: Vec<L>, output: Vec<L>, weight: W) -> Self {
        Self {
            input,
            output,
            weight,
        }
    }

    /// Create an empty path (accepts empty string).
    pub fn empty() -> Self {
        Self {
            input: Vec::new(),
            output: Vec::new(),
            weight: W::one(),
        }
    }
}

impl<L: Eq + std::hash::Hash, W: Semiring> std::hash::Hash for Path<L, W>
where
    W: std::hash::Hash,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.input.hash(state);
        self.output.hash(state);
        // Note: weight is not hashed due to floating-point issues
    }
}

impl<L: Eq, W: Semiring + PartialEq> Eq for Path<L, W> {}

// =============================================================================
// Path Enumeration
// =============================================================================

/// Enumerate all accepting paths in a WFST up to a maximum length.
///
/// This is exponential in path length and should only be used for small WFSTs.
///
/// # Parameters
///
/// - `fst`: The WFST to enumerate paths from
/// - `max_length`: Maximum path length (number of transitions)
///
/// # Returns
///
/// A vector of all accepting paths up to the given length.
pub fn enumerate_paths<L, W>(fst: &VectorWfst<L, W>, max_length: usize) -> Vec<Path<L, W>>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    if fst.is_empty() {
        return Vec::new();
    }

    let mut paths = Vec::new();
    let start = fst.start();

    // State for DFS: (state, input_path, output_path, weight, depth)
    let mut stack: Vec<(StateId, Vec<L>, Vec<L>, W, usize)> =
        vec![(start, Vec::new(), Vec::new(), W::one(), 0)];

    while let Some((state, input, output, weight, depth)) = stack.pop() {
        // Check if this is a final state
        if fst.is_final(state) {
            let final_weight = weight.times(&fst.final_weight(state));
            if !final_weight.is_zero() {
                paths.push(Path::new(input.clone(), output.clone(), final_weight));
            }
        }

        // Don't explore beyond max_length
        if depth >= max_length {
            continue;
        }

        // Explore transitions
        for trans in fst.transitions(state) {
            let mut new_input = input.clone();
            let mut new_output = output.clone();

            if let Some(ref label) = trans.input {
                new_input.push(label.clone());
            }
            if let Some(ref label) = trans.output {
                new_output.push(label.clone());
            }

            let new_weight = weight.times(&trans.weight);
            if !new_weight.is_zero() {
                stack.push((trans.to, new_input, new_output, new_weight, depth + 1));
            }
        }
    }

    paths
}

/// Enumerate all unique input/output string pairs accepted by the WFST.
///
/// For each unique (input, output) pair, returns the weight. If multiple
/// paths produce the same pair, weights are combined using semiring plus.
pub fn enumerate_transduction<L, W>(
    fst: &VectorWfst<L, W>,
    max_length: usize,
) -> HashMap<(Vec<L>, Vec<L>), W>
where
    L: Clone + Send + Sync + Eq + std::hash::Hash,
    W: Semiring,
{
    let paths = enumerate_paths(fst, max_length);
    let mut transduction: HashMap<(Vec<L>, Vec<L>), W> = HashMap::new();

    for path in paths {
        let key = (path.input, path.output);
        transduction
            .entry(key)
            .and_modify(|w| *w = w.plus(&path.weight))
            .or_insert(path.weight);
    }

    transduction
}

// =============================================================================
// Language Equivalence
// =============================================================================

/// Check if two WFSTs accept the same language (up to a maximum path length).
///
/// Two WFSTs are language-equivalent if:
/// 1. They accept the same set of input/output string pairs
/// 2. Each pair has approximately equal weights
///
/// # Warning
///
/// This is exponential in `max_length` and should only be used for small WFSTs.
pub fn language_eq<L, W>(
    fst1: &VectorWfst<L, W>,
    fst2: &VectorWfst<L, W>,
    max_length: usize,
    epsilon: f64,
) -> bool
where
    L: Clone + Send + Sync + Eq + std::hash::Hash,
    W: Semiring,
{
    let trans1 = enumerate_transduction(fst1, max_length);
    let trans2 = enumerate_transduction(fst2, max_length);

    // Check that both have the same keys
    if trans1.len() != trans2.len() {
        return false;
    }

    for (key, weight1) in &trans1 {
        match trans2.get(key) {
            Some(weight2) => {
                if !weight1.approx_eq(weight2, epsilon) {
                    return false;
                }
            }
            None => return false,
        }
    }

    true
}

/// Check if path weights are equivalent between two WFSTs.
///
/// This is a weaker check than `language_eq` - it only verifies that the
/// sum of all path weights is approximately equal.
pub fn path_weights_eq<L, W>(
    fst1: &VectorWfst<L, W>,
    fst2: &VectorWfst<L, W>,
    max_length: usize,
    epsilon: f64,
) -> bool
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let paths1 = enumerate_paths(fst1, max_length);
    let paths2 = enumerate_paths(fst2, max_length);

    // Sum all path weights
    let sum1 = paths1.iter().fold(W::zero(), |acc, p| acc.plus(&p.weight));
    let sum2 = paths2.iter().fold(W::zero(), |acc, p| acc.plus(&p.weight));

    sum1.approx_eq(&sum2, epsilon)
}

// =============================================================================
// String Acceptance
// =============================================================================

/// Check if a WFST accepts a given input string.
///
/// Returns the weight if accepted, None otherwise.
pub fn accepts_string<L, W>(fst: &VectorWfst<L, W>, input: &[L]) -> Option<W>
where
    L: Clone + Send + Sync + PartialEq,
    W: Semiring,
{
    if fst.is_empty() {
        return None;
    }

    // Track (state, position in input, accumulated weight)
    let mut frontier: Vec<(StateId, usize, W)> = vec![(fst.start(), 0, W::one())];
    let mut result = W::zero();

    while let Some((state, pos, weight)) = frontier.pop() {
        // Handle epsilon transitions
        for trans in fst.transitions(state) {
            if trans.input.is_none() {
                let new_weight = weight.times(&trans.weight);
                if !new_weight.is_zero() {
                    frontier.push((trans.to, pos, new_weight));
                }
            }
        }

        // If we've consumed all input, check for acceptance
        if pos == input.len() {
            if fst.is_final(state) {
                let final_weight = weight.times(&fst.final_weight(state));
                result = result.plus(&final_weight);
            }
            continue;
        }

        // Try to consume the next input symbol
        let next_symbol = &input[pos];
        for trans in fst.transitions(state) {
            if trans.input.as_ref() == Some(next_symbol) {
                let new_weight = weight.times(&trans.weight);
                if !new_weight.is_zero() {
                    frontier.push((trans.to, pos + 1, new_weight));
                }
            }
        }
    }

    if result.is_zero() {
        None
    } else {
        Some(result)
    }
}

/// Check if a WFST accepts a given input string with any output.
pub fn accepts_input<L, W>(fst: &VectorWfst<L, W>, input: &[L]) -> bool
where
    L: Clone + Send + Sync + PartialEq,
    W: Semiring,
{
    accepts_string(fst, input).is_some()
}

/// Get all outputs for a given input string.
pub fn transduce<L, W>(fst: &VectorWfst<L, W>, input: &[L]) -> Vec<(Vec<L>, W)>
where
    L: Clone + Send + Sync + PartialEq,
    W: Semiring,
{
    if fst.is_empty() {
        return Vec::new();
    }

    // Track (state, position in input, output so far, accumulated weight)
    let mut frontier: Vec<(StateId, usize, Vec<L>, W)> =
        vec![(fst.start(), 0, Vec::new(), W::one())];
    let mut results: Vec<(Vec<L>, W)> = Vec::new();

    while let Some((state, pos, output, weight)) = frontier.pop() {
        // Handle epsilon transitions
        for trans in fst.transitions(state) {
            if trans.input.is_none() {
                let new_weight = weight.times(&trans.weight);
                if !new_weight.is_zero() {
                    let mut new_output = output.clone();
                    if let Some(ref label) = trans.output {
                        new_output.push(label.clone());
                    }
                    frontier.push((trans.to, pos, new_output, new_weight));
                }
            }
        }

        // If we've consumed all input, check for acceptance
        if pos == input.len() {
            if fst.is_final(state) {
                let final_weight = weight.times(&fst.final_weight(state));
                if !final_weight.is_zero() {
                    results.push((output.clone(), final_weight));
                }
            }
            continue;
        }

        // Try to consume the next input symbol
        let next_symbol = &input[pos];
        for trans in fst.transitions(state) {
            if trans.input.as_ref() == Some(next_symbol) {
                let new_weight = weight.times(&trans.weight);
                if !new_weight.is_zero() {
                    let mut new_output = output.clone();
                    if let Some(ref label) = trans.output {
                        new_output.push(label.clone());
                    }
                    frontier.push((trans.to, pos + 1, new_output, new_weight));
                }
            }
        }
    }

    results
}

// =============================================================================
// Reachability
// =============================================================================

/// Get all states reachable from the start state.
pub fn reachable_states<L, W>(fst: &VectorWfst<L, W>) -> HashSet<StateId>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let mut reachable = HashSet::new();

    if fst.is_empty() {
        return reachable;
    }

    let mut queue = VecDeque::new();
    queue.push_back(fst.start());
    reachable.insert(fst.start());

    while let Some(state) = queue.pop_front() {
        for trans in fst.transitions(state) {
            if reachable.insert(trans.to) {
                queue.push_back(trans.to);
            }
        }
    }

    reachable
}

/// Get all states that can reach a final state.
pub fn productive_states<L, W>(fst: &VectorWfst<L, W>) -> HashSet<StateId>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let num_states = fst.num_states();
    let mut productive = HashSet::new();

    // Build reverse adjacency list
    let mut reverse_adj: Vec<Vec<StateId>> = vec![Vec::new(); num_states];
    for state in 0..num_states as StateId {
        for trans in fst.transitions(state) {
            reverse_adj[trans.to as usize].push(state);
        }
    }

    // BFS from final states
    let mut queue = VecDeque::new();
    for state in 0..num_states as StateId {
        if fst.is_final(state) {
            productive.insert(state);
            queue.push_back(state);
        }
    }

    while let Some(state) = queue.pop_front() {
        for &prev in &reverse_adj[state as usize] {
            if productive.insert(prev) {
                queue.push_back(prev);
            }
        }
    }

    productive
}

/// Get useful states (both reachable and productive).
pub fn useful_states<L, W>(fst: &VectorWfst<L, W>) -> HashSet<StateId>
where
    L: Clone + Send + Sync,
    W: Semiring,
{
    let reachable = reachable_states(fst);
    let productive = productive_states(fst);
    reachable.intersection(&productive).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{diamond_wfst, epsilon_wfst, linear_wfst};
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_enumerate_paths_linear() {
        let fst: VectorWfst<char, TropicalWeight> = linear_wfst(3);
        let paths = enumerate_paths(&fst, 10);

        // Linear FST with 3 states has one path: a -> b
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].input, vec!['a', 'b']);
        assert_eq!(paths[0].output, vec!['a', 'b']);
    }

    #[test]
    fn test_enumerate_paths_diamond() {
        let fst: VectorWfst<char, TropicalWeight> = diamond_wfst();
        let paths = enumerate_paths(&fst, 10);

        // Diamond has two paths
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_accepts_string() {
        let fst: VectorWfst<char, TropicalWeight> = linear_wfst(3);

        assert!(accepts_input(&fst, &['a', 'b']));
        assert!(!accepts_input(&fst, &['a']));
        assert!(!accepts_input(&fst, &['b']));
        assert!(!accepts_input(&fst, &['a', 'b', 'c']));
    }

    #[test]
    fn test_accepts_epsilon() {
        let fst: VectorWfst<char, TropicalWeight> = epsilon_wfst();

        // Epsilon WFST accepts just 'a'
        assert!(accepts_input(&fst, &['a']));
        assert!(!accepts_input(&fst, &[]));
    }

    #[test]
    fn test_language_eq() {
        let fst1: VectorWfst<char, TropicalWeight> = linear_wfst(3);
        let fst2: VectorWfst<char, TropicalWeight> = linear_wfst(3);

        assert!(language_eq(&fst1, &fst2, 10, 1e-10));
    }

    #[test]
    fn test_reachable_states() {
        let fst: VectorWfst<char, TropicalWeight> = linear_wfst(4);
        let reachable = reachable_states(&fst);

        assert_eq!(reachable.len(), 4);
        for i in 0..4 {
            assert!(reachable.contains(&(i as StateId)));
        }
    }

    #[test]
    fn test_useful_states() {
        let fst: VectorWfst<char, TropicalWeight> = linear_wfst(4);
        let useful = useful_states(&fst);

        // All states in a linear FST should be useful
        assert_eq!(useful.len(), 4);
    }
}
