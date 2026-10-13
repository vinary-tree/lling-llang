//! Reusable, snapshot-owned bridge to the native subsequential runner.

use super::{
    boundary, bounded_usize, graph_budget_from_v2, map_error, required_mut, set_error,
    wfst_domains, AbiScalarLabel, AbiScalarWeight, ArcticWeight, BoolWeight, CountWeight,
    GraphBudget, LlingBudgetV2, LlingLlangStatus, LogWeight, ProbabilityWeight,
    SignedTropicalWeight, TropicalWeight,
};
use crate::bindings::import_native_wfst_with_budget_and_stats;
use crate::subsequential::SubsequentialTransducer;
use crate::wfst::VectorWfst;
use std::hash::Hash;
use vinary_tree_interop::{VtResource, VtUnitDomain, VtWeightDomain};

type Apply = dyn Fn(&[u64]) -> Result<Option<(Vec<u64>, f64)>, LlingLlangStatus> + Send + Sync;

/// Immutable subsequential runner over one captured scalar-WFST snapshot.
pub struct LlingSubsequential {
    apply: Box<Apply>,
    max_input_units: usize,
}

fn make_typed<L, W>(
    resource: VtResource,
    mut budget: GraphBudget,
    max_input_units: usize,
) -> Result<LlingSubsequential, LlingLlangStatus>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    let (graph, _): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(resource, &mut budget).map_err(map_error)?;
    let runner = SubsequentialTransducer::from_wfst(graph).ok_or_else(|| {
        set_error("WFST is not input-deterministic or contains an epsilon-input arc");
        LlingLlangStatus::InvalidArgument
    })?;
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
        runner
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
    Ok(LlingSubsequential {
        apply: Box::new(apply),
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
        let runner =
            dispatch_scalar_domains!(unit, weight, make_typed, *source, budget, max_input_units)?;
        *output = Box::into_raw(Box::new(runner));
        Ok(())
    })
}

/// Apply a captured subsequential runner to one input sequence.
///
/// On success, `out_accepted` is zero for an unaccepted input and one for an
/// accepted input. The caller owns `out_labels`; it needs at most `input_len`
/// elements because this runner has no final-output suffix configuration.
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
