//! Edit distance correction layer for spelling correction.
//!
//! This layer adds alternative correction edges to the lattice based on
//! edit distance (Levenshtein or optimal string alignment) from a reference
//! dictionary.
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::layers::{EditDistanceLayer, EditDistanceLayerConfig};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Create a dictionary-based correction layer
//! let dictionary = vec!["hello", "world", "help", "held"];
//! let layer = EditDistanceLayer::<TropicalWeight>::new(dictionary)
//!     .with_max_distance(2)
//!     .with_cost_per_edit(1.0);
//!
//! // Apply to a lattice
//! let corrected = pipeline.apply(&input_lattice)?;
//! ```

use std::collections::{BinaryHeap, HashSet};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(feature = "levenshtein")]
use liblevenshtein::bindings::{
    BindingError, MatchBatch, MatchTerm, QueryOrder, ResourceTransducer, DEFAULT_MATCH_BATCH,
};
#[cfg(feature = "levenshtein")]
use liblevenshtein::transducer::Algorithm;
#[cfg(feature = "levenshtein")]
use vinary_tree_interop::{VtResource, VtUnitDomain};

use super::super::traits::{CorrectionLayer, LayerError, LayerResult};
use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder};
use crate::semiring::{Semiring, TropicalWeight};

/// Configuration for the edit distance correction layer.
#[derive(Clone, Debug)]
pub struct EditDistanceLayerConfig {
    /// Maximum edit distance to consider (default: 2)
    pub max_distance: usize,
    /// Cost per edit operation (default: 1.0)
    pub cost_per_edit: f64,
    /// Cost multiplier for substitutions vs insert/delete (default: 1.0)
    pub substitution_multiplier: f64,
    /// Cost multiplier for adjacent transpositions (default: 1.0)
    pub transposition_multiplier: f64,
    /// Enable optimal string alignment with adjacent transpositions (default: true)
    pub enable_transpositions: bool,
    /// Maximum number of corrections to generate per input word (default: 10)
    pub max_corrections_per_word: usize,
    /// Minimum word length to attempt correction (default: 2)
    pub min_word_length: usize,
    /// Case-insensitive matching (default: true)
    pub case_insensitive: bool,
    /// Keep original edges even when corrections are found (default: true)
    pub keep_original: bool,
    /// Weight boost for exact dictionary matches (default: 0.0 = no boost)
    pub exact_match_boost: f64,
}

impl Default for EditDistanceLayerConfig {
    fn default() -> Self {
        EditDistanceLayerConfig {
            max_distance: 2,
            cost_per_edit: 1.0,
            substitution_multiplier: 1.0,
            transposition_multiplier: 1.0,
            enable_transpositions: true,
            max_corrections_per_word: 10,
            min_word_length: 2,
            case_insensitive: true,
            keep_original: true,
            exact_match_boost: 0.0,
        }
    }
}

/// Unit-cost edit metric used to select dictionary candidates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditDistanceMetric {
    /// Insertion, deletion, and substitution.
    Levenshtein,
    /// Levenshtein operations plus one edit for an adjacent transposition.
    ///
    /// This is the restricted Damerau recurrence commonly called optimal
    /// string alignment (OSA): one substring cannot be edited more than once.
    #[default]
    OptimalStringAlignment,
}

/// Bounds and metric for one dictionary search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DictionarySearchOptions {
    /// Largest accepted unit edit distance.
    pub max_distance: usize,
    /// Largest number of matches to materialize.
    ///
    /// Zero guarantees an empty result without invoking the provider.
    pub max_results: usize,
    /// Unit-cost metric used for candidate selection and ordering.
    pub metric: EditDistanceMetric,
    /// Apply Unicode lowercase normalization to the query and candidate keys.
    pub case_insensitive: bool,
}

impl DictionarySearchOptions {
    /// Construct a bounded search.
    pub const fn new(max_distance: usize, max_results: usize, metric: EditDistanceMetric) -> Self {
        Self {
            max_distance,
            max_results,
            metric,
            case_insensitive: false,
        }
    }

    /// Select Unicode lowercase normalization for this search.
    pub const fn with_case_insensitive(mut self, enabled: bool) -> Self {
        self.case_insensitive = enabled;
        self
    }
}

/// Failure returned by a dictionary provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DictionaryError {
    /// A retained Vinary Tree resource rejected or failed an operation.
    #[cfg(feature = "levenshtein")]
    Resource(BindingError),
    /// A custom provider returned an application-specific failure.
    Provider(String),
    /// The requested normalization is not supported by this provider.
    UnsupportedNormalization(&'static str),
}

impl fmt::Display for DictionaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "levenshtein")]
            Self::Resource(error) => write!(formatter, "dictionary resource error: {error}"),
            Self::Provider(message) => write!(formatter, "dictionary provider error: {message}"),
            Self::UnsupportedNormalization(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DictionaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "levenshtein")]
            Self::Resource(error) => Some(error),
            Self::Provider(_) => None,
            Self::UnsupportedNormalization(_) => None,
        }
    }
}

#[cfg(feature = "levenshtein")]
impl From<BindingError> for DictionaryError {
    fn from(error: BindingError) -> Self {
        Self::Resource(error)
    }
}

/// Result of a dictionary operation.
pub type DictionaryResult<T> = Result<T, DictionaryError>;

/// Snapshot-consistent dictionary for bounded edit-distance lookups.
///
/// Implementations must return matches ordered by increasing distance and then
/// lexicographically by term. They may stop work as soon as the requested
/// number of matches is known. Every fallible provider boundary remains visible
/// to the caller through [DictionaryResult].
pub trait Dictionary: Send + Sync {
    /// Check if a word exists in the dictionary.
    fn contains(&self, word: &str) -> DictionaryResult<bool>;

    /// Return a bounded, deterministically ordered set of nearby words.
    fn find_within_distance(
        &self,
        query: &str,
        options: DictionarySearchOptions,
    ) -> DictionaryResult<Vec<(String, usize)>>;

    /// Return the size when the provider can supply it without a full walk.
    fn len(&self) -> DictionaryResult<Option<usize>>;

    /// Return whether the dictionary is empty when its size is known.
    fn is_empty(&self) -> DictionaryResult<Option<bool>> {
        self.len().map(|len| len.map(|len| len == 0))
    }
}

/// Simple in-memory dictionary implementation.
#[derive(Clone, Debug)]
pub struct InMemoryDictionary {
    words: HashSet<String>,
    words_lower: HashSet<String>,
    search_words: Vec<(String, String)>,
    case_insensitive: bool,
}

impl InMemoryDictionary {
    /// Create a new dictionary from a list of words.
    pub fn new<S: AsRef<str>>(words: &[S], case_insensitive: bool) -> Self {
        let words_set: HashSet<String> = words.iter().map(|w| w.as_ref().to_string()).collect();
        let words_lower: HashSet<String> =
            words_set.iter().map(|word| word.to_lowercase()).collect();
        let mut search_words: Vec<(String, String)> = words_set
            .iter()
            .map(|word| {
                let normalized = word.to_lowercase();
                (word.clone(), normalized)
            })
            .collect();
        search_words.sort_by(|left, right| left.0.cmp(&right.0));

        InMemoryDictionary {
            words: words_set,
            words_lower,
            search_words,
            case_insensitive,
        }
    }

    /// Add a word to the dictionary.
    pub fn add(&mut self, word: &str) {
        if self.words.insert(word.to_string()) {
            let normalized = word.to_lowercase();
            self.words_lower.insert(normalized.clone());
            let insertion = self
                .search_words
                .binary_search_by(|entry| entry.0.as_str().cmp(word))
                .unwrap_or_else(|index| index);
            self.search_words
                .insert(insertion, (word.to_string(), normalized));
        }
    }
}

impl Dictionary for InMemoryDictionary {
    fn contains(&self, word: &str) -> DictionaryResult<bool> {
        Ok(if self.case_insensitive {
            self.words_lower.contains(&word.to_lowercase())
        } else {
            self.words.contains(word)
        })
    }

    fn find_within_distance(
        &self,
        query: &str,
        options: DictionarySearchOptions,
    ) -> DictionaryResult<Vec<(String, usize)>> {
        if options.max_results == 0 {
            return Ok(Vec::new());
        }
        let query_normalized = if options.case_insensitive {
            query.to_lowercase()
        } else {
            query.to_string()
        };

        // The max-heap keeps only the worst retained match at its root. This
        // makes bounded searches O(n log k) in accepted candidates and O(k)
        // in memory instead of sorting and retaining every hit.
        let mut results =
            BinaryHeap::with_capacity(options.max_results.min(self.search_words.len()));
        for (word, word_normalized) in &self.search_words {
            let candidate = if options.case_insensitive {
                word_normalized
            } else {
                word
            };
            let distance = match options.metric {
                EditDistanceMetric::Levenshtein => {
                    levenshtein_distance(&query_normalized, candidate)
                }
                EditDistanceMetric::OptimalStringAlignment => {
                    optimal_string_alignment_distance(&query_normalized, candidate)
                }
            };
            if distance <= options.max_distance {
                results.push((distance, word.clone()));
                if results.len() > options.max_results {
                    results.pop();
                }
            }
        }

        let mut results: Vec<_> = results
            .into_iter()
            .map(|(distance, word)| (word, distance))
            .collect();
        results.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
        Ok(results)
    }

    fn len(&self) -> DictionaryResult<Option<usize>> {
        Ok(Some(self.words.len()))
    }
}

/// Retained Unicode dictionary resource consumed without materializing its keys.
///
/// Construction captures one immutable provider revision. All later queries
/// share that revision and stream bounded batches through liblevenshtein's
/// optimized resource traversal. Create another adapter to observe a newer
/// revision of a mutable producer.
#[cfg(feature = "levenshtein")]
#[derive(Clone, Debug)]
pub struct ResourceDictionary {
    transducer: ResourceTransducer,
    known_len: Option<usize>,
    normalization: ResourceDictionaryNormalization,
}

/// Normalization contract declared by a retained dictionary resource.
#[cfg(feature = "levenshtein")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResourceDictionaryNormalization {
    /// Keys are matched exactly as stored.
    #[default]
    Exact,
    /// Every stored key is already normalized with Rust Unicode lowercase.
    ///
    /// The adapter lowercases queries before traversal. The provider remains
    /// responsible for ensuring that all stored keys satisfy this declaration.
    UnicodeLowercaseKeys,
}

#[cfg(feature = "levenshtein")]
impl ResourceDictionary {
    /// Retain and capture a Unicode-scalar dictionary resource.
    ///
    /// # Safety
    ///
    /// The resource must obey the Vinary Tree interop retain/release contract.
    /// Its vtables and callbacks must remain valid until the final release.
    pub unsafe fn from_resource(resource: VtResource) -> DictionaryResult<Self> {
        // SAFETY: forwarded unchanged to the fully explicit constructor.
        unsafe {
            Self::from_resource_with_normalization(resource, ResourceDictionaryNormalization::Exact)
        }
    }

    /// Retain and capture a resource with an explicit key-normalization contract.
    ///
    /// # Safety
    ///
    /// The resource must obey the Vinary Tree interop retain/release contract.
    /// Its vtables and callbacks must remain valid until the final release.
    pub unsafe fn from_resource_with_normalization(
        resource: VtResource,
        normalization: ResourceDictionaryNormalization,
    ) -> DictionaryResult<Self> {
        // SAFETY: the caller supplies exactly the resource lifetime contract
        // required by ResourceTransducer.
        let transducer =
            unsafe { ResourceTransducer::from_resource(resource, Algorithm::Standard) }?;
        Self::from_transducer_with_normalization(transducer, normalization)
    }

    /// Capture a transducer's current dictionary revision.
    pub fn from_transducer(transducer: ResourceTransducer) -> DictionaryResult<Self> {
        Self::from_transducer_with_normalization(transducer, ResourceDictionaryNormalization::Exact)
    }

    /// Capture a transducer using an explicit key-normalization contract.
    pub fn from_transducer_with_normalization(
        transducer: ResourceTransducer,
        normalization: ResourceDictionaryNormalization,
    ) -> DictionaryResult<Self> {
        let actual = transducer.unit_domain();
        if actual != VtUnitDomain::UnicodeScalar {
            return Err(BindingError::UnitDomainMismatch {
                expected: VtUnitDomain::UnicodeScalar,
                actual,
            }
            .into());
        }
        let transducer = transducer.snapshot()?;
        let known_len = transducer.len()?;
        Ok(Self {
            transducer,
            known_len,
            normalization,
        })
    }

    /// Borrow the pinned transducer used by this adapter.
    pub fn transducer(&self) -> &ResourceTransducer {
        &self.transducer
    }

    /// Return the key-normalization contract declared at construction.
    pub fn normalization(&self) -> ResourceDictionaryNormalization {
        self.normalization
    }
}

#[cfg(feature = "levenshtein")]
impl Dictionary for ResourceDictionary {
    fn contains(&self, word: &str) -> DictionaryResult<bool> {
        Ok(!self
            .find_within_distance(
                word,
                DictionarySearchOptions::new(0, 1, EditDistanceMetric::Levenshtein)
                    .with_case_insensitive(matches!(
                        self.normalization,
                        ResourceDictionaryNormalization::UnicodeLowercaseKeys
                    )),
            )?
            .is_empty())
    }

    fn find_within_distance(
        &self,
        query: &str,
        options: DictionarySearchOptions,
    ) -> DictionaryResult<Vec<(String, usize)>> {
        if options.max_results == 0 {
            return Ok(Vec::new());
        }
        let normalized_query;
        let query = if options.case_insensitive {
            match self.normalization {
                ResourceDictionaryNormalization::Exact => {
                    return Err(DictionaryError::UnsupportedNormalization(
                        "case-insensitive search requires a resource declared with UnicodeLowercaseKeys",
                    ))
                }
                ResourceDictionaryNormalization::UnicodeLowercaseKeys => {
                    normalized_query = query.to_lowercase();
                    &normalized_query
                }
            }
        } else {
            query
        };
        let algorithm = match options.metric {
            EditDistanceMetric::Levenshtein => Algorithm::Standard,
            EditDistanceMetric::OptimalStringAlignment => Algorithm::Transposition,
        };
        let selected = self.transducer.with_algorithm(algorithm);
        let mut cursor =
            selected.query_utf8(query, options.max_distance, QueryOrder::DistanceThenTerm)?;
        let mut batch = MatchBatch::default();
        let initial_capacity = self
            .known_len
            .unwrap_or(DEFAULT_MATCH_BATCH)
            .min(options.max_results);
        let mut matches = Vec::with_capacity(initial_capacity);
        while matches.len() < options.max_results {
            let remaining = options.max_results - matches.len();
            let written = cursor.next_batch(&mut batch, remaining.min(DEFAULT_MATCH_BATCH))?;
            if written == 0 {
                break;
            }
            for candidate in batch.as_slice() {
                let MatchTerm::Utf8(term) = &candidate.term else {
                    return Err(BindingError::InvalidProviderOutput(
                        "Unicode dictionary query returned a non-UTF-8 term",
                    )
                    .into());
                };
                matches.push((term.clone(), candidate.distance));
            }
        }
        Ok(matches)
    }

    fn len(&self) -> DictionaryResult<Option<usize>> {
        Ok(self.known_len)
    }
}

/// Compute Levenshtein edit distance between two strings.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Use two rows for space efficiency
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;

        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };

            curr[j] = (prev[j] + 1) // deletion
                .min(curr[j - 1] + 1) // insertion
                .min(prev[j - 1] + cost); // substitution
        }

        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Compute optimal string alignment distance with adjacent transpositions.
///
/// Unlike unrestricted Damerau-Levenshtein distance, this recurrence cannot
/// edit one substring more than once.
pub fn optimal_string_alignment_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Full matrix needed for transposition lookback
    let mut dp = vec![vec![0usize; n + 1]; m + 1];

    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };

            dp[i][j] = (dp[i - 1][j] + 1) // deletion
                .min(dp[i][j - 1] + 1) // insertion
                .min(dp[i - 1][j - 1] + cost); // substitution

            // Transposition
            if i > 1
                && j > 1
                && a_chars[i - 1] == b_chars[j - 2]
                && a_chars[i - 2] == b_chars[j - 1]
            {
                dp[i][j] = dp[i][j].min(dp[i - 2][j - 2] + cost);
            }
        }
    }

    dp[m][n]
}

/// Compatibility name for [`optimal_string_alignment_distance`].
///
/// Earlier releases called the restricted recurrence Damerau-Levenshtein.
/// The implementation and current documentation use its precise name.
pub fn damerau_levenshtein_distance(a: &str, b: &str) -> usize {
    optimal_string_alignment_distance(a, b)
}

fn weighted_edit_cost(a: &str, b: &str, config: &EditDistanceLayerConfig) -> f64 {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let base = config.cost_per_edit;
    let substitution = base * config.substitution_multiplier;
    let transposition = base * config.transposition_multiplier;

    if a_chars.is_empty() {
        return b_chars.len() as f64 * base;
    }
    if b_chars.is_empty() {
        return a_chars.len() as f64 * base;
    }

    let mut previous_previous: Vec<f64> = (0..=b_chars.len())
        .map(|index| index as f64 * base)
        .collect();
    let mut previous = previous_previous.clone();
    let mut current = vec![0.0; b_chars.len() + 1];

    for i in 1..=a_chars.len() {
        current[0] = i as f64 * base;
        for j in 1..=b_chars.len() {
            let replace = if a_chars[i - 1] == b_chars[j - 1] {
                0.0
            } else {
                substitution
            };
            current[j] = (previous[j] + base)
                .min(current[j - 1] + base)
                .min(previous[j - 1] + replace);
            if config.enable_transpositions
                && i > 1
                && j > 1
                && a_chars[i - 1] == b_chars[j - 2]
                && a_chars[i - 2] == b_chars[j - 1]
            {
                current[j] = current[j].min(previous_previous[j - 2] + transposition);
            }
        }
        std::mem::swap(&mut previous_previous, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b_chars.len()]
}

impl From<DictionaryError> for LayerError {
    fn from(error: DictionaryError) -> Self {
        Self::ResourceError(error.to_string())
    }
}

/// Edit distance correction layer.
///
/// Adds correction edges to the lattice for words within a specified
/// edit distance of dictionary entries.
pub struct EditDistanceLayer<W: Semiring> {
    dictionary: Arc<dyn Dictionary>,
    config: EditDistanceLayerConfig,
    _phantom: PhantomData<W>,
}

impl<W: Semiring> EditDistanceLayer<W> {
    /// Create a new edit distance layer with an in-memory dictionary.
    pub fn new<S: AsRef<str>>(words: &[S]) -> Self {
        let config = EditDistanceLayerConfig::default();
        let dictionary = InMemoryDictionary::new(words, config.case_insensitive);
        EditDistanceLayer {
            dictionary: Arc::new(dictionary),
            config,
            _phantom: PhantomData,
        }
    }

    /// Create with a custom dictionary implementation.
    pub fn with_dictionary(dictionary: Arc<dyn Dictionary>) -> Self {
        EditDistanceLayer {
            dictionary,
            config: EditDistanceLayerConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create with custom configuration.
    pub fn with_config<S: AsRef<str>>(words: &[S], config: EditDistanceLayerConfig) -> Self {
        let dictionary = InMemoryDictionary::new(words, config.case_insensitive);
        EditDistanceLayer {
            dictionary: Arc::new(dictionary),
            config,
            _phantom: PhantomData,
        }
    }

    /// Set maximum edit distance.
    pub fn with_max_distance(mut self, distance: usize) -> Self {
        self.config.max_distance = distance;
        self
    }

    /// Set cost per edit operation.
    pub fn with_cost_per_edit(mut self, cost: f64) -> Self {
        self.config.cost_per_edit = cost;
        self
    }

    /// Enable or disable transpositions.
    pub fn with_transpositions(mut self, enabled: bool) -> Self {
        self.config.enable_transpositions = enabled;
        self
    }

    /// Set maximum corrections per word.
    pub fn with_max_corrections(mut self, max: usize) -> Self {
        self.config.max_corrections_per_word = max;
        self
    }

    /// Get the configuration.
    pub fn config(&self) -> &EditDistanceLayerConfig {
        &self.config
    }

    /// Get the dictionary.
    pub fn dictionary(&self) -> &dyn Dictionary {
        self.dictionary.as_ref()
    }

    /// Get the layer name (inherent method, doesn't require backend type).
    pub fn layer_name(&self) -> &str {
        "edit-distance"
    }

    /// Get estimated reduction factor (inherent method, doesn't require backend type).
    ///
    /// This layer typically increases paths, so returns > 1.0.
    pub fn estimated_reduction_factor(&self) -> f64 {
        1.0 + (self.config.max_corrections_per_word as f64 * 0.3)
    }

    /// Find corrections for a word.
    pub fn find_corrections(&self, word: &str) -> LayerResult<Vec<(String, f64)>> {
        self.validate_config()?;
        if word.chars().count() < self.config.min_word_length {
            return Ok(Vec::new());
        }

        let metric = if self.config.enable_transpositions {
            EditDistanceMetric::OptimalStringAlignment
        } else {
            EditDistanceMetric::Levenshtein
        };
        let candidates = self.dictionary.find_within_distance(
            word,
            DictionarySearchOptions::new(
                self.config.max_distance,
                self.config.max_corrections_per_word,
                metric,
            )
            .with_case_insensitive(self.config.case_insensitive),
        )?;

        let mut corrections: Vec<(String, f64)> = candidates
            .into_iter()
            .map(|(correction, _distance)| {
                let cost = self.compute_cost(word, &correction);
                (correction, cost)
            })
            .collect();

        // Apply exact match boost if applicable
        if self.config.exact_match_boost != 0.0 {
            let comparison_word = if self.config.case_insensitive {
                word.to_lowercase()
            } else {
                word.to_string()
            };
            for (correction, cost) in &mut corrections {
                let is_exact = if self.config.case_insensitive {
                    correction.to_lowercase() == comparison_word
                } else {
                    correction == word
                };
                if is_exact {
                    *cost -= self.config.exact_match_boost;
                }
            }
        }

        Ok(corrections)
    }

    fn validate_config(&self) -> LayerResult<()> {
        for (name, value) in [
            ("cost_per_edit", self.config.cost_per_edit),
            (
                "substitution_multiplier",
                self.config.substitution_multiplier,
            ),
            (
                "transposition_multiplier",
                self.config.transposition_multiplier,
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(LayerError::ConfigError(format!(
                    "{name} must be finite and non-negative"
                )));
            }
        }
        if !self.config.exact_match_boost.is_finite() {
            return Err(LayerError::ConfigError(
                "exact_match_boost must be finite".to_string(),
            ));
        }
        Ok(())
    }

    /// Compute the configured minimum edit cost for one candidate.
    fn compute_cost(&self, source: &str, correction: &str) -> f64 {
        if self.config.case_insensitive {
            weighted_edit_cost(
                &source.to_lowercase(),
                &correction.to_lowercase(),
                &self.config,
            )
        } else {
            weighted_edit_cost(source, correction, &self.config)
        }
    }
}

impl<W, B> CorrectionLayer<W, B> for EditDistanceLayer<W>
where
    W: Semiring + From<TropicalWeight>,
    B: LatticeBackend + Clone,
{
    fn name(&self) -> &str {
        "edit-distance"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        // Clone the backend for the new lattice
        let backend = lattice.backend().clone();
        let mut builder = LatticeBuilder::new(backend);

        // Track which edges we've added to avoid duplicates
        let mut added_edges: HashSet<(u32, u32, String)> = HashSet::new();

        // Process each edge in the lattice
        for edge in lattice.edges() {
            let word = match lattice.word(edge.label) {
                Some(w) => w.to_string(),
                None => continue, // Skip edges with unknown labels
            };

            let source = edge.source.value();
            let target = edge.target.value();

            // Always keep original edge if configured
            if self.config.keep_original {
                builder.add_correction(
                    source as usize,
                    target as usize,
                    &word,
                    edge.weight.clone(),
                    edge.metadata.clone(),
                );
                added_edges.insert((source, target, word.clone()));
            }

            // Find corrections
            let corrections = self.find_corrections(&word)?;

            for (correction, cost) in corrections {
                // Skip if this would duplicate the original
                if added_edges.contains(&(source, target, correction.clone())) {
                    continue;
                }

                // Compute new weight by adding edit cost
                let edit_weight = W::from(TropicalWeight::new(cost));
                let new_weight = edge.weight.clone().times(&edit_weight);

                // Create metadata indicating this is a correction
                let mut metadata = edge.metadata.clone();
                metadata.is_original = false;

                builder.add_correction(
                    source as usize,
                    target as usize,
                    &correction,
                    new_weight,
                    metadata,
                );
                added_edges.insert((source, target, correction));
            }
        }

        // Build the new lattice with the original node count
        let num_nodes = lattice.num_nodes();
        Ok(builder.build(num_nodes))
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        !matches!(self.dictionary.is_empty(), Ok(Some(true)))
    }

    fn check_applicability(&self, _lattice: &Lattice<W, B>) -> LayerResult<bool> {
        Ok(!self.dictionary.is_empty()?.unwrap_or(false))
    }

    fn estimated_reduction(&self) -> f64 {
        // This layer typically increases paths, so return > 1.0
        // Estimate based on average corrections per word
        1.0 + (self.config.max_corrections_per_word as f64 * 0.3)
    }
}

// Clone implementation requires W: Clone, which Semiring implies
impl<W: Semiring> Clone for EditDistanceLayer<W> {
    fn clone(&self) -> Self {
        EditDistanceLayer {
            dictionary: Arc::clone(&self.dictionary),
            config: self.config.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<W: Semiring> std::fmt::Debug for EditDistanceLayer<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditDistanceLayer")
            .field("config", &self.config)
            .field("dictionary_size", &self.dictionary.len())
            .finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::EdgeMetadata;
    use crate::layers::LayerPipeline;
    use crate::semiring::TropicalWeight;
    #[cfg(feature = "levenshtein")]
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", ""), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", "abc"), 0);
        assert_eq!(levenshtein_distance("abc", "ab"), 1);
        assert_eq!(levenshtein_distance("abc", "abcd"), 1);
        assert_eq!(levenshtein_distance("abc", "adc"), 1);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_damerau_levenshtein_distance() {
        assert_eq!(damerau_levenshtein_distance("abc", "abc"), 0);
        assert_eq!(damerau_levenshtein_distance("ab", "ba"), 1); // transposition
        assert_eq!(damerau_levenshtein_distance("abc", "acb"), 1); // transposition
        assert_eq!(damerau_levenshtein_distance("abc", "bac"), 1); // transposition at start
        assert_eq!(optimal_string_alignment_distance("CA", "ABC"), 3);
    }

    #[test]
    fn test_in_memory_dictionary() {
        let words = vec!["hello", "world", "help", "held", "helm"];
        let dict = InMemoryDictionary::new(&words, true);

        assert!(dict.contains("hello").unwrap());
        assert!(dict.contains("HELLO").unwrap()); // case insensitive
        assert!(!dict.contains("missing").unwrap());
        assert_eq!(dict.len().unwrap(), Some(5));
    }

    #[test]
    fn test_dictionary_find_within_distance() {
        let words = vec!["hello", "hallo", "help", "held", "world"];
        let dict = InMemoryDictionary::new(&words, false);

        let results = dict
            .find_within_distance(
                "hello",
                DictionarySearchOptions::new(1, usize::MAX, EditDistanceMetric::Levenshtein),
            )
            .unwrap();
        assert!(results.iter().any(|(w, _)| w == "hello")); // exact match
        assert!(results.iter().any(|(w, _)| w == "hallo")); // 1 edit

        let results = dict
            .find_within_distance(
                "hello",
                DictionarySearchOptions::new(2, usize::MAX, EditDistanceMetric::Levenshtein),
            )
            .unwrap();
        assert!(results.iter().any(|(w, _)| w == "help")); // 2 edits
        assert!(results.iter().any(|(w, _)| w == "held")); // 2 edits
    }

    #[test]
    fn bounded_dictionary_search_returns_best_deterministic_prefix() {
        let dict = InMemoryDictionary::new(&["ad", "ab", "ac", "aa"], false);
        let results = dict
            .find_within_distance(
                "aa",
                DictionarySearchOptions::new(1, 2, EditDistanceMetric::Levenshtein),
            )
            .unwrap();
        assert_eq!(results, vec![("aa".to_string(), 0), ("ab".to_string(), 1)]);
        assert!(dict
            .find_within_distance(
                "aa",
                DictionarySearchOptions::new(1, 0, EditDistanceMetric::Levenshtein),
            )
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_edit_distance_layer_creation() {
        let words = vec!["hello", "world"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words)
            .with_max_distance(2)
            .with_cost_per_edit(0.5);

        assert_eq!(layer.config().max_distance, 2);
        assert!((layer.config().cost_per_edit - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_find_corrections() {
        let words = vec!["hello", "hallo", "help", "world"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words).with_max_distance(2);

        let corrections = layer.find_corrections("helo").unwrap();

        // Should find hello and hallo (both 1 edit away)
        let words_found: Vec<&str> = corrections.iter().map(|(w, _)| w.as_str()).collect();
        assert!(words_found.contains(&"hello"));
        assert!(words_found.contains(&"hallo"));
    }

    #[test]
    fn test_find_corrections_respects_max() {
        let words: Vec<String> = (0..100).map(|i| format!("word{}", i)).collect();
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words).with_max_corrections(5);

        // Even with many potential matches, should limit to max
        let corrections = layer.find_corrections("word0").unwrap();
        assert!(corrections.len() <= 5);
    }

    #[test]
    fn transposition_selection_and_weight_multipliers_are_effective() {
        let enabled = EditDistanceLayer::<TropicalWeight>::with_config(
            &["ab"],
            EditDistanceLayerConfig {
                max_distance: 1,
                min_word_length: 0,
                case_insensitive: false,
                transposition_multiplier: 0.25,
                ..Default::default()
            },
        );
        assert_eq!(
            enabled.find_corrections("ba").unwrap(),
            vec![("ab".to_string(), 0.25)]
        );

        let disabled = EditDistanceLayer::<TropicalWeight>::with_config(
            &["ab"],
            EditDistanceLayerConfig {
                max_distance: 1,
                min_word_length: 0,
                case_insensitive: false,
                enable_transpositions: false,
                ..Default::default()
            },
        );
        assert!(disabled.find_corrections("ba").unwrap().is_empty());

        let substitution = EditDistanceLayer::<TropicalWeight>::with_config(
            &["ac"],
            EditDistanceLayerConfig {
                max_distance: 1,
                min_word_length: 0,
                case_insensitive: false,
                enable_transpositions: false,
                substitution_multiplier: 1.5,
                ..Default::default()
            },
        );
        assert_eq!(
            substitution.find_corrections("ab").unwrap(),
            vec![("ac".to_string(), 1.5)]
        );
    }

    #[test]
    fn invalid_numeric_configuration_is_rejected() {
        let layer = EditDistanceLayer::<TropicalWeight>::with_config(
            &["word"],
            EditDistanceLayerConfig {
                cost_per_edit: f64::NAN,
                ..Default::default()
            },
        );
        assert!(matches!(
            layer.find_corrections("ward"),
            Err(LayerError::ConfigError(message)) if message.contains("cost_per_edit")
        ));
    }

    #[test]
    fn minimum_word_length_counts_unicode_scalars() {
        let layer = EditDistanceLayer::<TropicalWeight>::with_config(
            &["é"],
            EditDistanceLayerConfig {
                min_word_length: 2,
                ..Default::default()
            },
        );
        assert!(layer.find_corrections("é").unwrap().is_empty());
    }

    #[derive(Debug)]
    struct FailingDictionary;

    impl Dictionary for FailingDictionary {
        fn contains(&self, _word: &str) -> DictionaryResult<bool> {
            Err(DictionaryError::Provider("contains failed".to_string()))
        }

        fn find_within_distance(
            &self,
            _query: &str,
            _options: DictionarySearchOptions,
        ) -> DictionaryResult<Vec<(String, usize)>> {
            Err(DictionaryError::Provider("search failed".to_string()))
        }

        fn len(&self) -> DictionaryResult<Option<usize>> {
            Err(DictionaryError::Provider("length failed".to_string()))
        }
    }

    #[test]
    fn pipeline_propagates_dictionary_applicability_failures() {
        let layer =
            EditDistanceLayer::<TropicalWeight>::with_dictionary(Arc::new(FailingDictionary));
        let lattice: Lattice<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(HashMapBackend::new()).build(0);
        let mut pipeline = LayerPipeline::new();
        pipeline.add_layer(layer);
        assert!(matches!(
            pipeline.apply(&lattice),
            Err(LayerError::ResourceError(message)) if message.contains("length failed")
        ));
    }

    #[cfg(feature = "levenshtein")]
    #[test]
    fn resource_dictionary_is_pinned_bounded_and_metric_aware() {
        let producer = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        for term in ["ab", "aa", "ac", "ad", "xy"] {
            producer.insert_text(term.as_bytes(), None).unwrap();
        }
        let owned_resource = producer.resource();
        let dictionary =
            unsafe { ResourceDictionary::from_resource(owned_resource.as_raw()) }.unwrap();

        assert_eq!(dictionary.len().unwrap(), Some(5));
        assert!(dictionary.contains("aa").unwrap());
        assert_eq!(
            dictionary
                .find_within_distance(
                    "aa",
                    DictionarySearchOptions::new(1, 2, EditDistanceMetric::Levenshtein),
                )
                .unwrap(),
            vec![("aa".to_string(), 0), ("ab".to_string(), 1)]
        );
        assert!(dictionary
            .find_within_distance(
                "yx",
                DictionarySearchOptions::new(1, 1, EditDistanceMetric::Levenshtein),
            )
            .unwrap()
            .is_empty());
        assert_eq!(
            dictionary
                .find_within_distance(
                    "yx",
                    DictionarySearchOptions::new(1, 1, EditDistanceMetric::OptimalStringAlignment,),
                )
                .unwrap(),
            vec![("xy".to_string(), 1)]
        );
        assert!(matches!(
            dictionary.find_within_distance(
                "AA",
                DictionarySearchOptions::new(0, 1, EditDistanceMetric::Levenshtein)
                    .with_case_insensitive(true),
            ),
            Err(DictionaryError::UnsupportedNormalization(_))
        ));

        let lowercase_dictionary = unsafe {
            ResourceDictionary::from_resource_with_normalization(
                owned_resource.as_raw(),
                ResourceDictionaryNormalization::UnicodeLowercaseKeys,
            )
        }
        .unwrap();
        assert_eq!(
            lowercase_dictionary.normalization(),
            ResourceDictionaryNormalization::UnicodeLowercaseKeys
        );
        assert!(lowercase_dictionary.contains("AA").unwrap());

        producer.insert_text(b"new", None).unwrap();
        assert!(!dictionary.contains("new").unwrap());
        drop(owned_resource);
        drop(producer);
        assert!(dictionary.contains("aa").unwrap());
        assert!(lowercase_dictionary.contains("AA").unwrap());
    }

    #[test]
    fn test_layer_apply() {
        let words = vec!["hello", "hallo", "help"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words)
            .with_max_distance(2)
            .with_cost_per_edit(1.0);

        // Build a simple lattice with "helo" (misspelled)
        let mut backend = HashMapBackend::new();
        let helo_id = backend.intern("helo");

        let mut builder: LatticeBuilder<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend);
        builder.add_correction_by_id(
            0,
            1,
            helo_id,
            TropicalWeight::one(),
            EdgeMetadata::default(),
        );
        let lattice = builder.build(1);

        // Apply the layer
        let result = layer.apply(&lattice).expect("should apply");

        // Should have more edges now (original + corrections)
        assert!(result.num_edges() >= 1);
    }

    #[test]
    fn test_layer_name() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]);
        assert_eq!(layer.layer_name(), "edit-distance");
    }

    #[test]
    fn test_layer_can_apply() {
        let layer_with_dict = EditDistanceLayer::<TropicalWeight>::new(&["test"]);
        let layer_empty = EditDistanceLayer::<TropicalWeight>::new::<&str>(&[]);

        let backend = HashMapBackend::new();
        let lattice: Lattice<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend).build(0);

        assert!(layer_with_dict.can_apply(&lattice));
        assert!(!layer_empty.can_apply(&lattice)); // Empty dictionary can't apply
    }

    #[test]
    fn test_config_default() {
        let config = EditDistanceLayerConfig::default();

        assert_eq!(config.max_distance, 2);
        assert!((config.cost_per_edit - 1.0).abs() < 0.001);
        assert!(config.enable_transpositions);
        assert!(config.case_insensitive);
        assert!(config.keep_original);
    }

    #[test]
    fn test_cost_computation() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]).with_cost_per_edit(0.5);

        assert!((layer.compute_cost("test", "test") - 0.0).abs() < 0.001);
        assert!((layer.compute_cost("test", "tent") - 0.5).abs() < 0.001);
        assert!((layer.compute_cost("test", "toast") - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_case_insensitive_corrections() {
        let words = vec!["Hello", "WORLD"];
        let layer = EditDistanceLayer::<TropicalWeight>::new(&words);

        // Should find corrections regardless of case
        let corrections = layer.find_corrections("hello").unwrap();
        assert!(!corrections.is_empty());

        let corrections = layer.find_corrections("HELLO").unwrap();
        assert!(!corrections.is_empty());
    }

    #[test]
    fn test_min_word_length() {
        let words = vec!["a", "ab", "abc", "abcd"];
        let config = EditDistanceLayerConfig {
            min_word_length: 3,
            ..Default::default()
        };
        let layer = EditDistanceLayer::<TropicalWeight>::with_config(&words, config);

        // Short words should get no corrections
        let corrections = layer.find_corrections("a").unwrap();
        assert!(corrections.is_empty());

        let corrections = layer.find_corrections("ab").unwrap();
        assert!(corrections.is_empty());

        // Longer words should work
        let corrections = layer.find_corrections("abc").unwrap();
        assert!(!corrections.is_empty());
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]).with_max_corrections(5);

        // Should return > 1.0 since this layer adds paths
        assert!(layer.estimated_reduction_factor() > 1.0);
    }

    #[test]
    fn test_layer_clone() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["test"]).with_max_distance(3);

        let cloned = layer.clone();
        assert_eq!(cloned.config().max_distance, 3);
    }

    #[test]
    fn test_layer_debug() {
        let layer = EditDistanceLayer::<TropicalWeight>::new(&["hello", "world"]);
        let debug_str = format!("{:?}", layer);

        assert!(debug_str.contains("EditDistanceLayer"));
        assert!(debug_str.contains("dictionary_size"));
    }
}
