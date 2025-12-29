# lling-llang Documentation

Welcome to the documentation for **lling-llang**, a Weighted Finite State Transducer (WFST) framework for text normalization and grammar correction.

## Quick Start

```rust
use lling_llang::prelude::*;

// Build a correction lattice
let backend = HashMapBackend::new();
let mut builder = LatticeBuilder::<TropicalWeight, _>::new(backend);

builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::default());
builder.add_correction(0, 1, "teh", TropicalWeight::new(0.0), EdgeMetadata::original());

let mut lattice = builder.build(1);

// Find the best correction
let result = viterbi(&mut lattice);
if result.success {
    println!("Best: {:?}", result.path.to_words(&lattice));
}
```

## Documentation Sections

### Architecture

Core concepts and design of the framework:

| Document | Description |
|----------|-------------|
| [Overview](architecture/overview.md) | High-level architecture and component relationships |
| [Semirings](architecture/semirings.md) | Algebraic weight structures (Tropical, Log, Probability, String, Expectation) |
| [Power Semiring](architecture/power-semiring.md) | η-power semiring for soft path selection and online learning |
| [WFST Operations](architecture/wfst-operations.md) | Rational operations (union, concat, closure) and unary operations (invert, project, reverse) |
| [Lattices](architecture/lattices.md) | Weighted DAGs representing correction alternatives |
| [WFST Traits](architecture/wfst-traits.md) | Trait hierarchy for finite state transducers |
| [Backends](architecture/backends.md) | Storage abstraction and implementations |
| [Layers](architecture/layers.md) | Correction layer pipeline architecture |

### Algorithms

Core WFST algorithms:

| Document | Description |
|----------|-------------|
| [Path Extraction](algorithms/path-extraction.md) | Viterbi, N-best, and beam search algorithms |
| [Shortest Distance](algorithms/shortest-distance.md) | Single-source and all-pairs shortest distance with queue disciplines |
| [Weight Pushing](algorithms/weight-pushing.md) | Weight normalization for beam search optimization |
| [Epsilon Removal](algorithms/epsilon-removal.md) | Remove epsilon transitions from WFSTs |
| [Determinization](algorithms/determinization.md) | Transform non-deterministic to deterministic WFSTs |
| [Minimization](algorithms/minimization.md) | Minimize WFST states and transitions |
| [Synchronization](algorithms/synchronization.md) | Normalize input/output label shifting in transducers |
| [Parsing](algorithms/parsing.md) | Earley parser for lattice input |
| [Composition](algorithms/composition.md) | Lazy FST and CFG composition operators |
| [Topological Sort](algorithms/topological-sort.md) | Kahn's algorithm for DAG ordering |
| [Path Sampling](algorithms/path-sampling.md) | Random path sampling from WFSTs for Monte Carlo methods |
| [RRWM](algorithms/rrwm.md) | Rational Randomized Weighted-Majority for online ensemble learning |

### Advanced Features

Advanced modules for speech recognition and deep learning:

| Document | Description |
|----------|-------------|
| [CTC Topologies](advanced/ctc-topologies.md) | CTC graph structures (Correct, Compact, Minimal, Selfless) |
| [Differentiable Operations](advanced/differentiable.md) | Gradient computation through WFST operations |
| [Deep Learning Integration](advanced/deep-learning.md) | WFST layers, token graphs, lexicon marginalization |
| [ASR Pipeline](advanced/asr-pipeline.md) | Speech recognition transducer construction (H∘C∘L∘G) |
| [Beam Optimization](advanced/beam-optimization.md) | Log-semiring pushing, lookahead, token grouping |
| [GPU Acceleration](advanced/gpu-acceleration.md) | CSR format, atomic recombination, batched streaming |

### Acoustic Integration

Acoustic model and ASR components:

| Document | Description |
|----------|-------------|
| [Acoustic Overview](acoustic/overview.md) | AcousticModel trait, TransitionMatrix, score fusion |
| [Subword Lexicon](asr/subword-lexicon.md) | BPE/subword lexicon builder for ASR |

### Integration Guides

Integrating lling-llang with other systems:

#### F1R3FLY.io Ecosystem

| Document | Description |
|----------|-------------|
| [Vision](integration/f1r3fly/vision.md) | Full stack vision for distributed correction |
| [PathMap Backend](integration/f1r3fly/pathmap-backend.md) | Distributed content-addressed storage |
| [MeTTaIL Layer](integration/f1r3fly/mettail-layer.md) | Type inference and verification |
| [MORK Layer](integration/f1r3fly/mork-layer.md) | Rule engine for grammar rules |
| [MeTTaTron Layer](integration/f1r3fly/mettatron-layer.md) | Compiler for correction specifications |
| [Rholang Layer](integration/f1r3fly/rholang-layer.md) | Concurrent, distributed pipelines |

#### External Systems

| Document | Description |
|----------|-------------|
| [Speech/NLP Pipelines](integration/external/speech-nlp.md) | ASR and NLP integration patterns |
| [Text Correction](integration/external/text-correction.md) | Spelling/grammar correction apps |
| [Library Usage](integration/external/library-usage.md) | Generic library integration |

#### libgrammstein Integration

| Document | Description |
|----------|-------------|
| [Phonetic Rescoring](integration/libgrammstein/phonetic-rescore.md) | Phonetic lattice rescoring with Zompist rules |

### API Reference

Detailed API documentation:

| Document | Description |
|----------|-------------|
| [Semiring Reference](api/semiring-reference.md) | `Semiring`, `DivisibleSemiring`, `StarSemiring` |
| [WFST Reference](api/wfst-reference.md) | `Wfst`, `MutableWfst`, `LazyWfst` |
| [Lattice Reference](api/lattice-reference.md) | `Lattice`, `LatticeBuilder`, `EdgeMetadata` |
| [Backend Reference](api/backend-reference.md) | `LatticeBackend`, `HashMapBackend` |
| [Path Reference](api/path-reference.md) | `viterbi`, `nbest`, `beam_search` |
| [Layer Reference](api/layer-reference.md) | `CorrectionLayer`, `LayerPipeline` |

## Feature Flags

| Feature | Description |
|---------|-------------|
| `default` | Standalone WFST framework |
| `levenshtein` | Integration with liblevenshtein |
| `pos-tagging` | POS tagging layer support |
| `lm-rerank` | Language model reranking |
| `f1r3fly` | Full F1R3FLY.io integration |
| `sexpr` | S-expression path format |
| `serde` | Serialization support |
| `acoustic` | Acoustic model integration and score fusion |
| `phonetic-rescore` | Phonetic rescoring layer with Zompist rules |
| `subword-lexicon` | BPE/subword lexicon builder for ASR |

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Correction Layer Stack                           │
├─────────────────────────────────────────────────────────────────────────┤
│  Layer N: [User-Defined]           ← Implement CorrectionLayer trait    │
│     ↑                                                                   │
│  Layer 3: CFG Grammar              ← Syntactic filtering                │
│     ↑                                                                   │
│  Layer 1: Lexical Correction       ← Levenshtein + phonetic candidates  │
│     ↑                                                                   │
│  [Input Lattice]                                                        │
└─────────────────────────────────────────────────────────────────────────┘
```

## Learning Path

**New to WFSTs?** Start here:

1. [Semirings](architecture/semirings.md) - Understand the algebraic foundation
2. [Lattices](architecture/lattices.md) - Learn about weighted DAGs
3. [Path Extraction](algorithms/path-extraction.md) - Find optimal paths
4. [Layers](architecture/layers.md) - Build correction pipelines

**Working with WFST algorithms?**

1. [WFST Operations](architecture/wfst-operations.md) - Union, concat, closure, invert, project
2. [Shortest Distance](algorithms/shortest-distance.md) - Core graph algorithms
3. [Weight Pushing](algorithms/weight-pushing.md) - Weight normalization
4. [Determinization](algorithms/determinization.md) - Remove non-determinism
5. [Minimization](algorithms/minimization.md) - Reduce WFST size

**Building speech recognition systems?**

1. [CTC Topologies](advanced/ctc-topologies.md) - Graph structures for CTC
2. [ASR Pipeline](advanced/asr-pipeline.md) - H∘C∘L∘G cascade construction
3. [Acoustic Overview](acoustic/overview.md) - Acoustic model integration
4. [Subword Lexicon](asr/subword-lexicon.md) - BPE lexicons for open vocabulary
5. [Beam Optimization](advanced/beam-optimization.md) - Log-semiring pushing for speed
6. [GPU Acceleration](advanced/gpu-acceleration.md) - High-performance decoding

**Integrating with deep learning?**

1. [Differentiable Operations](advanced/differentiable.md) - Gradients through WFSTs
2. [Deep Learning Integration](advanced/deep-learning.md) - WFST layers and marginalization

**Integrating with your system?**

1. [Library Usage](integration/external/library-usage.md) - General patterns
2. Choose your domain:
   - [Speech/NLP](integration/external/speech-nlp.md) for ASR pipelines
   - [Text Correction](integration/external/text-correction.md) for editors
   - [F1R3FLY.io Vision](integration/f1r3fly/vision.md) for distributed systems

**Building production systems?**

1. [API Reference](api/) - Complete API documentation
2. [Backends](architecture/backends.md) - Storage strategies
3. [PathMap](integration/f1r3fly/pathmap-backend.md) - Distributed storage
