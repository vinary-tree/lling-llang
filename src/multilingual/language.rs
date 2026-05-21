//! Language configuration and models for multilingual processing.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Debug, Display};
use std::hash::Hash;

/// Unique identifier for a language.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageId(pub String);

impl LanguageId {
    /// Create a new language ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the language code.
    pub fn code(&self) -> &str {
        &self.0
    }
}

impl Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<S: Into<String>> From<S> for LanguageId {
    fn from(s: S) -> Self {
        Self::new(s)
    }
}

/// Common language IDs.
impl LanguageId {
    /// English.
    pub fn english() -> Self {
        Self::new("en")
    }

    /// Spanish.
    pub fn spanish() -> Self {
        Self::new("es")
    }

    /// French.
    pub fn french() -> Self {
        Self::new("fr")
    }

    /// German.
    pub fn german() -> Self {
        Self::new("de")
    }

    /// Mandarin Chinese.
    pub fn mandarin() -> Self {
        Self::new("zh")
    }

    /// Hindi.
    pub fn hindi() -> Self {
        Self::new("hi")
    }

    /// Arabic.
    pub fn arabic() -> Self {
        Self::new("ar")
    }

    /// Japanese.
    pub fn japanese() -> Self {
        Self::new("ja")
    }
}

/// Configuration for a language in code-switching context.
#[derive(Debug, Clone)]
pub struct LanguageConfig {
    /// Language identifier.
    pub id: LanguageId,
    /// Prior probability of this language (0.0-1.0).
    pub prior: f64,
    /// Vocabulary (words known to be in this language).
    pub vocabulary: HashSet<String>,
    /// Word log-probabilities (for language modeling).
    pub word_probs: HashMap<String, f64>,
    /// Default log-probability for unknown words.
    pub unknown_word_prob: f64,
    /// Whether this language uses right-to-left script.
    pub rtl: bool,
    /// Script type (for detection heuristics).
    pub script: Script,
}

/// Script types for language detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    /// Latin alphabet.
    Latin,
    /// Cyrillic alphabet.
    Cyrillic,
    /// Arabic script.
    Arabic,
    /// Devanagari script.
    Devanagari,
    /// Chinese characters.
    Han,
    /// Japanese (mixed Kanji, Hiragana, Katakana).
    Japanese,
    /// Korean Hangul.
    Hangul,
    /// Greek alphabet.
    Greek,
    /// Hebrew script.
    Hebrew,
    /// Thai script.
    Thai,
    /// Unknown/mixed script.
    Unknown,
}

impl Default for Script {
    fn default() -> Self {
        Script::Latin
    }
}

impl LanguageConfig {
    /// Create a new language configuration.
    pub fn new(id: impl Into<LanguageId>) -> Self {
        Self {
            id: id.into(),
            prior: 1.0,
            vocabulary: HashSet::new(),
            word_probs: HashMap::new(),
            unknown_word_prob: -10.0, // Very low probability for unknown words
            rtl: false,
            script: Script::Latin,
        }
    }

    /// Set the prior probability.
    pub fn with_prior(mut self, prior: f64) -> Self {
        self.prior = prior;
        self
    }

    /// Set the vocabulary.
    pub fn with_vocabulary(mut self, words: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.vocabulary = words.into_iter().map(|w| w.into()).collect();
        self
    }

    /// Add words to vocabulary.
    pub fn add_words(mut self, words: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for word in words {
            self.vocabulary.insert(word.into());
        }
        self
    }

    /// Set word probabilities.
    pub fn with_word_probs(mut self, probs: HashMap<String, f64>) -> Self {
        self.word_probs = probs;
        self
    }

    /// Add a word probability.
    pub fn add_word_prob(mut self, word: impl Into<String>, log_prob: f64) -> Self {
        self.word_probs.insert(word.into(), log_prob);
        self
    }

    /// Set as right-to-left language.
    pub fn rtl(mut self) -> Self {
        self.rtl = true;
        self
    }

    /// Set the script type.
    pub fn with_script(mut self, script: Script) -> Self {
        self.script = script;
        self
    }

    /// Set the unknown word probability.
    pub fn with_unknown_prob(mut self, log_prob: f64) -> Self {
        self.unknown_word_prob = log_prob;
        self
    }

    /// Check if a word is in this language's vocabulary.
    pub fn contains_word(&self, word: &str) -> bool {
        self.vocabulary.contains(word) || self.vocabulary.contains(&word.to_lowercase())
    }

    /// Get the log-probability of a word.
    ///
    /// Returns in priority order:
    /// 1. Explicit probability from word_probs if set
    /// 2. A default "known word" probability if the word is in vocabulary
    /// 3. The unknown_word_prob otherwise
    pub fn word_log_prob(&self, word: &str) -> f64 {
        // First check explicit probabilities
        if let Some(&prob) = self.word_probs.get(word) {
            return prob;
        }
        if let Some(&prob) = self.word_probs.get(&word.to_lowercase()) {
            return prob;
        }

        // If in vocabulary but no explicit prob, use a default known-word probability
        if self.contains_word(word) {
            // Default for known words: uniform distribution over vocabulary
            let vocab_size = self.vocabulary.len().max(1) as f64;
            return -(vocab_size.ln());
        }

        // Unknown word
        self.unknown_word_prob
    }

    /// Get the language ID.
    pub fn id(&self) -> &LanguageId {
        &self.id
    }
}

/// Word probability entry.
#[derive(Debug, Clone)]
pub struct WordProbability {
    /// The word.
    pub word: String,
    /// Log probability.
    pub log_prob: f64,
}

impl WordProbability {
    /// Create a new word probability.
    pub fn new(word: impl Into<String>, log_prob: f64) -> Self {
        Self {
            word: word.into(),
            log_prob,
        }
    }
}

/// Trait for language models.
pub trait LanguageModel: Send + Sync + Debug {
    /// Get the language ID.
    fn language(&self) -> &LanguageId;

    /// Score a word in isolation.
    fn word_log_prob(&self, word: &str) -> f64;

    /// Score a word given context (previous words).
    fn context_log_prob(&self, word: &str, _context: &[&str]) -> f64 {
        // Default: ignore context
        self.word_log_prob(word)
    }

    /// Get the vocabulary size.
    fn vocabulary_size(&self) -> usize;

    /// Check if a word is in vocabulary.
    fn in_vocabulary(&self, word: &str) -> bool;
}

/// Simple unigram language model.
#[derive(Debug, Clone)]
pub struct SimpleLanguageModel {
    language: LanguageId,
    word_probs: HashMap<String, f64>,
    unknown_prob: f64,
}

impl SimpleLanguageModel {
    /// Create a new simple language model.
    pub fn new(language: impl Into<LanguageId>) -> Self {
        Self {
            language: language.into(),
            word_probs: HashMap::new(),
            unknown_prob: -10.0,
        }
    }

    /// Set word probabilities.
    pub fn with_probs(mut self, probs: HashMap<String, f64>) -> Self {
        self.word_probs = probs;
        self
    }

    /// Add a word probability.
    pub fn add_prob(&mut self, word: impl Into<String>, log_prob: f64) {
        self.word_probs.insert(word.into(), log_prob);
    }

    /// Set unknown word probability.
    pub fn with_unknown_prob(mut self, log_prob: f64) -> Self {
        self.unknown_prob = log_prob;
        self
    }

    /// Build from word counts.
    pub fn from_counts(language: impl Into<LanguageId>, counts: &HashMap<String, usize>) -> Self {
        let total: usize = counts.values().sum();
        let total_f64 = total as f64;

        let word_probs: HashMap<String, f64> = counts
            .iter()
            .map(|(word, count)| {
                let prob = (*count as f64) / total_f64;
                (word.clone(), prob.ln())
            })
            .collect();

        Self {
            language: language.into(),
            word_probs,
            unknown_prob: (1.0 / (total_f64 * 10.0)).ln(), // Very rare
        }
    }
}

impl LanguageModel for SimpleLanguageModel {
    fn language(&self) -> &LanguageId {
        &self.language
    }

    fn word_log_prob(&self, word: &str) -> f64 {
        self.word_probs
            .get(word)
            .or_else(|| self.word_probs.get(&word.to_lowercase()))
            .copied()
            .unwrap_or(self.unknown_prob)
    }

    fn vocabulary_size(&self) -> usize {
        self.word_probs.len()
    }

    fn in_vocabulary(&self, word: &str) -> bool {
        self.word_probs.contains_key(word) || self.word_probs.contains_key(&word.to_lowercase())
    }
}

/// Result of language detection.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Detected language.
    pub language: LanguageId,
    /// Confidence score (0.0-1.0).
    pub confidence: f64,
    /// Alternative languages with their scores.
    pub alternatives: Vec<(LanguageId, f64)>,
}

impl DetectionResult {
    /// Create a new detection result.
    pub fn new(language: LanguageId, confidence: f64) -> Self {
        Self {
            language,
            confidence,
            alternatives: Vec::new(),
        }
    }

    /// Add alternative languages.
    pub fn with_alternatives(mut self, alternatives: Vec<(LanguageId, f64)>) -> Self {
        self.alternatives = alternatives;
        self
    }
}

/// Language detector based on vocabulary overlap.
#[derive(Debug, Clone)]
pub struct LanguageDetector {
    configs: Vec<LanguageConfig>,
}

impl LanguageDetector {
    /// Create a new language detector.
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
        }
    }

    /// Add a language configuration.
    pub fn add_language(&mut self, config: LanguageConfig) {
        self.configs.push(config);
    }

    /// Build from configurations.
    pub fn from_configs(configs: Vec<LanguageConfig>) -> Self {
        Self { configs }
    }

    /// Detect the language of a word.
    pub fn detect_word(&self, word: &str) -> DetectionResult {
        let mut scores: Vec<(LanguageId, f64)> = Vec::new();

        for config in &self.configs {
            let mut score = config.prior.ln();

            if config.contains_word(word) {
                score += config.word_log_prob(word);
            } else {
                score += config.unknown_word_prob;
            }

            scores.push((config.id.clone(), score));
        }

        // Normalize to get probabilities
        if scores.is_empty() {
            return DetectionResult::new(LanguageId::new("unknown"), 0.0);
        }

        // Find max for numerical stability
        let max_score = scores
            .iter()
            .map(|(_, s)| *s)
            .fold(f64::NEG_INFINITY, f64::max);

        // Compute softmax
        let exp_scores: Vec<f64> = scores.iter().map(|(_, s)| (s - max_score).exp()).collect();
        let sum: f64 = exp_scores.iter().sum();

        let probs: Vec<(LanguageId, f64)> = scores
            .iter()
            .zip(exp_scores.iter())
            .map(|((id, _), exp)| (id.clone(), exp / sum))
            .collect();

        // Sort by probability
        let mut sorted = probs.clone();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let (best_lang, best_prob) = sorted.remove(0);

        DetectionResult::new(best_lang, best_prob).with_alternatives(sorted)
    }

    /// Detect the language of a sequence of words.
    pub fn detect_sequence(&self, words: &[&str]) -> DetectionResult {
        if words.is_empty() {
            return DetectionResult::new(LanguageId::new("unknown"), 0.0);
        }

        let mut total_scores: HashMap<LanguageId, f64> = HashMap::new();

        for word in words {
            let result = self.detect_word(word);
            *total_scores.entry(result.language).or_insert(0.0) += result.confidence;
            for (lang, score) in result.alternatives {
                *total_scores.entry(lang).or_insert(0.0) += score;
            }
        }

        // Normalize
        let total: f64 = total_scores.values().sum();
        let normalized: Vec<(LanguageId, f64)> = total_scores
            .into_iter()
            .map(|(id, score)| (id, score / total))
            .collect();

        // Sort by score
        let mut sorted = normalized;
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let (best_lang, best_prob) = sorted.remove(0);

        DetectionResult::new(best_lang, best_prob).with_alternatives(sorted)
    }

    /// Get all configured languages.
    pub fn languages(&self) -> impl Iterator<Item = &LanguageConfig> {
        self.configs.iter()
    }
}

impl Default for LanguageDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_id() {
        let id = LanguageId::new("en");
        assert_eq!(id.code(), "en");
        assert_eq!(format!("{}", id), "en");
    }

    #[test]
    fn test_language_id_presets() {
        assert_eq!(LanguageId::english().code(), "en");
        assert_eq!(LanguageId::spanish().code(), "es");
        assert_eq!(LanguageId::french().code(), "fr");
    }

    #[test]
    fn test_language_config() {
        let config = LanguageConfig::new("en")
            .with_prior(0.7)
            .add_words(vec!["hello", "world", "the"]);

        assert_eq!(config.id.code(), "en");
        assert!((config.prior - 0.7).abs() < f64::EPSILON);
        assert!(config.contains_word("hello"));
        assert!(config.contains_word("HELLO")); // Case insensitive
        assert!(!config.contains_word("hola"));
    }

    #[test]
    fn test_language_config_word_probs() {
        let config = LanguageConfig::new("en")
            .add_word_prob("the", -1.0)
            .add_word_prob("a", -2.0);

        assert!((config.word_log_prob("the") - (-1.0)).abs() < f64::EPSILON);
        assert!((config.word_log_prob("a") - (-2.0)).abs() < f64::EPSILON);
        assert!((config.word_log_prob("xyz") - config.unknown_word_prob).abs() < f64::EPSILON);
    }

    #[test]
    fn test_simple_language_model() {
        let mut lm = SimpleLanguageModel::new("en").with_unknown_prob(-15.0);
        lm.add_prob("hello", -2.0);
        lm.add_prob("world", -3.0);

        assert_eq!(lm.language().code(), "en");
        assert!((lm.word_log_prob("hello") - (-2.0)).abs() < f64::EPSILON);
        assert!((lm.word_log_prob("unknown") - (-15.0)).abs() < f64::EPSILON);
        assert_eq!(lm.vocabulary_size(), 2);
        assert!(lm.in_vocabulary("hello"));
        assert!(!lm.in_vocabulary("xyz"));
    }

    #[test]
    fn test_language_model_from_counts() {
        let mut counts = HashMap::new();
        counts.insert("the".to_string(), 100);
        counts.insert("a".to_string(), 50);
        counts.insert("an".to_string(), 25);

        let lm = SimpleLanguageModel::from_counts("en", &counts);

        assert_eq!(lm.vocabulary_size(), 3);
        // "the" should have higher probability than "a"
        assert!(lm.word_log_prob("the") > lm.word_log_prob("a"));
        // "a" should have higher probability than "an"
        assert!(lm.word_log_prob("a") > lm.word_log_prob("an"));
    }

    #[test]
    fn test_detection_result() {
        let result = DetectionResult::new(LanguageId::english(), 0.8).with_alternatives(vec![
            (LanguageId::spanish(), 0.15),
            (LanguageId::french(), 0.05),
        ]);

        assert_eq!(result.language.code(), "en");
        assert!((result.confidence - 0.8).abs() < f64::EPSILON);
        assert_eq!(result.alternatives.len(), 2);
    }

    #[test]
    fn test_language_detector_single_word() {
        let mut detector = LanguageDetector::new();

        detector.add_language(
            LanguageConfig::new("en")
                .with_prior(0.5)
                .add_words(vec!["hello", "world", "the"]),
        );

        detector.add_language(
            LanguageConfig::new("es")
                .with_prior(0.5)
                .add_words(vec!["hola", "mundo", "el"]),
        );

        let result = detector.detect_word("hello");
        assert_eq!(result.language.code(), "en");
        assert!(result.confidence > 0.5);

        let result2 = detector.detect_word("hola");
        assert_eq!(result2.language.code(), "es");
        assert!(result2.confidence > 0.5);
    }

    #[test]
    fn test_language_detector_sequence() {
        let mut detector = LanguageDetector::new();

        detector.add_language(
            LanguageConfig::new("en")
                .with_prior(0.5)
                .add_words(vec!["hello", "world", "the", "is", "good"]),
        );

        detector.add_language(
            LanguageConfig::new("es")
                .with_prior(0.5)
                .add_words(vec!["hola", "mundo", "el", "es", "bueno"]),
        );

        // English-dominant sequence
        let result = detector.detect_sequence(&["hello", "world", "is", "good"]);
        assert_eq!(result.language.code(), "en");

        // Spanish-dominant sequence
        let result2 = detector.detect_sequence(&["hola", "mundo", "es", "bueno"]);
        assert_eq!(result2.language.code(), "es");
    }

    #[test]
    fn test_script_default() {
        assert_eq!(Script::default(), Script::Latin);
    }

    #[test]
    fn test_language_config_rtl() {
        let config = LanguageConfig::new("ar").rtl().with_script(Script::Arabic);

        assert!(config.rtl);
        assert_eq!(config.script, Script::Arabic);
    }
}
