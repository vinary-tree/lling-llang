# Path Extraction API Reference

Complete API reference for path extraction algorithms.

## Path

Represents a path through the lattice.

```rust
pub struct Path<W: Semiring> {
    /// Sequence of labels along the path
    pub labels: Vec<String>,

    /// Total path weight
    pub weight: W,

    /// Edge IDs along the path
    pub edges: Vec<EdgeId>,

    /// Node IDs along the path
    pub nodes: Vec<NodeId>,
}

impl<W: Semiring> Path<W> {
    /// Create empty path
    pub fn empty() -> Self;

    /// Create with components
    pub fn new(labels: Vec<String>, weight: W, edges: Vec<EdgeId>, nodes: Vec<NodeId>) -> Self;

    /// Check if path is empty
    pub fn is_empty(&self) -> bool;

    /// Get path length (number of edges)
    pub fn len(&self) -> usize;

    /// Convert to string (space-separated labels)
    pub fn to_string(&self) -> String;

    /// Convert to string with custom separator
    pub fn join(&self, separator: &str) -> String;

    /// Get edge at position
    pub fn edge_at(&self, pos: usize) -> Option<EdgeId>;

    /// Get label at position
    pub fn label_at(&self, pos: usize) -> Option<&str>;
}
```

## Viterbi

Find the single best path.

```rust
/// Find the best (lowest weight) path through the lattice
pub fn viterbi<W, B>(lattice: &mut Lattice<W, B>) -> Path<W>
where
    W: Semiring,
    B: LatticeBackend;

/// Find best path with custom start/end
pub fn viterbi_between<W, B>(
    lattice: &mut Lattice<W, B>,
    start: NodeId,
    end: NodeId,
) -> Option<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;
```

### Usage

```rust
use lling_llang::path::viterbi;

let mut lattice = build_lattice();
let best = viterbi(&mut lattice);

println!("Best path: {}", best.to_string());
println!("Weight: {:?}", best.weight);
```

### Complexity

- Time: $`O(V + E)`$ after topological sort
- Space: $`O(V)`$ for backpointers

## N-best

Find the top-k best paths.

```rust
/// Find the n best paths through the lattice
pub fn nbest<W, B>(lattice: &mut Lattice<W, B>, n: usize) -> Vec<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;

/// Find n best paths with custom start/end
pub fn nbest_between<W, B>(
    lattice: &mut Lattice<W, B>,
    start: NodeId,
    end: NodeId,
    n: usize,
) -> Vec<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;
```

### Usage

```rust
use lling_llang::path::nbest;

let mut lattice = build_lattice();
let top_10 = nbest(&mut lattice, 10);

for (rank, path) in top_10.iter().enumerate() {
    println!("{}. {} (weight: {:?})", rank + 1, path.to_string(), path.weight);
}
```

### Complexity

- Time: $`O((V + E) \log n)`$ using heap
- Space: $`O(nV)`$ for $`n`$ paths

## Beam Search

Approximate best paths with bounded memory.

```rust
/// Find paths using beam search
pub fn beam_search<W, B>(
    lattice: &mut Lattice<W, B>,
    beam_width: usize,
) -> Vec<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;

/// Beam search with custom parameters
pub fn beam_search_with_options<W, B>(
    lattice: &mut Lattice<W, B>,
    options: BeamSearchOptions,
) -> Vec<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;
```

### BeamSearchOptions

```rust
pub struct BeamSearchOptions {
    /// Maximum hypotheses to keep at each step
    pub beam_width: usize,

    /// Maximum weight difference from best
    pub beam_threshold: Option<f64>,

    /// Maximum path length
    pub max_length: Option<usize>,

    /// Early stopping on first complete path
    pub early_stop: bool,

    /// Diversity bonus for different prefixes
    pub diversity_bonus: f64,
}

impl Default for BeamSearchOptions {
    fn default() -> Self {
        Self {
            beam_width: 10,
            beam_threshold: None,
            max_length: None,
            early_stop: false,
            diversity_bonus: 0.0,
        }
    }
}

impl BeamSearchOptions {
    /// Create with beam width
    pub fn with_width(beam_width: usize) -> Self;

    /// Set beam threshold
    pub fn threshold(self, threshold: f64) -> Self;

    /// Set max length
    pub fn max_length(self, length: usize) -> Self;

    /// Enable early stopping
    pub fn early_stop(self) -> Self;

    /// Set diversity bonus
    pub fn diversity(self, bonus: f64) -> Self;
}
```

### Usage

```rust
use lling_llang::path::{beam_search, beam_search_with_options, BeamSearchOptions};

// Simple beam search
let paths = beam_search(&mut lattice, 10);

// With options
let options = BeamSearchOptions::with_width(20)
    .threshold(5.0)
    .max_length(50)
    .diversity(0.1);

let paths = beam_search_with_options(&mut lattice, options);
```

### Complexity

- Time: $`O(VB \log B)`$ where $`B`$ = beam width
- Space: $`O(B)`$ for active hypotheses

## Diverse N-best

Find diverse paths with minimum distance.

```rust
/// Find n diverse paths (minimum edit distance between paths)
pub fn diverse_nbest<W, B>(
    lattice: &mut Lattice<W, B>,
    n: usize,
    min_distance: usize,
) -> Vec<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;
```

### Usage

```rust
use lling_llang::path::diverse_nbest;

// Get 10 paths that differ by at least 2 edits
let diverse = diverse_nbest(&mut lattice, 10, 2);
```

## Random Sampling

Sample paths according to weight distribution.

```rust
/// Sample paths weighted by probability
pub fn sample_paths<W, B>(
    lattice: &mut Lattice<W, B>,
    n: usize,
    temperature: f64,
) -> Vec<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;

/// Sample with custom RNG
pub fn sample_paths_with_rng<W, B, R: Rng>(
    lattice: &mut Lattice<W, B>,
    n: usize,
    temperature: f64,
    rng: &mut R,
) -> Vec<Path<W>>
where
    W: Semiring,
    B: LatticeBackend;
```

### Usage

```rust
use lling_llang::path::sample_paths;

// Sample 100 paths, temperature controls randomness
let samples = sample_paths(&mut lattice, 100, 1.0);
```

## Enumeration

Enumerate all paths (use with caution).

```rust
/// Enumerate all paths (may be exponential)
pub fn enumerate_paths<W, B>(
    lattice: &Lattice<W, B>,
    limit: usize,
) -> Result<Vec<Path<W>>, EnumerationError>
where
    W: Semiring,
    B: LatticeBackend;

#[derive(Debug)]
pub enum EnumerationError {
    /// Path count exceeded limit
    LimitExceeded(usize),

    /// Lattice contains cycle
    CycleDetected,
}
```

### Usage

```rust
use lling_llang::path::enumerate_paths;

match enumerate_paths(&lattice, 1000) {
    Ok(paths) => println!("Found {} paths", paths.len()),
    Err(EnumerationError::LimitExceeded(n)) => {
        println!("More than {} paths exist", n);
    }
    Err(EnumerationError::CycleDetected) => {
        println!("Lattice contains a cycle");
    }
}
```

## Path Iterator

Lazy path iteration.

```rust
pub struct PathIterator<'a, W: Semiring, B: LatticeBackend> {
    // Internal state
}

impl<'a, W: Semiring, B: LatticeBackend> Iterator for PathIterator<'a, W, B> {
    type Item = Path<W>;

    fn next(&mut self) -> Option<Self::Item>;
}

/// Create lazy path iterator
pub fn path_iter<'a, W, B>(lattice: &'a mut Lattice<W, B>) -> PathIterator<'a, W, B>
where
    W: Semiring,
    B: LatticeBackend;
```

### Usage

```rust
use lling_llang::path::path_iter;

// Lazy iteration - only computes paths as needed
for path in path_iter(&mut lattice).take(100) {
    println!("{}", path.to_string());
}
```

## Consensus Decoding

Find consensus across multiple paths.

```rust
/// Compute consensus (minimum Bayes risk)
pub fn consensus_decode<W, B>(
    lattice: &mut Lattice<W, B>,
    n_samples: usize,
) -> Result<ConsensusResult, ConsensusError>
where
    W: Semiring,
    B: LatticeBackend;

pub struct ConsensusResult {
    /// Consensus word sequence
    pub words: Vec<ConsensusWord>,

    /// Overall confidence
    pub confidence: f64,
}

pub struct ConsensusWord {
    /// Word text
    pub text: String,

    /// Position confidence
    pub confidence: f64,

    /// Alternative candidates at this position
    pub alternatives: Vec<(String, f64)>,
}
```

### Usage

```rust
use lling_llang::path::consensus_decode;

let result = consensus_decode(&mut lattice, 100)?;

for word in result.words {
    if word.confidence < 0.9 {
        println!("{} (low confidence: {:.2})", word.text, word.confidence);
        println!("  alternatives: {:?}", word.alternatives);
    }
}
```

## Utility Functions

```rust
/// Count all paths (without enumeration)
pub fn count_paths<W, B>(lattice: &mut Lattice<W, B>) -> Option<usize>
where
    W: Semiring,
    B: LatticeBackend;

/// Get path statistics
pub fn path_stats<W, B>(lattice: &mut Lattice<W, B>) -> PathStats
where
    W: Semiring,
    B: LatticeBackend;

pub struct PathStats {
    pub count: Option<usize>,  // None if too many
    pub min_length: usize,
    pub max_length: usize,
    pub avg_length: f64,
}

/// Check if path is valid in lattice
pub fn is_valid_path<W, B>(lattice: &Lattice<W, B>, path: &Path<W>) -> bool
where
    W: Semiring,
    B: LatticeBackend;

/// Compute path weight (recompute from edges)
pub fn compute_path_weight<W, B>(lattice: &Lattice<W, B>, edges: &[EdgeId]) -> W
where
    W: Semiring,
    B: LatticeBackend;
```

## Algorithm Comparison

| Algorithm | Best For | Time | Space | Exact |
|-----------|----------|------|-------|-------|
| viterbi | Single best | $`O(V+E)`$ | $`O(V)`$ | Yes |
| nbest | Top-k | $`O((V+E)\log n)`$ | $`O(nV)`$ | Yes |
| beam_search | Large lattices | $`O(VB \log B)`$ | $`O(B)`$ | No |
| diverse_nbest | Variety | $`O(n^2 VE)`$ | $`O(nV)`$ | Yes |
| sample_paths | Exploration | $`O(nL)`$ | $`O(L)`$ | No |

Where $`V`$ = nodes, $`E`$ = edges, $`n`$ = results, $`B`$ = beam width, $`L`$ = path length.

## See Also

- [Path Extraction (Algorithms)](../algorithms/path-extraction.md): Conceptual overview
- [Lattice Reference](lattice-reference.md): Lattice data structure
- [Semiring Reference](semiring-reference.md): Weight operations
