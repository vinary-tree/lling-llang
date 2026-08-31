//! Lazy WFST types for on-demand state expansion.
//!
//! This module provides infrastructure for lazy WFSTs that compute states
//! only when accessed. This is critical for composition operations where
//! the product state space can be exponentially large.
//!
//! # Architecture
//!
//! - [`LazyState`]: Represents a state that may or may not be computed
//! - [`StateSource`]: Trait for types that can produce states on demand
//! - [`LazyWfstWrapper`]: Generic lazy WFST wrapper around a StateSource

use std::fmt;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::traits::{CachePolicy, LazyWfst, Wfst};
use super::transition::WeightedTransition;
use super::types::StateId;
use crate::semiring::Semiring;

/// Stable identity of the source semantics used by one expansion attempt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceSnapshot([u8; 32]);

impl SourceSnapshot {
    /// Snapshot for immutable sources whose semantics never change.
    pub const IMMUTABLE: Self = Self([0; 32]);

    /// Construct a snapshot from an application-defined digest.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Return the digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Why an expansion was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CancellationReason {
    /// A caller explicitly requested cancellation.
    Requested,
    /// A deadline expired.
    Deadline,
    /// A resource budget was exhausted.
    Budget,
    /// The state source cancelled its own work.
    Source,
}

impl CancellationReason {
    const fn code(self) -> u8 {
        match self {
            Self::Requested => 1,
            Self::Deadline => 2,
            Self::Budget => 3,
            Self::Source => 4,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Requested),
            2 => Some(Self::Deadline),
            3 => Some(Self::Budget),
            4 => Some(Self::Source),
            _ => None,
        }
    }
}

/// Cloneable, first-writer-wins cancellation signal.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    reason: Arc<AtomicU8>,
}

impl CancellationToken {
    /// Create an uncancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Returns `true` only for the winning request.
    pub fn cancel(&self, reason: CancellationReason) -> bool {
        self.reason
            .compare_exchange(0, reason.code(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Return the first cancellation reason, if cancellation was requested.
    pub fn reason(&self) -> Option<CancellationReason> {
        CancellationReason::from_code(self.reason.load(Ordering::Acquire))
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.reason.load(Ordering::Acquire) != 0
    }
}

/// Whether a failed expansion may be retried.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RetryPolicy {
    /// The failure is terminal until explicitly reset.
    Never,
    /// An explicit retry operation is allowed.
    Explicit,
}

impl RetryPolicy {
    /// Whether this policy authorizes explicit retry.
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Explicit)
    }
}

/// Stable classification of expansion failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpansionFailureKind {
    /// The source could not compute the requested state.
    Source,
    /// The state identifier is outside the source domain.
    InvalidState,
    /// A bounded identifier or memory resource was exhausted.
    ResourceExhausted,
}

/// A source failure together with its retry contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpansionFailure {
    kind: ExpansionFailureKind,
    retry: RetryPolicy,
    message: Arc<str>,
}

impl ExpansionFailure {
    /// Construct a classified failure.
    pub fn new(
        kind: ExpansionFailureKind,
        retry: RetryPolicy,
        message: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            kind,
            retry,
            message: message.into(),
        }
    }

    /// Construct a permanent invalid-state failure.
    pub fn invalid_state(state: StateId) -> Self {
        Self::new(
            ExpansionFailureKind::InvalidState,
            RetryPolicy::Never,
            format!("state {state} is outside the source domain"),
        )
    }

    /// Construct a permanent resource-exhaustion failure.
    pub fn resource_exhausted(message: impl Into<Arc<str>>) -> Self {
        Self::new(
            ExpansionFailureKind::ResourceExhausted,
            RetryPolicy::Never,
            message,
        )
    }

    /// Failure classification.
    pub const fn kind(&self) -> ExpansionFailureKind {
        self.kind
    }

    /// Retry contract.
    pub const fn retry_policy(&self) -> RetryPolicy {
        self.retry
    }

    /// Human-readable source diagnostic.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether explicit retry is permitted.
    pub const fn is_retryable(&self) -> bool {
        self.retry.is_retryable()
    }
}

impl fmt::Display for ExpansionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ExpansionFailure {}

/// Exact externally visible phase of one lazy state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpansionStatus {
    /// No source attempt has produced a result.
    Unexpanded,
    /// The unique owner is currently invoking the source.
    Expanding,
    /// Expansion completed with no outgoing transitions.
    ExpandedEmpty,
    /// Expansion completed with at least one outgoing transition.
    ExpandedNonempty,
    /// Expansion failed.
    Failed,
    /// Expansion was cancelled.
    Cancelled,
}

impl ExpansionStatus {
    /// Whether this phase has an observable terminal result.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::ExpandedEmpty | Self::ExpandedNonempty | Self::Failed | Self::Cancelled
        )
    }

    /// Whether this phase contains a completed result eligible for caching.
    pub const fn is_cacheable(self) -> bool {
        matches!(self, Self::ExpandedEmpty | Self::ExpandedNonempty)
    }

    /// Exact observation associated with a terminal phase.
    pub const fn observation(self) -> Option<ExpansionObservation> {
        match self {
            Self::ExpandedEmpty => Some(ExpansionObservation::Empty),
            Self::ExpandedNonempty => Some(ExpansionObservation::Nonempty),
            Self::Failed => Some(ExpansionObservation::Failure),
            Self::Cancelled => Some(ExpansionObservation::Cancellation),
            Self::Unexpanded | Self::Expanding => None,
        }
    }
}

/// Lossless classification of an observable terminal expansion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpansionObservation {
    /// A fresh completed state has no outgoing transitions.
    Empty,
    /// A fresh completed state has outgoing transitions.
    Nonempty,
    /// A fresh attempt failed.
    Failure,
    /// A fresh attempt was cancelled.
    Cancellation,
}

/// Operation used to claim an expansion attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpansionMode {
    /// Begin only from `Unexpanded`.
    Normal,
    /// Begin only from an explicitly retryable failure.
    ExplicitRetry,
}

impl ExpansionMode {
    const fn authorizes_status(self, status: ExpansionStatus, retryable: bool) -> bool {
        matches!((self, status), (Self::Normal, ExpansionStatus::Unexpanded))
            || matches!(
                (self, status),
                (Self::ExplicitRetry, ExpansionStatus::Failed)
            ) && retryable
    }

    /// Whether the modeled preconditions authorize a new owner.
    pub const fn is_authorized(
        self,
        status: ExpansionStatus,
        retryable: bool,
        cancelled: bool,
    ) -> bool {
        !cancelled && self.authorizes_status(status, retryable)
    }
}

/// Immutable request passed to a [`StateSource`].
#[derive(Clone, Copy, Debug)]
pub struct ExpansionRequest<'a> {
    state: StateId,
    snapshot: SourceSnapshot,
    attempt: u64,
    cancellation: &'a CancellationToken,
}

impl<'a> ExpansionRequest<'a> {
    /// Requested state identifier.
    pub const fn state(self) -> StateId {
        self.state
    }

    /// Snapshot captured when this attempt acquired ownership.
    pub const fn snapshot(self) -> SourceSnapshot {
        self.snapshot
    }

    /// Monotonic, saturating attempt identifier.
    pub const fn attempt(self) -> u64 {
        self.attempt
    }

    /// Cooperative cancellation token.
    pub const fn cancellation(self) -> &'a CancellationToken {
        self.cancellation
    }
}

/// Result returned by a [`StateSource`] without conflating absence and emptiness.
#[derive(Clone, Debug)]
pub enum StateExpansion<L, W: Semiring> {
    /// Fully computed state information.
    Expanded {
        /// Whether the state accepts.
        is_final: bool,
        /// Final weight, or semiring zero for a non-final state.
        final_weight: W,
        /// Exact outgoing transition set.
        transitions: SmallVec<[WeightedTransition<L, W>; 4]>,
    },
    /// Classified source failure.
    Failed(ExpansionFailure),
    /// Source-directed cancellation.
    Cancelled(CancellationReason),
}

impl<L, W: Semiring> StateExpansion<L, W> {
    /// Construct a completed non-final state.
    pub fn non_final(transitions: SmallVec<[WeightedTransition<L, W>; 4]>) -> Self {
        Self::Expanded {
            is_final: false,
            final_weight: W::zero(),
            transitions,
        }
    }

    /// Construct a completed final state.
    pub fn final_state(
        final_weight: W,
        transitions: SmallVec<[WeightedTransition<L, W>; 4]>,
    ) -> Self {
        Self::Expanded {
            is_final: true,
            final_weight,
            transitions,
        }
    }

    /// Construct a failed expansion.
    pub fn failed(failure: ExpansionFailure) -> Self {
        Self::Failed(failure)
    }

    /// Construct a cancelled expansion.
    pub const fn cancelled(reason: CancellationReason) -> Self {
        Self::Cancelled(reason)
    }
}

/// A state whose lifecycle phase is represented explicitly.
#[derive(Clone, Debug, Default)]
pub enum LazyState<L, W: Semiring> {
    /// No attempt owns the state and no result is available.
    #[default]
    Unexpanded,
    /// Exactly one synchronous caller owns the source invocation.
    Expanding {
        /// Attempt identifier of the owner.
        attempt: u64,
    },
    /// State fully computed with exact transition information.
    Expanded {
        /// Whether this is a final state.
        is_final: bool,
        /// Final weight (semiring zero if not final).
        final_weight: W,
        /// Outgoing transitions.
        transitions: SmallVec<[WeightedTransition<L, W>; 4]>,
        /// Attempt that produced this result.
        attempt: u64,
    },
    /// Source expansion failed.
    Failed {
        /// Classified failure and retry contract.
        failure: ExpansionFailure,
        /// Attempt that failed.
        attempt: u64,
    },
    /// Expansion was cancelled.
    Cancelled {
        /// Winning cancellation reason.
        reason: CancellationReason,
        /// Attempt at cancellation, or the prior count for pre-cancellation.
        attempt: u64,
    },
}

impl<L, W: Semiring> LazyState<L, W> {
    /// Construct the unique-owner phase.
    pub const fn expanding(attempt: u64) -> Self {
        Self::Expanding { attempt }
    }

    /// Construct an exactly classified completed state.
    pub fn expanded(
        is_final: bool,
        final_weight: W,
        transitions: SmallVec<[WeightedTransition<L, W>; 4]>,
        attempt: u64,
    ) -> Self {
        Self::Expanded {
            is_final,
            final_weight: if is_final { final_weight } else { W::zero() },
            transitions,
            attempt,
        }
    }

    /// Construct a failed state.
    pub fn failed(failure: ExpansionFailure, attempt: u64) -> Self {
        Self::Failed { failure, attempt }
    }

    /// Construct a cancelled state.
    pub const fn cancelled(reason: CancellationReason, attempt: u64) -> Self {
        Self::Cancelled { reason, attempt }
    }

    /// Create a computed non-final state.
    pub fn non_final(transitions: SmallVec<[WeightedTransition<L, W>; 4]>) -> Self {
        Self::expanded(false, W::zero(), transitions, 0)
    }

    /// Create a computed final state.
    pub fn final_state(weight: W, transitions: SmallVec<[WeightedTransition<L, W>; 4]>) -> Self {
        Self::expanded(true, weight, transitions, 0)
    }

    /// Exact lifecycle status.
    pub fn status(&self) -> ExpansionStatus {
        match self {
            Self::Unexpanded => ExpansionStatus::Unexpanded,
            Self::Expanding { .. } => ExpansionStatus::Expanding,
            Self::Expanded { transitions, .. } if transitions.is_empty() => {
                ExpansionStatus::ExpandedEmpty
            }
            Self::Expanded { .. } => ExpansionStatus::ExpandedNonempty,
            Self::Failed { .. } => ExpansionStatus::Failed,
            Self::Cancelled { .. } => ExpansionStatus::Cancelled,
        }
    }

    /// Check if this state has been computed.
    #[inline]
    pub fn is_computed(&self) -> bool {
        matches!(self, Self::Expanded { .. })
    }

    /// Get transitions if computed.
    #[inline]
    pub fn transitions(&self) -> Option<&[WeightedTransition<L, W>]> {
        match self {
            Self::Expanded { transitions, .. } => Some(transitions.as_slice()),
            _ => None,
        }
    }

    /// Whether a failed state permits explicit retry.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Failed { failure, .. } if failure.is_retryable()
        )
    }

    /// Verify the representation invariants extracted from the formal model.
    pub fn is_well_formed(&self) -> bool {
        match self {
            Self::Expanded {
                is_final: false,
                final_weight,
                ..
            } => final_weight.is_zero(),
            _ => true,
        }
    }

    fn final_info(&self) -> Option<(bool, W)> {
        match self {
            Self::Expanded {
                is_final,
                final_weight,
                ..
            } => Some((*is_final, *final_weight)),
            _ => None,
        }
    }
}

/// Trait for types that can produce WFST states on demand.
///
/// Implement this trait to create custom lazy WFSTs (e.g., for composition).
pub trait StateSource<L, W: Semiring>: Clone + Send + Sync {
    /// Compute the state information for a given state ID.
    ///
    /// This method must return one explicit completion, failure, or cancellation.
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<L, W>;

    /// Identity of the source semantics observed by expansion requests.
    ///
    /// Immutable sources may use the default constant identity. A source with
    /// externally mutable semantics must change this value whenever its
    /// observable language or weights change.
    fn snapshot(&self) -> SourceSnapshot {
        SourceSnapshot::IMMUTABLE
    }

    /// Get the start state ID.
    fn start(&self) -> StateId;

    /// Get an upper bound on the number of states.
    ///
    /// Returns `None` if the number of states is unbounded or unknown.
    fn num_states_hint(&self) -> Option<usize> {
        None
    }
}

/// Failure of a lifecycle operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpansionError {
    /// A source attempt failed.
    Failure(ExpansionFailure),
    /// Expansion was cancelled.
    Cancelled(CancellationReason),
    /// The source changed after the wrapper captured its snapshot.
    StaleSnapshot {
        /// Snapshot expected by the wrapper.
        expected: SourceSnapshot,
        /// Snapshot currently reported by the source.
        observed: SourceSnapshot,
    },
    /// The requested mode is not valid for the state's current phase.
    Unauthorized {
        /// State identifier.
        state: StateId,
        /// Requested operation.
        mode: ExpansionMode,
        /// Current lifecycle phase.
        status: ExpansionStatus,
    },
    /// A completed transition slice was unavailable after successful expansion.
    MissingCompletedState(StateId),
}

impl fmt::Display for ExpansionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure(failure) => write!(formatter, "state expansion failed: {failure}"),
            Self::Cancelled(reason) => write!(formatter, "state expansion cancelled: {reason:?}"),
            Self::StaleSnapshot { expected, observed } => write!(
                formatter,
                "source snapshot changed from {expected:?} to {observed:?}"
            ),
            Self::Unauthorized {
                state,
                mode,
                status,
            } => write!(
                formatter,
                "{mode:?} expansion is unauthorized for state {state} in {status:?}"
            ),
            Self::MissingCompletedState(state) => {
                write!(
                    formatter,
                    "completed state {state} has no transition record"
                )
            }
        }
    }
}

impl std::error::Error for ExpansionError {}

#[derive(Clone, Debug)]
struct CacheEntry<L, W: Semiring> {
    state: LazyState<L, W>,
    previous: Option<StateId>,
    next: Option<StateId>,
}

/// Generic lazy WFST wrapper that computes states on demand.
///
/// Wraps a [`StateSource`] and caches computed states according to
/// the configured [`CachePolicy`].
///
/// # Type Parameters
///
/// - `S`: The state source type
/// - `L`: Label type
/// - `W`: Weight type
#[derive(Debug)]
pub struct LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    W: Semiring,
{
    /// The underlying state source.
    source: S,

    /// Cache of computed states.
    cache: FxHashMap<StateId, CacheEntry<L, W>>,

    /// Non-cacheable expanding, failed, and cancelled lifecycle records.
    lifecycle: FxHashMap<StateId, LazyState<L, W>>,

    /// Most recently computed state when caching is disabled.
    transient_state: Option<(StateId, LazyState<L, W>)>,

    /// Intrusive O(1) LRU endpoints; links are stored in cache entries.
    lru_head: Option<StateId>,
    lru_tail: Option<StateId>,

    /// Caching policy.
    policy: CachePolicy,

    /// Counter for computed states.
    computed_count: usize,

    /// Saturating count of source attempts in the active snapshot.
    total_attempts: u64,

    /// Source snapshot to which every retained entry belongs.
    current_snapshot: SourceSnapshot,

    /// Start state ID.
    start: StateId,
}

impl<S, L, W> Clone for LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W> + Clone,
    L: Clone,
    W: Semiring,
{
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            cache: self.cache.clone(),
            lifecycle: self.lifecycle.clone(),
            transient_state: self.transient_state.clone(),
            lru_head: self.lru_head,
            lru_tail: self.lru_tail,
            policy: self.policy,
            computed_count: self.computed_count,
            total_attempts: self.total_attempts,
            current_snapshot: self.current_snapshot,
            start: self.start,
        }
    }
}

impl<S, L, W> LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    /// Create a new lazy WFST with default caching.
    pub fn new(source: S) -> Self {
        let start = source.start();
        let initial_capacity = source.num_states_hint().unwrap_or(16);
        let current_snapshot = source.snapshot();

        Self {
            source,
            cache: FxHashMap::with_capacity_and_hasher(initial_capacity, Default::default()),
            lifecycle: FxHashMap::with_capacity_and_hasher(initial_capacity, Default::default()),
            transient_state: None,
            lru_head: None,
            lru_tail: None,
            policy: CachePolicy::default(),
            computed_count: 0,
            total_attempts: 0,
            current_snapshot,
            start,
        }
    }

    /// Create with a specific cache policy.
    pub fn with_cache_policy(source: S, policy: CachePolicy) -> Self {
        let mut wrapper = Self::new(source);
        wrapper.policy = policy;
        wrapper
    }

    /// Return any retained lifecycle record.
    fn state_entry(&self, state: StateId) -> Option<&LazyState<L, W>> {
        self.cache
            .get(&state)
            .map(|entry| &entry.state)
            .or_else(|| {
                self.transient_state
                    .as_ref()
                    .filter(|(transient_state, _)| *transient_state == state)
                    .map(|(_, entry)| entry)
            })
            .or_else(|| self.lifecycle.get(&state))
    }

    fn state_status_unchecked(&self, state: StateId) -> ExpansionStatus {
        self.state_entry(state)
            .map(LazyState::status)
            .unwrap_or(ExpansionStatus::Unexpanded)
    }

    fn check_snapshot(&self) -> Result<(), ExpansionError> {
        let observed = self.source.snapshot();
        if observed == self.current_snapshot {
            Ok(())
        } else {
            Err(ExpansionError::StaleSnapshot {
                expected: self.current_snapshot,
                observed,
            })
        }
    }

    fn rebind_snapshot(&mut self, snapshot: SourceSnapshot) {
        self.clear_all_states();
        self.current_snapshot = snapshot;
        self.start = self.source.start();
        self.computed_count = 0;
        self.total_attempts = 0;
    }

    fn store_lifecycle(&mut self, state: StateId, lifecycle_state: LazyState<L, W>) {
        debug_assert!(!self.cache.contains_key(&state));
        debug_assert!(!self
            .transient_state
            .as_ref()
            .is_some_and(|(transient, _)| *transient == state));
        self.lifecycle.insert(state, lifecycle_state);
    }

    /// Insert a completed state according to the cache policy.
    fn insert_completed(&mut self, state: StateId, completed: LazyState<L, W>) {
        debug_assert!(completed.status().is_cacheable());
        self.lifecycle.remove(&state);
        match self.policy {
            CachePolicy::NoCache => {
                self.transient_state = Some((state, completed));
            }
            CachePolicy::CacheAll => {
                self.transient_state = None;
                self.remove_cached(state);
                self.cache.insert(
                    state,
                    CacheEntry {
                        state: completed,
                        previous: None,
                        next: None,
                    },
                );
            }
            CachePolicy::Lru { max_states } => {
                self.transient_state = None;
                if max_states == 0 {
                    self.transient_state = Some((state, completed));
                    return;
                }

                self.remove_cached(state);
                while self.cache.len() >= max_states {
                    self.evict_lru();
                }
                let previous = self.lru_tail;
                self.cache.insert(
                    state,
                    CacheEntry {
                        state: completed,
                        previous,
                        next: None,
                    },
                );
                if let Some(tail) = previous {
                    if let Some(entry) = self.cache.get_mut(&tail) {
                        entry.next = Some(state);
                    }
                } else {
                    self.lru_head = Some(state);
                }
                self.lru_tail = Some(state);
            }
        }
    }

    /// Record a state-source computation without wrapping on extremely long traversals.
    fn record_computation(&mut self) {
        self.computed_count = self.computed_count.saturating_add(1);
    }

    /// Update LRU access order in constant time.
    fn touch_lru(&mut self, state: StateId) {
        if self.lru_tail == Some(state) {
            return;
        }
        let Some((previous, next)) = self
            .cache
            .get(&state)
            .map(|entry| (entry.previous, entry.next))
        else {
            return;
        };
        if let Some(previous) = previous {
            self.cache.get_mut(&previous).expect("valid LRU link").next = next;
        } else {
            self.lru_head = next;
        }
        if let Some(next) = next {
            self.cache.get_mut(&next).expect("valid LRU link").previous = previous;
        }
        let old_tail = self.lru_tail;
        {
            let entry = self
                .cache
                .get_mut(&state)
                .expect("cache membership checked");
            entry.previous = old_tail;
            entry.next = None;
        }
        if let Some(old_tail) = old_tail {
            self.cache.get_mut(&old_tail).expect("valid LRU tail").next = Some(state);
        } else {
            self.lru_head = Some(state);
        }
        self.lru_tail = Some(state);
    }

    fn remove_cached(&mut self, state: StateId) -> Option<CacheEntry<L, W>> {
        let entry = self.cache.remove(&state)?;
        if let Some(previous) = entry.previous {
            if let Some(previous_entry) = self.cache.get_mut(&previous) {
                previous_entry.next = entry.next;
            }
        } else if self.lru_head == Some(state) {
            self.lru_head = entry.next;
        }
        if let Some(next) = entry.next {
            if let Some(next_entry) = self.cache.get_mut(&next) {
                next_entry.previous = entry.previous;
            }
        } else if self.lru_tail == Some(state) {
            self.lru_tail = entry.previous;
        }
        Some(entry)
    }

    /// Evict the least-recently used persistent cached state in O(1).
    fn evict_lru(&mut self) {
        if let Some(head) = self.lru_head {
            self.remove_cached(head);
        }
    }

    fn rebuild_lru(&mut self) {
        let mut states: Vec<_> = self.cache.keys().copied().collect();
        states.sort_unstable();
        self.lru_head = states.first().copied();
        self.lru_tail = states.last().copied();
        for (index, state) in states.iter().copied().enumerate() {
            let entry = self.cache.get_mut(&state).expect("state came from cache");
            entry.previous = index.checked_sub(1).map(|previous| states[previous]);
            entry.next = states.get(index + 1).copied();
        }
    }

    fn clear_all_states(&mut self) {
        self.cache.clear();
        self.lifecycle.clear();
        self.transient_state = None;
        self.lru_head = None;
        self.lru_tail = None;
    }

    /// Get the underlying source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Replace the source and invalidate every entry atomically with respect to
    /// this uniquely borrowed wrapper.
    pub fn replace_source(&mut self, source: S) -> S {
        let previous = std::mem::replace(&mut self.source, source);
        self.rebind_snapshot(self.source.snapshot());
        previous
    }

    /// Take ownership of the source, discarding the cache.
    pub fn into_source(self) -> S {
        self.source
    }

    /// Snapshot to which the current lifecycle is bound.
    pub const fn current_snapshot(&self) -> SourceSnapshot {
        self.current_snapshot
    }

    /// Saturating attempt count for the active source snapshot.
    pub const fn total_attempts(&self) -> u64 {
        self.total_attempts
    }

    /// Refresh a changed source snapshot, invalidating all prior lifecycle data.
    pub fn refresh_snapshot(&mut self) -> bool {
        let snapshot = self.source.snapshot();
        if snapshot == self.current_snapshot {
            false
        } else {
            self.rebind_snapshot(snapshot);
            true
        }
    }

    /// Exact lifecycle status, rejecting stale observations.
    pub fn expansion_status(&self, state: StateId) -> Result<ExpansionStatus, ExpansionError> {
        self.check_snapshot()?;
        Ok(self.state_status_unchecked(state))
    }

    /// Retained state record, if one exists.
    pub fn lifecycle_state(
        &self,
        state: StateId,
    ) -> Result<Option<&LazyState<L, W>>, ExpansionError> {
        self.check_snapshot()?;
        Ok(self.state_entry(state))
    }

    /// Observe a fresh terminal result without conflating unexpanded and empty.
    pub fn observe(&self, state: StateId) -> Result<Option<ExpansionObservation>, ExpansionError> {
        Ok(self.expansion_status(state)?.observation())
    }

    /// Return transitions only for a fresh completed state.
    pub fn transitions_if_expanded(
        &self,
        state: StateId,
    ) -> Result<Option<&[WeightedTransition<L, W>]>, ExpansionError> {
        self.check_snapshot()?;
        Ok(self.state_entry(state).and_then(LazyState::transitions))
    }

    /// Normal expansion from the unexpanded phase.
    pub fn expand(&mut self, state: StateId) -> Result<ExpansionStatus, ExpansionError> {
        let cancellation = CancellationToken::new();
        self.expand_mode(state, ExpansionMode::Normal, &cancellation)
    }

    /// Normal expansion with cooperative cancellation.
    pub fn expand_with(
        &mut self,
        state: StateId,
        cancellation: &CancellationToken,
    ) -> Result<ExpansionStatus, ExpansionError> {
        self.expand_mode(state, ExpansionMode::Normal, cancellation)
    }

    /// Explicit retry from a retryable failure.
    pub fn retry(&mut self, state: StateId) -> Result<ExpansionStatus, ExpansionError> {
        let cancellation = CancellationToken::new();
        self.expand_mode(state, ExpansionMode::ExplicitRetry, &cancellation)
    }

    /// Explicit retry with cooperative cancellation.
    pub fn retry_with(
        &mut self,
        state: StateId,
        cancellation: &CancellationToken,
    ) -> Result<ExpansionStatus, ExpansionError> {
        self.expand_mode(state, ExpansionMode::ExplicitRetry, cancellation)
    }

    fn expand_mode(
        &mut self,
        state: StateId,
        mode: ExpansionMode,
        cancellation: &CancellationToken,
    ) -> Result<ExpansionStatus, ExpansionError> {
        self.check_snapshot()?;
        let entry = self.state_entry(state);
        let status = entry
            .map(LazyState::status)
            .unwrap_or(ExpansionStatus::Unexpanded);
        let retryable = entry.is_some_and(LazyState::is_retryable);
        if status.is_cacheable() {
            if matches!(self.policy, CachePolicy::Lru { .. }) {
                self.touch_lru(state);
            }
            return Ok(status);
        }
        if let Some(reason) = cancellation.reason() {
            if mode.authorizes_status(status, retryable) {
                self.store_lifecycle(state, LazyState::cancelled(reason, self.total_attempts));
                return Err(ExpansionError::Cancelled(reason));
            }
        }
        if !mode.is_authorized(status, retryable, false) {
            return Err(ExpansionError::Unauthorized {
                state,
                mode,
                status,
            });
        }

        self.total_attempts = self.total_attempts.saturating_add(1);
        self.record_computation();
        let attempt = self.total_attempts;
        let entry_snapshot = self.current_snapshot;
        self.store_lifecycle(state, LazyState::expanding(attempt));
        let request = ExpansionRequest {
            state,
            snapshot: entry_snapshot,
            attempt,
            cancellation,
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| self.source.compute_state(request)));
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(payload) => {
                let observed = self.source.snapshot();
                if observed == entry_snapshot {
                    self.clear_state(state);
                } else {
                    self.rebind_snapshot(observed);
                }
                resume_unwind(payload);
            }
        };

        let observed = self.source.snapshot();
        if observed != entry_snapshot {
            self.rebind_snapshot(observed);
            return Err(ExpansionError::StaleSnapshot {
                expected: entry_snapshot,
                observed,
            });
        }
        if let Some(reason) = cancellation.reason() {
            self.store_lifecycle(state, LazyState::cancelled(reason, attempt));
            return Err(ExpansionError::Cancelled(reason));
        }

        match outcome {
            StateExpansion::Expanded {
                is_final,
                final_weight,
                transitions,
            } => {
                let completed = LazyState::expanded(is_final, final_weight, transitions, attempt);
                let status = completed.status();
                self.insert_completed(state, completed);
                Ok(status)
            }
            StateExpansion::Failed(failure) => {
                self.store_lifecycle(state, LazyState::failed(failure.clone(), attempt));
                Err(ExpansionError::Failure(failure))
            }
            StateExpansion::Cancelled(reason) => {
                self.store_lifecycle(state, LazyState::cancelled(reason, attempt));
                Err(ExpansionError::Cancelled(reason))
            }
        }
    }

    /// Reset a cancelled state to unexpanded.
    pub fn reset_cancelled(&mut self, state: StateId) -> Result<(), ExpansionError> {
        self.check_snapshot()?;
        let status = self.state_status_unchecked(state);
        if status != ExpansionStatus::Cancelled {
            return Err(ExpansionError::Unauthorized {
                state,
                mode: ExpansionMode::Normal,
                status,
            });
        }
        self.clear_state(state);
        Ok(())
    }

    /// Reset a failed state to unexpanded, irrespective of retry policy.
    pub fn reset_failed(&mut self, state: StateId) -> Result<(), ExpansionError> {
        self.check_snapshot()?;
        let status = self.state_status_unchecked(state);
        if status != ExpansionStatus::Failed {
            return Err(ExpansionError::Unauthorized {
                state,
                mode: ExpansionMode::Normal,
                status,
            });
        }
        self.clear_state(state);
        Ok(())
    }

    /// Forget one state without changing attempt counters.
    pub fn clear_state(&mut self, state: StateId) {
        self.remove_cached(state);
        self.lifecycle.remove(&state);
        if self
            .transient_state
            .as_ref()
            .is_some_and(|(transient, _)| *transient == state)
        {
            self.transient_state = None;
        }
    }

    /// Fallibly get transitions, expanding exactly once when authorized.
    pub fn try_transitions_lazy(
        &mut self,
        state: StateId,
    ) -> Result<&[WeightedTransition<L, W>], ExpansionError> {
        self.expand(state)?;
        self.state_entry(state)
            .and_then(LazyState::transitions)
            .ok_or(ExpansionError::MissingCompletedState(state))
    }

    /// Get transitions, expanding exactly once or failing loudly.
    ///
    /// Use [`Self::try_transitions_lazy`] when failure or cancellation is an
    /// expected control-flow outcome.
    pub fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<L, W>] {
        self.try_transitions_lazy(state)
            .unwrap_or_else(|error| panic!("lazy transition expansion failed: {error}"))
    }

    fn require_expanded(&self, state: StateId) -> &LazyState<L, W> {
        if let Err(error) = self.check_snapshot() {
            panic!("stale lazy WFST observation: {error}");
        }
        match self.state_entry(state) {
            Some(state @ LazyState::Expanded { .. }) => state,
            Some(other) => panic!(
                "lazy WFST state {state} is {:?}; use expansion_status/expand before Wfst access",
                other.status()
            ),
            None => panic!(
                "lazy WFST state {state} is unexpanded; use expand or transitions_lazy first"
            ),
        }
    }
}

impl<S, L, W> Wfst<L, W> for LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    fn start(&self) -> StateId {
        self.start
    }

    fn is_final(&self, state: StateId) -> bool {
        self.require_expanded(state)
            .final_info()
            .expect("expanded state has final information")
            .0
    }

    fn final_weight(&self, state: StateId) -> W {
        self.require_expanded(state)
            .final_info()
            .expect("expanded state has final information")
            .1
    }

    fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>] {
        self.require_expanded(state)
            .transitions()
            .expect("expanded state has transitions")
    }

    fn num_states(&self) -> usize {
        self.source.num_states_hint().unwrap_or(0)
    }
}

impl<S, L, W> LazyWfst<L, W> for LazyWfstWrapper<S, L, W>
where
    S: StateSource<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    fn is_expanded(&self, state: StateId) -> bool {
        self.expansion_status(state)
            .unwrap_or_else(|error| panic!("stale lazy WFST status query: {error}"))
            .is_cacheable()
    }

    fn expand(&mut self, state: StateId) -> Result<ExpansionStatus, ExpansionError> {
        LazyWfstWrapper::expand(self, state)
    }

    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<L, W>] {
        LazyWfstWrapper::transitions_lazy(self, state)
    }

    fn cache_policy(&self) -> CachePolicy {
        self.policy
    }

    fn set_cache_policy(&mut self, policy: CachePolicy) {
        self.policy = policy;
        match policy {
            CachePolicy::NoCache | CachePolicy::Lru { max_states: 0 } => {
                self.cache.clear();
                self.lru_head = None;
                self.lru_tail = None;
                self.transient_state = None;
            }
            CachePolicy::CacheAll => {
                self.transient_state = None;
                self.lru_head = None;
                self.lru_tail = None;
                for entry in self.cache.values_mut() {
                    entry.previous = None;
                    entry.next = None;
                }
            }
            CachePolicy::Lru { max_states } => {
                self.transient_state = None;
                self.rebuild_lru();
                while self.cache.len() > max_states {
                    self.evict_lru();
                }
            }
        }
    }

    fn computed_states(&self) -> usize {
        self.computed_count
    }

    fn clear_cache(&mut self) {
        self.clear_all_states();
        // Don't reset computed_count - it tracks total ever computed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    /// Simple test source that generates a linear chain.
    #[derive(Clone)]
    struct LinearChainSource {
        num_states: usize,
    }

    #[derive(Clone)]
    struct PanicOnceSource {
        should_panic: Arc<AtomicBool>,
    }

    impl StateSource<char, TropicalWeight> for PanicOnceSource {
        fn compute_state(
            &self,
            request: ExpansionRequest<'_>,
        ) -> StateExpansion<char, TropicalWeight> {
            if request.state() != 0 {
                return StateExpansion::failed(ExpansionFailure::invalid_state(request.state()));
            }
            if self.should_panic.swap(false, Ordering::AcqRel) {
                panic!("deliberate source panic");
            }
            StateExpansion::non_final(SmallVec::new())
        }

        fn start(&self) -> StateId {
            0
        }

        fn num_states_hint(&self) -> Option<usize> {
            Some(1)
        }
    }

    #[derive(Clone)]
    struct BlockingSource {
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
    }

    impl StateSource<char, TropicalWeight> for BlockingSource {
        fn compute_state(
            &self,
            request: ExpansionRequest<'_>,
        ) -> StateExpansion<char, TropicalWeight> {
            self.entered.wait();
            self.release.wait();
            if let Some(reason) = request.cancellation().reason() {
                StateExpansion::cancelled(reason)
            } else {
                StateExpansion::non_final(SmallVec::new())
            }
        }

        fn start(&self) -> StateId {
            0
        }

        fn num_states_hint(&self) -> Option<usize> {
            Some(1)
        }
    }

    impl StateSource<char, TropicalWeight> for LinearChainSource {
        fn compute_state(
            &self,
            request: ExpansionRequest<'_>,
        ) -> StateExpansion<char, TropicalWeight> {
            let state = request.state();
            let state_idx = state as usize;

            if state_idx >= self.num_states {
                return StateExpansion::failed(ExpansionFailure::invalid_state(state));
            }

            let is_final = state_idx == self.num_states - 1;
            let mut transitions = SmallVec::new();

            if state_idx < self.num_states - 1 {
                transitions.push(WeightedTransition::new(
                    state,
                    Some('a'),
                    Some('a'),
                    state + 1,
                    TropicalWeight::new(1.0),
                ));
            }

            if is_final {
                StateExpansion::final_state(TropicalWeight::one(), transitions)
            } else {
                StateExpansion::non_final(transitions)
            }
        }

        fn start(&self) -> StateId {
            0
        }

        fn num_states_hint(&self) -> Option<usize> {
            Some(self.num_states)
        }
    }

    #[test]
    fn test_lazy_wrapper_basic() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::new(source);

        assert_eq!(lazy.start(), 0);
        assert_eq!(lazy.computed_states(), 0);

        // Access a state lazily
        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(lazy.computed_states(), 1);

        // Access another state
        let transitions = lazy.transitions_lazy(1);
        assert_eq!(transitions.len(), 1);
        assert_eq!(lazy.computed_states(), 2);

        // Final state
        lazy.expand(4).unwrap();
        assert!(lazy.is_expanded(4));
        assert_eq!(lazy.computed_states(), 3);
    }

    #[test]
    fn test_lru_eviction() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 3 });

        // Expand 5 states, should evict older ones
        for i in 0..5 {
            lazy.expand(i).unwrap();
        }

        // Only 3 should be cached
        assert_eq!(lazy.cache.len(), 3);

        // Most recent should still be cached
        assert!(lazy.is_expanded(4));
        assert!(lazy.is_expanded(3));
        assert!(lazy.is_expanded(2));

        // Oldest should be evicted
        assert!(!lazy.is_expanded(0));
        assert!(!lazy.is_expanded(1));
    }

    #[test]
    fn test_lru_policy_reconciles_existing_cache_on_policy_change() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy = LazyWfstWrapper::new(source);

        lazy.expand(0).unwrap();
        lazy.expand(1).unwrap();
        lazy.expand(2).unwrap();
        assert_eq!(lazy.cache.len(), 3);
        assert_eq!(lazy.lru_head, None);
        assert_eq!(lazy.lru_tail, None);

        lazy.set_cache_policy(CachePolicy::Lru { max_states: 2 });

        assert_eq!(lazy.cache.len(), 2);
        assert!(lazy.lru_head.is_some());
        assert!(lazy.lru_tail.is_some());

        lazy.expand(3).unwrap();

        assert_eq!(lazy.cache.len(), 2);
        assert!(lazy.lru_head.is_some());
        assert_eq!(lazy.lru_tail, Some(3));
        assert!(lazy.is_expanded(3));
    }

    #[test]
    fn test_lru_hit_moves_entry_to_tail_in_constant_time() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 2 });

        lazy.expand(0).unwrap();
        lazy.expand(1).unwrap();
        assert_eq!(lazy.lru_head, Some(0));
        assert_eq!(lazy.lru_tail, Some(1));

        let transitions = lazy.transitions_lazy(0);

        assert_eq!(transitions.len(), 1);
        assert_eq!(lazy.cache.len(), 2);
        assert_eq!(lazy.lru_head, Some(1));
        assert_eq!(lazy.lru_tail, Some(0));
    }

    #[test]
    fn test_lru_eviction_preserves_capacity_after_hits() {
        let source = LinearChainSource { num_states: 10 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 2 });

        lazy.expand(0).unwrap();
        lazy.expand(1).unwrap();
        lazy.expand(0).unwrap();

        lazy.expand(2).unwrap();

        assert_eq!(lazy.cache.len(), 2);
        assert!(lazy.is_expanded(0));
        assert!(!lazy.is_expanded(1));
        assert!(lazy.is_expanded(2));
    }

    #[test]
    fn test_no_cache_policy_returns_transitions_without_retaining_state() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::with_cache_policy(source, CachePolicy::NoCache);

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));

        let transitions = lazy.transitions_lazy(1);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 2);
        assert_eq!(lazy.computed_states(), 2);
        assert_eq!(lazy.cache.len(), 0);
        assert!(!lazy.is_expanded(0));
        assert!(lazy.is_expanded(1));
    }

    #[test]
    fn test_zero_capacity_lru_uses_transient_state() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy =
            LazyWfstWrapper::with_cache_policy(source, CachePolicy::Lru { max_states: 0 });

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));

        let transitions = lazy.transitions_lazy(0);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].to, 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));
    }

    #[test]
    fn test_switching_to_no_cache_discards_persistent_cache() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::new(source);

        assert_eq!(lazy.transitions_lazy(0).len(), 1);
        assert_eq!(lazy.computed_states(), 1);
        assert_eq!(lazy.cache.len(), 1);

        lazy.set_cache_policy(CachePolicy::NoCache);
        assert_eq!(lazy.cache.len(), 0);

        assert_eq!(lazy.transitions_lazy(0).len(), 1);
        assert_eq!(lazy.computed_states(), 2);
        assert_eq!(lazy.cache.len(), 0);
        assert!(lazy.is_expanded(0));
    }

    #[test]
    fn test_clear_cache() {
        let source = LinearChainSource { num_states: 5 };
        let mut lazy = LazyWfstWrapper::new(source);

        lazy.expand(0).unwrap();
        lazy.expand(1).unwrap();
        lazy.expand(2).unwrap();

        assert_eq!(lazy.cache.len(), 3);
        assert_eq!(lazy.computed_states(), 3);

        lazy.clear_cache();

        assert_eq!(lazy.cache.len(), 0);
        // computed_states tracks total ever computed
        assert_eq!(lazy.computed_states(), 3);
    }

    #[test]
    fn test_source_panic_rolls_back_unique_owner() {
        let source = PanicOnceSource {
            should_panic: Arc::new(AtomicBool::new(true)),
        };
        let mut lazy = LazyWfstWrapper::new(source);

        let panic = catch_unwind(AssertUnwindSafe(|| lazy.expand(0)));
        assert!(panic.is_err());
        assert_eq!(
            lazy.expansion_status(0).unwrap(),
            ExpansionStatus::Unexpanded
        );
        assert_eq!(lazy.total_attempts(), 1);

        assert_eq!(lazy.expand(0).unwrap(), ExpansionStatus::ExpandedEmpty);
        assert_eq!(lazy.total_attempts(), 2);
    }

    #[test]
    fn test_cancellation_token_has_one_concurrent_winner() {
        let token = CancellationToken::new();
        let start = Arc::new(Barrier::new(9));
        let reasons = [
            CancellationReason::Requested,
            CancellationReason::Deadline,
            CancellationReason::Budget,
            CancellationReason::Source,
        ];
        let mut workers = Vec::with_capacity(8);
        for index in 0..8 {
            let token = token.clone();
            let start = start.clone();
            workers.push(thread::spawn(move || {
                start.wait();
                token.cancel(reasons[index % reasons.len()])
            }));
        }
        start.wait();
        let winners = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|won| *won)
            .count();

        assert_eq!(winners, 1);
        assert!(token.reason().is_some());
    }

    #[test]
    fn test_concurrent_cancellation_during_source_call_is_terminal() {
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let source = BlockingSource {
            entered: entered.clone(),
            release: release.clone(),
        };
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker = thread::spawn(move || {
            let mut lazy = LazyWfstWrapper::new(source);
            let result = lazy.expand_with(0, &worker_token);
            (lazy, result)
        });

        entered.wait();
        assert!(token.cancel(CancellationReason::Requested));
        release.wait();
        let (lazy, result) = worker.join().unwrap();

        assert_eq!(
            result,
            Err(ExpansionError::Cancelled(CancellationReason::Requested))
        );
        assert_eq!(
            lazy.expansion_status(0).unwrap(),
            ExpansionStatus::Cancelled
        );
        assert_eq!(lazy.total_attempts(), 1);
    }

    #[test]
    fn test_independent_wrappers_expand_in_parallel() {
        let mut workers = Vec::with_capacity(8);
        for _ in 0..8 {
            workers.push(thread::spawn(|| {
                let mut lazy = LazyWfstWrapper::new(LinearChainSource { num_states: 2 });
                assert_eq!(lazy.expand(0).unwrap(), ExpansionStatus::ExpandedNonempty);
                lazy.transitions_lazy(0)[0].to
            }));
        }
        for worker in workers {
            assert_eq!(worker.join().unwrap(), 1);
        }
    }

    #[test]
    fn test_attempt_counter_saturates_without_wrapping() {
        let mut lazy = LazyWfstWrapper::new(LinearChainSource { num_states: 1 });
        lazy.total_attempts = u64::MAX;

        assert_eq!(lazy.expand(0).unwrap(), ExpansionStatus::ExpandedEmpty);
        assert_eq!(lazy.total_attempts(), u64::MAX);
        assert!(matches!(
            lazy.lifecycle_state(0).unwrap(),
            Some(LazyState::Expanded {
                attempt: u64::MAX,
                ..
            })
        ));
    }
}
