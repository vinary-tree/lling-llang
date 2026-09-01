//! Conformance properties extracted from the stack-machine proofs.
//!
//! The shallow properties compare public APIs with compact independent
//! reference interpreters.  The deep tests execute on a 256 KiB native stack;
//! input depth is deliberately 100,000 so recursive implementations fail while
//! heap-resident continuation machines succeed.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

#[cfg(feature = "mathml-semantic")]
use lling_llang::layers::mathml::{MathType, MathTypeChecker};
use lling_llang::programming::{
    Position, RepairAction, SyntaxRepairCosts, Token, TokenKind, TokenPredicate,
};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::symbolic::algebra_tower::{RejectSafeAlgebra, Sat3};
use lling_llang::symbolic::any_algebra::{AnyAlgebra, AnyDomain, AnyPred};
use lling_llang::symbolic::behavioral_algebra::{
    BehavioralAlgebra, BehavioralFormula, BehavioralWorld, FactBase, NoTerm, QDomain,
};
use lling_llang::symbolic::collection_algebra::{BagAlgebra, BagPred, MapAlgebra, MapPred};
use lling_llang::symbolic::kat_algebra::{eval_test_public, BooleanTest};
use lling_llang::symbolic::logict::{
    evaluate_quantified, evaluate_quantified_with_theory, ConstraintTheory, LogicStream,
    QuantifiedFormula, TheoryAlgebra, TheoryPred, TriState,
};
#[cfg(feature = "smt-z3")]
use lling_llang::symbolic::logict_smt::{SmtConstraint, SmtModel, SmtTerm, Z3Theory};
use lling_llang::symbolic::presburger::{
    evaluate_presburger, IntAssignment, LinearConstraint, PresburgerPred,
};
use lling_llang::symbolic::product_nary::{
    NaryProductAlgebra, NaryProductPred, SumAlgebra, SumPred, SumValue,
};
use lling_llang::symbolic::regex_sfa::{RegexAlgebra, RegexPred};
use lling_llang::symbolic::sym_tree::{
    SymTerm, SymbolicTreeAutomaton, TreeAlgebra, TreePred, TreeTrans,
};
use lling_llang::symbolic::{
    classify_decidability, BooleanAlgebra, CharClassAlgebra, CharClassPred, DecidabilityTier,
    IntervalAlgebra, IntervalPred, PredicateExpr, ProductAlgebra, ProductDomain, ProductPred,
};
use lling_llang::tree_transducers::{
    Tree, TreeChild, TreePattern, TreeTransducerBuilder, TreeTransducerOps,
};
use proptest::prelude::*;

const DEEP_INPUT_DEPTH: usize = 100_000;
const SMALL_NATIVE_STACK: usize = 256 * 1024;

fn assert_lifecycle<T>(value: T, debug_prefix: &str)
where
    T: Clone + Eq + Hash + std::fmt::Debug,
{
    let cloned = value.clone();
    assert_eq!(value, cloned);
    let mut left_hash = DefaultHasher::new();
    value.hash(&mut left_hash);
    let mut right_hash = DefaultHasher::new();
    cloned.hash(&mut right_hash);
    assert_eq!(left_hash.finish(), right_hash.finish());
    assert!(format!("{value:?}").starts_with(debug_prefix));
    drop(cloned);
    drop(value);
}

fn tree_strategy() -> impl Strategy<Value = Tree<u32>> {
    (0u8..8)
        .prop_map(|label| Tree::leaf(u32::from(label)))
        .prop_recursive(8, 256, 4, |inner| {
            (
                (0u8..8).prop_map(u32::from),
                prop::collection::vec(inner, 0..=4),
            )
                .prop_map(|(label, children)| Tree::node(label, children))
        })
}

fn iterative_tree_metrics(tree: &Tree<u32>) -> (usize, usize, Vec<u32>) {
    let mut stack = vec![(tree, 1usize)];
    let mut size = 0usize;
    let mut depth = 0usize;
    let mut labels = Vec::new();
    while let Some((node, node_depth)) = stack.pop() {
        size += 1;
        depth = depth.max(node_depth);
        labels.push(*node.label());
        stack.extend(
            node.children()
                .iter()
                .rev()
                .map(|child| (child, node_depth + 1)),
        );
    }
    (size, depth, labels)
}

fn identity_tree_transducer() -> impl TreeTransducerOps<u32, TropicalWeight> {
    let mut builder = TreeTransducerBuilder::new();
    let state = builder.add_state();
    builder.set_start(state);
    for arity in 0..=4 {
        for label in 0u8..8 {
            builder.add_identity_rule(state, u32::from(label), arity, TropicalWeight::one());
        }
    }
    builder.build()
}

fn presburger_strategy() -> impl Strategy<Value = PresburgerPred> {
    prop_oneof![
        Just(PresburgerPred::True),
        Just(PresburgerPred::False),
        (-32i64..=32)
            .prop_map(|rhs| PresburgerPred::Atom(LinearConstraint::new(vec![(0, 1)], rhs,))),
    ]
    .prop_recursive(8, 256, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|p| PresburgerPred::Not(Box::new(p))),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| PresburgerPred::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner).prop_map(|(a, b)| PresburgerPred::Or(Box::new(a), Box::new(b))),
        ]
    })
}

fn reference_presburger(pred: &PresburgerPred, assignment: &IntAssignment) -> bool {
    match pred {
        PresburgerPred::True => true,
        PresburgerPred::False => false,
        PresburgerPred::Atom(atom) => atom.evaluate(assignment),
        PresburgerPred::And(lhs, rhs) => {
            reference_presburger(lhs, assignment) && reference_presburger(rhs, assignment)
        }
        PresburgerPred::Or(lhs, rhs) => {
            reference_presburger(lhs, assignment) || reference_presburger(rhs, assignment)
        }
        PresburgerPred::Not(inner) => !reference_presburger(inner, assignment),
        PresburgerPred::Exists { .. } => {
            unreachable!("the bounded shallow generator does not emit quantifiers")
        }
    }
}

#[cfg(feature = "smt-z3")]
fn smt_constraint_strategy() -> impl Strategy<Value = SmtConstraint> {
    prop_oneof![
        Just(SmtConstraint::True),
        Just(SmtConstraint::False),
        any::<bool>().prop_map(|value| {
            if value {
                SmtConstraint::BoolVar("p".to_string())
            } else {
                SmtConstraint::BoolVar("q".to_string())
            }
        }),
    ]
    .prop_recursive(8, 256, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|p| SmtConstraint::Not(Box::new(p))),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| SmtConstraint::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner).prop_map(|(a, b)| SmtConstraint::Or(Box::new(a), Box::new(b))),
        ]
    })
}

fn predicate_expr_strategy() -> impl Strategy<Value = PredicateExpr> {
    prop_oneof![
        Just(PredicateExpr::True),
        Just(PredicateExpr::False),
        any::<u8>().prop_map(|value| PredicateExpr::Atom(format!("p{value}"))),
        any::<u8>().prop_map(|value| PredicateExpr::Relation {
            name: format!("r{value}"),
            args: vec!["x".to_string()],
        }),
    ]
    .prop_recursive(7, 192, 3, |inner| {
        prop_oneof![
            inner
                .clone()
                .prop_map(|body| PredicateExpr::Not(Box::new(body))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| PredicateExpr::And(Box::new(left), Box::new(right))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| PredicateExpr::Or(Box::new(left), Box::new(right))),
            inner.clone().prop_map(|body| PredicateExpr::ForallFinite {
                var: "x".to_string(),
                domain: vec!["a".to_string(), "b".to_string()],
                body: Box::new(body),
            }),
            inner
                .clone()
                .prop_map(|body| PredicateExpr::ForallInfinite {
                    var: "x".to_string(),
                    body: Box::new(body),
                }),
            inner.prop_map(|body| PredicateExpr::Bounded {
                body: Box::new(body),
                bound: 8,
            }),
        ]
    })
}

fn reference_decidability(expr: &PredicateExpr, in_bounded: bool) -> DecidabilityTier {
    match expr {
        PredicateExpr::True | PredicateExpr::False | PredicateExpr::Atom(_) => {
            DecidabilityTier::CompileTimeDecidable
        }
        PredicateExpr::Not(inner) => reference_decidability(inner, in_bounded),
        PredicateExpr::And(left, right) | PredicateExpr::Or(left, right) => {
            reference_decidability(left, in_bounded).max(reference_decidability(right, in_bounded))
        }
        PredicateExpr::ForallFinite { body, .. } | PredicateExpr::ExistsFinite { body, .. } => {
            reference_decidability(body, in_bounded)
        }
        PredicateExpr::ForallInfinite { body, .. } | PredicateExpr::ExistsInfinite { body, .. } => {
            if in_bounded {
                reference_decidability(body, true).max(DecidabilityTier::SemiDecidable)
            } else {
                DecidabilityTier::Undecidable
            }
        }
        PredicateExpr::Relation { .. } => DecidabilityTier::RuntimeDecidable,
        PredicateExpr::Bounded { body, .. } => reference_decidability(body, true),
    }
}

#[cfg(feature = "smt-z3")]
fn reference_smt(constraint: &SmtConstraint, model: &SmtModel) -> bool {
    match constraint {
        SmtConstraint::True => true,
        SmtConstraint::False => false,
        SmtConstraint::BoolVar(name) => model.bools.get(name).copied().unwrap_or(false),
        SmtConstraint::Not(inner) => !reference_smt(inner, model),
        SmtConstraint::And(lhs, rhs) => reference_smt(lhs, model) && reference_smt(rhs, model),
        SmtConstraint::Or(lhs, rhs) => reference_smt(lhs, model) || reference_smt(rhs, model),
        SmtConstraint::Eq(_, _)
        | SmtConstraint::Le(_, _)
        | SmtConstraint::Lt(_, _)
        | SmtConstraint::Ge(_, _)
        | SmtConstraint::Gt(_, _) => {
            unreachable!("the bounded shallow generator emits only Boolean nodes")
        }
    }
}

#[derive(Clone, Debug)]
struct TrivialTheory;

impl ConstraintTheory for TrivialTheory {
    type Constraint = ();
    type Assignment = ();
    type Store = ();

    fn empty_store(&self) -> Self::Store {}

    fn propagate(
        &self,
        _store: &Self::Store,
        _constraint: &Self::Constraint,
    ) -> Option<Self::Store> {
        Some(())
    }

    fn is_consistent(&self, _store: &Self::Store) -> bool {
        true
    }

    fn witness(&self, _store: &Self::Store) -> Option<Self::Assignment> {
        Some(())
    }

    fn label(&self, _store: &Self::Store) -> LogicStream<Self::Constraint> {
        LogicStream::empty()
    }

    fn evaluate(&self, _constraint: &Self::Constraint, _assignment: &Self::Assignment) -> bool {
        true
    }
}

fn nested_bounded_domain(values: Vec<String>, limits: &[usize]) -> QDomain {
    limits
        .iter()
        .copied()
        .fold(QDomain::Values(values), |inner, limit| {
            QDomain::Bounded(Box::new(inner), limit)
        })
}

fn token_predicate_strategy() -> impl Strategy<Value = TokenPredicate> {
    prop_oneof![
        Just(TokenPredicate::Any),
        Just(TokenPredicate::Kind(TokenKind::Identifier)),
        any::<u8>().prop_map(|value| TokenPredicate::Text(format!("t{value}"))),
    ]
    .prop_recursive(8, 256, 4, |inner| {
        prop_oneof![
            inner
                .clone()
                .prop_map(|predicate| TokenPredicate::Not(Box::new(predicate))),
            prop::collection::vec(inner.clone(), 0..=4).prop_map(TokenPredicate::Any_),
            prop::collection::vec(inner, 0..=4).prop_map(TokenPredicate::All),
        ]
    })
}

fn reference_token_predicate(predicate: &TokenPredicate, token: &Token) -> bool {
    match predicate {
        TokenPredicate::Any => true,
        TokenPredicate::Text(text) => token.text == *text,
        TokenPredicate::TextCaseInsensitive(text) => token.text.eq_ignore_ascii_case(text),
        TokenPredicate::Kind(kind) => token.kind == *kind,
        TokenPredicate::KindAndText(kind, text) => token.kind == *kind && token.text == *text,
        TokenPredicate::StartsWith(prefix) => token.text.starts_with(prefix),
        TokenPredicate::EndsWith(suffix) => token.text.ends_with(suffix),
        TokenPredicate::Contains(fragment) => token.text.contains(fragment),
        TokenPredicate::Regex(pattern) => regex::Regex::new(pattern)
            .map(|regex| regex.is_match(&token.text))
            .unwrap_or(false),
        TokenPredicate::Any_(predicates) => predicates
            .iter()
            .any(|inner| reference_token_predicate(inner, token)),
        TokenPredicate::All(predicates) => predicates
            .iter()
            .all(|inner| reference_token_predicate(inner, token)),
        TokenPredicate::Not(inner) => !reference_token_predicate(inner, token),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn shallow_tree_fold_and_map_match_iterative_reference(tree in tree_strategy()) {
        let (expected_size, expected_depth, labels) = iterative_tree_metrics(&tree);
        prop_assert_eq!(tree.size(), expected_size);
        prop_assert_eq!(tree.depth(), expected_depth);
        prop_assert_eq!(
            tree.preorder().map(|node| *node.label()).collect::<Vec<_>>(),
            labels.clone(),
        );

        let mapped = tree.map(&|label| label + 1);
        prop_assert_eq!(
            mapped.preorder().map(|node| *node.label()).collect::<Vec<_>>(),
            labels.into_iter().map(|label| label + 1).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn shallow_tree_transducer_preserves_identity_order(tree in tree_strategy()) {
        let transducer = identity_tree_transducer();
        let outputs = transducer.transduce(&tree);
        prop_assert_eq!(outputs.len(), 1);
        let expected = tree.preorder().map(|node| *node.label()).collect::<Vec<_>>();
        let actual = outputs[0].0.preorder().map(|node| *node.label()).collect::<Vec<_>>();
        prop_assert_eq!(actual, expected);
        prop_assert_eq!(outputs[0].1, TropicalWeight::one());
    }

    #[test]
    fn shallow_presburger_evaluation_matches_reference(
        pred in presburger_strategy(),
        value in -64i64..=64,
    ) {
        let assignment = IntAssignment(vec![value]);
        prop_assert_eq!(
            evaluate_presburger(&pred, &assignment, 8),
            reference_presburger(&pred, &assignment),
        );
    }

    #[cfg(feature = "smt-z3")]
    #[test]
    fn shallow_smt_evaluation_matches_reference(
        constraint in smt_constraint_strategy(),
        p in any::<bool>(),
        q in any::<bool>(),
    ) {
        let model = SmtModel {
            bools: HashMap::from([("p".to_string(), p), ("q".to_string(), q)]),
            ..SmtModel::default()
        };
        let theory = Z3Theory::default();
        prop_assert_eq!(
            theory.evaluate(&constraint, &model),
            reference_smt(&constraint, &model),
        );
    }

    #[test]
    fn shallow_interval_negation_matches_parity(
        negations in 0usize..128,
        value in -32i64..=32,
    ) {
        let algebra = IntervalAlgebra::new(-16, 17);
        let mut predicate = IntervalPred::Range(-4, 9);
        for _ in 0..negations {
            predicate = IntervalPred::Not(Box::new(predicate));
        }
        let base = (-4..9).contains(&value) && (-16..17).contains(&value);
        let expected = if negations % 2 == 0 { base } else { !base && (-16..17).contains(&value) };
        prop_assert_eq!(algebra.evaluate(&predicate, &value), expected);
    }

    #[test]
    fn shallow_character_class_negation_matches_parity(
        negations in 0usize..128,
        value in any::<char>(),
    ) {
        let algebra = CharClassAlgebra::new();
        let mut predicate = CharClassPred::Range('a', 'z');
        for _ in 0..negations {
            predicate = CharClassPred::Not(Box::new(predicate));
        }
        let base = value.is_ascii_lowercase();
        let expected = if negations % 2 == 0 { base } else { !base };
        prop_assert_eq!(algebra.evaluate(&predicate, &value), expected);
    }

    #[test]
    fn shallow_decidability_machine_matches_reference(expr in predicate_expr_strategy()) {
        prop_assert_eq!(
            classify_decidability(&expr),
            reference_decidability(&expr, false),
        );
    }

    #[test]
    fn shallow_behavioral_bounds_preserve_sticky_exactness(
        values in prop::collection::vec(any::<u8>(), 0..24),
        limits in prop::collection::vec(0usize..32, 0..12),
    ) {
        let values = values.into_iter().map(|value| value.to_string()).collect::<Vec<_>>();
        let exact = limits.iter().all(|&limit| values.len() <= limit);
        let formula = BehavioralFormula::Exists {
            var: "x".to_string(),
            domain: nested_bounded_domain(values, &limits),
            body: Box::new(BehavioralFormula::Bot),
        };
        let algebra = BehavioralAlgebra::<NoTerm>::new(FactBase::new());
        prop_assert_eq!(
            algebra.is_satisfiable_3v(&formula),
            if exact { Sat3::Unsat } else { Sat3::DontKnow },
        );
    }

    #[test]
    fn shallow_behavioral_quantifiers_match_ordered_reference(
        raw_values in prop::collection::vec(0u8..16, 0..24),
        limits in prop::collection::vec(0usize..32, 0..12),
        relation_holds_for_even in any::<bool>(),
        universal in any::<bool>(),
        negations in 0usize..9,
    ) {
        let mut facts = FactBase::new();
        for value in 0u8..16 {
            if value.is_multiple_of(2) == relation_holds_for_even {
                facts.add_fact("selected", vec![value.to_string()]);
            }
        }
        let mut body = BehavioralFormula::Relation {
            name: "selected".to_string(),
            args: vec![lling_llang::symbolic::behavioral_algebra::Arg::Var("x".to_string())],
        };
        for _ in 0..negations {
            body = BehavioralFormula::Not(Box::new(body));
        }
        let values = raw_values.iter().map(u8::to_string).collect::<Vec<_>>();
        let effective_len = limits
            .iter()
            .copied()
            .fold(values.len(), usize::min);
        let results = raw_values[..effective_len].iter().map(|value| {
            let selected = value.is_multiple_of(2) == relation_holds_for_even;
            if negations.is_multiple_of(2) { selected } else { !selected }
        });
        let expected = if universal {
            results.clone().all(std::convert::identity)
        } else {
            results.clone().any(std::convert::identity)
        };
        let formula = if universal {
            BehavioralFormula::Forall {
                var: "x".to_string(),
                domain: nested_bounded_domain(values, &limits),
                body: Box::new(body),
            }
        } else {
            BehavioralFormula::Exists {
                var: "x".to_string(),
                domain: nested_bounded_domain(values, &limits),
                body: Box::new(body),
            }
        };
        let algebra = BehavioralAlgebra::<NoTerm>::new(facts);
        prop_assert_eq!(
            algebra.evaluate(&formula, &BehavioralWorld::new(NoTerm)),
            expected,
        );
    }

    #[test]
    fn shallow_token_predicates_match_recursive_reference(
        predicate in token_predicate_strategy(),
        identifier in any::<bool>(),
        value in any::<u8>(),
    ) {
        let token = Token::simple(
            if identifier { TokenKind::Identifier } else { TokenKind::Keyword },
            format!("t{value}"),
        );
        prop_assert_eq!(
            predicate.matches(&token),
            reference_token_predicate(&predicate, &token),
        );
    }
}

#[test]
fn deep_decidability_expression_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut expression = PredicateExpr::Atom("p".to_string());
            for _ in 0..DEEP_INPUT_DEPTH {
                expression = PredicateExpr::Not(Box::new(expression));
            }
            assert_eq!(
                classify_decidability(&expression),
                DecidabilityTier::CompileTimeDecidable,
            );
            let cloned = expression.clone();
            assert_eq!(expression, cloned);
            let mut left_hash = DefaultHasher::new();
            expression.hash(&mut left_hash);
            let mut right_hash = DefaultHasher::new();
            cloned.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            assert!(format!("{expression}").starts_with("~("));
            assert!(format!("{expression:?}").starts_with("Not("));
            drop(cloned);
            drop(expression);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("decidability-expression lifecycle must not overflow the native stack");
}

#[test]
fn deep_boolean_test_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut predicate = BooleanTest::True;
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = BooleanTest::Not(Box::new(predicate));
            }
            assert!(eval_test_public(&predicate, &HashMap::new()));
            assert!(predicate.atoms().is_empty());
            assert!(format!("{predicate}").starts_with('~'));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("Boolean-test lifecycle must not overflow the native stack");
}

#[test]
fn deep_product_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra =
                ProductAlgebra::new(IntervalAlgebra::new(-16, 17), CharClassAlgebra::new());
            let mut predicate =
                ProductPred::Both(IntervalPred::Range(-4, 9), CharClassPred::Range('a', 'z'));
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = ProductPred::Not(Box::new(predicate));
            }
            let value = ProductDomain(0, 'm');
            assert!(algebra.evaluate(&predicate, &value));
            assert!(algebra.is_satisfiable(&predicate));
            assert!(format!("{predicate}").starts_with('¬'));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("binary-product predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_nary_product_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = NaryProductAlgebra::new(vec![IntervalAlgebra::new(-16, 17)]);
            let mut predicate = NaryProductPred::Field(0, IntervalPred::Range(-4, 9));
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = NaryProductPred::Not(Box::new(predicate));
            }
            assert!(algebra.evaluate(&predicate, &vec![0]));
            assert!(algebra.is_satisfiable(&predicate));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("n-ary product predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_sum_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = SumAlgebra::new(vec![IntervalAlgebra::new(-16, 17)]);
            let mut predicate = SumPred::InVariant(0, IntervalPred::Range(-4, 9));
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = SumPred::Not(Box::new(predicate));
            }
            let value = SumValue { tag: 0, payload: 0 };
            assert!(algebra.evaluate(&predicate, &value));
            assert!(algebra.is_satisfiable(&predicate));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("sum predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_bag_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = BagAlgebra::new(IntervalAlgebra::new(-16, 17));
            let mut predicate = BagPred::Count {
                class: IntervalPred::Range(-4, 9),
                lo: 1,
                hi: None,
            };
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = BagPred::Not(Box::new(predicate));
            }
            assert!(algebra.evaluate(&predicate, &vec![0]));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("bag predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_map_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra =
                MapAlgebra::new(IntervalAlgebra::new(-16, 17), IntervalAlgebra::new(-16, 17));
            let mut predicate = MapPred::CountEntries {
                key_class: IntervalPred::Range(-4, 9),
                val_class: IntervalPred::Range(-4, 9),
                lo: 1,
                hi: None,
            };
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = MapPred::Not(Box::new(predicate));
            }
            assert!(algebra.evaluate(&predicate, &vec![(0, 0)]));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("map predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_regex_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = RegexAlgebra::new(IntervalAlgebra::new(-16, 17));
            let mut predicate = RegexPred::Empty;
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = RegexPred::Compl(Box::new(predicate));
            }
            assert!(!algebra.evaluate(&predicate, &Vec::new()));
            assert_lifecycle(predicate, "Compl(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("regex predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_theory_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = TheoryAlgebra::new(TrivialTheory, 8);
            let mut predicate = TheoryPred::Atom(());
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = TheoryPred::Not(Box::new(predicate));
            }
            assert!(algebra.is_satisfiable(&predicate));
            assert!(algebra.evaluate(&predicate, &()));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("theory predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_any_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = AnyAlgebra::Int(IntervalAlgebra::new(-16, 17));
            let mut predicate = AnyPred::Int(IntervalPred::Range(-4, 9));
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = AnyPred::Not(Box::new(predicate));
            }
            assert!(algebra.evaluate(&predicate, &AnyDomain::Int(0)));
            assert_lifecycle(predicate, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("uniform predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_quantified_formula_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut formula = QuantifiedFormula::atom("r", Vec::new());
            for _ in 0..DEEP_INPUT_DEPTH {
                formula = QuantifiedFormula::Not(Box::new(formula));
            }
            let env = HashMap::new();
            let relation_query = |_: &str, _: &[String]| true;
            let domain_enumerate = |_: &str| Vec::<Vec<String>>::new();
            assert!(evaluate_quantified(
                &formula,
                &env,
                &relation_query,
                &domain_enumerate,
                8,
            ));
            assert_eq!(
                evaluate_quantified_with_theory(
                    &formula,
                    &TrivialTheory,
                    &relation_query,
                    &domain_enumerate,
                    &env,
                    8,
                ),
                TriState::True,
            );
            assert!(formula.free_vars().is_empty());
            let cloned = formula.clone();
            assert_eq!(formula, cloned);
            assert!(format!("{formula}").starts_with('¬'));
            assert!(format!("{formula:?}").starts_with("Not("));
            drop(cloned);
            drop(formula);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("quantified-formula lifecycle must not overflow the native stack");
}

#[test]
fn deep_behavioral_formula_and_domain_lifecycle_use_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut domain = QDomain::Values(vec!["a".to_string()]);
            for _ in 0..DEEP_INPUT_DEPTH {
                domain = QDomain::Bounded(Box::new(domain), 1);
            }
            let mut body = BehavioralFormula::Top;
            for _ in 0..DEEP_INPUT_DEPTH {
                body = BehavioralFormula::Not(Box::new(body));
            }
            let formula = BehavioralFormula::Forall {
                var: "x".to_string(),
                domain,
                body: Box::new(body),
            };
            let algebra = BehavioralAlgebra::<NoTerm>::new(FactBase::new());
            assert!(algebra.evaluate(&formula, &BehavioralWorld::new(NoTerm)));
            assert_eq!(algebra.is_satisfiable_3v(&formula), Sat3::Sat);
            assert_lifecycle(formula, "Forall {");

            let mut modal = BehavioralFormula::Atom(String::new());
            for _ in 0..DEEP_INPUT_DEPTH {
                modal = BehavioralFormula::Not(Box::new(modal));
            }
            assert!(algebra.evaluate(&modal, &BehavioralWorld::new(NoTerm)));
            assert_eq!(algebra.is_satisfiable_3v(&modal), Sat3::DontKnow);
            assert_lifecycle(modal, "Not(");
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("behavioral formula/domain lifecycle must not overflow the native stack");
}

#[test]
fn deep_token_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let token = Token::simple(TokenKind::Identifier, "identifier");
            let mut predicate = TokenPredicate::Kind(TokenKind::Identifier);
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = TokenPredicate::Not(Box::new(predicate));
            }
            assert!(predicate.matches(&token));
            let cloned = predicate.clone();
            assert!(cloned.matches(&token));
            assert!(format!("{predicate:?}").starts_with("Not("));
            drop(cloned);
            drop(predicate);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("token-predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_interval_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = IntervalAlgebra::new(-16, 17);
            let mut predicate = IntervalPred::Range(-4, 9);
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = IntervalPred::Not(Box::new(predicate));
            }
            assert!(algebra.evaluate(&predicate, &0));
            let cloned = predicate.clone();
            assert_eq!(predicate, cloned);
            let mut left_hash = DefaultHasher::new();
            predicate.hash(&mut left_hash);
            let mut right_hash = DefaultHasher::new();
            cloned.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            assert!(format!("{predicate}").starts_with('~'));
            assert!(format!("{predicate:?}").starts_with("Not("));
            drop(cloned);
            drop(predicate);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("interval-predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_character_class_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = CharClassAlgebra::new();
            let mut predicate = CharClassPred::Range('a', 'z');
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = CharClassPred::Not(Box::new(predicate));
            }
            assert!(algebra.evaluate(&predicate, &'m'));
            let cloned = predicate.clone();
            assert_eq!(predicate, cloned);
            let mut left_hash = DefaultHasher::new();
            predicate.hash(&mut left_hash);
            let mut right_hash = DefaultHasher::new();
            cloned.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            assert!(format!("{predicate}").starts_with('~'));
            assert!(format!("{predicate:?}").starts_with("Not("));
            drop(cloned);
            drop(predicate);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("character-class lifecycle must not overflow the native stack");
}

#[test]
fn deep_tree_operations_use_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut tree = Tree::leaf(0u32);
            for label in 1..DEEP_INPUT_DEPTH as u32 {
                tree = Tree::node(label, vec![tree]);
            }
            assert_eq!(tree.size(), DEEP_INPUT_DEPTH);
            assert_eq!(tree.depth(), DEEP_INPUT_DEPTH);
            let mapped = tree.map(&|label| label + 1);
            assert_eq!(mapped.size(), DEEP_INPUT_DEPTH);
            drop(mapped);
            drop(tree);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("tree operations must not overflow the native stack");
}

#[test]
fn deep_tree_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut tree = Tree::leaf(0u32);
            for label in 1..DEEP_INPUT_DEPTH as u32 {
                tree = Tree::node(label, vec![tree]);
            }
            let cloned = tree.clone();
            assert_eq!(tree, cloned);
            let mut left_hash = DefaultHasher::new();
            tree.hash(&mut left_hash);
            let mut right_hash = DefaultHasher::new();
            cloned.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            assert!(format!("{tree}").starts_with("99999("));
            assert!(format!("{tree:?}").starts_with("Tree(TreeNode"));
            drop(cloned);
            drop(tree);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("tree lifecycle must not overflow the native stack");
}

#[test]
fn deep_tree_transduction_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut tree = Tree::leaf(0u32);
            for _ in 1..DEEP_INPUT_DEPTH {
                tree = Tree::node(1, vec![tree]);
            }
            let mut builder = TreeTransducerBuilder::new();
            let state = builder.add_state();
            builder.set_start(state);
            builder.add_identity_rule(state, 0, 0, TropicalWeight::one());
            builder.add_identity_rule(state, 1, 1, TropicalWeight::one());
            let transducer = builder.build();
            let mut outputs = transducer.transduce(&tree);
            assert_eq!(outputs.len(), 1);
            assert_eq!(outputs[0].0.size(), DEEP_INPUT_DEPTH);
            let output = outputs.pop().expect("one output").0;
            drop(output);
            drop(tree);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("tree transduction must not overflow the native stack");
}

#[test]
fn deep_tree_pattern_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut pattern = TreePattern::leaf(0u32);
            for label in 1..DEEP_INPUT_DEPTH as u32 {
                pattern = TreePattern::new(label, vec![TreeChild::subtree(pattern)]);
            }
            assert!(pattern.variable_indices().is_empty());
            let cloned = pattern.clone();
            assert_eq!(pattern, cloned);
            let mut original_hash = DefaultHasher::new();
            pattern.hash(&mut original_hash);
            let mut cloned_hash = DefaultHasher::new();
            cloned.hash(&mut cloned_hash);
            assert_eq!(original_hash.finish(), cloned_hash.finish());
            assert!(format!("{pattern:?}").starts_with("TreePattern {"));
            drop(cloned);
            drop(pattern);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("tree-pattern lifecycle must not overflow the native stack");
}

#[test]
fn deep_presburger_evaluation_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut pred = PresburgerPred::True;
            for _ in 0..DEEP_INPUT_DEPTH {
                pred = PresburgerPred::Not(Box::new(pred));
            }
            assert!(evaluate_presburger(&pred, &IntAssignment(vec![0]), 8,));
            assert_eq!(pred.num_vars(), 0);
            let cloned = pred.clone();
            assert_eq!(pred, cloned);
            let mut left_hash = DefaultHasher::new();
            pred.hash(&mut left_hash);
            let mut right_hash = DefaultHasher::new();
            cloned.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            assert!(format!("{pred}").starts_with("~("));
            assert!(format!("{pred:?}").starts_with("Not("));
            drop(cloned);
            drop(pred);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("Presburger evaluation must not overflow the native stack");
}

#[test]
#[cfg(feature = "smt-z3")]
fn deep_smt_evaluation_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut constraint = SmtConstraint::True;
            for _ in 0..DEEP_INPUT_DEPTH {
                constraint = SmtConstraint::Not(Box::new(constraint));
            }
            assert!(Z3Theory::default().evaluate(&constraint, &SmtModel::default()));

            let mut term = SmtTerm::IntLit(1);
            for _ in 0..DEEP_INPUT_DEPTH {
                term = SmtTerm::Scale(1, Box::new(term));
            }
            let comparison = SmtConstraint::Eq(term, SmtTerm::IntLit(1));
            assert!(Z3Theory::default().evaluate(&comparison, &SmtModel::default()));
            let cloned_constraint = constraint.clone();
            let cloned_comparison = comparison.clone();
            assert_eq!(constraint, cloned_constraint);
            assert_eq!(comparison, cloned_comparison);
            let mut original_hash = DefaultHasher::new();
            constraint.hash(&mut original_hash);
            let mut cloned_hash = DefaultHasher::new();
            cloned_constraint.hash(&mut cloned_hash);
            assert_eq!(original_hash.finish(), cloned_hash.finish());
            assert!(format!("{constraint:?}").starts_with("Not("));
            assert!(format!("{comparison:?}").starts_with("Eq("));
            drop(cloned_comparison);
            drop(comparison);
            drop(cloned_constraint);
            drop(constraint);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("SMT evaluation must not overflow the native stack");
}

#[test]
fn deep_symbolic_tree_run_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut automaton = SymbolicTreeAutomaton::new(IntervalAlgebra::new(0, 1));
            automaton.register("Leaf", 0);
            automaton.register("Chain", 1);
            let state = automaton.add_state();
            automaton.set_accepting(state);
            automaton.add_transition(TreeTrans {
                constructor: "Leaf".to_string(),
                payload_guard: None,
                child_states: vec![],
                target: state,
            });
            automaton.add_transition(TreeTrans {
                constructor: "Chain".to_string(),
                payload_guard: None,
                child_states: vec![state],
                target: state,
            });

            let mut term = SymTerm::<i64>::constant("Leaf");
            for _ in 0..DEEP_INPUT_DEPTH {
                term = SymTerm::node("Chain", vec![term]);
            }
            assert!(automaton.accepts(&term));
            drop(term);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("symbolic tree execution must not overflow the native stack");
}

#[test]
fn deep_symbolic_term_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut term = SymTerm::<i64>::constant("Leaf");
            for _ in 0..DEEP_INPUT_DEPTH {
                term = SymTerm::node("Chain", vec![term]);
            }
            let cloned = term.clone();
            assert_eq!(term, cloned);
            let mut left_hash = DefaultHasher::new();
            term.hash(&mut left_hash);
            let mut right_hash = DefaultHasher::new();
            cloned.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            assert!(format!("{term:?}").starts_with("SymTerm {"));
            drop(cloned);
            drop(term);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("symbolic-term lifecycle must not overflow the native stack");
}

#[test]
fn deep_tree_algebra_compile_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let algebra = TreeAlgebra::new(
                IntervalAlgebra::new(0, 1),
                HashMap::from([("Leaf".to_string(), 0), ("Chain".to_string(), 1)]),
                Default::default(),
            );
            let mut predicate = TreePred::Wild;
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = TreePred::Node {
                    constructor: "Chain".to_string(),
                    payload_guard: None,
                    children: vec![predicate],
                };
            }
            assert!(algebra.is_satisfiable(&predicate));
            let cloned = predicate.clone();
            assert_eq!(predicate, cloned);
            let mut left_hash = DefaultHasher::new();
            predicate.hash(&mut left_hash);
            let mut right_hash = DefaultHasher::new();
            cloned.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            assert!(format!("{predicate:?}").starts_with("Node {"));
            drop(cloned);
            drop(predicate);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("tree-algebra compilation must not overflow the native stack");
}

#[test]
fn deep_repair_action_operations_use_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut action = RepairAction::Insert {
                position: Position {
                    line: 0,
                    column: 0,
                    byte_offset: 0,
                },
                text: "x".to_string(),
            };
            for _ in 0..DEEP_INPUT_DEPTH {
                action = RepairAction::Multiple(vec![action]);
            }
            assert_eq!(action.cost(&SyntaxRepairCosts::default()), 1.0);
            assert_eq!(action.apply(""), "x");
            assert!(format!("{action}").starts_with('['));
            let cloned = action.clone();
            assert_eq!(action, cloned);
            assert!(format!("{action:?}").starts_with("Multiple(["));
            drop(cloned);
            drop(action);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("repair-action operations must not overflow the native stack");
}

#[test]
#[cfg(feature = "mathml-semantic")]
fn deep_math_type_unification_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut left = MathType::Number;
            let mut right = MathType::Number;
            for _ in 0..DEEP_INPUT_DEPTH {
                left = MathType::Vector {
                    element: Box::new(left),
                    dimension: None,
                };
                right = MathType::Vector {
                    element: Box::new(right),
                    dimension: None,
                };
            }
            let unified = MathTypeChecker::new()
                .unify(&left, &right)
                .expect("identical vector towers unify");
            assert_eq!(left, right);
            assert_eq!(left, unified);
            let cloned = unified.clone();
            let mut original_hash = DefaultHasher::new();
            unified.hash(&mut original_hash);
            let mut cloned_hash = DefaultHasher::new();
            cloned.hash(&mut cloned_hash);
            assert_eq!(original_hash.finish(), cloned_hash.finish());
            assert!(format!("{unified}").starts_with("Vec<"));
            assert!(format!("{unified:?}").starts_with("Vector {"));
            drop(cloned);
            drop(unified);
            drop(right);
            drop(left);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("math-type unification must not overflow the native stack");
}
