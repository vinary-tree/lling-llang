//! Independent relation parity, malformed-source, and bounded traversal tests.

use std::sync::Arc;

use lling_llang::symbolic::bounded_restrict::{BoundedSftRestriction, SftRestrictionError};
use lling_llang::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use lling_llang::symbolic::{
    BooleanAlgebra, CharClassAlgebra, CharClassPred, IntervalAlgebra, SymbolicAutomaton,
};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationError, OperationLimits,
    OperationOutcome, OperationPlan,
};
use lling_llang::wfst::{CancellationReason, CancellationToken, SourceSnapshot};

type Sft = SymbolicFiniteTransducer<CharClassAlgebra, CharClassAlgebra>;
type Sfa = SymbolicAutomaton<CharClassAlgebra>;
const SFT: [u8; 32] = [151; 32];
const SFA: [u8; 32] = [152; 32];

fn fixture() -> (Sft, Sfa) {
    let algebra = CharClassAlgebra::new();
    let mut sft = Sft::new(algebra.clone(), algebra.clone());
    sft.add_state(false, None);
    sft.add_state(true, None);
    sft.set_initial(0);
    sft.add_transition(
        0,
        1,
        CharClassPred::Range('a', 'c'),
        OutputFunction::Identity,
    );
    sft.add_transition(
        0,
        1,
        CharClassPred::Range('b', 'b'),
        OutputFunction::Constant(vec!['x', 'y']),
    );
    sft.add_transition(
        0,
        1,
        CharClassPred::Range('c', 'c'),
        OutputFunction::Epsilon,
    );
    sft.add_transition(
        0,
        1,
        CharClassPred::Range('b', 'b'),
        OutputFunction::Map(Arc::new(|_: &char| 'm')),
    );
    sft.add_transition(
        0,
        1,
        CharClassPred::Range('c', 'c'),
        OutputFunction::FlatMap(Arc::new(|_: &char| vec!['p', 'q'])),
    );
    sft.add_transition(
        1,
        1,
        CharClassPred::Range('d', 'd'),
        OutputFunction::Identity,
    );
    let mut input = Sfa::new(algebra);
    input.add_state(false, None);
    input.add_state(true, None);
    input.add_state(true, None);
    input.set_initial(0);
    input.add_transition(0, 1, CharClassPred::Range('b', 'c'));
    input.add_transition(1, 2, CharClassPred::Range('d', 'd'));
    (sft, input)
}

fn machine(
    limits: OperationLimits,
    cancellation: CancellationToken,
) -> BoundedSftRestriction<CharClassAlgebra, CharClassAlgebra> {
    let (sft, input) = fixture();
    BoundedSftRestriction::new(sft, input, SFT, SFA, limits, cancellation).unwrap()
}

fn words() -> Vec<Vec<char>> {
    let mut result = vec![Vec::new()];
    for first in ['a', 'b', 'c', 'd'] {
        result.push(vec![first]);
        for second in ['a', 'b', 'c', 'd'] {
            result.push(vec![first, second]);
        }
    }
    result
}

// Separate shallow source-path enumerator, independent of the product builder
// and of the restricted transducer's `transduce` method.
fn source_outputs(sft: &Sft, word: &[char]) -> Vec<Vec<char>> {
    fn visit(
        sft: &Sft,
        word: &[char],
        pos: usize,
        state: usize,
        output: &[char],
        all: &mut Vec<Vec<char>>,
    ) {
        if pos == word.len() {
            if sft.accepting_states.contains(&state) {
                all.push(output.to_vec());
            }
            return;
        }
        for arc in &sft.transitions {
            if arc.from == state && sft.input_algebra.evaluate(&arc.guard, &word[pos]) {
                let mut next = output.to_vec();
                next.extend(arc.output.apply(&word[pos]));
                visit(sft, word, pos + 1, arc.to, &next, all);
            }
        }
    }
    let mut result = Vec::new();
    for &initial in &sft.initial_states {
        visit(sft, word, 0, initial, &[], &mut result);
    }
    result.sort();
    result.dedup();
    result
}

#[test]
fn exact_relation_parity_and_all_output_function_classes() {
    let (sft, input) = fixture();
    let mut machine = machine(OperationLimits::default(), CancellationToken::new());
    let restricted = machine
        .run(SFT, SFA, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    for word in words() {
        let expected = if input.accepts(&word) {
            source_outputs(&sft, &word)
        } else {
            Vec::new()
        };
        let mut actual = restricted.transduce(&word);
        actual.sort();
        actual.dedup();
        assert_eq!(actual, expected, "input {word:?}");
    }
    assert_eq!(restricted.transitions.len(), 6);
    assert!(restricted
        .transitions
        .iter()
        .all(|arc| restricted.input_algebra.is_satisfiable(&arc.guard)));
}

#[test]
fn mismatched_parameterized_input_algebras_fail_closed() {
    let sft = SymbolicFiniteTransducer::new(IntervalAlgebra::new(0, 4), CharClassAlgebra::new());
    let input = SymbolicAutomaton::new(IntervalAlgebra::new(0, 3));
    assert!(matches!(
        BoundedSftRestriction::new(
            sft,
            input,
            SFT,
            SFA,
            OperationLimits::default(),
            CancellationToken::new(),
        ),
        Err(SftRestrictionError::IncompatibleAlgebras)
    ));
}

#[test]
fn all_limits_atomicity_cancellation_resume_and_cache_exclusion() {
    let mut baseline = machine(OperationLimits::default(), CancellationToken::new());
    let usage = match baseline.run(SFT, SFA, |_| 7, |_| 11).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("uncapped restriction must complete"),
    };
    for (limits, reason) in [
        (
            OperationLimits {
                max_arcs: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::ArcLimit,
        ),
        (
            OperationLimits {
                max_states: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::StateLimit,
        ),
        (
            OperationLimits {
                max_work: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::WorkLimit,
        ),
        (
            OperationLimits {
                max_heap_bytes: usage.heap_bytes - 1,
                ..OperationLimits::default()
            },
            IncompleteReason::HeapLimit,
        ),
        (
            OperationLimits {
                max_elapsed_ns: 0,
                ..OperationLimits::default()
            },
            IncompleteReason::TimeLimit,
        ),
    ] {
        let mut machine = machine(limits, CancellationToken::new());
        let outcome = machine.run(SFT, SFA, |_| 7, |_| 11).unwrap();
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
            _ => panic!("a cap cannot prove exactness"),
        };
        assert_eq!(
            OperationCheckpoint::from_canonical_bytes(&checkpoint.canonical_bytes()).unwrap(),
            checkpoint
        );
        assert!(matches!(
            machine.resume(
                OperationCheckpoint {
                    next_index: checkpoint.next_index + 1,
                    ..checkpoint
                },
                OperationLimits::default(),
                CancellationToken::new()
            ),
            Err(SftRestrictionError::Operation(
                OperationError::StaleCheckpoint
            ))
        ));
        machine
            .resume(
                checkpoint,
                OperationLimits::default(),
                CancellationToken::new(),
            )
            .unwrap();
        assert!(machine
            .run(SFT, SFA, |_| 7, |_| 11)
            .unwrap()
            .into_complete()
            .is_some());
    }
    let mut exact_atomic = machine(
        OperationLimits {
            max_states: 0,
            ..OperationLimits::default()
        },
        CancellationToken::new(),
    );
    match exact_atomic.run(SFT, SFA, |_| 0, |_| 0).unwrap() {
        OperationOutcome::Incomplete {
            partial,
            reason: IncompleteReason::StateLimit,
            ..
        } => {
            assert_eq!(partial.transitions.len(), 0);
            assert_eq!(partial.states.len(), 1);
        }
        _ => panic!("state cap must stop before publishing a product batch"),
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut cancelled = machine(OperationLimits::default(), token);
    let outcome = cancelled.run(SFT, SFA, |_| 0, |_| 0).unwrap();
    assert!(matches!(
        outcome,
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
    assert!(matches!(
        cancelled.run([0; 32], SFA, |_| 0, |_| 0),
        Err(SftRestrictionError::Operation(OperationError::StaleSource))
    ));
    let mut digest = blake3::Hasher::new();
    digest.update(b"lling.sft.exact-domain-restriction-sources/v1\0");
    digest.update(&SFT);
    digest.update(&SFA);
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        *digest.finalize().as_bytes(),
        "lling.sft.exact-domain-restriction/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache
        .insert(&plan, baseline.run(SFT, SFA, |_| 0, |_| 0).unwrap())
        .is_err());
}

#[test]
fn malformed_reachable_targets_are_typed() {
    let (mut sft, input) = fixture();
    sft.transitions[0].to = 999;
    let mut machine = BoundedSftRestriction::new(
        sft,
        input,
        SFT,
        SFA,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, |_| 0, |_| 0),
        Err(SftRestrictionError::InvalidTarget {
            source: 1,
            transition_index: 0,
            target: 999
        })
    ));
    let (sft, mut input) = fixture();
    input.transitions[0].to = 999;
    let mut machine = BoundedSftRestriction::new(
        sft,
        input,
        SFT,
        SFA,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, |_| 0, |_| 0),
        Err(SftRestrictionError::InvalidTarget {
            source: 2,
            transition_index: 0,
            target: 999
        })
    ));
}

#[test]
fn deep_product_walk_runs_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let algebra = CharClassAlgebra::new();
            let mut sft = Sft::new(algebra.clone(), algebra.clone());
            let mut input = Sfa::new(algebra);
            for index in 0..=2048 {
                sft.add_state(index == 2048, None);
                input.add_state(index == 2048, None);
            }
            sft.set_initial(0);
            input.set_initial(0);
            for index in 0..2048 {
                sft.add_transition(
                    index,
                    index + 1,
                    CharClassPred::Range('a', 'a'),
                    OutputFunction::Identity,
                );
                input.add_transition(index, index + 1, CharClassPred::Range('a', 'a'));
            }
            let mut machine = BoundedSftRestriction::new(
                sft,
                input,
                SFT,
                SFA,
                OperationLimits::default(),
                CancellationToken::new(),
            )
            .unwrap();
            let result = machine
                .run(SFT, SFA, |_| 0, |_| 0)
                .unwrap()
                .into_complete()
                .unwrap();
            assert_eq!(result.states.len(), 2049);
            assert_eq!(result.transitions.len(), 2048);
        })
        .unwrap()
        .join()
        .unwrap();
}
