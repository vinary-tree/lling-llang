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
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                 lling-llang                                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐    │
│  │   Semiring    │  │    Lattice    │  │     WFST      │  │    Layers     │    │
│  │               │  │               │  │               │  │               │    │
│  │ - Tropical    │  │ - Nodes       │  │ - States      │  │ - Pipeline    │    │
│  │ - Log         │◄─│ - Edges       │◄─│ - Arcs        │◄─│ - CFG         │    │
│  │ - Probability │  │ - Weights     │  │ - Compose     │  │ - Custom      │    │
│  │ - String      │  │ - Builder     │  │ - Lazy        │  │               │    │
│  │ - Expectation │  │               │  │ - Rational    │  │               │    │
│  │ - Product     │  │               │  │ - Synchronize │  │               │    │
│  └───────────────┘  └───────┬───────┘  └───────────────┘  └───────────────┘    │
│         ▲                   │                                                    │
│         │                   ▼                                                    │
│  ┌──────┴──────┐  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐      │
│  │  Algorithms │  │    Backend    │  │      CFG      │  │      CTC      │      │
│  │             │  │               │  │               │  │               │      │
│  │ - Viterbi   │  │ - HashMap     │  │ - Grammar     │  │ - Correct     │      │
│  │ - N-best    │  │ - PathMap     │  │ - Earley      │  │ - Compact     │      │
│  │ - Beam      │  │ - (Custom)    │  │ - Forest      │  │ - Minimal     │      │
│  │ - ShortDist │  │               │  │               │  │ - Selfless    │      │
│  │ - WtPush    │  │               │  │               │  │               │      │
│  │ - EpsRemove │  │               │  │               │  │               │      │
│  │ - Determin  │  │               │  │               │  │               │      │
│  │ - Minimize  │  │               │  │               │  │               │      │
│  └─────────────┘  └───────────────┘  └───────────────┘  └───────────────┘      │
│                                                                                  │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐    │
│  │Differentiable │  │  Optimization │  │      ASR      │  │      GPU      │    │
│  │               │  │               │  │               │  │               │    │
│  │ - ForwardScr  │  │ - LogPush     │  │ - Context     │  │ - CSR         │    │
│  │ - Viterbi     │  │ - Lookahead   │  │ - N-gram LM   │  │ - TokenPack   │    │
│  │ - Gradients   │  │ - TokenGroup  │  │ - Cascade     │  │ - LoadBalance │    │
│  │ - Layers      │  │ - N-gramBO    │  │ - Factoring   │  │ - K-Vector    │    │
│  │ - SecondOrder │  │               │  │ - Rescoring   │  │ - Channels    │    │
│  │               │  │               │  │               │  │ - SoftPrune   │    │
│  └───────────────┘  └───────────────┘  └───────────────┘  └───────────────┘    │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Module Breakdown

| Module | Purpose | Key Types |
|--------|---------|-----------|
| `semiring` | Algebraic weight structures | `Semiring`, `TropicalWeight`, `LogWeight`, `ProbabilityWeight`, `StringWeight`, `ExpectationWeight` |
| `lattice` | Weighted DAG for alternatives | `Lattice`, `LatticeBuilder`, `Node`, `Edge` |
| `wfst` | Finite state transducers | `Wfst`, `MutableWfst`, `VectorWfst`, `UnionWfst`, `ConcatWfst`, `SyncWfst` |
| `backend` | Storage abstraction | `LatticeBackend`, `HashMapBackend` |
| `path` | Path extraction algorithms | `viterbi`, `nbest`, `beam_search` |
| `composition` | Lazy composition operators | `LazyComposition`, `LazyCfgComposition` |
| `cfg` | Context-free grammar parsing | `Grammar`, `EarleyParser`, `ParseForest` |
| `layers` | Correction pipeline | `CorrectionLayer`, `LayerPipeline` |
| `algorithms` | Core WFST algorithms | `shortest_distance`, `weight_push`, `epsilon_removal`, `determinize`, `minimize` |
| `ctc` | CTC topologies for ASR | `CorrectCtc`, `CompactCtc`, `MinimalCtc`, `SelflessCtc` |
| `differentiable` | Differentiable operations | `ForwardScore`, `ViterbiGradient`, `GradientWfst`, `WfstConvLayer` |
| `optimization` | Beam search optimization | `prepare_for_beam_search`, `LookaheadTable`, `TokenGroupManager`, `NgramLmBuilder` |
| `asr` | Speech recognition pipeline | `TriphoneBuilder`, `NgramLmBuilder`, `CascadeBuilder`, `ChainFactoring`, `LatticeRescorer` |
| `gpu` | GPU-optimized structures | `CsrWfst`, `PackedToken`, `LoadBalancer`, `KVector`, `BatchedDecoder`, `SoftPruneManager` |

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

## Advanced Features

lling-llang includes several advanced modules for speech recognition and deep learning:

### CTC Topologies

Connectionist Temporal Classification (CTC) graph topologies for speech recognition:

| Topology | States | Arcs | Memory Savings |
|----------|--------|------|----------------|
| Correct-CTC | N | N² | Baseline |
| Compact-CTC | N | 3N-2 | 1.5× smaller |
| Minimal-CTC | 1 | N | 2× smaller |

See [CTC Topologies](../advanced/ctc-topologies.md) for details.

### Differentiable Operations

Automatic differentiation through WFST operations enables end-to-end training:

```rust
// Compute forward score with gradients
let (score, gradients) = forward_score_with_gradient(&wfst);

// Gradients flow back through composition, intersection, etc.
```

See [Differentiable Operations](../advanced/differentiable.md) for details.

### GPU Acceleration

GPU-ready data structures follow the high-throughput decoder of Braun et al. (2020):

- **CSR Representation**: 1/3 memory of standard formats
- **uint64 Token Packing**: Lock-free atomic recombination
- **Cooperative Groups**: Dynamic load balancing
- **Channels/Lanes**: Batched streaming for thousands of concurrent streams

See [GPU Acceleration](../advanced/gpu-acceleration.md) for details.

### ASR Pipeline

Complete speech recognition transducer construction:

```
N = π(min(det(H̃ ∘ det(C̃ ∘ det(L̃ ∘ G)))))

Where:
  G = Word-level grammar (n-gram LM)
  L = Pronunciation lexicon
  C = Context-dependency (triphones)
  H = HMM transducer
```

See [ASR Pipeline](../advanced/asr-pipeline.md) for details.

## Next Steps

### Core Concepts
- [Semirings](semirings.md): Understand the algebraic foundation
- [WFST Operations](wfst-operations.md): Rational and unary operations
- [Lattices](lattices.md): Learn lattice construction and operations
- [Backends](backends.md): Storage abstraction layer

### Algorithms
- [Path Extraction](../algorithms/path-extraction.md): Viterbi, N-best, beam search
- [Shortest Distance](../algorithms/shortest-distance.md): Core graph algorithms
- [Weight Pushing](../algorithms/weight-pushing.md): Weight normalization
- [Determinization](../algorithms/determinization.md): Remove non-determinism
- [Minimization](../algorithms/minimization.md): Reduce WFST size

### Advanced
- [CTC Topologies](../advanced/ctc-topologies.md): ASR graph structures
- [Differentiable Operations](../advanced/differentiable.md): Gradient computation
- [Beam Optimization](../advanced/beam-optimization.md): Log-semiring pushing
- [GPU Acceleration](../advanced/gpu-acceleration.md): High-performance decoding
- [ASR Pipeline](../advanced/asr-pipeline.md): Speech recognition transducers
