//! Typed, owned C handles for the native effective Boolean algebras.

use super::{boundary, required_mut, set_error, LlingLlangStatus};
use crate::symbolic::{
    BooleanAlgebra, CharClassAlgebra, CharClassPred, IntervalAlgebra, IntervalPred,
    SymbolicAutomaton,
};

mod transducer;
pub use transducer::*;

/// Unicode scalar predicates use kind 1; bounded integer predicates use kind 2.
pub const LLING_SYMBOLIC_CHAR: u32 = 1;
/// Bounded signed integer domain.
pub const LLING_SYMBOLIC_INTERVAL: u32 = 2;

/// Native predicate with an exact algebra interpretation attached.
pub struct LlingSymbolicPredicate {
    value: PredicateValue,
}

enum PredicateValue {
    Char(CharClassPred),
    Interval {
        algebra: IntervalAlgebra,
        predicate: IntervalPred,
    },
}

/// Owned symbolic automaton over one exact native Boolean algebra.
pub struct LlingSymbolicAutomaton {
    value: AutomatonValue,
}

enum AutomatonValue {
    Char(SymbolicAutomaton<CharClassAlgebra>),
    Interval(SymbolicAutomaton<IntervalAlgebra>),
}

fn automaton_ref(
    raw: *const LlingSymbolicAutomaton,
) -> Result<&'static LlingSymbolicAutomaton, LlingLlangStatus> {
    if raw.is_null() {
        set_error("symbolic automaton is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the caller owns a live opaque automaton handle.
        Ok(unsafe { &*raw })
    }
}

fn automaton_mut(
    raw: *mut LlingSymbolicAutomaton,
) -> Result<&'static mut LlingSymbolicAutomaton, LlingLlangStatus> {
    if raw.is_null() {
        set_error("symbolic automaton is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the caller owns the handle exclusively while mutating it.
        Ok(unsafe { &mut *raw })
    }
}

fn checked_state(state: u64, count: usize) -> Result<usize, LlingLlangStatus> {
    let state = usize::try_from(state).map_err(|_| invalid("state ID exceeds host size"))?;
    if state >= count {
        Err(invalid("symbolic state ID is out of range"))
    } else {
        Ok(state)
    }
}

fn invalid(message: impl Into<String>) -> LlingLlangStatus {
    set_error(message);
    LlingLlangStatus::InvalidArgument
}

fn predicate(
    raw: *const LlingSymbolicPredicate,
) -> Result<&'static LlingSymbolicPredicate, LlingLlangStatus> {
    if raw.is_null() {
        set_error("symbolic predicate is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the caller owns a live opaque predicate handle.
        Ok(unsafe { &*raw })
    }
}

fn publish(
    out: *mut *mut LlingSymbolicPredicate,
    value: PredicateValue,
) -> Result<(), LlingLlangStatus> {
    let out = required_mut(out, "out_predicate")?;
    *out = Box::into_raw(Box::new(LlingSymbolicPredicate { value }));
    Ok(())
}

fn matching<'a>(
    first: &'a PredicateValue,
    second: &'a PredicateValue,
) -> Result<MatchedPredicates<'a>, LlingLlangStatus> {
    match (first, second) {
        (PredicateValue::Char(a), PredicateValue::Char(b)) => Ok(MatchedPredicates::Char(a, b)),
        (
            PredicateValue::Interval {
                algebra: aa,
                predicate: a,
            },
            PredicateValue::Interval {
                algebra: bb,
                predicate: b,
            },
        ) if aa.min_val == bb.min_val && aa.max_val == bb.max_val => {
            Ok(MatchedPredicates::Interval(aa, a, b))
        }
        _ => Err(invalid("symbolic predicate domains or universes differ")),
    }
}

enum MatchedPredicates<'a> {
    Char(&'a CharClassPred, &'a CharClassPred),
    Interval(&'a IntervalAlgebra, &'a IntervalPred, &'a IntervalPred),
}

fn char_value(value: i64) -> Result<char, LlingLlangStatus> {
    u32::try_from(value)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| invalid("symbolic Unicode value is not a scalar"))
}

/// Construct true or false in the requested exact native algebra.
///
/// # Safety
/// `out_predicate` must be writable; the caller owns the returned handle.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_predicate_constant(
    domain: u32,
    universe_min: i64,
    universe_max: i64,
    truth: u8,
    out_predicate: *mut *mut LlingSymbolicPredicate,
) -> LlingLlangStatus {
    boundary(|| {
        if truth > 1 {
            return Err(invalid("symbolic truth must be zero or one"));
        }
        let value = match domain {
            LLING_SYMBOLIC_CHAR => {
                if universe_min != 0 || universe_max != 0 {
                    return Err(invalid("Unicode algebra has no configurable universe"));
                }
                let algebra = CharClassAlgebra::new();
                PredicateValue::Char(if truth == 1 {
                    algebra.true_pred()
                } else {
                    algebra.false_pred()
                })
            }
            LLING_SYMBOLIC_INTERVAL => {
                if universe_min >= universe_max {
                    return Err(invalid("integer universe must be nonempty"));
                }
                let algebra = IntervalAlgebra::new(universe_min, universe_max);
                let predicate = if truth == 1 {
                    algebra.true_pred()
                } else {
                    algebra.false_pred()
                };
                PredicateValue::Interval { algebra, predicate }
            }
            _ => return Err(invalid("unknown symbolic domain")),
        };
        publish(out_predicate, value)
    })
}

/// Construct a half-open signed integer range in a bounded universe.
///
/// # Safety
/// `out_predicate` must be writable; the caller owns the returned handle.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_interval_range(
    universe_min: i64,
    universe_max: i64,
    lo: i64,
    hi_exclusive: i64,
    out_predicate: *mut *mut LlingSymbolicPredicate,
) -> LlingLlangStatus {
    boundary(|| {
        if universe_min >= universe_max
            || lo >= hi_exclusive
            || lo < universe_min
            || hi_exclusive > universe_max
        {
            return Err(invalid(
                "interval range must lie inside a nonempty universe",
            ));
        }
        publish(
            out_predicate,
            PredicateValue::Interval {
                algebra: IntervalAlgebra::new(universe_min, universe_max),
                predicate: IntervalPred::Range(lo, hi_exclusive),
            },
        )
    })
}

/// Construct an inclusive range over Unicode scalar values.
///
/// # Safety
/// `out_predicate` must be writable; the caller owns the returned handle.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_char_range(
    lo: u32,
    hi_inclusive: u32,
    out_predicate: *mut *mut LlingSymbolicPredicate,
) -> LlingLlangStatus {
    boundary(|| {
        let lo = char::from_u32(lo).ok_or_else(|| invalid("invalid lower Unicode scalar"))?;
        let hi =
            char::from_u32(hi_inclusive).ok_or_else(|| invalid("invalid upper Unicode scalar"))?;
        if lo > hi {
            return Err(invalid("Unicode range endpoints are reversed"));
        }
        publish(
            out_predicate,
            PredicateValue::Char(CharClassPred::Range(lo, hi)),
        )
    })
}

/// Apply native conjunction (operation 1) or disjunction (operation 2).
///
/// # Safety
/// Inputs must be live handles; `out_predicate` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_predicate_binary(
    operation: u32,
    first: *const LlingSymbolicPredicate,
    second: *const LlingSymbolicPredicate,
    out_predicate: *mut *mut LlingSymbolicPredicate,
) -> LlingLlangStatus {
    boundary(|| {
        if !matches!(operation, 1 | 2) {
            return Err(invalid("unknown symbolic Boolean operation"));
        }
        let value = match matching(&predicate(first)?.value, &predicate(second)?.value)? {
            MatchedPredicates::Char(a, b) => {
                let algebra = CharClassAlgebra::new();
                PredicateValue::Char(if operation == 1 {
                    algebra.and(a, b)
                } else {
                    algebra.or(a, b)
                })
            }
            MatchedPredicates::Interval(algebra, a, b) => PredicateValue::Interval {
                algebra: algebra.clone(),
                predicate: if operation == 1 {
                    algebra.and(a, b)
                } else {
                    algebra.or(a, b)
                },
            },
        };
        publish(out_predicate, value)
    })
}

/// Apply native Boolean complement within the predicate's universe.
///
/// # Safety
/// Input must be live; `out_predicate` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_predicate_not(
    input: *const LlingSymbolicPredicate,
    out_predicate: *mut *mut LlingSymbolicPredicate,
) -> LlingLlangStatus {
    boundary(|| {
        let value = match &predicate(input)?.value {
            PredicateValue::Char(value) => PredicateValue::Char(CharClassAlgebra::new().not(value)),
            PredicateValue::Interval { algebra, predicate } => PredicateValue::Interval {
                algebra: algebra.clone(),
                predicate: algebra.not(predicate),
            },
        };
        publish(out_predicate, value)
    })
}

/// Test one concrete scalar against a native predicate.
///
/// # Safety
/// Input must be live; `out_matches` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_predicate_evaluate(
    input: *const LlingSymbolicPredicate,
    value: i64,
    out_matches: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_matches, "out_matches")?;
        *output = match &predicate(input)?.value {
            PredicateValue::Char(p) => CharClassAlgebra::new().evaluate(p, &char_value(value)?),
            PredicateValue::Interval { algebra, predicate } => algebra.evaluate(predicate, &value),
        } as u8;
        Ok(())
    })
}

/// Decide satisfiability and return one concrete witness when present.
///
/// # Safety
/// Input must be live; both output pointers must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_predicate_witness(
    input: *const LlingSymbolicPredicate,
    out_satisfiable: *mut u8,
    out_witness: *mut i64,
) -> LlingLlangStatus {
    boundary(|| {
        let satisfiable = required_mut(out_satisfiable, "out_satisfiable")?;
        let witness = required_mut(out_witness, "out_witness")?;
        let value = match &predicate(input)?.value {
            PredicateValue::Char(p) => CharClassAlgebra::new().witness(p).map(|c| c as i64),
            PredicateValue::Interval { algebra, predicate } => algebra.witness(predicate),
        };
        *satisfiable = u8::from(value.is_some());
        *witness = value.unwrap_or_default();
        Ok(())
    })
}

/// Decide implication (1), equivalence (2), or overlap (3).
///
/// # Safety
/// Inputs must be live; `out_result` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_predicate_relation(
    relation: u32,
    first: *const LlingSymbolicPredicate,
    second: *const LlingSymbolicPredicate,
    out_result: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        if !matches!(relation, 1..=3) {
            return Err(invalid("unknown symbolic predicate relation"));
        }
        let output = required_mut(out_result, "out_result")?;
        let result = match matching(&predicate(first)?.value, &predicate(second)?.value)? {
            MatchedPredicates::Char(a, b) => {
                let algebra = CharClassAlgebra::new();
                match relation {
                    1 => algebra.implies(a, b),
                    2 => algebra.equivalent(a, b),
                    _ => algebra.overlaps(a, b),
                }
            }
            MatchedPredicates::Interval(algebra, a, b) => match relation {
                1 => algebra.implies(a, b),
                2 => algebra.equivalent(a, b),
                _ => algebra.overlaps(a, b),
            },
        };
        *output = u8::from(result);
        Ok(())
    })
}

/// Release one owned native symbolic predicate.
///
/// # Safety
/// Input must be null or an unfreed handle returned by this module.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_predicate_free(input: *mut LlingSymbolicPredicate) {
    if !input.is_null() {
        drop(Box::from_raw(input));
    }
}

/// Construct a native symbolic finite automaton over one exact algebra.
///
/// # Safety
/// `out_automaton` must be writable and receives an owned handle.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_automaton_new(
    domain: u32,
    universe_min: i64,
    universe_max: i64,
    out_automaton: *mut *mut LlingSymbolicAutomaton,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_automaton, "out_automaton")?;
        let value = match domain {
            LLING_SYMBOLIC_CHAR => {
                if universe_min != 0 || universe_max != 0 {
                    return Err(invalid("Unicode algebra has no configurable universe"));
                }
                AutomatonValue::Char(SymbolicAutomaton::new(CharClassAlgebra::new()))
            }
            LLING_SYMBOLIC_INTERVAL => {
                if universe_min >= universe_max {
                    return Err(invalid("integer universe must be nonempty"));
                }
                AutomatonValue::Interval(SymbolicAutomaton::new(IntervalAlgebra::new(
                    universe_min,
                    universe_max,
                )))
            }
            _ => return Err(invalid("unknown symbolic domain")),
        };
        *output = Box::into_raw(Box::new(LlingSymbolicAutomaton { value }));
        Ok(())
    })
}

/// Append one state; state IDs are zero based and stable for this handle.
///
/// # Safety
/// The handle must be live and exclusively owned during mutation; `out_state`
/// must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_automaton_add_state(
    automaton: *mut LlingSymbolicAutomaton,
    accepting: u8,
    out_state: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        if accepting > 1 {
            return Err(invalid("accepting must be zero or one"));
        }
        let output = required_mut(out_state, "out_state")?;
        let value = &mut automaton_mut(automaton)?.value;
        let id = match value {
            AutomatonValue::Char(a) => a.add_state(accepting == 1, None),
            AutomatonValue::Interval(a) => a.add_state(accepting == 1, None),
        };
        *output = u64::try_from(id).map_err(|_| invalid("state ID exceeds u64"))?;
        Ok(())
    })
}

/// Mark a previously added state as initial.
///
/// # Safety
/// The handle must be live and exclusively owned during mutation.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_automaton_set_initial(
    automaton: *mut LlingSymbolicAutomaton,
    state: u64,
) -> LlingLlangStatus {
    boundary(|| {
        let value = &mut automaton_mut(automaton)?.value;
        match value {
            AutomatonValue::Char(a) => {
                a.set_initial(checked_state(state, a.num_states())?);
            }
            AutomatonValue::Interval(a) => {
                a.set_initial(checked_state(state, a.num_states())?);
            }
        }
        Ok(())
    })
}

/// Append a guarded transition using an owned predicate from the same domain.
/// The predicate is copied into the native automaton and remains caller-owned.
///
/// # Safety
/// Both handles must be live; the automaton is exclusively owned during mutation.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_automaton_add_transition(
    automaton: *mut LlingSymbolicAutomaton,
    from: u64,
    to: u64,
    guard: *const LlingSymbolicPredicate,
) -> LlingLlangStatus {
    boundary(|| {
        let guard = &predicate(guard)?.value;
        let value = &mut automaton_mut(automaton)?.value;
        match (value, guard) {
            (AutomatonValue::Char(a), PredicateValue::Char(g)) => {
                let from = checked_state(from, a.num_states())?;
                let to = checked_state(to, a.num_states())?;
                a.add_transition(from, to, g.clone());
            }
            (AutomatonValue::Interval(a), PredicateValue::Interval { algebra, predicate })
                if a.algebra.min_val == algebra.min_val && a.algebra.max_val == algebra.max_val =>
            {
                let from = checked_state(from, a.num_states())?;
                let to = checked_state(to, a.num_states())?;
                a.add_transition(from, to, predicate.clone());
            }
            _ => return Err(invalid("symbolic guard domain or universe differs")),
        }
        Ok(())
    })
}

/// Decide whether the native automaton accepts one concrete input word.
///
/// # Safety
/// The handle must be live; `word` points to `length` readable i64 values,
/// or is null when `length` is zero. `out_accepted` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_automaton_accepts(
    automaton: *const LlingSymbolicAutomaton,
    word: *const i64,
    length: usize,
    out_accepted: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_accepted, "out_accepted")?;
        if length > (isize::MAX as usize) / std::mem::size_of::<i64>() {
            set_error("symbolic word length exceeds the native slice limit");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let values = if length == 0 {
            &[][..]
        } else if word.is_null() {
            set_error("symbolic word is null");
            return Err(LlingLlangStatus::NullPointer);
        } else {
            // SAFETY: the caller provides `length` readable i64 values.
            unsafe { std::slice::from_raw_parts(word, length) }
        };
        *output = match &automaton_ref(automaton)?.value {
            AutomatonValue::Char(a) => {
                let chars = values
                    .iter()
                    .copied()
                    .map(char_value)
                    .collect::<Result<Vec<_>, _>>()?;
                a.accepts(&chars)
            }
            AutomatonValue::Interval(a) => a.accepts(values),
        } as u8;
        Ok(())
    })
}

/// Decide whether any concrete word is accepted.
///
/// # Safety
/// The handle must be live and `out_empty` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_automaton_is_empty(
    automaton: *const LlingSymbolicAutomaton,
    out_empty: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_empty, "out_empty")?;
        *output = match &automaton_ref(automaton)?.value {
            AutomatonValue::Char(a) => a.is_empty(),
            AutomatonValue::Interval(a) => a.is_empty(),
        } as u8;
        Ok(())
    })
}

/// Release one owned native symbolic automaton.
///
/// # Safety
/// Input must be null or an unfreed handle returned by this module.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_automaton_free(input: *mut LlingSymbolicAutomaton) {
    if !input.is_null() {
        drop(Box::from_raw(input));
    }
}
