# Code Correction Layer

lling-llang provides pattern-aware code correction layers that combine syntax error recovery with learned code idioms to produce idiomatic code corrections.

## Why Code Correction?

Programming languages have strict syntax requirements. Common errors include:
- Missing brackets, parentheses, or braces
- Typos in keywords (`funciton` instead of `function`)
- Missing semicolons or colons
- Incorrect punctuation

The code correction layer uses WFST (Weighted Finite-State Transducer) lattices to:
1. Generate correction candidates
2. Score them using grammar knowledge
3. Boost paths matching common code patterns

## Architecture

The code-correction layers are `CorrectionLayer`s composed inside a
`LayerPipeline`: each consumes a `` `Lattice⟨W, B⟩` `` and returns a (typically
smaller) `` `Lattice⟨W, B⟩` ``, applied left to right. The broader correction
pipeline runs **lexical → CFG → LM → custom**, where the code-correction layer is
one of the *custom* domain stages; internally it chains **token correction →
syntax recovery → pattern-aware boosting**.

![Activity diagram: a LayerPipeline applies its CorrectionLayers in order — lexical filtering (EditDistance/Confusion), CFG filtering (CfgFilter), LM rescoring (LanguageModel), then a custom domain branch routing to CodeCorrection, LatexSyntax, or MathMLSemantic — each mapping a lattice to a lattice.](../../diagrams/layers/code-correction/correction-layers.svg)

*Amber = correction/NLP layers (`` `Lattice⟨W, B⟩ → Lattice⟨W, B⟩` ``); grey
diamond = the domain switch; blue start = the input WFSA; green terminal = the
filtered, reweighted output lattice.*

<details><summary>Text view (the code-correction layer's internal stack)</summary>

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                    Code Correction Layer Stack                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Input: Token sequence (possibly with errors)                            │
│         "def foo( x )"  (missing colon)                                 │
│              │                                                           │
│              ▼                                                           │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                    Token Correction Layer                          │  │
│  │  Uses edit distance + vocabulary to generate token corrections     │  │
│  │  "def" → ["def"]       (exact match)                              │  │
│  │  "foo" → ["foo"]       (identifier)                               │  │
│  │  "x"   → ["x", "y"]    (possible corrections)                     │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│              │                                                           │
│              ▼                                                           │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                   Syntax Recovery Layer                            │  │
│  │  Insert/delete tokens to fix parse errors                          │  │
│  │  • Insert missing ":" after ")"                                    │  │
│  │  • Delete extra punctuation                                        │  │
│  │  • Balance brackets                                                │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│              │                                                           │
│              ▼                                                           │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                   Pattern-Aware Layer                              │  │
│  │  Boost paths matching common code idioms                           │  │
│  │  Pattern: ["def", "_", "(", ")", ":"]  → boost=1.0                │  │
│  │  Paths matching this pattern get lower costs                       │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│              │                                                           │
│              ▼                                                           │
│  Output: Corrected token lattice                                        │
│          Best path: "def foo ( x ) :"                                   │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

</details>

## Quick Start

```rust
use lling_llang::layers::{LayerPipeline, CorrectionLayer};
use lling_llang::layers::code_correction::{
    CodeCorrectionLayer, CodeCorrectionConfig,
    SyntaxRecoveryConfig, PatternAwareConfig,
};
use lling_llang::backend::HashMapBackend;
use lling_llang::semiring::TropicalWeight;

// Create a Python code correction layer
let config = CodeCorrectionConfig::new("python")
    .with_syntax_recovery(SyntaxRecoveryConfig::default())
    .with_pattern_aware(PatternAwareConfig::python_patterns())
    .with_max_corrections(5);

let layer: CodeCorrectionLayer<TropicalWeight, HashMapBackend> =
    CodeCorrectionLayer::new(config);

// Apply to a lattice
let corrected = layer.apply(&input_lattice)?;
```

## Supported Languages

The code correction layer supports multiple programming languages:

| Language | Keyword Support | Pattern Support | Bracket Balancing |
|----------|-----------------|-----------------|-------------------|
| Python | Yes | Yes | Yes (uses indentation) |
| Rust | Yes | Yes | Yes |
| JavaScript | Yes | Yes | Yes |
| TypeScript | Yes | Yes | Yes |
| Go | Yes | Yes | Yes |
| Java | Yes | Yes | Yes |
| C/C++ | Yes | Yes | Yes |
| **Rholang** | Yes | Yes | Yes |
| **MeTTa** | Yes | Yes | Yes (no braces) |
| Generic | Basic | No | Yes |

### Language-Specific Features

**Rholang** (F1R3FLY.io):
- Keywords: `new`, `contract`, `for`, `match`, `select`
- Patterns: Process composition, channel operations
- Syntax: `|` for parallel, `<-` for receive

**MeTTa** (F1R3FLY.io):
- Keywords: `!`, `=`, `:`, `match`, `let`, `type`
- Patterns: S-expressions, type annotations
- Syntax: Parentheses-based, no braces

## Core Components

### CodeCorrectionLayer

The main layer that combines all correction stages:

```rust
pub struct CodeCorrectionLayer<W: Semiring, B: LatticeBackend> {
    config: CodeCorrectionConfig,
    syntax_layer: SyntaxRecoveryLayer,
    pattern_layer: Option<PatternAwareLayer>,
}

impl<W, B> CodeCorrectionLayer<W, B>
where
    W: Semiring + From<TropicalWeight>,
    B: LatticeBackend + Clone,
{
    /// Create a new layer with the given configuration.
    pub fn new(config: CodeCorrectionConfig) -> Self;

    /// Create for a specific language with defaults.
    pub fn for_language(language: &str) -> Self;

    /// Check if pattern-aware correction is enabled.
    pub fn has_patterns(&self) -> bool;
}
```

### Layer Pipeline Integration

Code correction integrates with lling-llang's layer pipeline:

```rust
use lling_llang::layers::LayerPipeline;

let mut pipeline = LayerPipeline::new();

// Add token correction (handled elsewhere)
// Add code-specific layers
pipeline.add_layer(CodeCorrectionLayer::for_language("rust"));

// Apply to lattice
let result = pipeline.apply(&input_lattice)?;
```

## Use Cases

### 1. IDE Code Completion

```rust
// User types: "fn main( )"
// Expected: "fn main() {"

let layer = CodeCorrectionLayer::for_language("rust");
let corrected = layer.apply(&partial_code_lattice)?;
// Best path includes missing "{"
```

### 2. Code Repair in Editors

```rust
// Code with syntax error
let code = "def greet(name)
    print(f'Hello {name}')";  // Missing colon

let layer = CodeCorrectionLayer::for_language("python");
// Layer adds ":" after ")" in the correction lattice
```

### 3. Transpiler Error Recovery

```rust
// When parsing fails, generate repair suggestions
let config = CodeCorrectionConfig::new("rholang")
    .with_syntax_recovery(SyntaxRecoveryConfig::default())
    .with_pattern_aware(PatternAwareConfig::rholang_patterns());

let layer = CodeCorrectionLayer::new(config);
```

### 4. Language Learning Tools

```rust
// Help learners with common mistakes
let config = CodeCorrectionConfig::new("python")
    .with_max_corrections(10)  // Show more alternatives
    .with_keyword_boost(2.0);  // Strongly prefer keywords

let layer = CodeCorrectionLayer::new(config);
```

## Configuration Overview

### CodeCorrectionConfig

```rust
pub struct CodeCorrectionConfig {
    /// Target programming language
    pub language: CodeCorrectionLanguage,

    /// Maximum corrections per token
    pub max_corrections_per_token: usize,

    /// Maximum edit distance for corrections
    pub max_edit_distance: usize,

    /// Cost weights
    pub edit_cost: f64,
    pub insertion_cost: f64,
    pub deletion_cost: f64,

    /// Boost for keyword matches
    pub keyword_boost: f64,

    /// Syntax recovery settings (optional)
    pub syntax_config: Option<SyntaxRecoveryConfig>,

    /// Pattern-aware settings (optional)
    pub pattern_config: Option<PatternAwareConfig>,
}
```

### Default Configuration

```rust
// Default settings
CodeCorrectionConfig {
    language: lang,
    max_corrections_per_token: 5,
    max_edit_distance: 2,
    edit_cost: 1.0,
    insertion_cost: 2.0,
    deletion_cost: 1.5,
    keyword_boost: 0.5,
    syntax_config: Some(SyntaxRecoveryConfig::default()),
    pattern_config: None,
    keep_original: true,
    min_token_length: 2,
}
```

## Performance

| Metric | Typical Value |
|--------|---------------|
| Lattice with 100 edges | ~1-2ms |
| Lattice with 1000 edges | ~10-20ms |
| Pattern matching | `O(n × p)` |
| Memory overhead | ~10KB per layer |

Where `n` = number of tokens, `p` = number of patterns.

## See Also

- [Syntax Recovery](syntax-recovery.md) - Error recovery layer details
- [Pattern-Aware Correction](pattern-aware.md) - Idiom-based boosting
- [Language Configuration](configuration.md) - Per-language settings

## References

- [Mohri 2002](../../BIBLIOGRAPHY.md#ref-mohri2002) — weighted finite-state
  transducers, the lattice algebra these layers operate over.
- [Mohri 2009](../../BIBLIOGRAPHY.md#ref-mohri2009) — weighted-automata algorithms
  (shortest path / best path used to read off the corrected token sequence).
- [Earley 1970](../../BIBLIOGRAPHY.md#ref-earley1970) — the context-free parsing
  algorithm the CFG-filter stage runs over the lattice.
