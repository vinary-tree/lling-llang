//! Bounded, deterministic-order weighted subset construction.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::{self, Debug};
use std::hash::Hash;
use std::mem::size_of;

use crate::semiring::{DivisibleSemiring, Semiring, TotallyOrderedSemiring};
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{
    CancellationToken, MutableWfst, SourceSnapshot, StateId, VectorWfst, WeightedTransition, Wfst,
    NO_STATE,
};

const ALGORITHM_ID: &str = "lling.determinize.ordered-subsets/v1";
type Subset<W> = BTreeMap<StateId, W>;
type SubsetKey<W> = Vec<(StateId, W)>;
type Targets<L, W> = Vec<(StateId, W, Option<L>)>;

struct PendingArc<L, W: Semiring> {
    input: L,
    output: Option<L>,
    weight: W,
    target: Subset<W>,
    key: SubsetKey<W>,
}

/// A failed determinization input or operation contract, never a partial
/// result mislabeled as complete.
#[derive(Debug)]
pub enum BoundedDeterminizeError {
    /// No valid start state exists.
    NoStartState,
    /// A reachable input-epsilon arc needs a separately bounded preprocessor.
    InputEpsilon,
    /// One input label produced conflicting output labels.
    ConflictingOutput,
    /// A required residual-weight division is undefined.
    NonDivisibleWeight,
    /// Result state IDs or logical costs cannot be represented.
    ExhaustedRepresentation,
    /// Shared operation-contract error.
    Operation(OperationError),
}

impl From<OperationError> for BoundedDeterminizeError {
    fn from(error: OperationError) -> Self {
        Self::Operation(error)
    }
}

impl fmt::Display for BoundedDeterminizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for BoundedDeterminizeError {}

/// An in-memory continuation of weighted powerset construction. It owns the
/// exact queue, subset map, source, and partial result needed for resume.
pub struct BoundedDeterminization<F, L, W>
where
    F: Wfst<L, W>,
    L: Clone + Eq + Hash + Ord + Debug + Send + Sync,
    W: DivisibleSemiring + TotallyOrderedSemiring + Clone + Debug + Hash + Eq,
{
    source: F,
    result: VectorWfst<L, W>,
    subsets: HashMap<SubsetKey<W>, StateId>,
    frontier: VecDeque<(StateId, Subset<W>)>,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<F, L, W> BoundedDeterminization<F, L, W>
where
    F: Wfst<L, W>,
    L: Clone + Eq + Hash + Ord + Debug + Send + Sync,
    W: DivisibleSemiring + TotallyOrderedSemiring + Clone + Debug + Hash + Eq,
{
    /// Bind an immutable source to a nonzero caller-computed content digest.
    /// No epsilon removal, trimming, or native recursion occurs inside this
    /// adapter; a reachable input epsilon is rejected explicitly.
    ///
    /// # Errors
    ///
    /// Rejects an invalid start or missing source-content binding.
    pub fn new(
        source: F,
        source_binding: [u8; 32],
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, BoundedDeterminizeError> {
        let plan =
            OperationPlan::new_dynamic(SourceSnapshot::IMMUTABLE, source_binding, ALGORITHM_ID)?;
        let session = OperationSession::new(plan, limits, cancellation);
        let mut result = VectorWfst::new();
        let mut subsets = HashMap::new();
        let mut frontier = VecDeque::new();
        if source.num_states() > 0 {
            let start = source.start();
            if start == NO_STATE || !source.is_valid_state(start) {
                return Err(BoundedDeterminizeError::NoStartState);
            }
            let start_id = result.add_state();
            result.set_start(start_id);
            let initial = BTreeMap::from([(start, W::one())]);
            subsets.insert(vec![(start, W::one())], start_id);
            frontier.push_back((start_id, initial));
        }
        Ok(Self {
            source,
            result,
            subsets,
            frontier,
            session,
            last_checkpoint: None,
        })
    }

    /// Resume the exact checkpoint from this live continuation after raising
    /// limits or replacing a cancelled token.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints without discarding the state.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<(), BoundedDeterminizeError> {
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

    /// Advance complete subsets until exhaustion or an interruption. The
    /// payload meters cover label/weight-owned logical heap bytes; structural
    /// subset/result charges are supplied by this adapter.
    ///
    /// # Errors
    ///
    /// Rejects source drift, unsupported input epsilon, ambiguous outputs,
    /// undefined division, or representation overflow.
    pub fn run<LM, WM>(
        &mut self,
        observed_source_binding: [u8; 32],
        label_meter: LM,
        weight_meter: WM,
    ) -> Result<OperationOutcome<VectorWfst<L, W>>, BoundedDeterminizeError>
    where
        LM: Fn(&L) -> u64,
        WM: Fn(&W) -> u64,
    {
        if observed_source_binding != self.session.plan().source_binding {
            return Err(OperationError::StaleSource.into());
        }
        while let Some((output_state, subset)) = self.frontier.front() {
            let minimum = OperationCost {
                states: 1,
                work: 1,
                ..OperationCost::default()
            };
            if let Err(reason) = self.session.preview_dynamic(minimum)? {
                return Ok(self.incomplete(reason));
            }
            let output_state = *output_state;
            let subset = subset.clone();
            let mut final_weight = W::zero();
            let mut labels: BTreeMap<L, Targets<L, W>> = BTreeMap::new();
            let mut source_arcs = 0_u64;
            for (&state, residual) in &subset {
                if self.source.is_final(state) {
                    final_weight =
                        final_weight.plus(&residual.times(&self.source.final_weight(state)));
                }
                for transition in self.source.transitions(state) {
                    if !self.source.is_valid_state(transition.to) {
                        continue;
                    }
                    source_arcs = source_arcs
                        .checked_add(1)
                        .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
                    let Some(input) = &transition.input else {
                        return Err(BoundedDeterminizeError::InputEpsilon);
                    };
                    labels.entry(input.clone()).or_default().push((
                        transition.to,
                        residual.times(&transition.weight),
                        transition.output.clone(),
                    ));
                }
            }
            let mut pending = Vec::with_capacity(labels.len());
            for (input, targets) in labels {
                let mut target = Subset::new();
                let mut output: Option<Option<L>> = None;
                for (state, weight, next_output) in targets {
                    target
                        .entry(state)
                        .and_modify(|old: &mut W| *old = old.plus(&weight))
                        .or_insert(weight);
                    match &output {
                        Some(existing) if existing != &next_output => {
                            return Err(BoundedDeterminizeError::ConflictingOutput)
                        }
                        None => output = Some(next_output),
                        _ => {}
                    }
                }
                let minimum = target
                    .values()
                    .cloned()
                    .min_by(|a, b| a.total_cmp(b))
                    .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
                let mut normalized = Subset::new();
                for (state, weight) in target {
                    let residual = weight
                        .divide(&minimum)
                        .ok_or(BoundedDeterminizeError::NonDivisibleWeight)?;
                    normalized.insert(state, residual);
                }
                let key = normalized
                    .iter()
                    .map(|(&state, weight)| (state, weight.clone()))
                    .collect();
                pending.push(PendingArc {
                    input,
                    output: output.flatten(),
                    weight: minimum,
                    target: normalized,
                    key,
                });
            }
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            let mut new_keys = HashSet::new();
            let mut new_members = 0_usize;
            for arc in &pending {
                if !self.subsets.contains_key(&arc.key) && new_keys.insert(&arc.key) {
                    new_members = new_members
                        .checked_add(arc.target.len())
                        .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
                }
            }
            if self
                .subsets
                .len()
                .checked_add(new_keys.len())
                .is_none_or(|n| n >= u32::MAX as usize)
            {
                return Err(BoundedDeterminizeError::ExhaustedRepresentation);
            }
            let output_arcs = u64::try_from(pending.len())
                .map_err(|_| BoundedDeterminizeError::ExhaustedRepresentation)?;
            let member_work = u64::try_from(new_members)
                .map_err(|_| BoundedDeterminizeError::ExhaustedRepresentation)?;
            let work = source_arcs
                .checked_add(output_arcs)
                .and_then(|n| n.checked_add(member_work))
                .and_then(|n| n.checked_add(1))
                .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
            let structural = (pending.len() as u128)
                .checked_mul(size_of::<WeightedTransition<L, W>>() as u128)
                .and_then(|n| {
                    n.checked_add(
                        (new_members as u128)
                            .checked_mul(2 * (size_of::<StateId>() + size_of::<W>()) as u128)?,
                    )
                })
                .and_then(|n| {
                    n.checked_add(
                        (new_keys.len() as u128)
                            .checked_mul((size_of::<StateId>() + size_of::<Subset<W>>()) as u128)?,
                    )
                })
                .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
            let mut heap = u64::try_from(structural)
                .map_err(|_| BoundedDeterminizeError::ExhaustedRepresentation)?;
            for arc in &pending {
                heap = heap
                    .checked_add(label_meter(&arc.input))
                    .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
                if let Some(output) = &arc.output {
                    heap = heap
                        .checked_add(label_meter(output))
                        .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
                }
                heap = heap
                    .checked_add(weight_meter(&arc.weight))
                    .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
                for weight in arc.target.values() {
                    heap = heap
                        .checked_add(weight_meter(weight))
                        .ok_or(BoundedDeterminizeError::ExhaustedRepresentation)?;
                }
            }
            let cost = OperationCost {
                states: 1,
                arcs: output_arcs,
                work,
                heap_bytes: heap,
            };
            if let Err(reason) = self.session.advance_dynamic(cost)? {
                return Ok(self.incomplete(reason));
            }
            if !final_weight.is_zero() {
                self.result.set_final(output_state, final_weight);
            }
            self.result.reserve_transitions(output_state, pending.len());
            for arc in pending {
                let target_id = if let Some(&id) = self.subsets.get(&arc.key) {
                    id
                } else {
                    let id = self.result.add_state();
                    self.subsets.insert(arc.key, id);
                    self.frontier.push_back((id, arc.target));
                    id
                };
                self.result.add_transition(WeightedTransition::new(
                    output_state,
                    Some(arc.input),
                    arc.output,
                    target_id,
                    arc.weight,
                ));
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
