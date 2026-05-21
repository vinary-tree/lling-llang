# LaTeX Syntax Layer Overview

The LaTeX syntax layer provides CFG-based filtering and structural validation for LaTeX documents within the lling-llang WFST correction pipeline.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   LaTeX Syntax Layer                         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Input Lattice                                              │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │               Phase 1: CFG Parsing                   │   │
│  │  ┌─────────────────┐   ┌─────────────────┐         │   │
│  │  │ LaTeX Grammar   │ → │ Earley Parser   │         │   │
│  │  │ (standard/math/ │   │ (parse lattice) │         │   │
│  │  │  minimal)       │   │                 │         │   │
│  │  └─────────────────┘   └────────┬────────┘         │   │
│  │                                 │                   │   │
│  │                                 ▼                   │   │
│  │                         Parse Forest                │   │
│  │                         (valid paths)               │   │
│  └─────────────────────────────────────────────────────┘   │
│                                 │                          │
│                                 ▼                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │            Phase 2: Structural Validation            │   │
│  │  ┌───────────┐ ┌───────────┐ ┌───────────┐         │   │
│  │  │  Brace    │ │Environment│ │   Math    │         │   │
│  │  │ Matching  │ │ Pairing   │ │ Delimiter │         │   │
│  │  └─────┬─────┘ └─────┬─────┘ └─────┬─────┘         │   │
│  │        └─────────────┴─────────────┘               │   │
│  │                      │                              │   │
│  │                      ▼                              │   │
│  │            Validation Result                        │   │
│  └─────────────────────────────────────────────────────┘   │
│                                 │                          │
│                                 ▼                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │             Phase 3: Repair Generation               │   │
│  │  ┌───────────┐ ┌───────────┐ ┌───────────┐         │   │
│  │  │  Brace    │ │Environment│ │   Math    │         │   │
│  │  │ Repairs   │ │ Repairs   │ │ Repairs   │         │   │
│  │  └─────┬─────┘ └─────┬─────┘ └─────┬─────┘         │   │
│  │        └─────────────┴─────────────┘               │   │
│  │                      │                              │   │
│  │                      ▼                              │   │
│  │            Repair Suggestions                       │   │
│  │            (sorted by confidence)                   │   │
│  └─────────────────────────────────────────────────────┘   │
│                                 │                          │
│                                 ▼                          │
│  Output: Filtered Lattice + Repair Suggestions             │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Components

| Component | Description |
|-----------|-------------|
| `LatexGrammar` | CFG rules for LaTeX document structure |
| `LatexSyntaxLayer` | Main correction layer implementation |
| `LatexValidator` | Structural validation (braces, environments, math) |
| `RepairStrategy` | Repair suggestion generation |

## Basic Usage

```rust
use lling_llang::layers::latex::{
    LatexSyntaxLayer,
    LatexGrammar,
    LatexSyntaxConfig
};

// Create grammar
let grammar = LatexGrammar::standard()?;

// Create layer with default configuration
let layer = LatexSyntaxLayer::new(grammar);

// Apply to a lattice
let filtered_lattice = layer.apply(&input_lattice)?;

// Check for repair suggestions
for repair in layer.last_repairs() {
    println!("{}: {} (confidence: {:.2})",
        repair.description,
        repair.tokens.join(" "),
        repair.confidence
    );
}
```

## Layer Integration

The LaTeX syntax layer integrates with the lling-llang correction pipeline:

```rust
use lling_llang::pipeline::CorrectionPipeline;
use lling_llang::layers::latex::LatexSyntaxLayer;
use lling_llang::layers::mathml::MathMLSemanticLayer;

let pipeline = CorrectionPipeline::new()
    .add_layer(LatexSyntaxLayer::new(LatexGrammar::standard()?))
    .add_layer(MathMLSemanticLayer::new());

let result = pipeline.correct(&input_lattice)?;
```

## Configuration Presets

| Preset | Description | Use Case |
|--------|-------------|----------|
| `default()` | Balanced pruning and repair | General use |
| `strict()` | Aggressive pruning, more repairs | High-quality output |
| `lenient()` | Keep more paths, auto-repair | Error-tolerant processing |
| `minimal()` | Fast processing, no repairs | Performance-critical |

## Grammar Variants

| Grammar | Coverage | Performance |
|---------|----------|-------------|
| `standard()` | Full LaTeX + AMS math | Normal |
| `math()` | Math expressions only | Fast |
| `minimal()` | Brace matching only | Fastest |

## Key Features

1. **Multi-Pass Processing**: CFG parsing followed by structural validation
2. **Incremental Repair**: Generate suggestions for each validation issue
3. **Configurable Strictness**: Balance between precision and recall
4. **Thread Safety**: Layer can be shared across threads

## Related Documentation

- [Grammar](./grammar.md): CFG rule details
- [Validator](./validator.md): Structural validation
- [Repair](./repair.md): Repair strategies
- [MathML Layer](../mathml/overview.md): Semantic type checking
