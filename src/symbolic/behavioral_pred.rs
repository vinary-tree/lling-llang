//! Behavioral predicate AST.
//!
//! Phase 6 / F.0-sibling: moved from `mettail-runtime` to `mettail-prattail`
//! so the WPDS walker can produce predicates without crossing the
//! `prattail → runtime` cycle (runtime depends on prattail). The runtime
//! crate re-exports this module's types for backward compatibility.
//!
//! This is the runtime-friendly counterpart to
//! `mettail_ast::language::BehavioralPred`. Where the AST type uses
//! `syn::Ident` (because it lives in a proc-macro-consuming crate that
//! reads from `ParseStream`), this type uses `String` so it can be
//! stored in generated runtime enum variants and parsed at source time.
//!
//! ## Role at runtime
//!
//! `BehavioralPred` is a **passive data type** — no `evaluate()` method
//! and no thread-local snapshot. The thread-local fact snapshot and
//! `evaluate_pred_with_bindings` live in `runtime/src/pred_eval.rs`,
//! using these types via re-export.
//!
//! ## Semantics deferred to Ascent
//!
//! - `RelationQuery` — lowered to an Ascent join clause `rel(args)`.
//! - `Quantified { ForAll | Exists, ... }` — lowered via
//!   `prattail::logict::QuantifiedFormula` + `evaluate_quantified`.
//! - `AcMatch` — lowered to specialized Ascent code using
//!   `prattail::logict::multiset_partitions`.
//! - `And`, `Or`, `Not`, `Implies` — Boolean rewrites to DNF + one
//!   Ascent rule per clause.
//! - `Top` — identity predicate; "no join clause".

use moniker::{BoundTerm, Var};
use std::fmt;

mod lifecycle;

/// Runtime behavioral predicate. Stored as a field on guarded receive
/// constructors for per-instance shape dispatch and introspection.
pub enum BehavioralPred {
    /// Atomic relation query: `path(x, {})`, `halts(p)`.
    /// `negated = true` corresponds to Ascent's `!path(...)`
    /// (stratified negation).
    RelationQuery {
        relation_name: String,
        args: Vec<PredArg>,
        negated: bool,
    },
    /// Quantified predicate: `forall(y, nodes, body)` / `exists(y, nodes, body)`.
    Quantified {
        quantifier: Quantifier,
        var: String,
        domain: Option<QuantifiedDomain>,
        body: Box<BehavioralPred>,
    },
    /// AC-matching predicate: `ac_match(bag, [elem1, elem2, ...rest])`.
    AcMatch {
        bag: PredArg,
        elements: Vec<PredArg>,
        rest: Option<String>,
    },
    And(Box<BehavioralPred>, Box<BehavioralPred>),
    Or(Box<BehavioralPred>, Box<BehavioralPred>),
    Not(Box<BehavioralPred>),
    Implies(Box<BehavioralPred>, Box<BehavioralPred>),
    /// Always true — used as the identity predicate when the predicate slot is
    /// declared at language-spec time but filled at source-parse time.
    Top,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Quantifier {
    ForAll,
    Exists,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QuantifiedDomain {
    /// Named domain: `forall(y, nodes, body)` — `nodes` is a declared
    /// relation.
    Named(String),
    /// Bounded depth: `exists(y, 100, body)` — search up to 100 steps.
    Bounded(usize),
    /// Enumerated set: `forall(y, {a, b, c}, body)`.
    Enumerated(Vec<PredArg>),
}

/// Arguments to a behavioral predicate. Variables refer to bindings
/// established by the structural pattern match (the `MatchBindings` of
/// §5 of the predicated-types design).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PredArg {
    /// Variable reference: looked up at compile time in the rule's
    /// MatchBindings context when generating the Ascent join clause.
    Var(String),
    /// Integer literal.
    IntLit(i64),
    /// String literal.
    StringLit(String),
}

impl BehavioralPred {
    /// Substitute variable references in this predicate. Used by the
    /// macro pipeline during pattern-match substitution when a bound
    /// variable's name changes.
    pub fn substitute_var(&self, old: &str, new: &str) -> Self {
        lifecycle::substitute_var(self, old, new)
    }

    /// Collect all free variable names referenced by this predicate.
    pub fn free_vars(&self) -> std::collections::HashSet<String> {
        lifecycle::free_vars(self)
    }
}

impl PredArg {
    pub fn substitute_var(&self, old: &str, new: &str) -> Self {
        match self {
            PredArg::Var(v) if v == old => PredArg::Var(new.to_string()),
            other => other.clone(),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// `moniker::BoundTerm` impl — trivial leaf
// ═════════════════════════════════════════════════════════════════════════
//
// `BehavioralPred` is a passive data field on guarded receive
// constructors. It does NOT participate in host-category alpha-
// equivalence: variables inside a predicate (e.g., `halts(y)`
// referencing a pattern-bound `y`) are bound by the parent's
// `MatchBindings`, not by host-category `FreeVar<String>`s.
//
// We therefore implement `BoundTerm<String>` as a leaf — `term_eq`
// delegates to structural `PartialEq`, and `close_term`/`open_term`/
// `visit_vars`/`visit_mut_vars` are no-ops.
impl BoundTerm<String> for BehavioralPred {
    fn term_eq(&self, other: &Self) -> bool {
        self.eq(other)
    }

    fn close_term(
        &mut self,
        _state: moniker::ScopeState,
        _on_free: &impl moniker::OnFreeFn<String>,
    ) {
        // No host-category variables inside a predicate.
    }

    fn open_term(
        &mut self,
        _state: moniker::ScopeState,
        _on_bound: &impl moniker::OnBoundFn<String>,
    ) {
        // No host-category variables inside a predicate.
    }

    fn visit_vars(&self, _on_var: &mut impl FnMut(&Var<String>)) {
        // No host-category variables inside a predicate.
    }

    fn visit_mut_vars(&mut self, _on_var: &mut impl FnMut(&mut Var<String>)) {
        // No host-category variables inside a predicate.
    }
}

impl QuantifiedDomain {
    fn substitute_var(&self, old: &str, new: &str) -> Self {
        match self {
            QuantifiedDomain::Named(n) => QuantifiedDomain::Named(n.clone()),
            QuantifiedDomain::Bounded(k) => QuantifiedDomain::Bounded(*k),
            QuantifiedDomain::Enumerated(es) => QuantifiedDomain::Enumerated(
                es.iter().map(|e| e.substitute_var(old, new)).collect(),
            ),
        }
    }
}

impl fmt::Display for PredArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PredArg::Var(v) => write!(f, "{}", v),
            PredArg::IntLit(n) => write!(f, "{}", n),
            PredArg::StringLit(s) => write!(f, "\"{}\"", s),
        }
    }
}

impl fmt::Display for QuantifiedDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantifiedDomain::Named(n) => write!(f, "{}", n),
            QuantifiedDomain::Bounded(k) => write!(f, "{}", k),
            QuantifiedDomain::Enumerated(es) => {
                write!(f, "{{")?;
                for (i, e) in es.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, "}}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_query_display_roundtrip() {
        let p = BehavioralPred::RelationQuery {
            relation_name: "halts".to_string(),
            args: vec![PredArg::Var("x".to_string())],
            negated: false,
        };
        assert_eq!(p.to_string(), "halts(x)");
    }

    #[test]
    fn substitute_var_preserves_other_vars() {
        let p = BehavioralPred::RelationQuery {
            relation_name: "rel".to_string(),
            args: vec![PredArg::Var("x".to_string()), PredArg::Var("y".to_string())],
            negated: false,
        };
        let p2 = p.substitute_var("x", "z");
        match &p2 {
            BehavioralPred::RelationQuery { args, .. } => {
                assert!(matches!(&args[0], PredArg::Var(v) if v == "z"));
                assert!(matches!(&args[1], PredArg::Var(v) if v == "y"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn free_vars_excludes_quantified_var() {
        let p = BehavioralPred::Quantified {
            quantifier: Quantifier::ForAll,
            var: "y".to_string(),
            domain: None,
            body: Box::new(BehavioralPred::RelationQuery {
                relation_name: "safe".to_string(),
                args: vec![PredArg::Var("y".to_string()), PredArg::Var("z".to_string())],
                negated: false,
            }),
        };
        let fvs = p.free_vars();
        assert!(fvs.contains("z"));
        assert!(!fvs.contains("y"));
    }
}
