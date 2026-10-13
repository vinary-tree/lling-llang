//! Checked runtime-arity multi-tape graphs for foreign-language bindings.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;

use super::traits::MultiTapeWfst;
use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, NO_STATE};

/// Invalid graph shape or exhausted representation/work bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeMultiTapeError {
    /// A multi-tape graph must have at least one tape.
    ZeroTapes,
    /// The state ID type cannot represent another state.
    StateIdOverflow,
    /// The arc count cannot represent another transition.
    TransitionCountOverflow,
    /// A supplied state ID does not name a state in the graph.
    InvalidState {
        /// Invalid state ID.
        state: StateId,
        /// Number of states present.
        num_states: usize,
    },
    /// A supplied tape index is outside this graph's arity.
    InvalidTape {
        /// Invalid zero-based tape index.
        tape: usize,
        /// Number of tapes present.
        num_tapes: usize,
    },
    /// A label vector has a different arity from the graph.
    InvalidArity {
        /// Required number of labels.
        expected: usize,
        /// Supplied number of labels.
        actual: usize,
    },
    /// A source graph yields a duplicate state ID or an arc to an unknown ID.
    InvalidSourceGraph,
    /// A caller-provided finite work bound has been exhausted.
    WorkLimitExceeded,
}

impl fmt::Display for RuntimeMultiTapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroTapes => write!(f, "multi-tape arity must be positive"),
            Self::StateIdOverflow => write!(f, "multi-tape state ID capacity exhausted"),
            Self::TransitionCountOverflow => write!(f, "multi-tape transition count overflow"),
            Self::InvalidState { state, num_states } => {
                write!(f, "state {state} is invalid for {num_states} states")
            }
            Self::InvalidTape { tape, num_tapes } => {
                write!(f, "tape {tape} is invalid for {num_tapes} tapes")
            }
            Self::InvalidArity { expected, actual } => {
                write!(f, "label arity {actual} does not match {expected} tapes")
            }
            Self::InvalidSourceGraph => write!(f, "source multi-tape graph is inconsistent"),
            Self::WorkLimitExceeded => write!(f, "multi-tape work bound exhausted"),
        }
    }
}

impl std::error::Error for RuntimeMultiTapeError {}

/// One weighted arc with one optional label per tape.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeMultiTapeTransition<L, W: Semiring> {
    /// Source state.
    pub from: StateId,
    /// Target state.
    pub to: StateId,
    /// Labels in tape order; `None` means epsilon.
    pub labels: Vec<Option<L>>,
    /// Arc weight.
    pub weight: W,
}

impl<L, W: Semiring> RuntimeMultiTapeTransition<L, W> {
    /// Whether this arc is epsilon on every tape.
    pub fn is_epsilon(&self) -> bool {
        self.labels.iter().all(Option::is_none)
    }
}

/// One state and its outgoing arcs.
#[derive(Clone, Debug)]
pub struct RuntimeMultiTapeState<L, W: Semiring> {
    /// Final weight, or `None` for a non-final state.
    pub final_weight: Option<W>,
    /// Outgoing arcs in insertion order.
    pub transitions: Vec<RuntimeMultiTapeTransition<L, W>>,
}

/// A checked multi-tape graph whose arity is chosen at runtime.
///
/// Freeze it behind `Arc` before sharing it with read-only foreign cursors.
#[derive(Clone, Debug)]
pub struct RuntimeMultiTapeWfst<L, W: Semiring> {
    num_tapes: usize,
    states: Vec<RuntimeMultiTapeState<L, W>>,
    start: Option<StateId>,
    num_transitions: usize,
}

impl<L, W: Semiring> RuntimeMultiTapeWfst<L, W> {
    /// Create an empty graph with any positive number of tapes.
    pub fn new(num_tapes: usize) -> Result<Self, RuntimeMultiTapeError> {
        if num_tapes == 0 {
            return Err(RuntimeMultiTapeError::ZeroTapes);
        }
        Ok(Self {
            num_tapes,
            states: Vec::new(),
            start: None,
            num_transitions: 0,
        })
    }

    /// Number of tapes in each transition.
    pub fn num_tapes(&self) -> usize {
        self.num_tapes
    }

    /// Number of states.
    pub fn num_states(&self) -> usize {
        self.states.len()
    }

    /// Number of arcs.
    pub fn num_transitions(&self) -> usize {
        self.num_transitions
    }

    /// Start state, if one was assigned.
    pub fn start(&self) -> Option<StateId> {
        self.start
    }

    /// Read a state, checking its ID.
    pub fn state(
        &self,
        state: StateId,
    ) -> Result<&RuntimeMultiTapeState<L, W>, RuntimeMultiTapeError> {
        self.states
            .get(state as usize)
            .ok_or(RuntimeMultiTapeError::InvalidState {
                state,
                num_states: self.states.len(),
            })
    }

    /// Add a non-final state.
    pub fn add_state(&mut self) -> Result<StateId, RuntimeMultiTapeError> {
        if self.states.len() >= NO_STATE as usize {
            return Err(RuntimeMultiTapeError::StateIdOverflow);
        }
        let id = self.states.len() as StateId;
        self.states.push(RuntimeMultiTapeState {
            final_weight: None,
            transitions: Vec::new(),
        });
        Ok(id)
    }

    /// Set the start state.
    pub fn set_start(&mut self, state: StateId) -> Result<(), RuntimeMultiTapeError> {
        self.state(state)?;
        self.start = Some(state);
        Ok(())
    }

    /// Make a state final with its semiring weight.
    pub fn set_final(&mut self, state: StateId, weight: W) -> Result<(), RuntimeMultiTapeError> {
        let num_states = self.states.len();
        let state = self
            .states
            .get_mut(state as usize)
            .ok_or(RuntimeMultiTapeError::InvalidState { state, num_states })?;
        state.final_weight = Some(weight);
        Ok(())
    }

    /// Remove a state's final status.
    pub fn unset_final(&mut self, state: StateId) -> Result<(), RuntimeMultiTapeError> {
        let num_states = self.states.len();
        let state = self
            .states
            .get_mut(state as usize)
            .ok_or(RuntimeMultiTapeError::InvalidState { state, num_states })?;
        state.final_weight = None;
        Ok(())
    }

    /// Add a checked arc, preserving insertion order.
    pub fn add_transition(
        &mut self,
        from: StateId,
        to: StateId,
        labels: Vec<Option<L>>,
        weight: W,
    ) -> Result<(), RuntimeMultiTapeError> {
        if labels.len() != self.num_tapes {
            return Err(RuntimeMultiTapeError::InvalidArity {
                expected: self.num_tapes,
                actual: labels.len(),
            });
        }
        self.state(to)?;
        self.state(from)?;
        let next_count = self
            .num_transitions
            .checked_add(1)
            .ok_or(RuntimeMultiTapeError::TransitionCountOverflow)?;
        self.states[from as usize]
            .transitions
            .push(RuntimeMultiTapeTransition {
                from,
                to,
                labels,
                weight,
            });
        self.num_transitions = next_count;
        Ok(())
    }
}

impl<L, W> RuntimeMultiTapeWfst<L, W>
where
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring + Clone,
{
    /// Capture any const-arity native multi-tape graph without an arity cap.
    pub fn from_fixed<T, const N: usize>(source: &T) -> Result<Self, RuntimeMultiTapeError>
    where
        T: MultiTapeWfst<L, W, N>,
    {
        let mut result = Self::new(N)?;
        let mut remap = HashMap::new();
        let source_states: Vec<StateId> = source.states().collect();
        for &source_state in &source_states {
            let target = result.add_state()?;
            if remap.insert(source_state, target).is_some() {
                return Err(RuntimeMultiTapeError::InvalidSourceGraph);
            }
            if source.is_final(source_state) {
                result.set_final(target, source.final_weight(source_state))?;
            }
        }
        if let Some(&start) = remap.get(&source.start()) {
            result.set_start(start)?;
        } else if !source_states.is_empty() {
            return Err(RuntimeMultiTapeError::InvalidSourceGraph);
        }
        for &source_state in &source_states {
            for transition in source.transitions(source_state) {
                let from = *remap
                    .get(&transition.from)
                    .ok_or(RuntimeMultiTapeError::InvalidSourceGraph)?;
                let to = *remap
                    .get(&transition.to)
                    .ok_or(RuntimeMultiTapeError::InvalidSourceGraph)?;
                result.add_transition(
                    from,
                    to,
                    transition.labels.labels.to_vec(),
                    transition.weight.clone(),
                )?;
            }
        }
        Ok(result)
    }

    /// Project an ordered selection of tapes, allowing repeated indices.
    pub fn project_tapes(&self, tapes: &[usize]) -> Result<Self, RuntimeMultiTapeError> {
        let mut result = Self::new(tapes.len())?;
        for &tape in tapes {
            if tape >= self.num_tapes {
                return Err(RuntimeMultiTapeError::InvalidTape {
                    tape,
                    num_tapes: self.num_tapes,
                });
            }
        }
        for state in &self.states {
            let id = result.add_state()?;
            if let Some(weight) = &state.final_weight {
                result.set_final(id, weight.clone())?;
            }
        }
        if let Some(start) = self.start {
            result.set_start(start)?;
        }
        for state in &self.states {
            for transition in &state.transitions {
                let labels = tapes
                    .iter()
                    .map(|&tape| transition.labels[tape].clone())
                    .collect();
                result.add_transition(
                    transition.from,
                    transition.to,
                    labels,
                    transition.weight.clone(),
                )?;
            }
        }
        Ok(result)
    }

    /// Project one tape to an ordinary single-tape WFST.
    pub fn project(&self, tape: usize) -> Result<VectorWfst<L, W>, RuntimeMultiTapeError> {
        if tape >= self.num_tapes {
            return Err(RuntimeMultiTapeError::InvalidTape {
                tape,
                num_tapes: self.num_tapes,
            });
        }
        let mut result = VectorWfst::new();
        for state in &self.states {
            let id = result.add_state();
            if let Some(weight) = &state.final_weight {
                result.set_final(id, weight.clone());
            }
        }
        if let Some(start) = self.start {
            result.set_start(start);
        }
        for state in &self.states {
            for transition in &state.transitions {
                let label = transition.labels[tape].clone();
                result.add_transition(WeightedTransition::new(
                    transition.from,
                    label.clone(),
                    label,
                    transition.to,
                    transition.weight.clone(),
                ));
            }
        }
        Ok(result)
    }

    /// Return the first accepting path weight for a sequence of label tuples.
    ///
    /// Pure epsilon arcs may be traversed without consuming an input tuple.
    /// The bounded search mirrors the const-arity native operation's path
    /// order and visited-state semantics.
    pub fn transduce(
        &self,
        input: &[Vec<Option<L>>],
        max_work: u64,
    ) -> Result<Option<W>, RuntimeMultiTapeError> {
        for labels in input {
            if labels.len() != self.num_tapes {
                return Err(RuntimeMultiTapeError::InvalidArity {
                    expected: self.num_tapes,
                    actual: labels.len(),
                });
            }
        }
        let Some(start) = self.start else {
            return Ok(None);
        };
        let mut agenda = vec![(start, 0usize, W::one())];
        let mut visited = HashSet::new();
        let mut work = 0u64;
        while let Some((state, position, path_weight)) = agenda.pop() {
            work = work
                .checked_add(1)
                .ok_or(RuntimeMultiTapeError::WorkLimitExceeded)?;
            if work > max_work {
                return Err(RuntimeMultiTapeError::WorkLimitExceeded);
            }
            if !visited.insert((state, position)) {
                continue;
            }
            if position == input.len() {
                if let Some(final_weight) = &self.states[state as usize].final_weight {
                    return Ok(Some(path_weight.times(final_weight)));
                }
            }
            let mut successors = Vec::new();
            for transition in &self.states[state as usize].transitions {
                work = work
                    .checked_add(1)
                    .ok_or(RuntimeMultiTapeError::WorkLimitExceeded)?;
                if work > max_work {
                    return Err(RuntimeMultiTapeError::WorkLimitExceeded);
                }
                if position < input.len() && transition.labels == input[position] {
                    successors.push((
                        transition.to,
                        position + 1,
                        path_weight.times(&transition.weight),
                    ));
                }
            }
            for transition in &self.states[state as usize].transitions {
                work = work
                    .checked_add(1)
                    .ok_or(RuntimeMultiTapeError::WorkLimitExceeded)?;
                if work > max_work {
                    return Err(RuntimeMultiTapeError::WorkLimitExceeded);
                }
                if transition.is_epsilon() {
                    successors.push((
                        transition.to,
                        position,
                        path_weight.times(&transition.weight),
                    ));
                }
            }
            agenda.extend(successors.into_iter().rev());
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multitape::{
        project, project_tapes, MultiTapeLabel, MultiTapeWfstBuilder, MultiTapeWfstOps,
    };
    use crate::semiring::TropicalWeight;
    use crate::wfst::Wfst;

    #[test]
    fn runtime_arity_matches_fixed_native_controls() {
        let mut builder = MultiTapeWfstBuilder::<char, TropicalWeight, 3>::new();
        let start = builder.add_state();
        let finish = builder.add_final_state(TropicalWeight::new(2.0));
        builder.set_start(start);
        builder.add_transition(
            start,
            finish,
            MultiTapeLabel::new([Some('a'), None, Some('z')]),
            TropicalWeight::new(3.0),
        );
        let fixed = builder.build();
        let runtime = RuntimeMultiTapeWfst::from_fixed(&fixed).expect("valid fixed graph");
        assert_eq!(runtime.num_tapes(), 3);
        assert_eq!(runtime.num_states(), fixed.num_states());
        let input = vec![vec![Some('a'), None, Some('z')]];
        assert_eq!(
            runtime.transduce(&input, 16).expect("within work bound"),
            fixed.transduce(&[MultiTapeLabel::new([Some('a'), None, Some('z')])])
        );
        let fixed_projected = project(&fixed, 2);
        let runtime_projected = runtime.project(2).expect("valid tape");
        assert_eq!(
            fixed_projected.wfst().transitions(start),
            runtime_projected.transitions(start)
        );
        let fixed_subset = project_tapes(&fixed, [2, 0]);
        let runtime_subset = runtime.project_tapes(&[2, 0]).expect("valid tapes");
        assert_eq!(
            fixed_subset.transitions(start)[0].labels.labels.to_vec(),
            runtime_subset
                .state(start)
                .expect("valid state")
                .transitions[0]
                .labels
        );
    }

    #[test]
    fn runtime_arity_and_bounds_are_checked() {
        assert_eq!(
            RuntimeMultiTapeWfst::<char, TropicalWeight>::new(0).unwrap_err(),
            RuntimeMultiTapeError::ZeroTapes
        );
        let mut graph = RuntimeMultiTapeWfst::<char, TropicalWeight>::new(9).expect("valid arity");
        let start = graph.add_state().expect("state capacity");
        graph.set_start(start).expect("valid state");
        graph
            .set_final(start, TropicalWeight::one())
            .expect("valid state");
        assert!(matches!(
            graph.add_transition(start, start, vec![None; 8], TropicalWeight::one()),
            Err(RuntimeMultiTapeError::InvalidArity {
                expected: 9,
                actual: 8
            })
        ));
        assert!(matches!(
            graph.project(9),
            Err(RuntimeMultiTapeError::InvalidTape {
                tape: 9,
                num_tapes: 9
            })
        ));
        assert!(matches!(
            graph.transduce(&[vec![None; 9]], 0),
            Err(RuntimeMultiTapeError::WorkLimitExceeded)
        ));
        assert_eq!(
            graph.transduce(&[], 1).expect("within work bound"),
            Some(TropicalWeight::one())
        );
    }
}
