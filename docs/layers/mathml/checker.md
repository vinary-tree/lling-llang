# MathML Type Checker

The type checker performs type inference and checking on mathematical expressions based on Content MathML semantics.

## Type Checker Structure

```rust
pub struct MathTypeChecker {
    signatures: HashMap<String, TypeSignature>,
    next_type_var: u32,
    config: TypeCheckerConfig,
}
```

## Configuration

```rust
pub struct TypeCheckerConfig {
    /// Whether to allow implicit coercions.
    pub allow_coercion: bool,
    /// Whether to infer types for undefined variables.
    pub infer_undefined: bool,
    /// Whether to check arity strictly.
    pub strict_arity: bool,
    /// Whether to warn about ambiguous constructs.
    pub warn_ambiguous: bool,
}
```

### Default Configuration

```rust
TypeCheckerConfig {
    allow_coercion: true,
    infer_undefined: true,
    strict_arity: true,
    warn_ambiguous: true,
}
```

## Creating a Type Checker

```rust
use lling_llang::layers::mathml::checker::{MathTypeChecker, TypeCheckerConfig};

// Default configuration
let checker = MathTypeChecker::new();

// Custom configuration
let config = TypeCheckerConfig {
    infer_undefined: false,
    strict_arity: true,
    ..Default::default()
};
let checker = MathTypeChecker::with_config(config);
```

## Built-in Type Signatures

The type checker registers standard mathematical signatures automatically:

### Arithmetic Operators

| Signature | Type | Category |
|-----------|------|----------|
| `+`, `plus` | BinaryOp | Arithmetic |
| `-`, `minus` | BinaryOp | Arithmetic |
| `*`, `times` | BinaryOp | Arithmetic |
| `/`, `divide` | BinaryOp | Arithmetic |
| `^`, `power` | BinaryOp | Arithmetic |

### LaTeX Commands

| Command | Type | Description |
|---------|------|-------------|
| `\frac` | (Number, Number) -> Number | Fraction |
| `\sqrt` | (Number) -> Number | Square root |
| `\binom` | (Number, Number) -> Number | Binomial coefficient |

### Trigonometric Functions

| Command | Type |
|---------|------|
| `\sin`, `\cos`, `\tan` | (Number) -> Number |
| `\cot`, `\sec`, `\csc` | (Number) -> Number |
| `\arcsin`, `\arccos`, `\arctan` | (Number) -> Number |
| `\sinh`, `\cosh`, `\tanh` | (Number) -> Number |

### Logarithms and Exponentials

| Command | Type |
|---------|------|
| `\log` | (Number) -> Number |
| `\ln` | (Number) -> Number |
| `\exp` | (Number) -> Number |

### N-ary Operators

| Command | Type | Category |
|---------|------|----------|
| `\sum` | NaryOp | Calculus |
| `\prod` | NaryOp | Calculus |
| `\int` | NaryOp | Calculus |
| `\lim` | (Number) -> Number | Calculus |

### Relations

| Symbol | Type |
|--------|------|
| `=`, `\neq`, `\equiv` | Relation |
| `<`, `>`, `\leq`, `\geq` | Relation |
| `\approx` | Relation |

### Logic Operators

| Command | Type |
|---------|------|
| `\land` | (Boolean, Boolean) -> Boolean |
| `\lor` | (Boolean, Boolean) -> Boolean |
| `\lnot` | (Boolean) -> Boolean |
| `\implies`, `\iff` | (Boolean, Boolean) -> Boolean |

### Set Operations

| Command | Type |
|---------|------|
| `\cup`, `\cap`, `\setminus` | BinaryOp |
| `\in`, `\subset`, `\subseteq` | Relation |

### Constants

| Command | Type |
|---------|------|
| `\pi`, `\e`, `\infty`, `\emptyset` | Number |

### Greek Letters

All Greek letters (`\alpha`, `\beta`, ..., `\omega`) are registered as `Variable` type.

### Linear Algebra

| Command | Type |
|---------|------|
| `\det` | (Matrix) -> Number |
| `\tr` | (Matrix) -> Number |

## Type Checking

### Basic Check

```rust
use lling_llang::layers::mathml::checker::MathTypeChecker;

let mut checker = MathTypeChecker::new();

// Check a number
let result = checker.check(&["42"]);
assert!(result.is_ok());
assert_eq!(result.inferred_type, MathType::Number);

// Check a known function
let result = checker.check(&["\\sin", "{", "x", "}"]);
assert!(result.is_ok());
```

### Check with Environment

```rust
use lling_llang::layers::mathml::types::TypeEnvironment;

let mut checker = MathTypeChecker::new();
let mut env = TypeEnvironment::new();

// Pre-bind variables
env.bind("x", MathType::Number);
env.bind("y", MathType::Number);

let result = checker.check_with_env(&["x", "+", "y"], &mut env);
assert!(result.is_ok());
```

### Lookup Signatures

```rust
let checker = MathTypeChecker::new();

if let Some(sig) = checker.lookup("\\sin") {
    println!("Name: {}", sig.name);
    println!("Type: {}", sig.math_type);
    println!("Category: {:?}", sig.category);
}
```

## Type Unification

The type checker implements Hindley-Milner style type unification:

```rust
let mut checker = MathTypeChecker::new();

// Same types unify
let result = checker.unify(&MathType::Number, &MathType::Number);
assert_eq!(result.unwrap(), MathType::Number);

// Type variables unify with any type
let result = checker.unify(&MathType::TypeVar(0), &MathType::Number);
assert_eq!(result.unwrap(), MathType::Number);

// Unknown unifies with any type
let result = checker.unify(&MathType::Unknown, &MathType::Set);
assert_eq!(result.unwrap(), MathType::Set);

// Variable can be Number
let result = checker.unify(&MathType::Variable, &MathType::Number);
assert_eq!(result.unwrap(), MathType::Number);

// Incompatible types fail
let result = checker.unify(&MathType::Set, &MathType::Number);
assert!(result.is_err());
```

### Vector Unification

```rust
let v1 = MathType::Vector {
    element: Box::new(MathType::Number),
    dimension: Some(3),
};
let v2 = MathType::Vector {
    element: Box::new(MathType::Number),
    dimension: Some(3),
};

let result = checker.unify(&v1, &v2);
assert!(result.is_ok());

// Dimension mismatch fails
let v3 = MathType::Vector {
    element: Box::new(MathType::Number),
    dimension: Some(4),
};
let result = checker.unify(&v1, &v3);
assert!(result.is_err());
```

### Matrix Unification

```rust
let m1 = MathType::Matrix {
    element: Box::new(MathType::Number),
    dimensions: Some((2, 3)),
};
let m2 = MathType::Matrix {
    element: Box::new(MathType::Number),
    dimensions: Some((2, 3)),
};

let result = checker.unify(&m1, &m2);
assert!(result.is_ok());
```

## Fresh Type Variables

Generate unique type variables for inference:

```rust
let mut checker = MathTypeChecker::new();

let t1 = checker.fresh_type_var();
let t2 = checker.fresh_type_var();

match (t1, t2) {
    (MathType::TypeVar(id1), MathType::TypeVar(id2)) => {
        assert_ne!(id1, id2);  // Each variable has unique ID
    }
    _ => panic!("Expected TypeVar"),
}
```

## Checking Fractions

Special handling for `\frac` with division by zero detection:

```rust
let mut checker = MathTypeChecker::new();
let mut env = TypeEnvironment::new();

// Valid fraction
let result = checker.check_frac(&["x"], &["y"], &mut env);
assert!(result.is_ok());

// Division by zero
let result = checker.check_frac(&["1"], &["0"], &mut env);
assert!(!result.is_ok());
assert!(result.errors.iter().any(|e| e.kind == TypeErrorKind::DivisionByZero));
```

## Arity Checking

The checker validates function arities when `strict_arity` is enabled:

```rust
let config = TypeCheckerConfig {
    strict_arity: true,
    ..Default::default()
};
let mut checker = MathTypeChecker::with_config(config);

// \sin without argument - arity mismatch
let result = checker.check(&["\\sin"]);
// May report ArityMismatch error

// \sin with argument - valid
let result = checker.check(&["\\sin", "{", "x", "}"]);
assert!(result.is_ok());
```

## Undefined Variable Handling

Behavior depends on `infer_undefined` configuration:

```rust
// With inference (default)
let mut checker = MathTypeChecker::new();
let result = checker.check(&["undefined_var"]);
// Infers type and adds warning

// Without inference
let config = TypeCheckerConfig {
    infer_undefined: false,
    ..Default::default()
};
let mut checker = MathTypeChecker::with_config(config);
let result = checker.check(&["undefined_var"]);
assert!(!result.is_ok());
assert!(result.errors.iter().any(|e| e.kind == TypeErrorKind::UndefinedVariable));
```

## Registering Custom Signatures

```rust
use lling_llang::layers::mathml::types::{TypeSignature, MathType, Arity, SemanticCategory};

let mut checker = MathTypeChecker::new();

// Register a custom function
checker.register_signature(TypeSignature::new(
    "\\myfunc",
    MathType::Function {
        arity: Arity::Binary,
        domain: vec![MathType::Number, MathType::Number],
        codomain: Box::new(MathType::Number),
    },
    SemanticCategory::Arithmetic,
).with_alias("myfunction"));

// Now it can be checked
assert!(checker.lookup("\\myfunc").is_some());
assert!(checker.lookup("myfunction").is_some());
```

## Error Handling

```rust
let mut checker = MathTypeChecker::new();
let result = checker.check(&["\\frac", "{", "a", "}", "{", "0", "}"]);

for error in &result.errors {
    match error.kind {
        TypeErrorKind::TypeMismatch => {
            println!("Type mismatch: {}", error.message);
        }
        TypeErrorKind::ArityMismatch => {
            println!("Wrong number of arguments: {}", error.message);
        }
        TypeErrorKind::DivisionByZero => {
            println!("Division by zero detected");
        }
        _ => {}
    }
}
```

## Related

- [Overview](./overview.md): Layer architecture
- [Types](./types.md): Type system
- [Homoglyph](./homoglyph.md): Glyph disambiguation
