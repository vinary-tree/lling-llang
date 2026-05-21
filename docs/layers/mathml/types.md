# MathML Type System

The type system defines mathematical types for expressions based on Content MathML semantics.

## Mathematical Types

```rust
pub enum MathType {
    /// Numeric value (integer, real, complex).
    Number,
    /// Variable/identifier.
    Variable,
    /// Function type with domain and codomain.
    Function {
        arity: Arity,
        domain: Vec<MathType>,
        codomain: Box<MathType>,
    },
    /// Binary operator.
    BinaryOp,
    /// Unary operator.
    UnaryOp,
    /// N-ary operator (summation, product, etc.).
    NaryOp,
    /// Relation (equals, less than, etc.).
    Relation,
    /// Set type.
    Set,
    /// Vector type.
    Vector {
        element: Box<MathType>,
        dimension: Option<usize>,
    },
    /// Matrix type.
    Matrix {
        element: Box<MathType>,
        dimensions: Option<(usize, usize)>,
    },
    /// Boolean type.
    Boolean,
    /// Unit type (for side-effect operations).
    Unit,
    /// Type variable for inference.
    TypeVar(u32),
    /// Unknown/unresolved type.
    Unknown,
    /// Error type (for invalid expressions).
    Error(String),
}
```

## Type Properties

### Numeric Check

```rust
impl MathType {
    pub fn is_numeric(&self) -> bool {
        matches!(self, MathType::Number | MathType::Variable | MathType::TypeVar(_))
    }
}

// Usage
assert!(MathType::Number.is_numeric());
assert!(MathType::Variable.is_numeric());
assert!(!MathType::Set.is_numeric());
```

### Function Check

```rust
impl MathType {
    pub fn is_function(&self) -> bool {
        matches!(self, MathType::Function { .. })
    }
}
```

### Operator Check

```rust
impl MathType {
    pub fn is_operator(&self) -> bool {
        matches!(self, MathType::BinaryOp | MathType::UnaryOp | MathType::NaryOp)
    }
}
```

### Type Compatibility

```rust
impl MathType {
    pub fn compatible_with(&self, other: &MathType) -> bool {
        // Logic handles type variables, unknown, and structural matching
    }
}

// Examples
assert!(MathType::Number.compatible_with(&MathType::Number));
assert!(MathType::Number.compatible_with(&MathType::Variable));
assert!(MathType::TypeVar(0).compatible_with(&MathType::Set));
assert!(!MathType::Set.compatible_with(&MathType::Number));
```

## Arity

```rust
pub enum Arity {
    /// No arguments (constant).
    Nullary,
    /// One argument.
    Unary,
    /// Two arguments.
    Binary,
    /// Three arguments.
    Ternary,
    /// Variable number of arguments.
    Variadic,
    /// Specific number of arguments.
    Fixed(usize),
}
```

### Arity Validation

```rust
impl Arity {
    pub fn accepts(&self, n: usize) -> bool;
    pub fn min_args(&self) -> usize;
}

// Examples
assert!(Arity::Nullary.accepts(0));
assert!(!Arity::Nullary.accepts(1));
assert!(Arity::Unary.accepts(1));
assert!(Arity::Binary.accepts(2));
assert!(Arity::Variadic.accepts(5));
assert!(Arity::Fixed(3).accepts(3));
```

## Type Signatures

```rust
pub struct TypeSignature {
    /// Name of the construct.
    pub name: String,
    /// The type.
    pub math_type: MathType,
    /// Alternative names (aliases).
    pub aliases: Vec<String>,
    /// Semantic category.
    pub category: SemanticCategory,
}
```

### Creating Type Signatures

```rust
use lling_llang::layers::mathml::types::{
    TypeSignature, MathType, Arity, SemanticCategory
};

// Function signature: sin : Number -> Number
let sin_sig = TypeSignature::new(
    "\\sin",
    MathType::Function {
        arity: Arity::Unary,
        domain: vec![MathType::Number],
        codomain: Box::new(MathType::Number),
    },
    SemanticCategory::Trigonometry,
).with_alias("sine");

assert_eq!(sin_sig.name, "\\sin");
assert_eq!(sin_sig.aliases, vec!["sine"]);
```

## Semantic Categories

```rust
pub enum SemanticCategory {
    Arithmetic,      // +, -, *, /, ^
    Algebra,         // Algebraic operations
    Calculus,        // ∫, ∑, ∏, lim
    SetTheory,       // ∪, ∩, ∈, ⊂
    Logic,           // ∧, ∨, ¬, →
    LinearAlgebra,   // det, tr, matrix ops
    Trigonometry,    // sin, cos, tan
    Constant,        // π, e, ∞
    Variable,        // Greek letters
    Delimiter,       // (, ), {, }
    Presentation,    // Formatting
}
```

## Type Environment

The type environment maps identifiers to types with lexical scoping:

```rust
pub struct TypeEnvironment {
    bindings: HashMap<String, MathType>,
    parent: Option<Box<TypeEnvironment>>,
}
```

### Basic Operations

```rust
use lling_llang::layers::mathml::types::{TypeEnvironment, MathType};

let mut env = TypeEnvironment::new();

// Bind variables
env.bind("x", MathType::Number);
env.bind("f", MathType::Function {
    arity: Arity::Unary,
    domain: vec![MathType::Number],
    codomain: Box::new(MathType::Number),
});

// Lookup
assert_eq!(env.lookup("x"), Some(&MathType::Number));
assert!(env.lookup("y").is_none());

// Check existence
assert!(env.contains("x"));
assert!(!env.contains("y"));
```

### Scoping

```rust
let mut parent = TypeEnvironment::new();
parent.bind("x", MathType::Number);

let mut child = parent.child();
child.bind("y", MathType::Variable);

// Child can see parent's bindings
assert!(child.lookup("x").is_some());
assert!(child.lookup("y").is_some());

// Parent cannot see child's bindings
assert!(parent.lookup("y").is_none());
```

## Type Results

```rust
pub struct TypeResult {
    /// Inferred type.
    pub inferred_type: MathType,
    /// Any type errors found.
    pub errors: Vec<TypeError>,
    /// Warnings (non-fatal issues).
    pub warnings: Vec<TypeWarning>,
}
```

### Creating Results

```rust
use lling_llang::layers::mathml::types::{TypeResult, MathType, TypeError, TypeErrorKind};

// Successful result
let ok = TypeResult::ok(MathType::Number);
assert!(ok.is_ok());

// Error result
let err = TypeResult::error(
    MathType::Error("test".to_string()),
    TypeError::new(TypeErrorKind::TypeMismatch, "mismatch"),
);
assert!(!err.is_ok());
```

### Adding Issues

```rust
let result = TypeResult::ok(MathType::Number)
    .with_error(TypeError::new(TypeErrorKind::ArityMismatch, "wrong args"))
    .with_warning(TypeWarning::new(TypeWarningKind::ImplicitCoercion, "coerced"));
```

## Type Errors

```rust
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub position: Option<usize>,
    pub message: String,
}

pub enum TypeErrorKind {
    TypeMismatch,       // Type incompatibility
    ArityMismatch,      // Wrong number of arguments
    UndefinedVariable,  // Unknown identifier
    InvalidOperator,    // Invalid operator application
    DivisionByZero,     // Division by zero
    InvalidStructure,   // Malformed expression
    AmbiguousType,      // Cannot resolve type
}
```

### Creating Errors

```rust
use lling_llang::layers::mathml::types::{TypeError, TypeErrorKind};

let error = TypeError::new(TypeErrorKind::TypeMismatch, "expected Number, got Set")
    .at(5);  // At position 5

println!("{}", error);  // "[5] TypeMismatch: expected Number, got Set"
```

## Type Warnings

```rust
pub struct TypeWarning {
    pub kind: TypeWarningKind,
    pub position: Option<usize>,
    pub message: String,
}

pub enum TypeWarningKind {
    ImplicitCoercion,  // Automatic type conversion
    UnusedVariable,    // Defined but not used
    Ambiguity,         // Potential ambiguity
    Deprecated,        // Deprecated construct
}
```

## Display Formatting

All types implement `Display` for readable output:

```rust
let num = MathType::Number;
println!("{}", num);  // "Number"

let func = MathType::Function {
    arity: Arity::Binary,
    domain: vec![MathType::Number, MathType::Number],
    codomain: Box::new(MathType::Number),
};
println!("{}", func);  // "(Number, Number) -> Number [arity: Binary]"

let vec = MathType::Vector {
    element: Box::new(MathType::Number),
    dimension: Some(3),
};
println!("{}", vec);  // "Vec<Number>^3"

let mat = MathType::Matrix {
    element: Box::new(MathType::Number),
    dimensions: Some((2, 3)),
};
println!("{}", mat);  // "Mat<Number>^(2x3)"
```

## Related

- [Overview](./overview.md): Layer architecture
- [Checker](./checker.md): Type checking
- [Homoglyph](./homoglyph.md): Glyph disambiguation
