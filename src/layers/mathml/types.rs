//! MathML type system for semantic validation.
//!
//! Defines types for mathematical expressions based on Content MathML semantics.

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Mathematical type for expressions.
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

impl Clone for MathType {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a MathType),
            Function(Arity, usize),
            Vector(Option<usize>),
            Matrix(Option<(usize, usize)>),
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(math_type) => match math_type {
                    MathType::Number => values.push(MathType::Number),
                    MathType::Variable => values.push(MathType::Variable),
                    MathType::Function {
                        arity,
                        domain,
                        codomain,
                    } => {
                        tasks.push(Task::Function(*arity, domain.len()));
                        tasks.push(Task::Clone(codomain));
                        tasks.extend(domain.iter().rev().map(Task::Clone));
                    }
                    MathType::BinaryOp => values.push(MathType::BinaryOp),
                    MathType::UnaryOp => values.push(MathType::UnaryOp),
                    MathType::NaryOp => values.push(MathType::NaryOp),
                    MathType::Relation => values.push(MathType::Relation),
                    MathType::Set => values.push(MathType::Set),
                    MathType::Vector { element, dimension } => {
                        tasks.push(Task::Vector(*dimension));
                        tasks.push(Task::Clone(element));
                    }
                    MathType::Matrix {
                        element,
                        dimensions,
                    } => {
                        tasks.push(Task::Matrix(*dimensions));
                        tasks.push(Task::Clone(element));
                    }
                    MathType::Boolean => values.push(MathType::Boolean),
                    MathType::Unit => values.push(MathType::Unit),
                    MathType::TypeVar(id) => values.push(MathType::TypeVar(*id)),
                    MathType::Unknown => values.push(MathType::Unknown),
                    MathType::Error(message) => values.push(MathType::Error(message.clone())),
                },
                Task::Function(arity, domain_len) => {
                    let codomain = values.pop().expect("function codomain clone is present");
                    let first = values
                        .len()
                        .checked_sub(domain_len)
                        .expect("all function domain clones are present");
                    let domain = values.split_off(first);
                    values.push(MathType::Function {
                        arity,
                        domain,
                        codomain: Box::new(codomain),
                    });
                }
                Task::Vector(dimension) => {
                    let element = values.pop().expect("vector element clone is present");
                    values.push(MathType::Vector {
                        element: Box::new(element),
                        dimension,
                    });
                }
                Task::Matrix(dimensions) => {
                    let element = values.pop().expect("matrix element clone is present");
                    values.push(MathType::Matrix {
                        element: Box::new(element),
                        dimensions,
                    });
                }
            }
        }
        values.pop().expect("the root math type produces one clone")
    }
}

impl PartialEq for MathType {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (MathType::Number, MathType::Number)
                | (MathType::Variable, MathType::Variable)
                | (MathType::BinaryOp, MathType::BinaryOp)
                | (MathType::UnaryOp, MathType::UnaryOp)
                | (MathType::NaryOp, MathType::NaryOp)
                | (MathType::Relation, MathType::Relation)
                | (MathType::Set, MathType::Set)
                | (MathType::Boolean, MathType::Boolean)
                | (MathType::Unit, MathType::Unit)
                | (MathType::Unknown, MathType::Unknown) => {}
                (
                    MathType::Function {
                        arity: la,
                        domain: ld,
                        codomain: lc,
                    },
                    MathType::Function {
                        arity: ra,
                        domain: rd,
                        codomain: rc,
                    },
                ) if la == ra && ld.len() == rd.len() => {
                    pending.push((lc, rc));
                    pending.extend(ld.iter().zip(rd).rev());
                }
                (
                    MathType::Vector {
                        element: le,
                        dimension: ld,
                    },
                    MathType::Vector {
                        element: re,
                        dimension: rd,
                    },
                ) if ld == rd => pending.push((le, re)),
                (
                    MathType::Matrix {
                        element: le,
                        dimensions: ld,
                    },
                    MathType::Matrix {
                        element: re,
                        dimensions: rd,
                    },
                ) if ld == rd => pending.push((le, re)),
                (MathType::TypeVar(left), MathType::TypeVar(right)) if left == right => {}
                (MathType::Error(left), MathType::Error(right)) if left == right => {}
                _ => return false,
            }
        }
        true
    }
}

impl Eq for MathType {}

impl Hash for MathType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(math_type) = pending.pop() {
            std::mem::discriminant(math_type).hash(state);
            match math_type {
                MathType::Function {
                    arity,
                    domain,
                    codomain,
                } => {
                    arity.hash(state);
                    domain.len().hash(state);
                    pending.push(codomain);
                    pending.extend(domain.iter().rev());
                }
                MathType::Vector { element, dimension } => {
                    pending.push(element);
                    dimension.hash(state);
                }
                MathType::Matrix {
                    element,
                    dimensions,
                } => {
                    pending.push(element);
                    dimensions.hash(state);
                }
                MathType::TypeVar(id) => id.hash(state),
                MathType::Error(message) => message.hash(state),
                MathType::Number
                | MathType::Variable
                | MathType::BinaryOp
                | MathType::UnaryOp
                | MathType::NaryOp
                | MathType::Relation
                | MathType::Set
                | MathType::Boolean
                | MathType::Unit
                | MathType::Unknown => {}
            }
        }
    }
}

impl fmt::Debug for MathType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Type(&'a MathType),
            Domain(&'a [MathType], usize),
            Text(&'static str),
            VectorSuffix(Option<usize>),
            MatrixSuffix(Option<(usize, usize)>),
        }
        let mut events = vec![Event::Type(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::VectorSuffix(dimension) => {
                    write!(f, ", dimension: {dimension:?} }}")?;
                }
                Event::MatrixSuffix(dimensions) => {
                    write!(f, ", dimensions: {dimensions:?} }}")?;
                }
                Event::Domain(domain, index) => {
                    if index == domain.len() {
                        write!(f, "]")?;
                    } else {
                        if index > 0 {
                            write!(f, ", ")?;
                        }
                        events.push(Event::Domain(domain, index + 1));
                        events.push(Event::Type(&domain[index]));
                    }
                }
                Event::Type(math_type) => match math_type {
                    MathType::Number => write!(f, "Number")?,
                    MathType::Variable => write!(f, "Variable")?,
                    MathType::Function {
                        arity,
                        domain,
                        codomain,
                    } => {
                        write!(f, "Function {{ arity: {arity:?}, domain: [")?;
                        events.push(Event::Text(" }"));
                        events.push(Event::Type(codomain));
                        events.push(Event::Text(", codomain: "));
                        events.push(Event::Domain(domain, 0));
                    }
                    MathType::BinaryOp => write!(f, "BinaryOp")?,
                    MathType::UnaryOp => write!(f, "UnaryOp")?,
                    MathType::NaryOp => write!(f, "NaryOp")?,
                    MathType::Relation => write!(f, "Relation")?,
                    MathType::Set => write!(f, "Set")?,
                    MathType::Vector { element, dimension } => {
                        write!(f, "Vector {{ element: ")?;
                        events.push(Event::VectorSuffix(*dimension));
                        events.push(Event::Type(element));
                    }
                    MathType::Matrix {
                        element,
                        dimensions,
                    } => {
                        write!(f, "Matrix {{ element: ")?;
                        events.push(Event::MatrixSuffix(*dimensions));
                        events.push(Event::Type(element));
                    }
                    MathType::Boolean => write!(f, "Boolean")?,
                    MathType::Unit => write!(f, "Unit")?,
                    MathType::TypeVar(id) => write!(f, "TypeVar({id:?})")?,
                    MathType::Unknown => write!(f, "Unknown")?,
                    MathType::Error(message) => write!(f, "Error({message:?})")?,
                },
            }
        }
        Ok(())
    }
}

impl Drop for MathType {
    fn drop(&mut self) {
        fn drain(math_type: &mut MathType, pending: &mut Vec<MathType>) {
            match math_type {
                MathType::Function {
                    domain, codomain, ..
                } => {
                    pending.append(domain);
                    pending.push(std::mem::replace(&mut **codomain, MathType::Unknown));
                }
                MathType::Vector { element, .. } | MathType::Matrix { element, .. } => {
                    pending.push(std::mem::replace(&mut **element, MathType::Unknown));
                }
                _ => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut math_type) = pending.pop() {
            drain(&mut math_type, &mut pending);
        }
    }
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
        enum Event<'a> {
            Type(&'a MathType),
            Domain(&'a [MathType], usize),
            FunctionSuffix(Arity),
            VectorSuffix(Option<usize>),
            MatrixSuffix(Option<(usize, usize)>),
        }
        let mut events = vec![Event::Type(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::FunctionSuffix(arity) => write!(f, " [arity: {arity:?}]")?,
                Event::VectorSuffix(dimension) => match dimension {
                    Some(dimension) => write!(f, ">^{dimension}")?,
                    None => write!(f, ">")?,
                },
                Event::MatrixSuffix(dimensions) => match dimensions {
                    Some((rows, columns)) => write!(f, ">^({rows}x{columns})")?,
                    None => write!(f, ">")?,
                },
                Event::Domain(domain, index) => {
                    if index == domain.len() {
                        write!(f, ") -> ")?;
                    } else {
                        if index > 0 {
                            write!(f, ", ")?;
                        }
                        events.push(Event::Domain(domain, index + 1));
                        events.push(Event::Type(&domain[index]));
                    }
                }
                Event::Type(math_type) => match math_type {
                    MathType::Number => write!(f, "Number")?,
                    MathType::Variable => write!(f, "Var")?,
                    MathType::Function {
                        arity,
                        domain,
                        codomain,
                    } => {
                        write!(f, "(")?;
                        events.push(Event::FunctionSuffix(*arity));
                        events.push(Event::Type(codomain));
                        events.push(Event::Domain(domain, 0));
                    }
                    MathType::BinaryOp => write!(f, "BinOp")?,
                    MathType::UnaryOp => write!(f, "UnaryOp")?,
                    MathType::NaryOp => write!(f, "NaryOp")?,
                    MathType::Relation => write!(f, "Relation")?,
                    MathType::Set => write!(f, "Set")?,
                    MathType::Vector { element, dimension } => {
                        write!(f, "Vec<")?;
                        events.push(Event::VectorSuffix(*dimension));
                        events.push(Event::Type(element));
                    }
                    MathType::Matrix {
                        element,
                        dimensions,
                    } => {
                        write!(f, "Mat<")?;
                        events.push(Event::MatrixSuffix(*dimensions));
                        events.push(Event::Type(element));
                    }
                    MathType::Boolean => write!(f, "Bool")?,
                    MathType::Unit => write!(f, "()")?,
                    MathType::TypeVar(id) => write!(f, "T{id}")?,
                    MathType::Unknown => write!(f, "?")?,
                    MathType::Error(message) => write!(f, "Error({message})")?,
                },
            }
        }
        Ok(())
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
#[derive(Default)]
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
        let mut current = Some(self);
        while let Some(environment) = current {
            if let Some(value) = environment.bindings.get(name) {
                return Some(value);
            }
            current = environment.parent.as_deref();
        }
        None
    }

    /// Check if a variable is bound.
    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }
}

impl Clone for TypeEnvironment {
    fn clone(&self) -> Self {
        let mut chain = Vec::new();
        let mut current = Some(self);
        while let Some(environment) = current {
            chain.push(environment);
            current = environment.parent.as_deref();
        }

        let mut cloned_parent = None;
        for environment in chain.into_iter().rev() {
            cloned_parent = Some(Box::new(TypeEnvironment {
                bindings: environment.bindings.clone(),
                parent: cloned_parent,
            }));
        }
        *cloned_parent.expect("an environment chain always contains its root")
    }
}

impl std::fmt::Debug for TypeEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut current = self;
        let mut parent_links = 0usize;
        loop {
            write!(
                formatter,
                "TypeEnvironment {{ bindings: {:?}, parent: ",
                current.bindings
            )?;
            match current.parent.as_deref() {
                Some(parent) => {
                    formatter.write_str("Some(")?;
                    parent_links += 1;
                    current = parent;
                }
                None => {
                    formatter.write_str("None }")?;
                    break;
                }
            }
        }
        for _ in 0..parent_links {
            formatter.write_str(") }")?;
        }
        Ok(())
    }
}

impl Drop for TypeEnvironment {
    fn drop(&mut self) {
        let mut parent = self.parent.take();
        while let Some(mut environment) = parent {
            parent = environment.parent.take();
        }
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
    use proptest::prelude::*;

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
    fn deep_type_environment_lifecycle_uses_constant_native_stack() {
        const DEPTH: usize = 100_000;
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut root_bindings = HashMap::new();
                root_bindings.insert("root".to_string(), MathType::Number);
                let mut environment = TypeEnvironment {
                    bindings: root_bindings,
                    parent: None,
                };
                for _ in 0..DEPTH {
                    environment = TypeEnvironment {
                        bindings: HashMap::new(),
                        parent: Some(Box::new(environment)),
                    };
                }
                assert_eq!(environment.lookup("root"), Some(&MathType::Number));
                let cloned = environment.clone();
                assert_eq!(cloned.lookup("root"), Some(&MathType::Number));
                assert!(format!("{environment:?}").starts_with("TypeEnvironment {"));
                drop(cloned);
                drop(environment);
            })
            .expect("small-stack worker must spawn")
            .join()
            .expect("type-environment lifecycle must not overflow the native stack");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn type_environment_lookup_matches_reverse_scope_reference(
            scopes in prop::collection::vec(
                prop::collection::vec((0u8..8, any::<bool>()), 0..=8),
                1..=8,
            ),
            query in 0u8..8,
        ) {
            let mut environment = TypeEnvironment::new();
            let mut reference = Vec::<HashMap<String, MathType>>::new();
            for (scope_index, scope) in scopes.into_iter().enumerate() {
                if scope_index > 0 {
                    environment = environment.child();
                }
                let mut model_scope = HashMap::new();
                for (name, is_number) in scope {
                    let name = format!("v{name}");
                    let value = if is_number {
                        MathType::Number
                    } else {
                        MathType::Set
                    };
                    environment.bind(name.clone(), value.clone());
                    model_scope.insert(name, value);
                }
                reference.push(model_scope);
            }

            let name = format!("v{query}");
            let expected = reference
                .iter()
                .rev()
                .find_map(|scope| scope.get(&name));
            prop_assert_eq!(environment.lookup(&name), expected);
            let cloned = environment.clone();
            prop_assert_eq!(cloned.lookup(&name), expected);
            prop_assert_eq!(format!("{cloned:?}"), format!("{environment:?}"));
        }
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
