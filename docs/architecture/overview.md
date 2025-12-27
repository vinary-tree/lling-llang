# Architecture Overview

This document provides a high-level view of lling-llang's architecture, explaining how components fit together and the design principles behind them.

## Concepts

### What is lling-llang?

lling-llang is a **Weighted Finite State Transducer (WFST)** framework designed for text correction and normalization. At its core, it represents the space of possible corrections as a **weighted directed acyclic graph (lattice)**, then uses efficient algorithms to find optimal paths through this space.

Think of it like a spell checker that:
1. Generates multiple candidate corrections for each word
2. Assigns weights (scores) to each candidate
3. Considers the full sentence context to pick the best overall sequence
4. Can apply multiple filtering layers (grammar, semantics, style)

### Core Design Principles

1. **Algebraic Foundation**: All weight computations use [semiring algebra](semirings.md), enabling consistent behavior across different optimization objectives (shortest path, highest probability, etc.)

2. **Pluggable Storage**: The [backend abstraction](backends.md) separates lattice logic from storage, enabling both in-memory and distributed implementations.

3. **Layered Processing**: [Correction layers](layers.md) can be composed into pipelines, each layer filtering or reweighting paths.

4. **Lazy Evaluation**: Composition operators expand on-demand, avoiding the exponential blowup of explicit intersection.

## Component Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              lling-llang                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐  │
│  │  Semiring   │    │   Lattice   │    │    WFST     │    │   Layers    │  │
│  │             │    │             │    │             │    │             │  │
│  │ - Tropical  │    │ - Nodes     │    │ - States    │    │ - Pipeline  │  │
│  │ - Log       │◄───│ - Edges     │◄───│ - Arcs      │◄───│ - CFG       │  │
│  │ - Boolean   │    │ - Weights   │    │ - Compose   │    │ - Custom    │  │
│  │ - Product   │    │ - Builder   │    │ - Lazy      │    │             │  │
│  └─────────────┘    └──────┬──────┘    └─────────────┘    └─────────────┘  │
│         ▲                  │                                                │
│         │                  ▼                                                │
│  ┌──────┴──────┐    ┌─────────────┐    ┌─────────────┐                     │
│  │  Algorithms │    │   Backend   │    │     CFG     │                     │
│  │             │    │             │    │             │                     │
│  │ - Viterbi   │    │ - HashMap   │    │ - Grammar   │                     │
│  │ - N-best    │    │ - PathMap   │    │ - Earley    │                     │
│  │ - Beam      │    │ - (Custom)  │    │ - Forest    │                     │
│  └─────────────┘    └─────────────┘    └─────────────┘                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Module Breakdown

| Module | Purpose | Key Types |
|--------|---------|-----------|
| `semiring` | Algebraic weight structures | `Semiring`, `TropicalWeight`, `LogWeight` |
| `lattice` | Weighted DAG for alternatives | `Lattice`, `LatticeBuilder`, `Node`, `Edge` |
| `wfst` | Finite state transducers | `Wfst`, `MutableWfst`, `VectorWfst` |
| `backend` | Storage abstraction | `LatticeBackend`, `HashMapBackend` |
| `path` | Path extraction algorithms | `viterbi`, `nbest`, `beam_search` |
| `composition` | Lazy composition operators | `LazyComposition`, `LazyCfgComposition` |
| `cfg` | Context-free grammar parsing | `Grammar`, `EarleyParser`, `ParseForest` |
| `layers` | Correction pipeline | `CorrectionLayer`, `LayerPipeline` |

## Data Flow

A typical correction workflow:

```
Input: "teh quik fox"
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  1. Tokenization + Candidate Generation                                     │
│                                                                             │
│     For each token, generate weighted alternatives:                         │
│     "teh" → { "the" (0.5), "teh" (0.0), "tea" (1.5) }                       │
│     "quik" → { "quick" (0.5), "quik" (0.0) }                                │
│     "fox" → { "fox" (0.0) }                                                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  2. Lattice Construction                                                     │
│                                                                             │
│            ┌───the(0.5)───┐                                                 │
│   start ──►│              ├───quick(0.5)───►fox(0.0)──►end                  │
│            ├───teh(0.0)───┤               ▲                                 │
│            └───tea(1.5)───┘───quik(0.0)───┘                                 │
│                                                                             │
│   Using LatticeBuilder to construct a weighted DAG                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  3. Layer Pipeline (Optional)                                                │
│                                                                             │
│     Layer 1: CFG Grammar Filter                                              │
│       - Removes paths that violate syntax rules                             │
│       - "tea quik fox" might be eliminated                                  │
│                                                                             │
│     Layer 2: Language Model Reranking                                        │
│       - Adjusts weights based on n-gram probabilities                       │
│       - "the quick fox" gets lower (better) weight                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  4. Path Extraction                                                          │
│                                                                             │
│     viterbi(&mut lattice) → Best path: "the quick fox" (1.0)                │
│                                                                             │
│     Or: nbest(&mut lattice, 3) → Top 3 paths                                │
│     Or: beam_search(&mut lattice, 10) → Approximate top paths               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         ▼
Output: "the quick fox"
```

## Details

### Weight Computation

Weights flow through the system following semiring algebra:

- **Sequential transitions**: Weights are **multiplied** (⊗)
  - Path "the" → "quick" has weight `0.5 ⊗ 0.5 = 0.5 + 0.5 = 1.0` (tropical semiring)

- **Parallel alternatives**: Weights are **added** (⊕)
  - If two paths reach the same node, we keep the **minimum** (tropical semiring)

This algebraic structure ensures that:
1. Path weights are computed consistently
2. Dynamic programming algorithms (Viterbi, forward-backward) work correctly
3. Different optimization objectives (shortest path, highest probability) use the same code with different semiring implementations

See [Semirings](semirings.md) for the full mathematical foundation.

### Lazy Composition

A naive approach to lattice-grammar intersection would:
1. Enumerate all lattice paths
2. Parse each path against the grammar
3. Keep valid paths

This is exponential in path length!

lling-llang uses **lazy composition** instead:

```
Lattice × Grammar → Lazy Composed Lattice
                         │
                         ├── Expands on-demand
                         ├── Caches computed states
                         └── Only explores reachable states
```

The composed lattice is never fully materialized. Instead:
1. States are computed lazily as needed
2. Caching policies (LRU, CacheAll, NoCache) control memory usage
3. Path extraction algorithms work directly on the lazy structure

See [Composition](../algorithms/composition.md) for details.

### Backend Abstraction

The `LatticeBackend` trait separates vocabulary storage from lattice logic:

```rust
pub trait LatticeBackend: Clone + Send + Sync {
    fn intern(&mut self, word: &str) -> VocabId;
    fn lookup(&self, id: VocabId) -> Option<&str>;
    fn vocab_size(&self) -> usize;
    // ...
}
```

This enables:
- **HashMapBackend**: In-memory, single-process
- **PathMapBackend**: Distributed, content-addressed (F1R3FLY.io)
- **Custom backends**: Your storage layer

See [Backends](backends.md) for implementation details.

### Layer Architecture

Correction layers implement a simple trait:

```rust
pub trait CorrectionLayer<W: Semiring, B: LatticeBackend> {
    fn name(&self) -> &str;
    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>>;
}
```

Layers can be composed into pipelines:

```rust
let pipeline = LayerPipelineBuilder::new()
    .add_layer(SpellingLayer::new())
    .add_layer(CfgFilterLayer::new(&grammar))
    .add_layer(LmRerankLayer::new(&model))
    .build();

let corrected = pipeline.apply(&lattice)?;
```

See [Layers](layers.md) for building custom layers.

## Performance Characteristics

| Operation | Time Complexity | Space Complexity |
|-----------|-----------------|------------------|
| Lattice construction | O(E) | O(V + E) |
| Topological sort | O(V + E) | O(V) |
| Viterbi | O(V + E) | O(V) |
| N-best extraction | O(k log k) | O(k × L) |
| Beam search | O(V × B × D) | O(B × L) |
| Lazy composition | Demand-driven | Depends on caching |

Where:
- V = nodes, E = edges, L = path length
- k = number of paths, B = beam width, D = average out-degree

## Next Steps

- [Semirings](semirings.md): Understand the algebraic foundation
- [Lattices](lattices.md): Learn lattice construction and operations
- [Path Extraction](../algorithms/path-extraction.md): Find optimal paths
- [Layers](layers.md): Build correction pipelines
