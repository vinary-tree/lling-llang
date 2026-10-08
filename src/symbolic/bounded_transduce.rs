//! Bounded, source-ordered concrete-input SFT transduction.

use std::collections::VecDeque;
use std::fmt;
use std::mem::size_of;
use std::ops::Range;

use super::sft::SymbolicFiniteTransducer;
use super::BooleanAlgebra;
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, SourceSnapshot};

const ALGORITHM_ID: &str = "lling.sft.concrete-transduction/v1";

/// Absolute caps in addition to shared logical operation resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SftTransductionLimits {
    /// Largest number of complete accepted paths emitted before suspension.
    pub max_paths: usize,
    /// Largest pending path-frame queue.
    pub max_frontier: usize,
}

impl Default for SftTransductionLimits {
    fn default() -> Self {
        Self {
            max_paths: usize::MAX,
            max_frontier: usize::MAX,
        }
    }
}

/// One source transition in an accepting path. `output_range` indexes the
/// complete witness output, so no output fragment is duplicated per step.
#[derive(Clone, Debug)]
pub struct SftWitnessStep<I> {
    /// Source state index.
    pub from: usize,
    /// Source transition's index in the original ordered transition vector.
    pub transition_index: usize,
    /// Position in the original input word.
    pub input_index: usize,
    /// The consumed input value.
    pub input: I,
    /// Target state index.
    pub to: usize,
    /// Half-open span of the output produced by this transition.
    pub output_range: Range<usize>,
}

/// An exact accepting path and the complete output sequence it produces.
#[derive(Clone, Debug)]
pub struct SftWitness<I, O> {
    /// Caller-computed identity of the exact ordered source SFT.
    pub source_binding: [u8; 32],
    /// Caller-computed identity of the exact input word.
    pub input_binding: [u8; 32],
    /// Source initial state.
    pub initial_state: usize,
    /// Source accepting state.
    pub final_state: usize,
    /// Ordered, complete source transition provenance.
    pub steps: Vec<SftWitnessStep<I>>,
    /// Concatenated output sequence; step ranges identify each contribution.
    pub output: Vec<O>,
}

/// Rejection outside the exact, source-bound continuation contract.
#[derive(Debug)]
pub enum SftTransductionError {
    /// An initial-state ID is not present in the source.
    InvalidInitial(usize),
    /// A taken transition has an invalid target.
    InvalidTarget {
        transition_index: usize,
        target: usize,
    },
    /// The two caller-computed content bindings must be nonzero.
    InvalidBinding,
    /// Index, cost, or output size cannot be represented.
    ExhaustedRepresentation,
    /// Shared operation-contract error.
    Operation(OperationError),
}

impl From<OperationError> for SftTransductionError {
    fn from(value: OperationError) -> Self {
        Self::Operation(value)
    }
}

impl fmt::Display for SftTransductionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SftTransductionError {}

struct PathNode<O> {
    parent: Option<usize>,
    from: usize,
    transition_index: Option<usize>,
    input_index: usize,
    state: usize,
    output: Vec<O>,
}

struct Pending<O> {
    transition_index: usize,
    to: usize,
    output: Vec<O>,
}

/// Ordered source-transition locations. Construction is incremental so the
/// caller can charge and checkpoint each admitted source transition.
pub(crate) struct OrderedSftTransitionIndex {
    buckets: Vec<Vec<usize>>,
}

impl OrderedSftTransitionIndex {
    pub(crate) fn new(state_count: usize) -> Self {
        Self {
            buckets: (0..state_count).map(|_| Vec::new()).collect(),
        }
    }

    pub(crate) fn admit(&mut self, from: usize, transition_index: usize) {
        if let Some(bucket) = self.buckets.get_mut(from) {
            bucket.push(transition_index);
        }
    }

    pub(crate) fn outgoing(&self, state: usize) -> &[usize] {
        &self.buckets[state]
    }
}

/// Iterative exact search over one immutable SFT and concrete input word.
/// The caller's source/input digests bind the plan and must be re-observed.
pub struct BoundedSftTransduction<A, B>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    A::Domain: Clone + Into<B::Domain>,
{
    source: SymbolicFiniteTransducer<A, B>,
    input: Vec<A::Domain>,
    source_binding: [u8; 32],
    input_binding: [u8; 32],
    outgoing: Option<OrderedSftTransitionIndex>,
    index_cursor: usize,
    initials: Vec<usize>,
    seeded: bool,
    arena: Vec<PathNode<B::Domain>>,
    frontier: VecDeque<usize>,
    emitted: Vec<SftWitness<A::Domain, B::Domain>>,
    path_limits: SftTransductionLimits,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<A, B> BoundedSftTransduction<A, B>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    A::Domain: Clone + Into<B::Domain>,
{
    /// Bind an owned source and input word to caller-computed content digests.
    ///
    /// # Errors
    ///
    /// Rejects missing bindings or an invalid operation plan.
    pub fn new(
        source: SymbolicFiniteTransducer<A, B>,
        input: Vec<A::Domain>,
        source_binding: [u8; 32],
        input_binding: [u8; 32],
        limits: OperationLimits,
        path_limits: SftTransductionLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, SftTransductionError> {
        if source_binding == [0; 32] || input_binding == [0; 32] {
            return Err(SftTransductionError::InvalidBinding);
        }
        let mut digest = blake3::Hasher::new();
        digest.update(b"lling.sft.source-and-word/v1\0");
        digest.update(&source_binding);
        digest.update(&input_binding);
        let plan = OperationPlan::new_dynamic(
            SourceSnapshot::IMMUTABLE,
            *digest.finalize().as_bytes(),
            ALGORITHM_ID,
        )?;
        Ok(Self {
            source,
            input,
            source_binding,
            input_binding,
            outgoing: None,
            index_cursor: 0,
            initials: Vec::new(),
            seeded: false,
            arena: Vec::new(),
            frontier: VecDeque::new(),
            emitted: Vec::new(),
            path_limits,
            session: OperationSession::new(plan, limits, cancellation),
            last_checkpoint: None,
        })
    }

    /// Resume only the exact last checkpoint of this live machine. Raised
    /// limits keep indexing progress, every pending path and all prior output.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        path_limits: SftTransductionLimits,
        cancellation: CancellationToken,
    ) -> Result<(), SftTransductionError> {
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

    /// Enumerate complete output witnesses for the bound input word.
    /// `input_meter` and `output_meter` report copied payload bytes beyond
    /// the inline sizes of domain elements. Callback functions must be pure,
    /// total and stable across resume; generic Rust closures cannot enforce
    /// those semantic properties or be preempted during a single invocation.
    ///
    /// # Errors
    ///
    /// Refuses source/input drift, invalid taken targets, and representation
    /// overflow. Caps and cancellation instead return typed `Incomplete`.
    pub fn run<MI, MO>(
        &mut self,
        observed_source_binding: [u8; 32],
        observed_input_binding: [u8; 32],
        input_meter: MI,
        output_meter: MO,
    ) -> Result<OperationOutcome<Vec<SftWitness<A::Domain, B::Domain>>>, SftTransductionError>
    where
        MI: Fn(&A::Domain) -> u64,
        MO: Fn(&B::Domain) -> u64,
    {
        if observed_source_binding != self.source_binding
            || observed_input_binding != self.input_binding
        {
            return Err(OperationError::StaleSource.into());
        }
        loop {
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            if self.outgoing.is_none() {
                if let Err(reason) = self.prepare()? {
                    return Ok(self.incomplete(reason));
                }
            }
            if self.index_cursor < self.source.transitions.len() {
                if let Err(reason) = self.index_one()? {
                    return Ok(self.incomplete(reason));
                }
                continue;
            }
            if !self.seeded {
                if let Err(reason) = self.seed()? {
                    return Ok(self.incomplete(reason));
                }
            }
            let Some(node) = self.frontier.front().copied() else {
                return Ok(self.complete());
            };
            if self.emitted.len() >= self.path_limits.max_paths {
                return Ok(self.incomplete(IncompleteReason::PathLimit));
            }
            if self.frontier.len() > self.path_limits.max_frontier {
                return Ok(self.incomplete(IncompleteReason::FrontierLimit));
            }
            if self.arena[node].input_index == self.input.len() {
                if let Err(reason) = self.finish(node, &input_meter, &output_meter)? {
                    return Ok(self.incomplete(reason));
                }
            } else if let Err(reason) = self.expand(node, &output_meter)? {
                return Ok(self.incomplete(reason));
            }
        }
    }

    fn prepare(&mut self) -> Result<Result<(), IncompleteReason>, SftTransductionError> {
        let states = self.source.states.len();
        let initial_count = self.source.initial_states.len();
        if let Some(invalid) = self
            .source
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= states)
            .min()
        {
            return Err(SftTransductionError::InvalidInitial(invalid));
        }
        let work = states
            .checked_add(initial_count)
            .and_then(|n| n.checked_add(1))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        let heap = (states as u128)
            .checked_mul(size_of::<Vec<usize>>() as u128)
            .and_then(|n| {
                n.checked_add((initial_count as u128).checked_mul(size_of::<usize>() as u128)?)
            })
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftTransductionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        let mut initials: Vec<usize> = self.source.initial_states.iter().copied().collect();
        initials.sort_unstable();
        self.initials = initials;
        self.outgoing = Some(OrderedSftTransitionIndex::new(states));
        Ok(Ok(()))
    }

    fn index_one(&mut self) -> Result<Result<(), IncompleteReason>, SftTransductionError> {
        let transition = &self.source.transitions[self.index_cursor];
        let indexed = transition.from < self.source.states.len();
        if let Err(reason) = self.session.charge(OperationCost {
            arcs: 1,
            work: 1,
            heap_bytes: if indexed {
                size_of::<usize>() as u64
            } else {
                0
            },
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        if indexed {
            self.outgoing
                .as_mut()
                .expect("index initialized")
                .admit(transition.from, self.index_cursor);
        }
        self.index_cursor += 1;
        Ok(Ok(()))
    }

    fn seed(&mut self) -> Result<Result<(), IncompleteReason>, SftTransductionError> {
        if self.initials.len() > self.path_limits.max_frontier {
            return Ok(Err(IncompleteReason::FrontierLimit));
        }
        let heap = (self.initials.len() as u128)
            .checked_mul((size_of::<PathNode<B::Domain>>() + size_of::<usize>()) as u128)
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work: u64::try_from(self.initials.len())
                .map_err(|_| SftTransductionError::ExhaustedRepresentation)?,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftTransductionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        for &state in &self.initials {
            let index = self.arena.len();
            self.arena.push(PathNode {
                parent: None,
                from: state,
                transition_index: None,
                input_index: 0,
                state,
                output: Vec::new(),
            });
            self.frontier.push_back(index);
        }
        self.seeded = true;
        Ok(Ok(()))
    }

    fn expand<MO>(
        &mut self,
        node: usize,
        output_meter: &MO,
    ) -> Result<Result<(), IncompleteReason>, SftTransductionError>
    where
        MO: Fn(&B::Domain) -> u64,
    {
        if let Err(reason) = self.session.preview_dynamic(OperationCost {
            states: 1,
            work: 1,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let path = &self.arena[node];
        let state = path.state;
        let input_index = path.input_index;
        let source_input = &self.input[path.input_index];
        let outgoing = self
            .outgoing
            .as_ref()
            .expect("index initialized")
            .outgoing(path.state);
        let mut pending = Vec::new();
        let mut payload_bytes = 0_u64;
        let mut output_count = 0_u64;
        for &transition_index in outgoing {
            let transition = &self.source.transitions[transition_index];
            if !self
                .source
                .input_algebra
                .evaluate(&transition.guard, source_input)
            {
                continue;
            }
            if transition.to >= self.source.states.len() {
                return Err(SftTransductionError::InvalidTarget {
                    transition_index,
                    target: transition.to,
                });
            }
            let output = transition.output.apply(source_input);
            output_count = output_count
                .checked_add(
                    u64::try_from(output.len())
                        .map_err(|_| SftTransductionError::ExhaustedRepresentation)?,
                )
                .ok_or(SftTransductionError::ExhaustedRepresentation)?;
            for value in &output {
                payload_bytes = payload_bytes
                    .checked_add(output_meter(value))
                    .ok_or(SftTransductionError::ExhaustedRepresentation)?;
            }
            pending.push(Pending {
                transition_index,
                to: transition.to,
                output,
            });
        }
        let frontier_after = self
            .frontier
            .len()
            .checked_sub(1)
            .and_then(|n| n.checked_add(pending.len()))
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        if frontier_after > self.path_limits.max_frontier {
            return Ok(Err(IncompleteReason::FrontierLimit));
        }
        if path.input_index.checked_add(1).is_none()
            || self.arena.len().checked_add(pending.len()).is_none()
        {
            return Err(SftTransductionError::ExhaustedRepresentation);
        }
        let inspected = u64::try_from(outgoing.len())
            .map_err(|_| SftTransductionError::ExhaustedRepresentation)?;
        let produced = u64::try_from(pending.len())
            .map_err(|_| SftTransductionError::ExhaustedRepresentation)?;
        let work = inspected
            .checked_add(produced)
            .and_then(|n| n.checked_add(output_count))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        let heap = (pending.len() as u128)
            .checked_mul((size_of::<PathNode<B::Domain>>() + size_of::<usize>()) as u128)
            .and_then(|n| {
                n.checked_add((output_count as u128).checked_mul(size_of::<B::Domain>() as u128)?)
            })
            .and_then(|n| n.checked_add(u128::from(payload_bytes)))
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            arcs: inspected,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftTransductionError::ExhaustedRepresentation)?,
        })? {
            return Ok(Err(reason));
        }
        self.frontier.pop_front();
        for entry in pending {
            let index = self.arena.len();
            self.arena.push(PathNode {
                parent: Some(node),
                from: state,
                transition_index: Some(entry.transition_index),
                input_index: input_index + 1,
                state: entry.to,
                output: entry.output,
            });
            self.frontier.push_back(index);
        }
        Ok(Ok(()))
    }

    fn finish<MI, MO>(
        &mut self,
        node: usize,
        input_meter: &MI,
        output_meter: &MO,
    ) -> Result<Result<(), IncompleteReason>, SftTransductionError>
    where
        MI: Fn(&A::Domain) -> u64,
        MO: Fn(&B::Domain) -> u64,
    {
        let state = self.arena[node].state;
        if !self.source.accepting_states.contains(&state) {
            if let Err(reason) = self.session.advance_dynamic(OperationCost {
                states: 1,
                work: 1,
                ..OperationCost::default()
            })? {
                return Ok(Err(reason));
            }
            self.frontier.pop_front();
            return Ok(Ok(()));
        }
        let mut reverse = Vec::new();
        let mut cursor = node;
        while let Some(parent) = self.arena[cursor].parent {
            if reverse.len() >= self.arena.len() || parent >= cursor {
                return Err(SftTransductionError::ExhaustedRepresentation);
            }
            reverse.push(cursor);
            cursor = parent;
        }
        reverse.reverse();
        if reverse.len() != self.input.len() {
            return Err(SftTransductionError::ExhaustedRepresentation);
        }
        let output_len = reverse
            .iter()
            .try_fold(0usize, |sum, &index| {
                sum.checked_add(self.arena[index].output.len())
            })
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        let mut payload = 0_u64;
        for &index in &reverse {
            let path = &self.arena[index];
            payload = payload
                .checked_add(input_meter(&self.input[path.input_index - 1]))
                .ok_or(SftTransductionError::ExhaustedRepresentation)?;
            for value in &path.output {
                payload = payload
                    .checked_add(output_meter(value))
                    .ok_or(SftTransductionError::ExhaustedRepresentation)?;
            }
        }
        let heap = (reverse.len() as u128)
            .checked_mul(size_of::<SftWitnessStep<A::Domain>>() as u128)
            .and_then(|n| {
                n.checked_add((output_len as u128).checked_mul(size_of::<B::Domain>() as u128)?)
            })
            .and_then(|n| n.checked_add(size_of::<SftWitness<A::Domain, B::Domain>>() as u128))
            .and_then(|n| n.checked_add(u128::from(payload)))
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        let work = u64::try_from(reverse.len())
            .ok()
            .and_then(|n| n.checked_add(u64::try_from(output_len).ok()?))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftTransductionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftTransductionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let initial_state = self.arena[cursor].state;
        let mut output = Vec::with_capacity(output_len);
        let mut steps = Vec::with_capacity(reverse.len());
        for index in reverse {
            let path = &self.arena[index];
            let start = output.len();
            output.extend(path.output.iter().cloned());
            let source_index = path
                .transition_index
                .ok_or(SftTransductionError::ExhaustedRepresentation)?;
            let source = &self.source.transitions[source_index];
            if source.from != path.from || source.to != path.state {
                return Err(SftTransductionError::ExhaustedRepresentation);
            }
            steps.push(SftWitnessStep {
                from: path.from,
                transition_index: source_index,
                input_index: path.input_index - 1,
                input: self.input[path.input_index - 1].clone(),
                to: path.state,
                output_range: start..output.len(),
            });
        }
        self.frontier.pop_front();
        self.emitted.push(SftWitness {
            source_binding: self.source_binding,
            input_binding: self.input_binding,
            initial_state,
            final_state: state,
            steps,
            output,
        });
        Ok(Ok(()))
    }

    fn incomplete(
        &mut self,
        reason: IncompleteReason,
    ) -> OperationOutcome<Vec<SftWitness<A::Domain, B::Domain>>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: self.emitted.clone(),
            reason,
            checkpoint,
        }
    }

    fn complete(&mut self) -> OperationOutcome<Vec<SftWitness<A::Domain, B::Domain>>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Complete {
            value: self.emitted.clone(),
            checkpoint,
        }
    }
}
