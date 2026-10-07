//! Shared bounded-operation contract for WFST/SFT algorithms.
//!
//! This module gives lazy state expansion a first adapter without changing
//! legacy expansion results. Other algorithms can use the same costs, limits,
//! cancellation, checkpoint identity and quality-typed outcome vocabulary.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

use crate::semiring::Semiring;

use super::{
    CancellationToken, ExpansionError, ExpansionStatus, LazyState, LazyWfstWrapper, SourceSnapshot,
    StateId, StateSource,
};

const CHECKPOINT_MAGIC: &[u8; 8] = b"LWOPCP01";
const RECEIPT_MAGIC: &[u8; 8] = b"LWOPRC01";
const CHECKPOINT_LENGTH: usize = 8 + 32 + 32 + 8 * 6 + 32;

/// Immutable identity of one ordered operation over one source snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationIdentity {
    /// Exact state-source semantic snapshot.
    pub source: SourceSnapshot,
    /// BLAKE3 digest of operation ID and ordered state requests.
    pub plan_digest: [u8; 32],
}

/// Ordered state requests. An operation ID must change when its semantics do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationPlan {
    /// Snapshot to which all requested expansions must belong.
    pub identity: OperationIdentity,
    /// Caller-supplied content identity of the source, including immutable
    /// sources whose live snapshot is the default all-zero value.
    pub source_binding: [u8; 32],
    /// Stable algorithm name/version.
    pub algorithm_id: String,
    /// Ordered state requests; duplicates are deliberate distinct visits.
    pub states: Vec<StateId>,
}

impl OperationPlan {
    /// Bind an ordered request list to an algorithm and source snapshot.
    ///
    /// # Errors
    ///
    /// An empty algorithm ID cannot form a replayable plan.
    pub fn new(
        source: SourceSnapshot,
        source_binding: [u8; 32],
        algorithm_id: impl Into<String>,
        states: Vec<StateId>,
    ) -> Result<Self, OperationError> {
        let algorithm_id = algorithm_id.into();
        if algorithm_id.is_empty() || source_binding == [0; 32] {
            return Err(OperationError::InvalidPlan);
        }
        let mut digest = blake3::Hasher::new();
        digest.update(b"lling.wfst.operation-plan/v1\0");
        digest.update(source.as_bytes());
        digest.update(&source_binding);
        digest.update(&(algorithm_id.len() as u64).to_be_bytes());
        digest.update(algorithm_id.as_bytes());
        let state_count = u64::try_from(states.len()).map_err(|_| OperationError::InvalidPlan)?;
        digest.update(&state_count.to_be_bytes());
        for state in &states {
            digest.update(&state.to_be_bytes());
        }
        Ok(Self {
            identity: OperationIdentity {
                source,
                plan_digest: *digest.finalize().as_bytes(),
            },
            source_binding,
            algorithm_id,
            states,
        })
    }
}

/// Absolute logical resource limits. `max_heap_bytes` bounds *caller-metered*
/// heap bytes, not process RSS or hidden allocations inside generic labels.
/// `max_elapsed_ns` is monotonic elapsed time across resume boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationLimits {
    /// Maximum ordered state visits.
    pub max_states: u64,
    /// Maximum outgoing arcs admitted across visits.
    pub max_arcs: u64,
    /// Maximum charged abstract work units.
    pub max_work: u64,
    /// Maximum caller-metered logical heap bytes.
    pub max_heap_bytes: u64,
    /// Maximum monotonic elapsed nanoseconds.
    pub max_elapsed_ns: u64,
}

impl Default for OperationLimits {
    fn default() -> Self {
        Self {
            max_states: u64::MAX,
            max_arcs: u64::MAX,
            max_work: u64::MAX,
            max_heap_bytes: u64::MAX,
            max_elapsed_ns: u64::MAX,
        }
    }
}

/// Logical resources charged and retained in a checkpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OperationUsage {
    /// Admitted ordered state visits.
    pub states: u64,
    /// Admitted outgoing arcs.
    pub arcs: u64,
    /// Admitted abstract work units.
    pub work: u64,
    /// Admitted caller-metered logical heap bytes.
    pub heap_bytes: u64,
}

/// Charge for one atomic operation step.
pub type OperationCost = OperationUsage;

/// Exact reason the supplied plan has not been exhausted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IncompleteReason {
    /// Cooperative cancellation was requested.
    Cancelled,
    /// Monotonic elapsed limit reached.
    TimeLimit,
    /// State-visit limit reached.
    StateLimit,
    /// Arc limit reached.
    ArcLimit,
    /// Abstract-work limit reached.
    WorkLimit,
    /// Caller-metered logical heap limit reached.
    HeapLimit,
}

impl IncompleteReason {
    const fn code(self) -> u8 {
        match self {
            Self::Cancelled => 1,
            Self::TimeLimit => 2,
            Self::StateLimit => 3,
            Self::ArcLimit => 4,
            Self::WorkLimit => 5,
            Self::HeapLimit => 6,
        }
    }
}

/// Explicit sound approximation claim from an operation-specific checker.
/// It is not a complete outcome or an assurance authority by itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApproximationBound {
    /// Maximum additive error in operation-defined micro-units.
    pub max_error_microunits: u64,
    /// Identity of the bound derivation/checker.
    pub method_digest: [u8; 32],
}

/// Checkpoint of one ordered operation. It never contains a partial result
/// under a complete-result key. `elapsed_ns` is carried across resume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationCheckpoint {
    /// Exact source and plan identity.
    pub identity: OperationIdentity,
    /// Next unprocessed request index.
    pub next_index: u64,
    /// Atomic charges of accepted requests only.
    pub usage: OperationUsage,
    /// Monotonic elapsed nanoseconds already consumed.
    pub elapsed_ns: u64,
}

impl OperationCheckpoint {
    /// Canonical big-endian bytes plus an accidental-corruption checksum.
    #[must_use]
    pub fn canonical_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CHECKPOINT_LENGTH);
        bytes.extend_from_slice(CHECKPOINT_MAGIC);
        bytes.extend_from_slice(self.identity.source.as_bytes());
        bytes.extend_from_slice(&self.identity.plan_digest);
        for value in [
            self.next_index,
            self.usage.states,
            self.usage.arcs,
            self.usage.work,
            self.usage.heap_bytes,
            self.elapsed_ns,
        ] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        let checksum = blake3::hash(&bytes);
        bytes.extend_from_slice(checksum.as_bytes());
        bytes
    }

    /// Decode only this canonical version and reject changed/trailing bytes.
    /// The checksum detects accidental corruption, not malicious forgery.
    ///
    /// # Errors
    ///
    /// Refuses a noncanonical or damaged checkpoint.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, OperationError> {
        if bytes.len() != CHECKPOINT_LENGTH || &bytes[..8] != CHECKPOINT_MAGIC {
            return Err(OperationError::CorruptCheckpoint);
        }
        let expected = blake3::hash(&bytes[..CHECKPOINT_LENGTH - 32]);
        if &bytes[CHECKPOINT_LENGTH - 32..] != expected.as_bytes() {
            return Err(OperationError::CorruptCheckpoint);
        }
        let mut source = [0; 32];
        source.copy_from_slice(&bytes[8..40]);
        let mut plan_digest = [0; 32];
        plan_digest.copy_from_slice(&bytes[40..72]);
        let mut values = [0; 6];
        for (index, value) in values.iter_mut().enumerate() {
            let start = 72 + index * 8;
            let raw: [u8; 8] = bytes[start..start + 8]
                .try_into()
                .map_err(|_| OperationError::CorruptCheckpoint)?;
            *value = u64::from_be_bytes(raw);
        }
        let checkpoint = Self {
            identity: OperationIdentity {
                source: SourceSnapshot::from_bytes(source),
                plan_digest,
            },
            next_index: values[0],
            usage: OperationUsage {
                states: values[1],
                arcs: values[2],
                work: values[3],
                heap_bytes: values[4],
            },
            elapsed_ns: values[5],
        };
        if checkpoint.canonical_bytes() != bytes {
            return Err(OperationError::CorruptCheckpoint);
        }
        Ok(checkpoint)
    }
}

/// Quality-typed operation outcome. No variant is silently promoted to
/// `Complete`; approximation requires an explicit bound identity.
#[derive(Debug)]
pub enum OperationOutcome<T> {
    /// Exact result for every supplied request.
    Complete {
        /// Exact result.
        value: T,
        /// Terminal resource/identity checkpoint.
        checkpoint: OperationCheckpoint,
    },
    /// A value with a separately checkable approximation bound.
    Approximate {
        /// Approximate value.
        value: T,
        /// Bound and checker identity.
        bound: ApproximationBound,
        /// Resource/identity checkpoint.
        checkpoint: OperationCheckpoint,
    },
    /// Only an admitted prefix was produced.
    Incomplete {
        /// Admitted prefix, never a complete result.
        partial: T,
        /// Exact interruption reason.
        reason: IncompleteReason,
        /// Next unprocessed request and charges.
        checkpoint: OperationCheckpoint,
    },
}

impl<T> OperationOutcome<T> {
    /// Return an exact value only; approximate and incomplete remain typed.
    pub fn into_complete(self) -> Option<T> {
        match self {
            Self::Complete { value, .. } => Some(value),
            Self::Approximate { .. } | Self::Incomplete { .. } => None,
        }
    }

    /// Canonical quality/resource receipt (not a proof of result contents).
    #[must_use]
    pub fn canonical_receipt_bytes(&self) -> Vec<u8> {
        let (checkpoint, quality, extra): (_, u8, Vec<u8>) = match self {
            Self::Complete { checkpoint, .. } => (checkpoint, 0, Vec::new()),
            Self::Approximate {
                checkpoint, bound, ..
            } => {
                let mut extra = Vec::with_capacity(40);
                extra.extend_from_slice(&bound.max_error_microunits.to_be_bytes());
                extra.extend_from_slice(&bound.method_digest);
                (checkpoint, 1, extra)
            }
            Self::Incomplete {
                checkpoint, reason, ..
            } => (checkpoint, 2, vec![reason.code()]),
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(RECEIPT_MAGIC);
        bytes.push(quality);
        bytes.extend_from_slice(&checkpoint.canonical_bytes());
        bytes.extend_from_slice(&extra);
        let checksum = blake3::hash(&bytes);
        bytes.extend_from_slice(checksum.as_bytes());
        bytes
    }
}

/// Cache that accepts only exact, complete outcomes. The key is derived from
/// the bound plan, not a caller-supplied partial-result label.
#[derive(Debug)]
pub struct CompleteResultCache<T> {
    entries: BTreeMap<[u8; 32], T>,
}

impl<T> Default for CompleteResultCache<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<T> CompleteResultCache<T> {
    /// Insert only an exact result at the end of the exact bound plan; return
    /// every other outcome unchanged, including a mislabeled partial cursor.
    pub fn insert(
        &mut self,
        plan: &OperationPlan,
        outcome: OperationOutcome<T>,
    ) -> Result<(), Box<OperationOutcome<T>>> {
        match outcome {
            OperationOutcome::Complete { value, checkpoint }
                if checkpoint.identity == plan.identity
                    && u64::try_from(plan.states.len()).ok() == Some(checkpoint.next_index)
                    && checkpoint.usage.states == checkpoint.next_index =>
            {
                self.entries.insert(plan.identity.plan_digest, value);
                Ok(())
            }
            other => Err(Box::new(other)),
        }
    }

    /// Look up an exact complete result by bound plan identity.
    pub fn get(&self, plan_digest: &[u8; 32]) -> Option<&T> {
        self.entries.get(plan_digest)
    }

    /// Number of exact completed plans retained.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no completed plans are retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Bounded, resumable cursor over one exact ordered state plan.
#[derive(Debug)]
pub struct OperationSession {
    plan: OperationPlan,
    limits: OperationLimits,
    cancellation: CancellationToken,
    next_index: usize,
    usage: OperationUsage,
    elapsed_before_ns: u64,
    started: Instant,
}

impl OperationSession {
    /// Start an unspent operation over one plan.
    #[must_use]
    pub fn new(
        plan: OperationPlan,
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            plan,
            limits,
            cancellation,
            next_index: 0,
            usage: OperationUsage::default(),
            elapsed_before_ns: 0,
            started: Instant::now(),
        }
    }

    /// Resume only the exact source/algorithm/ordered-state plan. Limits may
    /// be raised, but accepted charges and elapsed time cannot be reset.
    ///
    /// # Errors
    ///
    /// Refuses a stale or out-of-range checkpoint.
    pub fn resume(
        plan: OperationPlan,
        limits: OperationLimits,
        cancellation: CancellationToken,
        checkpoint: OperationCheckpoint,
    ) -> Result<Self, OperationError> {
        if checkpoint.identity != plan.identity
            || usize::try_from(checkpoint.next_index)
                .ok()
                .is_none_or(|index| index > plan.states.len())
            || checkpoint.usage.states != checkpoint.next_index
        {
            return Err(OperationError::StaleCheckpoint);
        }
        Ok(Self {
            plan,
            limits,
            cancellation,
            next_index: checkpoint.next_index as usize,
            usage: checkpoint.usage,
            elapsed_before_ns: checkpoint.elapsed_ns,
            started: Instant::now(),
        })
    }

    /// Snapshot identity, progress and resource accounting for persistence.
    #[must_use]
    pub fn checkpoint(&self) -> OperationCheckpoint {
        OperationCheckpoint {
            identity: self.plan.identity,
            next_index: self.next_index as u64,
            usage: self.usage,
            elapsed_ns: self.elapsed_ns(),
        }
    }

    /// Current resource accounting.
    #[must_use]
    pub const fn usage(&self) -> OperationUsage {
        self.usage
    }

    /// Poll shared cancellation and monotonic time.
    pub fn poll(&self) -> Result<(), IncompleteReason> {
        if self.cancellation.is_cancelled() {
            return Err(IncompleteReason::Cancelled);
        }
        if self.elapsed_ns() >= self.limits.max_elapsed_ns {
            return Err(IncompleteReason::TimeLimit);
        }
        Ok(())
    }

    /// Atomically charge one fully observed operation step.
    pub fn charge(&mut self, cost: OperationCost) -> Result<(), IncompleteReason> {
        self.usage = self.project_charge(cost)?;
        Ok(())
    }

    fn project_charge(&self, cost: OperationCost) -> Result<OperationUsage, IncompleteReason> {
        self.poll()?;
        let states = self
            .usage
            .states
            .checked_add(cost.states)
            .ok_or(IncompleteReason::StateLimit)?;
        let arcs = self
            .usage
            .arcs
            .checked_add(cost.arcs)
            .ok_or(IncompleteReason::ArcLimit)?;
        let work = self
            .usage
            .work
            .checked_add(cost.work)
            .ok_or(IncompleteReason::WorkLimit)?;
        let heap_bytes = self
            .usage
            .heap_bytes
            .checked_add(cost.heap_bytes)
            .ok_or(IncompleteReason::HeapLimit)?;
        for (used, limit, reason) in [
            (states, self.limits.max_states, IncompleteReason::StateLimit),
            (arcs, self.limits.max_arcs, IncompleteReason::ArcLimit),
            (work, self.limits.max_work, IncompleteReason::WorkLimit),
            (
                heap_bytes,
                self.limits.max_heap_bytes,
                IncompleteReason::HeapLimit,
            ),
        ] {
            if used > limit {
                return Err(reason);
            }
        }
        Ok(OperationUsage {
            states,
            arcs,
            work,
            heap_bytes,
        })
    }

    /// Expand ordered lazy states with the shared contract. The meter supplies
    /// the complete logical heap charge for one returned state, including any
    /// label/weight payloads it owns. A rejected newly computed state is
    /// removed from the wrapper cache before returning `Incomplete`.
    /// `observed_source_binding` must be freshly computed from the actual
    /// source contents; the live snapshot alone may be the default identity.
    ///
    /// # Errors
    ///
    /// Refuses source drift or a failed/unauthorized expansion; neither is a
    /// complete result. Cancellation and limits are typed incomplete outcomes.
    pub fn run_lazy<S, L, W, F>(
        &mut self,
        wrapper: &mut LazyWfstWrapper<S, L, W>,
        observed_source_binding: [u8; 32],
        meter: F,
    ) -> Result<OperationOutcome<Vec<StateId>>, OperationError>
    where
        S: StateSource<L, W>,
        L: Clone + Send + Sync,
        W: Semiring,
        F: Fn(&LazyState<L, W>) -> u64,
    {
        if wrapper.current_snapshot() != self.plan.identity.source
            || wrapper.source().snapshot() != self.plan.identity.source
            || observed_source_binding != self.plan.source_binding
        {
            return Err(OperationError::StaleSource);
        }
        while self.next_index < self.plan.states.len() {
            if let Err(reason) = self.poll() {
                return Ok(self.incomplete(reason));
            }
            let minimum = OperationCost {
                states: 1,
                work: 1,
                ..OperationCost::default()
            };
            if let Err(reason) = self.preview_charge(minimum) {
                return Ok(self.incomplete(reason));
            }
            let state = self.plan.states[self.next_index];
            let before = wrapper.expansion_status(state)?;
            if before == ExpansionStatus::Cancelled {
                wrapper.reset_cancelled(state)?;
            }
            let newly_computed = !before.is_cacheable();
            match wrapper.expand_with(state, &self.cancellation) {
                Ok(_) => {}
                Err(ExpansionError::Cancelled(_)) => {
                    if wrapper.expansion_status(state)? == ExpansionStatus::Cancelled {
                        wrapper.reset_cancelled(state)?;
                    }
                    return Ok(self.incomplete(IncompleteReason::Cancelled));
                }
                Err(error) => return Err(OperationError::Expansion(error)),
            }
            if let Err(reason) = self.poll() {
                if newly_computed {
                    wrapper.clear_state(state);
                }
                return Ok(self.incomplete(reason));
            }
            let completed = wrapper
                .lifecycle_state(state)?
                .ok_or(OperationError::MissingState)?;
            let arcs = completed
                .transitions()
                .ok_or(OperationError::MissingState)?
                .len() as u64;
            let heap_bytes = meter(completed);
            let cost = OperationCost {
                states: 1,
                arcs,
                work: arcs.checked_add(1).ok_or(OperationError::InvalidCost)?,
                heap_bytes,
            };
            if let Err(reason) = self.charge(cost) {
                if newly_computed {
                    wrapper.clear_state(state);
                }
                return Ok(self.incomplete(reason));
            }
            self.next_index += 1;
        }
        Ok(OperationOutcome::Complete {
            value: self.plan.states.clone(),
            checkpoint: self.checkpoint(),
        })
    }

    fn preview_charge(&self, cost: OperationCost) -> Result<(), IncompleteReason> {
        self.project_charge(cost).map(|_| ())
    }

    fn incomplete(&self, reason: IncompleteReason) -> OperationOutcome<Vec<StateId>> {
        OperationOutcome::Incomplete {
            partial: self.plan.states[..self.next_index].to_vec(),
            reason,
            checkpoint: self.checkpoint(),
        }
    }

    fn elapsed_ns(&self) -> u64 {
        self.elapsed_before_ns
            .saturating_add(u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX))
    }
}

/// Operation contract or lazy-expansion failure.
#[derive(Debug)]
pub enum OperationError {
    /// Empty or unsupported operation plan.
    InvalidPlan,
    /// Checkpoint does not belong to the exact plan or cursor.
    StaleCheckpoint,
    /// Source semantics changed relative to the plan.
    StaleSource,
    /// Canonical checkpoint bytes are damaged or noncanonical.
    CorruptCheckpoint,
    /// Source claimed completion but its state record is missing.
    MissingState,
    /// Abstract operation cost overflowed.
    InvalidCost,
    /// Lazy state source failed independently of a resource limit.
    Expansion(ExpansionError),
}

impl From<ExpansionError> for OperationError {
    fn from(error: ExpansionError) -> Self {
        Self::Expansion(error)
    }
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OperationError {}
