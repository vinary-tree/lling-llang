//! Bounded token-WFST constrained decoding for managed runtimes.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::bindings::import_native_wfst_with_budget_and_stats;
use crate::llm::{
    BeamSearchConfig, BeamSearchError, BeamSearchStats, BeamStep, ConstrainedBeamSession,
    WfstConstraint,
};
use crate::semiring::TropicalWeight;
use crate::wfst::{MutableWfst, VectorWfst, WeightedTransition, Wfst, NO_STATE};
use vinary_tree_interop::VtResource;

use super::{
    boundary, graph_budget_from_v2, map_error, required_mut, set_error, LlingBudgetV2,
    LlingLlangStatus,
};

const MAX_STATES: u64 = 65_536;
const MAX_ARCS: u64 = 1_048_576;
const MAX_GRAPH_BYTES: u64 = 128 * 1024 * 1024;
const MAX_WORK: u64 = 8_388_608;
const MAX_MASK_BYTES: usize = 64 * 1024 * 1024;

/// Fixed revision-1 scoring and work contract for one decoder session.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LlingLlmConfig {
    /// Must be one.
    pub version: u32,
    /// Must be zero.
    pub reserved: u32,
    /// Maximum live hypotheses after each step.
    pub beam_width: usize,
    /// Maximum number of token steps.
    pub max_length: usize,
    /// Hard bound on materialized expansions in one step.
    pub max_candidates_per_step: usize,
    /// Hard bound on the dense score buffer's vocabulary size.
    pub max_vocabulary: usize,
    /// Positive divisor applied to token log probabilities.
    pub temperature: f64,
    /// Nonnegative exponent applied to sequence length for ranking.
    pub length_penalty: f64,
}

/// Owned exclusive decoder session over a captured token WFST.
pub struct LlingLlmSession {
    inner: ConstrainedBeamSession<WfstConstraint<TropicalWeight>>,
}

/// Independently owned atomic cancellation token for a decoder session.
pub struct LlingLlmCancellation {
    flag: Arc<AtomicBool>,
}

/// Cumulative counters for one session.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LlingLlmStats {
    /// Successfully committed token steps.
    pub steps: u64,
    /// Model callback invocations.
    pub score_calls: u64,
    /// Legal token transitions inspected.
    pub considered_tokens: u64,
    /// Finite scored transitions materialized before pruning.
    pub candidates: u64,
    /// Number of current hypotheses.
    pub hypotheses: usize,
    /// One if the decoder reached a terminal condition.
    pub finished: u8,
    /// Zero-filled ABI padding.
    pub reserved: [u8; 7],
}

impl LlingLlmConfig {
    fn validated(self) -> Result<BeamSearchConfig, LlingLlangStatus> {
        if self.version != 1 || self.reserved != 0 {
            set_error("unsupported LLM config version or nonzero reserved field");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let config = BeamSearchConfig {
            beam_width: self.beam_width,
            max_length: self.max_length,
            max_candidates_per_step: self.max_candidates_per_step,
            max_vocabulary: self.max_vocabulary,
            temperature: self.temperature,
            length_penalty: self.length_penalty,
        };
        config.validate().map_err(|message| {
            set_error(message);
            LlingLlangStatus::InvalidArgument
        })?;
        if config.beam_width > 65_536
            || config.max_length > 1_048_576
            || config.max_candidates_per_step > 1_048_576
            || config.max_vocabulary > 1_048_576
        {
            set_error("LLM config exceeds native hard limits");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        Ok(config)
    }
}

fn session(
    pointer: *mut LlingLlmSession,
) -> Result<&'static mut LlingLlmSession, LlingLlangStatus> {
    required_mut(pointer, "session")
}

/// Capture a deterministic token acceptor and create a bounded beam session.
///
/// The resource must have `u64` labels and tropical weights. Every arc must
/// be a non-epsilon, deterministic acceptor transition with a label smaller
/// than `vocab_size`. The input is imported under `budget` and capped again
/// by the decoder's own graph and mask limits. The returned handle owns its
/// graph and remains valid after the provider resource is released.
///
/// # Safety
/// Pointers must address valid input/output objects. The resource must obey
/// the Vinary Tree retain/release contract for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_session_open_wfst(
    resource: *const VtResource,
    vocab_size: usize,
    config: *const LlingLlmConfig,
    budget: *const LlingBudgetV2,
    out_session: *mut *mut LlingLlmSession,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_session, "out_session")?;
        *output = std::ptr::null_mut();
        let resource = *unsafe { resource.as_ref() }.ok_or_else(|| {
            set_error("resource is null");
            LlingLlangStatus::NullPointer
        })?;
        let config = unsafe { config.as_ref() }
            .ok_or_else(|| {
                set_error("config is null");
                LlingLlangStatus::NullPointer
            })?
            .validated()?;
        if vocab_size == 0 || vocab_size > config.max_vocabulary || vocab_size > u32::MAX as usize {
            set_error("vocabulary size is zero or exceeds the configured token range");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let mut graph_budget = graph_budget_from_v2(budget)?;
        graph_budget.cap(MAX_STATES, MAX_ARCS, MAX_GRAPH_BYTES, MAX_WORK);
        let (graph, _) = import_native_wfst_with_budget_and_stats::<u64, TropicalWeight>(
            resource,
            &mut graph_budget,
        )
        .map_err(map_error)?;
        if graph.start() == NO_STATE {
            set_error("token constraint has no start state");
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let words = vocab_size.div_ceil(64);
        let mask_bytes = graph
            .num_states()
            .checked_mul(words)
            .and_then(|value| value.checked_mul(8))
            .ok_or_else(|| {
                set_error("token-mask size overflows");
                LlingLlangStatus::LimitExceeded
            })?;
        if mask_bytes > MAX_MASK_BYTES {
            set_error("token constraint exceeds the mask memory limit");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        let mut token_graph = VectorWfst::<u32, TropicalWeight>::new();
        for _ in 0..graph.num_states() {
            token_graph.add_state();
        }
        token_graph.set_start(graph.start());
        for state in 0..graph.num_states() {
            let state = state as u32;
            if graph.is_final(state) {
                token_graph.set_final(state, graph.final_weight(state));
            }
            let mut labels = HashSet::new();
            for arc in graph.transitions(state) {
                let Some(input) = arc.input else {
                    set_error("token constraint cannot contain epsilon arcs");
                    return Err(LlingLlangStatus::InvalidArgument);
                };
                if arc.output != Some(input) || input >= vocab_size as u64 {
                    set_error("token constraint must be an in-vocabulary acceptor");
                    return Err(LlingLlangStatus::InvalidArgument);
                }
                let token = input as u32;
                if !labels.insert(token) {
                    set_error("token constraint is nondeterministic");
                    return Err(LlingLlangStatus::InvalidArgument);
                }
                token_graph.add_transition(WeightedTransition {
                    from: state,
                    input: Some(token),
                    output: Some(token),
                    to: arc.to,
                    weight: arc.weight,
                });
            }
        }
        let decoder = WfstConstraint::new(token_graph, vocab_size);
        let inner = ConstrainedBeamSession::new(decoder, config).map_err(|message| {
            set_error(message);
            LlingLlangStatus::InvalidArgument
        })?;
        *output = Box::into_raw(Box::new(LlingLlmSession { inner }));
        Ok(())
    })
}

/// A same-thread scorer that fills `score_count` dense log probabilities.
/// Untouched elements retain negative infinity. Return zero on success; any
/// other value reports a model callback failure. All pointers are borrowed
/// only for the duration of the callback.
pub type LlingLlmScoreCallback = unsafe extern "C" fn(
    context: *mut c_void,
    prefix: *const u32,
    prefix_count: usize,
    out_scores: *mut f64,
    score_count: usize,
) -> u32;

/// Advance one token, publishing a complete new beam only on success.
///
/// `out_step` is 0 for advanced, 1 for complete, 2 for exhausted, and 3 for
/// cancelled. Cancellation is returned as a successful terminal observation
/// so callers can distinguish it from invalid scores or provider failure.
///
/// # Safety
/// The session must be live and exclusively borrowed. The callback and
/// context must remain valid until this call returns, and may not reenter
/// this session.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_session_step(
    pointer: *mut LlingLlmSession,
    callback: Option<LlingLlmScoreCallback>,
    context: *mut c_void,
    out_step: *mut u32,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_step, "out_step")?;
        let Some(callback) = callback else {
            set_error("model score callback is null");
            return Err(LlingLlangStatus::NullPointer);
        };
        let session = session(pointer)?;
        let result = session.inner.step(|prefix, scores| {
            let code = unsafe {
                callback(
                    context,
                    prefix.as_ptr(),
                    prefix.len(),
                    scores.as_mut_ptr(),
                    scores.len(),
                )
            };
            if code == 0 {
                Ok(())
            } else {
                Err(code)
            }
        });
        *output = match result {
            Ok(BeamStep::Advanced) => 0,
            Ok(BeamStep::Complete) => 1,
            Ok(BeamStep::Exhausted) => 2,
            Err(BeamSearchError::Cancelled) => 3,
            Err(BeamSearchError::Scorer(code)) => {
                set_error(format!("model score callback failed with code {code}"));
                return Err(LlingLlangStatus::ProviderError);
            }
            Err(BeamSearchError::InvalidScores(message)) => {
                set_error(message);
                return Err(LlingLlangStatus::InvalidArgument);
            }
            Err(BeamSearchError::CandidateLimit) => {
                set_error("LLM beam candidate limit exceeded");
                return Err(LlingLlangStatus::LimitExceeded);
            }
        };
        Ok(())
    })
}

/// Clone a session's independent cancellation token before a concurrent step.
/// The token remains valid after the session is released.
///
/// # Safety
/// `pointer` must be a live session and `out_cancellation` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_session_cancellation(
    pointer: *const LlingLlmSession,
    out_cancellation: *mut *mut LlingLlmCancellation,
) -> LlingLlangStatus {
    boundary(|| {
        let session = unsafe { pointer.as_ref() }.ok_or_else(|| {
            set_error("session is null");
            LlingLlangStatus::NullPointer
        })?;
        let output = required_mut(out_cancellation, "out_cancellation")?;
        *output = Box::into_raw(Box::new(LlingLlmCancellation {
            flag: session.inner.cancellation_flag(),
        }));
        Ok(())
    })
}

/// Request cancellation concurrently with a decoder step.
///
/// # Safety
/// `pointer` must be a live cancellation token.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_cancellation_request(
    pointer: *const LlingLlmCancellation,
) -> LlingLlangStatus {
    boundary(|| {
        let cancellation = unsafe { pointer.as_ref() }.ok_or_else(|| {
            set_error("cancellation token is null");
            LlingLlangStatus::NullPointer
        })?;
        cancellation.flag.store(true, Ordering::Release);
        Ok(())
    })
}

/// Release an independently owned cancellation token. Null is accepted.
///
/// # Safety
/// A non-null pointer must be a live token and must not already be released.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_cancellation_free(pointer: *mut LlingLlmCancellation) {
    if !pointer.is_null() {
        drop(Box::from_raw(pointer));
    }
}

/// Return cumulative committed work and current beam size.
///
/// # Safety
/// `pointer` must be a live session and `out_stats` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_session_stats(
    pointer: *const LlingLlmSession,
    out_stats: *mut LlingLlmStats,
) -> LlingLlangStatus {
    boundary(|| {
        let session = unsafe { pointer.as_ref() }.ok_or_else(|| {
            set_error("session is null");
            LlingLlangStatus::NullPointer
        })?;
        let output = required_mut(out_stats, "out_stats")?;
        let BeamSearchStats {
            steps,
            score_calls,
            considered_tokens,
            candidates,
        } = session.inner.stats();
        *output = LlingLlmStats {
            steps,
            score_calls,
            considered_tokens,
            candidates,
            hypotheses: session.inner.hypotheses().len(),
            finished: u8::from(session.inner.is_finished()),
            reserved: [0; 7],
        };
        Ok(())
    })
}

/// Copy one current hypothesis into caller-owned storage.
///
/// A zero capacity queries the required token count without writing tokens.
/// The caller must synchronize this read with `step` and `free`.
///
/// # Safety
/// Outputs must be valid; `out_tokens` must cover `capacity` elements when
/// capacity is nonzero.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_session_hypothesis(
    pointer: *const LlingLlmSession,
    index: usize,
    out_tokens: *mut u32,
    capacity: usize,
    out_written: *mut usize,
    out_required: *mut usize,
    out_score: *mut f64,
    out_accepting: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        let session = unsafe { pointer.as_ref() }.ok_or_else(|| {
            set_error("session is null");
            LlingLlangStatus::NullPointer
        })?;
        let hypothesis = session.inner.hypotheses().get(index).ok_or_else(|| {
            set_error("hypothesis index is out of range");
            LlingLlangStatus::InvalidArgument
        })?;
        let written = required_mut(out_written, "out_written")?;
        let required = required_mut(out_required, "out_required")?;
        let score = required_mut(out_score, "out_score")?;
        let accepting = required_mut(out_accepting, "out_accepting")?;
        if capacity != 0 && out_tokens.is_null() {
            set_error("out_tokens is null with nonzero capacity");
            return Err(LlingLlangStatus::NullPointer);
        }
        *required = hypothesis.tokens.len();
        *written = capacity.min(*required);
        *score = hypothesis.score;
        *accepting = u8::from(
            session
                .inner
                .is_accepting(index)
                .expect("index checked above"),
        );
        if *written != 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(hypothesis.tokens.as_ptr(), out_tokens, *written);
            }
        }
        Ok(())
    })
}

/// Release the exclusive session. Null is accepted.
///
/// # Safety
/// A non-null pointer must be a live session and not concurrently borrowed.
#[no_mangle]
pub unsafe extern "C" fn lling_llm_session_free(pointer: *mut LlingLlmSession) {
    if !pointer.is_null() {
        drop(Box::from_raw(pointer));
    }
}
