# Text Correction Applications

lling-llang provides a framework for building spelling and grammar correction applications.

## Overview

Text correction involves multiple stages:

```
Input Text: "Teh quikc brwon fox jmups ovre the lzay dog"
                │
                ▼
┌─────────────────────────────────────┐
│           Tokenization              │
└───────────────┬─────────────────────┘
                ▼
┌─────────────────────────────────────┐
│     Initial Lattice (1 path)        │
└───────────────┬─────────────────────┘
                ▼
┌─────────────────────────────────────┐
│       Spelling Candidates           │
│   (add edit-distance matches)       │
└───────────────┬─────────────────────┘
                ▼
┌─────────────────────────────────────┐
│        Grammar Filtering            │
│   (remove invalid sequences)        │
└───────────────┬─────────────────────┘
                ▼
┌─────────────────────────────────────┐
│      Language Model Ranking         │
│   (score by fluency)                │
└───────────────┬─────────────────────┘
                ▼
Output: "The quick brown fox jumps over the lazy dog"
```

## Building a Spelling Corrector

### Basic Architecture

```rust
use lling_llang::prelude::*;
use liblevenshtein::prelude::*;

pub struct SpellingCorrector {
    dictionary: DoubleArrayTrie,
    transducer: Transducer<DoubleArrayTrie>,
    max_edit_distance: usize,
}

impl SpellingCorrector {
    pub fn new(words: Vec<&str>, max_distance: usize) -> Self {
        let dictionary = DoubleArrayTrie::from_terms(words);
        let transducer = Transducer::with_transposition(dictionary.clone());

        Self {
            dictionary,
            transducer,
            max_edit_distance: max_distance,
        }
    }

    pub fn correct(&self, text: &str) -> String {
        // Tokenize
        let tokens = self.tokenize(text);

        // Build lattice with candidates
        let lattice = self.build_lattice(&tokens);

        // Extract best path
        let best = viterbi(&mut lattice.clone());

        // Reconstruct text
        self.reconstruct(&best, &tokens)
    }

    fn tokenize(&self, text: &str) -> Vec<Token> {
        // Preserve whitespace and punctuation info
        tokenize_preserving_context(text)
    }

    fn build_lattice(&self, tokens: &[Token]) -> Lattice<TropicalWeight, HashMapBackend> {
        let mut builder = LatticeBuilder::new(HashMapBackend::new());

        for (i, token) in tokens.iter().enumerate() {
            // Add original token
            builder.add_correction(
                i, i + 1,
                &token.text,
                TropicalWeight::zero(),  // No cost for original
                EdgeMetadata::original(),
            );

            // Skip if in dictionary
            if self.dictionary.contains(&token.text.to_lowercase()) {
                continue;
            }

            // Add spelling candidates
            for candidate in self.transducer.query_with_distance(
                &token.text.to_lowercase(),
                self.max_edit_distance
            ) {
                let weight = TropicalWeight::new(candidate.distance as f64);
                builder.add_correction(
                    i, i + 1,
                    &candidate.term,
                    weight,
                    EdgeMetadata::spelling_correction(&token.text, &candidate.term),
                );
            }
        }

        builder.build(tokens.len())
    }

    fn reconstruct(&self, path: &Path<TropicalWeight>, original: &[Token]) -> String {
        // Preserve original spacing and capitalization
        let mut result = String::new();

        for (i, (label, token)) in path.labels.iter().zip(original).enumerate() {
            if i > 0 {
                result.push_str(&token.leading_whitespace);
            }

            // Match original capitalization
            let corrected = match_case(label, &token.text);
            result.push_str(&corrected);
        }

        result
    }
}
```

### Preserving Context

```rust
#[derive(Clone)]
pub struct Token {
    pub text: String,
    pub leading_whitespace: String,
    pub is_capitalized: bool,
    pub is_all_caps: bool,
}

fn tokenize_preserving_context(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current_whitespace = String::new();

    for segment in text.split_inclusive(char::is_whitespace) {
        let trimmed = segment.trim_end();
        let whitespace = &segment[trimmed.len()..];

        if !trimmed.is_empty() {
            tokens.push(Token {
                text: trimmed.to_string(),
                leading_whitespace: current_whitespace.clone(),
                is_capitalized: trimmed.chars().next().map(|c| c.is_uppercase()).unwrap_or(false),
                is_all_caps: trimmed.chars().all(|c| c.is_uppercase()),
            });
        }

        current_whitespace = whitespace.to_string();
    }

    tokens
}

fn match_case(corrected: &str, original: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) {
        corrected.to_uppercase()
    } else if original.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        let mut chars = corrected.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    } else {
        corrected.to_string()
    }
}
```

## Grammar Correction

### Grammar-Aware Corrector

```rust
use lling_llang::cfg::Grammar;
use lling_llang::layers::CfgFilterLayer;

pub struct GrammarCorrector {
    spelling: SpellingCorrector,
    grammar: Grammar,
}

impl GrammarCorrector {
    pub fn correct(&self, text: &str) -> Result<String, CorrectionError> {
        let tokens = self.spelling.tokenize(text);

        // Build spelling lattice
        let spelling_lattice = self.spelling.build_lattice(&tokens);

        // Filter by grammar
        let grammar_layer = CfgFilterLayer::new(&self.grammar);
        let filtered = grammar_layer.apply(&spelling_lattice)?;

        // Check if any valid paths remain
        if filtered.num_edges() == 0 {
            return Err(CorrectionError::NoValidCorrection);
        }

        // Extract best
        let best = viterbi(&mut filtered.clone());
        Ok(self.spelling.reconstruct(&best, &tokens))
    }
}
```

### Part-of-Speech Based Grammar

```rust
use lling_llang::cfg::GrammarBuilder;

fn build_english_grammar() -> Grammar {
    GrammarBuilder::new()
        .start("S")
        // Sentence structures
        .rule("S", &["NP", "VP"])
        .rule("S", &["NP", "VP", "PP"])

        // Noun phrases
        .rule("NP", &["Det", "N"])
        .rule("NP", &["Det", "Adj", "N"])
        .rule("NP", &["Det", "Adj", "Adj", "N"])
        .rule("NP", &["PropN"])

        // Verb phrases
        .rule("VP", &["V"])
        .rule("VP", &["V", "NP"])
        .rule("VP", &["V", "NP", "PP"])
        .rule("VP", &["Aux", "V"])
        .rule("VP", &["Aux", "V", "NP"])

        // Prepositional phrases
        .rule("PP", &["P", "NP"])

        // Terminals (simplified)
        .terminal("Det", &["the", "a", "an", "this", "that"])
        .terminal("N", &["dog", "cat", "fox", "man", "woman"])
        .terminal("Adj", &["quick", "brown", "lazy", "big", "small"])
        .terminal("V", &["runs", "jumps", "eats", "sees", "likes"])
        .terminal("Aux", &["is", "was", "has", "have", "will"])
        .terminal("P", &["over", "under", "with", "to", "from"])
        .terminal("PropN", &["John", "Mary", "London", "Paris"])

        .build()
        .expect("valid grammar")
}
```

## Language Model Integration

### N-gram Scoring

```rust
pub struct NgramScorer {
    model: NgramModel,
    unknown_penalty: f64,
}

impl NgramScorer {
    pub fn score_path(&self, path: &[String]) -> f64 {
        let mut total_score = 0.0;

        // Add BOS token
        let mut context = vec!["<s>".to_string()];

        for word in path {
            let score = self.model.score(&context, word)
                .unwrap_or(self.unknown_penalty);
            total_score += score;
            context.push(word.clone());
        }

        // Add EOS token
        total_score += self.model.score(&context, "</s>")
            .unwrap_or(self.unknown_penalty);

        total_score
    }
}

pub struct LmScoringLayer {
    scorer: NgramScorer,
    weight: f64,
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for LmScoringLayer {
    fn name(&self) -> &str {
        "lm-scoring"
    }

    fn apply(&self, lattice: &Lattice<TropicalWeight, B>)
        -> Result<Lattice<TropicalWeight, B>, LayerError>
    {
        let mut new_lattice = lattice.clone();

        for path in enumerate_all_paths(&new_lattice) {
            let lm_score = self.scorer.score_path(&path.labels);
            let combined = path.weight.value() + self.weight * lm_score;

            // Update path weight
            set_path_weight(&mut new_lattice, &path, TropicalWeight::new(combined));
        }

        Ok(new_lattice)
    }

    fn estimated_reduction(&self) -> f64 {
        1.0
    }
}
```

## Real-Word Error Detection

### Context-Based Detection

```rust
pub struct ContextualCorrector {
    dictionary: DoubleArrayTrie,
    confusion_sets: HashMap<String, Vec<String>>,
    lm: NgramModel,
}

impl ContextualCorrector {
    /// Detect real-word errors using context
    pub fn detect_errors(&self, tokens: &[String]) -> Vec<(usize, Vec<String>)> {
        let mut errors = Vec::new();

        for i in 0..tokens.len() {
            // Get confusion set for this word
            if let Some(confusions) = self.confusion_sets.get(&tokens[i]) {
                let context = self.get_context(tokens, i);

                // Score each alternative
                let mut scores: Vec<_> = confusions.iter()
                    .map(|alt| {
                        let mut test_tokens = tokens.to_vec();
                        test_tokens[i] = alt.clone();
                        (alt.clone(), self.lm.score_sequence(&test_tokens))
                    })
                    .collect();

                scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

                // If best alternative is not the original, flag as error
                if scores[0].0 != tokens[i] {
                    errors.push((i, scores.into_iter().map(|(w, _)| w).collect()));
                }
            }
        }

        errors
    }

    fn get_context(&self, tokens: &[String], pos: usize) -> Vec<String> {
        let start = pos.saturating_sub(2);
        let end = (pos + 3).min(tokens.len());
        tokens[start..end].to_vec()
    }
}
```

### Common Confusion Sets

```rust
fn build_confusion_sets() -> HashMap<String, Vec<String>> {
    let mut sets = HashMap::new();

    // Homophones
    sets.insert("there".into(), vec!["their".into(), "they're".into()]);
    sets.insert("their".into(), vec!["there".into(), "they're".into()]);
    sets.insert("they're".into(), vec!["there".into(), "their".into()]);

    sets.insert("your".into(), vec!["you're".into()]);
    sets.insert("you're".into(), vec!["your".into()]);

    sets.insert("its".into(), vec!["it's".into()]);
    sets.insert("it's".into(), vec!["its".into()]);

    // Common typos that are words
    sets.insert("form".into(), vec!["from".into()]);
    sets.insert("from".into(), vec!["form".into()]);

    sets.insert("then".into(), vec!["than".into()]);
    sets.insert("than".into(), vec!["then".into()]);

    sets
}
```

## Complete Correction Pipeline

```rust
use lling_llang::layers::LayerPipelineBuilder;

pub struct TextCorrector {
    pipeline: LayerPipeline<TropicalWeight, HashMapBackend>,
    tokenizer: Tokenizer,
}

impl TextCorrector {
    pub fn new(config: CorrectorConfig) -> Result<Self, Error> {
        let spelling_layer = SpellingCorrectionLayer::new(
            config.dictionary.clone(),
            config.max_edit_distance,
        );

        let grammar_layer = CfgFilterLayer::new(&config.grammar);

        let lm_layer = LmScoringLayer::new(
            config.language_model,
            config.lm_weight,
        );

        let pipeline = LayerPipelineBuilder::new()
            .add_layer(spelling_layer)
            .add_layer(grammar_layer)
            .add_layer(lm_layer)
            .build();

        Ok(Self {
            pipeline,
            tokenizer: Tokenizer::new(),
        })
    }

    pub fn correct(&self, text: &str) -> CorrectionResult {
        let tokens = self.tokenizer.tokenize(text);

        // Build initial lattice
        let initial = tokens_to_lattice(&tokens);

        // Apply correction pipeline
        let corrected = match self.pipeline.apply(&initial) {
            Ok(lat) => lat,
            Err(e) => return CorrectionResult::error(e),
        };

        // Extract best path
        let best = viterbi(&mut corrected.clone());

        // Build result with change tracking
        CorrectionResult {
            original: text.to_string(),
            corrected: reconstruct_text(&best, &tokens),
            changes: extract_changes(&tokens, &best),
            confidence: calculate_confidence(&best),
        }
    }
}

#[derive(Debug)]
pub struct CorrectionResult {
    pub original: String,
    pub corrected: String,
    pub changes: Vec<Change>,
    pub confidence: f64,
}

#[derive(Debug)]
pub struct Change {
    pub position: usize,
    pub original: String,
    pub corrected: String,
    pub change_type: ChangeType,
}

#[derive(Debug)]
pub enum ChangeType {
    Spelling,
    Grammar,
    Punctuation,
    Capitalization,
}
```

## Interactive Correction

### Suggestions API

```rust
impl TextCorrector {
    /// Get suggestions for a specific position
    pub fn get_suggestions(&self, text: &str, position: usize, limit: usize)
        -> Vec<Suggestion>
    {
        let tokens = self.tokenizer.tokenize(text);
        let lattice = self.build_lattice(&tokens);

        // Get edges at position
        let candidates: Vec<_> = lattice.edges_at_position(position)
            .map(|edge| Suggestion {
                text: edge.label().to_string(),
                score: edge.weight.value(),
                reason: edge.metadata().correction_type(),
            })
            .sorted_by(|a, b| a.score.partial_cmp(&b.score).unwrap())
            .take(limit)
            .collect();

        candidates
    }

    /// Apply a specific suggestion
    pub fn apply_suggestion(&self, text: &str, position: usize, suggestion: &str)
        -> String
    {
        let mut tokens = self.tokenizer.tokenize(text);
        tokens[position].text = suggestion.to_string();
        reconstruct_from_tokens(&tokens)
    }
}

#[derive(Debug)]
pub struct Suggestion {
    pub text: String,
    pub score: f64,
    pub reason: String,
}
```

## Performance Optimization

### Caching Corrections

```rust
use std::collections::HashMap;
use std::sync::RwLock;

pub struct CachedCorrector {
    inner: TextCorrector,
    cache: RwLock<HashMap<String, CorrectionResult>>,
    max_cache_size: usize,
}

impl CachedCorrector {
    pub fn correct(&self, text: &str) -> CorrectionResult {
        // Check cache
        if let Some(result) = self.cache.read().unwrap().get(text) {
            return result.clone();
        }

        // Compute correction
        let result = self.inner.correct(text);

        // Cache result
        let mut cache = self.cache.write().unwrap();
        if cache.len() < self.max_cache_size {
            cache.insert(text.to_string(), result.clone());
        }

        result
    }
}
```

### Incremental Correction

```rust
pub struct IncrementalCorrector {
    inner: TextCorrector,
    last_text: String,
    last_lattice: Option<Lattice<TropicalWeight, HashMapBackend>>,
}

impl IncrementalCorrector {
    pub fn correct(&mut self, text: &str) -> CorrectionResult {
        // Find common prefix with last text
        let common_len = self.find_common_prefix_len(&self.last_text, text);

        if common_len > 0 && self.last_lattice.is_some() {
            // Reuse prefix of previous lattice
            let lattice = self.extend_lattice(text, common_len);
            self.last_lattice = Some(lattice.clone());
            self.last_text = text.to_string();
            // Process...
        } else {
            // Full recomputation
            let result = self.inner.correct(text);
            self.last_text = text.to_string();
            result
        }
    }
}
```

## Next Steps

- [Speech/NLP](speech-nlp.md): Speech recognition integration
- [Library Usage](library-usage.md): Generic integration patterns
- [liblevenshtein](../liblevenshtein/overview.md): Fuzzy matching library
- [Layers](../../architecture/layers.md): Layer architecture
