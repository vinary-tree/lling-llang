#![cfg(feature = "ffi")]

use lling_llang::ffi::{
    lling_symbolic_automaton_accepts, lling_symbolic_automaton_add_state,
    lling_symbolic_automaton_add_transition, lling_symbolic_automaton_free,
    lling_symbolic_automaton_is_empty, lling_symbolic_automaton_new,
    lling_symbolic_automaton_set_initial, lling_symbolic_char_range, lling_symbolic_interval_range,
    lling_symbolic_predicate_free, LlingLlangStatus, LlingSymbolicAutomaton,
    LlingSymbolicPredicate, LLING_SYMBOLIC_CHAR, LLING_SYMBOLIC_INTERVAL,
};
use lling_llang::symbolic::{
    CharClassAlgebra, CharClassPred, IntervalAlgebra, IntervalPred, SymbolicAutomaton,
};

struct Automaton(*mut LlingSymbolicAutomaton);

impl Automaton {
    fn new(domain: u32, lo: i64, hi: i64) -> Self {
        let mut value = std::ptr::null_mut();
        assert_eq!(
            unsafe { lling_symbolic_automaton_new(domain, lo, hi, &mut value) },
            LlingLlangStatus::Ok
        );
        Self(value)
    }

    fn add_state(&self, accepting: bool) -> u64 {
        let mut state = u64::MAX;
        assert_eq!(
            unsafe { lling_symbolic_automaton_add_state(self.0, accepting as u8, &mut state) },
            LlingLlangStatus::Ok
        );
        state
    }

    fn accepts(&self, word: &[i64]) -> bool {
        let mut accepted = 2;
        assert_eq!(
            unsafe {
                lling_symbolic_automaton_accepts(self.0, word.as_ptr(), word.len(), &mut accepted)
            },
            LlingLlangStatus::Ok
        );
        accepted == 1
    }

    fn is_empty(&self) -> bool {
        let mut empty = 2;
        assert_eq!(
            unsafe { lling_symbolic_automaton_is_empty(self.0, &mut empty) },
            LlingLlangStatus::Ok
        );
        empty == 1
    }
}

impl Drop for Automaton {
    fn drop(&mut self) {
        unsafe { lling_symbolic_automaton_free(self.0) }
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

#[test]
fn unicode_automaton_matches_native_sfa_and_copies_guard() {
    let ffi = Automaton::new(LLING_SYMBOLIC_CHAR, 0, 0);
    let mut rust = SymbolicAutomaton::new(CharClassAlgebra::new());
    let states = [(false, true), (true, false)];
    for (accepting, initial) in states {
        let ffi_state = ffi.add_state(accepting);
        let rust_state = rust.add_state(accepting, None);
        assert_eq!(ffi_state as usize, rust_state);
        if initial {
            assert_eq!(
                unsafe { lling_symbolic_automaton_set_initial(ffi.0, ffi_state) },
                LlingLlangStatus::Ok
            );
            rust.set_initial(rust_state);
        }
    }
    assert!(ffi.is_empty());
    let guard = Predicate::chars('a', 'z');
    assert_eq!(
        unsafe { lling_symbolic_automaton_add_transition(ffi.0, 0, 1, guard.0) },
        LlingLlangStatus::Ok
    );
    rust.add_transition(0, 1, CharClassPred::Range('a', 'z'));
    drop(guard);
    assert_eq!(ffi.is_empty(), rust.is_empty());
    for word in ["", "a", "z", "A", "aa", "😀"] {
        let values: Vec<i64> = word.chars().map(|c| c as i64).collect();
        assert_eq!(
            ffi.accepts(&values),
            rust.accepts(&word.chars().collect::<Vec<_>>())
        );
    }
    let mut result = 0;
    assert_eq!(
        unsafe { lling_symbolic_automaton_accepts(ffi.0, std::ptr::null(), 0, &mut result) },
        LlingLlangStatus::Ok
    );
    assert_eq!(result, 0);
}

#[test]
fn bounded_integer_automaton_rejects_invalid_edges_and_universes() {
    let ffi = Automaton::new(LLING_SYMBOLIC_INTERVAL, -10, 10);
    let mut rust = SymbolicAutomaton::new(IntervalAlgebra::new(-10, 10));
    let s0 = ffi.add_state(false);
    let s1 = ffi.add_state(true);
    assert_eq!(s0, rust.add_state(false, None) as u64);
    assert_eq!(s1, rust.add_state(true, None) as u64);
    assert_eq!(
        unsafe { lling_symbolic_automaton_set_initial(ffi.0, 2) },
        LlingLlangStatus::InvalidArgument
    );
    assert_eq!(
        unsafe { lling_symbolic_automaton_set_initial(ffi.0, s0) },
        LlingLlangStatus::Ok
    );
    rust.set_initial(0);
    let guard = Predicate::integers((-10, 10), (1, 5));
    let mismatch = Predicate::integers((0, 10), (1, 5));
    assert_eq!(
        unsafe { lling_symbolic_automaton_add_transition(ffi.0, 0, 1, mismatch.0) },
        LlingLlangStatus::InvalidArgument
    );
    assert_eq!(
        unsafe { lling_symbolic_automaton_add_transition(ffi.0, 0, 2, guard.0) },
        LlingLlangStatus::InvalidArgument
    );
    assert!(ffi.is_empty());
    assert_eq!(
        unsafe { lling_symbolic_automaton_add_transition(ffi.0, 0, 1, guard.0) },
        LlingLlangStatus::Ok
    );
    rust.add_transition(0, 1, IntervalPred::Range(1, 5));
    for word in [vec![], vec![0], vec![1], vec![4], vec![5], vec![1, 2]] {
        assert_eq!(ffi.accepts(&word), rust.accepts(&word));
    }
    let mut accepted = 2;
    assert_eq!(
        unsafe { lling_symbolic_automaton_accepts(ffi.0, std::ptr::null(), 1, &mut accepted) },
        LlingLlangStatus::NullPointer
    );
}
