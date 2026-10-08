//! Independent small-input oracle and bounded SFT continuation controls.

use std::sync::Arc;

use lling_llang::symbolic::bounded_transduce::{
    BoundedSftTransduction, SftTransductionError, SftTransductionLimits,
};
use lling_llang::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use lling_llang::symbolic::{BooleanAlgebra, CharClassAlgebra, CharClassPred};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationError, OperationLimits,
    OperationOutcome, OperationPlan,
};
use lling_llang::wfst::{CancellationReason, CancellationToken, SourceSnapshot};

type Sft = SymbolicFiniteTransducer<CharClassAlgebra, CharClassAlgebra>;
const SOURCE: [u8; 32] = [109; 32];
const INPUT: [u8; 32] = [110; 32];

fn fixture() -> Sft {
    let algebra = CharClassAlgebra::new();
    let mut source = Sft::new(algebra.clone(), algebra);
    for id in 0..4 {
        source.add_state(id == 3, None);
    }
    source.set_initial(1);
    source.set_initial(0);
    source.add_transition(0, 2, CharClassPred::True, OutputFunction::Identity);
    source.add_transition(
        0,
        2,
        CharClassPred::Range('a', 'a'),
        OutputFunction::Constant(vec!['x', 'y']),
    );
    source.add_transition(
        0,
        3,
        CharClassPred::False,
        OutputFunction::Constant(vec!['q']),
    );
    source.add_transition(1, 2, CharClassPred::True, OutputFunction::Epsilon);
    source.add_transition(
        2,
        3,
        CharClassPred::True,
        OutputFunction::Map(Arc::new(|c: &char| c.to_ascii_uppercase())),
    );
    source.add_transition(
        2,
        3,
        CharClassPred::True,
        OutputFunction::FlatMap(Arc::new(|c: &char| vec![*c, *c])),
    );
    source
}

// Independent recursive oracle, deliberately restricted to a two-symbol input.
fn oracle(source: &Sft, input: &[char]) -> Vec<(Vec<usize>, Vec<char>)> {
    fn visit(
        source: &Sft,
        input: &[char],
        state: usize,
        at: usize,
        path: &mut Vec<usize>,
        output: &mut Vec<char>,
        answer: &mut Vec<(Vec<usize>, Vec<char>)>,
    ) {
        if at == input.len() {
            if source.accepting_states.contains(&state) {
                answer.push((path.clone(), output.clone()));
            }
            return;
        }
        for (index, transition) in source.transitions.iter().enumerate() {
            if transition.from != state
                || !source.input_algebra.evaluate(&transition.guard, &input[at])
            {
                continue;
            }
            let old_len = output.len();
            path.push(index);
            output.extend(transition.output.apply(&input[at]));
            visit(source, input, transition.to, at + 1, path, output, answer);
            output.truncate(old_len);
            path.pop();
        }
    }
    let mut answer = Vec::new();
    let mut initials: Vec<usize> = source.initial_states.iter().copied().collect();
    initials.sort_unstable();
    for state in initials {
        visit(
            source,
            input,
            state,
            0,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut answer,
        );
    }
    answer
}

fn machine(
    limits: OperationLimits,
    path_limits: SftTransductionLimits,
    token: CancellationToken,
) -> BoundedSftTransduction<CharClassAlgebra, CharClassAlgebra> {
    BoundedSftTransduction::new(
        fixture(),
        vec!['a', 'b'],
        SOURCE,
        INPUT,
        limits,
        path_limits,
        token,
    )
    .unwrap()
}

#[test]
fn hand_outputs_recursive_oracle_and_full_step_provenance() {
    let source = fixture();
    let mut search = machine(
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    );
    let paths = search
        .run(SOURCE, INPUT, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    let actual = paths
        .iter()
        .map(|path| {
            (
                path.steps
                    .iter()
                    .map(|step| step.transition_index)
                    .collect::<Vec<_>>(),
                path.output.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, oracle(&source, &['a', 'b']));
    assert_eq!(
        paths
            .iter()
            .map(|path| path.output.clone())
            .collect::<Vec<_>>(),
        vec![
            vec!['a', 'B'],
            vec!['a', 'b', 'b'],
            vec!['x', 'y', 'B'],
            vec!['x', 'y', 'b', 'b'],
            vec!['B'],
            vec!['b', 'b']
        ]
    );
    for path in &paths {
        assert_eq!(path.source_binding, SOURCE);
        assert_eq!(path.input_binding, INPUT);
        assert_eq!(path.steps.len(), 2);
        assert_eq!(path.final_state, 3);
        let mut state = path.initial_state;
        let mut cursor = 0;
        for (input_index, step) in path.steps.iter().enumerate() {
            let transition = &source.transitions[step.transition_index];
            assert_eq!(step.from, state);
            assert_eq!(step.to, transition.to);
            assert_eq!(step.input_index, input_index);
            assert_eq!(step.input, ['a', 'b'][input_index]);
            assert_eq!(step.output_range.start, cursor);
            assert_eq!(
                &path.output[step.output_range.clone()],
                transition.output.apply(&step.input)
            );
            cursor = step.output_range.end;
            state = step.to;
        }
        assert_eq!(cursor, path.output.len());
    }
    let mut legacy = source.transduce(&['a', 'b']);
    let mut bounded = paths
        .iter()
        .map(|path| path.output.clone())
        .collect::<Vec<_>>();
    legacy.sort();
    bounded.sort();
    assert_eq!(bounded, legacy);
}

#[test]
fn indexing_state_path_and_frontier_caps_resume_exactly() {
    let mut baseline = machine(
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    );
    let expected = baseline
        .run(SOURCE, INPUT, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .iter()
        .map(|path| path.output.clone())
        .collect::<Vec<_>>();
    for (limits, path_limits, reason) in [
        (
            OperationLimits {
                max_arcs: 0,
                ..OperationLimits::default()
            },
            SftTransductionLimits::default(),
            IncompleteReason::ArcLimit,
        ),
        (
            OperationLimits {
                max_states: 0,
                ..OperationLimits::default()
            },
            SftTransductionLimits::default(),
            IncompleteReason::StateLimit,
        ),
        (
            OperationLimits {
                max_work: 0,
                ..OperationLimits::default()
            },
            SftTransductionLimits::default(),
            IncompleteReason::WorkLimit,
        ),
        (
            OperationLimits {
                max_elapsed_ns: 0,
                ..OperationLimits::default()
            },
            SftTransductionLimits::default(),
            IncompleteReason::TimeLimit,
        ),
        (
            OperationLimits::default(),
            SftTransductionLimits {
                max_paths: 1,
                ..SftTransductionLimits::default()
            },
            IncompleteReason::PathLimit,
        ),
        (
            OperationLimits::default(),
            SftTransductionLimits {
                max_frontier: 1,
                ..SftTransductionLimits::default()
            },
            IncompleteReason::FrontierLimit,
        ),
    ] {
        let mut search = machine(limits, path_limits, CancellationToken::new());
        let checkpoint = match search.run(SOURCE, INPUT, |_| 0, |_| 0).unwrap() {
            OperationOutcome::Incomplete {
                partial,
                reason: got,
                checkpoint,
            } => {
                assert_eq!(got, reason);
                assert!(partial.len() <= expected.len());
                assert_eq!(
                    OperationCheckpoint::from_canonical_bytes(&checkpoint.canonical_bytes())
                        .unwrap(),
                    checkpoint
                );
                checkpoint
            }
            _ => panic!("unexhausted search cannot be complete"),
        };
        assert!(matches!(
            search.resume(
                OperationCheckpoint {
                    next_index: checkpoint.next_index + 1,
                    ..checkpoint
                },
                OperationLimits::default(),
                SftTransductionLimits::default(),
                CancellationToken::new()
            ),
            Err(SftTransductionError::Operation(
                OperationError::StaleCheckpoint
            ))
        ));
        search
            .resume(
                checkpoint,
                OperationLimits::default(),
                SftTransductionLimits::default(),
                CancellationToken::new(),
            )
            .unwrap();
        let actual = search
            .run(SOURCE, INPUT, |_| 0, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .iter()
            .map(|path| path.output.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}

#[test]
fn cancellation_source_drift_late_heap_cap_and_cache_rejection() {
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut search = machine(
        OperationLimits::default(),
        SftTransductionLimits::default(),
        token,
    );
    let outcome = search.run(SOURCE, INPUT, |_| 0, |_| 0).unwrap();
    assert_eq!(outcome.canonical_receipt_bytes()[8], 2);
    let checkpoint = match outcome {
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            checkpoint,
            ..
        } => checkpoint,
        _ => panic!("cancelled search cannot complete"),
    };
    assert!(matches!(
        search.run(SOURCE, [111; 32], |_| 0, |_| 0),
        Err(SftTransductionError::Operation(OperationError::StaleSource))
    ));
    assert!(matches!(
        search.run([112; 32], INPUT, |_| 0, |_| 0),
        Err(SftTransductionError::Operation(OperationError::StaleSource))
    ));
    search
        .resume(
            checkpoint,
            OperationLimits::default(),
            SftTransductionLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        search
            .run(SOURCE, INPUT, |_| 0, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .len(),
        6
    );

    let mut baseline = machine(
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    );
    let usage = match baseline.run(SOURCE, INPUT, |_| 7, |_| 11).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("baseline must complete"),
    };
    let mut capped = machine(
        OperationLimits {
            max_heap_bytes: usage.heap_bytes - 1,
            ..OperationLimits::default()
        },
        SftTransductionLimits::default(),
        CancellationToken::new(),
    );
    let outcome = capped.run(SOURCE, INPUT, |_| 7, |_| 11).unwrap();
    assert_eq!(outcome.canonical_receipt_bytes()[8], 2);
    let mut digest = blake3::Hasher::new();
    digest.update(b"lling.sft.source-and-word/v1\0");
    digest.update(&SOURCE);
    digest.update(&INPUT);
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        *digest.finalize().as_bytes(),
        "lling.sft.concrete-transduction/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache.insert(&plan, outcome).is_err());
    let checkpoint = match capped.run(SOURCE, INPUT, |_| 7, |_| 11).unwrap() {
        OperationOutcome::Incomplete {
            reason: IncompleteReason::HeapLimit,
            checkpoint,
            ..
        } => checkpoint,
        _ => panic!("late heap cap cannot complete"),
    };
    capped
        .resume(
            checkpoint,
            OperationLimits::default(),
            SftTransductionLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        capped
            .run(SOURCE, INPUT, |_| 7, |_| 11)
            .unwrap()
            .into_complete()
            .unwrap()
            .len(),
        6
    );
}

#[test]
fn caller_payload_meters_account_for_retained_fragments_and_copied_witnesses() {
    let mut zero = machine(
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    );
    let zero_usage = match zero.run(SOURCE, INPUT, |_| 0, |_| 0).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("uncapped search must complete"),
    };
    let mut metered = machine(
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    );
    let metered_usage = match metered.run(SOURCE, INPUT, |_| 7, |_| 11).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("uncapped search must complete"),
    };
    // Twelve input copies in six two-step witnesses; twelve retained output
    // elements in arena fragments and fifteen copied into final witnesses.
    assert_eq!(
        metered_usage.heap_bytes - zero_usage.heap_bytes,
        12 * 7 + (12 + 15) * 11
    );
}

#[test]
fn empty_word_and_malformed_reachable_endpoints() {
    let algebra = CharClassAlgebra::new();
    let mut source = Sft::new(algebra.clone(), algebra);
    source.add_state(true, None);
    source.set_initial(0);
    let mut search = BoundedSftTransduction::new(
        source.clone(),
        Vec::<char>::new(),
        SOURCE,
        INPUT,
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let paths = search
        .run(SOURCE, INPUT, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].steps.is_empty() && paths[0].output.is_empty());
    source.initial_states.clear();
    let mut no_start = BoundedSftTransduction::new(
        source.clone(),
        Vec::<char>::new(),
        SOURCE,
        INPUT,
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(no_start
        .run(SOURCE, INPUT, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap()
        .is_empty());
    source.set_initial(0);
    source.initial_states.insert(99);
    let mut search = BoundedSftTransduction::new(
        source.clone(),
        Vec::<char>::new(),
        SOURCE,
        INPUT,
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        search.run(SOURCE, INPUT, |_| 0, |_| 0),
        Err(SftTransductionError::InvalidInitial(99))
    ));
    source.initial_states.remove(&99);
    source
        .transitions
        .push(lling_llang::symbolic::sft::SftTransition {
            from: 0,
            to: 99,
            guard: CharClassPred::True,
            output: OutputFunction::Identity,
        });
    let mut search = BoundedSftTransduction::new(
        source,
        vec!['a'],
        SOURCE,
        INPUT,
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        search.run(SOURCE, INPUT, |_| 0, |_| 0),
        Err(SftTransductionError::InvalidTarget {
            transition_index: 0,
            target: 99
        })
    ));
}
