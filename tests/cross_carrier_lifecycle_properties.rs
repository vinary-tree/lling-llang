//! Refinement properties for the typed cross-carrier lifecycle machine.
//!
//! Bounded generators compare recursive reference traces with cloned public
//! values.  Deep tests exercise the same observable lifecycle surfaces at
//! 100,000 logical levels on a 256 KiB native stack.  The recursive reference
//! walkers are intentionally test-only and are never used by production code.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

#[cfg(feature = "f1r3fly")]
use lling_llang::layers::syntactic::mettail_type::TypeExpr;
use lling_llang::llm::JsonType;
use lling_llang::symbolic::any_algebra::{AnyAlgebra, AnyDomain, AnyPred};
use lling_llang::symbolic::behavioral_pred::{
    BehavioralPred, PredArg, QuantifiedDomain, Quantifier,
};
use lling_llang::symbolic::collection_algebra::{BagAlgebra, BagPred, MapAlgebra, MapPred};
use lling_llang::symbolic::product_nary::{
    NaryProductAlgebra, NaryProductPred, SumAlgebra, SumPred, SumValue,
};
use lling_llang::symbolic::regex_sfa::{RegexAlgebra, RegexPred};
use lling_llang::symbolic::string_algebra::StrPred;
use lling_llang::symbolic::sym_tree::{SymTerm, TreeAlgebra, TreePred};
use lling_llang::symbolic::{CharClassPred, IntervalAlgebra};
use proptest::prelude::*;

const DEEP_INPUT_DEPTH: usize = 100_000;
const SMALL_NATIVE_STACK: usize = 256 * 1024;

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

fn any_domain_strategy() -> impl Strategy<Value = AnyDomain> {
    prop_oneof![
        any::<i16>().prop_map(|value| AnyDomain::Int(i64::from(value))),
        any::<char>().prop_map(AnyDomain::Char),
        "[a-z]{0,8}".prop_map(AnyDomain::Str),
    ]
    .prop_recursive(7, 192, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..=3).prop_map(AnyDomain::Product),
            prop::collection::vec(inner.clone(), 0..=3).prop_map(AnyDomain::List),
            prop::collection::vec(inner.clone(), 0..=3).prop_map(AnyDomain::Bag),
            inner
                .clone()
                .prop_map(|payload| AnyDomain::Sum(Box::new(SumValue { tag: 3, payload }))),
            prop::collection::vec((inner.clone(), inner.clone()), 0..=3).prop_map(AnyDomain::Map),
            (inner.clone(), prop::collection::vec(inner, 0..=3),).prop_map(
                |(payload, children)| {
                    AnyDomain::Tree(Box::new(SymTerm {
                        constructor: "Node".to_string(),
                        payload: Some(payload),
                        children: children
                            .into_iter()
                            .map(|child| SymTerm::leaf("Child", child))
                            .collect(),
                    }))
                }
            ),
        ]
    })
}

enum DomainVisit<'a> {
    Domain(&'a AnyDomain),
    Term(&'a SymTerm<AnyDomain>),
}

fn domain_event(value: &AnyDomain) -> String {
    match value {
        AnyDomain::Int(value) => format!("Int:{value}"),
        AnyDomain::Char(value) => format!("Char:{value:?}"),
        AnyDomain::Bool(value) => format!("Bool:{}", value.len()),
        AnyDomain::BigInt(value) => format!("BigInt:{value}"),
        AnyDomain::BigRat(value) => format!("BigRat:{value}"),
        AnyDomain::Fixed(value) => format!("Fixed:{value}"),
        AnyDomain::Float(value) => format!("Float:{value:?}"),
        AnyDomain::Str(value) => format!("Str:{value:?}"),
        AnyDomain::Product(values) => format!("Product:{}", values.len()),
        AnyDomain::Sum(value) => format!("Sum:{}", value.tag),
        AnyDomain::List(values) => format!("List:{}", values.len()),
        AnyDomain::Bag(values) => format!("Bag:{}", values.len()),
        AnyDomain::Tree(_) => "Tree".to_string(),
        AnyDomain::Map(values) => format!("Map:{}", values.len()),
    }
}

fn recursive_domain_trace(value: &AnyDomain, trace: &mut Vec<String>) {
    trace.push(domain_event(value));
    match value {
        AnyDomain::Product(values) | AnyDomain::List(values) | AnyDomain::Bag(values) => {
            for child in values {
                recursive_domain_trace(child, trace);
            }
        }
        AnyDomain::Sum(value) => recursive_domain_trace(&value.payload, trace),
        AnyDomain::Tree(term) => recursive_term_trace(term, trace),
        AnyDomain::Map(values) => {
            for (key, value) in values {
                recursive_domain_trace(key, trace);
                recursive_domain_trace(value, trace);
            }
        }
        AnyDomain::Int(_)
        | AnyDomain::Char(_)
        | AnyDomain::Bool(_)
        | AnyDomain::BigInt(_)
        | AnyDomain::BigRat(_)
        | AnyDomain::Fixed(_)
        | AnyDomain::Float(_)
        | AnyDomain::Str(_) => {}
    }
}

fn recursive_term_trace(term: &SymTerm<AnyDomain>, trace: &mut Vec<String>) {
    trace.push(format!(
        "Term:{:?}:{}:{}",
        term.constructor,
        term.payload.is_some(),
        term.children.len()
    ));
    if let Some(payload) = &term.payload {
        recursive_domain_trace(payload, trace);
    }
    for child in &term.children {
        recursive_term_trace(child, trace);
    }
}

fn iterative_domain_trace(value: &AnyDomain) -> Vec<String> {
    let mut trace = Vec::new();
    let mut pending = vec![DomainVisit::Domain(value)];
    while let Some(node) = pending.pop() {
        match node {
            DomainVisit::Domain(value) => {
                trace.push(domain_event(value));
                match value {
                    AnyDomain::Product(values)
                    | AnyDomain::List(values)
                    | AnyDomain::Bag(values) => {
                        pending.extend(values.iter().rev().map(DomainVisit::Domain));
                    }
                    AnyDomain::Sum(value) => {
                        pending.push(DomainVisit::Domain(&value.payload));
                    }
                    AnyDomain::Tree(term) => pending.push(DomainVisit::Term(term)),
                    AnyDomain::Map(values) => {
                        for (key, value) in values.iter().rev() {
                            pending.push(DomainVisit::Domain(value));
                            pending.push(DomainVisit::Domain(key));
                        }
                    }
                    AnyDomain::Int(_)
                    | AnyDomain::Char(_)
                    | AnyDomain::Bool(_)
                    | AnyDomain::BigInt(_)
                    | AnyDomain::BigRat(_)
                    | AnyDomain::Fixed(_)
                    | AnyDomain::Float(_)
                    | AnyDomain::Str(_) => {}
                }
            }
            DomainVisit::Term(term) => {
                trace.push(format!(
                    "Term:{:?}:{}:{}",
                    term.constructor,
                    term.payload.is_some(),
                    term.children.len()
                ));
                pending.extend(term.children.iter().rev().map(DomainVisit::Term));
                if let Some(payload) = &term.payload {
                    pending.push(DomainVisit::Domain(payload));
                }
            }
        }
    }
    trace
}

fn any_algebra_strategy() -> impl Strategy<Value = AnyAlgebra> {
    Just(AnyAlgebra::Int(IntervalAlgebra::new(-32, 33))).prop_recursive(7, 192, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..=3).prop_map(|fields| {
                AnyAlgebra::Product(Box::new(NaryProductAlgebra::new(fields)))
            }),
            prop::collection::vec(inner.clone(), 0..=3)
                .prop_map(|variants| { AnyAlgebra::Sum(Box::new(SumAlgebra::new(variants))) }),
            inner
                .clone()
                .prop_map(|elem| { AnyAlgebra::List(Box::new(RegexAlgebra::new(elem))) }),
            inner
                .clone()
                .prop_map(|elem| { AnyAlgebra::Bag(Box::new(BagAlgebra::new(elem))) }),
            inner.clone().prop_map(|elem| {
                AnyAlgebra::Tree(Box::new(TreeAlgebra::new(
                    elem,
                    HashMap::from([("Leaf".to_string(), 0)]),
                    HashSet::new(),
                )))
            }),
            (inner.clone(), inner).prop_map(|(key, value)| {
                AnyAlgebra::Map(Box::new(MapAlgebra::new(key, value)))
            }),
        ]
    })
}

fn recursive_algebra_trace(algebra: &AnyAlgebra, trace: &mut Vec<String>) {
    trace.push(format!("{:?}", algebra.sort()));
    match algebra {
        AnyAlgebra::Product(inner) => {
            for field in &inner.fields {
                recursive_algebra_trace(field, trace);
            }
        }
        AnyAlgebra::Sum(inner) => {
            for variant in &inner.variants {
                recursive_algebra_trace(variant, trace);
            }
        }
        AnyAlgebra::List(inner) => recursive_algebra_trace(&inner.elem, trace),
        AnyAlgebra::Bag(inner) => recursive_algebra_trace(&inner.elem, trace),
        AnyAlgebra::Tree(inner) => recursive_algebra_trace(&inner.elem, trace),
        AnyAlgebra::Map(inner) => {
            recursive_algebra_trace(&inner.key, trace);
            recursive_algebra_trace(&inner.val, trace);
        }
        AnyAlgebra::Int(_)
        | AnyAlgebra::Char(_)
        | AnyAlgebra::Bool(_)
        | AnyAlgebra::BigInt(_)
        | AnyAlgebra::BigRat(_)
        | AnyAlgebra::Fixed(_)
        | AnyAlgebra::Float(_)
        | AnyAlgebra::Str(_) => {}
    }
}

fn iterative_algebra_trace(algebra: &AnyAlgebra) -> Vec<String> {
    let mut trace = Vec::new();
    let mut pending = vec![algebra];
    while let Some(algebra) = pending.pop() {
        trace.push(format!("{:?}", algebra.sort()));
        match algebra {
            AnyAlgebra::Product(inner) => pending.extend(inner.fields.iter().rev()),
            AnyAlgebra::Sum(inner) => pending.extend(inner.variants.iter().rev()),
            AnyAlgebra::List(inner) => pending.push(&inner.elem),
            AnyAlgebra::Bag(inner) => pending.push(&inner.elem),
            AnyAlgebra::Tree(inner) => pending.push(&inner.elem),
            AnyAlgebra::Map(inner) => {
                pending.push(&inner.val);
                pending.push(&inner.key);
            }
            AnyAlgebra::Int(_)
            | AnyAlgebra::Char(_)
            | AnyAlgebra::Bool(_)
            | AnyAlgebra::BigInt(_)
            | AnyAlgebra::BigRat(_)
            | AnyAlgebra::Fixed(_)
            | AnyAlgebra::Float(_)
            | AnyAlgebra::Str(_) => {}
        }
    }
    trace
}

fn any_predicate_strategy() -> impl Strategy<Value = AnyPred> {
    any::<i16>()
        .prop_map(|value| {
            AnyPred::Int(lling_llang::symbolic::IntervalPred::Range(
                i64::from(value),
                i64::from(value) + 1,
            ))
        })
        .prop_recursive(7, 192, 4, |inner| {
            prop_oneof![
                inner
                    .clone()
                    .prop_map(|predicate| AnyPred::Not(Box::new(predicate))),
                (inner.clone(), inner.clone())
                    .prop_map(|(left, right)| { AnyPred::And(Box::new(left), Box::new(right)) }),
                inner.clone().prop_map(|predicate| {
                    AnyPred::Product(Box::new(NaryProductPred::Field(0, predicate)))
                }),
                inner.clone().prop_map(|predicate| {
                    AnyPred::Sum(Box::new(SumPred::InVariant(0, predicate)))
                }),
                inner
                    .clone()
                    .prop_map(|predicate| { AnyPred::List(Box::new(RegexPred::Elem(predicate))) }),
                inner.clone().prop_map(|predicate| {
                    AnyPred::Bag(Box::new(BagPred::Count {
                        class: predicate,
                        lo: 1,
                        hi: Some(2),
                    }))
                }),
                inner.clone().prop_map(|predicate| {
                    AnyPred::Tree(Box::new(TreePred::Node {
                        constructor: "Leaf".to_string(),
                        payload_guard: Some(predicate),
                        children: Vec::new(),
                    }))
                }),
                (inner.clone(), inner).prop_map(|(key, value)| {
                    AnyPred::Map(Box::new(MapPred::CountEntries {
                        key_class: key,
                        val_class: value,
                        lo: 1,
                        hi: Some(2),
                    }))
                }),
            ]
        })
}

fn predicate_event(predicate: &AnyPred) -> &'static str {
    match predicate {
        AnyPred::True => "True",
        AnyPred::False => "False",
        AnyPred::Int(_) => "Int",
        AnyPred::Char(_) => "Char",
        AnyPred::Bool(_) => "Bool",
        AnyPred::BigInt(_) => "BigInt",
        AnyPred::BigRat(_) => "BigRat",
        AnyPred::Fixed(_) => "Fixed",
        AnyPred::Float(_) => "Float",
        AnyPred::Str(_) => "Str",
        AnyPred::Product(_) => "Product",
        AnyPred::Sum(_) => "Sum",
        AnyPred::List(_) => "List",
        AnyPred::Bag(_) => "Bag",
        AnyPred::Tree(_) => "Tree",
        AnyPred::Map(_) => "Map",
        AnyPred::And(_, _) => "And",
        AnyPred::Or(_, _) => "Or",
        AnyPred::Not(_) => "Not",
    }
}

fn recursive_any_predicate_trace(predicate: &AnyPred, trace: &mut Vec<&'static str>) {
    trace.push(predicate_event(predicate));
    match predicate {
        AnyPred::Product(inner) => {
            if let NaryProductPred::Field(_, nested) = &**inner {
                recursive_any_predicate_trace(nested, trace);
            }
        }
        AnyPred::Sum(inner) => {
            if let SumPred::InVariant(_, nested) = &**inner {
                recursive_any_predicate_trace(nested, trace);
            }
        }
        AnyPred::List(inner) => {
            if let RegexPred::Elem(nested) = &**inner {
                recursive_any_predicate_trace(nested, trace);
            }
        }
        AnyPred::Bag(inner) => {
            if let BagPred::Count { class, .. } = &**inner {
                recursive_any_predicate_trace(class, trace);
            }
        }
        AnyPred::Tree(inner) => {
            if let TreePred::Node {
                payload_guard: Some(nested),
                ..
            } = &**inner
            {
                recursive_any_predicate_trace(nested, trace);
            }
        }
        AnyPred::Map(inner) => {
            if let MapPred::CountEntries {
                key_class,
                val_class,
                ..
            } = &**inner
            {
                recursive_any_predicate_trace(key_class, trace);
                recursive_any_predicate_trace(val_class, trace);
            }
        }
        AnyPred::And(left, right) | AnyPred::Or(left, right) => {
            recursive_any_predicate_trace(left, trace);
            recursive_any_predicate_trace(right, trace);
        }
        AnyPred::Not(inner) => recursive_any_predicate_trace(inner, trace),
        AnyPred::True
        | AnyPred::False
        | AnyPred::Int(_)
        | AnyPred::Char(_)
        | AnyPred::Bool(_)
        | AnyPred::BigInt(_)
        | AnyPred::BigRat(_)
        | AnyPred::Fixed(_)
        | AnyPred::Float(_)
        | AnyPred::Str(_) => {}
    }
}

fn iterative_any_predicate_trace(predicate: &AnyPred) -> Vec<&'static str> {
    let mut trace = Vec::new();
    let mut pending = vec![predicate];
    while let Some(predicate) = pending.pop() {
        trace.push(predicate_event(predicate));
        match predicate {
            AnyPred::Product(inner) => {
                if let NaryProductPred::Field(_, nested) = &**inner {
                    pending.push(nested);
                }
            }
            AnyPred::Sum(inner) => {
                if let SumPred::InVariant(_, nested) = &**inner {
                    pending.push(nested);
                }
            }
            AnyPred::List(inner) => {
                if let RegexPred::Elem(nested) = &**inner {
                    pending.push(nested);
                }
            }
            AnyPred::Bag(inner) => {
                if let BagPred::Count { class, .. } = &**inner {
                    pending.push(class);
                }
            }
            AnyPred::Tree(inner) => {
                if let TreePred::Node {
                    payload_guard: Some(nested),
                    ..
                } = &**inner
                {
                    pending.push(nested);
                }
            }
            AnyPred::Map(inner) => {
                if let MapPred::CountEntries {
                    key_class,
                    val_class,
                    ..
                } = &**inner
                {
                    pending.push(val_class);
                    pending.push(key_class);
                }
            }
            AnyPred::And(left, right) | AnyPred::Or(left, right) => {
                pending.push(right);
                pending.push(left);
            }
            AnyPred::Not(inner) => pending.push(inner),
            AnyPred::True
            | AnyPred::False
            | AnyPred::Int(_)
            | AnyPred::Char(_)
            | AnyPred::Bool(_)
            | AnyPred::BigInt(_)
            | AnyPred::BigRat(_)
            | AnyPred::Fixed(_)
            | AnyPred::Float(_)
            | AnyPred::Str(_) => {}
        }
    }
    trace
}

fn behavioral_strategy() -> impl Strategy<Value = BehavioralPred> {
    prop_oneof![
        Just(BehavioralPred::Top),
        any::<u8>().prop_map(|value| BehavioralPred::RelationQuery {
            relation_name: format!("r{value}"),
            args: vec![
                PredArg::Var("x".to_string()),
                PredArg::IntLit(i64::from(value))
            ],
            negated: value.is_multiple_of(2),
        }),
        any::<u8>().prop_map(|value| BehavioralPred::AcMatch {
            bag: PredArg::Var("bag".to_string()),
            elements: vec![PredArg::IntLit(i64::from(value))],
            rest: Some("rest".to_string()),
        }),
    ]
    .prop_recursive(7, 192, 4, |inner| {
        prop_oneof![
            inner
                .clone()
                .prop_map(|body| BehavioralPred::Not(Box::new(body))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| { BehavioralPred::And(Box::new(left), Box::new(right)) }),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| { BehavioralPred::Or(Box::new(left), Box::new(right)) }),
            (inner.clone(), inner.clone()).prop_map(|(premise, conclusion)| {
                BehavioralPred::Implies(Box::new(premise), Box::new(conclusion))
            }),
            inner.prop_map(|body| BehavioralPred::Quantified {
                quantifier: Quantifier::ForAll,
                var: "bound".to_string(),
                domain: Some(QuantifiedDomain::Enumerated(vec![PredArg::Var(
                    "x".to_string(),
                )])),
                body: Box::new(body),
            }),
        ]
    })
}

fn recursive_behavioral_trace(value: &BehavioralPred, trace: &mut Vec<&'static str>) {
    use BehavioralPred::*;
    trace.push(match value {
        RelationQuery { .. } => "RelationQuery",
        Quantified { .. } => "Quantified",
        AcMatch { .. } => "AcMatch",
        And(_, _) => "And",
        Or(_, _) => "Or",
        Not(_) => "Not",
        Implies(_, _) => "Implies",
        Top => "Top",
    });
    match value {
        Quantified { body, .. } | Not(body) => recursive_behavioral_trace(body, trace),
        And(left, right) | Or(left, right) | Implies(left, right) => {
            recursive_behavioral_trace(left, trace);
            recursive_behavioral_trace(right, trace);
        }
        RelationQuery { .. } | AcMatch { .. } | Top => {}
    }
}

fn iterative_behavioral_trace(value: &BehavioralPred) -> Vec<&'static str> {
    use BehavioralPred::*;
    let mut trace = Vec::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        trace.push(match value {
            RelationQuery { .. } => "RelationQuery",
            Quantified { .. } => "Quantified",
            AcMatch { .. } => "AcMatch",
            And(_, _) => "And",
            Or(_, _) => "Or",
            Not(_) => "Not",
            Implies(_, _) => "Implies",
            Top => "Top",
        });
        match value {
            Quantified { body, .. } | Not(body) => pending.push(body),
            And(left, right) | Or(left, right) | Implies(left, right) => {
                pending.push(right);
                pending.push(left);
            }
            RelationQuery { .. } | AcMatch { .. } | Top => {}
        }
    }
    trace
}

fn string_predicate_strategy() -> impl Strategy<Value = StrPred> {
    prop_oneof![
        Just(StrPred::Empty),
        Just(StrPred::Epsilon),
        (0u8..26).prop_map(|offset| {
            let value = char::from(b'a' + offset);
            StrPred::Class(CharClassPred::Range(value, value))
        }),
        "[a-z]{0,8}".prop_map(StrPred::Literal),
        (0usize..8, prop::option::of(0usize..12)).prop_map(|(lo, hi)| StrPred::Length(lo, hi)),
    ]
    .prop_recursive(7, 192, 4, |inner| {
        prop_oneof![
            inner
                .clone()
                .prop_map(|value| StrPred::Star(Box::new(value))),
            inner
                .clone()
                .prop_map(|value| StrPred::Compl(Box::new(value))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| StrPred::Concat(Box::new(left), Box::new(right))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| StrPred::Alt(Box::new(left), Box::new(right))),
            (inner.clone(), inner)
                .prop_map(|(left, right)| StrPred::Inter(Box::new(left), Box::new(right))),
        ]
    })
}

fn recursive_string_trace(value: &StrPred, trace: &mut Vec<&'static str>) {
    use StrPred::*;
    trace.push(match value {
        Empty => "Empty",
        Epsilon => "Epsilon",
        Class(_) => "Class",
        Literal(_) => "Literal",
        Length(_, _) => "Length",
        Concat(_, _) => "Concat",
        Alt(_, _) => "Alt",
        Star(_) => "Star",
        Inter(_, _) => "Inter",
        Compl(_) => "Compl",
    });
    match value {
        Concat(left, right) | Alt(left, right) | Inter(left, right) => {
            recursive_string_trace(left, trace);
            recursive_string_trace(right, trace);
        }
        Star(inner) | Compl(inner) => recursive_string_trace(inner, trace),
        Empty | Epsilon | Class(_) | Literal(_) | Length(_, _) => {}
    }
}

fn iterative_string_trace(value: &StrPred) -> Vec<&'static str> {
    use StrPred::*;
    let mut trace = Vec::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        trace.push(match value {
            Empty => "Empty",
            Epsilon => "Epsilon",
            Class(_) => "Class",
            Literal(_) => "Literal",
            Length(_, _) => "Length",
            Concat(_, _) => "Concat",
            Alt(_, _) => "Alt",
            Star(_) => "Star",
            Inter(_, _) => "Inter",
            Compl(_) => "Compl",
        });
        match value {
            Concat(left, right) | Alt(left, right) | Inter(left, right) => {
                pending.push(right);
                pending.push(left);
            }
            Star(inner) | Compl(inner) => pending.push(inner),
            Empty | Epsilon | Class(_) | Literal(_) | Length(_, _) => {}
        }
    }
    trace
}

fn json_type_strategy() -> impl Strategy<Value = JsonType> {
    prop_oneof![
        Just(JsonType::String),
        Just(JsonType::Number),
        Just(JsonType::Integer),
        Just(JsonType::Boolean),
        Just(JsonType::Null),
        Just(JsonType::Object),
        Just(JsonType::Any),
    ]
    .prop_recursive(8, 192, 1, |inner| {
        inner.prop_map(|value| JsonType::Array(Box::new(value)))
    })
}

fn recursive_json_trace(value: &JsonType, trace: &mut Vec<&'static str>) {
    trace.push(match value {
        JsonType::String => "String",
        JsonType::Number => "Number",
        JsonType::Integer => "Integer",
        JsonType::Boolean => "Boolean",
        JsonType::Null => "Null",
        JsonType::Array(_) => "Array",
        JsonType::Object => "Object",
        JsonType::Any => "Any",
    });
    if let JsonType::Array(inner) = value {
        recursive_json_trace(inner, trace);
    }
}

fn iterative_json_trace(value: &JsonType) -> Vec<&'static str> {
    let mut trace = Vec::new();
    let mut current = value;
    loop {
        trace.push(match current {
            JsonType::String => "String",
            JsonType::Number => "Number",
            JsonType::Integer => "Integer",
            JsonType::Boolean => "Boolean",
            JsonType::Null => "Null",
            JsonType::Array(_) => "Array",
            JsonType::Object => "Object",
            JsonType::Any => "Any",
        });
        match current {
            JsonType::Array(inner) => current = inner,
            JsonType::String
            | JsonType::Number
            | JsonType::Integer
            | JsonType::Boolean
            | JsonType::Null
            | JsonType::Object
            | JsonType::Any => return trace,
        }
    }
}

#[cfg(feature = "f1r3fly")]
fn type_expression_strategy() -> impl Strategy<Value = TypeExpr> {
    prop_oneof![
        "[A-Z][a-z]{0,7}".prop_map(TypeExpr::Base),
        "[a-z]{1,8}".prop_map(TypeExpr::Variable),
    ]
    .prop_recursive(7, 192, 4, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(input, output)| {
                TypeExpr::Function(Box::new(input), Box::new(output))
            }),
            (inner.clone(), prop::collection::vec(inner, 0..=3)).prop_map(
                |(constructor, arguments)| {
                    TypeExpr::Application(Box::new(constructor), arguments)
                }
            ),
        ]
    })
}

#[cfg(feature = "f1r3fly")]
fn recursive_type_expression_trace(value: &TypeExpr, trace: &mut Vec<&'static str>) {
    match value {
        TypeExpr::Base(_) => trace.push("Base"),
        TypeExpr::Variable(_) => trace.push("Variable"),
        TypeExpr::Function(input, output) => {
            trace.push("Function");
            recursive_type_expression_trace(input, trace);
            recursive_type_expression_trace(output, trace);
        }
        TypeExpr::Application(constructor, arguments) => {
            trace.push("Application");
            recursive_type_expression_trace(constructor, trace);
            for argument in arguments {
                recursive_type_expression_trace(argument, trace);
            }
        }
    }
}

#[cfg(feature = "f1r3fly")]
fn iterative_type_expression_trace(value: &TypeExpr) -> Vec<&'static str> {
    let mut trace = Vec::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            TypeExpr::Base(_) => trace.push("Base"),
            TypeExpr::Variable(_) => trace.push("Variable"),
            TypeExpr::Function(input, output) => {
                trace.push("Function");
                pending.push(output);
                pending.push(input);
            }
            TypeExpr::Application(constructor, arguments) => {
                trace.push("Application");
                pending.extend(arguments.iter().rev());
                pending.push(constructor);
            }
        }
    }
    trace
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn any_domain_clone_preserves_typed_recursive_preorder(value in any_domain_strategy()) {
        let mut expected = Vec::new();
        recursive_domain_trace(&value, &mut expected);
        let cloned = value.clone();
        prop_assert_eq!(iterative_domain_trace(&cloned), expected);
        prop_assert_eq!(&cloned, &value);
        prop_assert_eq!(format!("{cloned:?}"), format!("{value:?}"));
    }

    #[test]
    fn any_algebra_clone_preserves_typed_recursive_preorder(value in any_algebra_strategy()) {
        let mut expected = Vec::new();
        recursive_algebra_trace(&value, &mut expected);
        let cloned = value.clone();
        prop_assert_eq!(iterative_algebra_trace(&cloned), expected);
        prop_assert_eq!(format!("{:?}", cloned.sort()), format!("{:?}", value.sort()));
    }

    #[test]
    fn any_predicate_lifecycle_preserves_wrapper_crossing_preorder(value in any_predicate_strategy()) {
        let mut expected = Vec::new();
        recursive_any_predicate_trace(&value, &mut expected);
        let cloned = value.clone();
        prop_assert_eq!(iterative_any_predicate_trace(&cloned), expected);
        prop_assert_eq!(&cloned, &value);
        prop_assert_eq!(hash_of(&cloned), hash_of(&value));
        prop_assert_eq!(format!("{cloned:?}"), format!("{value:?}"));
    }

    #[test]
    fn behavioral_lifecycle_preserves_recursive_preorder(value in behavioral_strategy()) {
        let mut expected = Vec::new();
        recursive_behavioral_trace(&value, &mut expected);
        let cloned = value.clone();
        prop_assert_eq!(iterative_behavioral_trace(&cloned), expected);
        prop_assert_eq!(&cloned, &value);
        prop_assert_eq!(hash_of(&cloned), hash_of(&value));
        prop_assert_eq!(format!("{cloned:?}"), format!("{value:?}"));
        prop_assert_eq!(cloned.to_string(), value.to_string());
        prop_assert_eq!(cloned.free_vars(), value.free_vars());
        prop_assert_eq!(cloned.substitute_var("x", "renamed"), value.substitute_var("x", "renamed"));
    }

    #[test]
    fn string_predicate_lifecycle_preserves_recursive_preorder(value in string_predicate_strategy()) {
        let mut expected = Vec::new();
        recursive_string_trace(&value, &mut expected);
        let cloned = value.clone();
        prop_assert_eq!(iterative_string_trace(&cloned), expected);
        prop_assert_eq!(&cloned, &value);
        prop_assert_eq!(hash_of(&cloned), hash_of(&value));
        prop_assert_eq!(format!("{cloned:?}"), format!("{value:?}"));
    }

    #[test]
    fn json_type_lifecycle_preserves_recursive_preorder(value in json_type_strategy()) {
        let mut expected = Vec::new();
        recursive_json_trace(&value, &mut expected);
        let cloned = value.clone();
        prop_assert_eq!(iterative_json_trace(&cloned), expected);
        prop_assert_eq!(&cloned, &value);
        prop_assert_eq!(format!("{cloned:?}"), format!("{value:?}"));
    }

    #[test]
    #[cfg(feature = "f1r3fly")]
    fn type_expression_lifecycle_preserves_recursive_preorder(value in type_expression_strategy()) {
        let mut expected = Vec::new();
        recursive_type_expression_trace(&value, &mut expected);
        let cloned = value.clone();
        prop_assert_eq!(iterative_type_expression_trace(&cloned), expected);
        prop_assert_eq!(&cloned, &value);
        prop_assert_eq!(format!("{cloned:?}"), format!("{value:?}"));
    }
}

#[test]
fn deep_any_domain_cross_carrier_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut value = AnyDomain::Int(7);
            for _ in 0..DEEP_INPUT_DEPTH {
                value = AnyDomain::Tree(Box::new(SymTerm::leaf("Node", value)));
            }
            let cloned = value.clone();
            assert_eq!(value, cloned);
            assert!(format!("{value:?}").starts_with("Tree(SymTerm {"));
            drop(cloned);
            drop(value);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("uniform-domain lifecycle must not overflow the native stack");
}

#[test]
fn deep_any_algebra_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut algebra = AnyAlgebra::Int(IntervalAlgebra::new(-8, 9));
            for _ in 0..DEEP_INPUT_DEPTH {
                algebra = AnyAlgebra::List(Box::new(RegexAlgebra::new(algebra)));
            }
            let cloned = algebra.clone();
            assert_eq!(algebra.sort(), cloned.sort());
            assert!(format!("{algebra:?}").starts_with("List(RegexAlgebra {"));
            drop(cloned);
            drop(algebra);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("uniform-algebra lifecycle must not overflow the native stack");
}

#[test]
fn deep_any_predicate_wrapper_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut predicate = AnyPred::Int(lling_llang::symbolic::IntervalPred::Range(-4, 9));
            for _ in 0..DEEP_INPUT_DEPTH {
                predicate = AnyPred::Product(Box::new(NaryProductPred::Field(0, predicate)));
            }
            let cloned = predicate.clone();
            assert_eq!(predicate, cloned);
            assert_eq!(hash_of(&predicate), hash_of(&cloned));
            assert!(format!("{predicate:?}").starts_with("Product(Field("));
            drop(cloned);
            drop(predicate);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("uniform-predicate wrapper lifecycle must not overflow the native stack");
}

#[test]
fn deep_behavioral_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut value = BehavioralPred::RelationQuery {
                relation_name: "r".to_string(),
                args: vec![PredArg::Var("x".to_string())],
                negated: false,
            };
            for _ in 0..DEEP_INPUT_DEPTH {
                value = BehavioralPred::Not(Box::new(value));
            }
            assert_eq!(value.free_vars(), HashSet::from(["x".to_string()]));
            let substituted = value.substitute_var("x", "y");
            let cloned = value.clone();
            assert_eq!(value, cloned);
            assert_eq!(hash_of(&value), hash_of(&cloned));
            assert!(format!("{value:?}").starts_with("Not("));
            assert!(value.to_string().starts_with("(not "));
            drop(substituted);
            drop(cloned);
            drop(value);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("behavioral-predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_string_predicate_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut value = StrPred::Epsilon;
            for _ in 0..DEEP_INPUT_DEPTH {
                value = StrPred::Compl(Box::new(value));
            }
            let cloned = value.clone();
            assert_eq!(value, cloned);
            assert_eq!(hash_of(&value), hash_of(&cloned));
            assert!(format!("{value:?}").starts_with("Compl("));
            drop(cloned);
            drop(value);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("string-predicate lifecycle must not overflow the native stack");
}

#[test]
fn deep_json_type_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut value = JsonType::String;
            for _ in 0..DEEP_INPUT_DEPTH {
                value = JsonType::Array(Box::new(value));
            }
            let cloned = value.clone();
            assert_eq!(value, cloned);
            assert!(format!("{value:?}").starts_with("Array("));
            drop(cloned);
            drop(value);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("JSON-type lifecycle must not overflow the native stack");
}

#[test]
#[cfg(feature = "f1r3fly")]
fn deep_mettail_type_expression_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let mut value = TypeExpr::base("Leaf");
            for _ in 0..DEEP_INPUT_DEPTH {
                value = TypeExpr::Application(Box::new(value), Vec::new());
            }
            let cloned = value.clone();
            assert_eq!(value, cloned);
            assert!(format!("{value:?}").starts_with("Application("));
            drop(cloned);
            drop(value);
        })
        .expect("small-stack worker must spawn")
        .join()
        .expect("MeTTaIL type-expression lifecycle must not overflow the native stack");
}
