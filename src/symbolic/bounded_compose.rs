//! Exact bounded execution of a composed pair of SFTs on a concrete word.

use std::collections::VecDeque;
use std::fmt;
use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;

use super::bounded_transduce::OrderedSftTransitionIndex;
use super::sft::SymbolicFiniteTransducer;
use super::BooleanAlgebra;
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, SourceSnapshot};

const ALGORITHM_ID: &str = "lling.sft.concrete-composition/v1";

/// Absolute caps for the exact composed-path continuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SftCompositionLimits {
    /// Largest emitted accepting-path count.
    pub max_paths: usize,
    /// Largest pending path-frame queue.
    pub max_frontier: usize,
}

impl Default for SftCompositionLimits {
    fn default() -> Self {
        Self {
            max_paths: usize::MAX,
            max_frontier: usize::MAX,
        }
    }
}

/// One first-stage source transition in a complete composed witness.
#[derive(Clone, Debug)]
pub struct FirstSftStep<I> {
    /// First-stage source state.
    pub from: usize,
    /// Index in the first SFT's ordered transition vector.
    pub transition_index: usize,
    /// Position in the original input word.
    pub input_index: usize,
    /// Consumed original input element.
    pub input: I,
    /// First-stage target state.
    pub to: usize,
    /// Half-open span of this transition's output in `intermediate`.
    pub intermediate_range: Range<usize>,
}

/// One second-stage transition in a complete composed witness.
#[derive(Clone, Debug)]
pub struct SecondSftStep<M> {
    /// Second-stage source state.
    pub from: usize,
    /// Index in the second SFT's ordered transition vector.
    pub transition_index: usize,
    /// Position in the full intermediate word.
    pub intermediate_index: usize,
    /// Consumed intermediate element.
    pub input: M,
    /// Second-stage target state.
    pub to: usize,
    /// Half-open span of this transition's output in `output`.
    pub output_range: Range<usize>,
}

/// One exact accepted run of `first` followed by `second`.
#[derive(Clone, Debug)]
pub struct ComposedSftWitness<I, M, O> {
    /// Caller-computed identity of the first source SFT.
    pub first_binding: [u8; 32],
    /// Caller-computed identity of the second source SFT.
    pub second_binding: [u8; 32],
    /// Caller-computed identity of the original input word.
    pub input_binding: [u8; 32],
    /// First-stage initial state.
    pub first_initial: usize,
    /// First-stage final state.
    pub first_final: usize,
    /// Second-stage initial state.
    pub second_initial: usize,
    /// Second-stage final state.
    pub second_final: usize,
    /// Ordered first-stage source transitions.
    pub first_steps: Vec<FirstSftStep<I>>,
    /// Ordered second-stage source transitions.
    pub second_steps: Vec<SecondSftStep<M>>,
    /// Exact output of the first stage and input to the second.
    pub intermediate: Vec<M>,
    /// Exact final output of the composition.
    pub output: Vec<O>,
}

/// Rejection outside the exact bound-source continuation domain.
#[derive(Debug)]
pub enum SftCompositionError {
    /// An initial ID is not a declared source state.
    InvalidInitial { stage: u8, state: usize },
    /// A taken transition targets an undeclared state.
    InvalidTarget {
        stage: u8,
        transition_index: usize,
        target: usize,
    },
    /// A caller-computed content binding is zero.
    InvalidBinding,
    /// Count, cursor or allocation-account arithmetic is exhausted.
    ExhaustedRepresentation,
    /// Shared operation-contract error.
    Operation(OperationError),
}

impl From<OperationError> for SftCompositionError {
    fn from(value: OperationError) -> Self {
        Self::Operation(value)
    }
}

impl fmt::Display for SftCompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SftCompositionError {}

enum Phase<M> {
    Ready,
    Feeding { middle: Arc<Vec<M>>, offset: usize },
}

enum Event<M, O> {
    Root,
    First {
        transition_index: usize,
        input_index: usize,
        middle: Arc<Vec<M>>,
    },
    Second {
        transition_index: usize,
        intermediate_index: usize,
        input: M,
        output: Vec<O>,
    },
}

struct Node<M, O> {
    parent: Option<usize>,
    first_state: usize,
    second_state: usize,
    input_index: usize,
    middle_len: usize,
    phase: Phase<M>,
    event: Event<M, O>,
}

struct FirstPending<M> {
    transition_index: usize,
    to: usize,
    middle: Vec<M>,
}
struct SecondPending<O> {
    transition_index: usize,
    to: usize,
    output: Vec<O>,
}

/// Live, exact execution of the relational composition over one input word.
/// It retains source-order transition indices, product/microstep frames and
/// append-only provenance; it does not claim to materialize a finite SFT with
/// an exact symbolic guard pullback for arbitrary function closures.
pub struct BoundedSftComposition<A, B, C>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    C: BooleanAlgebra,
    A::Domain: Clone + Into<B::Domain>,
    B::Domain: Clone + Into<C::Domain>,
{
    first: SymbolicFiniteTransducer<A, B>,
    second: SymbolicFiniteTransducer<B, C>,
    input: Vec<A::Domain>,
    first_binding: [u8; 32],
    second_binding: [u8; 32],
    input_binding: [u8; 32],
    first_index: Option<OrderedSftTransitionIndex>,
    second_index: Option<OrderedSftTransitionIndex>,
    first_cursor: usize,
    second_cursor: usize,
    first_initials: Vec<usize>,
    second_initials: Vec<usize>,
    seeded: bool,
    arena: Vec<Node<B::Domain, C::Domain>>,
    frontier: VecDeque<usize>,
    emitted: Vec<ComposedSftWitness<A::Domain, B::Domain, C::Domain>>,
    path_limits: SftCompositionLimits,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<A, B, C> BoundedSftComposition<A, B, C>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    C: BooleanAlgebra,
    A::Domain: Clone + Into<B::Domain>,
    B::Domain: Clone + Into<C::Domain>,
{
    /// Bind both owned sources and the original word to nonzero content IDs.
    ///
    /// # Errors
    ///
    /// Rejects a missing binding or invalid operation plan.
    pub fn new(
        first: SymbolicFiniteTransducer<A, B>,
        second: SymbolicFiniteTransducer<B, C>,
        input: Vec<A::Domain>,
        first_binding: [u8; 32],
        second_binding: [u8; 32],
        input_binding: [u8; 32],
        limits: OperationLimits,
        path_limits: SftCompositionLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, SftCompositionError> {
        if first_binding == [0; 32] || second_binding == [0; 32] || input_binding == [0; 32] {
            return Err(SftCompositionError::InvalidBinding);
        }
        let mut digest = blake3::Hasher::new();
        digest.update(b"lling.sft.composed-source-and-word/v1\0");
        digest.update(&first_binding);
        digest.update(&second_binding);
        digest.update(&input_binding);
        let plan = OperationPlan::new_dynamic(
            SourceSnapshot::IMMUTABLE,
            *digest.finalize().as_bytes(),
            ALGORITHM_ID,
        )?;
        Ok(Self {
            first,
            second,
            input,
            first_binding,
            second_binding,
            input_binding,
            first_index: None,
            second_index: None,
            first_cursor: 0,
            second_cursor: 0,
            first_initials: Vec::new(),
            second_initials: Vec::new(),
            seeded: false,
            arena: Vec::new(),
            frontier: VecDeque::new(),
            emitted: Vec::new(),
            path_limits,
            session: OperationSession::new(plan, limits, cancellation),
            last_checkpoint: None,
        })
    }

    /// Resume only the last checkpoint of this live continuation.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        path_limits: SftCompositionLimits,
        cancellation: CancellationToken,
    ) -> Result<(), SftCompositionError> {
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

    /// Enumerate exact accepted paths of `second(first(input))` under a
    /// single shared resource/cancellation contract. Meters report heap bytes
    /// beyond each domain element's inline size. Function closures must be
    /// pure, total, stable and finite-output across resume; the adapter
    /// cannot preempt an individual generic callback invocation.
    ///
    /// # Errors
    ///
    /// Refuses binding drift, malformed taken targets and representation
    /// overflow. Exhausted limits and cancellation return `Incomplete`.
    pub fn run<MI, MM, MO>(
        &mut self,
        observed_first_binding: [u8; 32],
        observed_second_binding: [u8; 32],
        observed_input_binding: [u8; 32],
        input_meter: MI,
        middle_meter: MM,
        output_meter: MO,
    ) -> Result<
        OperationOutcome<Vec<ComposedSftWitness<A::Domain, B::Domain, C::Domain>>>,
        SftCompositionError,
    >
    where
        MI: Fn(&A::Domain) -> u64,
        MM: Fn(&B::Domain) -> u64,
        MO: Fn(&C::Domain) -> u64,
    {
        if observed_first_binding != self.first_binding
            || observed_second_binding != self.second_binding
            || observed_input_binding != self.input_binding
        {
            return Err(OperationError::StaleSource.into());
        }
        loop {
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            if self.first_index.is_none() {
                if let Err(reason) = self.prepare()? {
                    return Ok(self.incomplete(reason));
                }
            }
            if self.first_cursor < self.first.transitions.len() {
                if let Err(reason) = self.index_first()? {
                    return Ok(self.incomplete(reason));
                }
                continue;
            }
            if self.second_cursor < self.second.transitions.len() {
                if let Err(reason) = self.index_second()? {
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
            let step = match &self.arena[node].phase {
                Phase::Ready if self.arena[node].input_index == self.input.len() => {
                    self.finish(node, &input_meter, &middle_meter, &output_meter)?
                }
                Phase::Ready => self.expand_first(node, &middle_meter)?,
                Phase::Feeding { .. } => self.expand_second(node, &middle_meter, &output_meter)?,
            };
            if let Err(reason) = step {
                return Ok(self.incomplete(reason));
            }
        }
    }

    fn prepare(&mut self) -> Result<Result<(), IncompleteReason>, SftCompositionError> {
        let first_states = self.first.states.len();
        let second_states = self.second.states.len();
        if let Some(state) = self
            .first
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= first_states)
            .min()
        {
            return Err(SftCompositionError::InvalidInitial { stage: 1, state });
        }
        if let Some(state) = self
            .second
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= second_states)
            .min()
        {
            return Err(SftCompositionError::InvalidInitial { stage: 2, state });
        }
        let bucket_count = first_states
            .checked_add(second_states)
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let initial_count = self
            .first
            .initial_states
            .len()
            .checked_add(self.second.initial_states.len())
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let count = bucket_count
            .checked_add(initial_count)
            .and_then(|n| n.checked_add(1))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let heap = (bucket_count as u128)
            .checked_mul(size_of::<Vec<usize>>() as u128)
            .and_then(|n| {
                n.checked_add((initial_count as u128).checked_mul(size_of::<usize>() as u128)?)
            })
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work: u64::try_from(count).map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        self.first_index = Some(OrderedSftTransitionIndex::new(first_states));
        self.second_index = Some(OrderedSftTransitionIndex::new(second_states));
        self.first_initials = self.first.initial_states.iter().copied().collect();
        self.second_initials = self.second.initial_states.iter().copied().collect();
        self.first_initials.sort_unstable();
        self.second_initials.sort_unstable();
        Ok(Ok(()))
    }

    fn index_first(&mut self) -> Result<Result<(), IncompleteReason>, SftCompositionError> {
        let transition = &self.first.transitions[self.first_cursor];
        let indexed = transition.from < self.first.states.len();
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
        self.first_index
            .as_mut()
            .expect("prepared")
            .admit(transition.from, self.first_cursor);
        self.first_cursor += 1;
        Ok(Ok(()))
    }

    fn index_second(&mut self) -> Result<Result<(), IncompleteReason>, SftCompositionError> {
        let transition = &self.second.transitions[self.second_cursor];
        let indexed = transition.from < self.second.states.len();
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
        self.second_index
            .as_mut()
            .expect("prepared")
            .admit(transition.from, self.second_cursor);
        self.second_cursor += 1;
        Ok(Ok(()))
    }

    fn seed(&mut self) -> Result<Result<(), IncompleteReason>, SftCompositionError> {
        let roots = self
            .first_initials
            .len()
            .checked_mul(self.second_initials.len())
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if self.arena.len().checked_add(roots).is_none()
            || self.frontier.len().checked_add(roots).is_none()
        {
            return Err(SftCompositionError::ExhaustedRepresentation);
        }
        if roots > self.path_limits.max_frontier {
            return Ok(Err(IncompleteReason::FrontierLimit));
        }
        let heap = (roots as u128)
            .checked_mul((size_of::<Node<B::Domain, C::Domain>>() + size_of::<usize>()) as u128)
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work: u64::try_from(roots).map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        for &first_state in &self.first_initials {
            for &second_state in &self.second_initials {
                let index = self.arena.len();
                self.arena.push(Node {
                    parent: None,
                    first_state,
                    second_state,
                    input_index: 0,
                    middle_len: 0,
                    phase: Phase::Ready,
                    event: Event::Root,
                });
                self.frontier.push_back(index);
            }
        }
        self.seeded = true;
        Ok(Ok(()))
    }

    fn expand_first<MM>(
        &mut self,
        node: usize,
        middle_meter: &MM,
    ) -> Result<Result<(), IncompleteReason>, SftCompositionError>
    where
        MM: Fn(&B::Domain) -> u64,
    {
        if let Err(reason) = self.session.preview_dynamic(OperationCost {
            states: 1,
            work: 1,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let path = &self.arena[node];
        let first_state = path.first_state;
        let second_state = path.second_state;
        let input_index = path.input_index;
        let middle_len = path.middle_len;
        let outgoing = self
            .first_index
            .as_ref()
            .expect("prepared")
            .outgoing(first_state);
        let input = &self.input[input_index];
        let mut pending = Vec::new();
        let mut middle_count = 0_u64;
        let mut payload = 0_u64;
        for &transition_index in outgoing {
            let transition = &self.first.transitions[transition_index];
            if !self.first.input_algebra.evaluate(&transition.guard, input) {
                continue;
            }
            if transition.to >= self.first.states.len() {
                return Err(SftCompositionError::InvalidTarget {
                    stage: 1,
                    transition_index,
                    target: transition.to,
                });
            }
            let middle = transition.output.apply(input);
            if middle_len.checked_add(middle.len()).is_none() {
                return Err(SftCompositionError::ExhaustedRepresentation);
            }
            middle_count = middle_count
                .checked_add(
                    u64::try_from(middle.len())
                        .map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
                )
                .ok_or(SftCompositionError::ExhaustedRepresentation)?;
            for value in &middle {
                payload = payload
                    .checked_add(middle_meter(value))
                    .ok_or(SftCompositionError::ExhaustedRepresentation)?;
            }
            pending.push(FirstPending {
                transition_index,
                to: transition.to,
                middle,
            });
        }
        let next_frontier = self
            .frontier
            .len()
            .checked_sub(1)
            .and_then(|n| n.checked_add(pending.len()))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if next_frontier > self.path_limits.max_frontier {
            return Ok(Err(IncompleteReason::FrontierLimit));
        }
        if self.arena.len().checked_add(pending.len()).is_none()
            || input_index.checked_add(1).is_none()
        {
            return Err(SftCompositionError::ExhaustedRepresentation);
        }
        let inspected = u64::try_from(outgoing.len())
            .map_err(|_| SftCompositionError::ExhaustedRepresentation)?;
        let produced = u64::try_from(pending.len())
            .map_err(|_| SftCompositionError::ExhaustedRepresentation)?;
        let work = inspected
            .checked_add(produced)
            .and_then(|n| n.checked_add(middle_count))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let heap = (pending.len() as u128)
            .checked_mul((size_of::<Node<B::Domain, C::Domain>>() + size_of::<usize>()) as u128)
            .and_then(|n| {
                n.checked_add(
                    (pending.len() as u128).checked_mul(
                        (size_of::<Vec<B::Domain>>() + 2 * size_of::<usize>()) as u128,
                    )?,
                )
            })
            .and_then(|n| {
                n.checked_add((middle_count as u128).checked_mul(size_of::<B::Domain>() as u128)?)
            })
            .and_then(|n| n.checked_add(u128::from(payload)))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            arcs: inspected,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
        })? {
            return Ok(Err(reason));
        }
        self.frontier.pop_front();
        for entry in pending {
            let shared = Arc::new(entry.middle);
            let phase = if shared.is_empty() {
                Phase::Ready
            } else {
                Phase::Feeding {
                    middle: Arc::clone(&shared),
                    offset: 0,
                }
            };
            let index = self.arena.len();
            self.arena.push(Node {
                parent: Some(node),
                first_state: entry.to,
                second_state,
                input_index: input_index + 1,
                middle_len: middle_len + shared.len(),
                phase,
                event: Event::First {
                    transition_index: entry.transition_index,
                    input_index,
                    middle: shared,
                },
            });
            self.frontier.push_back(index);
        }
        Ok(Ok(()))
    }

    fn expand_second<MM, MO>(
        &mut self,
        node: usize,
        middle_meter: &MM,
        output_meter: &MO,
    ) -> Result<Result<(), IncompleteReason>, SftCompositionError>
    where
        MM: Fn(&B::Domain) -> u64,
        MO: Fn(&C::Domain) -> u64,
    {
        if let Err(reason) = self.session.preview_dynamic(OperationCost {
            states: 1,
            work: 1,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let path = &self.arena[node];
        let Phase::Feeding { middle, offset } = &path.phase else {
            unreachable!("feeding frame")
        };
        let offset = *offset;
        let first_state = path.first_state;
        let second_state = path.second_state;
        let input_index = path.input_index;
        let middle_len = path.middle_len;
        let intermediate_index = middle_len
            .checked_sub(middle.len())
            .and_then(|n| n.checked_add(offset))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let outgoing = self
            .second_index
            .as_ref()
            .expect("prepared")
            .outgoing(second_state);
        let value = &middle[offset];
        let mut pending = Vec::new();
        let mut output_count = 0_u64;
        let mut payload = 0_u64;
        for &transition_index in outgoing {
            let transition = &self.second.transitions[transition_index];
            if !self.second.input_algebra.evaluate(&transition.guard, value) {
                continue;
            }
            if transition.to >= self.second.states.len() {
                return Err(SftCompositionError::InvalidTarget {
                    stage: 2,
                    transition_index,
                    target: transition.to,
                });
            }
            let output = transition.output.apply(value);
            output_count = output_count
                .checked_add(
                    u64::try_from(output.len())
                        .map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
                )
                .ok_or(SftCompositionError::ExhaustedRepresentation)?;
            for element in &output {
                payload = payload
                    .checked_add(output_meter(element))
                    .ok_or(SftCompositionError::ExhaustedRepresentation)?;
            }
            pending.push(SecondPending {
                transition_index,
                to: transition.to,
                output,
            });
        }
        let next_frontier = self
            .frontier
            .len()
            .checked_sub(1)
            .and_then(|n| n.checked_add(pending.len()))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if next_frontier > self.path_limits.max_frontier {
            return Ok(Err(IncompleteReason::FrontierLimit));
        }
        if self.arena.len().checked_add(pending.len()).is_none() || offset.checked_add(1).is_none()
        {
            return Err(SftCompositionError::ExhaustedRepresentation);
        }
        let inspected = u64::try_from(outgoing.len())
            .map_err(|_| SftCompositionError::ExhaustedRepresentation)?;
        let produced = u64::try_from(pending.len())
            .map_err(|_| SftCompositionError::ExhaustedRepresentation)?;
        let input_payload = middle_meter(value);
        payload = payload
            .checked_add(
                input_payload
                    .checked_mul(produced)
                    .ok_or(SftCompositionError::ExhaustedRepresentation)?,
            )
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let work = inspected
            .checked_add(produced)
            .and_then(|n| n.checked_add(output_count))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let heap = (pending.len() as u128)
            .checked_mul((size_of::<Node<B::Domain, C::Domain>>() + size_of::<usize>()) as u128)
            .and_then(|n| {
                n.checked_add((output_count as u128).checked_mul(size_of::<C::Domain>() as u128)?)
            })
            .and_then(|n| n.checked_add(u128::from(payload)))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            arcs: inspected,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
        })? {
            return Ok(Err(reason));
        }
        let input_value = value.clone();
        let middle = Arc::clone(middle);
        self.frontier.pop_front();
        for entry in pending {
            let phase = if offset + 1 == middle.len() {
                Phase::Ready
            } else {
                Phase::Feeding {
                    middle: Arc::clone(&middle),
                    offset: offset + 1,
                }
            };
            let index = self.arena.len();
            self.arena.push(Node {
                parent: Some(node),
                first_state,
                second_state: entry.to,
                input_index,
                middle_len,
                phase,
                event: Event::Second {
                    transition_index: entry.transition_index,
                    intermediate_index,
                    input: input_value.clone(),
                    output: entry.output,
                },
            });
            self.frontier.push_back(index);
        }
        Ok(Ok(()))
    }

    fn finish<MI, MM, MO>(
        &mut self,
        node: usize,
        input_meter: &MI,
        middle_meter: &MM,
        output_meter: &MO,
    ) -> Result<Result<(), IncompleteReason>, SftCompositionError>
    where
        MI: Fn(&A::Domain) -> u64,
        MM: Fn(&B::Domain) -> u64,
        MO: Fn(&C::Domain) -> u64,
    {
        let path = &self.arena[node];
        if !self.first.accepting_states.contains(&path.first_state)
            || !self.second.accepting_states.contains(&path.second_state)
        {
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
                return Err(SftCompositionError::ExhaustedRepresentation);
            }
            reverse.push(cursor);
            cursor = parent;
        }
        reverse.reverse();
        let first_count = reverse
            .iter()
            .filter(|&&id| matches!(self.arena[id].event, Event::First { .. }))
            .count();
        let second_count = reverse.len() - first_count;
        if first_count != self.input.len() {
            return Err(SftCompositionError::ExhaustedRepresentation);
        }
        let mut intermediate_len = 0usize;
        let mut output_len = 0usize;
        let mut payload = 0_u64;
        for &id in &reverse {
            match &self.arena[id].event {
                Event::First {
                    transition_index,
                    input_index,
                    middle,
                } => {
                    if *input_index >= self.input.len()
                        || self.first.transitions.get(*transition_index).is_none()
                    {
                        return Err(SftCompositionError::ExhaustedRepresentation);
                    }
                    intermediate_len = intermediate_len
                        .checked_add(middle.len())
                        .ok_or(SftCompositionError::ExhaustedRepresentation)?;
                    payload = payload
                        .checked_add(input_meter(&self.input[*input_index]))
                        .ok_or(SftCompositionError::ExhaustedRepresentation)?;
                    for value in middle.iter() {
                        payload = payload
                            .checked_add(middle_meter(value))
                            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
                    }
                }
                Event::Second {
                    transition_index,
                    intermediate_index,
                    input,
                    output,
                } => {
                    if *intermediate_index >= intermediate_len
                        || self.second.transitions.get(*transition_index).is_none()
                    {
                        return Err(SftCompositionError::ExhaustedRepresentation);
                    }
                    output_len = output_len
                        .checked_add(output.len())
                        .ok_or(SftCompositionError::ExhaustedRepresentation)?;
                    payload = payload
                        .checked_add(middle_meter(input))
                        .ok_or(SftCompositionError::ExhaustedRepresentation)?;
                    for value in output {
                        payload = payload
                            .checked_add(output_meter(value))
                            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
                    }
                }
                Event::Root => return Err(SftCompositionError::ExhaustedRepresentation),
            }
        }
        if intermediate_len != second_count {
            return Err(SftCompositionError::ExhaustedRepresentation);
        }
        let heap = (first_count as u128)
            .checked_mul(size_of::<FirstSftStep<A::Domain>>() as u128)
            .and_then(|n| {
                n.checked_add(
                    (second_count as u128)
                        .checked_mul(size_of::<SecondSftStep<B::Domain>>() as u128)?,
                )
            })
            .and_then(|n| {
                n.checked_add(
                    (intermediate_len as u128).checked_mul(size_of::<B::Domain>() as u128)?,
                )
            })
            .and_then(|n| {
                n.checked_add((output_len as u128).checked_mul(size_of::<C::Domain>() as u128)?)
            })
            .and_then(|n| {
                n.checked_add(
                    size_of::<ComposedSftWitness<A::Domain, B::Domain, C::Domain>>() as u128,
                )
            })
            .and_then(|n| n.checked_add(u128::from(payload)))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        let work = u64::try_from(reverse.len())
            .ok()
            .and_then(|n| n.checked_add(u64::try_from(intermediate_len).ok()?))
            .and_then(|n| n.checked_add(u64::try_from(output_len).ok()?))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftCompositionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftCompositionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let root = &self.arena[cursor];
        let mut intermediate = Vec::with_capacity(intermediate_len);
        let mut output = Vec::with_capacity(output_len);
        let mut first_steps = Vec::with_capacity(first_count);
        let mut second_steps = Vec::with_capacity(second_count);
        for id in reverse {
            match &self.arena[id].event {
                Event::First {
                    transition_index,
                    input_index,
                    middle,
                } => {
                    let transition = &self.first.transitions[*transition_index];
                    let start = intermediate.len();
                    intermediate.extend(middle.iter().cloned());
                    first_steps.push(FirstSftStep {
                        from: transition.from,
                        transition_index: *transition_index,
                        input_index: *input_index,
                        input: self.input[*input_index].clone(),
                        to: transition.to,
                        intermediate_range: start..intermediate.len(),
                    });
                }
                Event::Second {
                    transition_index,
                    intermediate_index,
                    input,
                    output: fragment,
                } => {
                    let transition = &self.second.transitions[*transition_index];
                    let start = output.len();
                    output.extend(fragment.iter().cloned());
                    second_steps.push(SecondSftStep {
                        from: transition.from,
                        transition_index: *transition_index,
                        intermediate_index: *intermediate_index,
                        input: input.clone(),
                        to: transition.to,
                        output_range: start..output.len(),
                    });
                }
                Event::Root => return Err(SftCompositionError::ExhaustedRepresentation),
            }
        }
        let witness = ComposedSftWitness {
            first_binding: self.first_binding,
            second_binding: self.second_binding,
            input_binding: self.input_binding,
            first_initial: root.first_state,
            first_final: path.first_state,
            second_initial: root.second_state,
            second_final: path.second_state,
            first_steps,
            second_steps,
            intermediate,
            output,
        };
        self.frontier.pop_front();
        self.emitted.push(witness);
        Ok(Ok(()))
    }

    fn incomplete(
        &mut self,
        reason: IncompleteReason,
    ) -> OperationOutcome<Vec<ComposedSftWitness<A::Domain, B::Domain, C::Domain>>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: self.emitted.clone(),
            reason,
            checkpoint,
        }
    }

    fn complete(
        &mut self,
    ) -> OperationOutcome<Vec<ComposedSftWitness<A::Domain, B::Domain, C::Domain>>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Complete {
            value: self.emitted.clone(),
            checkpoint,
        }
    }
}
