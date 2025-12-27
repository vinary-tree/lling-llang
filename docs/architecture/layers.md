# Correction Layers

The correction layer system provides an extensible architecture for building text normalization pipelines. Each layer receives a lattice and returns a (typically smaller) lattice with paths filtered or reweighted.

## Concepts

### What is a Correction Layer?

A **correction layer** is a transformation stage in a text processing pipeline. Think of it like a filter in a photo editing app - each layer applies a specific transformation, and you can stack multiple layers to achieve complex effects.

```
Input Lattice
     │
     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Correction Layer Stack                               │
├─────────────────────────────────────────────────────────────────────────────┤
│  Layer N: Custom Layer             ← Your application-specific logic        │
│     ↑                                                                       │
│  Layer 3: Language Model           ← N-gram probability reweighting         │
│     ↑                                                                       │
│  Layer 2: CFG Grammar              ← Syntactic filtering                    │
│     ↑                                                                       │
│  Layer 1: Lexical Correction       ← Levenshtein + phonetic candidates      │
│     ↑                                                                       │
│  [Input Lattice]                                                            │
└─────────────────────────────────────────────────────────────────────────────┘
     │
     ▼
Output Lattice (filtered/reweighted)
```

Each layer can:
- **Filter**: Remove paths that violate constraints (grammar rules, semantic types)
- **Reweight**: Adjust weights based on external scores (language model, confidence)
- **Transform**: Modify the lattice structure (merge paths, add alternatives)

### Why Layers?

The layer architecture provides several benefits:

1. **Modularity**: Each concern is handled by a dedicated layer
2. **Composability**: Mix and match layers for different applications
3. **Testability**: Test each layer in isolation
4. **Extensibility**: Add new layers without modifying existing code
5. **Diagnostics**: Track statistics per layer for debugging

### Core Types

| Type | Description |
|------|-------------|
| `CorrectionLayer<W, B>` | Trait for implementing layers |
| `LayerPipeline<W, B>` | Sequence of layers applied in order |
| `LayerPipelineBuilder<W, B>` | Fluent API for constructing pipelines |
| `LayerResult<T>` | Result type (`Result<T, LayerError>`) |
| `LayerStats` | Statistics from applying a layer |
| `LayerError` | Error types for layer operations |

## The CorrectionLayer Trait

```rust
pub trait CorrectionLayer<W: Semiring, B: LatticeBackend>: Send + Sync {
    /// Human-readable layer name for diagnostics.
    fn name(&self) -> &str;

    /// Apply this layer's corrections/filtering to the input lattice.
    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>>;

    /// Pre-check if this layer can process the given lattice.
    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool { true }

    /// Estimated reduction factor (e.g., 0.1 = reduces to 10% of paths).
    fn estimated_reduction(&self) -> f64 { 1.0 }

    /// Apply and return statistics about the operation.
    fn apply_with_stats(&self, lattice: &Lattice<W, B>)
        -> LayerResult<(Lattice<W, B>, LayerStats)>;
}
```

### Method Semantics

| Method | Purpose |
|--------|---------|
| `name()` | Identifier for logging and diagnostics |
| `apply()` | Core transformation logic |
| `can_apply()` | Pre-flight check for preconditions |
| `estimated_reduction()` | Hint for pipeline optimization |
| `apply_with_stats()` | Transformation with timing/metrics |

### Error Types

```rust
pub enum LayerError {
    /// The layer cannot process this lattice.
    CannotApply(String),
    /// Parse error during CFG filtering.
    ParseError(String),
    /// Configuration error.
    ConfigError(String),
    /// Resource error (e.g., model not loaded).
    ResourceError(String),
    /// Generic error with message.
    Other(String),
}
```

## Layer Pipeline

The `LayerPipeline` applies layers sequentially:

```rust
use lling_llang::layers::{LayerPipeline, LayerPipelineBuilder};

// Using the builder pattern
let pipeline = LayerPipelineBuilder::new()
    .add_layer(SpellingLayer::new())
    .add_layer(CfgFilterLayer::new(&grammar))
    .add_layer(LmRerankLayer::new(&model))
    .build();

// Apply to a lattice
let result = pipeline.apply(&lattice)?;

// Or with statistics
let (result, stats) = pipeline.apply_with_stats(&lattice)?;
for (i, stat) in stats.iter().enumerate() {
    println!("Layer {}: {} edges → {} edges in {}μs",
        pipeline.layer_names()[i],
        stat.input_edges,
        stat.output_edges,
        stat.time_us);
}
```

### Pipeline Operations

```rust
impl<W: Semiring, B: LatticeBackend> LayerPipeline<W, B> {
    /// Create a new empty pipeline.
    pub fn new() -> Self;

    /// Add a layer to the end of the pipeline.
    pub fn add_layer<L: CorrectionLayer<W, B> + 'static>(&mut self, layer: L);

    /// Get the number of layers.
    pub fn len(&self) -> usize;

    /// Check if empty.
    pub fn is_empty(&self) -> bool;

    /// Get layer names for diagnostics.
    pub fn layer_names(&self) -> Vec<&str>;

    /// Apply all layers in sequence.
    pub fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>>;

    /// Apply with statistics collection.
    pub fn apply_with_stats(&self, lattice: &Lattice<W, B>)
        -> LayerResult<(Lattice<W, B>, Vec<LayerStats>)>;

    /// Get estimated total reduction factor.
    pub fn estimated_reduction(&self) -> f64;
}
```

### Layer Statistics

```rust
pub struct LayerStats {
    /// Number of input paths (or estimate).
    pub input_paths: usize,
    /// Number of output paths (or estimate).
    pub output_paths: usize,
    /// Number of edges in input lattice.
    pub input_edges: usize,
    /// Number of edges in output lattice.
    pub output_edges: usize,
    /// Time taken in microseconds.
    pub time_us: u64,
}

impl LayerStats {
    /// Calculate reduction ratio (0.0 = no paths remain, 1.0 = all paths remain).
    pub fn reduction_ratio(&self) -> f64;
}
```

## Built-in Layers

### CfgFilterLayer

Filters paths that don't parse according to a context-free grammar:

```rust
use lling_llang::layers::CfgFilterLayer;
use lling_llang::cfg::GrammarBuilder;

// Define a grammar
let grammar = GrammarBuilder::new()
    .start("S")
    .rule("S", &["NP", "VP"])
    .rule("NP", &["Det", "N"])
    .rule("VP", &["V", "NP"])
    .rule("VP", &["V"])
    .rule("Det", &["the", "a"])
    .rule("N", &["dog", "cat"])
    .rule("V", &["saw", "chased"])
    .build()?;

// Create the layer
let layer = CfgFilterLayer::new(&grammar);

// Apply to a lattice
let filtered = layer.apply(&lattice)?;
```

The layer uses an Earley parser to find all valid derivations and removes edges that don't participate in any parse.

**Configuration options**:

```rust
// With pruning (default: true)
let layer = CfgFilterLayer::new(&grammar).with_pruning(true);

// Without pruning (keep edges but don't remove them)
let layer = CfgFilterLayer::new(&grammar).with_pruning(false);
```

**Estimated reduction**: 0.1 (typically reduces to ~10% of paths)

### Feature-Gated Layers

Additional layers are available with feature flags:

| Layer | Feature | Description |
|-------|---------|-------------|
| `PosTaggingLayer` | `pos-tagging` | POS-based filtering |
| `LanguageModelLayer` | `lm-rerank` | N-gram probability reranking |
| `MeTTaILTypeLayer` | `f1r3fly` | MeTTaIL semantic type filtering |

Enable in `Cargo.toml`:

```toml
[dependencies]
lling-llang = { version = "0.1", features = ["pos-tagging", "lm-rerank"] }
```

## Implementing Custom Layers

### Basic Layer

```rust
use lling_llang::layers::{CorrectionLayer, LayerError, LayerResult};
use lling_llang::lattice::Lattice;
use lling_llang::semiring::Semiring;
use lling_llang::backend::LatticeBackend;

/// A layer that filters paths based on a custom predicate.
struct MyFilterLayer {
    threshold: f64,
}

impl MyFilterLayer {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }
}

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for MyFilterLayer {
    fn name(&self) -> &str {
        "my-filter"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Your filtering/transformation logic here
        // ...

        Ok(lattice.clone())
    }

    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool {
        // Pre-check: can we process this lattice?
        lattice.num_edges() > 0
    }

    fn estimated_reduction(&self) -> f64 {
        // Hint: we typically keep about 30% of paths
        0.3
    }
}
```

### Reweighting Layer

A layer that adjusts weights without removing edges:

```rust
use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};

struct ConfidenceBoostLayer {
    boost_factor: f64,
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for ConfidenceBoostLayer {
    fn name(&self) -> &str { "confidence-boost" }

    fn apply(&self, lattice: &Lattice<TropicalWeight, B>) -> LayerResult<Lattice<TropicalWeight, B>> {
        let mut builder = LatticeBuilder::new(lattice.backend().clone());

        for edge in lattice.edges() {
            // Boost weight based on metadata
            let new_weight = if edge.metadata.is_original() {
                // Original tokens get a boost (lower tropical weight = better)
                TropicalWeight::new(edge.weight.value() - self.boost_factor)
            } else {
                edge.weight
            };

            builder.add_correction_by_id(
                edge.source.0 as usize,
                edge.target.0 as usize,
                edge.label,
                new_weight,
                edge.metadata.clone(),
            );
        }

        Ok(builder.build(lattice.end().0 as usize))
    }

    fn estimated_reduction(&self) -> f64 {
        1.0  // No reduction, just reweighting
    }
}
```

### Implementation Guidelines

1. **Stateless or Interior Mutability**: Layers implement `Send + Sync`, so use `RwLock` for caching
2. **Don't Modify Input**: The `apply` method should not modify the input lattice
3. **Handle Empty Lattices**: Check `lattice.is_empty()` and return early
4. **Accurate Reduction Estimates**: Help the pipeline optimizer with realistic estimates
5. **Informative Errors**: Use appropriate `LayerError` variants

## Details

### Thread Safety

All layers must implement `Send + Sync`:

```rust
pub trait CorrectionLayer<W: Semiring, B: LatticeBackend>: Send + Sync { ... }
```

For caching within layers, use interior mutability:

```rust
use std::sync::RwLock;

struct CachingLayer {
    cache: RwLock<HashMap<String, Vec<Edge>>>,
}

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for CachingLayer {
    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        let mut cache = self.cache.write().unwrap();
        // Use cache...
        todo!()
    }
}
```

### Pipeline Execution Order

Layers are applied strictly in order:

```
input → layer[0].apply() → layer[1].apply() → ... → output
```

If any layer fails:
1. `can_apply()` returns `false`: `LayerError::CannotApply`
2. `apply()` returns `Err`: Error propagates up

### Performance Considerations

**Layer ordering matters**:

1. Put high-reduction layers first to minimize data for later layers
2. Put cheap layers before expensive ones when reduction is similar

```rust
// Good: CFG (0.1) first, then LM (0.5)
pipeline.add_layer(cfg_layer);      // 90% reduction
pipeline.add_layer(lm_layer);       // 50% reduction on remaining 10%

// Less efficient: LM (0.5) first, then CFG (0.1)
pipeline.add_layer(lm_layer);       // 50% reduction
pipeline.add_layer(cfg_layer);      // 90% reduction on remaining 50%
```

**Use `estimated_reduction()` for optimization**:

```rust
let total_reduction = pipeline.estimated_reduction();
// Product of individual reductions: 0.1 * 0.5 = 0.05
```

### Error Handling Best Practices

```rust
fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
    // Use specific error types
    if !self.model_loaded {
        return Err(LayerError::ResourceError(
            "language model not loaded".to_string()
        ));
    }

    // Wrap parsing errors
    let parsed = parser.parse(lattice).map_err(|e| {
        LayerError::ParseError(format!("failed to parse: {}", e))
    })?;

    // Use Other for unexpected errors
    something_unusual().map_err(|e| {
        LayerError::Other(e.to_string())
    })?;

    Ok(result)
}
```

## Common Patterns

### Conditional Layer Application

Apply different layers based on input characteristics:

```rust
fn build_pipeline(input: &InputContext) -> LayerPipeline<TropicalWeight, HashMapBackend> {
    let mut builder = LayerPipelineBuilder::new();

    // Always apply spelling correction
    builder = builder.add_layer(SpellingLayer::new());

    // Only apply grammar for formal text
    if input.is_formal {
        builder = builder.add_layer(CfgFilterLayer::new(&grammar));
    }

    // Only apply LM for long sequences
    if input.length > 5 {
        builder = builder.add_layer(LmRerankLayer::new(&model));
    }

    builder.build()
}
```

### Layer Composition

Create composite layers from simpler ones:

```rust
struct CompositeLayer {
    inner: Vec<Box<dyn CorrectionLayer<TropicalWeight, HashMapBackend>>>,
}

impl CorrectionLayer<TropicalWeight, HashMapBackend> for CompositeLayer {
    fn name(&self) -> &str { "composite" }

    fn apply(&self, lattice: &Lattice<TropicalWeight, HashMapBackend>)
        -> LayerResult<Lattice<TropicalWeight, HashMapBackend>>
    {
        let mut current = lattice.clone();
        for layer in &self.inner {
            current = layer.apply(&current)?;
        }
        Ok(current)
    }

    fn estimated_reduction(&self) -> f64 {
        self.inner.iter()
            .map(|l| l.estimated_reduction())
            .product()
    }
}
```

### Statistics Aggregation

Aggregate statistics across pipeline runs:

```rust
struct PipelineMetrics {
    runs: usize,
    total_time_us: u64,
    total_input_edges: usize,
    total_output_edges: usize,
}

impl PipelineMetrics {
    fn record(&mut self, stats: &[LayerStats]) {
        self.runs += 1;
        for stat in stats {
            self.total_time_us += stat.time_us;
            self.total_input_edges += stat.input_edges;
            self.total_output_edges += stat.output_edges;
        }
    }

    fn average_reduction(&self) -> f64 {
        if self.total_input_edges == 0 {
            1.0
        } else {
            self.total_output_edges as f64 / self.total_input_edges as f64
        }
    }
}
```

## Next Steps

- [Path Extraction](../algorithms/path-extraction.md): Find optimal paths through filtered lattices
- [Composition](../algorithms/composition.md): Lazy lattice-grammar composition
- [F1R3FLY.io Integration](../integration/f1r3fly/vision.md): MeTTaIL and other F1R3FLY.io layers
- [API Reference](../api/layer-reference.md): Complete API documentation
