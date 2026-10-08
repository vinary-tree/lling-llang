//! Independent S6 path oracles, small-stack gates, and quality controls.

use std::cell::Cell;

use lling_llang::algorithms::{BoundedShortestWitness, BoundedTopK, TopKLimits};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationLimits, OperationOutcome,
    OperationPlan, OperationUsage,
};
use lling_llang::wfst::{
    CancellationReason, CancellationToken, MutableWfst, SourceSnapshot, VectorWfst, Wfst,
};

type Graph = VectorWfst<u32, TropicalWeight>;
type ArcLocation = (u32, usize, u32);
type OraclePath = (f64, Vec<ArcLocation>);

fn binding(graph: &Graph) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"s6-qualification-source/v1");
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

// Independent recursive semantics oracle, only invoked on tiny acyclic DAGs.
fn enumerate(graph: &Graph) -> Vec<OraclePath> {
    fn walk(
        graph: &Graph,
        state: u32,
        cost: f64,
        path: &mut Vec<ArcLocation>,
        out: &mut Vec<OraclePath>,
    ) {
        if graph.is_final(state) {
            out.push((cost + graph.final_weight(state).value(), path.clone()));
        }
        for (index, arc) in graph.transitions(state).iter().enumerate() {
            path.push((state, index, arc.to));
            walk(graph, arc.to, cost + arc.weight.value(), path, out);
            path.pop();
        }
    }
    let mut paths = Vec::new();
    walk(graph, graph.start(), 0.0, &mut Vec::new(), &mut paths);
    paths
}

fn shallow(seed: u32) -> Graph {
    let mut graph = Graph::new();
    graph.add_states(6);
    graph.set_start(0);
    graph.set_final(4, TropicalWeight::new(f64::from(seed % 3)));
    graph.set_final(5, TropicalWeight::new(f64::from((seed + 1) % 3)));
    for (from, to, offset) in [
        (0, 1, 0),
        (0, 2, 1),
        (0, 3, 2),
        (1, 3, 3),
        (1, 4, 4),
        (2, 3, 5),
        (2, 4, 6),
        (3, 5, 7),
        (4, 5, 8),
    ] {
        let cost = f64::from(1 + (seed + offset) % 5);
        let input = if offset == 3 { None } else { Some(offset + 1) };
        graph.add_arc(
            from,
            input,
            Some(offset + 11),
            to,
            TropicalWeight::new(cost),
        );
    }
    graph
}

fn identity(path: &lling_llang::algorithms::ShortestWitness<u32>) -> (u64, Vec<ArcLocation>) {
    (
        path.total_weight.value().to_bits(),
        path.steps
            .iter()
            .map(|s| (s.from, s.arc_index, s.to))
            .collect(),
    )
}

fn assert_provenance(graph: &Graph, path: &lling_llang::algorithms::ShortestWitness<u32>) {
    let mut state = graph.start();
    let mut cost = 0.0;
    for step in &path.steps {
        assert_eq!(step.from, state);
        let arc = &graph.transitions(state)[step.arc_index];
        assert_eq!(step.to, arc.to);
        assert_eq!(step.input, arc.input);
        assert_eq!(step.output, arc.output);
        assert_eq!(step.weight, arc.weight);
        cost += arc.weight.value();
        state = arc.to;
    }
    assert_eq!(state, path.final_state);
    assert!(graph.is_final(state));
    assert_eq!(path.final_weight, graph.final_weight(state));
    assert_eq!(path.total_weight.value(), cost + path.final_weight.value());
}

#[test]
fn recursive_oracle_parity_on_several_independent_shallow_graphs() {
    for seed in 0..12 {
        let graph = shallow(seed);
        let content = binding(&graph);
        let oracle = enumerate(&graph);
        let mut shortest = BoundedShortestWitness::new(
            graph.clone(),
            content,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
        let best = shortest
            .run(content, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .unwrap();
        assert_provenance(&graph, &best);
        let oracle_min = oracle
            .iter()
            .map(|(cost, _)| *cost)
            .fold(f64::INFINITY, f64::min);
        assert_eq!(best.total_weight.value(), oracle_min);
        assert!(oracle
            .iter()
            .any(|path| path.0 == oracle_min
                && identity(&best) == (path.0.to_bits(), path.1.clone())));

        let mut topk = BoundedTopK::new(
            graph.clone(),
            content,
            OperationLimits::default(),
            TopKLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
        let paths = topk.run(content, |_| 0).unwrap().into_complete().unwrap();
        assert_eq!(paths.len(), oracle.len());
        for path in &paths {
            assert_provenance(&graph, path);
        }
        assert!(paths
            .windows(2)
            .all(|pair| pair[0].total_weight <= pair[1].total_weight));
        let mut actual = paths.iter().map(identity).collect::<Vec<_>>();
        let mut expected = oracle
            .into_iter()
            .map(|(cost, path)| (cost.to_bits(), path))
            .collect::<Vec<_>>();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }
}

#[test]
fn multi_sibling_equal_weight_ties_are_stable_and_source_ordered() {
    let mut graph = Graph::new();
    graph.add_states(5);
    graph.set_start(0);
    for state in 1..=4 {
        graph.set_final(state, TropicalWeight::new(2.0));
    }
    for state in 1..=3 {
        graph.add_arc(
            0,
            Some(state),
            Some(state + 10),
            state,
            TropicalWeight::new(1.0),
        );
    }
    graph.add_arc(1, Some(4), Some(14), 4, TropicalWeight::one());
    let content = binding(&graph);
    let mut shortest = BoundedShortestWitness::new(
        graph.clone(),
        content,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let best = shortest
        .run(content, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .unwrap();
    assert_eq!(best.steps[0].arc_index, 0);
    let mut topk = BoundedTopK::new(
        graph,
        content,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let paths = topk.run(content, |_| 0).unwrap().into_complete().unwrap();
    assert_eq!(
        paths
            .iter()
            .map(|path| path
                .steps
                .iter()
                .map(|step| step.arc_index)
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        vec![vec![0], vec![1], vec![2], vec![0, 0]]
    );
    assert_eq!(
        paths
            .iter()
            .map(|path| path.total_weight.value())
            .collect::<Vec<_>>(),
        vec![3.0, 3.0, 3.0, 3.0]
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

fn run_both(graph: &Graph, probe: &StackProbe) -> [OperationUsage; 2] {
    let content = binding(graph);
    let limits = OperationLimits {
        max_states: graph.num_states() as u64 + 1,
        max_arcs: graph.total_transitions() as u64 + 1,
        max_work: 1_000_000,
        max_heap_bytes: 10_000_000,
        max_elapsed_ns: 60_000_000_000,
    };
    let mut shortest =
        BoundedShortestWitness::new(graph.clone(), content, limits, CancellationToken::new())
            .unwrap();
    let (best, first) = match shortest.run(content, |_| probe.sample()).unwrap() {
        OperationOutcome::Complete { value, checkpoint } => (value, checkpoint),
        _ => panic!("shortest unexpectedly incomplete"),
    };
    assert!(best.is_some());
    let mut topk = BoundedTopK::new(
        graph.clone(),
        content,
        limits,
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let (paths, second) = match topk.run(content, |_| probe.sample()).unwrap() {
        OperationOutcome::Complete { value, checkpoint } => (value, checkpoint),
        _ => panic!("top-k unexpectedly incomplete"),
    };
    assert_eq!(paths.len(), graph.transitions(0).len().max(1));
    [first.usage, second.usage]
}

#[test]
fn deep_wide_128k_stack_and_linear_charged_resource_slopes() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let probe = StackProbe::new();
            let small = run_both(&chain(64), &probe);
            let initial_span = probe.span();
            let medium = run_both(&chain(512), &probe);
            for (a, b) in small.iter().zip(medium.iter()) {
                assert_eq!((a.states, a.arcs), (65, 64));
                assert_eq!((b.states, b.arcs), (513, 512));
                assert!(
                    b.work >= a.work * 7 && b.work <= a.work * 9,
                    "work slope: {a:?} -> {b:?}"
                );
                assert!(
                    b.heap_bytes >= a.heap_bytes * 7 && b.heap_bytes <= a.heap_bytes * 9,
                    "heap slope: {a:?} -> {b:?}"
                );
            }
            let deep = run_both(&chain(4096), &probe);
            let wide = run_both(&fanout(2048), &probe);
            for usage in deep {
                assert_eq!((usage.states, usage.arcs), (4097, 4096));
            }
            for usage in wide {
                assert_eq!((usage.states, usage.arcs), (2049, 2048));
            }
            assert!(probe.span() <= initial_span + 8 * 1024);
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

fn checkpoint_of<T>(outcome: OperationOutcome<T>, reason: IncompleteReason) -> OperationCheckpoint {
    let receipt = outcome.canonical_receipt_bytes();
    assert_eq!(receipt, outcome.canonical_receipt_bytes());
    assert_eq!(receipt[8], 2);
    let code = match reason {
        IncompleteReason::StateLimit => 3,
        IncompleteReason::WorkLimit => 5,
        IncompleteReason::HeapLimit => 6,
        IncompleteReason::DepthLimit => 7,
        IncompleteReason::PathLimit => 8,
        IncompleteReason::FrontierLimit => 9,
        _ => panic!("unexpected reason"),
    };
    assert_eq!(receipt[receipt.len() - 33], code);
    match outcome {
        OperationOutcome::Incomplete {
            reason: actual,
            checkpoint,
            ..
        } => {
            assert_eq!(actual, reason);
            assert_eq!(
                OperationCheckpoint::from_canonical_bytes(&checkpoint.canonical_bytes()).unwrap(),
                checkpoint
            );
            checkpoint
        }
        _ => panic!("cap must not report completion"),
    }
}

#[test]
fn interruption_receipts_and_resume_match_uncapped_results() {
    let graph = shallow(7);
    let content = binding(&graph);
    let mut baseline = BoundedShortestWitness::new(
        graph.clone(),
        content,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let best = baseline
        .run(content, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .unwrap();
    let mut shortest = BoundedShortestWitness::new(
        graph.clone(),
        content,
        OperationLimits {
            max_states: 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    let checkpoint = checkpoint_of(
        shortest.run(content, |_| 0).unwrap(),
        IncompleteReason::StateLimit,
    );
    shortest
        .resume(
            checkpoint,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        identity(
            &shortest
                .run(content, |_| 0)
                .unwrap()
                .into_complete()
                .unwrap()
                .unwrap()
        ),
        identity(&best)
    );

    let mut baseline = BoundedTopK::new(
        graph.clone(),
        content,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let expected = baseline
        .run(content, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .iter()
        .map(identity)
        .collect::<Vec<_>>();
    for (limits, path_limits, reason) in [
        (
            OperationLimits {
                max_work: 0,
                ..OperationLimits::default()
            },
            TopKLimits::default(),
            IncompleteReason::WorkLimit,
        ),
        (
            OperationLimits::default(),
            TopKLimits {
                max_depth: 0,
                ..TopKLimits::default()
            },
            IncompleteReason::DepthLimit,
        ),
        (
            OperationLimits::default(),
            TopKLimits {
                max_paths: 1,
                ..TopKLimits::default()
            },
            IncompleteReason::PathLimit,
        ),
        (
            OperationLimits::default(),
            TopKLimits {
                max_frontier: 1,
                ..TopKLimits::default()
            },
            IncompleteReason::FrontierLimit,
        ),
    ] {
        let mut topk = BoundedTopK::new(
            graph.clone(),
            content,
            limits,
            path_limits,
            CancellationToken::new(),
        )
        .unwrap();
        let checkpoint = checkpoint_of(topk.run(content, |_| 0).unwrap(), reason);
        topk.resume(
            checkpoint,
            OperationLimits::default(),
            TopKLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
        let actual = topk
            .run(content, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .iter()
            .map(identity)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut topk = BoundedTopK::new(
        graph,
        content,
        OperationLimits::default(),
        TopKLimits::default(),
        token,
    )
    .unwrap();
    let checkpoint = match topk.run(content, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            checkpoint,
            ..
        } => checkpoint,
        _ => panic!("cancelled search cannot be complete"),
    };
    topk.resume(
        checkpoint,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        topk.run(content, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .iter()
            .map(identity)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn false_complete_labels_and_partial_receipts_cannot_populate_complete_cache() {
    let graph = shallow(2);
    let content = binding(&graph);
    let mut shortest = BoundedShortestWitness::new(
        graph.clone(),
        content,
        OperationLimits {
            max_states: 0,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    let partial = shortest.run(content, |_| 0).unwrap();
    assert_eq!(partial.canonical_receipt_bytes()[8], 2);
    let checkpoint = checkpoint_of(partial, IncompleteReason::StateLimit);
    let dynamic = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        content,
        "lling.shortest.nonnegative-tropical-witness/v1",
    )
    .unwrap();
    let false_complete = OperationOutcome::Complete {
        value: (),
        checkpoint,
    };
    assert_eq!(false_complete.canonical_receipt_bytes()[8], 0);
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&dynamic, false_complete).is_err());
    assert!(cache.is_empty());

    let mut topk = BoundedTopK::new(
        graph,
        content,
        OperationLimits::default(),
        TopKLimits {
            max_paths: 1,
            ..TopKLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    let partial = topk.run(content, |_| 0).unwrap();
    assert_eq!(partial.canonical_receipt_bytes()[8], 2);
    let checkpoint = checkpoint_of(partial, IncompleteReason::PathLimit);
    let dynamic = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        content,
        "lling.topk.nonnegative-tropical-witness/v1",
    )
    .unwrap();
    let false_complete = OperationOutcome::Complete {
        value: (),
        checkpoint,
    };
    assert!(cache.insert(&dynamic, false_complete).is_err());
    assert!(cache.is_empty());
}

#[test]
fn copied_label_meter_and_late_heap_caps_are_charged_and_resumable() {
    let graph = shallow(3);
    let content = binding(&graph);
    let mut shortest_zero = BoundedShortestWitness::new(
        graph.clone(),
        content,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let shortest_zero_usage = match shortest_zero.run(content, |_| 0).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("uncapped shortest must complete"),
    };
    let mut shortest_metered = BoundedShortestWitness::new(
        graph.clone(),
        content,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let (best, shortest_usage) = match shortest_metered.run(content, |_| 7).unwrap() {
        OperationOutcome::Complete {
            value: Some(path),
            checkpoint,
        } => (path, checkpoint.usage),
        _ => panic!("uncapped shortest must produce a witness"),
    };
    let labels = best
        .steps
        .iter()
        .map(|step| usize::from(step.input.is_some()) + usize::from(step.output.is_some()))
        .sum::<usize>();
    assert_eq!(
        shortest_usage.heap_bytes - shortest_zero_usage.heap_bytes,
        (labels * 7) as u64
    );
    let mut shortest_capped = BoundedShortestWitness::new(
        graph.clone(),
        content,
        OperationLimits {
            max_heap_bytes: shortest_usage.heap_bytes - 1,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    let checkpoint = checkpoint_of(
        shortest_capped.run(content, |_| 7).unwrap(),
        IncompleteReason::HeapLimit,
    );
    shortest_capped
        .resume(
            checkpoint,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        identity(
            &shortest_capped
                .run(content, |_| 7)
                .unwrap()
                .into_complete()
                .unwrap()
                .unwrap()
        ),
        identity(&best)
    );

    let mut topk_zero = BoundedTopK::new(
        graph.clone(),
        content,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let topk_zero_usage = match topk_zero.run(content, |_| 0).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("uncapped top-k must complete"),
    };
    let mut topk_metered = BoundedTopK::new(
        graph.clone(),
        content,
        OperationLimits::default(),
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let (paths, topk_usage) = match topk_metered.run(content, |_| 7).unwrap() {
        OperationOutcome::Complete { value, checkpoint } => (value, checkpoint.usage),
        _ => panic!("uncapped top-k must complete"),
    };
    let labels = paths
        .iter()
        .flat_map(|path| &path.steps)
        .map(|step| usize::from(step.input.is_some()) + usize::from(step.output.is_some()))
        .sum::<usize>();
    assert_eq!(
        topk_usage.heap_bytes - topk_zero_usage.heap_bytes,
        (labels * 7) as u64
    );
    let mut topk_capped = BoundedTopK::new(
        graph,
        content,
        OperationLimits {
            max_heap_bytes: topk_usage.heap_bytes - 1,
            ..OperationLimits::default()
        },
        TopKLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let checkpoint = checkpoint_of(
        topk_capped.run(content, |_| 7).unwrap(),
        IncompleteReason::HeapLimit,
    );
    topk_capped
        .resume(
            checkpoint,
            OperationLimits::default(),
            TopKLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        topk_capped
            .run(content, |_| 7)
            .unwrap()
            .into_complete()
            .unwrap()
            .iter()
            .map(identity)
            .collect::<Vec<_>>(),
        paths.iter().map(identity).collect::<Vec<_>>()
    );
}
