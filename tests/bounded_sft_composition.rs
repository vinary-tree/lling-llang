//! Exact composition parity, provenance and bounded continuation controls.

use std::sync::Arc;

use lling_llang::symbolic::bounded_compose::{
    BoundedSftComposition, SftCompositionError, SftCompositionLimits,
};
use lling_llang::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use lling_llang::symbolic::{BooleanAlgebra, CharClassAlgebra, CharClassPred};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationError, OperationLimits,
    OperationOutcome, OperationPlan,
};
use lling_llang::wfst::{CancellationReason, CancellationToken, SourceSnapshot};

type Sft = SymbolicFiniteTransducer<CharClassAlgebra, CharClassAlgebra>;
const FIRST: [u8; 32] = [121; 32];
const SECOND: [u8; 32] = [122; 32];
const INPUT: [u8; 32] = [123; 32];

fn pair() -> (Sft, Sft) {
    let algebra = CharClassAlgebra::new();
    let mut first = Sft::new(algebra.clone(), algebra.clone());
    first.add_state(false, None);
    first.add_state(true, None);
    first.set_initial(0);
    first.add_transition(0, 1, CharClassPred::True, OutputFunction::Identity);
    first.add_transition(
        0,
        1,
        CharClassPred::True,
        OutputFunction::Constant(vec!['x', 'y']),
    );
    first.add_transition(0, 1, CharClassPred::True, OutputFunction::Epsilon);
    first.add_transition(
        0,
        1,
        CharClassPred::True,
        OutputFunction::Map(Arc::new(|c: &char| c.to_ascii_uppercase())),
    );
    first.add_transition(
        0,
        1,
        CharClassPred::True,
        OutputFunction::FlatMap(Arc::new(|c: &char| vec![*c, *c])),
    );

    let mut second = Sft::new(algebra.clone(), algebra);
    second.add_state(true, None);
    second.set_initial(0);
    second.add_transition(0, 0, CharClassPred::True, OutputFunction::Identity);
    second.add_transition(
        0,
        0,
        CharClassPred::True,
        OutputFunction::Constant(vec!['!']),
    );
    second.add_transition(
        0,
        0,
        CharClassPred::True,
        OutputFunction::Map(Arc::new(|c: &char| c.to_ascii_uppercase())),
    );
    second.add_transition(
        0,
        0,
        CharClassPred::Range('x', 'x'),
        OutputFunction::FlatMap(Arc::new(|_: &char| vec!['X', 'X'])),
    );
    (first, second)
}

fn machine(
    limits: OperationLimits,
    paths: SftCompositionLimits,
    token: CancellationToken,
) -> BoundedSftComposition<CharClassAlgebra, CharClassAlgebra, CharClassAlgebra> {
    let (first, second) = pair();
    BoundedSftComposition::new(
        first,
        second,
        vec!['a'],
        FIRST,
        SECOND,
        INPUT,
        limits,
        paths,
        token,
    )
    .unwrap()
}

#[test]
fn independent_sequential_semantics_and_complete_two_stage_provenance() {
    let (first, second) = pair();
    let mut expected = first
        .transduce(&['a'])
        .iter()
        .flat_map(|middle| second.transduce(middle))
        .collect::<Vec<_>>();
    let mut search = machine(
        OperationLimits::default(),
        SftCompositionLimits::default(),
        CancellationToken::new(),
    );
    let paths = search
        .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert_eq!(paths.len(), 28);
    let mut actual = paths.iter().map(|w| w.output.clone()).collect::<Vec<_>>();
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
    for witness in &paths {
        assert_eq!(
            (
                witness.first_binding,
                witness.second_binding,
                witness.input_binding
            ),
            (FIRST, SECOND, INPUT)
        );
        assert_eq!(
            (
                witness.first_initial,
                witness.first_final,
                witness.second_initial,
                witness.second_final
            ),
            (0, 1, 0, 0)
        );
        assert_eq!(witness.first_steps.len(), 1);
        let first_step = &witness.first_steps[0];
        let first_transition = &first.transitions[first_step.transition_index];
        assert_eq!(
            (
                first_step.from,
                first_step.to,
                first_step.input_index,
                first_step.input
            ),
            (0, 1, 0, 'a')
        );
        assert_eq!(
            &witness.intermediate[first_step.intermediate_range.clone()],
            first_transition.output.apply(&'a')
        );
        assert_eq!(witness.second_steps.len(), witness.intermediate.len());
        let mut output_cursor = 0;
        for step in &witness.second_steps {
            let transition = &second.transitions[step.transition_index];
            assert_eq!((step.from, step.to), (0, 0));
            assert_eq!(step.input, witness.intermediate[step.intermediate_index]);
            assert!(second
                .input_algebra
                .evaluate(&transition.guard, &step.input));
            assert_eq!(step.output_range.start, output_cursor);
            assert_eq!(
                &witness.output[step.output_range.clone()],
                transition.output.apply(&step.input)
            );
            output_cursor = step.output_range.end;
        }
        assert_eq!(output_cursor, witness.output.len());
    }
    let mut repeat = machine(
        OperationLimits::default(),
        SftCompositionLimits::default(),
        CancellationToken::new(),
    );
    let again = repeat
        .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert_eq!(
        paths
            .iter()
            .map(|w| (&w.intermediate, &w.output))
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|w| (&w.intermediate, &w.output))
            .collect::<Vec<_>>()
    );
}

#[test]
fn computed_and_identity_guard_mismatch_do_not_create_false_paths() {
    let algebra = CharClassAlgebra::new();
    for output in [
        OutputFunction::Identity,
        OutputFunction::Map(Arc::new(|_: &char| 'x')),
    ] {
        let mut first = Sft::new(algebra.clone(), algebra.clone());
        first.add_state(false, None);
        first.add_state(true, None);
        first.set_initial(0);
        first.add_transition(0, 1, CharClassPred::Range('a', 'a'), output);
        let mut second = Sft::new(algebra.clone(), algebra.clone());
        second.add_state(true, None);
        second.set_initial(0);
        second.add_transition(
            0,
            0,
            CharClassPred::Range('z', 'z'),
            OutputFunction::Identity,
        );
        let mut search = BoundedSftComposition::new(
            first,
            second,
            vec!['a'],
            FIRST,
            SECOND,
            INPUT,
            OperationLimits::default(),
            SftCompositionLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
        assert!(search
            .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn indexing_state_path_frontier_and_heap_caps_resume_exactly() {
    let mut baseline = machine(
        OperationLimits::default(),
        SftCompositionLimits::default(),
        CancellationToken::new(),
    );
    let (expected, usage) = match baseline
        .run(FIRST, SECOND, INPUT, |_| 7, |_| 11, |_| 13)
        .unwrap()
    {
        OperationOutcome::Complete { value, checkpoint } => (
            value
                .iter()
                .map(|w| (w.intermediate.clone(), w.output.clone()))
                .collect::<Vec<_>>(),
            checkpoint.usage,
        ),
        _ => panic!("uncapped must complete"),
    };
    for (limits, paths, reason) in [
        (
            OperationLimits {
                max_arcs: 0,
                ..OperationLimits::default()
            },
            SftCompositionLimits::default(),
            IncompleteReason::ArcLimit,
        ),
        (
            OperationLimits {
                max_states: 0,
                ..OperationLimits::default()
            },
            SftCompositionLimits::default(),
            IncompleteReason::StateLimit,
        ),
        (
            OperationLimits {
                max_work: 0,
                ..OperationLimits::default()
            },
            SftCompositionLimits::default(),
            IncompleteReason::WorkLimit,
        ),
        (
            OperationLimits {
                max_elapsed_ns: 0,
                ..OperationLimits::default()
            },
            SftCompositionLimits::default(),
            IncompleteReason::TimeLimit,
        ),
        (
            OperationLimits {
                max_heap_bytes: usage.heap_bytes - 1,
                ..OperationLimits::default()
            },
            SftCompositionLimits::default(),
            IncompleteReason::HeapLimit,
        ),
        (
            OperationLimits::default(),
            SftCompositionLimits {
                max_paths: 1,
                ..SftCompositionLimits::default()
            },
            IncompleteReason::PathLimit,
        ),
        (
            OperationLimits::default(),
            SftCompositionLimits {
                max_frontier: 1,
                ..SftCompositionLimits::default()
            },
            IncompleteReason::FrontierLimit,
        ),
    ] {
        let mut search = machine(limits, paths, CancellationToken::new());
        let outcome = search
            .run(FIRST, SECOND, INPUT, |_| 7, |_| 11, |_| 13)
            .unwrap();
        assert_eq!(outcome.canonical_receipt_bytes()[8], 2);
        let checkpoint = match outcome {
            OperationOutcome::Incomplete {
                reason: got,
                checkpoint,
                ..
            } => {
                assert_eq!(got, reason);
                checkpoint
            }
            _ => panic!("cap cannot certify completion"),
        };
        assert_eq!(
            OperationCheckpoint::from_canonical_bytes(&checkpoint.canonical_bytes()).unwrap(),
            checkpoint
        );
        assert!(matches!(
            search.resume(
                OperationCheckpoint {
                    next_index: checkpoint.next_index + 1,
                    ..checkpoint
                },
                OperationLimits::default(),
                SftCompositionLimits::default(),
                CancellationToken::new()
            ),
            Err(SftCompositionError::Operation(
                OperationError::StaleCheckpoint
            ))
        ));
        search
            .resume(
                checkpoint,
                OperationLimits::default(),
                SftCompositionLimits::default(),
                CancellationToken::new(),
            )
            .unwrap();
        let actual = search
            .run(FIRST, SECOND, INPUT, |_| 7, |_| 11, |_| 13)
            .unwrap()
            .into_complete()
            .unwrap();
        assert_eq!(
            actual
                .iter()
                .map(|w| (w.intermediate.clone(), w.output.clone()))
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn cancellation_drift_false_complete_cache_and_malformed_targets() {
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut search = machine(
        OperationLimits::default(),
        SftCompositionLimits::default(),
        token,
    );
    let outcome = search
        .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
        .unwrap();
    assert_eq!(outcome.canonical_receipt_bytes()[8], 2);
    let checkpoint = match outcome {
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            checkpoint,
            ..
        } => checkpoint,
        _ => panic!("cancelled"),
    };
    assert!(matches!(
        search.run([124; 32], SECOND, INPUT, |_| 0, |_| 0, |_| 0),
        Err(SftCompositionError::Operation(OperationError::StaleSource))
    ));
    assert!(matches!(
        search.run(FIRST, [125; 32], INPUT, |_| 0, |_| 0, |_| 0),
        Err(SftCompositionError::Operation(OperationError::StaleSource))
    ));
    assert!(matches!(
        search.run(FIRST, SECOND, [126; 32], |_| 0, |_| 0, |_| 0),
        Err(SftCompositionError::Operation(OperationError::StaleSource))
    ));
    search
        .resume(
            checkpoint,
            OperationLimits::default(),
            SftCompositionLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        search
            .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .len(),
        28
    );

    let mut digest = blake3::Hasher::new();
    digest.update(b"lling.sft.composed-source-and-word/v1\0");
    digest.update(&FIRST);
    digest.update(&SECOND);
    digest.update(&INPUT);
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        *digest.finalize().as_bytes(),
        "lling.sft.concrete-composition/v1",
    )
    .unwrap();
    let mut partial = machine(
        OperationLimits::default(),
        SftCompositionLimits {
            max_paths: 1,
            ..SftCompositionLimits::default()
        },
        CancellationToken::new(),
    );
    let outcome = partial
        .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
        .unwrap();
    let mut cache = CompleteResultCache::default();
    let rejected = cache.insert(&plan, outcome).unwrap_err();
    let checkpoint = match *rejected {
        OperationOutcome::Incomplete { checkpoint, .. } => checkpoint,
        _ => panic!("the path cap cannot complete"),
    };
    assert!(cache
        .insert(
            &plan,
            OperationOutcome::Complete {
                value: Vec::new(),
                checkpoint
            }
        )
        .is_err());
    assert!(cache.is_empty());

    let (mut first, second) = pair();
    first.transitions[0].to = 99;
    let mut malformed = BoundedSftComposition::new(
        first,
        second,
        vec!['a'],
        FIRST,
        SECOND,
        INPUT,
        OperationLimits::default(),
        SftCompositionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        malformed.run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0),
        Err(SftCompositionError::InvalidTarget {
            stage: 1,
            transition_index: 0,
            target: 99
        })
    ));

    let (first, mut second) = pair();
    second.transitions[0].to = 99;
    let mut malformed = BoundedSftComposition::new(
        first,
        second,
        vec!['a'],
        FIRST,
        SECOND,
        INPUT,
        OperationLimits::default(),
        SftCompositionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        malformed.run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0),
        Err(SftCompositionError::InvalidTarget {
            stage: 2,
            transition_index: 0,
            target: 99
        })
    ));
}

#[test]
fn empty_word_multiple_initial_product_roots_and_frontier_resume() {
    let algebra = CharClassAlgebra::new();
    let mut first = Sft::new(algebra.clone(), algebra.clone());
    let mut second = Sft::new(algebra.clone(), algebra);
    for _ in 0..2 {
        first.add_state(true, None);
        second.add_state(true, None);
    }
    first.set_initial(1);
    first.set_initial(0);
    second.set_initial(1);
    second.set_initial(0);
    let mut capped = BoundedSftComposition::new(
        first,
        second,
        Vec::<char>::new(),
        FIRST,
        SECOND,
        INPUT,
        OperationLimits::default(),
        SftCompositionLimits {
            max_frontier: 3,
            ..SftCompositionLimits::default()
        },
        CancellationToken::new(),
    )
    .unwrap();
    let checkpoint = match capped
        .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
        .unwrap()
    {
        OperationOutcome::Incomplete {
            reason: IncompleteReason::FrontierLimit,
            partial,
            checkpoint,
        } => {
            assert!(partial.is_empty());
            checkpoint
        }
        _ => panic!("four product roots exceed frontier cap"),
    };
    capped
        .resume(
            checkpoint,
            OperationLimits::default(),
            SftCompositionLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    let paths = capped
        .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    assert_eq!(
        paths
            .iter()
            .map(|w| (w.first_initial, w.second_initial))
            .collect::<Vec<_>>(),
        vec![(0, 0), (0, 1), (1, 0), (1, 1)]
    );
    assert!(paths.iter().all(|w| w.first_steps.is_empty()
        && w.second_steps.is_empty()
        && w.intermediate.is_empty()
        && w.output.is_empty()));
}
