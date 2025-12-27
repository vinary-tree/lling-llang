//! MeTTaIL semantic type filtering layer.
//!
//! This layer filters lattice paths based on MeTTaIL/OSLF type constraints.
//! It integrates with the F1R3FLY.io stack for semantic type checking.
//!
//! # Feature Gate
//!
//! This module is only available when the `f1r3fly` feature is enabled.

use crate::backend::LatticeBackend;
use crate::lattice::Lattice;
use crate::semiring::Semiring;

use super::traits::{CorrectionLayer, LayerError, LayerResult};

/// A MeTTaIL type expression.
///
/// Types in MeTTaIL follow OSLF (Operational Semantic Logic Framework) patterns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeExpr {
    /// Base type (e.g., "String", "Number").
    Base(String),
    /// Function type (input -> output).
    Function(Box<TypeExpr>, Box<TypeExpr>),
    /// Type variable (for polymorphism).
    Variable(String),
    /// Type application (e.g., "List String").
    Application(Box<TypeExpr>, Vec<TypeExpr>),
}

impl TypeExpr {
    /// Create a base type.
    pub fn base(name: impl Into<String>) -> Self {
        Self::Base(name.into())
    }

    /// Create a function type.
    pub fn function(input: TypeExpr, output: TypeExpr) -> Self {
        Self::Function(Box::new(input), Box::new(output))
    }

    /// Create a type variable.
    pub fn variable(name: impl Into<String>) -> Self {
        Self::Variable(name.into())
    }
}

/// A type constraint for filtering.
#[derive(Clone, Debug)]
pub struct TypeConstraint {
    /// Position in the token sequence (or None for any position).
    pub position: Option<usize>,
    /// Required type expression.
    pub required_type: TypeExpr,
    /// Whether this constraint is strict (reject) or soft (downweight).
    pub strict: bool,
}

impl TypeConstraint {
    /// Create a new strict type constraint.
    pub fn strict(required_type: TypeExpr) -> Self {
        Self {
            position: None,
            required_type,
            strict: true,
        }
    }

    /// Create a new soft type constraint.
    pub fn soft(required_type: TypeExpr) -> Self {
        Self {
            position: None,
            required_type,
            strict: false,
        }
    }

    /// Set the position for this constraint.
    pub fn at_position(mut self, pos: usize) -> Self {
        self.position = Some(pos);
        self
    }
}

/// Trait for MeTTaIL type checking.
///
/// Implement this trait to provide type checking against MeTTaIL/OSLF types.
pub trait TypeChecker: Send + Sync {
    /// Infer the type of a token.
    fn infer_type(&self, token: &str) -> Option<TypeExpr>;

    /// Check if a token satisfies a type constraint.
    fn check_type(&self, token: &str, expected: &TypeExpr) -> bool;

    /// Check if a token sequence satisfies type constraints.
    fn check_sequence(&self, tokens: &[&str], constraints: &[TypeConstraint]) -> bool {
        constraints.iter().all(|c| {
            match c.position {
                Some(pos) => {
                    if pos < tokens.len() {
                        self.check_type(tokens[pos], &c.required_type)
                    } else {
                        !c.strict
                    }
                }
                None => {
                    // Check if any token satisfies the constraint
                    tokens.iter().any(|t| self.check_type(t, &c.required_type))
                }
            }
        })
    }
}

/// MeTTaIL semantic type filtering layer.
///
/// This layer filters lattice paths to only those that satisfy
/// MeTTaIL/OSLF type constraints.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::{MeTTaILTypeLayer, TypeConstraint, TypeExpr};
///
/// let layer = MeTTaILTypeLayer::new(Box::new(my_type_checker))
///     .with_constraint(TypeConstraint::strict(TypeExpr::base("Noun")).at_position(1));
/// let filtered = layer.apply(&lattice)?;
/// ```
pub struct MeTTaILTypeLayer {
    type_checker: Box<dyn TypeChecker>,
    constraints: Vec<TypeConstraint>,
}

impl MeTTaILTypeLayer {
    /// Create a new MeTTaIL type layer.
    pub fn new(type_checker: Box<dyn TypeChecker>) -> Self {
        Self {
            type_checker,
            constraints: Vec::new(),
        }
    }

    /// Add a type constraint.
    pub fn with_constraint(mut self, constraint: TypeConstraint) -> Self {
        self.constraints.push(constraint);
        self
    }

    /// Add multiple type constraints.
    pub fn with_constraints(mut self, constraints: impl IntoIterator<Item = TypeConstraint>) -> Self {
        self.constraints.extend(constraints);
        self
    }

    /// Get the type checker.
    pub fn type_checker(&self) -> &dyn TypeChecker {
        self.type_checker.as_ref()
    }

    /// Get the constraints.
    pub fn constraints(&self) -> &[TypeConstraint] {
        &self.constraints
    }
}

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for MeTTaILTypeLayer {
    fn name(&self) -> &str {
        "mettail-types"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // TODO: Implement MeTTaIL type-based filtering
        // 1. Extract paths from the lattice
        // 2. For each path, check type constraints
        // 3. Filter paths that don't satisfy strict constraints
        // 4. Downweight paths that don't satisfy soft constraints
        // 5. Build a new lattice with filtered/reweighted edges

        Err(LayerError::Other(
            "MeTTaIL type layer not yet implemented - this is a stub".to_string()
        ))
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true
    }

    fn estimated_reduction(&self) -> f64 {
        // Type filtering typically provides significant reduction
        0.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::semiring::TropicalWeight;

    struct MockTypeChecker;

    impl TypeChecker for MockTypeChecker {
        fn infer_type(&self, token: &str) -> Option<TypeExpr> {
            // Simple mock: words starting with capital are Nouns
            if token.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                Some(TypeExpr::base("Noun"))
            } else {
                Some(TypeExpr::base("Other"))
            }
        }

        fn check_type(&self, token: &str, expected: &TypeExpr) -> bool {
            if let Some(inferred) = self.infer_type(token) {
                inferred == *expected
            } else {
                false
            }
        }
    }

    #[test]
    fn test_type_expr() {
        let noun = TypeExpr::base("Noun");
        assert_eq!(noun, TypeExpr::Base("Noun".to_string()));

        let func = TypeExpr::function(TypeExpr::base("A"), TypeExpr::base("B"));
        assert!(matches!(func, TypeExpr::Function(_, _)));
    }

    #[test]
    fn test_type_constraint() {
        let constraint = TypeConstraint::strict(TypeExpr::base("Noun"))
            .at_position(0);

        assert!(constraint.strict);
        assert_eq!(constraint.position, Some(0));
    }

    #[test]
    fn test_mock_type_checker() {
        let checker = MockTypeChecker;

        assert!(checker.check_type("Dog", &TypeExpr::base("Noun")));
        assert!(!checker.check_type("run", &TypeExpr::base("Noun")));
    }

    #[test]
    fn test_layer_name() {
        let layer = MeTTaILTypeLayer::new(Box::new(MockTypeChecker));
        // Use explicit trait method call with concrete types
        let name = <MeTTaILTypeLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer);
        assert_eq!(name, "mettail-types");
    }

    #[test]
    fn test_layer_builder() {
        let layer = MeTTaILTypeLayer::new(Box::new(MockTypeChecker))
            .with_constraint(TypeConstraint::strict(TypeExpr::base("Noun")).at_position(0))
            .with_constraint(TypeConstraint::soft(TypeExpr::base("Verb")).at_position(1));

        assert_eq!(layer.constraints.len(), 2);
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = MeTTaILTypeLayer::new(Box::new(MockTypeChecker));
        // Use explicit trait method call with concrete types
        let reduction = <MeTTaILTypeLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::estimated_reduction(&layer);
        assert!((reduction - 0.2).abs() < 0.001);
    }
}
