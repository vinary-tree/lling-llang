# MeTTaIL Type Layer

The MeTTaIL Type Layer filters lattice paths based on semantic type constraints from the Operational Semantic Logic Framework (OSLF).

## Overview

**MeTTaIL** (MeTTa Intermediate Language) is a typed intermediate representation used in the F1R3FLY.io ecosystem. The `MeTTaILTypeLayer` uses MeTTaIL's type system to:

- **Infer types**: Determine semantic types for tokens
- **Apply constraints**: Filter paths that violate type requirements
- **Soft filtering**: Downweight (rather than reject) type mismatches
- **Position-specific**: Apply different constraints at different positions

The MeTTaIL type layer is the **first** F1R3FLY filter in the integration flow: it
narrows a candidate lattice to type-consistent paths before MORK rules, MeTTaTron
transforms, and Rholang parallelism downstream.

![F1R3FLY layer integration overview: a candidate lattice is type-filtered by the MeTTaIL layer, rule-filtered by MORK, rewritten by MeTTaTron-compiled specs, and parallelized by the Rholang layer, with PathMap as the content-addressed substrate beneath; the output is a pruned, reweighted lattice.](../../diagrams/integration/mettail-mork-rholang.svg)

*Green = the candidate lattice; amber = the MeTTaIL/MORK type-and-rule filters;
purple = MeTTaTron compilation and Rholang concurrency; blue = the PathMap
substrate; grey = the final lattice. All four layers are forward-looking
integration **targets**. Dotted edges are cross-layer dependencies.*

<details><summary>Text view</summary>

```text
candidate lattice
      │  all paths
      ▼
[ MeTTaIL type layer ]  ── type-consistent paths ──▶ [ MORK rule layer ]
                                                            │ rule-valid paths
                                                            ▼
   pruned + reweighted  ◀── merged regions ── [ Rholang ] ◀── [ MeTTaTron ]
        lattice                                    │  compiled pass
                                                   ▼
                                  PathMap (persist · share by hash)
```

</details>

## Type System

### Type Expressions

```rust
pub enum TypeExpr {
    /// Base type (e.g., "String", "Noun", "Verb")
    Base(String),

    /// Function type (input -> output)
    Function(Box<TypeExpr>, Box<TypeExpr>),

    /// Type variable (for polymorphism)
    Variable(String),

    /// Type application (e.g., "List String")
    Application(Box<TypeExpr>, Vec<TypeExpr>),
}
```

### Creating Types

```rust
use lling_llang::layers::{TypeExpr};

// Base types
let noun = TypeExpr::base("Noun");
let verb = TypeExpr::base("Verb");
let number = TypeExpr::base("Number");

// Function types
let noun_to_verb = TypeExpr::function(
    TypeExpr::base("Noun"),
    TypeExpr::base("Verb")
);

// Type variables
let any = TypeExpr::variable("a");

// Type applications
let list_of_nouns = TypeExpr::Application(
    Box::new(TypeExpr::base("List")),
    vec![TypeExpr::base("Noun")]
);
```

### Type Constraints

```rust
pub struct TypeConstraint {
    /// Position in token sequence (None = any position)
    pub position: Option<usize>,

    /// Required type
    pub required_type: TypeExpr,

    /// Strict (reject) or soft (downweight)
    pub strict: bool,
}
```

Creating constraints:

```rust
use lling_llang::layers::TypeConstraint;

// Strict: position 0 must be a Noun
let c1 = TypeConstraint::strict(TypeExpr::base("Noun"))
    .at_position(0);

// Soft: position 1 should be a Verb (downweight if not)
let c2 = TypeConstraint::soft(TypeExpr::base("Verb"))
    .at_position(1);

// Any position: sequence must contain a Number
let c3 = TypeConstraint::strict(TypeExpr::base("Number"));
```

## Type Checker Trait

### Interface

```rust
pub trait TypeChecker: Send + Sync {
    /// Infer the type of a token.
    fn infer_type(&self, token: &str) -> Option<TypeExpr>;

    /// Check if a token satisfies a type constraint.
    fn check_type(&self, token: &str, expected: &TypeExpr) -> bool;

    /// Check if a token sequence satisfies constraints.
    fn check_sequence(&self, tokens: &[&str], constraints: &[TypeConstraint]) -> bool;
}
```

### Default Implementation

The `check_sequence` method has a default implementation:

```rust
fn check_sequence(&self, tokens: &[&str], constraints: &[TypeConstraint]) -> bool {
    constraints.iter().all(|c| {
        match c.position {
            Some(pos) => {
                if pos < tokens.len() {
                    self.check_type(tokens[pos], &c.required_type)
                } else {
                    !c.strict  // Missing position: fail if strict
                }
            }
            None => {
                // Any position: at least one token must satisfy
                tokens.iter().any(|t| self.check_type(t, &c.required_type))
            }
        }
    })
}
```

### Custom Type Checker

```rust
use lling_llang::layers::{TypeChecker, TypeExpr};

struct PosTagTypeChecker {
    // POS tagging model
    tagger: PosTagger,
}

impl TypeChecker for PosTagTypeChecker {
    fn infer_type(&self, token: &str) -> Option<TypeExpr> {
        let tag = self.tagger.tag(token)?;
        Some(match tag {
            "NN" | "NNS" | "NNP" | "NNPS" => TypeExpr::base("Noun"),
            "VB" | "VBD" | "VBG" | "VBN" | "VBP" | "VBZ" => TypeExpr::base("Verb"),
            "JJ" | "JJR" | "JJS" => TypeExpr::base("Adjective"),
            "RB" | "RBR" | "RBS" => TypeExpr::base("Adverb"),
            "DT" => TypeExpr::base("Determiner"),
            _ => TypeExpr::base("Other"),
        })
    }

    fn check_type(&self, token: &str, expected: &TypeExpr) -> bool {
        self.infer_type(token)
            .map(|t| t == *expected)
            .unwrap_or(false)
    }
}
```

## Using the Layer

### Basic Usage

```rust
use lling_llang::layers::{MeTTaILTypeLayer, TypeConstraint, TypeExpr};

// Create type checker
let checker = Box::new(MyTypeChecker::new());

// Create layer with constraints
let layer = MeTTaILTypeLayer::new(checker)
    .with_constraint(TypeConstraint::strict(TypeExpr::base("Determiner")).at_position(0))
    .with_constraint(TypeConstraint::strict(TypeExpr::base("Noun")).at_position(1))
    .with_constraint(TypeConstraint::strict(TypeExpr::base("Verb")).at_position(2));

// Apply to lattice
let filtered = layer.apply(&lattice)?;
```

### In a Pipeline

```rust
use lling_llang::layers::{LayerPipelineBuilder, CfgFilterLayer, MeTTaILTypeLayer};

let pipeline = LayerPipelineBuilder::new()
    // First: spelling candidates
    .add_layer(SpellingLayer::new())
    // Then: grammar filter
    .add_layer(CfgFilterLayer::new(&grammar))
    // Then: semantic type filter
    .add_layer(MeTTaILTypeLayer::new(checker)
        .with_constraint(TypeConstraint::strict(TypeExpr::base("Noun")).at_position(0)))
    .build();

let result = pipeline.apply(&lattice)?;
```

### Layer Properties

```rust
impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for MeTTaILTypeLayer {
    fn name(&self) -> &str {
        "mettail-types"
    }

    fn estimated_reduction(&self) -> f64 {
        0.2  // Type filtering typically reduces to ~20% of paths
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true  // Can apply to any lattice
    }
}
```

## Type Inference Strategies

### Lexicon-Based

Look up types in a dictionary:

```rust
struct LexiconTypeChecker {
    lexicon: HashMap<String, TypeExpr>,
}

impl TypeChecker for LexiconTypeChecker {
    fn infer_type(&self, token: &str) -> Option<TypeExpr> {
        self.lexicon.get(token).cloned()
    }
}
```

### Pattern-Based

Infer types from token patterns:

```rust
struct PatternTypeChecker;

impl TypeChecker for PatternTypeChecker {
    fn infer_type(&self, token: &str) -> Option<TypeExpr> {
        if token.chars().all(|c| c.is_numeric()) {
            Some(TypeExpr::base("Number"))
        } else if token.chars().next()?.is_uppercase() {
            Some(TypeExpr::base("ProperNoun"))
        } else if token.ends_with("ly") {
            Some(TypeExpr::base("Adverb"))
        } else if token.ends_with("ing") {
            Some(TypeExpr::base("Gerund"))
        } else {
            None
        }
    }
}
```

### Contextual (Planned)

Use context for disambiguation:

```rust
struct ContextualTypeChecker {
    model: ContextualModel,
}

impl TypeChecker for ContextualTypeChecker {
    fn infer_type(&self, token: &str) -> Option<TypeExpr> {
        // Single-token inference (limited)
        self.model.infer_standalone(token)
    }

    fn check_sequence(&self, tokens: &[&str], constraints: &[TypeConstraint]) -> bool {
        // Use full context for better inference
        let types = self.model.infer_sequence(tokens);
        // Check constraints against inferred types
        // ...
    }
}
```

## OSLF Integration

### Operational Semantic Logic Framework

OSLF provides a formal foundation for MeTTaIL types:

- **Constructors**: Build type expressions
- **Eliminators**: Decompose type expressions
- **Axioms**: Define type relationships
- **Inference rules**: Derive new type judgments

### Type Relationships (Planned)

```rust
trait OslfTypeChecker: TypeChecker {
    /// Check if one type is a subtype of another.
    fn is_subtype(&self, sub: &TypeExpr, sup: &TypeExpr) -> bool;

    /// Unify two types, returning the unified type if possible.
    fn unify(&self, t1: &TypeExpr, t2: &TypeExpr) -> Option<TypeExpr>;

    /// Apply type substitution.
    fn substitute(&self, expr: &TypeExpr, var: &str, replacement: &TypeExpr) -> TypeExpr;
}
```

## Current Status

**Status**: Implemented

The `MeTTaILTypeLayer` is available behind the `f1r3fly` feature:

- Type expression data structures are represented by `TypeExpr`.
- Type constraints are represented by `TypeConstraint`.
- Type checking is provided through the `TypeChecker` trait.
- Lattice filtering applies type checks to tokenized paths and preserves paths
  accepted by the configured checker.

### Implementation Structure

1. **Path extraction**: Extract all paths from lattice
2. **Type inference**: Infer types for each path's tokens
3. **Constraint checking**: Check paths against constraints
4. **Filtering**: Remove paths that fail strict constraints
5. **Reweighting**: Downweight paths that fail soft constraints
6. **Lattice reconstruction**: Build filtered lattice

## Related Integration Points

- [Vision](vision.md): F1R3FLY.io integration overview
- [MORK Layer](mork-layer.md): Rule-based filtering
- [Layers](../../architecture/layers.md): Layer architecture
