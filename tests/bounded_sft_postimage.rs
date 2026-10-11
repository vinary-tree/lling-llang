//! Independent shallow relation oracle and bounded-operation controls.

use std::sync::Arc;

use lling_llang::symbolic::bounded_postimage::{
    BoundedSftPostimage, ExactOutputPostimageOracle, PostimageCase, PostimageOracleError,
    SameAlgebraPostimageOracle, SftPostimageError,
};
use lling_llang::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use lling_llang::symbolic::{CharClassAlgebra, CharClassPred, SymbolicAutomaton};
use lling_llang::symbolic::{IntervalAlgebra, IntervalPred};
use lling_llang::wfst::operation::{
    CompleteResultCache, IncompleteReason, OperationCheckpoint, OperationError, OperationLimits,
    OperationOutcome, OperationPlan,
};
use lling_llang::wfst::{CancellationReason, CancellationToken, SourceSnapshot};

type Sft = SymbolicFiniteTransducer<CharClassAlgebra, CharClassAlgebra>;
type Sfa = SymbolicAutomaton<CharClassAlgebra>;
const SFT: [u8; 32] = [141; 32];
const SFA: [u8; 32] = [142; 32];
const ORACLE: [u8; 32] = [143; 32];
const CUSTOM_ORACLE: [u8; 32] = [144; 32];

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

    let mut input = Sfa::new(algebra);
    input.add_state(false, None);
    input.add_state(true, None);
    input.add_state(true, None);
    input.set_initial(0);
    input.add_transition(0, 1, CharClassPred::Range('a', 'a'));
    input.add_transition(0, 1, CharClassPred::Range('b', 'b'));
    input.add_transition(1, 2, CharClassPred::Range('c', 'c'));
    (sft, input)
}

fn machine(
    limits: OperationLimits,
    token: CancellationToken,
) -> BoundedSftPostimage<CharClassAlgebra, CharClassAlgebra, SameAlgebraPostimageOracle> {
    let (sft, input) = fixture();
    BoundedSftPostimage::new(
        sft,
        input,
        SameAlgebraPostimageOracle,
        SFT,
        SFA,
        ORACLE,
        limits,
        token,
    )
    .unwrap()
}

fn words(alphabet: &[char], max_len: usize) -> Vec<Vec<char>> {
    let mut all = vec![Vec::new()];
    let mut layer = vec![Vec::new()];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for prefix in &layer {
            for &symbol in alphabet {
                let mut word = prefix.clone();
                word.push(symbol);
                next.push(word);
            }
        }
        all.extend(next.iter().cloned());
        layer = next;
    }
    all
}

// Independent recursive shallow oracle: enumerate source paths and concatenate
// their concrete outputs. Only test inputs of length <=2 reach this function.
fn source_outputs(sft: &Sft, input: &[char]) -> Vec<Vec<char>> {
    fn visit(
        sft: &Sft,
        input: &[char],
        pos: usize,
        state: usize,
        output: &[char],
        all: &mut Vec<Vec<char>>,
    ) {
        if pos == input.len() {
            if sft.accepting_states.contains(&state) {
                all.push(output.to_vec());
            }
            return;
        }
        for arc in &sft.transitions {
            if arc.from == state && sft.input_algebra.evaluate(&arc.guard, &input[pos]) {
                let mut produced = output.to_vec();
                produced.extend(arc.output.apply(&input[pos]));
                visit(sft, input, pos + 1, arc.to, &produced, all);
            }
        }
    }
    use lling_llang::symbolic::BooleanAlgebra;
    let mut all = Vec::new();
    for &initial in &sft.initial_states {
        visit(sft, input, 0, initial, &[], &mut all);
    }
    all
}

#[test]
fn shallow_oracle_epsilon_identity_constant_and_nondeterminism() {
    let (sft, input) = fixture();
    let mut machine = machine(OperationLimits::default(), CancellationToken::new());
    let graph = machine
        .run(SFT, SFA, ORACLE, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    let source_words = words(&['a', 'b', 'c'], 2);
    for output in words(&['a', 'b', 'c', 'x', 'y', 'z'], 3) {
        let expected = source_words
            .iter()
            .any(|word| input.accepts(word) && source_outputs(&sft, word).contains(&output));
        assert_eq!(graph.accepts(&output), expected, "output {output:?}");
    }
    assert!(graph.accepts(&[]));
    assert!(graph.accepts(&['a']));
    assert!(graph.accepts(&['x']));
    assert!(graph.accepts(&['x', 'y']));
    assert!(!graph.accepts(&['y']));
    assert!(graph.transitions.iter().any(|edge| edge.guard.is_none()));
    assert!(graph.states.len() > 3); // constant word uses an intermediate state
}

#[test]
fn computed_output_needs_an_exact_oracle() {
    let (mut sft, input) = fixture();
    sft.transitions[0].output = OutputFunction::Map(Arc::new(|_: &char| 'x'));
    let mut machine = BoundedSftPostimage::new(
        sft,
        input,
        SameAlgebraPostimageOracle,
        SFT,
        SFA,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, ORACLE, |_| 0),
        Err(SftPostimageError::Oracle {
            transition_index: 0,
            error: PostimageOracleError::UnrepresentableOutput
        })
    ));
}

// The application knows these opaque closures are constants, unlike a
// generic symbolic oracle. Its separate binding identifies that assumption.
struct KnownComputedOracle;

impl ExactOutputPostimageOracle<CharClassAlgebra, CharClassAlgebra> for KnownComputedOracle {
    fn cases(
        &self,
        input_algebra: &CharClassAlgebra,
        output_algebra: &CharClassAlgebra,
        guard: &CharClassPred,
        output: &OutputFunction<CharClassAlgebra, CharClassAlgebra>,
    ) -> Result<Vec<PostimageCase<CharClassAlgebra, CharClassAlgebra>>, PostimageOracleError> {
        let basic = SameAlgebraPostimageOracle;
        match output {
            OutputFunction::Map(_) => basic.cases(
                input_algebra,
                output_algebra,
                guard,
                &OutputFunction::Constant(vec!['x']),
            ),
            OutputFunction::FlatMap(_) => basic.cases(
                input_algebra,
                output_algebra,
                guard,
                &OutputFunction::Constant(vec!['x', 'y']),
            ),
            other => basic.cases(input_algebra, output_algebra, guard, other),
        }
    }
}

#[test]
fn application_exact_oracle_admits_known_map_and_flatmap() {
    let (mut sft, input) = fixture();
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
    let mut machine = BoundedSftPostimage::new(
        sft.clone(),
        input.clone(),
        KnownComputedOracle,
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
    for output in words(&['a', 'b', 'x', 'y', 'z'], 2) {
        let expected = words(&['a', 'b', 'c'], 2)
            .iter()
            .any(|word| input.accepts(word) && source_outputs(&sft, word).contains(&output));
        assert_eq!(graph.accepts(&output), expected, "output {output:?}");
    }
}

#[test]
fn limits_cancellation_resume_and_cache_exclusion() {
    let mut baseline = machine(OperationLimits::default(), CancellationToken::new());
    let usage = match baseline.run(SFT, SFA, ORACLE, |_| 7).unwrap() {
        OperationOutcome::Complete { checkpoint, .. } => checkpoint.usage,
        _ => panic!("uncapped post-image must complete"),
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
            Err(SftPostimageError::Operation(
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
        let graph = machine
            .run(SFT, SFA, ORACLE, |_| 7)
            .unwrap()
            .into_complete()
            .unwrap();
        assert!(graph.accepts(&['x', 'y']));
    }
    let token = CancellationToken::new();
    token.cancel(CancellationReason::Requested);
    let mut cancelled = machine(OperationLimits::default(), token);
    assert!(matches!(
        cancelled.run(SFT, SFA, ORACLE, |_| 0).unwrap(),
        OperationOutcome::Incomplete {
            reason: IncompleteReason::Cancelled,
            ..
        }
    ));
    assert!(matches!(
        baseline.run([0; 32], SFA, ORACLE, |_| 0),
        Err(SftPostimageError::Operation(OperationError::StaleSource))
    ));
    let mut digest = blake3::Hasher::new();
    digest.update(b"lling.sft.exact-postimage-sources/v1\0");
    digest.update(&SFT);
    digest.update(&SFA);
    digest.update(&ORACLE);
    let plan = OperationPlan::new_dynamic(
        SourceSnapshot::IMMUTABLE,
        *digest.finalize().as_bytes(),
        "lling.sft.exact-postimage/v1",
    )
    .unwrap();
    let mut cache = CompleteResultCache::default();
    assert!(cache
        .insert(&plan, baseline.run(SFT, SFA, ORACLE, |_| 0).unwrap())
        .is_err());
}

#[test]
fn invalid_reachable_target_is_typed_and_atomic() {
    let (mut sft, input) = fixture();
    sft.transitions[0].to = 999;
    let mut machine = BoundedSftPostimage::new(
        sft,
        input,
        SameAlgebraPostimageOracle,
        SFT,
        SFA,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, ORACLE, |_| 0),
        Err(SftPostimageError::InvalidTarget {
            source: 1,
            transition_index: 0,
            target: 999
        })
    ));
}

#[test]
fn bounded_integer_algebras_require_matching_identity_semantics() {
    let input_algebra = IntervalAlgebra::new(0, 4);
    let output_algebra = IntervalAlgebra::new(0, 2);
    let mut sft = SymbolicFiniteTransducer::new(input_algebra.clone(), output_algebra);
    sft.add_state(false, None);
    sft.add_state(true, None);
    sft.set_initial(0);
    sft.add_transition(0, 1, IntervalPred::Range(1, 3), OutputFunction::Identity);
    let mut input = SymbolicAutomaton::new(input_algebra);
    input.add_state(false, None);
    input.add_state(true, None);
    input.set_initial(0);
    input.add_transition(0, 1, IntervalPred::Range(1, 3));
    let mut machine = BoundedSftPostimage::new(
        sft,
        input,
        SameAlgebraPostimageOracle,
        SFT,
        SFA,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        machine.run(SFT, SFA, ORACLE, |_| 0),
        Err(SftPostimageError::Oracle {
            error: PostimageOracleError::IncompatibleAlgebras,
            ..
        })
    ));
}
