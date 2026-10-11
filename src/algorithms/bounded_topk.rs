//! Exact, resumable top-k accepting paths for nonnegative tropical WFSTs.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::mem::size_of;

use crate::semiring::{Semiring, TropicalWeight};
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, SourceSnapshot, StateId, Wfst, NO_STATE};

use super::{ShortestWitness, ShortestWitnessError, ShortestWitnessStep};

const ALGORITHM_ID: &str = "lling.topk.nonnegative-tropical-witness/v1";

/// Absolute bounds on path depth, emitted paths, and pending candidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopKLimits {
    /// Largest admitted number of arcs in one path.
    pub max_depth: usize,
    /// Largest number of accepting paths emitted before suspension.
    pub max_paths: usize,
    /// Largest number of pending prefix and terminal candidates.
    pub max_frontier: usize,
}

impl Default for TopKLimits {
    fn default() -> Self {
        Self {
            max_depth: usize::MAX,
            max_paths: usize::MAX,
            max_frontier: usize::MAX,
        }
    }
}

#[derive(Clone, Copy)]
struct PathNode {
    parent: Option<usize>,
    from: StateId,
    arc_index: usize,
    state: StateId,
    depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Candidate {
    cost: TropicalWeight,
    depth: usize,
    order: u64,
    node: usize,
    terminal: bool,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.depth.cmp(&self.depth))
            .then_with(|| other.order.cmp(&self.order))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A best-first continuation. Prefix cost lower-bounds every completion;
/// equal-cost depth/order ties make zero-cost cycles fair at finite depth.
/// A finite `max_paths` does not claim language exhaustion while work remains.
pub struct BoundedTopK<F, L>
where
    F: Wfst<L, TropicalWeight>,
    L: Clone + Send + Sync,
{
    source: F,
    frontier: BinaryHeap<Candidate>,
    arena: Vec<PathNode>,
    emitted: Vec<ShortestWitness<L>>,
    next_order: u64,
    path_limits: TopKLimits,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<F, L> BoundedTopK<F, L>
where
    F: Wfst<L, TropicalWeight>,
    L: Clone + Send + Sync,
{
    /// Bind a finite source and caller-computed nonzero content digest.
    ///
    /// # Errors
    ///
    /// Rejects invalid start/count or absent source binding.
    pub fn new(
        source: F,
        source_binding: [u8; 32],
        limits: OperationLimits,
        path_limits: TopKLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, ShortestWitnessError> {
        if source.num_states() >= u32::MAX as usize {
            return Err(ShortestWitnessError::SourceTooLarge);
        }
        let start = source.start();
        if (source.num_states() == 0 && start != NO_STATE)
            || (source.num_states() > 0 && !source.is_valid_state(start))
        {
            return Err(ShortestWitnessError::InvalidStart);
        }
        let plan =
            OperationPlan::new_dynamic(SourceSnapshot::IMMUTABLE, source_binding, ALGORITHM_ID)?;
        let mut frontier = BinaryHeap::new();
        let mut arena = Vec::new();
        if source.num_states() > 0 {
            arena.push(PathNode {
                parent: None,
                from: start,
                arc_index: 0,
                state: start,
                depth: 0,
            });
            frontier.push(Candidate {
                cost: TropicalWeight::one(),
                depth: 0,
                order: 0,
                node: 0,
                terminal: false,
            });
        }
        Ok(Self {
            source,
            frontier,
            arena,
            emitted: Vec::new(),
            next_order: 1,
            path_limits,
            session: OperationSession::new(plan, limits, cancellation),
            last_checkpoint: None,
        })
    }

    /// Resume the exact last checkpoint of this live continuation. Raised
    /// limits retain all pending candidates and previously emitted paths.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        path_limits: TopKLimits,
        cancellation: CancellationToken,
    ) -> Result<(), ShortestWitnessError> {
        if self.last_checkpoint != Some(checkpoint) {
            return Err(OperationError::StaleCheckpoint.into());
        }
        self.session = OperationSession::resume(
            self.session.plan().clone(),
            limits,
            cancellation,
            checkpoint,
        )?;
        self.path_limits = path_limits;
        self.last_checkpoint = None;
        Ok(())
    }

    /// Emit sorted accepting paths until exact exhaustion or a typed bound.
    /// `label_meter` reports the heap footprint of each copied label.
    ///
    /// # Errors
    ///
    /// Refuses source drift, malformed reachable arcs, unsupported reachable
    /// weights, and arithmetic or indexing overflow.
    pub fn run<M>(
        &mut self,
        observed_source_binding: [u8; 32],
        label_meter: M,
    ) -> Result<OperationOutcome<Vec<ShortestWitness<L>>>, ShortestWitnessError>
    where
        M: Fn(&L) -> u64,
    {
        if observed_source_binding != self.session.plan().source_binding {
            return Err(OperationError::StaleSource.into());
        }
        loop {
            let Some(candidate) = self.frontier.peek().copied() else {
                return Ok(self.complete());
            };
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            if self.emitted.len() >= self.path_limits.max_paths {
                return Ok(self.incomplete(IncompleteReason::PathLimit));
            }
            if self.frontier.len() > self.path_limits.max_frontier {
                return Ok(self.incomplete(IncompleteReason::FrontierLimit));
            }
            if candidate.terminal {
                if let Err(reason) = self.emit(candidate, &label_meter)? {
                    return Ok(self.incomplete(reason));
                }
            } else if let Err(reason) = self.expand(candidate)? {
                return Ok(self.incomplete(reason));
            }
        }
    }

    fn expand(
        &mut self,
        candidate: Candidate,
    ) -> Result<Result<(), IncompleteReason>, ShortestWitnessError> {
        let path = self.arena[candidate.node];
        let arcs = self.source.transitions(path.state);
        if path.depth >= self.path_limits.max_depth && !arcs.is_empty() {
            return Ok(Err(IncompleteReason::DepthLimit));
        }
        if let Err(reason) = self.session.preview_dynamic(OperationCost {
            states: 1,
            work: 1,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let mut children = Vec::new();
        for (index, arc) in arcs.iter().enumerate() {
            if !self.source.is_valid_state(arc.to) {
                return Err(ShortestWitnessError::InvalidTarget {
                    state: path.state,
                    arc_index: index,
                });
            }
            let weight = arc.weight.value();
            if weight.is_nan()
                || weight < 0.0
                || (weight.is_infinite() && weight.is_sign_negative())
            {
                return Err(ShortestWitnessError::UnsupportedWeight {
                    state: path.state,
                    arc_index: Some(index),
                });
            }
            if weight.is_infinite() {
                continue;
            }
            let cost = candidate.cost.value() + weight;
            if !cost.is_finite() {
                return Err(ShortestWitnessError::ExhaustedRepresentation);
            }
            children.push((index, arc.to, TropicalWeight::new(cost)));
        }
        let terminal = if self.source.is_final(path.state) {
            let weight = self.source.final_weight(path.state).value();
            if weight.is_nan()
                || weight < 0.0
                || (weight.is_infinite() && weight.is_sign_negative())
            {
                return Err(ShortestWitnessError::UnsupportedWeight {
                    state: path.state,
                    arc_index: None,
                });
            }
            if weight.is_finite() {
                let cost = candidate.cost.value() + weight;
                if !cost.is_finite() {
                    return Err(ShortestWitnessError::ExhaustedRepresentation);
                }
                Some(TropicalWeight::new(cost))
            } else {
                None
            }
        } else {
            None
        };
        let new_count = children
            .len()
            .checked_add(usize::from(terminal.is_some()))
            .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
        let next_frontier = self
            .frontier
            .len()
            .checked_sub(1)
            .and_then(|n| n.checked_add(new_count))
            .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
        if next_frontier > self.path_limits.max_frontier {
            return Ok(Err(IncompleteReason::FrontierLimit));
        }
        let new_count_u64 =
            u64::try_from(new_count).map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
        if self.next_order.checked_add(new_count_u64).is_none()
            || self.arena.len().checked_add(children.len()).is_none()
            || path.depth.checked_add(1).is_none()
        {
            return Err(ShortestWitnessError::ExhaustedRepresentation);
        }
        if let Err(reason) = self.session.poll() {
            return Ok(Err(reason));
        }
        let arc_count =
            u64::try_from(arcs.len()).map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
        let work = arc_count
            .checked_add(new_count_u64)
            .and_then(|n| n.checked_add(1))
            .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
        let heap = (children.len() as u128)
            .checked_mul(size_of::<PathNode>() as u128)
            .and_then(|n| {
                n.checked_add((new_count as u128).checked_mul(size_of::<Candidate>() as u128)?)
            })
            .and_then(|n| {
                n.checked_add(
                    u128::from(self.session.usage().states == 0)
                        * (size_of::<PathNode>() + size_of::<Candidate>()) as u128,
                )
            })
            .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
        let cost = OperationCost {
            states: 1,
            arcs: arc_count,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?,
        };
        if let Err(reason) = self.session.advance_dynamic(cost)? {
            return Ok(Err(reason));
        }
        self.frontier.pop();
        if let Some(cost) = terminal {
            let order = self.next_order;
            self.next_order += 1;
            self.frontier.push(Candidate {
                cost,
                depth: path.depth,
                order,
                node: candidate.node,
                terminal: true,
            });
        }
        for (arc_index, state, cost) in children {
            let node = self.arena.len();
            self.arena.push(PathNode {
                parent: Some(candidate.node),
                from: path.state,
                arc_index,
                state,
                depth: path.depth + 1,
            });
            let order = self.next_order;
            self.next_order += 1;
            self.frontier.push(Candidate {
                cost,
                depth: path.depth + 1,
                order,
                node,
                terminal: false,
            });
        }
        Ok(Ok(()))
    }

    fn emit<M>(
        &mut self,
        candidate: Candidate,
        label_meter: &M,
    ) -> Result<Result<(), IncompleteReason>, ShortestWitnessError>
    where
        M: Fn(&L) -> u64,
    {
        let mut reverse = Vec::new();
        let mut node = candidate.node;
        while let Some(parent) = self.arena[node].parent {
            if reverse.len() >= self.arena.len() || parent >= node {
                return Err(ShortestWitnessError::ExhaustedRepresentation);
            }
            reverse.push(node);
            node = parent;
        }
        reverse.reverse();
        let mut heap = (reverse.len() as u128)
            .checked_mul(size_of::<ShortestWitnessStep<L>>() as u128)
            .and_then(|n| n.checked_add(size_of::<ShortestWitness<L>>() as u128))
            .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
        for &node in &reverse {
            let path = self.arena[node];
            let arc = self
                .source
                .transitions(path.from)
                .get(path.arc_index)
                .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
            if arc.to != path.state {
                return Err(ShortestWitnessError::ExhaustedRepresentation);
            }
            for label in [arc.input.as_ref(), arc.output.as_ref()]
                .into_iter()
                .flatten()
            {
                heap = heap
                    .checked_add(u128::from(label_meter(label)))
                    .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
            }
        }
        let work = u64::try_from(reverse.len())
            .map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work: work
                .checked_add(1)
                .ok_or(ShortestWitnessError::ExhaustedRepresentation)?,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        let steps = reverse
            .into_iter()
            .map(|node| {
                let path = self.arena[node];
                let arc = &self.source.transitions(path.from)[path.arc_index];
                ShortestWitnessStep {
                    from: path.from,
                    arc_index: path.arc_index,
                    input: arc.input.clone(),
                    output: arc.output.clone(),
                    to: path.state,
                    weight: arc.weight,
                }
            })
            .collect();
        let state = self.arena[candidate.node].state;
        self.emitted.push(ShortestWitness {
            steps,
            final_state: state,
            final_weight: self.source.final_weight(state),
            total_weight: candidate.cost,
        });
        self.frontier.pop();
        Ok(Ok(()))
    }

    fn incomplete(
        &mut self,
        reason: IncompleteReason,
    ) -> OperationOutcome<Vec<ShortestWitness<L>>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: self.emitted.clone(),
            reason,
            checkpoint,
        }
    }

    fn complete(&mut self) -> OperationOutcome<Vec<ShortestWitness<L>>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Complete {
            value: self.emitted.clone(),
            checkpoint,
        }
    }
}
