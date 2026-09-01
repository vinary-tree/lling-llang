//! `SymbolicTreeTransducer<A, B>` — a bottom-up symbolic tree transducer,
//! generalizing the word transducer (`sft.rs`) to ranked terms.
//!
//! It reads an input [`SymTerm<A::Domain>`] bottom-up; each transition
//! `(constructor, payload_guard, child_states) → (target, output)` fires when the
//! input node's constructor/children/payload match, and an [`OutputBuilder`]
//! constructs the output node from the input payload and the already-transduced
//! children. The result is the set of output terms producible at an accepting
//! root state.
//!
//! Operations provided here (all exact): [`transduce`](SymbolicTreeTransducer::transduce),
//! [`domain_sta`](SymbolicTreeTransducer::domain_sta) (the underlying input tree
//! automaton), [`is_total`](SymbolicTreeTransducer::is_total) (every input has an
//! output — decided by complementing the domain), and
//! [`compose_transduce`] (exact sequential composition of two transductions).
//! The composition/functionality *algebraic laws* are established at the FV
//! layer (M4: `StftComposition.v` / `StftFunctionality.v`), which model the
//! abstract bottom-up transduction `f : SymTerm A → list (SymTerm B)`.

use std::collections::HashMap;
use std::sync::Arc;

use super::sym_tree::{SymTerm, SymbolicTreeAutomaton, TreeTrans};
use super::BooleanAlgebra;

// ══════════════════════════════════════════════════════════════════════════════
// Output builders
// ══════════════════════════════════════════════════════════════════════════════

/// How a transition produces the output node's payload.
#[derive(Clone)]
pub enum PayloadOut<A: BooleanAlgebra, B: BooleanAlgebra> {
    /// Structural output node — no payload.
    Structural,
    /// A fixed output payload.
    Const(B::Domain),
    /// The output payload computed from the input node's payload.
    Map(Arc<dyn Fn(&A::Domain) -> B::Domain + Send + Sync>),
}

/// How a transition builds the output term from the transduced children.
#[derive(Clone)]
pub enum OutputBuilder<A: BooleanAlgebra, B: BooleanAlgebra> {
    /// Emit `constructor` with `payload` and the transduced children selected
    /// (and reordered) by `children` (indices into the input node's children).
    Build {
        constructor: String,
        payload: PayloadOut<A, B>,
        children: Vec<usize>,
    },
    /// Emit the `i`-th transduced child directly (delete this node).
    Project(usize),
}

/// A bottom-up transition with an attached output builder.
#[derive(Clone)]
pub struct TransducerRule<A: BooleanAlgebra, B: BooleanAlgebra> {
    /// Input head constructor.
    pub constructor: String,
    /// Input payload guard (`None` for structural input nodes).
    pub payload_guard: Option<A::Predicate>,
    /// Required state for each input child.
    pub child_states: Vec<usize>,
    /// Resulting state.
    pub target: usize,
    /// How to build the output.
    pub output: OutputBuilder<A, B>,
}

// ══════════════════════════════════════════════════════════════════════════════
// SymbolicTreeTransducer
// ══════════════════════════════════════════════════════════════════════════════

/// A bottom-up symbolic tree transducer from terms over `A` to terms over `B`.
#[derive(Clone)]
pub struct SymbolicTreeTransducer<A: BooleanAlgebra, B: BooleanAlgebra> {
    /// Input element algebra.
    pub input_algebra: A,
    /// Output element algebra.
    pub output_algebra: B,
    /// Number of states.
    pub num_states: usize,
    /// Transition rules.
    pub rules: Vec<TransducerRule<A, B>>,
    /// Accepting (root) states.
    pub accepting: std::collections::HashSet<usize>,
    /// Ranked input alphabet: constructor → arity.
    pub arities: HashMap<String, usize>,
}

impl<A: BooleanAlgebra, B: BooleanAlgebra> SymbolicTreeTransducer<A, B> {
    /// An empty transducer.
    pub fn new(input_algebra: A, output_algebra: B) -> Self {
        SymbolicTreeTransducer {
            input_algebra,
            output_algebra,
            num_states: 0,
            rules: Vec::new(),
            accepting: std::collections::HashSet::new(),
            arities: HashMap::new(),
        }
    }

    /// Add a state, returning its id.
    pub fn add_state(&mut self) -> usize {
        let id = self.num_states;
        self.num_states += 1;
        id
    }

    /// Mark a state accepting.
    pub fn set_accepting(&mut self, state: usize) {
        self.accepting.insert(state);
    }

    /// Register a constructor's arity.
    pub fn register(&mut self, constructor: impl Into<String>, arity: usize) {
        self.arities.insert(constructor.into(), arity);
    }

    /// Add a transition rule.
    pub fn add_rule(&mut self, rule: TransducerRule<A, B>) {
        self.rules.push(rule);
    }

    fn payload_matches(&self, guard: &Option<A::Predicate>, payload: &Option<A::Domain>) -> bool {
        match (guard, payload) {
            (None, None) => true,
            (Some(g), Some(v)) => self.input_algebra.evaluate(g, v),
            _ => false,
        }
    }

    fn build_output(
        &self,
        builder: &OutputBuilder<A, B>,
        input_payload: &Option<A::Domain>,
        child_outputs: &[SymTerm<B::Domain>],
    ) -> Option<SymTerm<B::Domain>> {
        match builder {
            OutputBuilder::Project(i) => child_outputs.get(*i).cloned(),
            OutputBuilder::Build {
                constructor,
                payload,
                children,
            } => {
                let pl = match payload {
                    PayloadOut::Structural => None,
                    PayloadOut::Const(d) => Some(d.clone()),
                    PayloadOut::Map(f) => Some(f(input_payload.as_ref()?)),
                };
                let kids: Option<Vec<SymTerm<B::Domain>>> = children
                    .iter()
                    .map(|&i| child_outputs.get(i).cloned())
                    .collect();
                Some(SymTerm {
                    constructor: constructor.clone(),
                    payload: pl,
                    children: kids?,
                })
            }
        }
    }

    /// Assemble a deterministic singleton result by consuming owned children.
    ///
    /// Each uniquely selected child is moved into the result. Repeated selector
    /// indices clone only the non-final occurrences because the public output is
    /// an owned tree and therefore cannot represent sharing.
    fn build_output_owned(
        &self,
        builder: &OutputBuilder<A, B>,
        input_payload: &Option<A::Domain>,
        child_outputs: Vec<SymTerm<B::Domain>>,
    ) -> Option<SymTerm<B::Domain>> {
        match builder {
            OutputBuilder::Project(index) => child_outputs.into_iter().nth(*index),
            OutputBuilder::Build {
                constructor,
                payload,
                children,
            } => {
                let payload = match payload {
                    PayloadOut::Structural => None,
                    PayloadOut::Const(value) => Some(value.clone()),
                    PayloadOut::Map(function) => Some(function(input_payload.as_ref()?)),
                };
                let mut remaining = vec![0usize; child_outputs.len()];
                for &index in children {
                    *remaining.get_mut(index)? += 1;
                }
                let mut slots = child_outputs.into_iter().map(Some).collect::<Vec<_>>();
                let mut selected = Vec::with_capacity(children.len());
                for &index in children {
                    remaining[index] -= 1;
                    if remaining[index] == 0 {
                        selected.push(slots[index].take().expect("the selected child is present"));
                    } else {
                        selected.push(slots[index].as_ref()?.clone());
                    }
                }
                Some(SymTerm {
                    constructor: constructor.clone(),
                    payload,
                    children: selected,
                })
            }
        }
    }

    fn rule_matches_completed_node(
        &self,
        rule: &TransducerRule<A, B>,
        node: &SymTerm<A::Domain>,
        child_maps: &[HashMap<usize, Vec<SymTerm<B::Domain>>>],
    ) -> bool {
        rule.constructor == node.constructor
            && rule.child_states.len() == node.children.len()
            && self.payload_matches(&rule.payload_guard, &node.payload)
            && rule
                .child_states
                .iter()
                .enumerate()
                .all(|(index, state)| child_maps[index].contains_key(state))
    }

    fn deterministic_singleton_output(
        &self,
        rule: &TransducerRule<A, B>,
        input_payload: &Option<A::Domain>,
        child_maps: &mut [HashMap<usize, Vec<SymTerm<B::Domain>>>],
    ) -> Option<SymTerm<B::Domain>> {
        if !rule
            .child_states
            .iter()
            .enumerate()
            .all(|(index, state)| child_maps[index][state].len() == 1)
        {
            return None;
        }
        let child_outputs = rule
            .child_states
            .iter()
            .enumerate()
            .map(|(index, state)| {
                child_maps[index]
                    .remove(state)
                    .expect("the applicable rule's child state is present")
                    .pop()
                    .expect("the deterministic child output was length-checked")
            })
            .collect();
        self.build_output_owned(&rule.output, input_payload, child_outputs)
    }

    /// Bottom-up: state → output terms producible at this node in that state.
    fn run_outputs(&self, node: &SymTerm<A::Domain>) -> HashMap<usize, Vec<SymTerm<B::Domain>>> {
        struct Frame<'a, Input, Output> {
            node: &'a SymTerm<Input>,
            next_child: usize,
            child_maps: Vec<HashMap<usize, Vec<SymTerm<Output>>>>,
        }

        let mut frames = vec![Frame {
            node,
            next_child: 0,
            child_maps: Vec::with_capacity(node.children.len()),
        }];
        loop {
            let frame = frames
                .last_mut()
                .expect("the root transduction frame remains until completion");
            if let Some(child) = frame.node.children.get(frame.next_child) {
                frame.next_child += 1;
                frames.push(Frame {
                    node: child,
                    next_child: 0,
                    child_maps: Vec::with_capacity(child.children.len()),
                });
                continue;
            }

            let mut completed = frames
                .pop()
                .expect("a completed transduction frame is present");
            let mut unique_rule = None;
            let mut ambiguous = false;
            for rule in &self.rules {
                if self.rule_matches_completed_node(rule, completed.node, &completed.child_maps) {
                    if unique_rule.replace(rule).is_some() {
                        ambiguous = true;
                        break;
                    }
                }
            }

            let deterministic = if ambiguous {
                None
            } else {
                unique_rule.and_then(|rule| {
                    let singleton = rule
                        .child_states
                        .iter()
                        .enumerate()
                        .all(|(index, state)| completed.child_maps[index][state].len() == 1);
                    singleton.then_some(rule)
                })
            };

            let mut result: HashMap<usize, Vec<SymTerm<B::Domain>>> = HashMap::new();
            if let Some(rule) = deterministic {
                if let Some(output) = self.deterministic_singleton_output(
                    rule,
                    &completed.node.payload,
                    &mut completed.child_maps,
                ) {
                    result.entry(rule.target).or_default().push(output);
                }
            } else {
                for rule in &self.rules {
                    if !self.rule_matches_completed_node(
                        rule,
                        completed.node,
                        &completed.child_maps,
                    ) {
                        continue;
                    }
                    let per_child = rule
                        .child_states
                        .iter()
                        .enumerate()
                        .map(|(index, &state)| {
                            completed.child_maps[index]
                                .get(&state)
                                .expect("the matching rule's child state is present")
                        })
                        .collect::<Vec<_>>();
                    for combo in cartesian_terms(&per_child) {
                        if let Some(output) =
                            self.build_output(&rule.output, &completed.node.payload, &combo)
                        {
                            result.entry(rule.target).or_default().push(output);
                        }
                    }
                }
            }
            if let Some(parent) = frames.last_mut() {
                parent.child_maps.push(result);
            } else {
                return result;
            }
        }
    }

    /// The set of output terms produced for `input`.
    pub fn transduce(&self, input: &SymTerm<A::Domain>) -> Vec<SymTerm<B::Domain>> {
        let outs = self.run_outputs(input);
        let mut result = Vec::new();
        for (state, terms) in outs {
            if self.accepting.contains(&state) {
                result.extend(terms);
            }
        }
        result
    }

    /// The underlying input tree automaton (outputs dropped): accepts exactly the
    /// terms in this transducer's domain.
    pub fn domain_sta(&self) -> SymbolicTreeAutomaton<A> {
        let mut sta = SymbolicTreeAutomaton::new(self.input_algebra.clone());
        sta.num_states = self.num_states;
        sta.arities = self.arities.clone();
        sta.accepting = self.accepting.clone();
        for rule in &self.rules {
            sta.add_transition(TreeTrans {
                constructor: rule.constructor.clone(),
                payload_guard: rule.payload_guard.clone(),
                child_states: rule.child_states.clone(),
                target: rule.target,
            });
        }
        sta
    }

    /// Whether every well-formed input term (over the registered alphabet) has at
    /// least one output — i.e. the domain accepts all terms.
    pub fn is_total(&self) -> bool {
        self.domain_sta().complement().is_empty()
    }
}

/// All ways to pick one output per child (cartesian product).
fn cartesian_terms<D: Clone>(per_child: &[&Vec<SymTerm<D>>]) -> Vec<Vec<SymTerm<D>>> {
    let mut out = vec![Vec::new()];
    for child in per_child {
        let mut next = Vec::new();
        for prefix in &out {
            for term in child.iter() {
                let mut t = prefix.clone();
                t.push(term.clone());
                next.push(t);
            }
        }
        out = next;
    }
    out
}

/// Exact sequential composition of two transductions: `(t1 ; t2)(input)` is the
/// set of final terms obtained by transducing `input` with `t1` then each
/// intermediate with `t2`.
pub fn compose_transduce<A, B, C>(
    t1: &SymbolicTreeTransducer<A, B>,
    t2: &SymbolicTreeTransducer<B, C>,
    input: &SymTerm<A::Domain>,
) -> Vec<SymTerm<C::Domain>>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    C: BooleanAlgebra,
{
    t1.transduce(input)
        .iter()
        .flat_map(|mid| t2.transduce(mid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{IntervalAlgebra, IntervalPred};
    use super::*;
    use proptest::prelude::*;

    const DEEP_TRANSDUCTION_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    fn lit(n: i64) -> SymTerm<i64> {
        SymTerm::leaf("Lit", n)
    }
    fn pair(a: SymTerm<i64>, b: SymTerm<i64>) -> SymTerm<i64> {
        SymTerm::node("Pair", vec![a, b])
    }

    fn unary_term(depth: usize) -> SymTerm<i64> {
        let mut term = SymTerm::constant("L");
        for _ in 0..depth {
            term = SymTerm::node("U", vec![term]);
        }
        term
    }

    fn unary_depth(term: &SymTerm<i64>) -> Option<usize> {
        let mut depth = 0usize;
        let mut current = term;
        loop {
            match (current.constructor.as_str(), current.children.as_slice()) {
                ("L", []) => return Some(depth),
                ("U", [child]) => {
                    depth += 1;
                    current = child;
                }
                _ => return None,
            }
        }
    }

    fn unary_identity() -> SymbolicTreeTransducer<IntervalAlgebra, IntervalAlgebra> {
        let mut transducer =
            SymbolicTreeTransducer::new(IntervalAlgebra::new(0, 1), IntervalAlgebra::new(0, 1));
        transducer.register("L", 0);
        transducer.register("U", 1);
        let state = transducer.add_state();
        transducer.set_accepting(state);
        transducer.add_rule(TransducerRule {
            constructor: "L".to_owned(),
            payload_guard: None,
            child_states: Vec::new(),
            target: state,
            output: OutputBuilder::Build {
                constructor: "L".to_owned(),
                payload: PayloadOut::Structural,
                children: Vec::new(),
            },
        });
        transducer.add_rule(TransducerRule {
            constructor: "U".to_owned(),
            payload_guard: None,
            child_states: vec![state],
            target: state,
            output: OutputBuilder::Build {
                constructor: "U".to_owned(),
                payload: PayloadOut::Structural,
                children: vec![0],
            },
        });
        transducer
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn unary_transduction_preserves_exact_depth(depth in 0usize..128) {
            let input = unary_term(depth);
            let output = unary_identity().transduce(&input);
            prop_assert_eq!(output.len(), 1);
            prop_assert_eq!(unary_depth(&output[0]), Some(depth));
        }

        #[test]
        fn owned_output_assembly_refines_clone_based_selection(
            (child_count, selectors, project_index) in (1usize..8).prop_flat_map(|child_count| {
                (
                    Just(child_count),
                    prop::collection::vec(0usize..child_count, 0..16),
                    0usize..child_count,
                )
            }),
        ) {
            let transducer = unary_identity();
            let children = (0..child_count)
                .map(|index| SymTerm::constant(format!("C{index}")))
                .collect::<Vec<SymTerm<i64>>>();
            let build = OutputBuilder::Build {
                constructor: "Out".to_owned(),
                payload: PayloadOut::Structural,
                children: selectors,
            };
            prop_assert_eq!(
                transducer.build_output(&build, &None, &children),
                transducer.build_output_owned(&build, &None, children.clone()),
            );

            let project = OutputBuilder::Project(project_index);
            prop_assert_eq!(
                transducer.build_output(&project, &None, &children),
                transducer.build_output_owned(&project, &None, children),
            );
        }
    }

    /// A transducer that doubles every Lit payload and rebuilds Pairs.
    fn doubler() -> SymbolicTreeTransducer<IntervalAlgebra, IntervalAlgebra> {
        let mut t = SymbolicTreeTransducer::new(
            IntervalAlgebra::new(0, 1000),
            IntervalAlgebra::new(0, 1000),
        );
        t.register("Lit", 0);
        t.register("Pair", 2);
        // A single recursive "term" state so the transducer handles arbitrary
        // nesting (and is total over all Lit/Pair terms).
        let q = t.add_state();
        t.set_accepting(q);
        t.add_rule(TransducerRule {
            constructor: "Lit".to_string(),
            payload_guard: Some(IntervalPred::True),
            child_states: vec![],
            target: q,
            output: OutputBuilder::Build {
                constructor: "Lit".to_string(),
                payload: PayloadOut::Map(Arc::new(|x: &i64| x * 2)),
                children: vec![],
            },
        });
        t.add_rule(TransducerRule {
            constructor: "Pair".to_string(),
            payload_guard: None,
            child_states: vec![q, q],
            target: q,
            output: OutputBuilder::Build {
                constructor: "Pair".to_string(),
                payload: PayloadOut::Structural,
                children: vec![0, 1],
            },
        });
        t
    }

    #[test]
    fn transduce_doubles_payloads() {
        let t = doubler();
        let out = t.transduce(&pair(lit(3), lit(4)));
        assert_eq!(out, vec![pair(lit(6), lit(8))]);
        let out_lit = t.transduce(&lit(5));
        assert_eq!(out_lit, vec![lit(10)]);
    }

    #[test]
    fn project_deletes_node() {
        // A transducer that projects Pair(a, b) to its first child.
        let mut t = SymbolicTreeTransducer::new(
            IntervalAlgebra::new(0, 1000),
            IntervalAlgebra::new(0, 1000),
        );
        t.register("Lit", 0);
        t.register("Pair", 2);
        let q = t.add_state();
        t.set_accepting(q);
        t.add_rule(TransducerRule {
            constructor: "Lit".to_string(),
            payload_guard: Some(IntervalPred::True),
            child_states: vec![],
            target: q,
            output: OutputBuilder::Build {
                constructor: "Lit".to_string(),
                payload: PayloadOut::Map(Arc::new(|x: &i64| *x)),
                children: vec![],
            },
        });
        t.add_rule(TransducerRule {
            constructor: "Pair".to_string(),
            payload_guard: None,
            child_states: vec![q, q],
            target: q,
            output: OutputBuilder::Project(0),
        });
        assert_eq!(t.transduce(&pair(lit(7), lit(9))), vec![lit(7)]);
    }

    #[test]
    fn domain_and_totality() {
        let t = doubler();
        let dom = t.domain_sta();
        assert!(dom.accepts(&pair(lit(1), lit(2))));
        assert!(dom.accepts(&lit(5)));
        // The doubler accepts every well-formed Lit/Pair term → total.
        assert!(t.is_total());
    }

    #[test]
    fn not_total_when_guard_restricts() {
        // Only transduces Lits in [0,10); larger Lits have no output.
        let mut t = SymbolicTreeTransducer::new(
            IntervalAlgebra::new(0, 1000),
            IntervalAlgebra::new(0, 1000),
        );
        t.register("Lit", 0);
        let q = t.add_state();
        t.set_accepting(q);
        t.add_rule(TransducerRule {
            constructor: "Lit".to_string(),
            payload_guard: Some(IntervalPred::Range(0, 10)),
            child_states: vec![],
            target: q,
            output: OutputBuilder::Build {
                constructor: "Lit".to_string(),
                payload: PayloadOut::Map(Arc::new(|x: &i64| *x)),
                children: vec![],
            },
        });
        assert!(t.transduce(&lit(5)).len() == 1);
        assert!(t.transduce(&lit(50)).is_empty());
        assert!(!t.is_total()); // Lit[10,1000) has no output
    }

    #[test]
    fn composition_sequences_transductions() {
        let t = doubler();
        // double then double = quadruple
        let out = compose_transduce(&t, &t, &pair(lit(3), lit(4)));
        assert_eq!(out, vec![pair(lit(12), lit(16))]);
    }

    #[test]
    fn deep_symbolic_tree_transduction_uses_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-symbolic-tree-transduction".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let transducer = unary_identity();
                let input = unary_term(DEEP_TRANSDUCTION_DEPTH);
                let output = transducer.transduce(&input);
                assert_eq!(output.len(), 1);
                assert_eq!(unary_depth(&output[0]), Some(DEEP_TRANSDUCTION_DEPTH));
                drop(output);
                drop(input);
                drop(transducer);
            })
            .expect("the bounded-stack symbolic-transduction worker must spawn")
            .join()
            .expect("symbolic tree transduction must not overflow the native stack");
    }
}
