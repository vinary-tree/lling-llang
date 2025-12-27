# liblevenshtein Integration with lling-llang

This guide explains how to use liblevenshtein with lling-llang for spelling correction, phonetic matching, and code completion.

## Overview

lling-llang's correction layers use liblevenshtein for:

- **Spelling correction**: Generate candidate corrections via fuzzy matching
- **Phonetic matching**: Find phonetically similar words
- **Code completion**: Fuzzy prefix and infix completion

```
Input: "teh quick brwon fox"
          │
          ▼
┌─────────────────────────┐
│   SpellingCorrectionLayer│
│   (liblevenshtein)       │
└───────────┬─────────────┘
            ▼
┌─────────────────────────┐
│     Lattice Builder     │
│   (add correction edges) │
└───────────┬─────────────┘
            ▼
┌─────────────────────────┐
│   Grammar Filter Layer  │
│   (select valid paths)  │
└───────────┬─────────────┘
            ▼
Output: "the quick brown fox"
```

## SpellingCorrectionLayer

Generates spelling candidates and adds them to the lattice.

### Basic Implementation

```rust
use lling_llang::layers::CorrectionLayer;
use lling_llang::lattice::{Lattice, LatticeBuilder, EdgeMetadata};
use lling_llang::semiring::TropicalWeight;
use lling_llang::backend::HashMapBackend;
use liblevenshtein::prelude::*;

pub struct SpellingCorrectionLayer {
    transducer: Transducer<DoubleArrayTrie>,
    max_distance: usize,
}

impl SpellingCorrectionLayer {
    pub fn new(dictionary: DoubleArrayTrie, max_distance: usize) -> Self {
        Self {
            transducer: Transducer::standard(dictionary),
            max_distance,
        }
    }

    pub fn from_word_list(words: Vec<&str>, max_distance: usize) -> Self {
        let dict = DoubleArrayTrie::from_terms(words);
        Self::new(dict, max_distance)
    }
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for SpellingCorrectionLayer {
    fn name(&self) -> &str {
        "spelling-correction"
    }

    fn apply(&self, lattice: &Lattice<TropicalWeight, B>)
        -> Result<Lattice<TropicalWeight, B>, LayerError>
    {
        let mut builder = LatticeBuilder::new(lattice.backend().clone());

        // Copy existing edges
        for edge in lattice.edges() {
            builder.add_edge(edge.clone());
        }

        // For each position, add correction candidates
        for (pos, token) in lattice.tokens().enumerate() {
            // Skip if token is already in dictionary
            if self.transducer.dictionary().contains(token) {
                continue;
            }

            // Find fuzzy matches
            for candidate in self.transducer.query_with_distance(token, self.max_distance) {
                let weight = TropicalWeight::new(candidate.distance as f64);
                let meta = EdgeMetadata::correction(
                    token.to_string(),
                    candidate.term.clone(),
                    candidate.distance,
                );

                builder.add_correction(
                    pos,
                    pos + 1,
                    &candidate.term,
                    weight,
                    meta,
                );
            }
        }

        Ok(builder.build(lattice.num_nodes()))
    }

    fn estimated_reduction(&self) -> f64 {
        1.0  // Doesn't reduce, only adds alternatives
    }
}
```

### Usage

```rust
// Load dictionary
let words = vec!["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog"];
let layer = SpellingCorrectionLayer::from_word_list(words, 2);

// Create input lattice
let mut builder = LatticeBuilder::new(HashMapBackend::new());
builder.add_token(0, 1, "teh", TropicalWeight::one());
builder.add_token(1, 2, "quikc", TropicalWeight::one());
let input = builder.build(2);

// Apply correction
let corrected = layer.apply(&input)?;

// Extract paths
for path in nbest(&mut corrected, 5) {
    println!("{:?}", path.labels);
}
```

### With Transposition

```rust
pub struct TranspositionSpellingLayer {
    transducer: Transducer<DoubleArrayTrie>,
    max_distance: usize,
}

impl TranspositionSpellingLayer {
    pub fn new(dictionary: DoubleArrayTrie, max_distance: usize) -> Self {
        Self {
            transducer: Transducer::with_transposition(dictionary),
            max_distance,
        }
    }
}
```

## PhoneticMatchingLayer

Finds phonetically similar words using Soundex, Metaphone, etc.

### Phonetic Encoding

```rust
use liblevenshtein::phonetic::{Soundex, Metaphone, DoubleMetaphone};

// Encode words
let soundex = Soundex::new();
let code = soundex.encode("smith");  // "S530"

let metaphone = Metaphone::new();
let code = metaphone.encode("smith"); // "SM0"

let dmetaphone = DoubleMetaphone::new();
let (primary, alt) = dmetaphone.encode("smith"); // ("SM0", "XMT")
```

### Phonetic Dictionary

```rust
use std::collections::HashMap;
use liblevenshtein::dictionary::DynamicDawgChar;

pub struct PhoneticDictionary {
    // Maps phonetic code to original words
    code_to_words: DynamicDawgChar<Vec<String>>,
    encoder: Box<dyn PhoneticEncoder>,
}

impl PhoneticDictionary {
    pub fn new(words: Vec<&str>, encoder: Box<dyn PhoneticEncoder>) -> Self {
        let mut code_to_words = DynamicDawgChar::new();

        for word in words {
            let code = encoder.encode(word);
            code_to_words.update_or_insert(
                &code,
                vec![word.to_string()],
                |existing| existing.push(word.to_string()),
            );
        }

        Self { code_to_words, encoder }
    }

    pub fn find_similar(&self, word: &str, max_distance: usize) -> Vec<String> {
        let code = self.encoder.encode(word);
        let transducer = Transducer::standard(&self.code_to_words);

        let mut results = Vec::new();
        for matching_code in transducer.query(&code, max_distance) {
            if let Some(words) = self.code_to_words.get_value(&matching_code) {
                results.extend(words.clone());
            }
        }
        results
    }
}
```

### PhoneticMatchingLayer

```rust
pub struct PhoneticMatchingLayer {
    phonetic_dict: PhoneticDictionary,
    max_phonetic_distance: usize,
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for PhoneticMatchingLayer {
    fn name(&self) -> &str {
        "phonetic-matching"
    }

    fn apply(&self, lattice: &Lattice<TropicalWeight, B>)
        -> Result<Lattice<TropicalWeight, B>, LayerError>
    {
        let mut builder = LatticeBuilder::new(lattice.backend().clone());

        for edge in lattice.edges() {
            builder.add_edge(edge.clone());
        }

        for (pos, token) in lattice.tokens().enumerate() {
            let similar = self.phonetic_dict.find_similar(token, self.max_phonetic_distance);

            for candidate in similar {
                if candidate != token {
                    let weight = TropicalWeight::new(0.5);  // Phonetic match cost
                    let meta = EdgeMetadata::phonetic_correction(
                        token.to_string(),
                        candidate.clone(),
                    );

                    builder.add_correction(pos, pos + 1, &candidate, weight, meta);
                }
            }
        }

        Ok(builder.build(lattice.num_nodes()))
    }

    fn estimated_reduction(&self) -> f64 {
        1.0
    }
}
```

## CodeCompletionLayer

Fuzzy prefix and infix completion for code editors.

### Prefix Completion

```rust
pub struct CodeCompletionLayer {
    symbols: DynamicDawgChar<SymbolInfo>,
    max_distance: usize,
}

#[derive(Clone)]
pub struct SymbolInfo {
    pub kind: SymbolKind,
    pub scope: ScopeId,
    pub priority: u8,
}

impl CodeCompletionLayer {
    pub fn complete_prefix(&self, prefix: &str, max_results: usize) -> Vec<Completion> {
        let transducer = Transducer::standard(&self.symbols);

        transducer
            .query_ranked(prefix, self.max_distance)
            .take(max_results)
            .filter_map(|candidate| {
                self.symbols.get_value(&candidate.term).map(|info| {
                    Completion {
                        text: candidate.term,
                        distance: candidate.distance,
                        kind: info.kind,
                        priority: info.priority,
                    }
                })
            })
            .collect()
    }
}
```

### Scope-Aware Completion

```rust
impl CodeCompletionLayer {
    pub fn complete_in_scope(
        &self,
        prefix: &str,
        current_scope: ScopeId,
        max_results: usize,
    ) -> Vec<Completion> {
        let transducer = Transducer::standard(&self.symbols);

        // Filter by scope during traversal
        transducer
            .query_filtered(prefix, self.max_distance, |info| {
                info.scope.is_visible_from(current_scope)
            })
            .take(max_results)
            .map(|term| {
                let info = self.symbols.get_value(&term).unwrap();
                Completion {
                    text: term,
                    distance: 0,  // Filtered query doesn't return distance
                    kind: info.kind,
                    priority: info.priority,
                }
            })
            .collect()
    }
}
```

### Infix Completion

For substring matching (e.g., "usr" → "getUser"):

```rust
use liblevenshtein::dictionary::SuffixAutomatonChar;

pub struct InfixCompletionLayer {
    symbols: SuffixAutomatonChar<SymbolInfo>,
    max_distance: usize,
}

impl InfixCompletionLayer {
    pub fn complete_infix(&self, query: &str, max_results: usize) -> Vec<Completion> {
        let transducer = Transducer::standard(&self.symbols);

        transducer
            .query_ranked(query, self.max_distance)
            .take(max_results)
            .map(|candidate| Completion {
                text: candidate.term,
                distance: candidate.distance,
                kind: SymbolKind::Unknown,
                priority: 0,
            })
            .collect()
    }
}
```

## Combined Spelling Layer

Combines edit distance and phonetic matching:

```rust
pub struct CombinedSpellingLayer {
    edit_transducer: Transducer<DoubleArrayTrie>,
    phonetic_dict: PhoneticDictionary,
    edit_distance: usize,
    phonetic_distance: usize,
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for CombinedSpellingLayer {
    fn name(&self) -> &str {
        "combined-spelling"
    }

    fn apply(&self, lattice: &Lattice<TropicalWeight, B>)
        -> Result<Lattice<TropicalWeight, B>, LayerError>
    {
        let mut builder = LatticeBuilder::new(lattice.backend().clone());
        let mut seen = HashSet::new();

        for edge in lattice.edges() {
            builder.add_edge(edge.clone());
        }

        for (pos, token) in lattice.tokens().enumerate() {
            seen.clear();

            // Edit distance candidates
            for candidate in self.edit_transducer.query_with_distance(token, self.edit_distance) {
                if seen.insert(candidate.term.clone()) {
                    let weight = TropicalWeight::new(candidate.distance as f64);
                    builder.add_correction(pos, pos + 1, &candidate.term, weight,
                        EdgeMetadata::edit_correction(token, &candidate.term, candidate.distance));
                }
            }

            // Phonetic candidates
            for candidate in self.phonetic_dict.find_similar(token, self.phonetic_distance) {
                if seen.insert(candidate.clone()) {
                    let weight = TropicalWeight::new(0.5);
                    builder.add_correction(pos, pos + 1, &candidate, weight,
                        EdgeMetadata::phonetic_correction(token, &candidate));
                }
            }
        }

        Ok(builder.build(lattice.num_nodes()))
    }

    fn estimated_reduction(&self) -> f64 {
        1.0
    }
}
```

## Integration Pipeline

Full correction pipeline:

```rust
use lling_llang::layers::LayerPipelineBuilder;

fn build_correction_pipeline(
    dictionary: DoubleArrayTrie,
    grammar: &Grammar,
) -> LayerPipeline<TropicalWeight, HashMapBackend> {
    LayerPipelineBuilder::new()
        // Layer 1: Spelling candidates
        .add_layer(SpellingCorrectionLayer::new(dictionary.clone(), 2))
        // Layer 2: Phonetic candidates
        .add_layer(PhoneticMatchingLayer::new(
            PhoneticDictionary::new(dictionary.terms(), Box::new(Soundex::new())),
            1
        ))
        // Layer 3: Grammar filter
        .add_layer(CfgFilterLayer::new(grammar))
        // Layer 4: Language model ranking
        .add_layer(LanguageModelLayer::new(lm))
        .build()
}

// Usage
let pipeline = build_correction_pipeline(dictionary, &grammar);

let input = tokenize("teh quikc brwon fox");
let lattice = tokens_to_lattice(&input);

let result = pipeline.apply(&lattice)?;
let best_path = viterbi(&mut result);
println!("{}", best_path.to_string());  // "the quick brown fox"
```

## Performance Optimization

### Dictionary Selection

```rust
// Static dictionary: use DoubleArrayTrie
let dict = DoubleArrayTrie::from_terms(words);

// Dynamic dictionary: use DynamicDawg
let dict = DynamicDawg::from_terms(words);

// Unicode: use Char variants
let dict = DoubleArrayTrieChar::from_terms(unicode_words);
```

### Caching Results

```rust
use liblevenshtein::cache::eviction::Lru;

pub struct CachedSpellingLayer {
    transducer: Transducer<DoubleArrayTrie>,
    cache: RwLock<HashMap<String, Vec<Candidate>>>,
    max_cache_size: usize,
}

impl CachedSpellingLayer {
    fn get_candidates(&self, token: &str) -> Vec<Candidate> {
        // Check cache
        if let Some(cached) = self.cache.read().unwrap().get(token) {
            return cached.clone();
        }

        // Compute
        let candidates: Vec<_> = self.transducer
            .query_with_distance(token, 2)
            .collect();

        // Cache
        let mut cache = self.cache.write().unwrap();
        if cache.len() < self.max_cache_size {
            cache.insert(token.to_string(), candidates.clone());
        }

        candidates
    }
}
```

### Parallel Processing

```rust
use rayon::prelude::*;

impl CombinedSpellingLayer {
    fn apply_parallel(&self, tokens: &[&str]) -> Vec<Vec<Candidate>> {
        tokens
            .par_iter()
            .map(|token| {
                self.edit_transducer
                    .query_with_distance(token, self.edit_distance)
                    .collect()
            })
            .collect()
    }
}
```

## Error Types

```rust
pub enum SpellingLayerError {
    DictionaryLoadError(String),
    TransducerError(String),
    LatticeError(LayerError),
}

impl From<SpellingLayerError> for LayerError {
    fn from(e: SpellingLayerError) -> Self {
        LayerError::Layer(format!("Spelling layer error: {:?}", e))
    }
}
```

## Next Steps

- [Overview](overview.md): liblevenshtein architecture
- [Dictionaries](dictionaries.md): Dictionary implementations
- [Transducers](transducers.md): Query API
- [Fuzzy Collections](fuzzy-collections.md): Maps and caches
- [Layers](../../architecture/layers.md): lling-llang layer architecture
