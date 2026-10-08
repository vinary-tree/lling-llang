//! Bounded shortest accepting witness for nonnegative tropical WFSTs.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;
use std::mem::size_of;

use crate::semiring::{Semiring, TropicalWeight};
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, SourceSnapshot, StateId, Wfst, NO_STATE};

const ALGORITHM_ID: &str = "lling.shortest.nonnegative-tropical-witness/v1";

/// One source arc in a complete witness, including its ordered provenance.
#[derive(Clone, Debug)]
pub struct ShortestWitnessStep<L> {
    /// Source state ID.
    pub from: StateId,
    /// Position in that state's ordered arc list.
    pub arc_index: usize,
    /// Input label, or epsilon.
    pub input: Option<L>,
    /// Output label, or epsilon.
    pub output: Option<L>,
    /// Target state ID.
    pub to: StateId,
    /// Original arc weight.
    pub weight: TropicalWeight,
}

/// Exact best accepting path and its terminal weight.
#[derive(Clone, Debug)]
pub struct ShortestWitness<L> {
    /// Source-ordered, complete arc provenance.
    pub steps: Vec<ShortestWitnessStep<L>>,
    /// Last source state.
    pub final_state: StateId,
    /// Original final weight.
    pub final_weight: TropicalWeight,
    /// Arc weights plus final weight.
    pub total_weight: TropicalWeight,
}

/// Rejection outside the exact supported domain.
#[derive(Debug)]
pub enum ShortestWitnessError {
    /// No valid start state exists.
    InvalidStart,
    /// State count is not representable by `StateId`.
    SourceTooLarge,
    /// A reachable arc has an invalid target.
    InvalidTarget {
        /// Source state containing the invalid arc.
        state: StateId,
        /// Index in the state's ordered arc list.
        arc_index: usize,
    },
    /// A reachable arc or final weight is negative or not in the tropical domain.
    UnsupportedWeight {
        /// Source state containing the weight.
        state: StateId,
        /// Arc index, or `None` for the state's final weight.
        arc_index: Option<usize>,
    },
    /// A finite path cost overflowed or the path representation was exhausted.
    ExhaustedRepresentation,
    /// Shared operation-contract error.
    Operation(OperationError),
}

impl From<OperationError> for ShortestWitnessError {
    fn from(error: OperationError) -> Self {
        Self::Operation(error)
    }
}

impl fmt::Display for ShortestWitnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ShortestWitnessError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct QueueEntry {
    cost: TropicalWeight,
    sequence: u64,
    state: StateId,
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.sequence.cmp(&self.sequence))
            .then_with(|| other.state.cmp(&self.state))
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy)]
struct NodeRecord {
    cost: TropicalWeight,
    predecessor: Option<(StateId, usize)>,
    sequence: u64,
}

/// Exact, in-memory Dijkstra continuation. Equal-cost paths choose the first
/// discovered source-arc order; each settled state keeps one predecessor.
/// Reachable negative or invalid weights fail rather than imply exactness.
pub struct BoundedShortestWitness<F, L>
where
    F: Wfst<L, TropicalWeight>,
    L: Clone + Send + Sync,
{
    source: F,
    frontier: BinaryHeap<QueueEntry>,
    best: HashMap<StateId, NodeRecord>,
    settled: HashSet<StateId>,
    best_final: Option<(TropicalWeight, StateId, u64)>,
    next_sequence: u64,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
    _label: std::marker::PhantomData<L>,
}

impl<F, L> BoundedShortestWitness<F, L>
where
    F: Wfst<L, TropicalWeight>,
    L: Clone + Send + Sync,
{
    /// Bind a finite source and caller-computed nonzero content digest.
    ///
    /// # Errors
    ///
    /// Rejects an invalid start/count or missing content binding.
    pub fn new(
        source: F,
        source_binding: [u8; 32],
        limits: OperationLimits,
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
        let session = OperationSession::new(plan, limits, cancellation);
        let mut frontier = BinaryHeap::new();
        let mut best = HashMap::new();
        if source.num_states() > 0 {
            frontier.push(QueueEntry {
                cost: TropicalWeight::one(),
                sequence: 0,
                state: start,
            });
            best.insert(
                start,
                NodeRecord {
                    cost: TropicalWeight::one(),
                    predecessor: None,
                    sequence: 0,
                },
            );
        }
        Ok(Self {
            source,
            frontier,
            best,
            settled: HashSet::new(),
            best_final: None,
            next_sequence: 1,
            session,
            last_checkpoint: None,
            _label: std::marker::PhantomData,
        })
    }

    /// Resume only the exact last checkpoint of this live continuation.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
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
        self.last_checkpoint = None;
        Ok(())
    }

    /// Search and reconstruct one exact best accepting witness. `None` under
    /// `Complete` means exact absence of an accepting path. `Incomplete`
    /// always has `None`, never a guessed best path. `label_meter` accounts
    /// for payload allocations when the witness copies source labels.
    ///
    /// # Errors
    ///
    /// Refuses source drift, malformed targets, unsupported reachable weights
    /// or representation overflow.
    pub fn run<M>(
        &mut self,
        observed_source_binding: [u8; 32],
        label_meter: M,
    ) -> Result<OperationOutcome<Option<ShortestWitness<L>>>, ShortestWitnessError>
    where
        M: Fn(&L) -> u64,
    {
        if observed_source_binding != self.session.plan().source_binding {
            return Err(OperationError::StaleSource.into());
        }
        while let Some(entry) = self.frontier.peek().copied() {
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            let live = self.best.get(&entry.state).is_some_and(|record| {
                record.cost == entry.cost && record.sequence == entry.sequence
            });
            if !live || self.settled.contains(&entry.state) {
                let cost = OperationCost {
                    work: 1,
                    ..OperationCost::default()
                };
                if let Err(reason) = self.session.charge(cost) {
                    return Ok(self.incomplete(reason));
                }
                self.frontier.pop();
                continue;
            }
            let minimum = OperationCost {
                states: 1,
                work: 1,
                ..OperationCost::default()
            };
            if let Err(reason) = self.session.preview_dynamic(minimum)? {
                return Ok(self.incomplete(reason));
            }
            let arcs = self.source.transitions(entry.state);
            let mut updates: Vec<(StateId, TropicalWeight, usize)> = Vec::new();
            let mut update_index = HashMap::new();
            for (arc_index, arc) in arcs.iter().enumerate() {
                if !self.source.is_valid_state(arc.to) {
                    return Err(ShortestWitnessError::InvalidTarget {
                        state: entry.state,
                        arc_index,
                    });
                }
                let weight = arc.weight.value();
                if weight.is_nan()
                    || weight < 0.0
                    || (weight.is_infinite() && weight.is_sign_negative())
                {
                    return Err(ShortestWitnessError::UnsupportedWeight {
                        state: entry.state,
                        arc_index: Some(arc_index),
                    });
                }
                if weight.is_infinite() || self.settled.contains(&arc.to) {
                    continue;
                }
                let next_cost = entry.cost.value() + weight;
                if !next_cost.is_finite() {
                    return Err(ShortestWitnessError::ExhaustedRepresentation);
                }
                let next_cost = TropicalWeight::new(next_cost);
                if self
                    .best
                    .get(&arc.to)
                    .is_some_and(|record| next_cost >= record.cost)
                {
                    continue;
                }
                if let Some(&index) = update_index.get(&arc.to) {
                    let old: &mut (StateId, TropicalWeight, usize) = &mut updates[index];
                    if next_cost < old.1 {
                        *old = (arc.to, next_cost, arc_index);
                    }
                } else {
                    update_index.insert(arc.to, updates.len());
                    updates.push((arc.to, next_cost, arc_index));
                }
            }
            let mut final_candidate = None;
            if self.source.is_final(entry.state) {
                let final_weight = self.source.final_weight(entry.state).value();
                if final_weight.is_nan()
                    || final_weight < 0.0
                    || (final_weight.is_infinite() && final_weight.is_sign_negative())
                {
                    return Err(ShortestWitnessError::UnsupportedWeight {
                        state: entry.state,
                        arc_index: None,
                    });
                }
                if final_weight.is_finite() {
                    let total = entry.cost.value() + final_weight;
                    if !total.is_finite() {
                        return Err(ShortestWitnessError::ExhaustedRepresentation);
                    }
                    final_candidate = Some(TropicalWeight::new(total));
                }
            }
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            let new_records = updates
                .iter()
                .filter(|(target, _, _)| !self.best.contains_key(target))
                .count();
            let arc_count = u64::try_from(arcs.len())
                .map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
            let update_count = u64::try_from(updates.len())
                .map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
            if self.next_sequence.checked_add(update_count).is_none() {
                return Err(ShortestWitnessError::ExhaustedRepresentation);
            }
            let work = arc_count
                .checked_add(update_count)
                .and_then(|n| n.checked_add(1))
                .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
            let heap =
                (updates.len() as u128)
                    .checked_mul(size_of::<QueueEntry>() as u128)
                    .and_then(|n| {
                        n.checked_add((new_records as u128).checked_mul(
                            (size_of::<StateId>() + size_of::<NodeRecord>()) as u128,
                        )?)
                    })
                    .and_then(|n| {
                        n.checked_add(
                            u128::from(self.session.usage().states == 0)
                                * (size_of::<QueueEntry>() + size_of::<NodeRecord>()) as u128,
                        )
                    })
                    .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
            let heap_bytes =
                u64::try_from(heap).map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
            let cost = OperationCost {
                states: 1,
                arcs: arc_count,
                work,
                heap_bytes,
            };
            if let Err(reason) = self.session.advance_dynamic(cost)? {
                return Ok(self.incomplete(reason));
            }
            self.frontier.pop();
            self.settled.insert(entry.state);
            if let Some(total) = final_candidate {
                let better = self.best_final.is_none_or(|(old, _, old_sequence)| {
                    total < old || (total == old && entry.sequence < old_sequence)
                });
                if better {
                    self.best_final = Some((total, entry.state, entry.sequence));
                }
            }
            for (target, next_cost, arc_index) in updates {
                let sequence = self.next_sequence;
                self.next_sequence = self
                    .next_sequence
                    .checked_add(1)
                    .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
                self.best.insert(
                    target,
                    NodeRecord {
                        cost: next_cost,
                        predecessor: Some((entry.state, arc_index)),
                        sequence,
                    },
                );
                self.frontier.push(QueueEntry {
                    cost: next_cost,
                    sequence,
                    state: target,
                });
            }
        }
        let witness = match self.reconstruct(label_meter)? {
            Ok(witness) => witness,
            Err(reason) => return Ok(self.incomplete(reason)),
        };
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        Ok(OperationOutcome::Complete {
            value: witness,
            checkpoint,
        })
    }

    fn reconstruct<M>(
        &mut self,
        label_meter: M,
    ) -> Result<Result<Option<ShortestWitness<L>>, IncompleteReason>, ShortestWitnessError>
    where
        M: Fn(&L) -> u64,
    {
        let Some((total_weight, final_state, _)) = self.best_final else {
            return Ok(Ok(None));
        };
        let mut reverse = Vec::new();
        let mut state = final_state;
        while let Some((previous, arc_index)) = self
            .best
            .get(&state)
            .ok_or(ShortestWitnessError::ExhaustedRepresentation)?
            .predecessor
        {
            if reverse.len() >= self.settled.len() {
                return Err(ShortestWitnessError::ExhaustedRepresentation);
            }
            let arc = self
                .source
                .transitions(previous)
                .get(arc_index)
                .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
            if arc.to != state {
                return Err(ShortestWitnessError::ExhaustedRepresentation);
            }
            reverse.push((previous, arc_index, state));
            state = previous;
        }
        reverse.reverse();
        let structural = (reverse.len() as u128)
            .checked_mul(size_of::<ShortestWitnessStep<L>>() as u128)
            .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
        let mut heap_bytes =
            u64::try_from(structural).map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
        for &(previous, arc_index, _) in &reverse {
            let arc = &self.source.transitions(previous)[arc_index];
            if let Some(label) = &arc.input {
                heap_bytes = heap_bytes
                    .checked_add(label_meter(label))
                    .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
            }
            if let Some(label) = &arc.output {
                heap_bytes = heap_bytes
                    .checked_add(label_meter(label))
                    .ok_or(ShortestWitnessError::ExhaustedRepresentation)?;
            }
        }
        let work = u64::try_from(reverse.len())
            .map_err(|_| ShortestWitnessError::ExhaustedRepresentation)?;
        let cost = OperationCost {
            work,
            heap_bytes,
            ..OperationCost::default()
        };
        if let Err(reason) = self.session.charge(cost) {
            return Ok(Err(reason));
        }
        let steps = reverse
            .into_iter()
            .map(|(from, arc_index, to)| {
                let arc = &self.source.transitions(from)[arc_index];
                ShortestWitnessStep {
                    from,
                    arc_index,
                    input: arc.input.clone(),
                    output: arc.output.clone(),
                    to,
                    weight: arc.weight,
                }
            })
            .collect();
        Ok(Ok(Some(ShortestWitness {
            steps,
            final_state,
            final_weight: self.source.final_weight(final_state),
            total_weight,
        })))
    }

    fn incomplete(
        &mut self,
        reason: IncompleteReason,
    ) -> OperationOutcome<Option<ShortestWitness<L>>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: None,
            reason,
            checkpoint,
        }
    }
}
