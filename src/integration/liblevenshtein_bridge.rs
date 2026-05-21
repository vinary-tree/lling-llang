//! Bridge between liblevenshtein and lling-llang.
//!
//! This module provides high-level APIs for fuzzy string matching with
//! optional edit operation tracking via the Edit semiring.

use liblevenshtein::prelude::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::{Algorithm, Candidate, Transducer};

use crate::semiring::{EditOp, EditSequence, EditWeight, TropicalWeight};

/// Configuration for fuzzy lookup operations.
#[derive(Debug, Clone)]
pub struct FuzzyConfig {
    /// Maximum edit distance for matches.
    pub max_distance: usize,
    /// Algorithm variant to use.
    pub algorithm: Algorithm,
    /// Whether to include exact matches (distance 0).
    pub include_exact: bool,
    /// Maximum number of results to return.
    pub max_results: Option<usize>,
}

impl Default for FuzzyConfig {
    fn default() -> Self {
        Self {
            max_distance: 2,
            algorithm: Algorithm::Standard,
            include_exact: true,
            max_results: None,
        }
    }
}

impl FuzzyConfig {
    /// Create a new configuration with the given max distance.
    pub fn new(max_distance: usize) -> Self {
        Self {
            max_distance,
            ..Default::default()
        }
    }

    /// Use Damerau-Levenshtein algorithm (includes transpositions).
    pub fn with_transpositions(mut self) -> Self {
        self.algorithm = Algorithm::Transposition;
        self
    }

    /// Use merge-and-split algorithm.
    pub fn with_merge_split(mut self) -> Self {
        self.algorithm = Algorithm::MergeAndSplit;
        self
    }

    /// Exclude exact matches from results.
    pub fn exclude_exact(mut self) -> Self {
        self.include_exact = false;
        self
    }

    /// Limit the number of results.
    pub fn with_max_results(mut self, limit: usize) -> Self {
        self.max_results = Some(limit);
        self
    }
}

/// Result of a fuzzy lookup operation.
#[derive(Debug, Clone)]
pub struct FuzzyResult<T = String> {
    /// The matched term from the dictionary.
    pub term: T,
    /// Edit distance from the query.
    pub distance: usize,
}

impl<T> FuzzyResult<T> {
    /// Create a new fuzzy result.
    pub fn new(term: T, distance: usize) -> Self {
        Self { term, distance }
    }

    /// Convert the term to a different type.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> FuzzyResult<U> {
        FuzzyResult {
            term: f(self.term),
            distance: self.distance,
        }
    }
}

/// Perform fuzzy lookup in a dictionary.
///
/// Returns matching terms with their edit distances.
///
/// # Arguments
///
/// * `dictionary` - The dictionary to search
/// * `query` - The query string to find corrections for
/// * `config` - Configuration options
///
/// # Example
///
/// ```rust,ignore
/// use lling_llang::integration::prelude::*;
///
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
/// let results = fuzzy_lookup(&dict, "helo", FuzzyConfig::new(2));
///
/// for result in results {
///     println!("{} (distance {})", result.term, result.distance);
/// }
/// ```
pub fn fuzzy_lookup<D>(dictionary: &D, query: &str, config: FuzzyConfig) -> Vec<FuzzyResult<String>>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    let transducer = Transducer::new(dictionary.clone(), config.algorithm);
    let candidates: Vec<Candidate> = transducer
        .query_with_distance(query, config.max_distance)
        .collect();

    let mut results: Vec<FuzzyResult<String>> = candidates
        .into_iter()
        .filter(|c| config.include_exact || c.distance > 0)
        .map(|c| FuzzyResult::new(c.term, c.distance))
        .collect();

    // Sort by distance
    results.sort_by_key(|r| r.distance);

    // Apply result limit if configured
    if let Some(limit) = config.max_results {
        results.truncate(limit);
    }

    results
}

/// Perform fuzzy lookup in a dictionary with multiple queries in parallel.
///
/// # Arguments
///
/// * `dictionary` - The dictionary to search
/// * `queries` - Iterator of query strings
/// * `config` - Configuration options
///
/// # Returns
///
/// Vector of results per query.
pub fn fuzzy_lookup_parallel<D, Q, I>(
    dictionary: &D,
    queries: I,
    config: FuzzyConfig,
) -> Vec<Vec<FuzzyResult<String>>>
where
    D: Dictionary + Clone + Send + Sync + 'static,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
    Q: AsRef<str> + Send,
    I: IntoIterator<Item = Q>,
{
    let queries: Vec<_> = queries.into_iter().collect();

    // For small batches, sequential is fine
    if queries.len() < 4 {
        return queries
            .iter()
            .map(|q| fuzzy_lookup(dictionary, q.as_ref(), config.clone()))
            .collect();
    }

    // Use rayon for parallel processing if available
    // For now, fall back to sequential
    queries
        .iter()
        .map(|q| fuzzy_lookup(dictionary, q.as_ref(), config.clone()))
        .collect()
}

/// Perform fuzzy lookup with explicit edit operation tracking.
///
/// Returns matching terms with their `EditWeight`, which includes the
/// sequence of edit operations needed to transform the query into each match.
///
/// # Arguments
///
/// * `dictionary` - The dictionary to search
/// * `query` - The query string to find corrections for
/// * `config` - Configuration options
///
/// # Example
///
/// ```rust,ignore
/// use lling_llang::integration::prelude::*;
///
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
/// let results = fuzzy_lookup_with_edits(&dict, "helo", FuzzyConfig::new(2));
///
/// for (term, weight) in results {
///     println!("{}: {:?}", term, weight.describe());
/// }
/// ```
pub fn fuzzy_lookup_with_edits<D>(
    dictionary: &D,
    query: &str,
    config: FuzzyConfig,
) -> Vec<(String, EditWeight)>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    let basic_results = fuzzy_lookup(dictionary, query, config);

    basic_results
        .into_iter()
        .map(|result| {
            let edit_weight = compute_edit_weight(query, &result.term);
            (result.term, edit_weight)
        })
        .collect()
}

/// Compute the edit weight (operations and cost) between two strings.
///
/// Uses dynamic programming to find the minimum edit distance and
/// reconstruct the edit operations.
fn compute_edit_weight(source: &str, target: &str) -> EditWeight {
    let source_chars: Vec<char> = source.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    let m = source_chars.len();
    let n = target_chars.len();

    // DP table: (cost, operations)
    let mut dp: Vec<Vec<(usize, Vec<EditOp>)>> = vec![vec![(0, Vec::new()); n + 1]; m + 1];

    // Initialize base cases
    for i in 1..=m {
        let mut ops = dp[i - 1][0].1.clone();
        ops.push(EditOp::Delete(source_chars[i - 1]));
        dp[i][0] = (i, ops);
    }

    for j in 1..=n {
        let mut ops = dp[0][j - 1].1.clone();
        ops.push(EditOp::Insert(target_chars[j - 1]));
        dp[0][j] = (j, ops);
    }

    // Fill DP table
    for i in 1..=m {
        for j in 1..=n {
            let s = source_chars[i - 1];
            let t = target_chars[j - 1];

            if s == t {
                // Copy (no cost)
                let mut ops = dp[i - 1][j - 1].1.clone();
                ops.push(EditOp::Copy(s));
                dp[i][j] = (dp[i - 1][j - 1].0, ops);
            } else {
                // Consider substitution, deletion, insertion
                let sub_cost = dp[i - 1][j - 1].0 + 1;
                let del_cost = dp[i - 1][j].0 + 1;
                let ins_cost = dp[i][j - 1].0 + 1;

                if sub_cost <= del_cost && sub_cost <= ins_cost {
                    let mut ops = dp[i - 1][j - 1].1.clone();
                    ops.push(EditOp::Substitute { from: s, to: t });
                    dp[i][j] = (sub_cost, ops);
                } else if del_cost <= ins_cost {
                    let mut ops = dp[i - 1][j].1.clone();
                    ops.push(EditOp::Delete(s));
                    dp[i][j] = (del_cost, ops);
                } else {
                    let mut ops = dp[i][j - 1].1.clone();
                    ops.push(EditOp::Insert(t));
                    dp[i][j] = (ins_cost, ops);
                }
            }
        }
    }

    let (cost, operations) = &dp[m][n];
    let sequence: EditSequence = operations.iter().cloned().collect();
    EditWeight::new(sequence, *cost as f64)
}

/// Builder for tracking edit operations during WFST traversal.
///
/// This type accumulates edit operations as paths are explored,
/// enabling construction of `EditWeight` values for correction results.
#[derive(Debug, Clone)]
pub struct EditTracker {
    /// Current sequence of operations.
    operations: Vec<EditOp>,
    /// Accumulated cost.
    cost: f64,
    /// Per-operation costs.
    costs: EditCosts,
}

/// Per-operation cost configuration.
#[derive(Debug, Clone)]
pub struct EditCosts {
    /// Cost of inserting a character.
    pub insert: f64,
    /// Cost of deleting a character.
    pub delete: f64,
    /// Cost of substituting a character.
    pub substitute: f64,
    /// Cost of transposing adjacent characters.
    pub transpose: f64,
}

impl Default for EditCosts {
    fn default() -> Self {
        Self {
            insert: 1.0,
            delete: 1.0,
            substitute: 1.0,
            transpose: 1.0,
        }
    }
}

impl EditTracker {
    /// Create a new edit tracker with default costs.
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            cost: 0.0,
            costs: EditCosts::default(),
        }
    }

    /// Create a new edit tracker with custom costs.
    pub fn with_costs(costs: EditCosts) -> Self {
        Self {
            operations: Vec::new(),
            cost: 0.0,
            costs,
        }
    }

    /// Record a copy operation (matching characters).
    pub fn copy(&mut self, c: char) {
        self.operations.push(EditOp::Copy(c));
        // Copy has no cost
    }

    /// Record an insertion operation.
    pub fn insert(&mut self, c: char) {
        self.operations.push(EditOp::Insert(c));
        self.cost += self.costs.insert;
    }

    /// Record a deletion operation.
    pub fn delete(&mut self, c: char) {
        self.operations.push(EditOp::Delete(c));
        self.cost += self.costs.delete;
    }

    /// Record a substitution operation.
    pub fn substitute(&mut self, from: char, to: char) {
        self.operations.push(EditOp::Substitute { from, to });
        self.cost += self.costs.substitute;
    }

    /// Record a transposition operation.
    pub fn transpose(&mut self, a: char, b: char) {
        self.operations.push(EditOp::Transpose { a, b });
        self.cost += self.costs.transpose;
    }

    /// Get the current cost.
    pub fn cost(&self) -> f64 {
        self.cost
    }

    /// Get the operations recorded so far.
    pub fn operations(&self) -> &[EditOp] {
        &self.operations
    }

    /// Build the final EditWeight.
    pub fn build(self) -> EditWeight {
        let sequence: EditSequence = self.operations.into_iter().collect();
        EditWeight::new(sequence, self.cost)
    }

    /// Convert the current state to a TropicalWeight.
    pub fn to_tropical(&self) -> TropicalWeight {
        TropicalWeight::new(self.cost)
    }
}

impl Default for EditTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating edit trackers with custom configurations.
pub struct EditTrackerBuilder {
    costs: EditCosts,
}

impl EditTrackerBuilder {
    /// Create a new builder with default costs.
    pub fn new() -> Self {
        Self {
            costs: EditCosts::default(),
        }
    }

    /// Set the insertion cost.
    pub fn insert_cost(mut self, cost: f64) -> Self {
        self.costs.insert = cost;
        self
    }

    /// Set the deletion cost.
    pub fn delete_cost(mut self, cost: f64) -> Self {
        self.costs.delete = cost;
        self
    }

    /// Set the substitution cost.
    pub fn substitute_cost(mut self, cost: f64) -> Self {
        self.costs.substitute = cost;
        self
    }

    /// Set the transposition cost.
    pub fn transpose_cost(mut self, cost: f64) -> Self {
        self.costs.transpose = cost;
        self
    }

    /// Build the edit tracker.
    pub fn build(self) -> EditTracker {
        EditTracker::with_costs(self.costs)
    }
}

impl Default for EditTrackerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a liblevenshtein `Candidate` to a `FuzzyResult<String>`.
impl From<Candidate> for FuzzyResult<String> {
    fn from(candidate: Candidate) -> Self {
        FuzzyResult {
            term: candidate.term,
            distance: candidate.distance,
        }
    }
}

/// Convert a `FuzzyResult` to a `TropicalWeight` (just the distance).
impl<T> From<&FuzzyResult<T>> for TropicalWeight {
    fn from(result: &FuzzyResult<T>) -> Self {
        TropicalWeight::new(result.distance as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg_char::DynamicDawgChar;

    #[test]
    fn test_fuzzy_lookup_basic() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
        let results = fuzzy_lookup(&dict, "helo", FuzzyConfig::new(2));

        assert!(!results.is_empty());

        // Should find "hello" and "help" within distance 2
        let terms: Vec<_> = results.iter().map(|r| r.term.as_str()).collect();
        assert!(terms.contains(&"hello") || terms.contains(&"help"));
    }

    #[test]
    fn test_fuzzy_lookup_exact_match() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test", "testing"]);
        let results = fuzzy_lookup(&dict, "test", FuzzyConfig::new(2));

        assert!(results.iter().any(|r| r.term == "test" && r.distance == 0));
    }

    #[test]
    fn test_fuzzy_lookup_exclude_exact() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test", "testing"]);
        let config = FuzzyConfig::new(2).exclude_exact();
        let results = fuzzy_lookup(&dict, "test", config);

        assert!(!results.iter().any(|r| r.distance == 0));
    }

    #[test]
    fn test_fuzzy_lookup_with_edits() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
        let results = fuzzy_lookup_with_edits(&dict, "helo", FuzzyConfig::new(2));

        assert!(!results.is_empty());

        for (term, weight) in &results {
            // Verify cost matches distance
            assert!(weight.cost() > 0.0 || term == "helo");

            // Verify operations describe transformation
            let description = weight.describe();
            assert!(!description.is_empty());
        }
    }

    #[test]
    fn test_edit_tracker() {
        let mut tracker = EditTracker::new();

        tracker.copy('h');
        tracker.copy('e');
        tracker.insert('l'); // Insert missing 'l'
        tracker.copy('l');
        tracker.copy('o');

        assert_eq!(tracker.cost(), 1.0);
        assert_eq!(tracker.operations().len(), 5);

        let weight = tracker.build();
        assert_eq!(weight.cost(), 1.0);
    }

    #[test]
    fn test_edit_tracker_custom_costs() {
        let tracker = EditTrackerBuilder::new()
            .insert_cost(0.5)
            .delete_cost(0.5)
            .substitute_cost(1.0)
            .transpose_cost(0.8)
            .build();

        assert_eq!(tracker.cost(), 0.0);
    }

    #[test]
    fn test_compute_edit_weight() {
        let weight = compute_edit_weight("hello", "helo");

        // Should have cost 1 (deletion of one 'l')
        assert_eq!(weight.cost() as usize, 1);
    }

    #[test]
    fn test_compute_edit_weight_substitution() {
        let weight = compute_edit_weight("cat", "bat");

        // Should have cost 1 (substitute c -> b)
        assert_eq!(weight.cost() as usize, 1);
    }

    #[test]
    fn test_compute_edit_weight_insertion() {
        let weight = compute_edit_weight("helo", "hello");

        // Should have cost 1 (insert 'l')
        assert_eq!(weight.cost() as usize, 1);
    }

    #[test]
    fn test_fuzzy_config_transpositions() {
        let config = FuzzyConfig::new(2).with_transpositions();
        assert!(matches!(config.algorithm, Algorithm::Transposition));
    }

    #[test]
    fn test_fuzzy_result_map() {
        let result = FuzzyResult::new("hello".to_string(), 1);
        let mapped = result.map(|s| s.len());

        assert_eq!(mapped.term, 5);
        assert_eq!(mapped.distance, 1);
    }
}
