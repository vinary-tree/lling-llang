//! Reusable, snapshot-owned bridge to the native subsequential runner.

use super::{
    boundary, bounded_usize, graph_budget_from_v2, map_error, required_mut, set_error,
    wfst_domains, AbiScalarLabel, AbiScalarWeight, ArcticWeight, BoolWeight, CountWeight,
    GraphBudget, LlingBudgetV2, LlingLlangStatus, LogWeight, ProbabilityWeight,
    SignedTropicalWeight, TropicalWeight,
};
use crate::bindings::import_native_wfst_with_budget_and_stats;
use crate::subsequential::{SubsequentialSession, SubsequentialTransducer};
use crate::wfst::{StateId, VectorWfst, Wfst};
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;
use vinary_tree_interop::{VtResource, VtUnitDomain, VtWeightDomain};

type Apply = dyn Fn(&[u64]) -> Result<Option<(Vec<u64>, f64)>, LlingLlangStatus> + Send + Sync;
type SessionFactory = dyn Fn() -> Box<dyn SessionOps> + Send + Sync;

trait SessionOps: Send {
    fn advance(&mut self, raw: u64) -> Result<Option<Option<u64>>, LlingLlangStatus>;
    fn finish(&self) -> Result<Option<(Vec<u64>, f64)>, LlingLlangStatus>;
}

struct TypedSession<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    session: SubsequentialSession<L, W>,
}

impl<L, W> SessionOps for TypedSession<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    fn advance(&mut self, raw: u64) -> Result<Option<Option<u64>>, LlingLlangStatus> {
        let symbol = L::decode(raw).ok_or_else(|| {
            set_error("input label is outside the WFST unit domain");
            LlingLlangStatus::InvalidArgument
        })?;
        Ok(self
            .session
            .advance(&symbol)
            .map(|output| output.map(|label| label.encode())))
    }

    fn finish(&self) -> Result<Option<(Vec<u64>, f64)>, LlingLlangStatus> {
        self.session
            .finish()
            .map(|(suffix, weight)| {
                let weight = weight.encode().ok_or_else(|| {
                    set_error(
                        "native result weight is not exactly representable by the scalar ABI",
                    );
                    LlingLlangStatus::LimitExceeded
                })?;
                Ok((suffix.iter().map(L::encode).collect(), weight))
            })
            .transpose()
    }
}

/// Immutable subsequential runner over one captured scalar-WFST snapshot.
pub struct LlingSubsequential {
    apply: Box<Apply>,
    start_session: Box<SessionFactory>,
    max_input_units: usize,
}

/// One mutable run over a captured immutable subsequential graph.
pub struct LlingSubsequentialSession {
    inner: Box<dyn SessionOps>,
    max_input_units: usize,
    consumed: usize,
}

fn make_typed<L, W>(
    resource: VtResource,
    mut budget: GraphBudget,
    max_input_units: usize,
    final_states: &[u64],
    final_offsets: &[usize],
    final_labels: &[u64],
) -> Result<LlingSubsequential, LlingLlangStatus>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    let (graph, _): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(resource, &mut budget).map_err(map_error)?;
    let mut runner = SubsequentialTransducer::from_wfst(graph).ok_or_else(|| {
        set_error("WFST is not input-deterministic or contains an epsilon-input arc");
        LlingLlangStatus::InvalidArgument
    })?;
    if !final_states.is_empty() {
        let bytes = final_states
            .len()
            .checked_mul(std::mem::size_of::<(StateId, usize)>())
            .and_then(|states| {
                final_labels
                    .len()
                    .checked_mul(std::mem::size_of::<L>())
                    .and_then(|labels| states.checked_add(labels))
            })
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or(LlingLlangStatus::LimitExceeded)?;
        let work = final_states
            .len()
            .checked_add(final_labels.len())
            .and_then(|units| u64::try_from(units).ok())
            .ok_or(LlingLlangStatus::LimitExceeded)?;
        budget.charge(0, 0, bytes, work).map_err(map_error)?;
        if final_offsets.len() != final_states.len() + 1
            || final_offsets.first() != Some(&0)
            || final_offsets.last() != Some(&final_labels.len())
        {
            set_error("final output offsets must delimit the complete flat label array");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let mut seen = HashSet::new();
        for (index, &raw_state) in final_states.iter().enumerate() {
            let state = StateId::try_from(raw_state).map_err(|_| {
                set_error("final output state is outside the native state range");
                LlingLlangStatus::InvalidArgument
            })?;
            if !seen.insert(state) || !runner.wfst().is_final(state) {
                set_error("final output state must be a distinct accepting state");
                return Err(LlingLlangStatus::InvalidArgument);
            }
            let begin = final_offsets[index];
            let end = final_offsets[index + 1];
            if begin > end || end > final_labels.len() {
                set_error("final output offsets are not monotone and in bounds");
                return Err(LlingLlangStatus::InvalidArgument);
            }
            let labels = final_labels[begin..end]
                .iter()
                .map(|&raw| {
                    L::decode(raw).ok_or_else(|| {
                        set_error("final output label is outside the WFST unit domain");
                        LlingLlangStatus::InvalidArgument
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            runner.set_final_output(state, labels);
        }
    }
    let runner = Arc::new(runner);
    let apply_runner = Arc::clone(&runner);
    let apply = move |raw: &[u64]| {
        let input: Vec<L> = raw
            .iter()
            .map(|&value| {
                L::decode(value).ok_or_else(|| {
                    set_error("input label is outside the WFST unit domain");
                    LlingLlangStatus::InvalidArgument
                })
            })
            .collect::<Result<_, _>>()?;
        apply_runner
            .apply(&input)
            .map(|(output, weight)| {
                let weight = weight.encode().ok_or_else(|| {
                    set_error(
                        "native result weight is not exactly representable by the scalar ABI",
                    );
                    LlingLlangStatus::LimitExceeded
                })?;
                Ok((output.iter().map(L::encode).collect(), weight))
            })
            .transpose()
    };
    let start_session = move || {
        Box::new(TypedSession {
            session: SubsequentialSession::new(Arc::clone(&runner)),
        }) as Box<dyn SessionOps>
    };
    Ok(LlingSubsequential {
        apply: Box::new(apply),
        start_session: Box::new(start_session),
        max_input_units,
    })
}

/// Capture and validate an input-deterministic WFST for repeated streaming runs.
///
/// The source resource is borrowed for this call only. The returned runner owns
/// its native snapshot. `budget` bounds capture; `max_input_units` bounds each
/// subsequent call and must be finite and nonzero.
///
/// # Safety
///
/// `resource` and `budget` must point to readable values for this call, and
/// `out_runner` must point to a writable, nonoverlapping output slot. The
/// caller must keep the resource alive throughout snapshot capture.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_new_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    max_input_units: u64,
    out_runner: *mut *mut LlingSubsequential,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_runner, "out_runner")?;
        if resource.is_null() {
            set_error("resource is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let source = unsafe { &*resource };
        let max_input_units = bounded_usize(max_input_units, "max_input_units")?;
        if max_input_units == 0 {
            set_error("max_input_units must be positive");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let budget = graph_budget_from_v2(budget)?;
        let (unit, weight) = wfst_domains(*source).map_err(map_error)?;
        let runner = dispatch_scalar_domains!(
            unit,
            weight,
            make_typed,
            *source,
            budget,
            max_input_units,
            &[],
            &[],
            &[]
        )?;
        *output = Box::into_raw(Box::new(runner));
        Ok(())
    })
}

/// Capture a runner with final-output suffixes for selected accepting states.
///
/// `final_states` has `final_count` entries. `final_offsets` has
/// `final_count + 1` entries, starts at zero, and ends at
/// `final_label_count`. State `i` receives labels in the half-open range
/// `final_offsets[i]..final_offsets[i + 1]`. The four-axis graph budget also
/// charges suffix storage and decoding work. The captured runner is immutable.
///
/// # Safety
///
/// `resource` and `budget` must be readable for this call. For nonzero
/// lengths, each input array must be readable for its declared length;
/// `final_offsets` is required whenever `final_count` is nonzero. The arrays
/// and resource must stay alive through snapshot capture. `out_runner` must
/// be a writable, nonoverlapping output slot.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_new_with_final_outputs_ref(
    resource: *const VtResource,
    budget: *const LlingBudgetV2,
    max_input_units: u64,
    final_states: *const u64,
    final_offsets: *const usize,
    final_labels: *const u64,
    final_count: usize,
    final_label_count: usize,
    out_runner: *mut *mut LlingSubsequential,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_runner, "out_runner")?;
        if resource.is_null() {
            set_error("resource is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let source = unsafe { &*resource };
        let max_input_units = bounded_usize(max_input_units, "max_input_units")?;
        if max_input_units == 0 {
            set_error("max_input_units must be positive");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        if final_count != 0 && (final_states.is_null() || final_offsets.is_null()) {
            set_error("final output state or offset array is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        if final_label_count != 0 && final_labels.is_null() {
            set_error("final output label array is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let offset_count = final_count.checked_add(1).ok_or_else(|| {
            set_error("final output count overflows the offset array length");
            LlingLlangStatus::LimitExceeded
        })?;
        let states = if final_count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(final_states, final_count) }
        };
        let offsets = if final_count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(final_offsets, offset_count) }
        };
        let labels = if final_label_count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(final_labels, final_label_count) }
        };
        if states.is_empty() && !labels.is_empty() {
            set_error("final output labels require at least one final state");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let budget = graph_budget_from_v2(budget)?;
        let (unit, weight) = wfst_domains(*source).map_err(map_error)?;
        let runner = dispatch_scalar_domains!(
            unit,
            weight,
            make_typed,
            *source,
            budget,
            max_input_units,
            states,
            offsets,
            labels
        )?;
        *output = Box::into_raw(Box::new(runner));
        Ok(())
    })
}

/// Apply a captured subsequential runner to one input sequence.
///
/// On success, `out_accepted` is zero for an unaccepted input and one for an
/// accepted input. The caller owns `out_labels`; it needs at most `input_len`
/// plus the largest configured final-output suffix length elements.
/// All outputs remain untouched on error.
///
/// # Safety
///
/// `runner` must remain open throughout this call. `input` must contain
/// `input_len` readable values when nonempty, and `out_labels` must contain
/// `out_capacity` writable values when nonempty. The three result pointers
/// must be writable and must not overlap any other argument or one another.
/// The owner must coordinate concurrent calls with `lling_subsequential_free`.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_apply(
    runner: *const LlingSubsequential,
    input: *const u64,
    input_len: usize,
    out_labels: *mut u64,
    out_capacity: usize,
    out_len: *mut usize,
    out_weight: *mut f64,
    out_accepted: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        if runner.is_null() {
            set_error("runner is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let runner = unsafe { &*runner };
        let output_len = required_mut(out_len, "out_len")?;
        let output_weight = required_mut(out_weight, "out_weight")?;
        let accepted = required_mut(out_accepted, "out_accepted")?;
        if input_len > runner.max_input_units {
            set_error("input exceeds max_input_units");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        if input_len != 0 && input.is_null() {
            set_error("input is null for nonempty input");
            return Err(LlingLlangStatus::NullPointer);
        }
        let raw = if input_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(input, input_len) }
        };
        let result = (runner.apply)(raw)?;
        match result {
            None => {
                *output_len = 0;
                *output_weight = 0.0;
                *accepted = 0;
            }
            Some((labels, weight)) => {
                if labels.len() > out_capacity {
                    set_error("output buffer is smaller than the accepted result");
                    return Err(LlingLlangStatus::LimitExceeded);
                }
                if !labels.is_empty() && out_labels.is_null() {
                    set_error("out_labels is null for nonempty output");
                    return Err(LlingLlangStatus::NullPointer);
                }
                if !labels.is_empty() {
                    unsafe {
                        std::ptr::copy_nonoverlapping(labels.as_ptr(), out_labels, labels.len())
                    };
                }
                *output_len = labels.len();
                *output_weight = weight;
                *accepted = 1;
            }
        }
        Ok(())
    })
}

/// Start a bounded incremental run over the runner's immutable graph.
///
/// The returned session owns a shared graph reference and remains valid after
/// the runner is freed. The caller owns and must release the session.
///
/// # Safety
///
/// `runner` must be an open runner for this call. `out_session` must point to
/// a writable output slot that does not overlap `runner`.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_session_new(
    runner: *const LlingSubsequential,
    out_session: *mut *mut LlingSubsequentialSession,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_session, "out_session")?;
        if runner.is_null() {
            set_error("runner is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let runner = unsafe { &*runner };
        *output = Box::into_raw(Box::new(LlingSubsequentialSession {
            inner: (runner.start_session)(),
            max_input_units: runner.max_input_units,
            consumed: 0,
        }));
        Ok(())
    })
}

/// Consume one input label and return an optional emitted label.
///
/// `out_active` is zero after a rejected prefix; `out_emitted` is one only
/// when the consumed arc emitted a label. A rejected session remains rejected.
/// On error, the session and outputs are unchanged.
///
/// # Safety
///
/// `session` must remain open throughout this call. The three output slots
/// must be writable and mutually nonoverlapping, and the owner must
/// coordinate concurrent calls with `lling_subsequential_session_free`.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_session_step(
    session: *mut LlingSubsequentialSession,
    input_label: u64,
    out_active: *mut u8,
    out_emitted: *mut u8,
    out_label: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        let active = required_mut(out_active, "out_active")?;
        let emitted = required_mut(out_emitted, "out_emitted")?;
        let label = required_mut(out_label, "out_label")?;
        if session.is_null() {
            set_error("session is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let session = unsafe { &mut *session };
        if session.consumed >= session.max_input_units {
            set_error("input exceeds max_input_units");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let result = session.inner.advance(input_label)?;
        session.consumed += 1;
        *active = u8::from(result.is_some());
        *emitted = u8::from(result.is_some_and(|value| value.is_some()));
        *label = result.flatten().unwrap_or(0);
        Ok(())
    })
}

/// Inspect the current final output suffix and total weight.
///
/// `out_len` reports the required suffix capacity even when the supplied
/// buffer is too small and the function returns `LimitExceeded`. All other
/// outputs remain unchanged on that error. The session itself is unchanged,
/// so the caller may allocate `out_len` elements and retry.
///
/// # Safety
///
/// `session` must remain open throughout this call. When `out_capacity` is
/// nonzero, `out_labels` must be writable for that many elements. The three
/// scalar outputs and the label buffer must not overlap one another or the
/// session. The owner must coordinate concurrent access and release.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_session_finish(
    session: *const LlingSubsequentialSession,
    out_labels: *mut u64,
    out_capacity: usize,
    out_len: *mut usize,
    out_weight: *mut f64,
    out_accepted: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        let output_len = required_mut(out_len, "out_len")?;
        let weight = required_mut(out_weight, "out_weight")?;
        let accepted = required_mut(out_accepted, "out_accepted")?;
        if session.is_null() {
            set_error("session is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let session = unsafe { &*session };
        match session.inner.finish()? {
            None => {
                *output_len = 0;
                *weight = 0.0;
                *accepted = 0;
            }
            Some((suffix, result_weight)) => {
                *output_len = suffix.len();
                if suffix.len() > out_capacity {
                    set_error("final output buffer is smaller than the suffix");
                    return Err(LlingLlangStatus::LimitExceeded);
                }
                if !suffix.is_empty() && out_labels.is_null() {
                    set_error("out_labels is null for nonempty final output");
                    return Err(LlingLlangStatus::NullPointer);
                }
                if !suffix.is_empty() {
                    unsafe {
                        std::ptr::copy_nonoverlapping(suffix.as_ptr(), out_labels, suffix.len())
                    };
                }
                *weight = result_weight;
                *accepted = 1;
            }
        }
        Ok(())
    })
}

/// Release an incremental subsequential session.
///
/// # Safety
///
/// A non-null session must be an open handle returned by
/// `lling_subsequential_session_new`, released exactly once. It must not be
/// accessed concurrently or after this function returns.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_session_free(session: *mut LlingSubsequentialSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Release a runner returned by `lling_subsequential_new_ref`.
///
/// # Safety
///
/// A non-null `runner` must be an open handle returned by
/// `lling_subsequential_new_ref`, used exactly once for release. No call may
/// access it concurrently or after this function returns.
#[no_mangle]
pub unsafe extern "C" fn lling_subsequential_free(runner: *mut LlingSubsequential) {
    if !runner.is_null() {
        drop(unsafe { Box::from_raw(runner) });
    }
}
