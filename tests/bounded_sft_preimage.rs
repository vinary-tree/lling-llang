//! Independent language oracle, exact pullback and interruption controls.

use std::sync::Arc;

use lling_llang::symbolic::bounded_preimage::{
    BoundedSftPreimage, ExactOutputPreimageOracle, PreimageCases, PreimageOracleError,
    SameAlgebraPreimageOracle, SftPreimageError,
};
use lling_llang::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use lling_llang::symbolic::{BooleanAlgebra, CharClassAlgebra, CharClassPred, SymbolicAutomaton};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationError, OperationLimits,
    OperationOutcome, OperationPlan,
};
use lling_llang::wfst::{CancellationReason, CancellationToken, SourceSnapshot};

type Sft = SymbolicFiniteTransducer<CharClassAlgebra, CharClassAlgebra>;
type Sfa = SymbolicAutomaton<CharClassAlgebra>;
const SFT: [u8; 32] = [131; 32];
const SFA: [u8; 32] = [132; 32];
const ORACLE: [u8; 32] = [133; 32];
const CUSTOM_ORACLE: [u8; 32] = [135; 32];

fn fixture() -> (Sft, Sfa) {
    let algebra = CharClassAlgebra::new();
    let mut sft = Sft::new(algebra.clone(), algebra.clone());
    sft.add_state(false, None);
    sft.add_state(true, None);
    sft.add_state(true, None);
    sft.set_initial(0);
    sft.add_transition(
        0,
        1,
        CharClassPred::Range('a', 'a'),
        OutputFunction::Identity,
    );
    sft.add_transition(
        0,
        2,
        CharClassPred::Range('a', 'a'),
        OutputFunction::Constant(vec!['x', 'y']),
    );
    sft.add_transition(
        0,
        1,
        CharClassPred::Range('b', 'b'),
        OutputFunction::Epsilon,
    );
    sft.add_transition(
        0,
        2,
        CharClassPred::Range('a', 'a'),
        OutputFunction::Constant(vec!['x']),
    );
    sft.add_transition(
        1,
        1,
        CharClassPred::Range('c', 'c'),
        OutputFunction::Epsilon,
    );

    let mut sfa = Sfa::new(algebra);
    sfa.add_state(true, None); // empty output is accepted
    sfa.add_state(false, None);
    sfa.add_state(true, None);
    sfa.add_state(true, None);
    sfa.set_initial(0);
    sfa.add_transition(0, 1, CharClassPred::Range('x', 'x'));
    sfa.add_transition(0, 2, CharClassPred::Range('x', 'x'));
    sfa.add_transition(1, 3, CharClassPred::Range('y', 'y'));
    sfa.add_transition(2, 3, CharClassPred::Range('y', 'y'));
    sfa.add_transition(0, 3, CharClassPred::Range('a', 'a'));
    (sft, sfa)
}

fn machine(
    limits: OperationLimits,
    token: CancellationToken,
) -> BoundedSftPreimage<CharClassAlgebra, CharClassAlgebra, SameAlgebraPreimageOracle> {
    let (sft, sfa) = fixture();
    BoundedSftPreimage::new(
        sft,
        sfa,
        SameAlgebraPreimageOracle,
        SFT,
        SFA,
        ORACLE,
        limits,
        token,
    )
    .unwrap()
}

fn words() -> Vec<Vec<char>> {
    let mut result = vec![Vec::new()];
    for a in ['a', 'b', 'c'] {
        result.push(vec![a]);
    }
    for a in ['a', 'b', 'c'] {
        for b in ['a', 'b', 'c'] {
            result.push(vec![a, b]);
        }
    }
    result
}

fn expected(sft: &Sft, sfa: &Sfa, word: &[char]) -> bool {
    sft.transduce(word).iter().any(|output| sfa.accepts(output))
}

#[test]
fn recursive_shallow_language_oracle_and_nondeterministic_constant_paths() {
    let (sft, sfa) = fixture();
    let mut machine = machine(OperationLimits::default(), CancellationToken::new());
    let graph = machine
        .run(SFT, SFA, ORACLE, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    for word in words() {
        assert_eq!(
            graph.accepts(&word),
            expected(&sft, &sfa, &word),
            "word {word:?}"
        );
    }
    assert!(graph.accepts(&['a']));
    assert!(graph.accepts(&['b']));
    assert!(graph.accepts(&['b', 'c']));
    assert!(!graph.accepts(&['c']));
    assert!(!graph.accepts(&[]));
    assert_eq!(graph.initial_states.len(), 1);
    assert!(graph
        .transitions
        .iter()
        .all(|t| graph.algebra.is_satisfiable(&t.guard)));
}

#[test]
fn unrepresentable_computed_output_rejects_instead_of_broadening() {
    let (mut sft, sfa) = fixture();
    sft.transitions[0].output = OutputFunction::Map(Arc::new(|_: &char| 'x'));
    let mut machine = BoundedSftPreimage::new(
        sft,
        sfa,
        SameAlgebraPreimageOracle,
        SFT,
        SFA,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, ORACLE, |_| 0),
        Err(SftPreimageError::Oracle {
            transition_index: 0,
            error: PreimageOracleError::UnrepresentableOutput
        })
    ));
}

// This application knows the two computed functions below are constants.
// Its exact finite case derivation is explicit; a generic oracle cannot
// infer that property from an opaque closure.
struct KnownConstantComputedOracle;
impl ExactOutputPreimageOracle<CharClassAlgebra, CharClassAlgebra> for KnownConstantComputedOracle {
    fn cases(
        &self,
        algebra: &CharClassAlgebra,
        output: &OutputFunction<CharClassAlgebra, CharClassAlgebra>,
        acceptor: &Sfa,
        index: &[Vec<usize>],
        start: usize,
    ) -> Result<PreimageCases<CharClassPred>, PreimageOracleError> {
        let basic = SameAlgebraPreimageOracle;
        match output {
            OutputFunction::Map(_) => basic.cases(
                algebra,
                &OutputFunction::Constant(vec!['x']),
                acceptor,
                index,
                start,
            ),
            OutputFunction::FlatMap(_) => basic.cases(
                algebra,
                &OutputFunction::Constant(vec!['x', 'y']),
                acceptor,
                index,
                start,
            ),
            other => basic.cases(algebra, other, acceptor, index, start),
        }
    }
}

#[test]
fn explicit_exact_oracle_admits_known_map_and_flatmap() {
    let (mut sft, sfa) = fixture();
    sft.add_transition(
        0,
        2,
        CharClassPred::Range('a', 'a'),
        OutputFunction::Map(Arc::new(|_: &char| 'x')),
    );
    sft.add_transition(
        0,
        2,
        CharClassPred::Range('a', 'a'),
        OutputFunction::FlatMap(Arc::new(|_: &char| vec!['x', 'y'])),
    );
    let mut machine = BoundedSftPreimage::new(
        sft.clone(),
        sfa.clone(),
        KnownConstantComputedOracle,
        SFT,
        SFA,
        CUSTOM_ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let graph = machine
        .run(SFT, SFA, CUSTOM_ORACLE, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    for word in words() {
        assert_eq!(
            graph.accepts(&word),
            expected(&sft, &sfa, &word),
            "word {word:?}"
        );
    }
}

#[test]
fn all_limits_cancellation_resume_and_complete_cache_exclusion() {
    let (sft, sfa) = fixture();
    let mut baseline = machine(OperationLimits::default(), CancellationToken::new());
    let usage = match baseline.run(SFT, SFA, ORACLE, |_| 7).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("uncapped preimage must complete"),
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
        let outcome = machine.run(SFT, SFA, ORACLE, |_| 7).unwrap();
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
            Err(SftPreimageError::Operation(OperationError::StaleCheckpoint))
        ));
        machine
            .resume(
                checkpoint,
                OperationLimits::default(),
                CancellationToken::new(),
            )
            .unwrap();
        let graph = machine
            .run(SFT, SFA, ORACLE, |_| 7)
            .unwrap()
            .into_complete()
            .unwrap();
        for word in words() {
            assert_eq!(graph.accepts(&word), expected(&sft, &sfa, &word));
        }
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut cancelled = machine(OperationLimits::default(), token);
    let outcome = cancelled.run(SFT, SFA, ORACLE, |_| 0).unwrap();
    assert!(matches!(
        &outcome,
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
    let mut digest = blake3::Hasher::new();
    digest.update(b"lling.sft.exact-preimage-sources/v1\0");
    digest.update(&SFT);
    digest.update(&SFA);
    digest.update(&ORACLE);
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        *digest.finalize().as_bytes(),
        "lling.sft.exact-preimage/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    let rejected = cache.insert(&plan, outcome).unwrap_err();
    let checkpoint = match *rejected {
        OperationOutcome::Incomplete { checkpoint, .. } => checkpoint,
        _ => panic!("cancelled"),
    };
    assert!(cache
        .insert(
            &plan,
            OperationOutcome::Complete {
                value: Sfa::new(CharClassAlgebra::new()),
                checkpoint
            }
        )
        .is_err());
    assert!(cache.is_empty());
    assert!(matches!(
        cancelled.run([134; 32], SFA, ORACLE, |_| 0),
        Err(SftPreimageError::Operation(OperationError::StaleSource))
    ));
    cancelled
        .resume(
            checkpoint,
            OperationLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
    assert!(cancelled
        .run(SFT, SFA, ORACLE, |_| 0)
        .unwrap()
        .into_complete()
        .is_some());
}

#[test]
fn malformed_targets_fail_closed_and_unreachable_computed_case_is_ignored() {
    let (mut sft, sfa) = fixture();
    sft.transitions[0].to = 99;
    let mut machine = BoundedSftPreimage::new(
        sft,
        sfa.clone(),
        SameAlgebraPreimageOracle,
        SFT,
        SFA,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, ORACLE, |_| 0),
        Err(SftPreimageError::InvalidTarget {
            source: 1,
            transition_index: 0,
            target: 99
        })
    ));
    let (mut sft, mut sfa) = fixture();
    sfa.transitions[0].to = 99;
    let mut machine = BoundedSftPreimage::new(
        sft.clone(),
        sfa,
        SameAlgebraPreimageOracle,
        SFT,
        SFA,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, ORACLE, |_| 0),
        Err(SftPreimageError::Oracle {
            error: PreimageOracleError::InvalidAcceptorTarget {
                transition_index: 0,
                target: 99
            },
            ..
        })
    ));
    sft.add_transition(
        0,
        1,
        CharClassPred::False,
        OutputFunction::Map(Arc::new(|_: &char| 'x')),
    );
    let (_, sfa) = fixture();
    let mut machine = BoundedSftPreimage::new(
        sft,
        sfa,
        SameAlgebraPreimageOracle,
        SFT,
        SFA,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(machine
        .run(SFT, SFA, ORACLE, |_| 0)
        .unwrap()
        .into_complete()
        .is_some());
}
