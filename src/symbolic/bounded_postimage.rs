//! Bounded exact SFT/SFA post-image with an epsilon-capable output automaton.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::mem::size_of;

use super::bounded_transduce::OrderedSftTransitionIndex;
use super::sfa::{
    CharClassAlgebra, CharClassPred, ExactAlgebraSemantics, IntervalAlgebra, IntervalPred,
    SymbolicState,
};
use super::sft::{OutputFunction, SymbolicFiniteTransducer};
use super::{BooleanAlgebra, SymbolicAutomaton};
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, SourceSnapshot};

const ALGORITHM_ID: &str = "lling.sft.exact-postimage/v1";

/// One output edge. `None` is epsilon and consumes no symbol.
#[derive(Clone, Debug)]
pub struct OutputEdge<B: BooleanAlgebra> {
    /// Source state.
    pub from: usize,
    /// Destination state.
    pub to: usize,
    /// Exact output predicate, or epsilon.
    pub guard: Option<B::Predicate>,
}

/// Finite symbolic NFA with explicit epsilon edges. A plain SFA cannot
/// represent zero-output transitions or split constant output words.
#[derive(Clone, Debug)]
pub struct OutputAutomaton<B: BooleanAlgebra> {
    /// Output algebra.
    pub algebra: B,
    /// Declared states, including intermediate word-chain states.
    pub states: Vec<SymbolicState>,
    /// Ordered epsilon or guarded transitions.
    pub transitions: Vec<OutputEdge<B>>,
    /// Initial state IDs.
    pub initial_states: HashSet<usize>,
    /// Accepting state IDs.
    pub accepting_states: HashSet<usize>,
}

impl<B: BooleanAlgebra> OutputAutomaton<B> {
    fn new(algebra: B) -> Self {
        Self {
            algebra,
            states: Vec::new(),
            transitions: Vec::new(),
            initial_states: HashSet::new(),
            accepting_states: HashSet::new(),
        }
    }

    fn add_state(&mut self, accepting: bool) -> usize {
        let id = self.states.len();
        self.states.push(SymbolicState {
            id,
            is_accepting: accepting,
            label: None,
        });
        if accepting {
            self.accepting_states.insert(id);
        }
        id
    }

    fn closure(&self, current: &mut HashSet<usize>, outgoing: &[Vec<usize>]) {
        let mut queue: VecDeque<_> = current.iter().copied().collect();
        while let Some(state) = queue.pop_front() {
            for &edge_id in &outgoing[state] {
                let edge = &self.transitions[edge_id];
                if edge.guard.is_none() && current.insert(edge.to) {
                    queue.push_back(edge.to);
                }
            }
        }
    }

    /// Exact concrete membership with iterative epsilon closure. Malformed
    /// user-mutated endpoints are ignored rather than indexed out of bounds.
    #[must_use]
    pub fn accepts(&self, word: &[B::Domain]) -> bool {
        let mut outgoing = vec![Vec::new(); self.states.len()];
        for (index, edge) in self.transitions.iter().enumerate() {
            if edge.from < self.states.len() && edge.to < self.states.len() {
                outgoing[edge.from].push(index);
            }
        }
        let mut current: HashSet<_> = self
            .initial_states
            .iter()
            .copied()
            .filter(|&state| state < self.states.len())
            .collect();
        self.closure(&mut current, &outgoing);
        for symbol in word {
            let mut next = HashSet::new();
            for &state in &current {
                for &edge_id in &outgoing[state] {
                    let edge = &self.transitions[edge_id];
                    if edge
                        .guard
                        .as_ref()
                        .is_some_and(|guard| self.algebra.evaluate(guard, symbol))
                    {
                        next.insert(edge.to);
                    }
                }
            }
            self.closure(&mut next, &outgoing);
            current = next;
        }
        current.iter().any(|s| self.accepting_states.contains(s))
    }
}

/// Construct exact singleton predicates for concrete output constants.
/// Returning `None` means this algebra cannot denote that singleton.
pub trait ExactSingletonPredicate: ExactAlgebraSemantics {
    /// A predicate satisfied by precisely this domain element, if expressible.
    fn singleton(&self, value: &Self::Domain) -> Option<Self::Predicate>;
}

impl ExactSingletonPredicate for CharClassAlgebra {
    fn singleton(&self, value: &char) -> Option<CharClassPred> {
        Some(CharClassPred::Range(*value, *value))
    }
}

impl ExactSingletonPredicate for IntervalAlgebra {
    fn singleton(&self, value: &i64) -> Option<IntervalPred> {
        if *value < self.min_val || *value >= self.max_val {
            return None;
        }
        value
            .checked_add(1)
            .map(|end| IntervalPred::Range(*value, end))
    }
}

/// One exact rectangular case of the input/output relation. For all input
/// symbols satisfying `input_guard`, every word matching `output_word` is an
/// output of the function, and all its outputs must be covered by some case.
/// Cases may overlap. An empty output word denotes epsilon.
pub struct PostimageCase<A: BooleanAlgebra, B: BooleanAlgebra> {
    /// Exact input subregion.
    pub input_guard: A::Predicate,
    /// A word of output predicates, possibly empty.
    pub output_word: Vec<B::Predicate>,
}

/// Typed inability to describe an output function exactly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostimageOracleError {
    /// No exact finite rectangular representation exists or was supplied.
    UnrepresentableOutput,
    /// A constant symbol has no representable singleton predicate.
    UnrepresentableConstant,
    /// Input/output algebra instances do not share predicate semantics.
    IncompatibleAlgebras,
}

/// Exact relation-image oracle. Implementations must return a finite union of
/// rectangular cases equal to the output relation restricted by `input_guard`.
/// The callback must be pure and stable under the caller's binding; it is not
/// preemptible during one invocation. Approximate cases must return an error.
pub trait ExactOutputPostimageOracle<A: BooleanAlgebra, B: BooleanAlgebra>: Send + Sync {
    /// Derive the complete exact case set for one transition.
    fn cases(
        &self,
        input_algebra: &A,
        output_algebra: &B,
        input_guard: &A::Predicate,
        output: &OutputFunction<A, B>,
    ) -> Result<Vec<PostimageCase<A, B>>, PostimageOracleError>;
}

/// Built-in exact cases for same-algebra epsilon, constant and identity.
/// Computed maps require a caller-supplied exact oracle.
#[derive(Clone, Copy, Debug, Default)]
pub struct SameAlgebraPostimageOracle;

impl<A: ExactSingletonPredicate> ExactOutputPostimageOracle<A, A> for SameAlgebraPostimageOracle {
    fn cases(
        &self,
        input_algebra: &A,
        output_algebra: &A,
        input_guard: &A::Predicate,
        output: &OutputFunction<A, A>,
    ) -> Result<Vec<PostimageCase<A, A>>, PostimageOracleError> {
        let output_word = match output {
            OutputFunction::Epsilon => Vec::new(),
            OutputFunction::Constant(values) => values
                .iter()
                .map(|value| {
                    output_algebra
                        .singleton(value)
                        .ok_or(PostimageOracleError::UnrepresentableConstant)
                })
                .collect::<Result<Vec<_>, _>>()?,
            OutputFunction::Identity => {
                if !input_algebra.same_semantics(output_algebra) {
                    return Err(PostimageOracleError::IncompatibleAlgebras);
                }
                vec![input_guard.clone()]
            }
            OutputFunction::Map(_) | OutputFunction::FlatMap(_) => {
                return Err(PostimageOracleError::UnrepresentableOutput);
            }
        };
        Ok(vec![PostimageCase {
            input_guard: input_guard.clone(),
            output_word,
        }])
    }
}

/// Error outside the exact bounded construction domain.
#[derive(Debug)]
pub enum SftPostimageError {
    /// Invalid source initial state.
    InvalidInitial { source: u8, state: usize },
    /// Reachable malformed source target.
    InvalidTarget {
        source: u8,
        transition_index: usize,
        target: usize,
    },
    /// Exact image oracle refused a source transition.
    Oracle {
        transition_index: usize,
        error: PostimageOracleError,
    },
    /// Missing content or oracle binding.
    InvalidBinding,
    /// Checked size or cost arithmetic overflow.
    ExhaustedRepresentation,
    /// Shared operation-contract failure.
    Operation(OperationError),
}

impl From<OperationError> for SftPostimageError {
    fn from(value: OperationError) -> Self {
        Self::Operation(value)
    }
}

impl fmt::Display for SftPostimageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SftPostimageError {}

struct Pending<B: BooleanAlgebra> {
    target: (usize, usize),
    output_word: Vec<B::Predicate>,
}

/// Iterative reachable SFA/SFT product and exact output projection. Product
/// states are charged as visits; intermediate output states and edges are
/// charged as heap and work before each atomic product-state expansion.
pub struct BoundedSftPostimage<A, B, P>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    P: ExactOutputPostimageOracle<A, B>,
{
    sft: SymbolicFiniteTransducer<A, B>,
    input: SymbolicAutomaton<A>,
    oracle: P,
    sft_binding: [u8; 32],
    input_binding: [u8; 32],
    oracle_binding: [u8; 32],
    sft_index: Option<OrderedSftTransitionIndex>,
    input_index: Option<Vec<Vec<usize>>>,
    sft_cursor: usize,
    input_cursor: usize,
    initial_sft: Vec<usize>,
    initial_input: Vec<usize>,
    seeded: bool,
    state_map: HashMap<(usize, usize), usize>,
    frontier: VecDeque<(usize, usize)>,
    result: OutputAutomaton<B>,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<A, B, P> BoundedSftPostimage<A, B, P>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    P: ExactOutputPostimageOracle<A, B>,
{
    /// Bind owned sources and exact oracle semantics to nonzero identities.
    ///
    /// # Errors
    ///
    /// Refuses missing bindings or invalid operation plans.
    pub fn new(
        sft: SymbolicFiniteTransducer<A, B>,
        input: SymbolicAutomaton<A>,
        oracle: P,
        sft_binding: [u8; 32],
        input_binding: [u8; 32],
        oracle_binding: [u8; 32],
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, SftPostimageError> {
        if sft_binding == [0; 32] || input_binding == [0; 32] || oracle_binding == [0; 32] {
            return Err(SftPostimageError::InvalidBinding);
        }
        let mut digest = blake3::Hasher::new();
        digest.update(b"lling.sft.exact-postimage-sources/v1\0");
        digest.update(&sft_binding);
        digest.update(&input_binding);
        digest.update(&oracle_binding);
        let plan = OperationPlan::new_dynamic(
            SourceSnapshot::IMMUTABLE,
            *digest.finalize().as_bytes(),
            ALGORITHM_ID,
        )?;
        let result = OutputAutomaton::new(sft.output_algebra.clone());
        Ok(Self {
            sft,
            input,
            oracle,
            sft_binding,
            input_binding,
            oracle_binding,
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
    ) -> Result<(), SftPostimageError> {
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

    /// Build the exact reachable post-image graph. `predicate_meter` reports
    /// payload bytes beyond each predicate's inline size. Logical heap charges
    /// exclude allocator slack and oracle-private storage.
    ///
    /// # Errors
    ///
    /// Refuses source drift, malformed reachable endpoints, unrepresentable
    /// output relations and arithmetic overflow. Limits yield `Incomplete`.
    pub fn run<M>(
        &mut self,
        observed_sft: [u8; 32],
        observed_input: [u8; 32],
        observed_oracle: [u8; 32],
        predicate_meter: M,
    ) -> Result<OperationOutcome<OutputAutomaton<B>>, SftPostimageError>
    where
        M: Fn(&B::Predicate) -> u64,
    {
        if observed_sft != self.sft_binding
            || observed_input != self.input_binding
            || observed_oracle != self.oracle_binding
        {
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
            if let Err(reason) = self.expand(pair, &predicate_meter)? {
                return Ok(self.incomplete(reason));
            }
        }
    }

    fn prepare(&mut self) -> Result<Result<(), IncompleteReason>, SftPostimageError> {
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
            return Err(SftPostimageError::InvalidInitial { source: 1, state });
        }
        if let Some(state) = self
            .input
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= input_states)
            .min()
        {
            return Err(SftPostimageError::InvalidInitial { source: 2, state });
        }
        let bucket_count = sft_states
            .checked_add(input_states)
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        let initial_count = self
            .sft
            .initial_states
            .len()
            .checked_add(self.input.initial_states.len())
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        let work = bucket_count
            .checked_add(initial_count)
            .and_then(|n| n.checked_add(1))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        let heap = (bucket_count as u128)
            .checked_mul(size_of::<Vec<usize>>() as u128)
            .and_then(|n| {
                n.checked_add((initial_count as u128).checked_mul(size_of::<usize>() as u128)?)
            })
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftPostimageError::ExhaustedRepresentation)?,
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

    fn index_sft(&mut self) -> Result<Result<(), IncompleteReason>, SftPostimageError> {
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

    fn index_input(&mut self) -> Result<Result<(), IncompleteReason>, SftPostimageError> {
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

    fn seed(&mut self) -> Result<Result<(), IncompleteReason>, SftPostimageError> {
        let roots = self
            .initial_sft
            .len()
            .checked_mul(self.initial_input.len())
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        self.result
            .states
            .len()
            .checked_add(roots)
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        let heap = (roots as u128)
            .checked_mul((size_of::<(usize, usize)>() * 2 + size_of::<SymbolicState>()) as u128)
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work: u64::try_from(roots).map_err(|_| SftPostimageError::ExhaustedRepresentation)?,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftPostimageError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        for &sft_state in &self.initial_sft {
            for &input_state in &self.initial_input {
                let accepting = self.sft.accepting_states.contains(&sft_state)
                    && self.input.accepting_states.contains(&input_state);
                let id = self.result.add_state(accepting);
                self.result.initial_states.insert(id);
                self.state_map.insert((sft_state, input_state), id);
                self.frontier.push_back((sft_state, input_state));
            }
        }
        self.seeded = true;
        Ok(Ok(()))
    }

    fn expand<M>(
        &mut self,
        pair: (usize, usize),
        predicate_meter: &M,
    ) -> Result<Result<(), IncompleteReason>, SftPostimageError>
    where
        M: Fn(&B::Predicate) -> u64,
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
        let mut pending: Vec<Pending<B>> = Vec::new();
        let mut tested_pairs = 0_u64;
        let mut payload = 0_u64;
        let mut output_edges = 0_usize;
        let mut intermediate = 0_usize;
        for &sft_index in sft_outgoing {
            let sft_transition = &self.sft.transitions[sft_index];
            if !self.sft.input_algebra.is_satisfiable(&sft_transition.guard) {
                continue;
            }
            if sft_transition.to >= self.sft.states.len() {
                return Err(SftPostimageError::InvalidTarget {
                    source: 1,
                    transition_index: sft_index,
                    target: sft_transition.to,
                });
            }
            for &input_index in input_outgoing {
                tested_pairs = tested_pairs
                    .checked_add(1)
                    .ok_or(SftPostimageError::ExhaustedRepresentation)?;
                let input_transition = &self.input.transitions[input_index];
                let combined = self
                    .sft
                    .input_algebra
                    .and(&sft_transition.guard, &input_transition.guard);
                if !self.sft.input_algebra.is_satisfiable(&combined) {
                    continue;
                }
                if input_transition.to >= self.input.states.len() {
                    return Err(SftPostimageError::InvalidTarget {
                        source: 2,
                        transition_index: input_index,
                        target: input_transition.to,
                    });
                }
                let cases = self
                    .oracle
                    .cases(
                        &self.sft.input_algebra,
                        &self.sft.output_algebra,
                        &combined,
                        &sft_transition.output,
                    )
                    .map_err(|error| SftPostimageError::Oracle {
                        transition_index: sft_index,
                        error,
                    })?;
                for case in cases {
                    if !self
                        .sft
                        .input_algebra
                        .is_satisfiable(&self.sft.input_algebra.and(&combined, &case.input_guard))
                    {
                        continue;
                    }
                    if case
                        .output_word
                        .iter()
                        .any(|guard| !self.sft.output_algebra.is_satisfiable(guard))
                    {
                        continue;
                    }
                    for guard in &case.output_word {
                        payload = payload
                            .checked_add(predicate_meter(guard))
                            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
                    }
                    output_edges = output_edges
                        .checked_add(case.output_word.len().max(1))
                        .ok_or(SftPostimageError::ExhaustedRepresentation)?;
                    intermediate = intermediate
                        .checked_add(case.output_word.len().saturating_sub(1))
                        .ok_or(SftPostimageError::ExhaustedRepresentation)?;
                    pending.push(Pending {
                        target: (sft_transition.to, input_transition.to),
                        output_word: case.output_word,
                    });
                }
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
        let new_states = new_pairs
            .len()
            .checked_add(intermediate)
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        self.result
            .states
            .len()
            .checked_add(new_states)
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        self.result
            .transitions
            .len()
            .checked_add(output_edges)
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        let inspected = u64::try_from(sft_outgoing.len())
            .map_err(|_| SftPostimageError::ExhaustedRepresentation)?;
        let emitted =
            u64::try_from(output_edges).map_err(|_| SftPostimageError::ExhaustedRepresentation)?;
        let work = inspected
            .checked_add(tested_pairs)
            .and_then(|n| n.checked_add(emitted))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        let heap = (new_pairs.len() as u128)
            .checked_mul((size_of::<(usize, usize)>() * 2) as u128)
            .and_then(|n| {
                n.checked_add((new_states as u128).checked_mul(size_of::<SymbolicState>() as u128)?)
            })
            .and_then(|n| {
                n.checked_add(
                    (output_edges as u128).checked_mul(size_of::<OutputEdge<B>>() as u128)?,
                )
            })
            .and_then(|n| n.checked_add(u128::from(payload)))
            .ok_or(SftPostimageError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            arcs: inspected
                .checked_add(tested_pairs)
                .ok_or(SftPostimageError::ExhaustedRepresentation)?,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftPostimageError::ExhaustedRepresentation)?,
        })? {
            return Ok(Err(reason));
        }
        self.frontier.pop_front();
        for product in new_pairs {
            let accepting = self.sft.accepting_states.contains(&product.0)
                && self.input.accepting_states.contains(&product.1);
            let id = self.result.add_state(accepting);
            self.state_map.insert(product, id);
            self.frontier.push_back(product);
        }
        let from = self.state_map[&pair];
        for entry in pending {
            let to = self.state_map[&entry.target];
            if entry.output_word.is_empty() {
                self.result.transitions.push(OutputEdge {
                    from,
                    to,
                    guard: None,
                });
                continue;
            }
            let length = entry.output_word.len();
            let mut previous = from;
            for (position, guard) in entry.output_word.into_iter().enumerate() {
                let next = if position + 1 == length {
                    to
                } else {
                    self.result.add_state(false)
                };
                self.result.transitions.push(OutputEdge {
                    from: previous,
                    to: next,
                    guard: Some(guard),
                });
                previous = next;
            }
        }
        Ok(Ok(()))
    }

    fn incomplete(&mut self, reason: IncompleteReason) -> OperationOutcome<OutputAutomaton<B>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: self.result.clone(),
            reason,
            checkpoint,
        }
    }

    fn complete(&mut self) -> OperationOutcome<OutputAutomaton<B>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Complete {
            value: self.result.clone(),
            checkpoint,
        }
    }
}
