//! Cross-operation recursive shallow oracles and small-stack resource gates.

use std::cell::Cell;
use std::collections::BTreeMap;

use lling_llang::algorithms::BoundedDeterminization;
use lling_llang::composition::{compose, BoundedComposition, BoundedIntersection};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    ApproximationBound, CompleteResultCache, IncompleteReason, OperationCheckpoint,
    OperationLimits, OperationOutcome, OperationPlan, OperationUsage,
};
use lling_llang::wfst::{
    BoundedInputProjection, BoundedOutputProjection, CancellationToken, MutableWfst,
    SourceSnapshot, VectorWfst, Wfst, NO_STATE,
};

type Language = BTreeMap<(Vec<u32>, Vec<u32>), f64>;
type Graph = VectorWfst<u32, TropicalWeight>;

// Deliberately recursive and depth-limited: independent semantic oracle for
// tiny acyclic graphs, not an implementation used by production operations.
fn paths<G: Wfst<u32, TropicalWeight>>(graph: &G, max_depth: usize) -> Language {
    struct Walk<'a, G> {
        graph: &'a G,
        max_depth: usize,
        language: Language,
    }
    impl<G: Wfst<u32, TropicalWeight>> Walk<'_, G> {
        fn visit(
            &mut self,
            state: u32,
            depth: usize,
            input: &mut Vec<u32>,
            output: &mut Vec<u32>,
            cost: f64,
        ) {
            if self.graph.is_final(state) {
                let key = (input.clone(), output.clone());
                let accepted = cost + self.graph.final_weight(state).value();
                self.language
                    .entry(key)
                    .and_modify(|old| *old = old.min(accepted))
                    .or_insert(accepted);
            }
            if depth == self.max_depth {
                return;
            }
            for arc in self.graph.transitions(state) {
                if let Some(label) = arc.input {
                    input.push(label);
                }
                if let Some(label) = arc.output {
                    output.push(label);
                }
                self.visit(arc.to, depth + 1, input, output, cost + arc.weight.value());
                if arc.input.is_some() {
                    input.pop();
                }
                if arc.output.is_some() {
                    output.pop();
                }
            }
        }
    }
    let mut walk = Walk {
        graph,
        max_depth,
        language: Language::new(),
    };
    if graph.start() != NO_STATE {
        walk.visit(graph.start(), 0, &mut Vec::new(), &mut Vec::new(), 0.0);
    }
    walk.language
}

fn compose_oracle(first: &Language, second: &Language) -> Language {
    let mut answer = Language::new();
    for ((first_input, first_output), first_cost) in first {
        for ((second_input, second_output), second_cost) in second {
            if first_output == second_input {
                answer
                    .entry((first_input.clone(), second_output.clone()))
                    .and_modify(|old| *old = old.min(first_cost + second_cost))
                    .or_insert(first_cost + second_cost);
            }
        }
    }
    answer
}

fn project_oracle(source: &Language, input: bool) -> Language {
    let mut answer = Language::new();
    for ((source_input, source_output), cost) in source {
        let labels = if input { source_input } else { source_output };
        answer
            .entry((labels.clone(), labels.clone()))
            .and_modify(|old| *old = old.min(*cost))
            .or_insert(*cost);
    }
    answer
}

fn transducers() -> (Graph, Graph) {
    let mut first = Graph::new();
    first.add_states(4);
    first.set_start(0);
    first.set_final(2, TropicalWeight::new(5.0));
    first.set_final(3, TropicalWeight::new(7.0));
    first.add_arc(0, Some(2), Some(20), 2, TropicalWeight::new(2.0));
    first.add_arc(0, Some(1), Some(10), 1, TropicalWeight::new(1.0));
    first.add_arc(1, Some(3), Some(30), 3, TropicalWeight::new(3.0));
    let mut second = Graph::new();
    second.add_states(4);
    second.set_start(0);
    second.set_final(2, TropicalWeight::new(10.0));
    second.set_final(3, TropicalWeight::new(12.0));
    second.add_arc(0, Some(10), Some(100), 1, TropicalWeight::new(4.0));
    second.add_arc(0, Some(20), Some(200), 2, TropicalWeight::new(6.0));
    second.add_arc(1, Some(30), Some(300), 3, TropicalWeight::new(8.0));
    (first, second)
}

fn nondeterministic_acceptor() -> Graph {
    let mut graph = Graph::new();
    graph.add_states(4);
    graph.set_start(0);
    graph.set_final(3, TropicalWeight::new(5.0));
    graph.add_arc(0, Some(1), Some(1), 1, TropicalWeight::new(1.0));
    graph.add_arc(0, Some(1), Some(1), 2, TropicalWeight::new(2.0));
    graph.add_arc(0, Some(3), Some(3), 3, TropicalWeight::new(6.0));
    graph.add_arc(1, Some(2), Some(2), 3, TropicalWeight::new(3.0));
    graph.add_arc(2, Some(2), Some(2), 3, TropicalWeight::new(4.0));
    graph
}

fn acceptors() -> (Graph, Graph) {
    let mut first = Graph::new();
    first.add_states(3);
    first.set_start(0);
    first.set_final(1, TropicalWeight::new(3.0));
    first.set_final(2, TropicalWeight::new(4.0));
    first.add_arc(0, Some(2), Some(2), 2, TropicalWeight::new(2.0));
    first.add_arc(0, Some(1), Some(1), 1, TropicalWeight::new(1.0));
    let mut second = Graph::new();
    second.add_states(3);
    second.set_start(0);
    second.set_final(1, TropicalWeight::new(5.0));
    second.set_final(2, TropicalWeight::new(6.0));
    second.add_arc(0, Some(1), Some(1), 1, TropicalWeight::new(7.0));
    second.add_arc(0, Some(2), Some(2), 2, TropicalWeight::new(8.0));
    (first, second)
}

fn binding(graph: &Graph) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"qualification-graph/v1");
    hash.update(&(graph.num_states() as u64).to_be_bytes());
    hash.update(&graph.start().to_be_bytes());
    for state in 0..graph.num_states() as u32 {
        hash.update(&state.to_be_bytes());
        hash.update(&graph.final_weight(state).value().to_bits().to_be_bytes());
        hash.update(&(graph.transitions(state).len() as u64).to_be_bytes());
        for arc in graph.transitions(state) {
            for label in [arc.input, arc.output] {
                hash.update(&[u8::from(label.is_some())]);
                hash.update(&label.unwrap_or_default().to_be_bytes());
            }
            hash.update(&arc.to.to_be_bytes());
            hash.update(&arc.weight.value().to_bits().to_be_bytes());
        }
    }
    *hash.finalize().as_bytes()
}

fn pair_binding(first: &Graph, second: &Graph) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"qualification-ordered-pair/v1");
    hash.update(&binding(first));
    hash.update(&binding(second));
    *hash.finalize().as_bytes()
}

fn complete<T>(outcome: OperationOutcome<T>) -> (T, OperationCheckpoint) {
    let receipt = outcome.canonical_receipt_bytes();
    assert_eq!(receipt[8], 0, "only exact completion is accepted");
    assert_eq!(receipt, outcome.canonical_receipt_bytes());
    match outcome {
        OperationOutcome::Complete { value, checkpoint } => {
            assert_eq!(
                OperationCheckpoint::from_canonical_bytes(&checkpoint.canonical_bytes()).unwrap(),
                checkpoint
            );
            (value, checkpoint)
        }
        _ => panic!("qualification expected complete graph"),
    }
}

#[test]
fn independent_recursive_oracle_parity_for_all_four_operations() {
    let limits = OperationLimits::default();
    let (first, second) = transducers();
    let content = pair_binding(&first, &second);
    let mut composition = BoundedComposition::new(
        compose(first.clone(), second.clone()),
        content,
        limits,
        CancellationToken::new(),
    )
    .unwrap();
    let (composed, _) = complete(composition.run(content, |_| 0).unwrap());
    assert_eq!(
        paths(&composed, 4),
        compose_oracle(&paths(&first, 3), &paths(&second, 3))
    );

    let (first, second) = acceptors();
    let content = pair_binding(&first, &second);
    let mut intersection = BoundedIntersection::new(
        first.clone(),
        second.clone(),
        content,
        limits,
        CancellationToken::new(),
    )
    .unwrap();
    let (intersected, _) = complete(intersection.run(content, |_| 0).unwrap());
    assert_eq!(
        paths(&intersected, 3),
        compose_oracle(&paths(&first, 2), &paths(&second, 2))
    );

    let source = nondeterministic_acceptor();
    let content = binding(&source);
    let mut determinization =
        BoundedDeterminization::new(source.clone(), content, limits, CancellationToken::new())
            .unwrap();
    let (determinized, _) = complete(determinization.run(content, |_| 0, |_| 0).unwrap());
    assert_eq!(paths(&determinized, 3), paths(&source, 3));

    let (source, _) = transducers();
    let content = binding(&source);
    let mut input =
        BoundedInputProjection::new(source.clone(), content, limits, CancellationToken::new())
            .unwrap();
    let mut output =
        BoundedOutputProjection::new(source.clone(), content, limits, CancellationToken::new())
            .unwrap();
    let (input_graph, _) = complete(input.run(content, |_| 0).unwrap());
    let (output_graph, _) = complete(output.run(content, |_| 0).unwrap());
    assert_eq!(
        paths(&input_graph, 3),
        project_oracle(&paths(&source, 3), true)
    );
    assert_eq!(
        paths(&output_graph, 3),
        project_oracle(&paths(&source, 3), false)
    );
}

fn chain(arcs: u32) -> Graph {
    let mut graph = Graph::new();
    graph.add_states(arcs as usize + 1);
    graph.set_start(0);
    graph.set_final(arcs, TropicalWeight::one());
    for state in 0..arcs {
        graph.add_arc(
            state,
            Some(state + 1),
            Some(state + 1),
            state + 1,
            TropicalWeight::one(),
        );
    }
    graph
}

fn fanout(arcs: u32) -> Graph {
    let mut graph = Graph::new();
    graph.add_states(arcs as usize + 1);
    graph.set_start(0);
    for label in 1..=arcs {
        graph.set_final(label, TropicalWeight::one());
        graph.add_arc(0, Some(label), Some(label), label, TropicalWeight::one());
    }
    graph
}

struct StackProbe {
    low: Cell<usize>,
    high: Cell<usize>,
}

impl StackProbe {
    fn new() -> Self {
        Self {
            low: Cell::new(usize::MAX),
            high: Cell::new(0),
        }
    }
    fn sample(&self) -> u64 {
        let marker = 0_u8;
        let address = std::hint::black_box(&marker) as *const u8 as usize;
        self.low.set(self.low.get().min(address));
        self.high.set(self.high.get().max(address));
        0
    }
    fn span(&self) -> usize {
        self.high.get().saturating_sub(self.low.get())
    }
}

fn run_four(graph: &Graph, probe: &StackProbe) -> [OperationUsage; 4] {
    let content = binding(graph);
    let pair_content = pair_binding(graph, graph);
    let limits = OperationLimits {
        max_states: graph.num_states() as u64 + 1,
        max_arcs: graph.total_transitions() as u64 + 1,
        max_work: 1_000_000,
        max_heap_bytes: 10_000_000,
        max_elapsed_ns: 60_000_000_000,
    };
    let mut composition = BoundedComposition::new(
        compose(graph.clone(), graph.clone()),
        pair_content,
        limits,
        CancellationToken::new(),
    )
    .unwrap();
    let (composed, composition_cp) =
        complete(composition.run(pair_content, |_| probe.sample()).unwrap());
    let mut intersection = BoundedIntersection::new(
        graph.clone(),
        graph.clone(),
        pair_content,
        limits,
        CancellationToken::new(),
    )
    .unwrap();
    let (intersected, intersection_cp) =
        complete(intersection.run(pair_content, |_| probe.sample()).unwrap());
    let mut determinization =
        BoundedDeterminization::new(graph.clone(), content, limits, CancellationToken::new())
            .unwrap();
    let (determinized, determinization_cp) = complete(
        determinization
            .run(content, |_| probe.sample(), |_| probe.sample())
            .unwrap(),
    );
    let mut projection =
        BoundedInputProjection::new(graph.clone(), content, limits, CancellationToken::new())
            .unwrap();
    let (projected, projection_cp) = complete(projection.run(content, |_| probe.sample()).unwrap());
    for result in [&composed, &intersected, &determinized, &projected] {
        assert_eq!(result.num_states(), graph.num_states());
        assert_eq!(result.total_transitions(), graph.total_transitions());
    }
    [
        composition_cp.usage,
        determinization_cp.usage,
        intersection_cp.usage,
        projection_cp.usage,
    ]
}

#[test]
fn deep_wide_small_stack_and_linear_logical_resource_slopes() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let probe = StackProbe::new();
            let small = run_four(&chain(64), &probe);
            let small_stack_span = probe.span();
            let medium = run_four(&chain(512), &probe);
            assert!(probe.span() <= small_stack_span + 8 * 1024);
            for (before, after) in small.iter().zip(medium.iter()) {
                assert_eq!(before.states, 65);
                assert_eq!(after.states, 513);
                assert_eq!(before.arcs, 64);
                assert_eq!(after.arcs, 512);
                assert!(after.work >= before.work * 7 && after.work <= before.work * 9);
                assert!(
                    after.heap_bytes >= before.heap_bytes * 7
                        && after.heap_bytes <= before.heap_bytes * 9
                );
            }
            let deep = run_four(&chain(4096), &probe);
            let wide = run_four(&fanout(2048), &probe);
            for usage in deep {
                assert_eq!(usage.states, 4097);
                assert_eq!(usage.arcs, 4096);
            }
            for usage in wide {
                assert_eq!(usage.states, 2049);
                assert_eq!(usage.arcs, 2048);
            }
            // Sampled callback addresses remain in a small band despite a 64x
            // increase in graph depth; the actual gate is the 128 KiB thread.
            assert!(probe.span() <= small_stack_span + 8 * 1024);
            assert!(
                probe.span() < 48 * 1024,
                "stack probe span {}",
                probe.span()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn incomplete_and_approximate_receipts_cannot_become_complete_cache_entries() {
    let source = chain(2);
    let content = binding(&source);
    let pair_content = pair_binding(&source, &source);
    let limits = OperationLimits {
        max_states: 0,
        ..OperationLimits::default()
    };
    let mut composition = BoundedComposition::new(
        compose(source.clone(), source.clone()),
        pair_content,
        limits,
        CancellationToken::new(),
    )
    .unwrap();
    let mut intersection = BoundedIntersection::new(
        source.clone(),
        source.clone(),
        pair_content,
        limits,
        CancellationToken::new(),
    )
    .unwrap();
    let mut determinization =
        BoundedDeterminization::new(source.clone(), content, limits, CancellationToken::new())
            .unwrap();
    let mut projection =
        BoundedOutputProjection::new(source, content, limits, CancellationToken::new()).unwrap();
    let outcomes = [
        composition.run(pair_content, |_| 0).unwrap(),
        intersection.run(pair_content, |_| 0).unwrap(),
        determinization.run(content, |_| 0, |_| 0).unwrap(),
        projection.run(content, |_| 0).unwrap(),
    ];
    for outcome in outcomes {
        let receipt = outcome.canonical_receipt_bytes();
        assert_eq!(receipt[8], 2, "incomplete quality tag");
        assert_eq!(receipt, outcome.canonical_receipt_bytes());
        match outcome {
            OperationOutcome::Incomplete {
                reason, checkpoint, ..
            } => {
                assert_eq!(reason, IncompleteReason::StateLimit);
                assert_eq!(checkpoint.next_index, 0);
                assert_eq!(
                    OperationCheckpoint::from_canonical_bytes(&checkpoint.canonical_bytes())
                        .unwrap(),
                    checkpoint
                );
            }
            _ => panic!("zero-state cap cannot be complete or approximate"),
        }
    }

    // A bound identity cannot elevate approximation to exact completion,
    // even when its cursor appears terminal.
    let plan = OperationPlan::new(
        SourceSnapshot::IMMUTABLE,
        [79; 32],
        "qualification-approx/v1",
        vec![],
    )
    .unwrap();
    let checkpoint = OperationCheckpoint {
        identity: plan.identity,
        next_index: 0,
        usage: OperationUsage::default(),
        elapsed_ns: 0,
    };
    let approximate = OperationOutcome::Approximate {
        value: (),
        bound: ApproximationBound {
            max_error_microunits: 1,
            method_digest: [17; 32],
        },
        checkpoint,
    };
    assert_eq!(approximate.canonical_receipt_bytes()[8], 1);
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, approximate).is_err());
    assert!(cache.is_empty());
}
