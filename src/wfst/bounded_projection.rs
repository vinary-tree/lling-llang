//! Reachable, bounded materialization of the existing lazy projection source.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::mem::size_of;

use crate::semiring::Semiring;
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};

use super::{
    CancellationToken, ExpansionError, ExpansionStatus, LazyState, LazyWfstWrapper, MutableWfst,
    ProjectSource, SourceSnapshot, StateId, VectorWfst, WeightedTransition, Wfst, NO_STATE,
};

/// Input or operation-contract failure, never silent truncation.
#[derive(Debug)]
pub enum ProjectionError {
    /// Source start state is invalid.
    InvalidStart,
    /// A reachable transition points outside the source state space.
    InvalidTarget {
        /// Source state containing the invalid arc.
        state: StateId,
        /// Index of the arc in source order.
        arc_index: usize,
    },
    /// Source has more states than its state-ID type can represent.
    SourceTooLarge,
    /// Shared operation-contract or source expansion error.
    Operation(OperationError),
}

impl From<OperationError> for ProjectionError {
    fn from(error: OperationError) -> Self {
        Self::Operation(error)
    }
}

impl From<ExpansionError> for ProjectionError {
    fn from(error: ExpansionError) -> Self {
        Self::Operation(OperationError::Expansion(error))
    }
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProjectionError {}

/// One in-memory FIFO continuation. `INPUT=true` keeps input labels;
/// `INPUT=false` keeps output labels. Both become acceptor labels on output.
pub struct BoundedProjection<T, L, W, const INPUT: bool>
where
    T: Wfst<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    wrapper: LazyWfstWrapper<ProjectSource<L, W, T, INPUT>, L, W>,
    source_states: usize,
    result: VectorWfst<L, W>,
    state_map: HashMap<StateId, StateId>,
    frontier: VecDeque<StateId>,
    session: OperationSession,
    cancellation: CancellationToken,
    last_checkpoint: Option<OperationCheckpoint>,
}

/// Bounded projection retaining source input labels.
pub type BoundedInputProjection<T, L, W> = BoundedProjection<T, L, W, true>;
/// Bounded projection retaining source output labels.
pub type BoundedOutputProjection<T, L, W> = BoundedProjection<T, L, W, false>;

impl<T, L, W, const INPUT: bool> BoundedProjection<T, L, W, INPUT>
where
    T: Wfst<L, W>,
    L: Clone + Send + Sync,
    W: Semiring,
{
    /// Bind a source-content digest and shared limits to one projection
    /// direction. The digest must include every relevant source label and
    /// weight; direction is part of the versioned operation identity.
    ///
    /// # Errors
    ///
    /// Rejects invalid start/state count or absent content binding.
    pub fn new(
        source: T,
        source_binding: [u8; 32],
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, ProjectionError> {
        let source_states = source.num_states();
        if source_states >= u32::MAX as usize {
            return Err(ProjectionError::SourceTooLarge);
        }
        let start = source.start();
        if (source_states == 0 && start != NO_STATE)
            || (source_states > 0 && !source.is_valid_state(start))
        {
            return Err(ProjectionError::InvalidStart);
        }
        let algorithm_id = if INPUT {
            "lling.projection.input-reachable/v1"
        } else {
            "lling.projection.output-reachable/v1"
        };
        let plan =
            OperationPlan::new_dynamic(SourceSnapshot::IMMUTABLE, source_binding, algorithm_id)?;
        let session = OperationSession::new(plan, limits, cancellation.clone());
        let wrapper = LazyWfstWrapper::new(ProjectSource::new(source));
        let mut result = VectorWfst::new();
        let mut state_map = HashMap::new();
        let mut frontier = VecDeque::new();
        if source_states > 0 {
            let start_id = result.add_state();
            result.set_start(start_id);
            state_map.insert(start, start_id);
            frontier.push_back(start);
        }
        Ok(Self {
            wrapper,
            source_states,
            result,
            state_map,
            frontier,
            session,
            cancellation,
            last_checkpoint: None,
        })
    }

    /// Resume the exact checkpoint last returned by this live continuation.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<(), ProjectionError> {
        if self.last_checkpoint != Some(checkpoint) {
            return Err(OperationError::StaleCheckpoint.into());
        }
        self.session = OperationSession::resume(
            self.session.plan().clone(),
            limits,
            cancellation.clone(),
            checkpoint,
        )?;
        self.cancellation = cancellation;
        self.last_checkpoint = None;
        Ok(())
    }

    /// Expand reachable source states in FIFO order, preserving source arc
    /// order and weights. `meter` covers label/weight-owned logical heap
    /// bytes retained in one lazy state; structural output bytes are charged
    /// here. This is not a hard RSS or transient-allocation cap.
    ///
    /// # Errors
    ///
    /// Refuses source drift, invalid targets or expansion errors.
    pub fn run<M>(
        &mut self,
        observed_source_binding: [u8; 32],
        meter: M,
    ) -> Result<OperationOutcome<VectorWfst<L, W>>, ProjectionError>
    where
        M: Fn(&LazyState<L, W>) -> u64,
    {
        if observed_source_binding != self.session.plan().source_binding {
            return Err(OperationError::StaleSource.into());
        }
        while let Some(&source_state) = self.frontier.front() {
            let minimum = OperationCost {
                states: 1,
                work: 1,
                ..OperationCost::default()
            };
            if let Err(reason) = self.session.preview_dynamic(minimum)? {
                return Ok(self.incomplete(reason));
            }
            let before = self.wrapper.expansion_status(source_state)?;
            if before == ExpansionStatus::Cancelled {
                self.wrapper.reset_cancelled(source_state)?;
            }
            let newly_computed = !before.is_cacheable();
            match self.wrapper.expand_with(source_state, &self.cancellation) {
                Ok(_) => {}
                Err(ExpansionError::Cancelled(_)) => {
                    if self.wrapper.expansion_status(source_state)? == ExpansionStatus::Cancelled {
                        self.wrapper.reset_cancelled(source_state)?;
                    }
                    return Ok(self.incomplete(IncompleteReason::Cancelled));
                }
                Err(error) => return Err(error.into()),
            }
            if let Err(reason) = self.session.poll() {
                if newly_computed {
                    self.wrapper.clear_state(source_state);
                }
                return Ok(self.incomplete(reason));
            }
            let state = self
                .wrapper
                .lifecycle_state(source_state)?
                .ok_or(OperationError::MissingState)?;
            let LazyState::Expanded {
                is_final,
                final_weight,
                transitions,
                ..
            } = state
            else {
                return Err(OperationError::MissingState.into());
            };
            let is_final = *is_final;
            let final_weight = *final_weight;
            let transitions = transitions.clone();
            let mut new_targets = HashSet::new();
            for (arc_index, arc) in transitions.iter().enumerate() {
                if arc.to as usize >= self.source_states {
                    if newly_computed {
                        self.wrapper.clear_state(source_state);
                    }
                    return Err(ProjectionError::InvalidTarget {
                        state: source_state,
                        arc_index,
                    });
                }
                if !self.state_map.contains_key(&arc.to) {
                    new_targets.insert(arc.to);
                }
            }
            if self
                .state_map
                .len()
                .checked_add(new_targets.len())
                .is_none_or(|n| n >= u32::MAX as usize)
            {
                return Err(OperationError::InvalidCost.into());
            }
            let arcs = u64::try_from(transitions.len()).map_err(|_| OperationError::InvalidCost)?;
            let structural = (transitions.len() as u128)
                .checked_mul(size_of::<WeightedTransition<L, W>>() as u128)
                .and_then(|n| {
                    n.checked_add(
                        (new_targets.len() as u128 + u128::from(self.session.usage().states == 0))
                            * (2 * size_of::<StateId>()) as u128,
                    )
                })
                .ok_or(OperationError::InvalidCost)?;
            let heap_bytes = u64::try_from(structural)
                .ok()
                .and_then(|n| n.checked_add(meter(state)))
                .ok_or(OperationError::InvalidCost)?;
            let cost = OperationCost {
                states: 1,
                arcs,
                work: arcs.checked_add(1).ok_or(OperationError::InvalidCost)?,
                heap_bytes,
            };
            if let Err(reason) = self.session.advance_dynamic(cost)? {
                if newly_computed {
                    self.wrapper.clear_state(source_state);
                }
                return Ok(self.incomplete(reason));
            }
            let current_id = self.state_map[&source_state];
            if is_final {
                self.result.set_final(current_id, final_weight);
            }
            self.result
                .reserve_transitions(current_id, transitions.len());
            for arc in transitions {
                let target_id = if let Some(&id) = self.state_map.get(&arc.to) {
                    id
                } else {
                    let id = self.result.add_state();
                    self.state_map.insert(arc.to, id);
                    self.frontier.push_back(arc.to);
                    id
                };
                self.result
                    .add_arc(current_id, arc.input, arc.output, target_id, arc.weight);
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
