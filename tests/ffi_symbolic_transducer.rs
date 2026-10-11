#![cfg(feature = "ffi")]

use lling_llang::ffi::{
    lling_symbolic_char_range, lling_symbolic_interval_range, lling_symbolic_predicate_free,
    lling_symbolic_transducer_add_state, lling_symbolic_transducer_add_transition,
    lling_symbolic_transducer_compose_transduce, lling_symbolic_transducer_free,
    lling_symbolic_transducer_new, lling_symbolic_transducer_set_initial,
    lling_symbolic_transducer_transduce, lling_symbolic_transduction_count,
    lling_symbolic_transduction_free, lling_symbolic_transduction_output, LlingLlangStatus,
    LlingSymbolicPredicate, LlingSymbolicTransducer, LlingSymbolicTransduction,
    LlingSymbolicTransductionLimits, LLING_SYMBOLIC_CHAR, LLING_SYMBOLIC_INTERVAL,
    LLING_SYMBOLIC_OUTPUT_CONSTANT, LLING_SYMBOLIC_OUTPUT_EPSILON, LLING_SYMBOLIC_OUTPUT_IDENTITY,
};
use lling_llang::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use lling_llang::symbolic::{CharClassAlgebra, CharClassPred, IntervalAlgebra, IntervalPred};

struct Transducer(*mut LlingSymbolicTransducer);

impl Transducer {
    fn new(domain: u32, lo: i64, hi: i64) -> Self {
        let mut value = std::ptr::null_mut();
        assert_eq!(
            unsafe { lling_symbolic_transducer_new(domain, lo, hi, &mut value) },
            LlingLlangStatus::Ok
        );
        Self(value)
    }

    fn state(&self, accepting: bool) -> u64 {
        let mut id = u64::MAX;
        assert_eq!(
            unsafe { lling_symbolic_transducer_add_state(self.0, accepting as u8, &mut id) },
            LlingLlangStatus::Ok
        );
        id
    }

    fn initial(&self, id: u64) {
        assert_eq!(
            unsafe { lling_symbolic_transducer_set_initial(self.0, id) },
            LlingLlangStatus::Ok
        );
    }

    fn edge(&self, from: u64, to: u64, guard: &Predicate, kind: u32, output: &[i64]) {
        assert_eq!(
            unsafe {
                lling_symbolic_transducer_add_transition(
                    self.0,
                    from,
                    to,
                    guard.0,
                    kind,
                    output.as_ptr(),
                    output.len(),
                )
            },
            LlingLlangStatus::Ok
        );
    }

    fn run(&self, input: &[i64], limits: &LlingSymbolicTransductionLimits) -> Vec<Vec<i64>> {
        let mut raw = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                lling_symbolic_transducer_transduce(
                    self.0,
                    input.as_ptr(),
                    input.len(),
                    limits,
                    &mut raw,
                )
            },
            LlingLlangStatus::Ok
        );
        let result = Transduction(raw);
        result.outputs()
    }
}

impl Drop for Transducer {
    fn drop(&mut self) {
        unsafe { lling_symbolic_transducer_free(self.0) }
    }
}

struct Predicate(*mut LlingSymbolicPredicate);

impl Predicate {
    fn chars(lo: char, hi: char) -> Self {
        let mut value = std::ptr::null_mut();
        assert_eq!(
            unsafe { lling_symbolic_char_range(lo as u32, hi as u32, &mut value) },
            LlingLlangStatus::Ok
        );
        Self(value)
    }

    fn integers(universe: (i64, i64), range: (i64, i64)) -> Self {
        let mut value = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                lling_symbolic_interval_range(universe.0, universe.1, range.0, range.1, &mut value)
            },
            LlingLlangStatus::Ok
        );
        Self(value)
    }
}

impl Drop for Predicate {
    fn drop(&mut self) {
        unsafe { lling_symbolic_predicate_free(self.0) }
    }
}

struct Transduction(*mut LlingSymbolicTransduction);

impl Transduction {
    fn outputs(&self) -> Vec<Vec<i64>> {
        let mut count = 0;
        assert_eq!(
            unsafe { lling_symbolic_transduction_count(self.0, &mut count) },
            LlingLlangStatus::Ok
        );
        (0..count)
            .map(|index| {
                let mut required = 0;
                assert_eq!(
                    unsafe {
                        lling_symbolic_transduction_output(
                            self.0,
                            index,
                            std::ptr::null_mut(),
                            0,
                            &mut required,
                        )
                    },
                    LlingLlangStatus::Ok
                );
                let mut output = vec![0; required];
                assert_eq!(
                    unsafe {
                        lling_symbolic_transduction_output(
                            self.0,
                            index,
                            output.as_mut_ptr(),
                            output.len(),
                            &mut required,
                        )
                    },
                    LlingLlangStatus::Ok
                );
                output
            })
            .collect()
    }
}

impl Drop for Transduction {
    fn drop(&mut self) {
        unsafe { lling_symbolic_transduction_free(self.0) }
    }
}

fn limits() -> LlingSymbolicTransductionLimits {
    LlingSymbolicTransductionLimits {
        struct_size: std::mem::size_of::<LlingSymbolicTransductionLimits>() as u32,
        version: 1,
        max_states: 100,
        max_arcs: 100,
        max_work: 10_000,
        max_heap_bytes: 1_000_000,
        max_elapsed_ns: 5_000_000_000,
        max_paths: 100,
        max_frontier: 100,
    }
}

#[test]
fn char_outputs_match_rust_sft_for_constant_identity_and_epsilon() {
    let ffi = Transducer::new(LLING_SYMBOLIC_CHAR, 0, 0);
    let mut rust = SymbolicFiniteTransducer::new(CharClassAlgebra::new(), CharClassAlgebra::new());
    assert_eq!(ffi.state(false), rust.add_state(false, None) as u64);
    assert_eq!(ffi.state(true), rust.add_state(true, None) as u64);
    ffi.initial(0);
    rust.set_initial(0);
    let guard = Predicate::chars('a', 'z');
    ffi.edge(0, 1, &guard, LLING_SYMBOLIC_OUTPUT_IDENTITY, &[]);
    rust.add_transition(
        0,
        1,
        CharClassPred::Range('a', 'z'),
        OutputFunction::Identity,
    );
    ffi.edge(
        0,
        1,
        &guard,
        LLING_SYMBOLIC_OUTPUT_CONSTANT,
        &['X' as i64, 'Y' as i64],
    );
    rust.add_transition(
        0,
        1,
        CharClassPred::Range('a', 'z'),
        OutputFunction::Constant(vec!['X', 'Y']),
    );
    ffi.edge(0, 1, &guard, LLING_SYMBOLIC_OUTPUT_EPSILON, &[]);
    rust.add_transition(
        0,
        1,
        CharClassPred::Range('a', 'z'),
        OutputFunction::Epsilon,
    );
    drop(guard);
    for input in ["", "a", "z", "A", "aa"] {
        let mut got = ffi.run(
            &input.chars().map(|c| c as i64).collect::<Vec<_>>(),
            &limits(),
        );
        let mut expected: Vec<Vec<i64>> = rust
            .transduce(&input.chars().collect::<Vec<_>>())
            .into_iter()
            .map(|word| word.into_iter().map(|c| c as i64).collect())
            .collect();
        got.sort();
        expected.sort();
        assert_eq!(got, expected);
    }
}

#[test]
fn integer_outputs_match_native_sft_and_incomplete_is_not_published() {
    let ffi = Transducer::new(LLING_SYMBOLIC_INTERVAL, -10, 10);
    let mut rust =
        SymbolicFiniteTransducer::new(IntervalAlgebra::new(-10, 10), IntervalAlgebra::new(-10, 10));
    ffi.state(false);
    ffi.state(true);
    rust.add_state(false, None);
    rust.add_state(true, None);
    ffi.initial(0);
    rust.set_initial(0);
    let guard = Predicate::integers((-10, 10), (2, 6));
    ffi.edge(0, 1, &guard, LLING_SYMBOLIC_OUTPUT_CONSTANT, &[4, 5]);
    rust.add_transition(
        0,
        1,
        IntervalPred::Range(2, 6),
        OutputFunction::Constant(vec![4, 5]),
    );
    assert_eq!(ffi.run(&[3], &limits()), rust.transduce(&[3]));
    assert_eq!(ffi.run(&[6], &limits()), rust.transduce(&[6]));

    ffi.edge(0, 1, &guard, LLING_SYMBOLIC_OUTPUT_IDENTITY, &[]);
    rust.add_transition(0, 1, IntervalPred::Range(2, 6), OutputFunction::Identity);
    let mut after_mutation = ffi.run(&[3], &limits());
    let mut expected_after_mutation = rust.transduce(&[3]);
    after_mutation.sort();
    expected_after_mutation.sort();
    assert_eq!(after_mutation, expected_after_mutation);

    let mut strict = limits();
    strict.max_paths = 0;
    let mut output = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            lling_symbolic_transducer_transduce(ffi.0, [3].as_ptr(), 1, &strict, &mut output)
        },
        LlingLlangStatus::LimitExceeded
    );
    assert!(output.is_null());

    let mut no_input_heap = limits();
    no_input_heap.max_heap_bytes = 0;
    assert_eq!(
        unsafe {
            lling_symbolic_transducer_transduce(ffi.0, [3].as_ptr(), 1, &no_input_heap, &mut output)
        },
        LlingLlangStatus::LimitExceeded
    );
    assert!(output.is_null());

    let mismatch = Predicate::integers((0, 10), (2, 6));
    assert_eq!(
        unsafe {
            lling_symbolic_transducer_add_transition(
                ffi.0,
                0,
                1,
                mismatch.0,
                LLING_SYMBOLIC_OUTPUT_EPSILON,
                std::ptr::null(),
                0,
            )
        },
        LlingLlangStatus::InvalidArgument
    );
    assert_eq!(
        unsafe {
            lling_symbolic_transducer_add_transition(
                ffi.0,
                0,
                1,
                guard.0,
                LLING_SYMBOLIC_OUTPUT_IDENTITY,
                [1].as_ptr(),
                1,
            )
        },
        LlingLlangStatus::InvalidArgument
    );
}

#[test]
fn bounded_composition_is_exact_and_rejects_partial_results() {
    let first = Transducer::new(LLING_SYMBOLIC_CHAR, 0, 0);
    let second = Transducer::new(LLING_SYMBOLIC_CHAR, 0, 0);
    for machine in [&first, &second] {
        machine.state(false);
        machine.state(true);
        machine.initial(0);
    }
    let input_guard = Predicate::chars('a', 'z');
    let middle_guard = Predicate::chars('b', 'b');
    first.edge(
        0,
        1,
        &input_guard,
        LLING_SYMBOLIC_OUTPUT_CONSTANT,
        &['b' as i64],
    );
    second.edge(
        0,
        1,
        &middle_guard,
        LLING_SYMBOLIC_OUTPUT_CONSTANT,
        &['Q' as i64],
    );

    let mut result = std::ptr::null_mut();
    let input = ['a' as i64];
    assert_eq!(
        unsafe {
            lling_symbolic_transducer_compose_transduce(
                first.0,
                second.0,
                input.as_ptr(),
                input.len(),
                &limits(),
                &mut result,
            )
        },
        LlingLlangStatus::Ok
    );
    let outputs = Transduction(result).outputs();
    assert_eq!(outputs, vec![vec!['Q' as i64]]);

    let mut strict = limits();
    strict.max_work = 0;
    let mut incomplete = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            lling_symbolic_transducer_compose_transduce(
                first.0,
                second.0,
                input.as_ptr(),
                input.len(),
                &strict,
                &mut incomplete,
            )
        },
        LlingLlangStatus::LimitExceeded
    );
    assert!(incomplete.is_null());

    let mismatch = Transducer::new(LLING_SYMBOLIC_INTERVAL, -10, 10);
    assert_eq!(
        unsafe {
            lling_symbolic_transducer_compose_transduce(
                first.0,
                mismatch.0,
                input.as_ptr(),
                input.len(),
                &limits(),
                &mut incomplete,
            )
        },
        LlingLlangStatus::InvalidArgument
    );
    assert!(incomplete.is_null());
}
