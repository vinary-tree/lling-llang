//! Checked acceptor intersection over the bounded composition worklist.

use std::fmt;
use std::hash::Hash;

use super::{BoundedComposition, ComposedTransition, LazyComposition};
use crate::semiring::Semiring;
use crate::wfst::operation::{
    OperationCheckpoint, OperationError, OperationLimits, OperationOutcome,
};
use crate::wfst::{CancellationToken, StateId, VectorWfst, Wfst, NO_STATE};

/// Failure to admit the operands or obey the bounded-operation contract.
#[derive(Debug)]
pub enum IntersectionError {
    /// A source arc is not an acceptor arc: input and output labels differ.
    NotAcceptor {
        /// One-based operand position.
        operand: u8,
        /// State containing the invalid arc.
        state: StateId,
        /// Index in that state's ordered transition list.
        arc_index: usize,
    },
    /// Start ID is inconsistent with the stated number of source states.
    InvalidStart {
        /// One-based operand position.
        operand: u8,
    },
    /// A transition target is outside its operand's state space.
    InvalidTarget {
        /// One-based operand position.
        operand: u8,
        /// Source state of the invalid arc.
        state: StateId,
        /// Index in that state's ordered transition list.
        arc_index: usize,
    },
    /// Source state count cannot be represented by the WFST state-ID type.
    SourceTooLarge,
    /// Shared operation-contract error.
    Operation(OperationError),
}

impl From<OperationError> for IntersectionError {
    fn from(error: OperationError) -> Self {
        Self::Operation(error)
    }
}

impl fmt::Display for IntersectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for IntersectionError {}

fn check_acceptor<F, L, W>(source: &F, operand: u8) -> Result<(), IntersectionError>
where
    F: Wfst<L, W>,
    L: Clone + Eq,
    W: Semiring,
{
    if source.num_states() >= u32::MAX as usize {
        return Err(IntersectionError::SourceTooLarge);
    }
    if (source.num_states() == 0 && source.start() != NO_STATE)
        || (source.num_states() > 0 && !source.is_valid_state(source.start()))
    {
        return Err(IntersectionError::InvalidStart { operand });
    }
    for state in 0..source.num_states() as StateId {
        for (arc_index, arc) in source.transitions(state).iter().enumerate() {
            if !source.is_valid_state(arc.to) {
                return Err(IntersectionError::InvalidTarget {
                    operand,
                    state,
                    arc_index,
                });
            }
            if arc.input != arc.output {
                return Err(IntersectionError::NotAcceptor {
                    operand,
                    state,
                    arc_index,
                });
            }
        }
    }
    Ok(())
}

/// A finite acceptor intersection. Both input graphs are checked in full
/// before their epsilon-filtered composition can produce any result. The
/// inner machine's FIFO frontier gives deterministic, iterative product-state
/// exploration; no partial graph is promoted to complete.
pub struct BoundedIntersection<F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring,
{
    inner: BoundedComposition<F1, F2, L, W>,
}

impl<F1, F2, L, W> BoundedIntersection<F1, F2, L, W>
where
    F1: Wfst<L, W>,
    F2: Wfst<L, W>,
    L: Clone + Eq + Hash + Send + Sync,
    W: Semiring,
{
    /// Validate both acceptors, then bind their ordered contents and default
    /// sequencing-filter semantics to the caller-supplied digest.
    /// Validation is an admission scan, separate from traversal resource
    /// charges; callers should bound input acquisition independently.
    ///
    /// # Errors
    ///
    /// Rejects non-acceptor arcs, unrepresentable state counts or absent
    /// content binding.
    pub fn new(
        first: F1,
        second: F2,
        source_binding: [u8; 32],
        limits: OperationLimits,
        cancellation: CancellationToken,
    ) -> Result<Self, IntersectionError> {
        check_acceptor(&first, 1)?;
        check_acceptor(&second, 2)?;
        let lazy = LazyComposition::new(first, second);
        Ok(Self {
            inner: BoundedComposition::new(lazy, source_binding, limits, cancellation)?,
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
    ) -> Result<(), IntersectionError> {
        Ok(self.inner.resume(checkpoint, limits, cancellation)?)
    }

    /// Traverse until complete, limited, or cancelled. `meter` accounts for
    /// label/weight payload storage beyond the structural charges.
    ///
    /// # Errors
    ///
    /// Refuses a changed source binding or shared contract error.
    pub fn run<M>(
        &mut self,
        observed_source_binding: [u8; 32],
        meter: M,
    ) -> Result<OperationOutcome<VectorWfst<L, W>>, IntersectionError>
    where
        M: Fn(&[ComposedTransition<L, W>]) -> u64,
    {
        Ok(self.inner.run(observed_source_binding, meter)?)
    }
}
