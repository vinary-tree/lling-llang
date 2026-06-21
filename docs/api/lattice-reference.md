# Lattice API Reference

Complete API reference for lattice types and operations.

## Lattice

Core lattice structure.

```rust
pub struct Lattice<W: Semiring, B: LatticeBackend> {
    // Internal fields
}

impl<W: Semiring, B: LatticeBackend> Lattice<W, B> {
    /// Get number of nodes
    pub fn num_nodes(&self) -> usize;

    /// Get number of edges
    pub fn num_edges(&self) -> usize;

    /// Get start node ID
    pub fn start(&self) -> NodeId;

    /// Get end node ID
    pub fn end(&self) -> NodeId;

    /// Get backend reference
    pub fn backend(&self) -> &B;

    /// Get mutable backend reference
    pub fn backend_mut(&mut self) -> &mut B;

    /// Iterate over all nodes
    pub fn nodes(&self) -> impl Iterator<Item = &Node>;

    /// Iterate over all edges
    pub fn edges(&self) -> impl Iterator<Item = &Edge<W>>;

    /// Get mutable edge iterator
    pub fn edges_mut(&mut self) -> impl Iterator<Item = &mut Edge<W>>;

    /// Get outgoing edges from a node
    pub fn outgoing_edges(&self, node: NodeId) -> impl Iterator<Item = &Edge<W>>;

    /// Get incoming edges to a node
    pub fn incoming_edges(&self, node: NodeId) -> impl Iterator<Item = &Edge<W>>;

    /// Get edges at a specific position
    pub fn edges_at_position(&self, pos: usize) -> impl Iterator<Item = &Edge<W>>;

    /// Get topological order (computed and cached)
    pub fn topological_order(&mut self) -> Option<&[NodeId]>;

    /// Invalidate cached topological order
    pub fn invalidate_cache(&mut self);

    /// Clone with new backend
    pub fn with_backend<B2: LatticeBackend>(self, backend: B2) -> Lattice<W, B2>;
}
```

## NodeId

Node identifier.

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Create from index
    pub fn new(index: usize) -> Self;

    /// Get index
    pub fn index(&self) -> usize;
}
```

## EdgeId

Edge identifier.

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct EdgeId(pub u32);

impl EdgeId {
    /// Create from index
    pub fn new(index: usize) -> Self;

    /// Get index
    pub fn index(&self) -> usize;
}
```

## Node

Node in the lattice.

```rust
pub struct Node {
    /// Node identifier
    pub id: NodeId,

    /// Outgoing edge IDs
    pub outgoing: SmallVec<[EdgeId; 4]>,

    /// Incoming edge IDs
    pub incoming: SmallVec<[EdgeId; 4]>,
}

impl Node {
    /// Create new node
    pub fn new(id: NodeId) -> Self;

    /// Check if this is a leaf (no outgoing edges)
    pub fn is_leaf(&self) -> bool;

    /// Check if this is a root (no incoming edges)
    pub fn is_root(&self) -> bool;

    /// Get out-degree
    pub fn out_degree(&self) -> usize;

    /// Get in-degree
    pub fn in_degree(&self) -> usize;
}
```

## Edge

Weighted edge in the lattice.

```rust
pub struct Edge<W: Semiring> {
    /// Edge identifier
    pub id: EdgeId,

    /// Source node
    pub source: NodeId,

    /// Target node
    pub target: NodeId,

    /// Vocabulary ID for label lookup
    pub vocab_id: VocabId,

    /// Edge weight
    pub weight: W,

    /// Edge metadata
    pub metadata: EdgeMetadata,
}

impl<W: Semiring> Edge<W> {
    /// Create new edge
    pub fn new(
        id: EdgeId,
        source: NodeId,
        target: NodeId,
        vocab_id: VocabId,
        weight: W,
        metadata: EdgeMetadata,
    ) -> Self;

    /// Get label using backend
    pub fn label<'b, B: LatticeBackend>(&self, backend: &'b B) -> Option<&'b str>;

    /// Get weight reference
    pub fn weight(&self) -> &W;

    /// Set weight
    pub fn set_weight(&mut self, weight: W);

    /// Get metadata reference
    pub fn metadata(&self) -> &EdgeMetadata;

    /// Get mutable metadata reference
    pub fn metadata_mut(&mut self) -> &mut EdgeMetadata;
}
```

## EdgeMetadata

Edge metadata for provenance tracking.

```rust
#[derive(Clone, Debug, Default)]
pub struct EdgeMetadata {
    /// Source layer that created this edge
    pub source: Option<String>,

    /// Original form (before correction)
    pub original: Option<String>,

    /// Correction type
    pub correction_type: Option<CorrectionType>,

    /// Edit distance (for spelling corrections)
    pub edit_distance: Option<usize>,

    /// Confidence score
    pub confidence: Option<f64>,

    /// Timing information (for ASR)
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,

    /// Custom properties
    pub properties: HashMap<String, String>,
}

impl EdgeMetadata {
    /// Create empty metadata
    pub fn new() -> Self;

    /// Create for original (uncorrected) token
    pub fn original() -> Self;

    /// Create for spelling correction
    pub fn spelling_correction(original: &str, corrected: &str) -> Self;

    /// Create for edit correction with distance
    pub fn edit_correction(original: &str, corrected: &str, distance: usize) -> Self;

    /// Create for phonetic correction
    pub fn phonetic_correction(original: &str, corrected: &str) -> Self;

    /// Builder: set source
    pub fn with_source(self, source: impl Into<String>) -> Self;

    /// Builder: set timing
    pub fn with_timing(self, start: f64, end: f64) -> Self;

    /// Builder: set confidence
    pub fn with_confidence(self, confidence: f64) -> Self;

    /// Builder: set property
    pub fn with_property(self, key: impl Into<String>, value: impl Into<String>) -> Self;
}
```

## CorrectionType

Type of correction applied.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorrectionType {
    /// Original token (no correction)
    Original,

    /// Spelling correction via edit distance
    Spelling,

    /// Phonetic similarity correction
    Phonetic,

    /// Grammar-based correction
    Grammar,

    /// Semantic correction
    Semantic,

    /// OCR error correction
    Ocr,

    /// ASR (speech) correction
    Asr,

    /// Custom correction type
    Custom(String),
}
```

## LatticeBuilder

Builder for constructing lattices.

```rust
pub struct LatticeBuilder<W: Semiring, B: LatticeBackend> {
    backend: B,
    nodes: Vec<Node>,
    edges: Vec<Edge<W>>,
}

impl<W: Semiring, B: LatticeBackend> LatticeBuilder<W, B> {
    /// Create new builder with backend
    pub fn new(backend: B) -> Self;

    /// Add a node, return its ID
    pub fn add_node(&mut self) -> NodeId;

    /// Add an edge between nodes
    pub fn add_edge(&mut self, edge: Edge<W>) -> EdgeId;

    /// Add a correction edge (convenience method)
    pub fn add_correction(
        &mut self,
        source: usize,
        target: usize,
        label: &str,
        weight: W,
        metadata: EdgeMetadata,
    ) -> EdgeId;

    /// Add a token (original text, no correction)
    pub fn add_token(
        &mut self,
        source: usize,
        target: usize,
        label: &str,
        weight: W,
    ) -> EdgeId;

    /// Ensure node exists, creating if necessary
    pub fn ensure_node(&mut self, id: usize) -> NodeId;

    /// Build the lattice
    pub fn build(self, num_positions: usize) -> Lattice<W, B>;

    /// Build partial lattice (for streaming)
    pub fn build_partial(self, up_to_position: usize) -> Lattice<W, B>;
}
```

### Usage

```rust
use lling_llang::lattice::LatticeBuilder;
use lling_llang::backend::HashMapBackend;
use lling_llang::semiring::TropicalWeight;

let mut builder = LatticeBuilder::new(HashMapBackend::new());

// Add original tokens
builder.add_token(0, 1, "the", TropicalWeight::one());
builder.add_token(1, 2, "cat", TropicalWeight::one());

// Add spelling alternatives
builder.add_correction(
    1, 2,
    "car",
    TropicalWeight::new(1.0),
    EdgeMetadata::spelling_correction("cat", "car"),
);

let lattice = builder.build(2);
```

## VocabId

Vocabulary identifier for label lookup.

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VocabId(pub u32);

impl VocabId {
    /// Create from index
    pub fn new(index: usize) -> Self;

    /// Get index
    pub fn index(&self) -> usize;
}
```

## Lattice Operations

### Clone

```rust
impl<W: Semiring + Clone, B: LatticeBackend + Clone> Clone for Lattice<W, B> {
    fn clone(&self) -> Self;
}
```

### Path Counting

```rust
use lling_llang::lattice::algorithms::count_paths;

if let Some(count) = count_paths(&mut lattice) {
    println!("Lattice contains {} paths", count);
}
```

### Reachability

```rust
use lling_llang::lattice::algorithms::{reachable_nodes, path_exists};

// All nodes reachable from start
let reachable = reachable_nodes(&lattice, lattice.start());

// Check path existence
if path_exists(&lattice, lattice.start(), lattice.end()) {
    println!("Path from start to end exists");
}
```

### Pruning

The shipped pruning primitive is the path-side `beam_search` in
`lling_llang::path` (re-exported from the prelude). A dedicated
`lling_llang::lattice::prune` module with standalone `beam_prune` /
`posterior_prune` lattice→lattice helpers is *illustrative / not yet shipped*;
the sketch below shows the intended shape:

```rust,ignore
// Illustrative API sketch — not yet provided by the crate.
// The shipped primitive is `lling_llang::path::{beam_search, BeamSearchConfig}`.
use lling_llang::lattice::prune::{beam_prune, posterior_prune};

// Beam pruning
let pruned = beam_prune(&lattice, 10.0)?;

// Posterior pruning
let pruned = posterior_prune(&lattice, 0.01)?;
```

## Serialization

> **Illustrative.** A `lling_llang::io` module with `LatticeWriter` /
> `LatticeReader` is *not yet shipped*; the sketch below documents the intended
> serialization surface (gated behind the `serde` feature) rather than a current
> API.

```rust,ignore
use lling_llang::io::{LatticeWriter, LatticeReader};

// Write
let writer = LatticeWriter::new();
writer.write_json(&lattice, "lattice.json")?;

// Read
let reader = LatticeReader::new();
let lattice: Lattice<TropicalWeight, HashMapBackend> =
    reader.read_json("lattice.json")?;
```

## See Also

- [Lattices (Architecture)](../architecture/lattices.md): Conceptual overview
- [Backend Reference](backend-reference.md): Storage backends
- [Path Reference](path-reference.md): Path extraction
