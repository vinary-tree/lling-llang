//! Bounded exact SFT domain restriction by a symbolic input acceptor.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::mem::size_of;

use super::bounded_transduce::OrderedSftTransitionIndex;
use super::sfa::ExactAlgebraSemantics;
use super::sft::{OutputFunction, SftState, SftTransition, SymbolicFiniteTransducer};
use super::{BooleanAlgebra, SymbolicAutomaton};
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, SourceSnapshot};

const ALGORITHM_ID: &str = "lling.sft.exact-domain-restriction/v1";

/// Rejection outside the exact bounded domain-restriction contract.
#[derive(Debug)]
pub enum SftRestrictionError {
    /// The SFT and input-SFA instances interpret input predicates differently.
    IncompatibleAlgebras,
    /// An initial state is not declared in its source.
    InvalidInitial { source: u8, state: usize },
    /// A reachable transition targets no declared state.
    InvalidTarget {
        source: u8,
        transition_index: usize,
        target: usize,
    },
    /// A content binding is absent.
    InvalidBinding,
    /// Checked size or cost arithmetic overflow.
    ExhaustedRepresentation,
    /// Shared operation-contract failure.
    Operation(OperationError),
}

impl From<OperationError> for SftRestrictionError {
    fn from(value: OperationError) -> Self {
        Self::Operation(value)
    }
}

impl fmt::Display for SftRestrictionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SftRestrictionError {}

struct Pending<A: BooleanAlgebra, B: BooleanAlgebra> {
    target: (usize, usize),
    guard: A::Predicate,
    output: OutputFunction<A, B>,
}

/// Reachable product of an SFT and input SFA, retaining the SFT's complete
/// output functions and transition multiplicity. Expansion is iterative and
/// each product-state batch is all-or-nothing under the shared limits.
pub struct BoundedSftRestriction<A, B>
where
    A: ExactAlgebraSemantics,
    B: BooleanAlgebra,
{
    sft: SymbolicFiniteTransducer<A, B>,
    input: SymbolicAutomaton<A>,
    sft_binding: [u8; 32],
    input_binding: [u8; 32],
    sft_index: Option<OrderedSftTransitionIndex>,
    input_index: Option<Vec<Vec<usize>>>,
    sft_cursor: usize,
    input_cursor: usize,
    initial_sft: Vec<usize>,
    initial_input: Vec<usize>,
    seeded: bool,
    state_map: HashMap<(usize, usize), usize>,
    frontier: VecDeque<(usize, usize)>,
    result: SymbolicFiniteTransducer<A, B>,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<A, B> BoundedSftRestriction<A, B>
where
    A: ExactAlgebraSemantics,
    B: BooleanAlgebra,
{
    /// Bind owned sources to immutable, nonzero semantic identities.
    ///
    /// # Errors
    ///
    /// Refuses mismatched input-algebra semantics, missing bindings or an
    /// invalid operation plan.
    pub fn new(
        sft: SymbolicFiniteTransducer<A, B>,
        input: SymbolicAutomaton<A>,
        sft_binding: [u8; 32],
        input_binding: [u8; 32],
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, SftRestrictionError> {
        if !sft.input_algebra.same_semantics(&input.algebra) {
            return Err(SftRestrictionError::IncompatibleAlgebras);
        }
        if sft_binding == [0; 32] || input_binding == [0; 32] {
            return Err(SftRestrictionError::InvalidBinding);
        }
        let mut digest = blake3::Hasher::new();
        digest.update(b"lling.sft.exact-domain-restriction-sources/v1\0");
        digest.update(&sft_binding);
        digest.update(&input_binding);
        let plan = OperationPlan::new_dynamic(
            SourceSnapshot::IMMUTABLE,
            *digest.finalize().as_bytes(),
            ALGORITHM_ID,
        )?;
        let result =
            SymbolicFiniteTransducer::new(sft.input_algebra.clone(), sft.output_algebra.clone());
        Ok(Self {
            sft,
            input,
            sft_binding,
            input_binding,
            sft_index: None,
            input_index: None,
            sft_cursor: 0,
            input_cursor: 0,
            initial_sft: Vec::new(),
            initial_input: Vec::new(),
            seeded: false,
            state_map: HashMap::new(),
            frontier: VecDeque::new(),
            result,
            session: OperationSession::new(plan, limits, cancellation),
            last_checkpoint: None,
        })
    }

    /// Resume only the exact last checkpoint of this live machine.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<(), SftRestrictionError> {
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

    /// Build the exact restricted transducer. Meters count payload bytes
    /// beyond inline predicate/constant-element sizes. Opaque output closures
    /// are cloned by `Arc` and must remain stable under the SFT binding.
    ///
    /// # Errors
    ///
    /// Refuses source drift, malformed reachable endpoints and arithmetic
    /// overflow. Resource limits return `Incomplete` with a checkpoint.
    pub fn run<PM, OM>(
        &mut self,
        observed_sft: [u8; 32],
        observed_input: [u8; 32],
        predicate_meter: PM,
        output_meter: OM,
    ) -> Result<OperationOutcome<SymbolicFiniteTransducer<A, B>>, SftRestrictionError>
    where
        PM: Fn(&A::Predicate) -> u64,
        OM: Fn(&OutputFunction<A, B>) -> u64,
    {
        if observed_sft != self.sft_binding || observed_input != self.input_binding {
            return Err(OperationError::StaleSource.into());
        }
        loop {
            if let Err(reason) = self.session.poll() {
                return Ok(self.incomplete(reason));
            }
            if self.sft_index.is_none() {
                if let Err(reason) = self.prepare()? {
                    return Ok(self.incomplete(reason));
                }
            }
            if self.sft_cursor < self.sft.transitions.len() {
                if let Err(reason) = self.index_sft()? {
                    return Ok(self.incomplete(reason));
                }
                continue;
            }
            if self.input_cursor < self.input.transitions.len() {
                if let Err(reason) = self.index_input()? {
                    return Ok(self.incomplete(reason));
                }
                continue;
            }
            if !self.seeded {
                if let Err(reason) = self.seed()? {
                    return Ok(self.incomplete(reason));
                }
            }
            let Some(pair) = self.frontier.front().copied() else {
                return Ok(self.complete());
            };
            if let Err(reason) = self.expand(pair, &predicate_meter, &output_meter)? {
                return Ok(self.incomplete(reason));
            }
        }
    }

    fn prepare(&mut self) -> Result<Result<(), IncompleteReason>, SftRestrictionError> {
        let sft_states = self.sft.states.len();
        let input_states = self.input.states.len();
        if let Some(state) = self
            .sft
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= sft_states)
            .min()
        {
            return Err(SftRestrictionError::InvalidInitial { source: 1, state });
        }
        if let Some(state) = self
            .input
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= input_states)
            .min()
        {
            return Err(SftRestrictionError::InvalidInitial { source: 2, state });
        }
        let bucket_count = sft_states
            .checked_add(input_states)
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        let initial_count = self
            .sft
            .initial_states
            .len()
            .checked_add(self.input.initial_states.len())
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        let work = bucket_count
            .checked_add(initial_count)
            .and_then(|n| n.checked_add(1))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        let heap = (bucket_count as u128)
            .checked_mul(size_of::<Vec<usize>>() as u128)
            .and_then(|n| {
                n.checked_add((initial_count as u128).checked_mul(size_of::<usize>() as u128)?)
            })
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftRestrictionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        self.sft_index = Some(OrderedSftTransitionIndex::new(sft_states));
        self.input_index = Some((0..input_states).map(|_| Vec::new()).collect());
        self.initial_sft = self.sft.initial_states.iter().copied().collect();
        self.initial_input = self.input.initial_states.iter().copied().collect();
        self.initial_sft.sort_unstable();
        self.initial_input.sort_unstable();
        Ok(Ok(()))
    }

    fn index_sft(&mut self) -> Result<Result<(), IncompleteReason>, SftRestrictionError> {
        let transition = &self.sft.transitions[self.sft_cursor];
        let indexed = transition.from < self.sft.states.len();
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
        self.sft_index
            .as_mut()
            .expect("prepared")
            .admit(transition.from, self.sft_cursor);
        self.sft_cursor += 1;
        Ok(Ok(()))
    }

    fn index_input(&mut self) -> Result<Result<(), IncompleteReason>, SftRestrictionError> {
        let transition = &self.input.transitions[self.input_cursor];
        let indexed = transition.from < self.input.states.len();
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
            self.input_index.as_mut().expect("prepared")[transition.from].push(self.input_cursor);
        }
        self.input_cursor += 1;
        Ok(Ok(()))
    }

    fn seed(&mut self) -> Result<Result<(), IncompleteReason>, SftRestrictionError> {
        let roots = self
            .initial_sft
            .len()
            .checked_mul(self.initial_input.len())
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        self.result
            .states
            .len()
            .checked_add(roots)
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        let heap = (roots as u128)
            .checked_mul((size_of::<(usize, usize)>() * 2 + size_of::<SftState>()) as u128)
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work: u64::try_from(roots).map_err(|_| SftRestrictionError::ExhaustedRepresentation)?,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftRestrictionError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        for &sft_state in &self.initial_sft {
            for &input_state in &self.initial_input {
                let accepting = self.sft.accepting_states.contains(&sft_state)
                    && self.input.accepting_states.contains(&input_state);
                let id = self.result.add_state(accepting, None);
                self.result.set_initial(id);
                self.state_map.insert((sft_state, input_state), id);
                self.frontier.push_back((sft_state, input_state));
            }
        }
        self.seeded = true;
        Ok(Ok(()))
    }

    fn expand<PM, OM>(
        &mut self,
        pair: (usize, usize),
        predicate_meter: &PM,
        output_meter: &OM,
    ) -> Result<Result<(), IncompleteReason>, SftRestrictionError>
    where
        PM: Fn(&A::Predicate) -> u64,
        OM: Fn(&OutputFunction<A, B>) -> u64,
    {
        if let Err(reason) = self.session.preview_dynamic(OperationCost {
            states: 1,
            work: 1,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let sft_outgoing = self.sft_index.as_ref().expect("prepared").outgoing(pair.0);
        let input_outgoing = &self.input_index.as_ref().expect("prepared")[pair.1];
        let mut pending: Vec<Pending<A, B>> = Vec::new();
        let mut tested_pairs = 0_u64;
        let mut payload = 0_u64;
        for &sft_index in sft_outgoing {
            let sft_transition = &self.sft.transitions[sft_index];
            if !self.sft.input_algebra.is_satisfiable(&sft_transition.guard) {
                continue;
            }
            if sft_transition.to >= self.sft.states.len() {
                return Err(SftRestrictionError::InvalidTarget {
                    source: 1,
                    transition_index: sft_index,
                    target: sft_transition.to,
                });
            }
            for &input_index in input_outgoing {
                tested_pairs = tested_pairs
                    .checked_add(1)
                    .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
                let input_transition = &self.input.transitions[input_index];
                let guard = self
                    .sft
                    .input_algebra
                    .and(&sft_transition.guard, &input_transition.guard);
                if !self.sft.input_algebra.is_satisfiable(&guard) {
                    continue;
                }
                if input_transition.to >= self.input.states.len() {
                    return Err(SftRestrictionError::InvalidTarget {
                        source: 2,
                        transition_index: input_index,
                        target: input_transition.to,
                    });
                }
                payload = payload
                    .checked_add(predicate_meter(&guard))
                    .and_then(|n| n.checked_add(output_meter(&sft_transition.output)))
                    .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
                if let OutputFunction::Constant(values) = &sft_transition.output {
                    let bytes = (values.len() as u128)
                        .checked_mul(size_of::<B::Domain>() as u128)
                        .and_then(|n| u64::try_from(n).ok())
                        .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
                    payload = payload
                        .checked_add(bytes)
                        .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
                }
                pending.push(Pending {
                    target: (sft_transition.to, input_transition.to),
                    guard,
                    output: sft_transition.output.clone(),
                });
            }
        }
        if let Err(reason) = self.session.poll() {
            return Ok(Err(reason));
        }
        let mut new_pairs = Vec::new();
        let mut seen = HashSet::new();
        for entry in &pending {
            if !self.state_map.contains_key(&entry.target) && seen.insert(entry.target) {
                new_pairs.push(entry.target);
            }
        }
        self.result
            .states
            .len()
            .checked_add(new_pairs.len())
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        self.result
            .transitions
            .len()
            .checked_add(pending.len())
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        let inspected = u64::try_from(sft_outgoing.len())
            .map_err(|_| SftRestrictionError::ExhaustedRepresentation)?;
        let produced = u64::try_from(pending.len())
            .map_err(|_| SftRestrictionError::ExhaustedRepresentation)?;
        let work = inspected
            .checked_add(tested_pairs)
            .and_then(|n| n.checked_add(produced))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        let heap = (new_pairs.len() as u128)
            .checked_mul((size_of::<(usize, usize)>() * 2 + size_of::<SftState>()) as u128)
            .and_then(|n| {
                n.checked_add(
                    (pending.len() as u128).checked_mul(size_of::<SftTransition<A, B>>() as u128)?,
                )
            })
            .and_then(|n| n.checked_add(u128::from(payload)))
            .ok_or(SftRestrictionError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            arcs: inspected
                .checked_add(tested_pairs)
                .ok_or(SftRestrictionError::ExhaustedRepresentation)?,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftRestrictionError::ExhaustedRepresentation)?,
        })? {
            return Ok(Err(reason));
        }
        self.frontier.pop_front();
        for product in new_pairs {
            let accepting = self.sft.accepting_states.contains(&product.0)
                && self.input.accepting_states.contains(&product.1);
            let id = self.result.add_state(accepting, None);
            self.state_map.insert(product, id);
            self.frontier.push_back(product);
        }
        let from = self.state_map[&pair];
        for entry in pending {
            let to = self.state_map[&entry.target];
            self.result
                .add_transition(from, to, entry.guard, entry.output);
        }
        Ok(Ok(()))
    }

    fn incomplete(
        &mut self,
        reason: IncompleteReason,
    ) -> OperationOutcome<SymbolicFiniteTransducer<A, B>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: self.result.clone(),
            reason,
            checkpoint,
        }
    }

    fn complete(&mut self) -> OperationOutcome<SymbolicFiniteTransducer<A, B>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Complete {
            value: self.result.clone(),
            checkpoint,
        }
    }
}
