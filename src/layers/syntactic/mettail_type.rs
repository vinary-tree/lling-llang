//! MeTTaIL semantic type filtering layer.
//!
//! This layer filters lattice paths based on MeTTaIL/OSLF type constraints.
//! It integrates with the F1R3FLY.io stack for semantic type checking.
//!
//! # Feature Gate
//!
//! This module is only available when the `f1r3fly` feature is enabled.

use rustc_hash::FxHashSet;

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder, LatticePathExt};
use crate::semiring::Semiring;

use crate::layers::traits::{CorrectionLayer, LayerError, LayerResult};

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
    /// Penalty weight multiplier for soft constraint violations (default: 2.0).
    soft_penalty: f64,
}

impl MeTTaILTypeLayer {
    /// Create a new MeTTaIL type layer.
    pub fn new(type_checker: Box<dyn TypeChecker>) -> Self {
        Self {
            type_checker,
            constraints: Vec::new(),
            soft_penalty: 2.0,
        }
    }

    /// Add a type constraint.
    pub fn with_constraint(mut self, constraint: TypeConstraint) -> Self {
        self.constraints.push(constraint);
        self
    }

    /// Add multiple type constraints.
    pub fn with_constraints(
        mut self,
        constraints: impl IntoIterator<Item = TypeConstraint>,
    ) -> Self {
        self.constraints.extend(constraints);
        self
    }

    /// Set the penalty multiplier for soft constraint violations.
    pub fn with_soft_penalty(mut self, penalty: f64) -> Self {
        self.soft_penalty = penalty;
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

    /// Check if a token sequence passes strict constraints.
    ///
    /// Returns false if any strict constraint is violated.
    fn passes_strict_constraints(&self, tokens: &[&str]) -> bool {
        self.constraints
            .iter()
            .filter(|c| c.strict)
            .all(|c| self.check_constraint(tokens, c))
    }

    /// Check a single constraint against a token sequence.
    fn check_constraint(&self, tokens: &[&str], constraint: &TypeConstraint) -> bool {
        match constraint.position {
            Some(pos) => {
                if pos < tokens.len() {
                    self.type_checker
                        .check_type(tokens[pos], &constraint.required_type)
                } else {
                    !constraint.strict // Strict = fail, soft = pass
                }
            }
            None => {
                // Check if any token satisfies the constraint
                tokens
                    .iter()
                    .any(|t| self.type_checker.check_type(t, &constraint.required_type))
            }
        }
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

        // Collect edge IDs from paths that pass strict type constraints
        let mut used_edges: FxHashSet<crate::lattice::EdgeId> = FxHashSet::default();

        // Iterate over all paths in the lattice
        for path in lattice.paths() {
            // Get words from the path
            let words: Vec<&str> = path.words(lattice).collect();

            if words.is_empty() {
                continue;
            }

            // Check if the path passes strict type constraints
            if self.passes_strict_constraints(&words) {
                // Add all edges from this path to the used set
                for edge_id in &path.edges {
                    used_edges.insert(*edge_id);
                }
            }
        }

        // If no paths passed, return error
        if used_edges.is_empty() {
            return Err(LayerError::Other(
                "no paths passed MeTTaIL type constraints".to_string(),
            ));
        }

        // Build a new lattice with only the used edges
        let mut new_builder = LatticeBuilder::new(lattice.backend().clone());

        for edge in lattice.edges() {
            if used_edges.contains(&edge.id) {
                new_builder.add_correction_by_id(
                    edge.source.0 as usize,
                    edge.target.0 as usize,
                    edge.label,
                    edge.weight,
                    edge.metadata.clone(),
                );
            }
        }

        // Build with the same end position
        let end_pos = lattice.end().0 as usize;
        Ok(new_builder.build(end_pos))
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
            if token
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
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
        let constraint = TypeConstraint::strict(TypeExpr::base("Noun")).at_position(0);

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
        let name =
            <MeTTaILTypeLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer);
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
