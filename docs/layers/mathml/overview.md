# MathML Semantic Layer Overview

The MathML semantic layer provides type checking and homoglyph disambiguation for mathematical expressions based on Content MathML semantics.

## Architecture

`MathMLSemanticLayer::apply` runs two phases over the input lattice, each gated by
a `MathMLSemanticConfig` flag. **Phase 1** disambiguates homoglyphs: the
`HomoglyphDisambiguator` scores each confusable glyph against its `MathContext`
(and optionally normalizes it to a canonical form). **Phase 2** type-checks the
expression: the `MathTypeChecker` looks up signatures and applies Hindley–Milner
unification, optionally pruning any path that carries a type error.

![Activity diagram: MathMLSemanticLayer.apply flows from the input lattice through Phase 1 homoglyph disambiguation (context scoring, optional canonical normalization, low-confidence AmbiguousGlyph issues) into Phase 2 type checking (signature lookup, Hindley-Milner unification, arity and division-by-zero checks, optional pruning of paths with type errors), ending at the output lattice plus a SemanticResult.](../../diagrams/layers/mathml/checker-flow.svg)

*Amber = the disambiguation/type-check activities; grey diamonds = the
`MathMLSemanticConfig` gates (`disambiguate_homoglyphs`, `normalize_homoglyphs`,
`check_types`, `prune_type_errors`); green terminal = the filtered lattice plus
its `SemanticResult` (`inferred_type`, `issues`, `disambiguations`).*

<details><summary>Text view</summary>

```text
┌─────────────────────────────────────────────────────────────┐
│                   MathML Semantic Layer                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Input Lattice                                              │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │          Phase 1: Homoglyph Disambiguation          │   │
│  │  ┌───────────────────────────────────────────────┐ │   │
│  │  │         HomoglyphDisambiguator                │ │   │
│  │  │  ┌─────────────┐   ┌─────────────┐          │ │   │
│  │  │  │ Confusion   │ → │  Context    │          │ │   │
│  │  │  │ Sets        │   │  Analysis   │          │ │   │
│  │  │  └─────────────┘   └──────┬──────┘          │ │   │
│  │  │                           │                  │ │   │
│  │  │                           ▼                  │ │   │
│  │  │              Disambiguation Decisions        │ │   │
│  │  └───────────────────────────────────────────────┘ │   │
│  └─────────────────────────────────────────────────────┘   │
│                                 │                          │
│                                 ▼                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Phase 2: Type Checking                 │   │
│  │  ┌───────────────────────────────────────────────┐ │   │
│  │  │            MathTypeChecker                    │ │   │
│  │  │  ┌─────────────┐   ┌─────────────┐          │ │   │
│  │  │  │ Type        │ → │  Inference  │          │ │   │
│  │  │  │ Signatures  │   │  & Unify    │          │ │   │
│  │  │  └─────────────┘   └──────┬──────┘          │ │   │
│  │  │                           │                  │ │   │
│  │  │                           ▼                  │ │   │
│  │  │               TypeResult (errors/warnings)   │ │   │
│  │  └───────────────────────────────────────────────┘ │   │
│  └─────────────────────────────────────────────────────┘   │
│                                 │                          │
│                                 ▼                          │
│  Output: Filtered Lattice + Semantic Analysis              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

</details>

## Components

| Component | Description |
|-----------|-------------|
| `MathMLSemanticLayer` | Main correction layer implementation |
| `MathTypeChecker` | Type inference and checking |
| `HomoglyphDisambiguator` | Context-aware glyph disambiguation |
| `TypeEnvironment` | Variable binding with scoping |

## Basic Usage

```rust
use lling_llang::layers::mathml::{
    MathMLSemanticLayer,
    MathMLSemanticConfig
};

// Create layer with default configuration
let layer = MathMLSemanticLayer::new();

// Apply to a lattice
let filtered_lattice = layer.apply(&input_lattice)?;

// Check analysis results
for result in layer.last_results() {
    if !result.is_valid {
        for issue in result.errors() {
            println!("Error at {:?}: {}", issue.position, issue.message);
        }
    }
}
```

## Layer Integration

The MathML semantic layer integrates with the lling-llang correction pipeline:

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
| `default()` | Type checking + disambiguation, prune errors | General use |
| `strict()` | Aggressive pruning, normalization enabled | High-quality output |
| `lenient()` | Keep more paths, no pruning | Error-tolerant processing |
| `minimal()` | No type checking, disambiguation only | Performance-critical |

```rust
// Strict configuration
let layer = MathMLSemanticLayer::with_config(MathMLSemanticConfig::strict());

// Lenient configuration
let layer = MathMLSemanticLayer::with_config(MathMLSemanticConfig::lenient());
```

## Semantic Analysis Result

```rust
pub struct SemanticResult {
    pub is_valid: bool,
    pub inferred_type: Option<MathType>,
    pub issues: Vec<SemanticIssue>,
    pub disambiguations: Vec<DisambiguationDecision>,
}
```

### Issue Kinds

| Kind | Description |
|------|-------------|
| `TypeMismatch` | Type incompatibility in expression |
| `ArityMismatch` | Wrong number of function arguments |
| `UndefinedVariable` | Unknown identifier |
| `DivisionByZero` | Division by literal zero |
| `AmbiguousGlyph` | Low-confidence disambiguation |
| `InvalidStructure` | Malformed expression |

### Issue Severity

| Severity | Description |
|----------|-------------|
| `Info` | Informational only |
| `Warning` | Non-fatal issue |
| `Error` | May cause path pruning |

## Key Features

1. **Two-Phase Processing**: Homoglyph disambiguation followed by type checking
2. **Context-Aware Disambiguation**: Uses surrounding tokens to determine glyph meaning
3. **Hindley-Milner Style Inference**: Type variables and unification
4. **Configurable Strictness**: Balance between precision and recall
5. **Thread Safety**: Layer can be shared across threads

## Example Analysis

```rust
use lling_llang::layers::mathml::MathMLSemanticLayer;

let layer = MathMLSemanticLayer::new();

// Analyze a mathematical expression
let result = layer.analyze(&["\\frac", "{", "1", "}", "{", "2", "}"]);

println!("Valid: {}", result.is_valid);
println!("Type: {:?}", result.inferred_type);

// Check for disambiguations
for decision in &result.disambiguations {
    println!("Glyph '{}' interpreted as {:?} (confidence: {:.2})",
        decision.original,
        decision.meaning,
        decision.confidence
    );
}
```

## Related Documentation

- [Types](./types.md): Type system details
- [Checker](./checker.md): Type checking
- [Homoglyph](./homoglyph.md): Homoglyph disambiguation
- [LaTeX Layer](../latex/overview.md): Syntactic filtering

## References

- [Mohri 2002](../../BIBLIOGRAPHY.md#ref-mohri2002) — weighted finite-state
  transducers; the lattice the two phases filter and reweight.
- [Goodman 1999](../../BIBLIOGRAPHY.md#ref-goodman1999) — semiring parsing; the
  algebraic basis for propagating per-path semantic scores.
- The type semantics follow the W3C *Content MathML* model (MathML 3.0,
  §4 *Content Markup*): <https://www.w3.org/TR/MathML3/chapter4.html>.
