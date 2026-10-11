//! Bounded, resumable breadth-first materialization of a lazy composition.

use std::collections::VecDeque;
use std::hash::Hash;
use std::mem::size_of;

use rustc_hash::{FxHashMap, FxHashSet};

use super::fst_fst::{ComposedTransition, LazyComposition, ProductStateId};
use crate::semiring::Semiring;
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, MutableWfst, SourceSnapshot, StateId, VectorWfst, Wfst};

const ALGORITHM_ID: &str = "lling.composition.materialize-bfs/v1";

/// One in-memory continuation. It owns the exact frontier and partial graph;
/// an [`OperationCheckpoint`] alone is deliberately not a serialized frontier.
pub struct BoundedComposition<F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring,
{
    lazy: LazyComposition<F1, F2, L, W>,
    result: VectorWfst<L, W>,
    state_map: FxHashMap<ProductStateId, StateId>,
    frontier: VecDeque<ProductStateId>,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<F1, F2, L, W> BoundedComposition<F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring,
{
    /// Create a deterministic traversal. `source_binding` must be a nonzero
    /// digest of both operand contents, filter semantics, and their order.
    /// The caller must recompute it at every run/resume boundary.
    ///
    /// # Errors
    ///
    /// Rejects an absent content binding.
    pub fn new(
        lazy: LazyComposition<F1, F2, L, W>,
        source_binding: [u8; 32],
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, OperationError> {
        let plan =
            OperationPlan::new_dynamic(SourceSnapshot::IMMUTABLE, source_binding, ALGORITHM_ID)?;
        let session = OperationSession::new(plan, limits, cancellation);
        let mut result = VectorWfst::new();
        let start = lazy.start();
        let start_id = result.add_state();
        result.set_start(start_id);
        let mut state_map = FxHashMap::default();
        state_map.insert(start, start_id);
        let mut frontier = VecDeque::new();
        frontier.push_back(start);
        Ok(Self {
            lazy,
            result,
            state_map,
            frontier,
            session,
            last_checkpoint: None,
        })
    }

    /// Resume only the exact checkpoint last returned by this continuation.
    /// Raising limits is allowed; spent work and elapsed time are retained.
    ///
    /// # Errors
    ///
    /// Refuses a stale or independently fabricated checkpoint.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<(), OperationError> {
        if self.last_checkpoint != Some(checkpoint) {
            return Err(OperationError::StaleCheckpoint);
        }
        let next = OperationSession::resume(
            self.session.plan().clone(),
            limits,
            cancellation,
            checkpoint,
        )?;
        self.session = next;
        self.last_checkpoint = None;
        Ok(())
    }

    /// Materialize until the frontier is empty or a limit/cancellation fires.
    /// Each expanded product state is committed atomically in source transition
    /// order. `meter` returns the logical heap bytes owned by its labels and
    /// weights; structural graph/frontier bytes are charged here. It is the
    /// caller's responsibility to account for any additional adapter storage.
    ///
    /// # Errors
    ///
    /// Refuses changed source identity or arithmetic/state-ID exhaustion.
    pub fn run<M>(
        &mut self,
        observed_source_binding: [u8; 32],
        meter: M,
    ) -> Result<OperationOutcome<VectorWfst<L, W>>, OperationError>
    where
        M: Fn(&[ComposedTransition<L, W>]) -> u64,
    {
        if observed_source_binding != self.session.plan().source_binding {
            return Err(OperationError::StaleSource);
        }
        while let Some(&product) = self.frontier.front() {
            let minimum = OperationCost {
                states: 1,
                work: 1,
                ..OperationCost::default()
            };
            if let Err(reason) = self.session.preview_dynamic(minimum)? {
                return Ok(self.incomplete(reason));
            }
            let final_weight = self.lazy.final_weight(product);
            let transitions = self.lazy.transitions(product);
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            let mut new_targets = FxHashSet::default();
            for transition in &transitions {
                if !self.state_map.contains_key(&transition.target) {
                    new_targets.insert(transition.target);
                }
            }
            // `u32::MAX` is the invalid-state sentinel. Never wrap a result ID.
            if self
                .state_map
                .len()
                .checked_add(new_targets.len())
                .is_none_or(|n| n >= u32::MAX as usize)
            {
                return Err(OperationError::InvalidCost);
            }
            let arcs = u64::try_from(transitions.len()).map_err(|_| OperationError::InvalidCost)?;
            let structural = (transitions.len() as u128)
                .checked_mul(size_of::<ComposedTransition<L, W>>() as u128)
                .and_then(|n| {
                    n.checked_add(
                        (new_targets.len() as u128 + u128::from(self.session.usage().states == 0))
                            * (size_of::<ProductStateId>() + size_of::<StateId>()) as u128,
                    )
                })
                .ok_or(OperationError::InvalidCost)?;
            let heap_bytes = u64::try_from(structural)
                .ok()
                .and_then(|n| n.checked_add(meter(&transitions)))
                .ok_or(OperationError::InvalidCost)?;
            let cost = OperationCost {
                states: 1,
                arcs,
                work: arcs.checked_add(1).ok_or(OperationError::InvalidCost)?,
                heap_bytes,
            };
            if let Err(reason) = self.session.advance_dynamic(cost)? {
                return Ok(self.incomplete(reason));
            }
            let current_id = self.state_map[&product];
            if !final_weight.is_zero() {
                self.result.set_final(current_id, final_weight);
            }
            self.result
                .reserve_transitions(current_id, transitions.len());
            for transition in transitions {
                let target_id = if let Some(&id) = self.state_map.get(&transition.target) {
                    id
                } else {
                    let id = self.result.add_state();
                    self.state_map.insert(transition.target, id);
                    self.frontier.push_back(transition.target);
                    id
                };
                self.result.add_arc(
                    current_id,
                    transition.input,
                    transition.output,
                    target_id,
                    transition.weight,
                );
            }
            self.frontier.pop_front();
        }
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        Ok(OperationOutcome::Complete {
            value: self.result.clone(),
            checkpoint,
        })
    }

    fn incomplete(&mut self, reason: IncompleteReason) -> OperationOutcome<VectorWfst<L, W>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: self.result.clone(),
            reason,
            checkpoint,
        }
    }
}
