//! MathML type system for semantic validation.
//!
//! Defines types for mathematical expressions based on Content MathML semantics.

use std::collections::HashMap;
use std::fmt;

/// Mathematical type for expressions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MathType {
    /// Numeric value (integer, real, complex).
    Number,
    /// Variable/identifier.
    Variable,
    /// Function type with domain and codomain.
    Function {
        /// Number of arguments.
        arity: Arity,
        /// Domain type for each argument.
        domain: Vec<MathType>,
        /// Return type.
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
        /// Element type.
        element: Box<MathType>,
        /// Optional fixed dimension.
        dimension: Option<usize>,
    },
    /// Matrix type.
    Matrix {
        /// Element type.
        element: Box<MathType>,
        /// Optional dimensions (rows, cols).
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

impl MathType {
    /// Check if this type is numeric (Number, Variable that could be numeric).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            MathType::Number | MathType::Variable | MathType::TypeVar(_)
        )
    }

    /// Check if this type is a function.
    pub fn is_function(&self) -> bool {
        matches!(self, MathType::Function { .. })
    }

    /// Check if this type is an operator.
    pub fn is_operator(&self) -> bool {
        matches!(
            self,
            MathType::BinaryOp | MathType::UnaryOp | MathType::NaryOp
        )
    }

    /// Check if this type is compatible with another for unification.
    pub fn compatible_with(&self, other: &MathType) -> bool {
        match (self, other) {
            // Same types are compatible
            (a, b) if a == b => true,
            // Type variables are compatible with anything
            (MathType::TypeVar(_), _) | (_, MathType::TypeVar(_)) => true,
            // Unknown is compatible with anything
            (MathType::Unknown, _) | (_, MathType::Unknown) => true,
            // Variable can be numeric
            (MathType::Variable, MathType::Number) | (MathType::Number, MathType::Variable) => true,
            // Functions are compatible if arities match
            (MathType::Function { arity: a1, .. }, MathType::Function { arity: a2, .. }) => {
                a1 == a2
            }
            // Vectors are compatible regardless of dimension
            (MathType::Vector { .. }, MathType::Vector { .. }) => true,
            // Matrices are compatible regardless of dimension
            (MathType::Matrix { .. }, MathType::Matrix { .. }) => true,
            _ => false,
        }
    }
}

impl fmt::Display for MathType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MathType::Number => write!(f, "Number"),
            MathType::Variable => write!(f, "Var"),
            MathType::Function {
                arity,
                domain,
                codomain,
            } => {
                let args: Vec<_> = domain.iter().map(|t| t.to_string()).collect();
                write!(
                    f,
                    "({}) -> {} [arity: {:?}]",
                    args.join(", "),
                    codomain,
                    arity
                )
            }
            MathType::BinaryOp => write!(f, "BinOp"),
            MathType::UnaryOp => write!(f, "UnaryOp"),
            MathType::NaryOp => write!(f, "NaryOp"),
            MathType::Relation => write!(f, "Relation"),
            MathType::Set => write!(f, "Set"),
            MathType::Vector { element, dimension } => {
                if let Some(d) = dimension {
                    write!(f, "Vec<{}>^{}", element, d)
                } else {
                    write!(f, "Vec<{}>", element)
                }
            }
            MathType::Matrix {
                element,
                dimensions,
            } => {
                if let Some((r, c)) = dimensions {
                    write!(f, "Mat<{}>^({}x{})", element, r, c)
                } else {
                    write!(f, "Mat<{}>", element)
                }
            }
            MathType::Boolean => write!(f, "Bool"),
            MathType::Unit => write!(f, "()"),
            MathType::TypeVar(id) => write!(f, "T{}", id),
            MathType::Unknown => write!(f, "?"),
            MathType::Error(msg) => write!(f, "Error({})", msg),
        }
    }
}

/// Arity of a function or operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

impl Arity {
    /// Check if this arity accepts the given number of arguments.
    pub fn accepts(&self, n: usize) -> bool {
        match self {
            Arity::Nullary => n == 0,
            Arity::Unary => n == 1,
            Arity::Binary => n == 2,
            Arity::Ternary => n == 3,
            Arity::Variadic => true,
            Arity::Fixed(k) => n == *k,
        }
    }

    /// Get minimum required arguments.
    pub fn min_args(&self) -> usize {
        match self {
            Arity::Nullary => 0,
            Arity::Unary => 1,
            Arity::Binary => 2,
            Arity::Ternary => 3,
            Arity::Variadic => 0,
            Arity::Fixed(k) => *k,
        }
    }
}

/// Type signature for a mathematical construct.
#[derive(Debug, Clone)]
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

impl TypeSignature {
    /// Create a new type signature.
    pub fn new(name: impl Into<String>, math_type: MathType, category: SemanticCategory) -> Self {
        Self {
            name: name.into(),
            math_type,
            aliases: Vec::new(),
            category,
        }
    }

    /// Add an alias.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }
}

/// Semantic category of a mathematical construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticCategory {
    /// Arithmetic operations.
    Arithmetic,
    /// Algebraic operations.
    Algebra,
    /// Calculus operations.
    Calculus,
    /// Set theory operations.
    SetTheory,
    /// Logic operations.
    Logic,
    /// Linear algebra operations.
    LinearAlgebra,
    /// Trigonometric functions.
    Trigonometry,
    /// Constants.
    Constant,
    /// Variable/identifier.
    Variable,
    /// Delimiter/grouping.
    Delimiter,
    /// Formatting/presentation.
    Presentation,
}

/// Type environment mapping identifiers to types.
#[derive(Debug, Clone, Default)]
pub struct TypeEnvironment {
    /// Variable bindings.
    bindings: HashMap<String, MathType>,
    /// Parent environment (for scoping).
    parent: Option<Box<TypeEnvironment>>,
}

impl TypeEnvironment {
    /// Create a new empty environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a child environment.
    pub fn child(&self) -> Self {
        Self {
            bindings: HashMap::new(),
            parent: Some(Box::new(self.clone())),
        }
    }

    /// Bind a variable to a type.
    pub fn bind(&mut self, name: impl Into<String>, ty: MathType) {
        self.bindings.insert(name.into(), ty);
    }

    /// Look up a variable's type.
    pub fn lookup(&self, name: &str) -> Option<&MathType> {
        self.bindings
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|p| p.lookup(name)))
    }

    /// Check if a variable is bound.
    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }
}

/// Result of type checking.
#[derive(Debug, Clone)]
pub struct TypeResult {
    /// Inferred type.
    pub inferred_type: MathType,
    /// Any type errors found.
    pub errors: Vec<TypeError>,
    /// Warnings (non-fatal issues).
    pub warnings: Vec<TypeWarning>,
}

impl TypeResult {
    /// Create a successful result.
    pub fn ok(ty: MathType) -> Self {
        Self {
            inferred_type: ty,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create an error result.
    pub fn error(ty: MathType, error: TypeError) -> Self {
        Self {
            inferred_type: ty,
            errors: vec![error],
            warnings: Vec::new(),
        }
    }

    /// Check if type checking succeeded.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Add an error.
    pub fn with_error(mut self, error: TypeError) -> Self {
        self.errors.push(error);
        self
    }

    /// Add a warning.
    pub fn with_warning(mut self, warning: TypeWarning) -> Self {
        self.warnings.push(warning);
        self
    }
}

/// Type error.
#[derive(Debug, Clone)]
pub struct TypeError {
    /// Error kind.
    pub kind: TypeErrorKind,
    /// Position in the expression (if known).
    pub position: Option<usize>,
    /// Error message.
    pub message: String,
}

impl TypeError {
    /// Create a new type error.
    pub fn new(kind: TypeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            position: None,
            message: message.into(),
        }
    }

    /// Set position.
    pub fn at(mut self, pos: usize) -> Self {
        self.position = Some(pos);
        self
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(pos) = self.position {
            write!(f, "[{}] {:?}: {}", pos, self.kind, self.message)
        } else {
            write!(f, "{:?}: {}", self.kind, self.message)
        }
    }
}

/// Kind of type error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeErrorKind {
    /// Type mismatch.
    TypeMismatch,
    /// Wrong number of arguments.
    ArityMismatch,
    /// Undefined variable.
    UndefinedVariable,
    /// Invalid operator application.
    InvalidOperator,
    /// Division by zero.
    DivisionByZero,
    /// Invalid expression structure.
    InvalidStructure,
    /// Ambiguous type.
    AmbiguousType,
}

/// Type warning (non-fatal issue).
#[derive(Debug, Clone)]
pub struct TypeWarning {
    /// Warning kind.
    pub kind: TypeWarningKind,
    /// Position in the expression.
    pub position: Option<usize>,
    /// Warning message.
    pub message: String,
}

impl TypeWarning {
    /// Create a new warning.
    pub fn new(kind: TypeWarningKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            position: None,
            message: message.into(),
        }
    }
}

/// Kind of type warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeWarningKind {
    /// Implicit type coercion.
    ImplicitCoercion,
    /// Unused variable.
    UnusedVariable,
    /// Potential ambiguity.
    Ambiguity,
    /// Deprecated construct.
    Deprecated,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_math_type_display() {
        assert_eq!(format!("{}", MathType::Number), "Number");
        assert_eq!(format!("{}", MathType::Variable), "Var");
        assert_eq!(format!("{}", MathType::BinaryOp), "BinOp");
    }

    #[test]
    fn test_math_type_is_numeric() {
        assert!(MathType::Number.is_numeric());
        assert!(MathType::Variable.is_numeric());
        assert!(!MathType::Set.is_numeric());
    }

    #[test]
    fn test_math_type_compatible() {
        assert!(MathType::Number.compatible_with(&MathType::Number));
        assert!(MathType::Number.compatible_with(&MathType::Variable));
        assert!(MathType::TypeVar(0).compatible_with(&MathType::Set));
        assert!(!MathType::Set.compatible_with(&MathType::Number));
    }

    #[test]
    fn test_arity_accepts() {
        assert!(Arity::Nullary.accepts(0));
        assert!(!Arity::Nullary.accepts(1));
        assert!(Arity::Unary.accepts(1));
        assert!(Arity::Binary.accepts(2));
        assert!(Arity::Variadic.accepts(5));
        assert!(Arity::Fixed(3).accepts(3));
        assert!(!Arity::Fixed(3).accepts(2));
    }

    #[test]
    fn test_type_environment() {
        let mut env = TypeEnvironment::new();
        env.bind("x", MathType::Number);
        env.bind(
            "f",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Number],
                codomain: Box::new(MathType::Number),
            },
        );

        assert_eq!(env.lookup("x"), Some(&MathType::Number));
        assert!(env.lookup("f").is_some());
        assert!(env.lookup("y").is_none());
    }

    #[test]
    fn test_type_environment_scoping() {
        let mut parent = TypeEnvironment::new();
        parent.bind("x", MathType::Number);

        let mut child = parent.child();
        child.bind("y", MathType::Variable);

        // Child can see parent's bindings
        assert!(child.lookup("x").is_some());
        assert!(child.lookup("y").is_some());

        // Parent cannot see child's bindings
        assert!(parent.lookup("y").is_none());
    }

    #[test]
    fn test_type_result() {
        let ok = TypeResult::ok(MathType::Number);
        assert!(ok.is_ok());

        let err = TypeResult::error(
            MathType::Error("test".to_string()),
            TypeError::new(TypeErrorKind::TypeMismatch, "mismatch"),
        );
        assert!(!err.is_ok());
    }

    #[test]
    fn test_type_signature() {
        let sig = TypeSignature::new(
            "sin",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Trigonometry,
        )
        .with_alias("sine");

        assert_eq!(sig.name, "sin");
        assert_eq!(sig.aliases, vec!["sine"]);
        assert_eq!(sig.category, SemanticCategory::Trigonometry);
    }
}
