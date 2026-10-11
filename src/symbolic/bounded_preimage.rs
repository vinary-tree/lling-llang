//! Bounded exact symbolic pre-image with explicit output-guard pullbacks.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::mem::size_of;

use super::bounded_transduce::OrderedSftTransitionIndex;
use super::sft::{OutputFunction, SymbolicFiniteTransducer};
use super::{BooleanAlgebra, ExactAlgebraSemantics, SymbolicAutomaton};
use crate::wfst::operation::{
    IncompleteReason, OperationCheckpoint, OperationCost, OperationError, OperationLimits,
    OperationOutcome, OperationPlan, OperationSession,
};
use crate::wfst::{CancellationToken, SourceSnapshot};

const ALGORITHM_ID: &str = "lling.sft.exact-preimage/v1";

/// One exact symbolic input region and acceptor endpoint for an SFT output.
pub struct PreimageCase<P> {
    /// Acceptor state reached after the SFT transition's complete output.
    pub target_state: usize,
    /// Predicate true exactly on inputs following this acceptor path.
    pub input_guard: P,
}

/// Finite exact case set plus the oracle's charged source-inspection work.
pub struct PreimageCases<P> {
    /// Cases may overlap for nondeterministic acceptor paths.
    pub cases: Vec<PreimageCase<P>>,
    /// Acceptor transitions inspected while deriving these cases.
    pub inspected_arcs: u64,
    /// Additional abstract derivation work beyond inspected arcs and cases.
    pub work: u64,
}

/// A semantic inability to derive an exact finite predicate case set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreimageOracleError {
    /// No exact finite pullback is available for this output function.
    UnrepresentableOutput,
    /// Identity guards cannot be transferred across these algebra instances.
    IncompatibleAlgebras,
    /// A concretely taken acceptor transition targets no declared state.
    InvalidAcceptorTarget {
        transition_index: usize,
        target: usize,
    },
    /// The derivation's representation or accounting overflowed.
    ExhaustedRepresentation,
}

/// Contract for exact pre-image of one SFT output function through an SFA.
/// For every input element, the returned cases must describe **all and only**
/// acceptor endpoints reachable by consuming the output function's entire
/// output sequence. Implementors must be pure, deterministic and stable under
/// their caller-supplied binding; unproved approximation must return an error.
pub trait ExactOutputPreimageOracle<A: BooleanAlgebra, B: BooleanAlgebra>: Send + Sync {
    /// Derive a finite, exact case set from one acceptor start state.
    /// `acceptor_outgoing` stores ordered transition indices for each state.
    fn cases(
        &self,
        input_algebra: &A,
        output: &OutputFunction<A, B>,
        acceptor: &SymbolicAutomaton<B>,
        acceptor_outgoing: &[Vec<usize>],
        start_state: usize,
    ) -> Result<PreimageCases<A::Predicate>, PreimageOracleError>;
}

/// Built-in exact oracle when both SFT alphabets use the same algebra.
/// Epsilon, finite constants and identity are representable; map/flat-map
/// require a caller-supplied exact pullback oracle.
#[derive(Clone, Copy, Debug, Default)]
pub struct SameAlgebraPreimageOracle;

impl<A: ExactAlgebraSemantics> ExactOutputPreimageOracle<A, A> for SameAlgebraPreimageOracle {
    fn cases(
        &self,
        input_algebra: &A,
        output: &OutputFunction<A, A>,
        acceptor: &SymbolicAutomaton<A>,
        acceptor_outgoing: &[Vec<usize>],
        start_state: usize,
    ) -> Result<PreimageCases<A::Predicate>, PreimageOracleError> {
        match output {
            OutputFunction::Epsilon => Ok(PreimageCases {
                cases: vec![PreimageCase {
                    target_state: start_state,
                    input_guard: input_algebra.true_pred(),
                }],
                inspected_arcs: 0,
                work: 1,
            }),
            OutputFunction::Constant(values) => {
                let mut current = BTreeSet::from([start_state]);
                let mut inspected = 0_u64;
                let mut work = 0_u64;
                for value in values {
                    let mut next = BTreeSet::new();
                    for &state in &current {
                        let outgoing = acceptor_outgoing
                            .get(state)
                            .ok_or(PreimageOracleError::ExhaustedRepresentation)?;
                        for &index in outgoing {
                            inspected = inspected
                                .checked_add(1)
                                .ok_or(PreimageOracleError::ExhaustedRepresentation)?;
                            let transition = &acceptor.transitions[index];
                            if acceptor.algebra.evaluate(&transition.guard, value) {
                                if transition.to >= acceptor.states.len() {
                                    return Err(PreimageOracleError::InvalidAcceptorTarget {
                                        transition_index: index,
                                        target: transition.to,
                                    });
                                }
                                next.insert(transition.to);
                            }
                        }
                    }
                    work = work
                        .checked_add(
                            u64::try_from(current.len())
                                .map_err(|_| PreimageOracleError::ExhaustedRepresentation)?,
                        )
                        .ok_or(PreimageOracleError::ExhaustedRepresentation)?;
                    current = next;
                    if current.is_empty() {
                        break;
                    }
                }
                Ok(PreimageCases {
                    cases: current
                        .into_iter()
                        .map(|target_state| PreimageCase {
                            target_state,
                            input_guard: input_algebra.true_pred(),
                        })
                        .collect(),
                    inspected_arcs: inspected,
                    work,
                })
            }
            OutputFunction::Identity => {
                if !input_algebra.same_semantics(&acceptor.algebra) {
                    return Err(PreimageOracleError::IncompatibleAlgebras);
                }
                let outgoing = acceptor_outgoing
                    .get(start_state)
                    .ok_or(PreimageOracleError::ExhaustedRepresentation)?;
                let cases = outgoing
                    .iter()
                    .map(|&index| {
                        let transition = &acceptor.transitions[index];
                        PreimageCase {
                            target_state: transition.to,
                            input_guard: transition.guard.clone(),
                        }
                    })
                    .collect();
                Ok(PreimageCases {
                    cases,
                    inspected_arcs: u64::try_from(outgoing.len())
                        .map_err(|_| PreimageOracleError::ExhaustedRepresentation)?,
                    work: 0,
                })
            }
            OutputFunction::Map(_) | OutputFunction::FlatMap(_) => {
                Err(PreimageOracleError::UnrepresentableOutput)
            }
        }
    }
}

/// Rejection outside the declared exact symbolic domain.
#[derive(Debug)]
pub enum SftPreimageError {
    /// An initial state is not declared in its source.
    InvalidInitial { source: u8, state: usize },
    /// A reachable transition targets no declared state.
    InvalidTarget {
        source: u8,
        transition_index: usize,
        target: usize,
    },
    /// The oracle refused or could not represent an exact pullback.
    Oracle {
        transition_index: usize,
        error: PreimageOracleError,
    },
    /// A content or oracle binding is missing.
    InvalidBinding,
    /// Size or cost arithmetic was exhausted.
    ExhaustedRepresentation,
    /// Shared operation-contract error.
    Operation(OperationError),
}

impl From<OperationError> for SftPreimageError {
    fn from(value: OperationError) -> Self {
        Self::Operation(value)
    }
}

impl fmt::Display for SftPreimageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SftPreimageError {}

struct Pending<P> {
    sft_target: usize,
    acceptor_target: usize,
    guard: P,
}

/// Reachable exact SFT/SFA pre-image product construction. A custom oracle
/// may cover computed output functions when it can prove finite pullbacks.
pub struct BoundedSftPreimage<A, B, P>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    P: ExactOutputPreimageOracle<A, B>,
{
    sft: SymbolicFiniteTransducer<A, B>,
    acceptor: SymbolicAutomaton<B>,
    oracle: P,
    sft_binding: [u8; 32],
    acceptor_binding: [u8; 32],
    oracle_binding: [u8; 32],
    sft_index: Option<OrderedSftTransitionIndex>,
    acceptor_index: Option<Vec<Vec<usize>>>,
    sft_cursor: usize,
    acceptor_cursor: usize,
    initial_sft: Vec<usize>,
    initial_acceptor: Vec<usize>,
    seeded: bool,
    state_map: HashMap<(usize, usize), usize>,
    frontier: VecDeque<(usize, usize)>,
    result: SymbolicAutomaton<A>,
    session: OperationSession,
    last_checkpoint: Option<OperationCheckpoint>,
}

impl<A, B, P> BoundedSftPreimage<A, B, P>
where
    A: BooleanAlgebra,
    B: BooleanAlgebra,
    P: ExactOutputPreimageOracle<A, B>,
{
    /// Bind owned SFT, output acceptor and oracle semantics to nonzero IDs.
    ///
    /// # Errors
    ///
    /// Rejects missing bindings or an invalid operation plan.
    pub fn new(
        sft: SymbolicFiniteTransducer<A, B>,
        acceptor: SymbolicAutomaton<B>,
        oracle: P,
        sft_binding: [u8; 32],
        acceptor_binding: [u8; 32],
        oracle_binding: [u8; 32],
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, SftPreimageError> {
        if sft_binding == [0; 32] || acceptor_binding == [0; 32] || oracle_binding == [0; 32] {
            return Err(SftPreimageError::InvalidBinding);
        }
        let mut digest = blake3::Hasher::new();
        digest.update(b"lling.sft.exact-preimage-sources/v1\0");
        digest.update(&sft_binding);
        digest.update(&acceptor_binding);
        digest.update(&oracle_binding);
        let plan = OperationPlan::new_dynamic(
            SourceSnapshot::IMMUTABLE,
            *digest.finalize().as_bytes(),
            ALGORITHM_ID,
        )?;
        let result = SymbolicAutomaton::new(sft.input_algebra.clone());
        Ok(Self {
            sft,
            acceptor,
            oracle,
            sft_binding,
            acceptor_binding,
            oracle_binding,
            sft_index: None,
            acceptor_index: None,
            sft_cursor: 0,
            acceptor_cursor: 0,
            initial_sft: Vec::new(),
            initial_acceptor: Vec::new(),
            seeded: false,
            state_map: HashMap::new(),
            frontier: VecDeque::new(),
            result,
            session: OperationSession::new(plan, limits, cancellation),
            last_checkpoint: None,
        })
    }

    /// Resume the exact last checkpoint of this live machine.
    ///
    /// # Errors
    ///
    /// Refuses stale or fabricated checkpoints.
    pub fn resume(
        &mut self,
        checkpoint: OperationCheckpoint,
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<(), SftPreimageError> {
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

    /// Build the exact reachable pre-image graph. `predicate_meter` reports
    /// heap payload bytes beyond each predicate's inline size. An oracle
    /// implementation must be pure, exact, finite and stable across resume;
    /// its own callback is not preemptible during one invocation.
    ///
    /// # Errors
    ///
    /// Refuses drift, malformed reachable endpoints, unsupported output
    /// pullbacks and representation overflow. Limits return `Incomplete`.
    pub fn run<M>(
        &mut self,
        observed_sft: [u8; 32],
        observed_acceptor: [u8; 32],
        observed_oracle: [u8; 32],
        predicate_meter: M,
    ) -> Result<OperationOutcome<SymbolicAutomaton<A>>, SftPreimageError>
    where
        M: Fn(&A::Predicate) -> u64,
    {
        if observed_sft != self.sft_binding
            || observed_acceptor != self.acceptor_binding
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
            if self.acceptor_cursor < self.acceptor.transitions.len() {
                if let Err(reason) = self.index_acceptor()? {
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

    fn prepare(&mut self) -> Result<Result<(), IncompleteReason>, SftPreimageError> {
        let sft_states = self.sft.states.len();
        let acceptor_states = self.acceptor.states.len();
        if let Some(state) = self
            .sft
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= sft_states)
            .min()
        {
            return Err(SftPreimageError::InvalidInitial { source: 1, state });
        }
        if let Some(state) = self
            .acceptor
            .initial_states
            .iter()
            .copied()
            .filter(|&id| id >= acceptor_states)
            .min()
        {
            return Err(SftPreimageError::InvalidInitial { source: 2, state });
        }
        let bucket_count = sft_states
            .checked_add(acceptor_states)
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        let initial_count = self
            .sft
            .initial_states
            .len()
            .checked_add(self.acceptor.initial_states.len())
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        let work = bucket_count
            .checked_add(initial_count)
            .and_then(|n| n.checked_add(1))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        let heap = (bucket_count as u128)
            .checked_mul(size_of::<Vec<usize>>() as u128)
            .and_then(|n| {
                n.checked_add((initial_count as u128).checked_mul(size_of::<usize>() as u128)?)
            })
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftPreimageError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        self.sft_index = Some(OrderedSftTransitionIndex::new(sft_states));
        self.acceptor_index = Some((0..acceptor_states).map(|_| Vec::new()).collect());
        self.initial_sft = self.sft.initial_states.iter().copied().collect();
        self.initial_acceptor = self.acceptor.initial_states.iter().copied().collect();
        self.initial_sft.sort_unstable();
        self.initial_acceptor.sort_unstable();
        Ok(Ok(()))
    }

    fn index_sft(&mut self) -> Result<Result<(), IncompleteReason>, SftPreimageError> {
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

    fn index_acceptor(&mut self) -> Result<Result<(), IncompleteReason>, SftPreimageError> {
        let transition = &self.acceptor.transitions[self.acceptor_cursor];
        let indexed = transition.from < self.acceptor.states.len();
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
            self.acceptor_index.as_mut().expect("prepared")[transition.from]
                .push(self.acceptor_cursor);
        }
        self.acceptor_cursor += 1;
        Ok(Ok(()))
    }

    fn seed(&mut self) -> Result<Result<(), IncompleteReason>, SftPreimageError> {
        let roots = self
            .initial_sft
            .len()
            .checked_mul(self.initial_acceptor.len())
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        if self.result.states.len().checked_add(roots).is_none() {
            return Err(SftPreimageError::ExhaustedRepresentation);
        }
        let heap = (roots as u128)
            .checked_mul(
                (size_of::<(usize, usize)>() * 2 + size_of::<super::SymbolicState>()) as u128,
            )
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.charge(OperationCost {
            work: u64::try_from(roots).map_err(|_| SftPreimageError::ExhaustedRepresentation)?,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftPreimageError::ExhaustedRepresentation)?,
            ..OperationCost::default()
        }) {
            return Ok(Err(reason));
        }
        for &sft_state in &self.initial_sft {
            for &acceptor_state in &self.initial_acceptor {
                let accepting = self.sft.accepting_states.contains(&sft_state)
                    && self.acceptor.accepting_states.contains(&acceptor_state);
                let result_id = self.result.add_state(accepting, None);
                self.result.set_initial(result_id);
                self.state_map
                    .insert((sft_state, acceptor_state), result_id);
                self.frontier.push_back((sft_state, acceptor_state));
            }
        }
        self.seeded = true;
        Ok(Ok(()))
    }

    fn expand<M>(
        &mut self,
        pair: (usize, usize),
        predicate_meter: &M,
    ) -> Result<Result<(), IncompleteReason>, SftPreimageError>
    where
        M: Fn(&A::Predicate) -> u64,
    {
        if let Err(reason) = self.session.preview_dynamic(OperationCost {
            states: 1,
            work: 1,
            ..OperationCost::default()
        })? {
            return Ok(Err(reason));
        }
        let outgoing = self.sft_index.as_ref().expect("prepared").outgoing(pair.0);
        let acceptor_index = self.acceptor_index.as_ref().expect("prepared");
        let mut pending = Vec::new();
        let mut oracle_arcs = 0_u64;
        let mut oracle_work = 0_u64;
        let mut payload = 0_u64;
        for &transition_index in outgoing {
            let transition = &self.sft.transitions[transition_index];
            if !self.sft.input_algebra.is_satisfiable(&transition.guard) {
                continue;
            }
            if transition.to >= self.sft.states.len() {
                return Err(SftPreimageError::InvalidTarget {
                    source: 1,
                    transition_index,
                    target: transition.to,
                });
            }
            let cases = self
                .oracle
                .cases(
                    &self.sft.input_algebra,
                    &transition.output,
                    &self.acceptor,
                    acceptor_index,
                    pair.1,
                )
                .map_err(|error| SftPreimageError::Oracle {
                    transition_index,
                    error,
                })?;
            oracle_arcs = oracle_arcs
                .checked_add(cases.inspected_arcs)
                .ok_or(SftPreimageError::ExhaustedRepresentation)?;
            oracle_work = oracle_work
                .checked_add(cases.work)
                .ok_or(SftPreimageError::ExhaustedRepresentation)?;
            for case in cases.cases {
                let guard = self
                    .sft
                    .input_algebra
                    .and(&transition.guard, &case.input_guard);
                if !self.sft.input_algebra.is_satisfiable(&guard) {
                    continue;
                }
                if case.target_state >= self.acceptor.states.len() {
                    return Err(SftPreimageError::InvalidTarget {
                        source: 2,
                        transition_index,
                        target: case.target_state,
                    });
                }
                payload = payload
                    .checked_add(predicate_meter(&guard))
                    .ok_or(SftPreimageError::ExhaustedRepresentation)?;
                pending.push(Pending {
                    sft_target: transition.to,
                    acceptor_target: case.target_state,
                    guard,
                });
            }
        }
        if let Err(reason) = self.session.poll() {
            return Ok(Err(reason));
        }
        let mut new_pairs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for entry in &pending {
            let target = (entry.sft_target, entry.acceptor_target);
            if !self.state_map.contains_key(&target) && seen.insert(target) {
                new_pairs.push(target);
            }
        }
        if self
            .result
            .states
            .len()
            .checked_add(new_pairs.len())
            .is_none()
            || self
                .result
                .transitions
                .len()
                .checked_add(pending.len())
                .is_none()
        {
            return Err(SftPreimageError::ExhaustedRepresentation);
        }
        let source_arcs =
            u64::try_from(outgoing.len()).map_err(|_| SftPreimageError::ExhaustedRepresentation)?;
        let produced =
            u64::try_from(pending.len()).map_err(|_| SftPreimageError::ExhaustedRepresentation)?;
        let work = source_arcs
            .checked_add(oracle_arcs)
            .and_then(|n| n.checked_add(oracle_work))
            .and_then(|n| n.checked_add(produced))
            .and_then(|n| n.checked_add(1))
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        let heap = (pending.len() as u128)
            .checked_mul(size_of::<super::SymbolicTransition<A>>() as u128)
            .and_then(|n| {
                n.checked_add((new_pairs.len() as u128).checked_mul(
                    (size_of::<super::SymbolicState>() + size_of::<(usize, usize)>() * 2) as u128,
                )?)
            })
            .and_then(|n| n.checked_add(u128::from(payload)))
            .ok_or(SftPreimageError::ExhaustedRepresentation)?;
        if let Err(reason) = self.session.advance_dynamic(OperationCost {
            states: 1,
            arcs: source_arcs
                .checked_add(oracle_arcs)
                .ok_or(SftPreimageError::ExhaustedRepresentation)?,
            work,
            heap_bytes: u64::try_from(heap)
                .map_err(|_| SftPreimageError::ExhaustedRepresentation)?,
        })? {
            return Ok(Err(reason));
        }
        self.frontier.pop_front();
        for (sft_state, acceptor_state) in new_pairs {
            let accepting = self.sft.accepting_states.contains(&sft_state)
                && self.acceptor.accepting_states.contains(&acceptor_state);
            let result_id = self.result.add_state(accepting, None);
            self.state_map
                .insert((sft_state, acceptor_state), result_id);
            self.frontier.push_back((sft_state, acceptor_state));
        }
        let from = self.state_map[&pair];
        for entry in pending {
            let to = self.state_map[&(entry.sft_target, entry.acceptor_target)];
            self.result.add_transition(from, to, entry.guard);
        }
        Ok(Ok(()))
    }

    fn incomplete(&mut self, reason: IncompleteReason) -> OperationOutcome<SymbolicAutomaton<A>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Incomplete {
            partial: self.result.clone(),
            reason,
            checkpoint,
        }
    }

    fn complete(&mut self) -> OperationOutcome<SymbolicAutomaton<A>> {
        let checkpoint = self.session.checkpoint();
        self.last_checkpoint = Some(checkpoint);
        OperationOutcome::Complete {
            value: self.result.clone(),
            checkpoint,
        }
    }
}
