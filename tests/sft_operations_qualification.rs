//! Cross-operation SFT language laws, resource slopes and small-stack checks.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lling_llang::symbolic::bounded_compose::{BoundedSftComposition, SftCompositionLimits};
use lling_llang::symbolic::bounded_postimage::{BoundedSftPostimage, SameAlgebraPostimageOracle};
use lling_llang::symbolic::bounded_preimage::{
    BoundedSftPreimage, PreimageOracleError, SameAlgebraPreimageOracle, SftPreimageError,
};
use lling_llang::symbolic::bounded_restrict::BoundedSftRestriction;
use lling_llang::symbolic::bounded_transduce::{BoundedSftTransduction, SftTransductionLimits};
use lling_llang::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use lling_llang::symbolic::{
    BooleanAlgebra, CharClassAlgebra, CharClassPred, IntervalAlgebra, IntervalPred,
    SymbolicAutomaton,
};
use lling_llang::wfst::operation::{OperationLimits, OperationOutcome, OperationUsage};
use lling_llang::wfst::CancellationToken;

type Sft = SymbolicFiniteTransducer<CharClassAlgebra, CharClassAlgebra>;
type Sfa = SymbolicAutomaton<CharClassAlgebra>;
type QualificationSample = (Vec<OperationUsage>, usize);
const FIRST: [u8; 32] = [161; 32];
const SECOND: [u8; 32] = [162; 32];
const INPUT: [u8; 32] = [163; 32];
const ACCEPTOR: [u8; 32] = [164; 32];
const ORACLE: [u8; 32] = [165; 32];

fn fixture() -> (Sft, Sft, Sfa, Sfa) {
    let algebra = CharClassAlgebra::new();
    let mut first = Sft::new(algebra.clone(), algebra.clone());
    first.add_state(false, None);
    first.add_state(true, None);
    first.set_initial(0);
    first.add_transition(
        0,
        1,
        CharClassPred::Range('a', 'a'),
        OutputFunction::Identity,
    );
    first.add_transition(
        0,
        1,
        CharClassPred::Range('a', 'a'),
        OutputFunction::Constant(vec!['x', 'y']),
    );
    first.add_transition(
        0,
        1,
        CharClassPred::Range('b', 'b'),
        OutputFunction::Epsilon,
    );
    first.add_transition(
        0,
        1,
        CharClassPred::Range('b', 'b'),
        OutputFunction::Constant(vec!['x']),
    );
    first.add_transition(
        1,
        1,
        CharClassPred::Range('c', 'c'),
        OutputFunction::Identity,
    );
    first.add_transition(
        1,
        1,
        CharClassPred::Range('c', 'c'),
        OutputFunction::Epsilon,
    );
    first.add_transition(
        0,
        1,
        CharClassPred::False,
        OutputFunction::Constant(vec!['z']),
    );

    let mut second = Sft::new(algebra.clone(), algebra.clone());
    second.add_state(true, None);
    second.set_initial(0);
    second.add_transition(0, 0, CharClassPred::True, OutputFunction::Identity);

    let mut input = Sfa::new(algebra.clone());
    input.add_state(false, None);
    input.add_state(true, None);
    input.add_state(true, None);
    input.set_initial(0);
    input.add_transition(0, 1, CharClassPred::Range('a', 'b'));
    input.add_transition(1, 2, CharClassPred::Range('c', 'c'));

    let mut acceptor = Sfa::new(algebra);
    acceptor.add_state(true, None); // epsilon
    acceptor.add_state(false, None);
    acceptor.add_state(true, None);
    acceptor.set_initial(0);
    acceptor.add_transition(0, 2, CharClassPred::Range('a', 'a'));
    acceptor.add_transition(0, 1, CharClassPred::Range('x', 'x'));
    acceptor.add_transition(1, 2, CharClassPred::Range('y', 'y'));
    (first, second, input, acceptor)
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

fn source_outputs(sft: &Sft, input: &[char]) -> Vec<Vec<char>> {
    fn visit(
        sft: &Sft,
        input: &[char],
        state: usize,
        pos: usize,
        output: &[char],
        all: &mut Vec<Vec<char>>,
    ) {
        if pos == input.len() {
            if sft.accepting_states.contains(&state) {
                all.push(output.to_vec());
            }
            return;
        }
        for transition in &sft.transitions {
            if transition.from == state
                && sft.input_algebra.evaluate(&transition.guard, &input[pos])
            {
                let mut next = output.to_vec();
                next.extend(transition.output.apply(&input[pos]));
                visit(sft, input, transition.to, pos + 1, &next, all);
            }
        }
    }
    let mut all = Vec::new();
    for &initial in &sft.initial_states {
        visit(sft, input, initial, 0, &[], &mut all);
    }
    all.sort();
    all.dedup();
    all
}

#[test]
fn shared_composition_retains_both_sources_without_graph_copies() {
    let (first, second, _, _) = fixture();
    let first = Arc::new(first);
    let second = Arc::new(second);
    let mut search = BoundedSftComposition::new_shared(
        Arc::clone(&first),
        Arc::clone(&second),
        vec!['a'],
        FIRST,
        SECOND,
        INPUT,
        OperationLimits::default(),
        SftCompositionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(Arc::strong_count(&first), 2);
    assert_eq!(Arc::strong_count(&second), 2);
    assert!(matches!(
        search
            .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
            .unwrap(),
        OperationOutcome::Complete { .. }
    ));
    drop(search);
    assert_eq!(Arc::strong_count(&first), 1);
    assert_eq!(Arc::strong_count(&second), 1);
}

#[test]
fn five_operation_laws_match_an_independent_shallow_relation() {
    let (first, second, input, acceptor) = fixture();
    let mut preimage = BoundedSftPreimage::new(
        first.clone(),
        acceptor.clone(),
        SameAlgebraPreimageOracle,
        FIRST,
        ACCEPTOR,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let preimage = preimage
        .run(FIRST, ACCEPTOR, ORACLE, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    let mut postimage = BoundedSftPostimage::new(
        first.clone(),
        input.clone(),
        SameAlgebraPostimageOracle,
        FIRST,
        INPUT,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let postimage = postimage
        .run(FIRST, INPUT, ORACLE, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();
    let mut restriction = BoundedSftRestriction::new(
        first.clone(),
        input.clone(),
        FIRST,
        INPUT,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let restriction = restriction
        .run(FIRST, INPUT, |_| 0, |_| 0)
        .unwrap()
        .into_complete()
        .unwrap();

    let inputs = words(&['a', 'b', 'c'], 2);
    for word in &inputs {
        let expected = source_outputs(&first, word);
        let mut transduction = BoundedSftTransduction::new(
            first.clone(),
            word.clone(),
            FIRST,
            INPUT,
            OperationLimits::default(),
            SftTransductionLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
        let mut observed = transduction
            .run(FIRST, INPUT, |_| 0, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .into_iter()
            .map(|path| path.output)
            .collect::<Vec<_>>();
        observed.sort();
        observed.dedup();
        assert_eq!(observed, expected, "transduction {word:?}");

        let mut composition = BoundedSftComposition::new(
            first.clone(),
            second.clone(),
            word.clone(),
            FIRST,
            SECOND,
            INPUT,
            OperationLimits::default(),
            SftCompositionLimits::default(),
            CancellationToken::new(),
        )
        .unwrap();
        let mut composed = composition
            .run(FIRST, SECOND, INPUT, |_| 0, |_| 0, |_| 0)
            .unwrap()
            .into_complete()
            .unwrap()
            .into_iter()
            .map(|path| path.output)
            .collect::<Vec<_>>();
        composed.sort();
        composed.dedup();
        assert_eq!(composed, expected, "identity composition {word:?}");

        let accepted_output = expected.iter().any(|output| acceptor.accepts(output));
        assert_eq!(
            preimage.accepts(word),
            accepted_output,
            "pre-image {word:?}"
        );
        let mut restricted = restriction.transduce(word);
        restricted.sort();
        restricted.dedup();
        assert_eq!(
            restricted,
            if input.accepts(word) {
                expected
            } else {
                Vec::new()
            },
            "restriction {word:?}"
        );
    }
    for output in words(&['a', 'b', 'c', 'x', 'y'], 3) {
        let expected = inputs
            .iter()
            .any(|word| input.accepts(word) && source_outputs(&first, word).contains(&output));
        assert_eq!(
            postimage.accepts(&output),
            expected,
            "post-image {output:?}"
        );
    }
}

#[test]
fn identity_pullback_refuses_an_incompatible_true_predicate() {
    let source_algebra = IntervalAlgebra::new(0, 4);
    let mut sft = SymbolicFiniteTransducer::new(source_algebra.clone(), source_algebra);
    sft.add_state(false, None);
    sft.add_state(true, None);
    sft.set_initial(0);
    sft.add_transition(0, 1, IntervalPred::Range(1, 3), OutputFunction::Identity);
    let mut acceptor = SymbolicAutomaton::new(IntervalAlgebra::new(0, 2));
    acceptor.add_state(false, None);
    acceptor.add_state(true, None);
    acceptor.set_initial(0);
    acceptor.add_transition(0, 1, IntervalPred::True);
    let mut preimage = BoundedSftPreimage::new(
        sft,
        acceptor,
        SameAlgebraPreimageOracle,
        FIRST,
        ACCEPTOR,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(matches!(
        preimage.run(FIRST, ACCEPTOR, ORACLE, |_| 0),
        Err(SftPreimageError::Oracle {
            error: PreimageOracleError::IncompatibleAlgebras,
            ..
        })
    ));
}

fn chain(n: usize) -> (Sft, Sfa, Sfa) {
    let algebra = CharClassAlgebra::new();
    let mut sft = Sft::new(algebra.clone(), algebra.clone());
    let mut input = Sfa::new(algebra.clone());
    let mut output = Sfa::new(algebra);
    for index in 0..=n {
        sft.add_state(index == n, None);
        input.add_state(index == n, None);
        output.add_state(index == n, None);
    }
    sft.set_initial(0);
    input.set_initial(0);
    output.set_initial(0);
    for index in 0..n {
        sft.add_transition(
            index,
            index + 1,
            CharClassPred::Range('a', 'a'),
            OutputFunction::Identity,
        );
        input.add_transition(index, index + 1, CharClassPred::Range('a', 'a'));
        output.add_transition(index, index + 1, CharClassPred::Range('a', 'a'));
    }
    (sft, input, output)
}

fn wide(n: usize) -> (Sft, Sfa, Sfa) {
    let algebra = CharClassAlgebra::new();
    let mut sft = Sft::new(algebra.clone(), algebra.clone());
    let mut input = Sfa::new(algebra.clone());
    let mut output = Sfa::new(algebra);
    sft.add_state(false, None);
    for _ in 0..n {
        let target = sft.add_state(true, None);
        sft.add_transition(
            0,
            target,
            CharClassPred::Range('a', 'a'),
            OutputFunction::Identity,
        );
    }
    input.add_state(false, None);
    input.add_state(true, None);
    input.add_transition(0, 1, CharClassPred::Range('a', 'a'));
    output.add_state(false, None);
    output.add_state(true, None);
    output.add_transition(0, 1, CharClassPred::Range('a', 'a'));
    sft.set_initial(0);
    input.set_initial(0);
    output.set_initial(0);
    (sft, input, output)
}

fn observe_stack(low: &AtomicUsize, high: &AtomicUsize) -> u64 {
    let local = 0_u8;
    let address = (&raw const local).addr();
    low.fetch_min(address, Ordering::Relaxed);
    high.fetch_max(address, Ordering::Relaxed);
    0
}

fn qualify_case(
    sft: Sft,
    input: Sfa,
    output: Sfa,
    word: Vec<char>,
    expected_paths: usize,
    expected_states: usize,
) -> (Vec<OperationUsage>, usize) {
    let low = AtomicUsize::new(usize::MAX);
    let high = AtomicUsize::new(0);
    let mut transduction = BoundedSftTransduction::new(
        sft.clone(),
        word.clone(),
        FIRST,
        INPUT,
        OperationLimits::default(),
        SftTransductionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let transduction_usage = match transduction
        .run(FIRST, INPUT, |_| observe_stack(&low, &high), |_| 0)
        .unwrap()
    {
        OperationOutcome::Complete { value, checkpoint } => {
            assert_eq!(value.len(), expected_paths);
            assert!(value.iter().all(|path| path.output == word));
            checkpoint.usage
        }
        _ => panic!("uncapped transduction"),
    };
    let mut identity = Sft::new(CharClassAlgebra::new(), CharClassAlgebra::new());
    identity.add_state(true, None);
    identity.set_initial(0);
    identity.add_transition(0, 0, CharClassPred::True, OutputFunction::Identity);
    let mut composition = BoundedSftComposition::new(
        sft.clone(),
        identity,
        word.clone(),
        FIRST,
        SECOND,
        INPUT,
        OperationLimits::default(),
        SftCompositionLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let composition_usage = match composition
        .run(
            FIRST,
            SECOND,
            INPUT,
            |_| observe_stack(&low, &high),
            |_| 0,
            |_| 0,
        )
        .unwrap()
    {
        OperationOutcome::Complete { value, checkpoint } => {
            assert_eq!(value.len(), expected_paths);
            assert!(value.iter().all(|path| path.output == word));
            checkpoint.usage
        }
        _ => panic!("uncapped composition"),
    };
    let mut preimage = BoundedSftPreimage::new(
        sft.clone(),
        output,
        SameAlgebraPreimageOracle,
        FIRST,
        ACCEPTOR,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let preimage_usage = match preimage
        .run(FIRST, ACCEPTOR, ORACLE, |_| observe_stack(&low, &high))
        .unwrap()
    {
        OperationOutcome::Complete { value, checkpoint } => {
            assert_eq!(value.states.len(), expected_states);
            checkpoint.usage
        }
        _ => panic!("uncapped pre-image"),
    };
    let mut postimage = BoundedSftPostimage::new(
        sft.clone(),
        input.clone(),
        SameAlgebraPostimageOracle,
        FIRST,
        INPUT,
        ORACLE,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let postimage_usage = match postimage
        .run(FIRST, INPUT, ORACLE, |_| observe_stack(&low, &high))
        .unwrap()
    {
        OperationOutcome::Complete { value, checkpoint } => {
            assert_eq!(value.states.len(), expected_states);
            checkpoint.usage
        }
        _ => panic!("uncapped post-image"),
    };
    let mut restriction = BoundedSftRestriction::new(
        sft,
        input,
        FIRST,
        INPUT,
        OperationLimits::default(),
        CancellationToken::new(),
    )
    .unwrap();
    let restriction_usage = match restriction
        .run(FIRST, INPUT, |_| observe_stack(&low, &high), |_| 0)
        .unwrap()
    {
        OperationOutcome::Complete { value, checkpoint } => {
            assert_eq!(value.states.len(), expected_states);
            checkpoint.usage
        }
        _ => panic!("uncapped restriction"),
    };
    (
        vec![
            transduction_usage,
            composition_usage,
            preimage_usage,
            postimage_usage,
            restriction_usage,
        ],
        high.load(Ordering::Relaxed)
            .saturating_sub(low.load(Ordering::Relaxed)),
    )
}

fn qualified_chain(n: usize) -> (Vec<OperationUsage>, usize) {
    let (sft, input, output) = chain(n);
    qualify_case(sft, input, output, vec!['a'; n], 1, n + 1)
}

fn qualified_wide(n: usize) -> (Vec<OperationUsage>, usize) {
    let (sft, input, output) = wide(n);
    qualify_case(sft, input, output, vec!['a'], n, n + 1)
}

#[test]
fn all_five_deep_and_wide_operations_have_linear_charges_and_flat_sampled_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            for (shape, qualify) in [
                ("deep", qualified_chain as fn(usize) -> QualificationSample),
                ("wide", qualified_wide as fn(usize) -> QualificationSample),
            ] {
                let (small, small_span) = qualify(128);
                let (large, large_span) = qualify(512);
                for (name, a, b) in [
                    "transduction",
                    "composition",
                    "pre-image",
                    "post-image",
                    "restriction",
                ]
                .into_iter()
                .zip(small)
                .zip(large)
                .map(|((name, a), b)| (name, a, b))
                {
                    assert!(b.work > a.work, "{shape}/{name}: work must increase");
                    assert!(
                        b.heap_bytes > a.heap_bytes,
                        "{shape}/{name}: heap must increase"
                    );
                    assert!(
                        b.work <= a.work * 5,
                        "{shape}/{name}: work slope {} -> {}",
                        a.work,
                        b.work
                    );
                    assert!(
                        b.heap_bytes <= a.heap_bytes * 5,
                        "{shape}/{name}: heap slope {} -> {}",
                        a.heap_bytes,
                        b.heap_bytes
                    );
                }
                assert!(
                    small_span < 16 * 1024,
                    "{shape} small sampled stack span {small_span}"
                );
                assert!(
                    large_span < 16 * 1024,
                    "{shape} large sampled stack span {large_span}"
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
