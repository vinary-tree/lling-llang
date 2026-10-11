//! Owned native symbolic transducers and exact, bounded concrete execution.

use super::{
    char_value, checked_state, invalid, predicate, PredicateValue, LLING_SYMBOLIC_CHAR,
    LLING_SYMBOLIC_INTERVAL,
};
use crate::symbolic::bounded_compose::{BoundedSftComposition, SftCompositionLimits};
use crate::symbolic::bounded_transduce::{BoundedSftTransduction, SftTransductionLimits};
use crate::symbolic::sft::{OutputFunction, SymbolicFiniteTransducer};
use crate::symbolic::{BooleanAlgebra, CharClassAlgebra, IntervalAlgebra};
use crate::wfst::operation::{OperationLimits, OperationOutcome};
use crate::wfst::CancellationToken;
use std::sync::{Arc, Mutex};

use super::super::{boundary, bounded_usize, required_mut, set_error, LlingLlangStatus};

/// One transition emits nothing.
pub const LLING_SYMBOLIC_OUTPUT_EPSILON: u32 = 1;
/// One transition emits the consumed input scalar.
pub const LLING_SYMBOLIC_OUTPUT_IDENTITY: u32 = 2;
/// One transition emits a caller-provided constant word.
pub const LLING_SYMBOLIC_OUTPUT_CONSTANT: u32 = 3;

/// Explicit logical limits for one exact concrete-input transduction.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingSymbolicTransductionLimits {
    /// Size of this struct in bytes.
    pub struct_size: u32,
    /// Version 1 of the concrete transduction limit contract.
    pub version: u32,
    /// Maximum source-state visits.
    pub max_states: u64,
    /// Maximum source arcs admitted.
    pub max_arcs: u64,
    /// Maximum abstract work units.
    pub max_work: u64,
    /// Maximum caller-metered logical heap bytes.
    pub max_heap_bytes: u64,
    /// Maximum monotonic elapsed nanoseconds.
    pub max_elapsed_ns: u64,
    /// Maximum complete accepted paths.
    pub max_paths: u64,
    /// Maximum pending path frames.
    pub max_frontier: u64,
}

/// Owned native symbolic finite transducer over one exact scalar domain.
pub struct LlingSymbolicTransducer {
    value: TransducerValue,
    binding: Mutex<Option<[u8; 32]>>,
}

impl LlingSymbolicTransducer {
    fn binding(&self) -> [u8; 32] {
        let mut cached = self
            .binding
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *cached.get_or_insert_with(|| match &self.value {
            TransducerValue::Char(source) => source_binding(source),
            TransducerValue::Interval(source) => source_binding(source),
        })
    }

    fn invalidate_binding(&mut self) {
        *self
            .binding
            .get_mut()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }
}

enum TransducerValue {
    Char(Arc<SymbolicFiniteTransducer<CharClassAlgebra, CharClassAlgebra>>),
    Interval(Arc<SymbolicFiniteTransducer<IntervalAlgebra, IntervalAlgebra>>),
}

/// Exact output word for each accepting source path, in source order.
/// Different accepting paths may produce identical words.
pub struct LlingSymbolicTransduction {
    outputs: Vec<Vec<i64>>,
}

fn transducer_ref(
    raw: *const LlingSymbolicTransducer,
) -> Result<&'static LlingSymbolicTransducer, LlingLlangStatus> {
    if raw.is_null() {
        set_error("symbolic transducer is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the caller owns a live opaque transducer handle.
        Ok(unsafe { &*raw })
    }
}

fn transducer_mut(
    raw: *mut LlingSymbolicTransducer,
) -> Result<&'static mut LlingSymbolicTransducer, LlingLlangStatus> {
    if raw.is_null() {
        set_error("symbolic transducer is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the caller exclusively owns the transducer during mutation.
        Ok(unsafe { &mut *raw })
    }
}

fn transduction_ref(
    raw: *const LlingSymbolicTransduction,
) -> Result<&'static LlingSymbolicTransduction, LlingLlangStatus> {
    if raw.is_null() {
        set_error("symbolic transduction is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the caller owns a live immutable output handle.
        Ok(unsafe { &*raw })
    }
}

fn input_values<'a>(word: *const i64, length: usize) -> Result<&'a [i64], LlingLlangStatus> {
    if length > (isize::MAX as usize) / std::mem::size_of::<i64>() {
        set_error("symbolic input length exceeds the native slice limit");
        return Err(LlingLlangStatus::LimitExceeded);
    }
    if length == 0 {
        Ok(&[])
    } else if word.is_null() {
        set_error("symbolic input word is null");
        Err(LlingLlangStatus::NullPointer)
    } else {
        // SAFETY: the caller provides `length` readable i64 values.
        Ok(unsafe { std::slice::from_raw_parts(word, length) })
    }
}

fn check_input_heap_limit(values: &[i64], limits: OperationLimits) -> Result<(), LlingLlangStatus> {
    let bytes = u64::try_from(values.len())
        .ok()
        .and_then(|length| length.checked_mul(std::mem::size_of::<i64>() as u64))
        .ok_or_else(|| {
            set_error("symbolic input byte count exceeds u64");
            LlingLlangStatus::LimitExceeded
        })?;
    if bytes > limits.max_heap_bytes {
        set_error("symbolic input exceeds the logical heap limit");
        Err(LlingLlangStatus::LimitExceeded)
    } else {
        Ok(())
    }
}

fn limits_from_raw(
    raw: *const LlingSymbolicTransductionLimits,
) -> Result<(OperationLimits, SftTransductionLimits), LlingLlangStatus> {
    if raw.is_null() {
        set_error("symbolic transduction limits are null");
        return Err(LlingLlangStatus::NullPointer);
    }
    // SAFETY: the caller provides a readable configuration struct.
    let value = unsafe { &*raw };
    if value.struct_size as usize != std::mem::size_of::<LlingSymbolicTransductionLimits>()
        || value.version != 1
    {
        return Err(invalid(
            "symbolic transduction limits layout/version mismatch",
        ));
    }
    let common = OperationLimits {
        max_states: value.max_states,
        max_arcs: value.max_arcs,
        max_work: value.max_work,
        max_heap_bytes: value.max_heap_bytes,
        max_elapsed_ns: value.max_elapsed_ns,
    };
    let paths = SftTransductionLimits {
        max_paths: bounded_usize(value.max_paths, "max_paths")?,
        max_frontier: bounded_usize(value.max_frontier, "max_frontier")?,
    };
    Ok((common, paths))
}

fn exact_transduce<A, F>(
    source: &Arc<SymbolicFiniteTransducer<A, A>>,
    source_binding: [u8; 32],
    values: &[i64],
    input: Vec<A::Domain>,
    operation_limits: OperationLimits,
    path_limits: SftTransductionLimits,
    encode: F,
) -> Result<Vec<Vec<i64>>, LlingLlangStatus>
where
    A: BooleanAlgebra,
    A::Domain: Clone + Into<A::Domain>,
    F: Fn(&A::Domain) -> i64,
{
    let state_cap = bounded_usize(operation_limits.max_states, "max_states")?;
    let arc_cap = bounded_usize(operation_limits.max_arcs, "max_arcs")?;
    if source.num_states() > state_cap || source.num_transitions() > arc_cap {
        set_error("symbolic transduction source exceeds state or arc limit");
        return Err(LlingLlangStatus::LimitExceeded);
    }
    let input_binding = word_binding(values);
    let mut search = BoundedSftTransduction::new_shared(
        Arc::clone(source),
        input,
        source_binding,
        input_binding,
        operation_limits,
        path_limits,
        CancellationToken::new(),
    )
    .map_err(|error| invalid(format!("symbolic transduction plan: {error}")))?;
    match search
        .run(source_binding, input_binding, |_| 0, |_| 0)
        .map_err(|error| invalid(format!("symbolic transduction: {error}")))?
    {
        OperationOutcome::Complete { value, .. } => Ok(value
            .into_iter()
            .map(|witness| witness.output.iter().map(&encode).collect())
            .collect()),
        OperationOutcome::Incomplete { reason, .. } => {
            set_error(format!("symbolic transduction incomplete: {reason:?}"));
            Err(LlingLlangStatus::LimitExceeded)
        }
        OperationOutcome::Approximate { .. } => {
            set_error("symbolic transduction was not exact");
            Err(LlingLlangStatus::Unsupported)
        }
    }
}

fn source_binding<A: BooleanAlgebra>(source: &SymbolicFiniteTransducer<A, A>) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"lling.symbolic-ffi.transducer/v1\0");
    hash.update(format!("{:?}", source.input_algebra).as_bytes());
    hash.update(format!("{:?}", source.states).as_bytes());
    hash.update(format!("{:?}", source.transitions).as_bytes());
    let mut initials: Vec<_> = source.initial_states.iter().copied().collect();
    initials.sort_unstable();
    hash.update(format!("{initials:?}").as_bytes());
    *hash.finalize().as_bytes()
}

fn word_binding(values: &[i64]) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"lling.symbolic-ffi.word/v1\0");
    for value in values {
        hash.update(&value.to_le_bytes());
    }
    *hash.finalize().as_bytes()
}

fn exact_compose<A, F>(
    first: &Arc<SymbolicFiniteTransducer<A, A>>,
    second: &Arc<SymbolicFiniteTransducer<A, A>>,
    first_binding: [u8; 32],
    second_binding: [u8; 32],
    values: &[i64],
    input: Vec<A::Domain>,
    operation_limits: OperationLimits,
    path_limits: SftTransductionLimits,
    encode: F,
) -> Result<Vec<Vec<i64>>, LlingLlangStatus>
where
    A: BooleanAlgebra,
    A::Domain: Clone + Into<A::Domain>,
    F: Fn(&A::Domain) -> i64,
{
    let state_cap = bounded_usize(operation_limits.max_states, "max_states")?;
    let arc_cap = bounded_usize(operation_limits.max_arcs, "max_arcs")?;
    if first.num_states() > state_cap
        || second.num_states() > state_cap
        || first.num_transitions() > arc_cap
        || second.num_transitions() > arc_cap
    {
        set_error("symbolic composition source exceeds state or arc limit");
        return Err(LlingLlangStatus::LimitExceeded);
    }
    let input_binding = word_binding(values);
    let mut search = BoundedSftComposition::new_shared(
        Arc::clone(first),
        Arc::clone(second),
        input,
        first_binding,
        second_binding,
        input_binding,
        operation_limits,
        SftCompositionLimits {
            max_paths: path_limits.max_paths,
            max_frontier: path_limits.max_frontier,
        },
        CancellationToken::new(),
    )
    .map_err(|error| invalid(format!("symbolic composition plan: {error}")))?;
    match search
        .run(
            first_binding,
            second_binding,
            input_binding,
            |_| 0,
            |_| 0,
            |_| 0,
        )
        .map_err(|error| invalid(format!("symbolic composition: {error}")))?
    {
        OperationOutcome::Complete { value, .. } => Ok(value
            .into_iter()
            .map(|witness| witness.output.iter().map(&encode).collect())
            .collect()),
        OperationOutcome::Incomplete { reason, .. } => {
            set_error(format!("symbolic composition incomplete: {reason:?}"));
            Err(LlingLlangStatus::LimitExceeded)
        }
        OperationOutcome::Approximate { .. } => {
            set_error("symbolic composition was not exact");
            Err(LlingLlangStatus::Unsupported)
        }
    }
}

/// Construct an owned native symbolic transducer over one scalar domain.
///
/// # Safety
/// `out_transducer` must be writable and receives an owned handle.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transducer_new(
    domain: u32,
    universe_min: i64,
    universe_max: i64,
    out_transducer: *mut *mut LlingSymbolicTransducer,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_transducer, "out_transducer")?;
        let value = match domain {
            LLING_SYMBOLIC_CHAR => {
                if universe_min != 0 || universe_max != 0 {
                    return Err(invalid("Unicode algebra has no configurable universe"));
                }
                TransducerValue::Char(Arc::new(SymbolicFiniteTransducer::new(
                    CharClassAlgebra::new(),
                    CharClassAlgebra::new(),
                )))
            }
            LLING_SYMBOLIC_INTERVAL => {
                if universe_min >= universe_max {
                    return Err(invalid("integer universe must be nonempty"));
                }
                let algebra = IntervalAlgebra::new(universe_min, universe_max);
                TransducerValue::Interval(Arc::new(SymbolicFiniteTransducer::new(
                    algebra.clone(),
                    algebra,
                )))
            }
            _ => return Err(invalid("unknown symbolic domain")),
        };
        *output = Box::into_raw(Box::new(LlingSymbolicTransducer {
            value,
            binding: Mutex::new(None),
        }));
        Ok(())
    })
}

/// Add one accepting or nonaccepting state and return its zero-based ID.
///
/// # Safety
/// The handle must be live and exclusively owned; `out_state` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transducer_add_state(
    transducer: *mut LlingSymbolicTransducer,
    accepting: u8,
    out_state: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        if accepting > 1 {
            return Err(invalid("accepting must be zero or one"));
        }
        let output = required_mut(out_state, "out_state")?;
        let handle = transducer_mut(transducer)?;
        let id = match &mut handle.value {
            TransducerValue::Char(t) => Arc::make_mut(t).add_state(accepting == 1, None),
            TransducerValue::Interval(t) => Arc::make_mut(t).add_state(accepting == 1, None),
        };
        handle.invalidate_binding();
        *output = u64::try_from(id).map_err(|_| invalid("state ID exceeds u64"))?;
        Ok(())
    })
}

/// Mark a previously added state as initial.
///
/// # Safety
/// The handle must be live and exclusively owned during mutation.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transducer_set_initial(
    transducer: *mut LlingSymbolicTransducer,
    state: u64,
) -> LlingLlangStatus {
    boundary(|| {
        let handle = transducer_mut(transducer)?;
        match &mut handle.value {
            TransducerValue::Char(t) => {
                let state = checked_state(state, t.num_states())?;
                Arc::make_mut(t).set_initial(state);
            }
            TransducerValue::Interval(t) => {
                let state = checked_state(state, t.num_states())?;
                Arc::make_mut(t).set_initial(state);
            }
        }
        handle.invalidate_binding();
        Ok(())
    })
}

/// Add a guard with epsilon, identity, or constant output.
/// A constant output may contain zero or more values. The guard and output
/// are copied into the transducer and remain caller-owned.
///
/// # Safety
/// The transducer is exclusively owned, the guard is live, and `output_values`
/// points to `output_length` readable values or is null for zero length.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transducer_add_transition(
    transducer: *mut LlingSymbolicTransducer,
    from: u64,
    to: u64,
    guard: *const super::LlingSymbolicPredicate,
    output_kind: u32,
    output_values: *const i64,
    output_length: usize,
) -> LlingLlangStatus {
    boundary(|| {
        if !matches!(
            output_kind,
            LLING_SYMBOLIC_OUTPUT_EPSILON..=LLING_SYMBOLIC_OUTPUT_CONSTANT
        ) {
            return Err(invalid("unknown symbolic output kind"));
        }
        if output_kind != LLING_SYMBOLIC_OUTPUT_CONSTANT && output_length != 0 {
            return Err(invalid(
                "epsilon and identity outputs cannot carry constant values",
            ));
        }
        let values = input_values(output_values, output_length)?;
        let guard = &predicate(guard)?.value;
        let handle = transducer_mut(transducer)?;
        match (&mut handle.value, guard) {
            (TransducerValue::Char(t), PredicateValue::Char(g)) => {
                let from = checked_state(from, t.num_states())?;
                let to = checked_state(to, t.num_states())?;
                let output = match output_kind {
                    LLING_SYMBOLIC_OUTPUT_EPSILON => OutputFunction::Epsilon,
                    LLING_SYMBOLIC_OUTPUT_IDENTITY => OutputFunction::Identity,
                    _ => OutputFunction::Constant(
                        values
                            .iter()
                            .copied()
                            .map(char_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                };
                Arc::make_mut(t).add_transition(from, to, g.clone(), output);
            }
            (TransducerValue::Interval(t), PredicateValue::Interval { algebra, predicate })
                if t.input_algebra.min_val == algebra.min_val
                    && t.input_algebra.max_val == algebra.max_val =>
            {
                let from = checked_state(from, t.num_states())?;
                let to = checked_state(to, t.num_states())?;
                if values
                    .iter()
                    .any(|value| *value < algebra.min_val || *value >= algebra.max_val)
                {
                    return Err(invalid("symbolic constant output is outside its universe"));
                }
                let output = match output_kind {
                    LLING_SYMBOLIC_OUTPUT_EPSILON => OutputFunction::Epsilon,
                    LLING_SYMBOLIC_OUTPUT_IDENTITY => OutputFunction::Identity,
                    _ => OutputFunction::Constant(values.to_vec()),
                };
                Arc::make_mut(t).add_transition(from, to, predicate.clone(), output);
            }
            _ => return Err(invalid("symbolic guard domain or universe differs")),
        }
        handle.invalidate_binding();
        Ok(())
    })
}

/// Enumerate exact native output words for one concrete input under explicit
/// logical limits. An incomplete search returns LIMIT_EXCEEDED with no result.
///
/// # Safety
/// All handles must be live, limits and `out_result` writable/readable as
/// appropriate, and `word` points to `length` readable values or is null for
/// zero length.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transducer_transduce(
    transducer: *const LlingSymbolicTransducer,
    word: *const i64,
    length: usize,
    limits: *const LlingSymbolicTransductionLimits,
    out_result: *mut *mut LlingSymbolicTransduction,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_result, "out_result")?;
        *output = std::ptr::null_mut();
        let values = input_values(word, length)?;
        let (operation_limits, path_limits) = limits_from_raw(limits)?;
        check_input_heap_limit(values, operation_limits)?;
        let transducer = transducer_ref(transducer)?;
        let source_binding = transducer.binding();
        let outputs = match &transducer.value {
            TransducerValue::Char(t) => {
                let chars = values
                    .iter()
                    .copied()
                    .map(char_value)
                    .collect::<Result<Vec<_>, _>>()?;
                exact_transduce(
                    t,
                    source_binding,
                    values,
                    chars,
                    operation_limits,
                    path_limits,
                    |c| *c as i64,
                )?
            }
            TransducerValue::Interval(t) => exact_transduce(
                t,
                source_binding,
                values,
                values.to_vec(),
                operation_limits,
                path_limits,
                |v| *v,
            )?,
        };
        *output = Box::into_raw(Box::new(LlingSymbolicTransduction { outputs }));
        Ok(())
    })
}

/// Enumerate exact outputs from `second(first(word))` with one shared budget.
/// No intermediate output or approximate result is published on interruption.
///
/// # Safety
/// Both transducers must be live. `word`, `limits`, and `out_result` follow the
/// same contracts as [`lling_symbolic_transducer_transduce`].
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transducer_compose_transduce(
    first: *const LlingSymbolicTransducer,
    second: *const LlingSymbolicTransducer,
    word: *const i64,
    length: usize,
    limits: *const LlingSymbolicTransductionLimits,
    out_result: *mut *mut LlingSymbolicTransduction,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_result, "out_result")?;
        *output = std::ptr::null_mut();
        let values = input_values(word, length)?;
        let (operation_limits, path_limits) = limits_from_raw(limits)?;
        check_input_heap_limit(values, operation_limits)?;
        let first = transducer_ref(first)?;
        let second = transducer_ref(second)?;
        let first_binding = first.binding();
        let second_binding = second.binding();
        let first = &first.value;
        let second = &second.value;
        let outputs = match (first, second) {
            (TransducerValue::Char(a), TransducerValue::Char(b)) => {
                let chars = values
                    .iter()
                    .copied()
                    .map(char_value)
                    .collect::<Result<Vec<_>, _>>()?;
                exact_compose(
                    a,
                    b,
                    first_binding,
                    second_binding,
                    values,
                    chars,
                    operation_limits,
                    path_limits,
                    |c| *c as i64,
                )?
            }
            (TransducerValue::Interval(a), TransducerValue::Interval(b))
                if a.input_algebra.min_val == b.input_algebra.min_val
                    && a.input_algebra.max_val == b.input_algebra.max_val =>
            {
                exact_compose(
                    a,
                    b,
                    first_binding,
                    second_binding,
                    values,
                    values.to_vec(),
                    operation_limits,
                    path_limits,
                    |v| *v,
                )?
            }
            _ => return Err(invalid("symbolic composition domains or universes differ")),
        };
        *output = Box::into_raw(Box::new(LlingSymbolicTransduction { outputs }));
        Ok(())
    })
}

/// Return the number of exact accepted paths in the result.
///
/// # Safety
/// The result must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transduction_count(
    result: *const LlingSymbolicTransduction,
    out_count: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_count, "out_count")?;
        *output = u64::try_from(transduction_ref(result)?.outputs.len())
            .map_err(|_| invalid("output count exceeds u64"))?;
        Ok(())
    })
}

/// Copy one exact output word. `out_required` receives the exact value count;
/// `out_values` may be null only when capacity is zero.
///
/// # Safety
/// The result must be live; `out_required` writable; `out_values` points to
/// capacity writable i64 values when nonnull.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transduction_output(
    result: *const LlingSymbolicTransduction,
    index: u64,
    out_values: *mut i64,
    capacity: usize,
    out_required: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let required = required_mut(out_required, "out_required")?;
        let result = transduction_ref(result)?;
        let index =
            usize::try_from(index).map_err(|_| invalid("output index exceeds host size"))?;
        let values = result
            .outputs
            .get(index)
            .ok_or_else(|| invalid("symbolic output index is out of range"))?;
        *required = values.len();
        if capacity == 0 && out_values.is_null() {
            return Ok(());
        }
        if capacity < values.len() {
            return Err(invalid("symbolic output buffer is too small"));
        }
        if !values.is_empty() {
            if out_values.is_null() {
                set_error("symbolic output buffer is null");
                return Err(LlingLlangStatus::NullPointer);
            }
            // SAFETY: capacity covers the complete output word.
            unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), out_values, values.len()) };
        }
        Ok(())
    })
}

/// Release an owned native symbolic transducer.
///
/// # Safety
/// Input must be null or an unfreed handle returned by this module.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transducer_free(input: *mut LlingSymbolicTransducer) {
    if !input.is_null() {
        drop(Box::from_raw(input));
    }
}

/// Release an owned exact transduction result.
///
/// # Safety
/// Input must be null or an unfreed handle returned by this module.
#[no_mangle]
pub unsafe extern "C" fn lling_symbolic_transduction_free(input: *mut LlingSymbolicTransduction) {
    if !input.is_null() {
        drop(Box::from_raw(input));
    }
}
