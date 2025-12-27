# lling-llang

A Weighted Finite State Transducer (WFST) framework for text normalization and grammar correction.

[![Crates.io](https://img.shields.io/crates/v/lling-llang.svg)](https://crates.io/crates/lling-llang)
[![Documentation](https://docs.rs/lling-llang/badge.svg)](https://docs.rs/lling-llang)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

## Overview

lling-llang represents the space of possible text corrections as a **weighted directed acyclic graph (lattice)**, then uses efficient algorithms to find optimal paths through this space. The framework is built on **semiring algebra**, which provides a unified interface for different optimization objectives (shortest path, highest probability, reachability).

```
Input: "teh quik fox"
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  1. Candidate Generation                                                    │
│     "teh" → { "the" (0.5), "teh" (0.0), "tea" (1.5) }                       │
│     "quik" → { "quick" (0.5), "quik" (0.0) }                                │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  2. Lattice (Weighted DAG)                                                  │
│                                                                             │
│            ┌───the(0.5)───┐                                                 │
│   start ──►│              ├───quick(0.5)───►fox(0.0)──►end                  │
│            ├───teh(0.0)───┤               ▲                                 │
│            └───tea(1.5)───┘───quik(0.0)───┘                                 │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  3. Path Extraction                                                         │
│     viterbi() → Best path: "the quick fox" (weight: 1.0)                    │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         ▼
Output: "the quick fox"
```

## Features

- **Weighted lattice representation** for correction alternatives
- **Multiple semiring types**: Tropical (shortest path), Log (probabilities), Boolean (reachability), Product (multi-objective)
- **Correction layer pipeline** for modular, composable processing stages
- **Path extraction algorithms**: Viterbi (best), N-best (top-k), beam search (approximate)
- **CFG grammar filtering** with Earley parser
- **Lazy composition** to avoid exponential blowup
- **Pluggable storage backends** (in-memory, distributed)
- **Optional integrations**: liblevenshtein for fuzzy matching, F1R3FLY.io ecosystem

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
lling-llang = "0.1"
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `default` | Standalone WFST framework |
| `levenshtein` | Integration with liblevenshtein for fuzzy matching |
| `pcfg` | Probabilistic CFG support |
| `pos-tagging` | POS tagging layer |
| `lm-rerank` | Language model reranking layer |
| `f1r3fly` | Full F1R3FLY.io integration (PathMap, MeTTaIL, etc.) |
| `serde` | Serialization support |

Enable features:

```toml
[dependencies]
lling-llang = { version = "0.1", features = ["levenshtein", "serde"] }
```

## Quick Start

```rust
use lling_llang::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a backend for vocabulary storage
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::<TropicalWeight, _>::new(backend);

    // Build a lattice with correction alternatives
    // Position 0 → 1: alternatives for "teh"
    builder.add_token(0, 1, "teh", TropicalWeight::new(0.0));      // original
    builder.add_correction(0, 1, "the", TropicalWeight::new(0.5),
        EdgeMetadata::spelling_correction("teh", "the"));

    // Position 1 → 2: alternatives for "quik"
    builder.add_token(1, 2, "quik", TropicalWeight::new(0.0));     // original
    builder.add_correction(1, 2, "quick", TropicalWeight::new(0.5),
        EdgeMetadata::spelling_correction("quik", "quick"));

    // Position 2 → 3: "fox" (no correction needed)
    builder.add_token(2, 3, "fox", TropicalWeight::new(0.0));

    // Build the lattice (3 positions)
    let mut lattice = builder.build(3);

    // Find the best path using Viterbi algorithm
    let best = viterbi(&mut lattice);
    println!("Best path: {}", best.join(" "));
    println!("Total weight: {:?}", best.weight);

    Ok(())
}
```

## Core Concepts

### Semirings

A **semiring** is an algebraic structure that generalizes addition and multiplication. Different semirings encode different optimization objectives:

| Semiring | Plus (⊕) | Times (⊗) | Zero | One | Use Case |
|----------|----------|-----------|------|-----|----------|
| Tropical | min | + | ∞ | 0 | Shortest path (edit distance) |
| Log | log-add | + | ∞ | 0 | Probabilities (language models) |
| Boolean | OR | AND | false | true | Reachability queries |
| Product | (⊕₁, ⊕₂) | (⊗₁, ⊗₂) | (0̄₁, 0̄₂) | (1̄₁, 1̄₂) | Multi-objective optimization |

```rust
use lling_llang::semiring::{Semiring, TropicalWeight};

let a = TropicalWeight::new(2.0);
let b = TropicalWeight::new(3.0);

// Parallel paths: take the minimum
let parallel = a.plus(&b);   // min(2, 3) = 2

// Sequential edges: add the costs
let sequential = a.times(&b); // 2 + 3 = 5
```

The semiring abstraction allows the same algorithms to work with different weight types.

### Lattices

A **lattice** is a weighted directed acyclic graph (DAG) where:
- **Nodes** represent positions in the input sequence
- **Edges** represent token alternatives with weights
- **Paths** from start to end represent complete correction hypotheses

```
Position:   0         1         2         3
            │         │         │         │
            │  ┌─the──┤  ┌quick─┤         │
   start ───┼──┤      ├──┤      ├──fox────┼───► end
            │  └─teh──┤  └quik──┤         │
            │         │         │         │

Paths:
  "the quick fox"  (weight: 0.5 + 0.5 + 0.0 = 1.0)
  "the quik fox"   (weight: 0.5 + 0.0 + 0.0 = 0.5)
  "teh quick fox"  (weight: 0.0 + 0.5 + 0.0 = 0.5)
  "teh quik fox"   (weight: 0.0 + 0.0 + 0.0 = 0.0)  ← original
```

Lattices compactly represent exponentially many alternatives through shared structure.

### Correction Layers

A **correction layer** transforms a lattice by filtering paths or adjusting weights. Layers can be composed into pipelines:

```
Input Lattice
     │
     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  Layer 1: Lexical Correction       ← Add spelling alternatives              │
│     ↓                                                                       │
│  Layer 2: CFG Grammar Filter       ← Remove syntactically invalid paths     │
│     ↓                                                                       │
│  Layer 3: Language Model           ← Reweight based on n-gram scores        │
│     ↓                                                                       │
│  Layer N: Custom                   ← Your application-specific logic        │
└─────────────────────────────────────────────────────────────────────────────┘
     │
     ▼
Filtered Lattice
```

```rust
use lling_llang::layers::{LayerPipelineBuilder, CfgFilterLayer};

let pipeline = LayerPipelineBuilder::new()
    .add_layer(SpellingLayer::new(dictionary, max_distance))
    .add_layer(CfgFilterLayer::new(&grammar))
    .add_layer(LanguageModelLayer::new(&model))
    .build();

let corrected = pipeline.apply(&lattice)?;
```

### Path Extraction

After building (and optionally filtering) a lattice, extract the best correction(s):

| Algorithm | Returns | Complexity | Use Case |
|-----------|---------|------------|----------|
| `viterbi()` | Single best path | O(V + E) | Production correction |
| `nbest(n)` | Top-n paths | O((V + E) log n) | Alternative suggestions |
| `beam_search(width)` | Approximate top paths | O(V × B) | Large lattices |

```rust
// Single best path
let best = viterbi(&mut lattice);

// Top 5 alternatives
let top5 = nbest(&mut lattice, 5);

// Beam search for large lattices
let paths = beam_search(&mut lattice, 10);
```

## Examples

### Spelling Correction

```rust
use lling_llang::prelude::*;

// Build lattice with spelling alternatives from a dictionary
fn build_spelling_lattice(
    tokens: &[&str],
    dictionary: &impl Dictionary,
    max_distance: usize,
) -> Lattice<TropicalWeight, HashMapBackend> {
    let mut builder = LatticeBuilder::new(HashMapBackend::new());

    for (pos, &token) in tokens.iter().enumerate() {
        // Always include the original token
        builder.add_token(pos, pos + 1, token, TropicalWeight::one());

        // Add fuzzy matches from dictionary
        for candidate in dictionary.fuzzy_search(token, max_distance) {
            let weight = TropicalWeight::new(candidate.distance as f64);
            builder.add_correction(
                pos, pos + 1,
                &candidate.term,
                weight,
                EdgeMetadata::edit_correction(token, &candidate.term, candidate.distance),
            );
        }
    }

    builder.build(tokens.len())
}
```

### Grammar-Constrained Correction

```rust
use lling_llang::prelude::*;
use lling_llang::cfg::GrammarBuilder;
use lling_llang::layers::CfgFilterLayer;

// Define a grammar
let grammar = GrammarBuilder::new()
    .start("S")
    .rule("S", &["NP", "VP"])
    .rule("NP", &["Det", "N"])
    .rule("VP", &["V", "NP"])
    .rule("Det", &["the", "a"])
    .rule("N", &["cat", "dog"])
    .rule("V", &["chased", "saw"])
    .build()?;

// Filter lattice to only grammatically valid paths
let layer = CfgFilterLayer::new(&grammar);
let filtered = layer.apply(&lattice)?;

// Extract best grammatical path
let best = viterbi(&mut filtered);
```

### Multi-Layer Pipeline

```rust
use lling_llang::prelude::*;
use lling_llang::layers::*;

// Build a correction pipeline
let pipeline = LayerPipelineBuilder::new()
    // Layer 1: Add spelling alternatives (edit distance ≤ 2)
    .add_layer(SpellingCorrectionLayer::new(dictionary, 2))
    // Layer 2: Filter by grammar
    .add_layer(CfgFilterLayer::new(&grammar))
    // Layer 3: Rerank by language model
    .add_layer(LanguageModelLayer::new(&lm).with_weight(0.5))
    .build();

// Apply pipeline and get statistics
let (corrected, stats) = pipeline.apply_with_stats(&lattice)?;

for (i, stat) in stats.iter().enumerate() {
    println!("Layer {}: {} edges → {} edges ({:.1}% reduction)",
        pipeline.layer_names()[i],
        stat.input_edges,
        stat.output_edges,
        (1.0 - stat.reduction_ratio()) * 100.0);
}
```

## Documentation

Comprehensive documentation is available in the [`docs/`](docs/) directory:

| Section | Description |
|---------|-------------|
| [Architecture](docs/architecture/) | Core concepts: semirings, lattices, backends, layers |
| [Algorithms](docs/algorithms/) | Path extraction, parsing, composition, topological sort |
| [Integration](docs/integration/) | F1R3FLY.io ecosystem, liblevenshtein, external systems |
| [API Reference](docs/api/) | Complete API documentation for all modules |

**Start here:**
- [Architecture Overview](docs/architecture/overview.md) - High-level design
- [Semirings](docs/architecture/semirings.md) - Algebraic foundation
- [Lattices](docs/architecture/lattices.md) - Core data structure
- [Path Extraction](docs/algorithms/path-extraction.md) - Finding optimal paths

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              lling-llang                                    │
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

## Project Status

**Version**: 0.1.0 (Early Development)

This project is in active development. The core API is stabilizing, but breaking changes may occur before 1.0.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

Contributions are welcome. Please see the [documentation](docs/) for architecture details before submitting PRs.
