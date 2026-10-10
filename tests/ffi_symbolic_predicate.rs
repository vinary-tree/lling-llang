#![cfg(feature = "ffi")]

use lling_llang::ffi::{
    lling_symbolic_char_range, lling_symbolic_interval_range, lling_symbolic_predicate_binary,
    lling_symbolic_predicate_constant, lling_symbolic_predicate_evaluate,
    lling_symbolic_predicate_free, lling_symbolic_predicate_not, lling_symbolic_predicate_relation,
    lling_symbolic_predicate_witness, LlingLlangStatus, LlingSymbolicPredicate,
    LLING_SYMBOLIC_CHAR, LLING_SYMBOLIC_INTERVAL,
};
use lling_llang::symbolic::{
    BooleanAlgebra, CharClassAlgebra, CharClassPred, IntervalAlgebra, IntervalPred,
};

struct Predicate(*mut LlingSymbolicPredicate);

impl Predicate {
    fn output(call: impl FnOnce(*mut *mut LlingSymbolicPredicate) -> LlingLlangStatus) -> Self {
        let mut value = std::ptr::null_mut();
        assert_eq!(call(&mut value), LlingLlangStatus::Ok);
        assert!(!value.is_null());
        Self(value)
    }

    fn evaluate(&self, input: i64) -> bool {
        let mut result = 0;
        assert_eq!(
            unsafe { lling_symbolic_predicate_evaluate(self.0, input, &mut result) },
            LlingLlangStatus::Ok
        );
        result == 1
    }

    fn witness(&self) -> Option<i64> {
        let mut satisfiable = 0;
        let mut witness = 0;
        assert_eq!(
            unsafe { lling_symbolic_predicate_witness(self.0, &mut satisfiable, &mut witness) },
            LlingLlangStatus::Ok
        );
        (satisfiable == 1).then_some(witness)
    }
}

impl Drop for Predicate {
    fn drop(&mut self) {
        unsafe { lling_symbolic_predicate_free(self.0) }
    }
}

#[test]
fn char_predicates_agree_with_native_boolean_algebra() {
    let algebra = CharClassAlgebra::new();
    let latin =
        Predicate::output(|out| unsafe { lling_symbolic_char_range('a' as u32, 'z' as u32, out) });
    let vowels =
        Predicate::output(|out| unsafe { lling_symbolic_char_range('a' as u32, 'e' as u32, out) });
    let overlap = Predicate::output(|out| unsafe {
        lling_symbolic_predicate_binary(1, latin.0, vowels.0, out)
    });
    let outside = Predicate::output(|out| unsafe { lling_symbolic_predicate_not(overlap.0, out) });
    let native = algebra.not(&algebra.and(
        &CharClassPred::Range('a', 'z'),
        &CharClassPred::Range('a', 'e'),
    ));
    for value in ['\0', 'A', 'a', 'e', 'f', 'z', '😀'] {
        assert_eq!(
            outside.evaluate(value as i64),
            algebra.evaluate(&native, &value)
        );
    }
    let witness = overlap
        .witness()
        .expect("overlapping ranges have a witness");
    assert!(overlap.evaluate(witness));
    let mut related = 0;
    assert_eq!(
        unsafe { lling_symbolic_predicate_relation(1, vowels.0, latin.0, &mut related) },
        LlingLlangStatus::Ok
    );
    assert_eq!(related, 1);
    assert_eq!(
        unsafe { lling_symbolic_predicate_relation(3, outside.0, overlap.0, &mut related) },
        LlingLlangStatus::Ok
    );
    assert_eq!(related, 0);
}

#[test]
fn unicode_complement_excludes_surrogates_without_losing_upper_scalars() {
    let algebra = CharClassAlgebra::new();
    let below_gap = CharClassPred::Range('\0', '\u{d7ff}');
    let above_gap = CharClassPred::Range('\u{e000}', char::MAX);
    let direct_complement = CharClassPred::Not(Box::new(below_gap.clone()));
    assert!(algebra.is_satisfiable(&direct_complement));
    assert_eq!(algebra.witness(&direct_complement), Some('\u{e000}'));
    assert!(algebra.equivalent(&algebra.not(&below_gap), &above_gap));
    let full_scalars = algebra.or(&below_gap, &above_gap);
    assert!(!algebra.is_satisfiable(&algebra.not(&full_scalars)));

    let low =
        Predicate::output(|out| unsafe { lling_symbolic_char_range(0, '\u{d7ff}' as u32, out) });
    let native_complement =
        Predicate::output(|out| unsafe { lling_symbolic_predicate_not(low.0, out) });
    assert_eq!(native_complement.witness(), Some('\u{e000}' as i64));
    assert!(!native_complement.evaluate('\u{d7ff}' as i64));
    assert!(native_complement.evaluate(char::MAX as i64));
}

#[test]
fn bounded_integer_predicates_preserve_universe_and_native_witnesses() {
    let algebra = IntervalAlgebra::new(-20, 20);
    let first =
        Predicate::output(|out| unsafe { lling_symbolic_interval_range(-20, 20, -4, 8, out) });
    let second =
        Predicate::output(|out| unsafe { lling_symbolic_interval_range(-20, 20, 5, 15, out) });
    let union = Predicate::output(|out| unsafe {
        lling_symbolic_predicate_binary(2, first.0, second.0, out)
    });
    let native = algebra.or(&IntervalPred::Range(-4, 8), &IntervalPred::Range(5, 15));
    for value in -22..22 {
        assert_eq!(union.evaluate(value), algebra.evaluate(&native, &value));
    }
    assert_eq!(union.witness(), algebra.witness(&native));
    let false_pred = Predicate::output(|out| unsafe {
        lling_symbolic_predicate_constant(LLING_SYMBOLIC_INTERVAL, -20, 20, 0, out)
    });
    assert_eq!(false_pred.witness(), None);
}

#[test]
fn symbolic_abi_rejects_invalid_scalars_mismatched_universes_and_null_outputs() {
    let mut output = std::ptr::null_mut();
    assert_eq!(
        unsafe { lling_symbolic_char_range(0xd800, 0xe000, &mut output) },
        LlingLlangStatus::InvalidArgument
    );
    assert!(output.is_null());
    assert_eq!(
        unsafe { lling_symbolic_interval_range(0, 10, 9, 11, &mut output) },
        LlingLlangStatus::InvalidArgument
    );
    assert_eq!(
        unsafe { lling_symbolic_predicate_constant(LLING_SYMBOLIC_CHAR, 1, 0, 1, &mut output) },
        LlingLlangStatus::InvalidArgument
    );
    let char_pred =
        Predicate::output(|out| unsafe { lling_symbolic_char_range('a' as u32, 'z' as u32, out) });
    let interval =
        Predicate::output(|out| unsafe { lling_symbolic_interval_range(0, 10, 1, 5, out) });
    assert_eq!(
        unsafe { lling_symbolic_predicate_binary(1, char_pred.0, interval.0, &mut output) },
        LlingLlangStatus::InvalidArgument
    );
    assert!(output.is_null());
    assert_eq!(
        unsafe { lling_symbolic_predicate_evaluate(char_pred.0, 0xd800, std::ptr::null_mut()) },
        LlingLlangStatus::NullPointer
    );
    let mut result = 0;
    assert_eq!(
        unsafe { lling_symbolic_predicate_evaluate(char_pred.0, 0xd800, &mut result) },
        LlingLlangStatus::InvalidArgument
    );
}
