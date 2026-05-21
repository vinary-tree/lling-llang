//! Code-switching transducer for multilingual speech processing.

use std::collections::HashMap;
use std::fmt::Debug;

use crate::semiring::Semiring;
#[cfg(test)]
use crate::wfst::Wfst;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition};

use super::language::{LanguageConfig, LanguageId, LanguageModel, SimpleLanguageModel};

/// Configuration for code-switching transducer.
#[derive(Debug, Clone)]
pub struct CodeSwitchConfig {
    /// Penalty for switching languages (in log space).
    pub switch_penalty: f64,
    /// Bonus for staying in same language (in log space, typically 0 or small positive).
    pub same_language_bonus: f64,
    /// Whether to allow starting in any language.
    pub allow_any_start: bool,
    /// Maximum number of switches allowed (-1 for unlimited).
    pub max_switches: i32,
}

impl Default for CodeSwitchConfig {
    fn default() -> Self {
        Self {
            switch_penalty: 2.0, // log-space penalty
            same_language_bonus: 0.0,
            allow_any_start: true,
            max_switches: -1, // unlimited
        }
    }
}

impl CodeSwitchConfig {
    /// Set the switch penalty.
    pub fn with_switch_penalty(mut self, penalty: f64) -> Self {
        self.switch_penalty = penalty;
        self
    }

    /// Set the same-language bonus.
    pub fn with_same_language_bonus(mut self, bonus: f64) -> Self {
        self.same_language_bonus = bonus;
        self
    }

    /// Set maximum switches.
    pub fn with_max_switches(mut self, max: i32) -> Self {
        self.max_switches = max;
        self
    }
}

/// A point where language switching occurs.
#[derive(Debug, Clone)]
pub struct SwitchPoint {
    /// Position in the sequence (word index).
    pub position: usize,
    /// Language before the switch.
    pub from_language: LanguageId,
    /// Language after the switch.
    pub to_language: LanguageId,
    /// Cost of this switch (in log space).
    pub cost: f64,
}

impl SwitchPoint {
    /// Create a new switch point.
    pub fn new(
        position: usize,
        from_language: LanguageId,
        to_language: LanguageId,
        cost: f64,
    ) -> Self {
        Self {
            position,
            from_language,
            to_language,
            cost,
        }
    }
}

/// A span of text in a single language.
#[derive(Debug, Clone)]
pub struct LanguageSpan {
    /// Language of this span.
    pub language: LanguageId,
    /// Start position (word index, inclusive).
    pub start: usize,
    /// End position (word index, exclusive).
    pub end: usize,
    /// Words in this span.
    pub words: Vec<String>,
}

impl LanguageSpan {
    /// Create a new language span.
    pub fn new(language: LanguageId, start: usize, end: usize, words: Vec<String>) -> Self {
        Self {
            language,
            start,
            end,
            words,
        }
    }

    /// Get the length of this span.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Check if this span is empty.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Result path from code-switching analysis.
#[derive(Debug, Clone)]
pub struct CodeSwitchPath {
    /// Total score of this path.
    pub score: f64,
    /// Language assigned to each word.
    pub word_languages: Vec<LanguageId>,
    /// Switch points.
    pub switch_points: Vec<SwitchPoint>,
    /// Language spans.
    pub spans: Vec<LanguageSpan>,
}

impl CodeSwitchPath {
    /// Create a new code-switch path.
    pub fn new(score: f64, word_languages: Vec<LanguageId>) -> Self {
        let switch_points = Self::find_switch_points(&word_languages);
        let spans = Self::compute_spans(&word_languages);

        Self {
            score,
            word_languages,
            switch_points,
            spans,
        }
    }

    /// Find switch points in a language sequence.
    fn find_switch_points(languages: &[LanguageId]) -> Vec<SwitchPoint> {
        let mut switches = Vec::new();

        for i in 1..languages.len() {
            if languages[i] != languages[i - 1] {
                switches.push(SwitchPoint::new(
                    i,
                    languages[i - 1].clone(),
                    languages[i].clone(),
                    0.0, // Cost filled in by transducer
                ));
            }
        }

        switches
    }

    /// Compute contiguous language spans.
    fn compute_spans(languages: &[LanguageId]) -> Vec<LanguageSpan> {
        if languages.is_empty() {
            return vec![];
        }

        let mut spans = Vec::new();
        let mut start = 0;
        let mut current_lang = &languages[0];

        for (i, lang) in languages.iter().enumerate().skip(1) {
            if lang != current_lang {
                spans.push(LanguageSpan::new(
                    current_lang.clone(),
                    start,
                    i,
                    Vec::new(), // Words filled in later
                ));
                start = i;
                current_lang = lang;
            }
        }

        // Final span
        spans.push(LanguageSpan::new(
            current_lang.clone(),
            start,
            languages.len(),
            Vec::new(),
        ));

        spans
    }

    /// Get the number of language switches.
    pub fn num_switches(&self) -> usize {
        self.switch_points.len()
    }

    /// Get the dominant language (most words).
    pub fn dominant_language(&self) -> Option<&LanguageId> {
        if self.spans.is_empty() {
            return None;
        }

        self.spans
            .iter()
            .max_by_key(|s| s.len())
            .map(|s| &s.language)
    }
}

/// Builder for code-switching transducer.
#[derive(Debug, Clone)]
pub struct CodeSwitchBuilder {
    languages: Vec<LanguageConfig>,
    language_models: HashMap<LanguageId, SimpleLanguageModel>,
    config: CodeSwitchConfig,
}

impl CodeSwitchBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            languages: Vec::new(),
            language_models: HashMap::new(),
            config: CodeSwitchConfig::default(),
        }
    }

    /// Add a language configuration.
    pub fn add_language(mut self, config: LanguageConfig) -> Self {
        // Create a simple language model from the config
        let mut lm =
            SimpleLanguageModel::new(config.id.clone()).with_unknown_prob(config.unknown_word_prob);

        for (word, prob) in &config.word_probs {
            lm.add_prob(word.clone(), *prob);
        }

        self.language_models.insert(config.id.clone(), lm);
        self.languages.push(config);
        self
    }

    /// Add a language model.
    pub fn add_language_model(mut self, lm: SimpleLanguageModel) -> Self {
        self.language_models.insert(lm.language().clone(), lm);
        self
    }

    /// Set switch penalty.
    pub fn switch_penalty(mut self, penalty: f64) -> Self {
        self.config.switch_penalty = penalty;
        self
    }

    /// Set configuration.
    pub fn config(mut self, config: CodeSwitchConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the transducer.
    pub fn build<W: Semiring + Clone>(self) -> CodeSwitchTransducer<W> {
        CodeSwitchTransducer {
            languages: self.languages,
            language_models: self.language_models,
            config: self.config,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl Default for CodeSwitchBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Code-switching transducer for multilingual processing.
///
/// Models language switches in multilingual text/speech with weighted penalties
/// for switching between languages.
#[derive(Debug, Clone)]
pub struct CodeSwitchTransducer<W: Semiring> {
    languages: Vec<LanguageConfig>,
    language_models: HashMap<LanguageId, SimpleLanguageModel>,
    config: CodeSwitchConfig,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring + Clone> CodeSwitchTransducer<W> {
    /// Create a new transducer with default settings.
    pub fn new() -> Self {
        Self {
            languages: Vec::new(),
            language_models: HashMap::new(),
            config: CodeSwitchConfig::default(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &CodeSwitchConfig {
        &self.config
    }

    /// Get the number of languages.
    pub fn num_languages(&self) -> usize {
        self.languages.len()
    }

    /// Get language configurations.
    pub fn languages(&self) -> &[LanguageConfig] {
        &self.languages
    }

    /// Score a word in a specific language.
    pub fn word_score(&self, word: &str, language: &LanguageId) -> f64 {
        if let Some(lm) = self.language_models.get(language) {
            lm.word_log_prob(word)
        } else if let Some(config) = self.languages.iter().find(|l| &l.id == language) {
            config.word_log_prob(word)
        } else {
            -100.0 // Very low for unknown language
        }
    }

    /// Score a sequence of words with assigned languages.
    pub fn score_with_languages(&self, words: &[&str], languages: &[LanguageId]) -> f64 {
        if words.len() != languages.len() {
            return f64::NEG_INFINITY;
        }

        let mut total_score = 0.0;

        for (i, (word, lang)) in words.iter().zip(languages.iter()).enumerate() {
            // Word score
            total_score += self.word_score(word, lang);

            // Switch penalty
            if i > 0 && &languages[i] != &languages[i - 1] {
                total_score -= self.config.switch_penalty;
            } else if i > 0 {
                total_score += self.config.same_language_bonus;
            }

            // Language prior
            if let Some(config) = self.languages.iter().find(|l| &l.id == lang) {
                total_score += config.prior.ln();
            }
        }

        total_score
    }

    /// Find the best language assignment for a sequence.
    pub fn best_path(&self, words: &[&str]) -> CodeSwitchPath {
        if words.is_empty() {
            return CodeSwitchPath::new(0.0, vec![]);
        }

        if self.languages.is_empty() {
            return CodeSwitchPath::new(f64::NEG_INFINITY, vec![]);
        }

        // Dynamic programming: best[i][lang] = best score ending at word i in lang
        let n = words.len();
        let langs: Vec<LanguageId> = self.languages.iter().map(|l| l.id.clone()).collect();
        let _m = langs.len();

        let mut best: Vec<HashMap<LanguageId, f64>> = vec![HashMap::new(); n];
        let mut back: Vec<HashMap<LanguageId, LanguageId>> = vec![HashMap::new(); n];

        // Initialize first word
        for lang in &langs {
            let word_score = self.word_score(words[0], lang);
            let prior = self
                .languages
                .iter()
                .find(|l| &l.id == lang)
                .map(|l| l.prior.ln())
                .unwrap_or(0.0);
            best[0].insert(lang.clone(), word_score + prior);
        }

        // Fill rest
        for i in 1..n {
            for to_lang in &langs {
                let word_score = self.word_score(words[i], to_lang);
                let prior = self
                    .languages
                    .iter()
                    .find(|l| &l.id == to_lang)
                    .map(|l| l.prior.ln())
                    .unwrap_or(0.0);

                let mut best_prev_score = f64::NEG_INFINITY;
                let mut best_prev_lang = to_lang.clone();

                for from_lang in &langs {
                    let prev_score = best[i - 1]
                        .get(from_lang)
                        .copied()
                        .unwrap_or(f64::NEG_INFINITY);

                    let transition_cost = if from_lang == to_lang {
                        self.config.same_language_bonus
                    } else {
                        -self.config.switch_penalty
                    };

                    let score = prev_score + transition_cost + word_score + prior;

                    if score > best_prev_score {
                        best_prev_score = score;
                        best_prev_lang = from_lang.clone();
                    }
                }

                best[i].insert(to_lang.clone(), best_prev_score);
                back[i].insert(to_lang.clone(), best_prev_lang);
            }
        }

        // Find best final language
        let mut best_final_score = f64::NEG_INFINITY;
        let mut best_final_lang = langs[0].clone();

        for lang in &langs {
            let score = best[n - 1].get(lang).copied().unwrap_or(f64::NEG_INFINITY);
            if score > best_final_score {
                best_final_score = score;
                best_final_lang = lang.clone();
            }
        }

        // Backtrack
        let mut path = vec![best_final_lang.clone()];
        let mut current_lang = best_final_lang;

        for i in (1..n).rev() {
            if let Some(prev_lang) = back[i].get(&current_lang) {
                path.push(prev_lang.clone());
                current_lang = prev_lang.clone();
            }
        }

        path.reverse();

        CodeSwitchPath::new(best_final_score, path)
    }

    /// Build a WFST representation of the code-switching model.
    ///
    /// The WFST has:
    /// - One state per language
    /// - Self-loops for words in vocabulary
    /// - Cross-transitions with switch penalty
    pub fn build_wfst(&self, vocabulary: &[String]) -> VectorWfst<String, W>
    where
        W: Clone,
    {
        let mut fst = VectorWfst::new();

        // Create one state per language
        let lang_states: HashMap<LanguageId, StateId> = self
            .languages
            .iter()
            .map(|l| {
                let state = fst.add_state();
                (l.id.clone(), state)
            })
            .collect();

        // All states are final
        for &state in lang_states.values() {
            fst.set_final(state, W::one());
        }

        // Set start state (first language or create super-start)
        if let Some(first_state) = lang_states.values().next() {
            if self.config.allow_any_start {
                // Create super-start with epsilon to all languages
                let super_start = fst.add_state();
                fst.set_start(super_start);

                for (_, &state) in lang_states.iter() {
                    fst.add_transition(WeightedTransition::epsilon(super_start, state, W::one()));
                }
            } else {
                fst.set_start(*first_state);
            }
        }

        // Add transitions for each word
        for word in vocabulary {
            for (from_lang, &from_state) in &lang_states {
                for (to_lang, &to_state) in &lang_states {
                    let word_score = self.word_score(word, to_lang);

                    let transition_cost = if from_lang == to_lang {
                        self.config.same_language_bonus
                    } else {
                        -self.config.switch_penalty
                    };

                    // Convert log-probability to weight
                    // For tropical semiring: weight = -log_prob (cost)
                    // We use the negative since tropical minimizes
                    let total_cost = -(word_score + transition_cost);

                    // Skip very high cost transitions
                    if total_cost < 100.0 {
                        fst.add_transition(WeightedTransition::new(
                            from_state,
                            Some(word.clone()),
                            Some(word.clone()),
                            to_state,
                            W::one(), // Weight encoding depends on semiring type
                        ));
                    }
                }
            }
        }

        fst
    }
}

impl<W: Semiring + Clone> Default for CodeSwitchTransducer<W> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_code_switch_config() {
        let config = CodeSwitchConfig::default()
            .with_switch_penalty(3.0)
            .with_same_language_bonus(0.1);

        assert!((config.switch_penalty - 3.0).abs() < f64::EPSILON);
        assert!((config.same_language_bonus - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_switch_point() {
        let sp = SwitchPoint::new(5, LanguageId::english(), LanguageId::spanish(), 2.0);

        assert_eq!(sp.position, 5);
        assert_eq!(sp.from_language.code(), "en");
        assert_eq!(sp.to_language.code(), "es");
    }

    #[test]
    fn test_language_span() {
        let span = LanguageSpan::new(
            LanguageId::english(),
            0,
            5,
            vec!["hello".to_string(), "world".to_string()],
        );

        assert_eq!(span.len(), 5);
        assert!(!span.is_empty());
        assert_eq!(span.language.code(), "en");
    }

    #[test]
    fn test_code_switch_path() {
        let languages = vec![
            LanguageId::english(),
            LanguageId::english(),
            LanguageId::spanish(),
            LanguageId::spanish(),
            LanguageId::english(),
        ];

        let path = CodeSwitchPath::new(0.0, languages);

        assert_eq!(path.num_switches(), 2);
        assert_eq!(path.spans.len(), 3);
    }

    #[test]
    fn test_code_switch_builder() {
        let english = LanguageConfig::new("en")
            .with_prior(0.6)
            .add_words(vec!["hello", "world", "the"]);

        let spanish = LanguageConfig::new("es")
            .with_prior(0.4)
            .add_words(vec!["hola", "mundo", "el"]);

        let transducer: CodeSwitchTransducer<TropicalWeight> = CodeSwitchBuilder::new()
            .add_language(english)
            .add_language(spanish)
            .switch_penalty(2.0)
            .build();

        assert_eq!(transducer.num_languages(), 2);
    }

    #[test]
    fn test_word_score() {
        let english = LanguageConfig::new("en")
            .with_prior(0.5)
            .add_word_prob("hello", -1.0)
            .add_word_prob("world", -2.0);

        let transducer: CodeSwitchTransducer<TropicalWeight> =
            CodeSwitchBuilder::new().add_language(english).build();

        let score = transducer.word_score("hello", &LanguageId::english());
        assert!((score - (-1.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_with_languages() {
        let english = LanguageConfig::new("en")
            .with_prior(1.0)
            .add_word_prob("hello", -1.0)
            .add_word_prob("world", -1.0);

        let spanish = LanguageConfig::new("es")
            .with_prior(1.0)
            .add_word_prob("hola", -1.0)
            .add_word_prob("mundo", -1.0);

        let transducer: CodeSwitchTransducer<TropicalWeight> = CodeSwitchBuilder::new()
            .add_language(english)
            .add_language(spanish)
            .switch_penalty(5.0)
            .build();

        // Same language should score better than switching
        let same_lang_score = transducer.score_with_languages(
            &["hello", "world"],
            &[LanguageId::english(), LanguageId::english()],
        );

        let switch_score = transducer.score_with_languages(
            &["hello", "mundo"],
            &[LanguageId::english(), LanguageId::spanish()],
        );

        // Same language should be better (higher score = better)
        assert!(same_lang_score > switch_score);
    }

    #[test]
    fn test_best_path() {
        let english = LanguageConfig::new("en")
            .with_prior(0.5)
            .add_word_prob("hello", -1.0)
            .add_word_prob("world", -1.0)
            .add_word_prob("the", -0.5);

        let spanish = LanguageConfig::new("es")
            .with_prior(0.5)
            .add_word_prob("hola", -1.0)
            .add_word_prob("mundo", -1.0)
            .add_word_prob("el", -0.5);

        let transducer: CodeSwitchTransducer<TropicalWeight> = CodeSwitchBuilder::new()
            .add_language(english)
            .add_language(spanish)
            .switch_penalty(3.0)
            .build();

        // English sequence should stay English
        let path = transducer.best_path(&["hello", "world"]);
        assert_eq!(path.word_languages.len(), 2);
        assert_eq!(path.word_languages[0].code(), "en");
        assert_eq!(path.word_languages[1].code(), "en");
        assert_eq!(path.num_switches(), 0);

        // Spanish sequence should stay Spanish
        let path2 = transducer.best_path(&["hola", "mundo"]);
        assert_eq!(path2.word_languages[0].code(), "es");
        assert_eq!(path2.word_languages[1].code(), "es");
        assert_eq!(path2.num_switches(), 0);
    }

    #[test]
    fn test_best_path_code_switch() {
        let english = LanguageConfig::new("en")
            .with_prior(0.5)
            .add_word_prob("hello", -0.5)
            .add_word_prob("mundo", -10.0); // Low prob for Spanish word

        let spanish = LanguageConfig::new("es")
            .with_prior(0.5)
            .add_word_prob("hola", -10.0) // Low prob for English word
            .add_word_prob("mundo", -0.5);

        let transducer: CodeSwitchTransducer<TropicalWeight> = CodeSwitchBuilder::new()
            .add_language(english)
            .add_language(spanish)
            .switch_penalty(1.0) // Low penalty to allow switching
            .build();

        // Code-switched sequence: English word followed by Spanish word
        let path = transducer.best_path(&["hello", "mundo"]);
        assert_eq!(path.word_languages[0].code(), "en");
        assert_eq!(path.word_languages[1].code(), "es");
        assert_eq!(path.num_switches(), 1);
    }

    #[test]
    fn test_dominant_language() {
        let languages = vec![
            LanguageId::english(),
            LanguageId::english(),
            LanguageId::english(),
            LanguageId::spanish(),
        ];

        let path = CodeSwitchPath::new(0.0, languages);
        assert_eq!(
            path.dominant_language()
                .expect("multilingual/code_switch.rs: required value was None/Err")
                .code(),
            "en"
        );
    }

    #[test]
    fn test_build_wfst() {
        let english = LanguageConfig::new("en")
            .with_prior(0.5)
            .add_words(vec!["hello", "world"]);

        let spanish = LanguageConfig::new("es")
            .with_prior(0.5)
            .add_words(vec!["hola", "mundo"]);

        let transducer: CodeSwitchTransducer<TropicalWeight> = CodeSwitchBuilder::new()
            .add_language(english)
            .add_language(spanish)
            .build();

        let vocab = vec![
            "hello".to_string(),
            "world".to_string(),
            "hola".to_string(),
            "mundo".to_string(),
        ];

        let fst = transducer.build_wfst(&vocab);

        // Should have states for languages plus super-start
        assert!(fst.num_states() >= 2);
    }

    #[test]
    fn test_empty_sequence() {
        let transducer: CodeSwitchTransducer<TropicalWeight> = CodeSwitchBuilder::new()
            .add_language(LanguageConfig::new("en"))
            .build();

        let path = transducer.best_path(&[]);
        assert!(path.word_languages.is_empty());
        assert_eq!(path.num_switches(), 0);
    }
}
