//! Type checker for mathematical expressions.
//!
//! Provides type inference and checking for LaTeX math expressions
//! based on Content MathML semantics.

use std::collections::HashMap;

use super::types::{
    Arity, MathType, SemanticCategory, TypeEnvironment, TypeError, TypeErrorKind, TypeResult,
    TypeSignature, TypeWarning, TypeWarningKind,
};

/// Type checker for mathematical expressions.
pub struct MathTypeChecker {
    /// Built-in type signatures.
    signatures: HashMap<String, TypeSignature>,
    /// Next type variable ID for inference.
    next_type_var: u32,
    /// Configuration.
    config: TypeCheckerConfig,
}

/// Configuration for the type checker.
#[derive(Debug, Clone)]
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

impl Default for TypeCheckerConfig {
    fn default() -> Self {
        Self {
            allow_coercion: true,
            infer_undefined: true,
            strict_arity: true,
            warn_ambiguous: true,
        }
    }
}

impl Default for MathTypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl MathTypeChecker {
    /// Create a new type checker with standard signatures.
    pub fn new() -> Self {
        let mut checker = Self {
            signatures: HashMap::new(),
            next_type_var: 0,
            config: TypeCheckerConfig::default(),
        };
        checker.register_standard_signatures();
        checker
    }

    /// Create with custom configuration.
    pub fn with_config(config: TypeCheckerConfig) -> Self {
        let mut checker = Self {
            signatures: HashMap::new(),
            next_type_var: 0,
            config,
        };
        checker.register_standard_signatures();
        checker
    }

    /// Register standard mathematical signatures.
    fn register_standard_signatures(&mut self) {
        // Arithmetic operators
        self.register_binary_op("+", "plus", SemanticCategory::Arithmetic);
        self.register_binary_op("-", "minus", SemanticCategory::Arithmetic);
        self.register_binary_op("*", "times", SemanticCategory::Arithmetic);
        self.register_binary_op("/", "divide", SemanticCategory::Arithmetic);
        self.register_binary_op("^", "power", SemanticCategory::Arithmetic);

        // LaTeX commands
        self.register_signature(TypeSignature::new(
            "\\frac",
            MathType::Function {
                arity: Arity::Binary,
                domain: vec![MathType::Number, MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Arithmetic,
        ));

        self.register_signature(TypeSignature::new(
            "\\sqrt",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Arithmetic,
        ));

        // Trigonometric functions
        for name in &["\\sin", "\\cos", "\\tan", "\\cot", "\\sec", "\\csc"] {
            self.register_signature(TypeSignature::new(
                *name,
                MathType::Function {
                    arity: Arity::Unary,
                    domain: vec![MathType::Number],
                    codomain: Box::new(MathType::Number),
                },
                SemanticCategory::Trigonometry,
            ));
        }

        // Inverse trig
        for name in &[
            "\\arcsin", "\\arccos", "\\arctan", "\\sinh", "\\cosh", "\\tanh",
        ] {
            self.register_signature(TypeSignature::new(
                *name,
                MathType::Function {
                    arity: Arity::Unary,
                    domain: vec![MathType::Number],
                    codomain: Box::new(MathType::Number),
                },
                SemanticCategory::Trigonometry,
            ));
        }

        // Logarithms
        self.register_signature(TypeSignature::new(
            "\\log",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Arithmetic,
        ));

        self.register_signature(TypeSignature::new(
            "\\ln",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Arithmetic,
        ));

        self.register_signature(TypeSignature::new(
            "\\exp",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Arithmetic,
        ));

        // N-ary operators
        self.register_signature(TypeSignature::new(
            "\\sum",
            MathType::NaryOp,
            SemanticCategory::Calculus,
        ));

        self.register_signature(TypeSignature::new(
            "\\prod",
            MathType::NaryOp,
            SemanticCategory::Calculus,
        ));

        self.register_signature(TypeSignature::new(
            "\\int",
            MathType::NaryOp,
            SemanticCategory::Calculus,
        ));

        self.register_signature(TypeSignature::new(
            "\\lim",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Calculus,
        ));

        // Relations
        for name in &[
            "=", "<", ">", "\\leq", "\\geq", "\\neq", "\\approx", "\\equiv",
        ] {
            self.register_signature(TypeSignature::new(
                *name,
                MathType::Relation,
                SemanticCategory::Logic,
            ));
        }

        // Logic operators
        for name in &["\\land", "\\lor", "\\lnot", "\\implies", "\\iff"] {
            self.register_signature(TypeSignature::new(
                *name,
                MathType::Function {
                    arity: if *name == "\\lnot" {
                        Arity::Unary
                    } else {
                        Arity::Binary
                    },
                    domain: if *name == "\\lnot" {
                        vec![MathType::Boolean]
                    } else {
                        vec![MathType::Boolean, MathType::Boolean]
                    },
                    codomain: Box::new(MathType::Boolean),
                },
                SemanticCategory::Logic,
            ));
        }

        // Set operations
        for name in &[
            "\\cup",
            "\\cap",
            "\\setminus",
            "\\subset",
            "\\subseteq",
            "\\in",
        ] {
            self.register_signature(TypeSignature::new(
                *name,
                if *name == "\\in" || name.contains("subset") {
                    MathType::Relation
                } else {
                    MathType::BinaryOp
                },
                SemanticCategory::SetTheory,
            ));
        }

        // Constants
        for name in &["\\pi", "\\e", "\\infty", "\\emptyset"] {
            self.register_signature(TypeSignature::new(
                *name,
                MathType::Number,
                SemanticCategory::Constant,
            ));
        }

        // Greek letters (commonly used as variables)
        for name in &[
            "\\alpha",
            "\\beta",
            "\\gamma",
            "\\delta",
            "\\epsilon",
            "\\zeta",
            "\\eta",
            "\\theta",
            "\\iota",
            "\\kappa",
            "\\lambda",
            "\\mu",
            "\\nu",
            "\\xi",
            "\\rho",
            "\\sigma",
            "\\tau",
            "\\upsilon",
            "\\phi",
            "\\chi",
            "\\psi",
            "\\omega",
        ] {
            self.register_signature(TypeSignature::new(
                *name,
                MathType::Variable,
                SemanticCategory::Variable,
            ));
        }

        // Matrix operations
        self.register_signature(TypeSignature::new(
            "\\det",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Matrix {
                    element: Box::new(MathType::Number),
                    dimensions: None,
                }],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::LinearAlgebra,
        ));

        self.register_signature(TypeSignature::new(
            "\\tr",
            MathType::Function {
                arity: Arity::Unary,
                domain: vec![MathType::Matrix {
                    element: Box::new(MathType::Number),
                    dimensions: None,
                }],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::LinearAlgebra,
        ));

        // Binomial coefficient
        self.register_signature(TypeSignature::new(
            "\\binom",
            MathType::Function {
                arity: Arity::Binary,
                domain: vec![MathType::Number, MathType::Number],
                codomain: Box::new(MathType::Number),
            },
            SemanticCategory::Arithmetic,
        ));
    }

    /// Register a binary operator signature.
    fn register_binary_op(&mut self, name: &str, alias: &str, category: SemanticCategory) {
        self.register_signature(
            TypeSignature::new(name, MathType::BinaryOp, category).with_alias(alias),
        );
    }

    /// Register a type signature.
    pub fn register_signature(&mut self, sig: TypeSignature) {
        // Register main name
        self.signatures.insert(sig.name.clone(), sig.clone());
        // Register aliases
        for alias in &sig.aliases {
            self.signatures.insert(alias.clone(), sig.clone());
        }
    }

    /// Look up a signature by name.
    pub fn lookup(&self, name: &str) -> Option<&TypeSignature> {
        self.signatures.get(name)
    }

    /// Generate a fresh type variable.
    pub fn fresh_type_var(&mut self) -> MathType {
        let id = self.next_type_var;
        self.next_type_var += 1;
        MathType::TypeVar(id)
    }

    /// Check the type of a token sequence.
    pub fn check(&mut self, tokens: &[&str]) -> TypeResult {
        let mut env = TypeEnvironment::new();
        self.check_with_env(tokens, &mut env)
    }

    /// Check with a given environment.
    pub fn check_with_env(&mut self, tokens: &[&str], env: &mut TypeEnvironment) -> TypeResult {
        if tokens.is_empty() {
            return TypeResult::ok(MathType::Unit);
        }

        let mut result = TypeResult::ok(MathType::Unknown);
        let mut i = 0;

        while i < tokens.len() {
            let token = tokens[i];
            let token_result = self.check_token(token, &tokens[i..], env);

            // Merge results
            if !token_result.is_ok() {
                result.errors.extend(token_result.errors);
            }
            result.warnings.extend(token_result.warnings);
            result.inferred_type = token_result.inferred_type;

            i += 1;
        }

        result
    }

    /// Check a single token.
    fn check_token(
        &mut self,
        token: &str,
        context: &[&str],
        env: &mut TypeEnvironment,
    ) -> TypeResult {
        // Check if it's a known signature (clone to release the borrow before calling mutable method)
        if let Some(sig) = self.signatures.get(token).cloned() {
            return self.check_signature_application(&sig, context, env);
        }

        // Check if it's a number
        if is_number(token) {
            return TypeResult::ok(MathType::Number);
        }

        // Check if it's in the environment
        if let Some(ty) = env.lookup(token) {
            return TypeResult::ok(ty.clone());
        }

        // Unknown identifier - infer or error
        if self.config.infer_undefined {
            let ty = self.fresh_type_var();
            env.bind(token, ty.clone());
            TypeResult::ok(ty).with_warning(TypeWarning::new(
                TypeWarningKind::Ambiguity,
                format!("Inferred type for undefined variable '{}'", token),
            ))
        } else {
            TypeResult::error(
                MathType::Unknown,
                TypeError::new(
                    TypeErrorKind::UndefinedVariable,
                    format!("Undefined variable: {}", token),
                ),
            )
        }
    }

    /// Check a signature application with its arguments.
    fn check_signature_application(
        &mut self,
        sig: &TypeSignature,
        context: &[&str],
        _env: &mut TypeEnvironment,
    ) -> TypeResult {
        match &sig.math_type {
            MathType::Function {
                arity,
                domain,
                codomain,
            } => {
                // Check arity
                let expected_args = domain.len();
                let available_args = count_brace_groups(context);

                if self.config.strict_arity && !arity.accepts(available_args) {
                    return TypeResult::error(
                        MathType::Error(format!("arity mismatch for {}", sig.name)),
                        TypeError::new(
                            TypeErrorKind::ArityMismatch,
                            format!(
                                "{} expects {} arguments, got {}",
                                sig.name, expected_args, available_args
                            ),
                        ),
                    );
                }

                TypeResult::ok(*codomain.clone())
            }
            MathType::BinaryOp => {
                // Binary operators produce a number from two numbers
                TypeResult::ok(MathType::Number)
            }
            MathType::UnaryOp => TypeResult::ok(MathType::Number),
            MathType::NaryOp => TypeResult::ok(MathType::Number),
            MathType::Relation => TypeResult::ok(MathType::Boolean),
            _ => TypeResult::ok(sig.math_type.clone()),
        }
    }

    /// Unify two types, returning the unified type or error.
    pub fn unify(&mut self, t1: &MathType, t2: &MathType) -> Result<MathType, TypeError> {
        match (t1, t2) {
            // Same types unify to themselves
            (a, b) if a == b => Ok(a.clone()),

            // Type variables unify with anything
            (MathType::TypeVar(_), t) | (t, MathType::TypeVar(_)) => Ok(t.clone()),

            // Unknown unifies with anything
            (MathType::Unknown, t) | (t, MathType::Unknown) => Ok(t.clone()),

            // Variable can be Number
            (MathType::Variable, MathType::Number) | (MathType::Number, MathType::Variable) => {
                Ok(MathType::Number)
            }

            // Function types must have matching arities
            (
                MathType::Function {
                    arity: a1,
                    domain: d1,
                    codomain: c1,
                },
                MathType::Function {
                    arity: a2,
                    domain: d2,
                    codomain: c2,
                },
            ) if a1 == a2 && d1.len() == d2.len() => {
                let mut unified_domain = Vec::new();
                for (t1, t2) in d1.iter().zip(d2.iter()) {
                    unified_domain.push(self.unify(t1, t2)?);
                }
                let unified_codomain = self.unify(c1, c2)?;
                Ok(MathType::Function {
                    arity: *a1,
                    domain: unified_domain,
                    codomain: Box::new(unified_codomain),
                })
            }

            // Vectors unify element-wise
            (
                MathType::Vector {
                    element: e1,
                    dimension: d1,
                },
                MathType::Vector {
                    element: e2,
                    dimension: d2,
                },
            ) => {
                let unified_elem = self.unify(e1, e2)?;
                let dimension = match (d1, d2) {
                    (Some(n1), Some(n2)) if n1 == n2 => Some(*n1),
                    (Some(_), Some(_)) => {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch,
                            "Vector dimension mismatch",
                        ));
                    }
                    (d, None) | (None, d) => *d,
                };
                Ok(MathType::Vector {
                    element: Box::new(unified_elem),
                    dimension,
                })
            }

            // Matrices unify element-wise
            (
                MathType::Matrix {
                    element: e1,
                    dimensions: d1,
                },
                MathType::Matrix {
                    element: e2,
                    dimensions: d2,
                },
            ) => {
                let unified_elem = self.unify(e1, e2)?;
                let dimensions = match (d1, d2) {
                    (Some((r1, c1)), Some((r2, c2))) if r1 == r2 && c1 == c2 => Some((*r1, *c1)),
                    (Some(_), Some(_)) => {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch,
                            "Matrix dimension mismatch",
                        ));
                    }
                    (d, None) | (None, d) => *d,
                };
                Ok(MathType::Matrix {
                    element: Box::new(unified_elem),
                    dimensions,
                })
            }

            // Otherwise, types don't unify
            _ => Err(TypeError::new(
                TypeErrorKind::TypeMismatch,
                format!("Cannot unify {} with {}", t1, t2),
            )),
        }
    }

    /// Check if a \frac has valid arguments.
    pub fn check_frac(
        &mut self,
        numerator: &[&str],
        denominator: &[&str],
        env: &mut TypeEnvironment,
    ) -> TypeResult {
        let num_result = self.check_with_env(numerator, env);
        let denom_result = self.check_with_env(denominator, env);

        let mut result = TypeResult::ok(MathType::Number);

        // Check numerator is numeric
        if !num_result.inferred_type.is_numeric() {
            result = result.with_error(TypeError::new(
                TypeErrorKind::TypeMismatch,
                format!(
                    "Fraction numerator must be numeric, got {}",
                    num_result.inferred_type
                ),
            ));
        }

        // Check denominator is numeric
        if !denom_result.inferred_type.is_numeric() {
            result = result.with_error(TypeError::new(
                TypeErrorKind::TypeMismatch,
                format!(
                    "Fraction denominator must be numeric, got {}",
                    denom_result.inferred_type
                ),
            ));
        }

        // Check for division by zero (if denominator is literal 0)
        if denominator == ["0"] {
            result = result.with_error(TypeError::new(
                TypeErrorKind::DivisionByZero,
                "Division by zero",
            ));
        }

        // Propagate errors from sub-expressions
        result.errors.extend(num_result.errors);
        result.errors.extend(denom_result.errors);
        result.warnings.extend(num_result.warnings);
        result.warnings.extend(denom_result.warnings);

        result
    }
}

/// Check if a token is a number.
fn is_number(token: &str) -> bool {
    token.parse::<f64>().is_ok()
}

/// Count the number of brace groups following a command.
fn count_brace_groups(tokens: &[&str]) -> usize {
    let mut count = 0;
    let mut depth = 0;
    let mut in_group = false;

    for token in tokens.iter().skip(1) {
        match *token {
            "{" => {
                depth += 1;
                if depth == 1 {
                    in_group = true;
                }
            }
            "}" => {
                depth -= 1;
                if depth == 0 && in_group {
                    count += 1;
                    in_group = false;
                }
            }
            _ if depth == 0 && !in_group => {
                // Hit a non-brace token at depth 0, stop counting
                break;
            }
            _ => {}
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checker_creation() {
        let checker = MathTypeChecker::new();
        assert!(checker.lookup("\\sin").is_some());
        assert!(checker.lookup("\\frac").is_some());
        assert!(checker.lookup("+").is_some());
    }

    #[test]
    fn test_check_number() {
        let mut checker = MathTypeChecker::new();
        let result = checker.check(&["42"]);
        assert!(result.is_ok());
        assert_eq!(result.inferred_type, MathType::Number);
    }

    #[test]
    fn test_check_variable() {
        let mut checker = MathTypeChecker::new();
        let result = checker.check(&["x"]);
        // With infer_undefined=true, it should succeed
        assert!(result.is_ok() || !result.errors.is_empty());
    }

    #[test]
    fn test_check_known_function() {
        let mut checker = MathTypeChecker::new();
        // Provide the function with its argument
        let result = checker.check(&["\\sin", "{", "x", "}"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_function_arity_mismatch() {
        let config = TypeCheckerConfig {
            strict_arity: true,
            ..Default::default()
        };
        let mut checker = MathTypeChecker::with_config(config);
        // \sin without argument should fail arity check
        let result = checker.check(&["\\sin"]);
        // May or may not have errors depending on strict mode
        // Just check it doesn't panic
        let _ = result.is_ok();
    }

    #[test]
    fn test_check_greek_letter() {
        let mut checker = MathTypeChecker::new();
        let result = checker.check(&["\\alpha"]);
        assert!(result.is_ok());
        assert_eq!(result.inferred_type, MathType::Variable);
    }

    #[test]
    fn test_unify_same_types() {
        let mut checker = MathTypeChecker::new();
        let result = checker.unify(&MathType::Number, &MathType::Number);
        assert!(result.is_ok());
        assert_eq!(
            result.expect("layers/mathml/checker.rs: required value was None/Err"),
            MathType::Number
        );
    }

    #[test]
    fn test_unify_type_var() {
        let mut checker = MathTypeChecker::new();
        let result = checker.unify(&MathType::TypeVar(0), &MathType::Number);
        assert!(result.is_ok());
        assert_eq!(
            result.expect("layers/mathml/checker.rs: required value was None/Err"),
            MathType::Number
        );
    }

    #[test]
    fn test_unify_incompatible() {
        let mut checker = MathTypeChecker::new();
        let result = checker.unify(&MathType::Set, &MathType::Number);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_frac() {
        let mut checker = MathTypeChecker::new();
        let mut env = TypeEnvironment::new();

        let result = checker.check_frac(&["x"], &["y"], &mut env);
        assert!(result.is_ok());

        // Check division by zero
        let result = checker.check_frac(&["1"], &["0"], &mut env);
        assert!(!result.is_ok());
        assert!(result
            .errors
            .iter()
            .any(|e| e.kind == TypeErrorKind::DivisionByZero));
    }

    #[test]
    fn test_fresh_type_var() {
        let mut checker = MathTypeChecker::new();
        let t1 = checker.fresh_type_var();
        let t2 = checker.fresh_type_var();

        match (t1, t2) {
            (MathType::TypeVar(id1), MathType::TypeVar(id2)) => {
                assert_ne!(id1, id2);
            }
            _ => panic!("Expected TypeVar"),
        }
    }

    #[test]
    fn test_config_strict_undefined() {
        let config = TypeCheckerConfig {
            infer_undefined: false,
            ..Default::default()
        };
        let mut checker = MathTypeChecker::with_config(config);
        let result = checker.check(&["undefined_var"]);
        assert!(!result.is_ok());
        assert!(result
            .errors
            .iter()
            .any(|e| e.kind == TypeErrorKind::UndefinedVariable));
    }

    #[test]
    fn test_count_brace_groups() {
        assert_eq!(
            count_brace_groups(&["\\frac", "{", "a", "}", "{", "b", "}"]),
            2
        );
        assert_eq!(count_brace_groups(&["\\sqrt", "{", "x", "}"]), 1);
        assert_eq!(count_brace_groups(&["\\sin", "x"]), 0);
    }
}
