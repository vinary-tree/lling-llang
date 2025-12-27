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
| [Semirings](architecture/semirings.md) | Algebraic weight structures for path computation |
| [Lattices](architecture/lattices.md) | Weighted DAGs representing correction alternatives |
| [WFST Traits](architecture/wfst-traits.md) | Trait hierarchy for finite state transducers |
| [Backends](architecture/backends.md) | Storage abstraction and implementations |
| [Layers](architecture/layers.md) | Correction layer pipeline architecture |

### Algorithms

Algorithms for path extraction and parsing:

| Document | Description |
|----------|-------------|
| [Path Extraction](algorithms/path-extraction.md) | Viterbi, N-best, and beam search algorithms |
| [Parsing](algorithms/parsing.md) | Earley parser for lattice input |
| [Composition](algorithms/composition.md) | Lazy FST and CFG composition operators |
| [Topological Sort](algorithms/topological-sort.md) | Kahn's algorithm for DAG ordering |

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
