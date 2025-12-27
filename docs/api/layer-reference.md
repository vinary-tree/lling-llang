# Layer API Reference

Complete API reference for correction layers and pipelines.

## CorrectionLayer Trait

Base trait for all correction layers.

```rust
pub trait CorrectionLayer<W: Semiring, B: LatticeBackend>: Send + Sync {
    /// Human-readable layer name
    fn name(&self) -> &str;

    /// Apply the layer to a lattice
    fn apply(&self, lattice: &Lattice<W, B>) -> Result<Lattice<W, B>, LayerError>;

    /// Estimated reduction ratio (0.0 to 1.0)
    /// 1.0 = no reduction, 0.1 = reduces to 10%
    fn estimated_reduction(&self) -> f64 {
        1.0
    }

    /// Check if layer can be applied to this lattice
    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool {
        true
    }

    /// Layer metadata
    fn metadata(&self) -> LayerMetadata {
        LayerMetadata::default()
    }
}
```

## LayerError

Error type for layer operations.

```rust
#[derive(Debug, Clone)]
pub enum LayerError {
    /// Layer produced no valid paths
    NoValidPaths,

    /// Layer cannot be applied to this lattice
    NotApplicable(String),

    /// Internal layer error
    Internal(String),

    /// Configuration error
    Configuration(String),

    /// Timeout during processing
    Timeout,

    /// Resource exhaustion
    ResourceExhausted(String),
}

impl std::error::Error for LayerError {}

impl std::fmt::Display for LayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
}
```

## LayerMetadata

Layer metadata for introspection.

```rust
#[derive(Clone, Debug, Default)]
pub struct LayerMetadata {
    /// Layer version
    pub version: Option<String>,

    /// Layer description
    pub description: Option<String>,

    /// Whether layer is deterministic
    pub deterministic: bool,

    /// Whether layer modifies weights
    pub modifies_weights: bool,

    /// Whether layer adds edges
    pub adds_edges: bool,

    /// Whether layer removes edges
    pub removes_edges: bool,

    /// Custom properties
    pub properties: HashMap<String, String>,
}
```

## LayerPipeline

Composed sequence of layers.

```rust
pub struct LayerPipeline<W: Semiring, B: LatticeBackend> {
    layers: Vec<Box<dyn CorrectionLayer<W, B>>>,
}

impl<W: Semiring, B: LatticeBackend> LayerPipeline<W, B> {
    /// Create empty pipeline
    pub fn new() -> Self;

    /// Add a layer to the pipeline
    pub fn push(&mut self, layer: impl CorrectionLayer<W, B> + 'static);

    /// Get number of layers
    pub fn len(&self) -> usize;

    /// Check if pipeline is empty
    pub fn is_empty(&self) -> bool;

    /// Get layer by index
    pub fn get(&self, index: usize) -> Option<&dyn CorrectionLayer<W, B>>;

    /// Apply all layers in sequence
    pub fn apply(&self, lattice: &Lattice<W, B>) -> Result<Lattice<W, B>, PipelineError>;

    /// Apply with statistics
    pub fn apply_with_stats(&self, lattice: &Lattice<W, B>)
        -> Result<(Lattice<W, B>, PipelineStats), PipelineError>;

    /// Get layer names
    pub fn layer_names(&self) -> Vec<&str>;
}
```

## LayerPipelineBuilder

Builder for layer pipelines.

```rust
pub struct LayerPipelineBuilder<W: Semiring, B: LatticeBackend> {
    layers: Vec<Box<dyn CorrectionLayer<W, B>>>,
}

impl<W: Semiring, B: LatticeBackend> LayerPipelineBuilder<W, B> {
    /// Create new builder
    pub fn new() -> Self;

    /// Add a layer
    pub fn add_layer<L>(self, layer: L) -> Self
    where
        L: CorrectionLayer<W, B> + 'static;

    /// Add multiple layers
    pub fn add_layers<I, L>(self, layers: I) -> Self
    where
        I: IntoIterator<Item = L>,
        L: CorrectionLayer<W, B> + 'static;

    /// Build the pipeline
    pub fn build(self) -> LayerPipeline<W, B>;
}
```

### Usage

```rust
use lling_llang::layers::LayerPipelineBuilder;

let pipeline = LayerPipelineBuilder::new()
    .add_layer(SpellingLayer::new(dict, 2))
    .add_layer(GrammarLayer::new(grammar))
    .add_layer(LanguageModelLayer::new(lm))
    .build();

let result = pipeline.apply(&lattice)?;
```

## PipelineStats

Statistics from pipeline execution.

```rust
pub struct PipelineStats {
    /// Total execution time
    pub total_time: Duration,

    /// Per-layer statistics
    pub layer_stats: Vec<LayerStats>,

    /// Input lattice stats
    pub input: LatticeStats,

    /// Output lattice stats
    pub output: LatticeStats,
}

pub struct LayerStats {
    /// Layer name
    pub name: String,

    /// Execution time
    pub time: Duration,

    /// Edges before
    pub edges_before: usize,

    /// Edges after
    pub edges_after: usize,

    /// Reduction ratio
    pub reduction: f64,
}

pub struct LatticeStats {
    pub nodes: usize,
    pub edges: usize,
    pub path_count: Option<usize>,
}
```

## PipelineError

Pipeline-specific errors.

```rust
#[derive(Debug)]
pub enum PipelineError {
    /// Layer error at specific index
    Layer { index: usize, name: String, error: LayerError },

    /// Pipeline configuration error
    Configuration(String),

    /// Empty pipeline
    Empty,
}
```

## CfgFilterLayer

Context-free grammar filtering layer.

```rust
pub struct CfgFilterLayer<'g> {
    grammar: &'g Grammar,
    mode: FilterMode,
}

impl<'g> CfgFilterLayer<'g> {
    /// Create with grammar reference
    pub fn new(grammar: &'g Grammar) -> Self;

    /// Set filter mode
    pub fn with_mode(self, mode: FilterMode) -> Self;
}

pub enum FilterMode {
    /// Remove all invalid edges
    Strict,

    /// Keep invalid edges with penalty weight
    Soft { penalty: f64 },

    /// Only mark edges, don't modify
    MarkOnly,
}
```

### Usage

```rust
use lling_llang::layers::CfgFilterLayer;
use lling_llang::cfg::GrammarBuilder;

let grammar = GrammarBuilder::new()
    .start("S")
    .rule("S", &["NP", "VP"])
    // ...
    .build()?;

let layer = CfgFilterLayer::new(&grammar);
let filtered = layer.apply(&lattice)?;
```

## SpellingCorrectionLayer

Spelling correction using fuzzy matching.

```rust
pub struct SpellingCorrectionLayer<D: Dictionary> {
    transducer: Transducer<D>,
    max_distance: usize,
    weight_scale: f64,
}

impl<D: Dictionary> SpellingCorrectionLayer<D> {
    /// Create with dictionary and max edit distance
    pub fn new(dictionary: D, max_distance: usize) -> Self;

    /// Set weight scale factor
    pub fn with_weight_scale(self, scale: f64) -> Self;
}
```

### Usage

```rust
use lling_llang::layers::SpellingCorrectionLayer;
use liblevenshtein::dictionary::DoubleArrayTrie;

let dict = DoubleArrayTrie::from_terms(words);
let layer = SpellingCorrectionLayer::new(dict, 2);

let corrected = layer.apply(&lattice)?;
```

## LanguageModelLayer

Language model scoring layer.

```rust
pub struct LanguageModelLayer<LM: LanguageModel> {
    model: LM,
    weight: f64,
}

impl<LM: LanguageModel> LanguageModelLayer<LM> {
    /// Create with language model
    pub fn new(model: LM) -> Self;

    /// Set LM weight
    pub fn with_weight(self, weight: f64) -> Self;
}

pub trait LanguageModel: Send + Sync {
    /// Score a word given context
    fn score(&self, context: &[&str], word: &str) -> f64;

    /// Score a sequence
    fn score_sequence(&self, words: &[&str]) -> f64;

    /// Get vocabulary size
    fn vocab_size(&self) -> usize;
}
```

## ReweightLayer

Weight transformation layer.

```rust
pub struct ReweightLayer<F> {
    transform: F,
}

impl<F, W: Semiring> ReweightLayer<F>
where
    F: Fn(&W) -> W + Send + Sync,
{
    /// Create with transform function
    pub fn new(transform: F) -> Self;
}
```

### Usage

```rust
use lling_llang::layers::ReweightLayer;

// Scale all weights by 2
let layer = ReweightLayer::new(|w: &TropicalWeight| {
    TropicalWeight::new(w.value() * 2.0)
});
```

## PruneLayer

Lattice pruning layer.

```rust
pub struct PruneLayer {
    mode: PruneMode,
}

pub enum PruneMode {
    /// Keep paths within beam of best
    Beam(f64),

    /// Keep paths with posterior > threshold
    Posterior(f64),

    /// Keep top-k paths
    TopK(usize),

    /// Remove edges with weight > threshold
    WeightThreshold(f64),
}

impl PruneLayer {
    pub fn beam(threshold: f64) -> Self;
    pub fn posterior(threshold: f64) -> Self;
    pub fn top_k(k: usize) -> Self;
    pub fn weight_threshold(threshold: f64) -> Self;
}
```

## MeTTaILTypeLayer

MeTTaIL type filtering layer.

```rust
pub struct MeTTaILTypeLayer {
    checker: Box<dyn TypeChecker>,
    constraints: Vec<TypeConstraint>,
}

impl MeTTaILTypeLayer {
    /// Create with type checker
    pub fn new(checker: Box<dyn TypeChecker>) -> Self;

    /// Add type constraint
    pub fn with_constraint(self, constraint: TypeConstraint) -> Self;
}
```

### Usage

```rust
use lling_llang::layers::{MeTTaILTypeLayer, TypeConstraint, TypeExpr};

let layer = MeTTaILTypeLayer::new(Box::new(checker))
    .with_constraint(TypeConstraint::strict(TypeExpr::base("Noun")).at_position(0))
    .with_constraint(TypeConstraint::soft(TypeExpr::base("Verb")).at_position(1));
```

## Custom Layers

### Implementing CorrectionLayer

```rust
use lling_llang::layers::{CorrectionLayer, LayerError};

pub struct MyCustomLayer {
    // Configuration
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for MyCustomLayer {
    fn name(&self) -> &str {
        "my-custom-layer"
    }

    fn apply(&self, lattice: &Lattice<TropicalWeight, B>)
        -> Result<Lattice<TropicalWeight, B>, LayerError>
    {
        // Clone and modify
        let mut result = lattice.clone();

        for edge in result.edges_mut() {
            // Modify edges...
        }

        Ok(result)
    }

    fn estimated_reduction(&self) -> f64 {
        0.8  // Reduces to ~80% of edges
    }

    fn can_apply(&self, lattice: &Lattice<TropicalWeight, B>) -> bool {
        lattice.num_edges() > 0
    }
}
```

### Composing Layers

```rust
pub struct ComposedLayer<L1, L2> {
    first: L1,
    second: L2,
}

impl<W: Semiring, B: LatticeBackend, L1, L2> CorrectionLayer<W, B> for ComposedLayer<L1, L2>
where
    L1: CorrectionLayer<W, B>,
    L2: CorrectionLayer<W, B>,
{
    fn name(&self) -> &str {
        "composed"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> Result<Lattice<W, B>, LayerError> {
        let intermediate = self.first.apply(lattice)?;
        self.second.apply(&intermediate)
    }

    fn estimated_reduction(&self) -> f64 {
        self.first.estimated_reduction() * self.second.estimated_reduction()
    }
}
```

## Layer Utilities

```rust
/// Identity layer (no-op)
pub struct IdentityLayer;

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for IdentityLayer {
    fn name(&self) -> &str { "identity" }
    fn apply(&self, lattice: &Lattice<W, B>) -> Result<Lattice<W, B>, LayerError> {
        Ok(lattice.clone())
    }
}

/// Conditional layer application
pub fn conditional_apply<W, B, L>(
    layer: &L,
    lattice: &Lattice<W, B>,
    condition: impl Fn(&Lattice<W, B>) -> bool,
) -> Result<Lattice<W, B>, LayerError>
where
    W: Semiring,
    B: LatticeBackend,
    L: CorrectionLayer<W, B>,
{
    if condition(lattice) {
        layer.apply(lattice)
    } else {
        Ok(lattice.clone())
    }
}

/// Retry layer with fallback
pub fn with_fallback<W, B, L1, L2>(
    primary: &L1,
    fallback: &L2,
    lattice: &Lattice<W, B>,
) -> Result<Lattice<W, B>, LayerError>
where
    W: Semiring,
    B: LatticeBackend,
    L1: CorrectionLayer<W, B>,
    L2: CorrectionLayer<W, B>,
{
    match primary.apply(lattice) {
        Ok(result) if result.num_edges() > 0 => Ok(result),
        _ => fallback.apply(lattice),
    }
}
```

## See Also

- [Layers (Architecture)](../architecture/layers.md): Conceptual overview
- [Lattice Reference](lattice-reference.md): Lattice operations
- [Parsing](../algorithms/parsing.md): Grammar-based filtering
