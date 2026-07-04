# Library Usage Patterns

This guide covers common patterns for integrating lling-llang into applications.

## Getting Started

### Adding the Dependency

```toml
[dependencies]
lling-llang = "0.1"

# Optional features
lling-llang = { version = "0.1", features = [
    "serialization",  # Save/load lattices
    "parallel",       # Parallel processing
]}
```

### Basic Usage

```rust
use lling_llang::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple lattice
    let mut builder = LatticeBuilder::new(HashMapBackend::new());

    builder.add_correction(0, 1, "hello", TropicalWeight::one(), EdgeMetadata::default());
    builder.add_correction(0, 1, "hallo", TropicalWeight::new(1.0), EdgeMetadata::default());
    builder.add_correction(1, 2, "world", TropicalWeight::one(), EdgeMetadata::default());

    let mut lattice = builder.build(2);

    // Find best path
    let best = viterbi(&mut lattice);
    println!("Best path: {:?}", best.labels);

    Ok(())
}
```

## Common Patterns

### Builder Pattern

lling-llang uses the builder pattern extensively:

```rust
// Lattice building
let lattice = LatticeBuilder::new(backend)
    .add_correction(0, 1, "word1", weight, meta)
    .add_correction(1, 2, "word2", weight, meta)
    .build(2);

// Pipeline building
let pipeline = LayerPipelineBuilder::new()
    .add_layer(layer1)
    .add_layer(layer2)
    .add_layer(layer3)
    .build();

// Grammar building
let grammar = GrammarBuilder::new()
    .start("S")
    .rule("S", &["NP", "VP"])
    .build()?;
```

### Trait Objects for Flexibility

Use trait objects when you need runtime polymorphism:

```rust
use lling_llang::layers::CorrectionLayer;

// Store different layer types in a collection
let layers: Vec<Box<dyn CorrectionLayer<TropicalWeight, HashMapBackend>>> = vec![
    Box::new(SpellingLayer::new(dict)),
    Box::new(GrammarLayer::new(grammar)),
    Box::new(LanguageModelLayer::new(lm)),
];

// Apply dynamically
for layer in &layers {
    lattice = layer.apply(&lattice)?;
}
```

### Generic Over Semiring

Write code that works with any semiring:

```rust
use lling_llang::semiring::Semiring;

fn total_weight<W: Semiring>(paths: &[Path<W>]) -> W {
    paths.iter()
        .fold(W::zero(), |acc, path| acc.plus(&path.weight))
}

// Works with any semiring
let tropical_total = total_weight::<TropicalWeight>(&tropical_paths);
let log_total = total_weight::<LogWeight>(&log_paths);
```

### Generic Over Backend

Write code that works with any backend:

```rust
use lling_llang::backend::LatticeBackend;

fn process_lattice<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
) -> Vec<String> {
    lattice.edges()
        .map(|e| lattice.backend().lookup(e.vocab_id()).unwrap().to_string())
        .collect()
}
```

## Error Handling

### Layer Errors

```rust
use lling_llang::layers::{LayerError, CorrectionLayer};

fn apply_with_fallback<W, B, L>(
    layer: &L,
    lattice: &Lattice<W, B>,
) -> Lattice<W, B>
where
    W: Semiring,
    B: LatticeBackend + Clone,
    L: CorrectionLayer<W, B>,
{
    match layer.apply(lattice) {
        Ok(result) => result,
        Err(LayerError::NoValidPaths) => {
            // Fall back to original
            eprintln!("Warning: layer {} produced no valid paths", layer.name());
            lattice.clone()
        }
        Err(e) => {
            // Log and continue
            eprintln!("Error in layer {}: {:?}", layer.name(), e);
            lattice.clone()
        }
    }
}
```

### Result Types

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CorrectionError {
    #[error("Layer error: {0}")]
    Layer(#[from] LayerError),

    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("No valid correction found")]
    NoValidCorrection,

    #[error("Dictionary not found: {0}")]
    DictionaryNotFound(String),
}

fn correct(text: &str) -> Result<String, CorrectionError> {
    let lattice = build_lattice(text)?;
    let filtered = grammar_layer.apply(&lattice)?;

    if filtered.num_edges() == 0 {
        return Err(CorrectionError::NoValidCorrection);
    }

    let best = viterbi(&mut filtered);
    Ok(best.to_string())
}
```

## Configuration

### Runtime Configuration

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectorConfig {
    pub max_edit_distance: usize,
    pub beam_width: usize,
    pub lm_weight: f64,
    pub use_grammar: bool,
    pub dictionary_path: String,
}

impl Default for CorrectorConfig {
    fn default() -> Self {
        Self {
            max_edit_distance: 2,
            beam_width: 10,
            lm_weight: 1.0,
            use_grammar: true,
            dictionary_path: "dictionary.txt".into(),
        }
    }
}

pub struct Corrector {
    config: CorrectorConfig,
    // ...
}

impl Corrector {
    pub fn from_config(config: CorrectorConfig) -> Result<Self, Error> {
        let dictionary = load_dictionary(&config.dictionary_path)?;
        // ...
        Ok(Self { config, /* ... */ })
    }
}
```

### Environment Configuration

```rust
use std::env;

fn load_config() -> CorrectorConfig {
    CorrectorConfig {
        max_edit_distance: env::var("MAX_EDIT_DISTANCE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2),

        beam_width: env::var("BEAM_WIDTH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10),

        lm_weight: env::var("LM_WEIGHT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0),

        use_grammar: env::var("USE_GRAMMAR")
            .ok()
            .map(|s| s == "true" || s == "1")
            .unwrap_or(true),

        dictionary_path: env::var("DICTIONARY_PATH")
            .unwrap_or_else(|_| "dictionary.txt".into()),
    }
}
```

## Serialization

### Saving and Loading Lattices

> **Illustrative.** A `lling_llang::io` module with `LatticeWriter` /
> `LatticeReader` is *not yet shipped*; this sketch documents the intended
> save/load surface (behind the `serde` feature), not a current API.

```rust,ignore
use lling_llang::io::{LatticeWriter, LatticeReader};

// Save lattice
let writer = LatticeWriter::new();
writer.write_json(&lattice, "lattice.json")?;
writer.write_binary(&lattice, "lattice.bin")?;

// Load lattice
let reader = LatticeReader::new();
let lattice: Lattice<TropicalWeight, HashMapBackend> =
    reader.read_json("lattice.json")?;
```

### Custom Serialization

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct SerializableLattice {
    nodes: Vec<SerializableNode>,
    edges: Vec<SerializableEdge>,
    vocabulary: Vec<String>,
}

impl<W: Semiring, B: LatticeBackend> From<&Lattice<W, B>> for SerializableLattice {
    fn from(lattice: &Lattice<W, B>) -> Self {
        // Convert to serializable form
        // ...
    }
}
```

## Parallel Processing

### Parallel N-best

```rust
use rayon::prelude::*;

fn parallel_nbest<W, B>(
    lattices: Vec<Lattice<W, B>>,
    n: usize,
) -> Vec<Vec<Path<W>>>
where
    W: Semiring + Send + Sync,
    B: LatticeBackend + Send + Sync,
{
    lattices.par_iter()
        .map(|mut lat| nbest(&mut lat, n))
        .collect()
}
```

### Thread-Safe Caching

```rust
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

pub struct ThreadSafeCorrector {
    inner: Arc<Corrector>,
    cache: Arc<RwLock<HashMap<String, CorrectionResult>>>,
}

impl ThreadSafeCorrector {
    pub fn correct(&self, text: &str) -> CorrectionResult {
        // Try read lock first
        if let Some(result) = self.cache.read().unwrap().get(text) {
            return result.clone();
        }

        // Compute
        let result = self.inner.correct(text);

        // Write to cache
        self.cache.write().unwrap().insert(text.to_string(), result.clone());

        result
    }
}

// Clone for use across threads
impl Clone for ThreadSafeCorrector {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            cache: Arc::clone(&self.cache),
        }
    }
}
```

## Testing

### Unit Testing Layers

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
        let mut builder = LatticeBuilder::new(HashMapBackend::new());
        builder.add_correction(0, 1, "the", TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction(0, 1, "teh", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "cat", TropicalWeight::one(), EdgeMetadata::default());
        builder.build(2)
    }

    #[test]
    fn test_spelling_layer() {
        let dict = DoubleArrayTrie::from_terms(vec!["the", "cat"]);
        let layer = SpellingLayer::new(dict, 2);

        let lattice = create_test_lattice();
        let result = layer.apply(&lattice).unwrap();

        // Original edges preserved
        assert!(result.contains_edge("the", 0, 1));

        // Misspelling has candidates
        assert!(result.edges_at_position(0).count() >= 2);
    }

    #[test]
    fn test_viterbi() {
        let mut lattice = create_test_lattice();
        let best = viterbi(&mut lattice);

        assert_eq!(best.labels, vec!["the", "cat"]);
    }
}
```

### Integration Testing

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_correction_pipeline() {
        let corrector = Corrector::from_config(CorrectorConfig::default())
            .expect("Failed to create corrector");

        let result = corrector.correct("Teh quikc brown fox");

        assert_eq!(result.corrected, "The quick brown fox");
        assert!(result.confidence > 0.5);
    }

    #[test]
    fn test_preserves_punctuation() {
        let corrector = Corrector::from_config(CorrectorConfig::default())
            .expect("Failed to create corrector");

        let result = corrector.correct("Hello, wrold!");

        assert_eq!(result.corrected, "Hello, world!");
    }
}
```

### Property Testing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn viterbi_finds_path(
        words in prop::collection::vec("[a-z]+", 1..10),
    ) {
        let mut builder = LatticeBuilder::new(HashMapBackend::new());

        for (i, word) in words.iter().enumerate() {
            builder.add_correction(
                i, i + 1,
                word,
                TropicalWeight::one(),
                EdgeMetadata::default(),
            );
        }

        let mut lattice = builder.build(words.len());
        let best = viterbi(&mut lattice);

        // Viterbi always finds a path if lattice is non-empty
        prop_assert!(!best.labels.is_empty());
        prop_assert_eq!(best.labels.len(), words.len());
    }
}
```

## Benchmarking

### Criterion Benchmarks

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_viterbi(c: &mut Criterion) {
    let mut group = c.benchmark_group("viterbi");

    for size in [10, 100, 1000].iter() {
        let lattice = create_lattice_with_size(*size);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, _| {
                b.iter(|| {
                    let mut lat = lattice.clone();
                    viterbi(&mut lat)
                });
            },
        );
    }

    group.finish();
}

fn bench_nbest(c: &mut Criterion) {
    let mut group = c.benchmark_group("nbest");

    for n in [1, 10, 100].iter() {
        let lattice = create_test_lattice();

        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            n,
            |b, &n| {
                b.iter(|| {
                    let mut lat = lattice.clone();
                    nbest(&mut lat, n)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_viterbi, bench_nbest);
criterion_main!(benches);
```

## Logging and Metrics

### Structured Logging

```rust
use tracing::{info, debug, warn, instrument};

#[instrument(skip(lattice))]
fn process_lattice<W: Semiring, B: LatticeBackend>(
    lattice: &Lattice<W, B>,
) -> Result<Path<W>, Error> {
    debug!(
        nodes = lattice.num_nodes(),
        edges = lattice.num_edges(),
        "Processing lattice"
    );

    let result = viterbi(&mut lattice.clone());

    info!(
        path_length = result.labels.len(),
        weight = ?result.weight,
        "Found best path"
    );

    Ok(result)
}
```

### Metrics Collection

```rust
use metrics::{counter, histogram, gauge};
use std::time::Instant;

pub struct InstrumentedCorrector {
    inner: Corrector,
}

impl InstrumentedCorrector {
    pub fn correct(&self, text: &str) -> CorrectionResult {
        counter!("corrections_total").increment(1);
        gauge!("input_length").set(text.len() as f64);

        let start = Instant::now();
        let result = self.inner.correct(text);
        let elapsed = start.elapsed();

        histogram!("correction_duration_seconds").record(elapsed.as_secs_f64());

        if result.changes.is_empty() {
            counter!("corrections_no_changes").increment(1);
        } else {
            counter!("corrections_with_changes").increment(1);
            histogram!("changes_per_correction").record(result.changes.len() as f64);
        }

        result
    }
}
```

## Related Topics

- [Speech/NLP](speech-nlp.md): Speech recognition integration
- [Text Correction](text-correction.md): Grammar and spelling
- [Architecture](../../architecture/overview.md): System architecture
- [API Reference](../../api/lattice-reference.md): Detailed API docs
