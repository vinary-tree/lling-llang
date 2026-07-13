# LaTeX Syntax Layer Overview

The LaTeX syntax layer provides CFG-based filtering and structural validation for LaTeX documents within the lling-llang WFST correction pipeline.

## Architecture

`LatexSyntaxLayer::apply` runs a three-pass pipeline over the input lattice,
each pass gated by a `LatexSyntaxConfig` flag: **Pass 1** prunes ungrammatical
edges with an Earley parse over the `LatexGrammar`; **Pass 2** validates structure
(brace matching, environment begin/end pairing, math-delimiter balance) with the
`LatexValidator`; **Pass 3** generates repair suggestions for any
`ValidationIssue`s, optionally applying high-confidence ones automatically
when $`\text{confidence} \ge \text{auto\_repair\_threshold}`$.

![Activity diagram: LatexSyntaxLayer.apply flows from the input lattice through an optional grammar-filter pass (prune_ungrammatical), a structural-validation pass (validate_structure) that emits ValidationIssues, and a repair-generation pass (generate_repairs) that sorts suggestions by confidence and either auto-applies them or stashes them in last_repairs, ending at the output lattice plus repair suggestions.](../../diagrams/layers/latex/repair-flow.svg)

*Amber = the validate/repair activities; grey diamonds = the `LatexSyntaxConfig`
gates (`prune_ungrammatical`, `validate_structure`, `generate_repairs`,
`auto_repair`); green terminal = the filtered lattice plus its
`RepairSuggestion` list.*

<details><summary>Text view</summary>

```text
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

</details>

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
use lling_llang::layers::LayerPipelineBuilder;
use lling_llang::layers::latex::LatexSyntaxLayer;
use lling_llang::layers::mathml::MathMLSemanticLayer;

let pipeline = LayerPipelineBuilder::new()
    .add_layer(LatexSyntaxLayer::new(LatexGrammar::standard()?))
    .add_layer(MathMLSemanticLayer::new())
    .build();

let result = pipeline.apply(&input_lattice)?;
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

## References

- [Earley 1970](../../BIBLIOGRAPHY.md#ref-earley1970) — the context-free parsing
  algorithm that drives Pass 1 (grammar filtering) over the lattice.
- [Mohri 2002](../../BIBLIOGRAPHY.md#ref-mohri2002) — weighted finite-state
  transducers; the lattice representation the layer filters and repairs.
