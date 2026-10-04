//! Bounded, snapshot-pinned path traversal for the scalar resource ABI.
//!
//! This cursor never asks a provider for its state count. It expands only
//! states reached by the current depth-first walk, so lazy compositions and
//! externally implemented providers retain their on-demand semantics.

use super::{scalar_one, scalar_times, BindingError, CapturedWfst, StateData};
use std::collections::HashSet;
use std::sync::Arc;
use vinary_tree_interop::{VtResource, VtWeightDomain, VtWfstArc};

/// Path traversal treats loss of a finite, nonzero weight as a numeric
/// failure. This is deliberately local to this cursor: the established
/// scalar-composition algebra retains its existing IEEE/saturation behavior.
fn path_times(domain: VtWeightDomain, left: f64, right: f64) -> Result<f64, BindingError> {
    match domain {
        VtWeightDomain::TropicalF64
        | VtWeightDomain::LogF64
        | VtWeightDomain::SignedTropicalF64
            if left.is_finite() && right.is_finite() && !(left + right).is_finite() =>
        {
            return Err(BindingError::RepresentationLimit);
        }
        VtWeightDomain::ProbabilityF64 if left > 0.0 && right > 0.0 && left * right == 0.0 => {
            return Err(BindingError::RepresentationLimit);
        }
        _ => {}
    }
    scalar_times(domain, left, right)
}

/// Explicit resource and scheduling bounds for one path traversal.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScalarPathConfig {
    pub max_states: usize,
    pub max_arcs: usize,
    pub max_work: usize,
    pub work_per_call: usize,
    pub max_depth: usize,
    pub max_paths: usize,
}

impl ScalarPathConfig {
    pub fn validate(self) -> Result<Self, BindingError> {
        if self.max_states == 0 || self.max_work == 0 || self.work_per_call == 0 {
            return Err(BindingError::InvalidArgument(
                "path bounds must include positive state, total-work, and per-call-work limits",
            ));
        }
        Ok(self)
    }
}

/// One arc together with the state from which it was taken.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScalarPathStep {
    pub from: u64,
    pub arc: VtWfstArc,
}

/// One accepting path. Its weight includes the terminal state's final weight.
#[derive(Clone, Debug)]
pub(crate) struct ScalarPath {
    pub steps: Vec<ScalarPathStep>,
    pub final_state: u64,
    pub weight: f64,
}

/// A single bounded call either yields a path or allows the caller to poll later.
#[derive(Clone, Debug)]
pub(crate) enum PathPoll {
    Path(ScalarPath),
    Pending,
    Exhausted,
    Truncated,
    Cancelled,
}

#[derive(Clone, Debug)]
enum TerminalOutcome {
    Exhausted,
    Truncated,
    Cancelled,
    Failed(BindingError),
}

impl TerminalOutcome {
    fn poll(&self) -> Result<PathPoll, BindingError> {
        match self {
            Self::Exhausted => Ok(PathPoll::Exhausted),
            Self::Truncated => Ok(PathPoll::Truncated),
            Self::Cancelled => Ok(PathPoll::Cancelled),
            Self::Failed(error) => Err(error.clone()),
        }
    }
}

struct Frame {
    state: u64,
    expanded: Arc<StateData>,
    next_arc: usize,
    emitted_final: bool,
    weight: f64,
}

/// Iterative depth-first cursor owning exactly one immutable provider snapshot.
pub(crate) struct ScalarPathCursor {
    captured: CapturedWfst,
    config: ScalarPathConfig,
    frames: Vec<Frame>,
    steps: Vec<ScalarPathStep>,
    seen: HashSet<u64>,
    expanded_arcs: usize,
    work: usize,
    yielded: usize,
    started: bool,
    truncated: bool,
    terminal: Option<TerminalOutcome>,
}

impl ScalarPathCursor {
    /// Capture the foreign resource once. The source may close immediately.
    ///
    /// # Safety
    /// `resource` must be a live `VtResource` for the duration of capture.
    pub(crate) unsafe fn capture(
        resource: VtResource,
        config: ScalarPathConfig,
    ) -> Result<Self, BindingError> {
        let config = config.validate()?;
        let captured = unsafe { CapturedWfst::capture(resource)? };
        Ok(Self {
            captured,
            config,
            frames: Vec::new(),
            steps: Vec::new(),
            seen: HashSet::new(),
            expanded_arcs: 0,
            work: 0,
            yielded: 0,
            started: false,
            truncated: false,
            terminal: None,
        })
    }

    fn push_state(&mut self, state: u64, weight: f64) -> Result<(), BindingError> {
        let unseen = !self.seen.contains(&state);
        if unseen && self.seen.len() >= self.config.max_states {
            return Err(BindingError::BudgetExceeded("states"));
        }
        let remaining_arcs = self.config.max_arcs.saturating_sub(self.expanded_arcs);
        let expanded = self
            .captured
            .state_with_arc_limit(state, unseen.then_some(remaining_arcs))?;
        if !expanded.valid {
            return Err(BindingError::InvalidProviderOutput(
                "a reachable state is invalid in the captured snapshot",
            ));
        }
        if unseen {
            self.seen.insert(state);
            self.expanded_arcs = self
                .expanded_arcs
                .checked_add(expanded.arcs.len())
                .ok_or(BindingError::BudgetExceeded("arcs"))?;
        }
        self.frames.push(Frame {
            state,
            expanded,
            next_arc: 0,
            emitted_final: false,
            weight,
        });
        Ok(())
    }

    /// Advance by at most `work_per_call` traversal decisions, plus one
    /// state expansion bounded by the cursor's remaining arc budget.
    ///
    /// `cancelled` is polled before each traversal decision, not during a
    /// provider callback or the bounded multi-page expansion of one state.
    /// It is not stored beyond the call, so a foreign cancellation owner need
    /// only stay live during this invocation.
    pub(crate) fn poll(&mut self, cancelled: impl Fn() -> bool) -> Result<PathPoll, BindingError> {
        if let Some(terminal) = &self.terminal {
            return terminal.poll();
        }
        if self.yielded >= self.config.max_paths {
            self.terminal = Some(TerminalOutcome::Truncated);
            return Ok(PathPoll::Truncated);
        }

        let mut call_work = 0usize;
        loop {
            if cancelled() {
                self.terminal = Some(TerminalOutcome::Cancelled);
                return Ok(PathPoll::Cancelled);
            }
            if self.frames.is_empty() && self.started {
                let outcome = if self.truncated {
                    TerminalOutcome::Truncated
                } else {
                    TerminalOutcome::Exhausted
                };
                let result = outcome.poll();
                self.terminal = Some(outcome);
                return result;
            }
            if call_work >= self.config.work_per_call {
                return Ok(PathPoll::Pending);
            }
            if self.work >= self.config.max_work {
                let error = BindingError::BudgetExceeded("work");
                self.terminal = Some(TerminalOutcome::Failed(error.clone()));
                return Err(error);
            }
            self.work += 1;
            call_work += 1;

            if self.frames.is_empty() {
                self.started = true;
                if let Err(error) =
                    self.push_state(self.captured.start, scalar_one(self.captured.weight_domain))
                {
                    self.terminal = Some(TerminalOutcome::Failed(error.clone()));
                    return Err(error);
                }
                continue;
            }

            let top = self.frames.len() - 1;
            if !self.frames[top].emitted_final {
                self.frames[top].emitted_final = true;
                if self.frames[top].expanded.is_final {
                    let state = self.frames[top].state;
                    let path_weight = self.frames[top].weight;
                    let final_weight = self.frames[top].expanded.final_weight;
                    let weight = path_times(self.captured.weight_domain, path_weight, final_weight)
                        .map_err(|error| {
                            self.terminal = Some(TerminalOutcome::Failed(error.clone()));
                            error
                        })?;
                    self.yielded += 1;
                    return Ok(PathPoll::Path(ScalarPath {
                        steps: self.steps.clone(),
                        final_state: state,
                        weight,
                    }));
                }
            }

            if self.steps.len() >= self.config.max_depth
                && self.frames[top].next_arc < self.frames[top].expanded.arcs.len()
            {
                self.truncated = true;
                self.frames[top].next_arc = self.frames[top].expanded.arcs.len();
            }

            if let Some(arc) = self.frames[top]
                .expanded
                .arcs
                .get(self.frames[top].next_arc)
                .copied()
            {
                self.frames[top].next_arc += 1;
                let from = self.frames[top].state;
                let weight = path_times(
                    self.captured.weight_domain,
                    self.frames[top].weight,
                    arc.weight,
                )
                .map_err(|error| {
                    self.terminal = Some(TerminalOutcome::Failed(error.clone()));
                    error
                })?;
                self.steps.push(ScalarPathStep { from, arc });
                if let Err(error) = self.push_state(arc.target_state, weight) {
                    self.steps.pop();
                    self.terminal = Some(TerminalOutcome::Failed(error.clone()));
                    return Err(error);
                }
            } else {
                self.frames.pop();
                if !self.frames.is_empty() {
                    self.steps.pop();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_numeric_failures_do_not_redefine_shared_composition() {
        assert_eq!(
            path_times(VtWeightDomain::TropicalF64, f64::MAX, f64::MAX),
            Err(BindingError::RepresentationLimit)
        );
        assert_eq!(
            scalar_times(VtWeightDomain::TropicalF64, f64::MAX, f64::MAX),
            Ok(f64::INFINITY)
        );
        assert_eq!(
            path_times(
                VtWeightDomain::ProbabilityF64,
                f64::MIN_POSITIVE,
                f64::MIN_POSITIVE
            ),
            Err(BindingError::RepresentationLimit)
        );
        assert_eq!(
            scalar_times(
                VtWeightDomain::ProbabilityF64,
                f64::MIN_POSITIVE,
                f64::MIN_POSITIVE
            ),
            Ok(0.0)
        );
        assert_eq!(
            path_times(VtWeightDomain::LogF64, f64::INFINITY, 1.0),
            Ok(f64::INFINITY)
        );
    }
}
