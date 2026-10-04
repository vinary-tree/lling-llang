//! Semiring-correct analysis of an exact, snapshot-independent scalar graph.
//!
//! This module deliberately never treats a budgeted prefix of graph capture as
//! a complete graph. The acyclic dynamic program is shared by all seven scalar
//! semirings; cyclic idempotent distance is handled separately below.

use super::{graph::ScalarGraph, scalar_zero, valid_scalar_weight};
use std::collections::VecDeque;
use std::sync::Arc;
use vinary_tree_interop::VtWeightDomain;

/// A semantic failure, distinct from incomplete graph capture or ABI failure.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GraphAnalysisError {
    /// Exact result cannot be represented in the declared scalar carrier.
    NumericFailure,
    /// A cycle prevents the requested finite-path semiring computation.
    NonConvergent,
    /// This domain/cycle combination needs a separate convergent solver.
    UnsupportedCycle,
    /// The requested probability-like operation is not defined for this domain.
    UnsupportedDomain,
    /// The graph has zero accepting-path mass.
    NoAcceptingPath,
    /// The complete graph's target map is internally inconsistent.
    InvalidGraph,
    /// The caller's declared analysis-work bound was exhausted.
    WorkLimit,
    /// The ranked path frontier would exceed its declared allocation bound.
    FrontierLimit,
}

/// Exact semiring sums from the start and toward any final state.
#[derive(Clone, Debug)]
pub(crate) struct GraphDistances {
    pub forward: Vec<f64>,
    pub backward: Vec<f64>,
    pub total: f64,
}

impl GraphDistances {
    fn posterior_factor(
        &self,
        domain: VtWeightDomain,
        prefix: f64,
        transition: f64,
        suffix: f64,
    ) -> Result<f64, GraphAnalysisError> {
        let zero = scalar_zero(domain);
        if self.total == zero {
            return Err(GraphAnalysisError::NoAcceptingPath);
        }
        if prefix == zero || transition == zero || suffix == zero {
            return Ok(0.0);
        }
        match domain {
            VtWeightDomain::ProbabilityF64 => {
                let exponent = prefix.ln() + transition.ln() + suffix.ln() - self.total.ln();
                let result = exponent.exp();
                if result == 0.0 || !result.is_finite() || result > 1.0 + 1e-12 {
                    return Err(GraphAnalysisError::NumericFailure);
                }
                Ok(result.min(1.0))
            }
            VtWeightDomain::LogF64 => {
                let score = times(domain, prefix, transition)?;
                let score = times(domain, score, suffix)?;
                let exponent = self.total - score;
                let result = exponent.exp();
                if result == 0.0 || !result.is_finite() || result > 1.0 + 1e-12 {
                    return Err(GraphAnalysisError::NumericFailure);
                }
                Ok(result.min(1.0))
            }
            VtWeightDomain::CountF64 => {
                let count = times(domain, times(domain, prefix, transition)?, suffix)?;
                Ok(count / self.total)
            }
            _ => Err(GraphAnalysisError::UnsupportedDomain),
        }
    }

    /// Probability that one graph arc participates in a sampled accepting
    /// path. Arc multiplicity and final multiplicity are preserved exactly.
    pub(crate) fn arc_posterior(
        &self,
        graph: &ScalarGraph,
        source: usize,
        arc_index: usize,
    ) -> Result<f64, GraphAnalysisError> {
        let state = graph
            .states
            .get(source)
            .ok_or(GraphAnalysisError::InvalidGraph)?;
        let arc = state
            .arcs
            .get(arc_index)
            .ok_or(GraphAnalysisError::InvalidGraph)?;
        let target = *graph
            .local_ids
            .get(&arc.target_state)
            .ok_or(GraphAnalysisError::InvalidGraph)?;
        self.posterior_factor(
            graph.weight_domain,
            self.forward[source],
            arc.weight,
            self.backward[target],
        )
    }

    /// Probability of terminating at one final state.
    pub(crate) fn final_posterior(
        &self,
        graph: &ScalarGraph,
        state: usize,
    ) -> Result<f64, GraphAnalysisError> {
        let state_data = graph
            .states
            .get(state)
            .ok_or(GraphAnalysisError::InvalidGraph)?;
        if !state_data.is_final {
            return Ok(0.0);
        }
        self.posterior_factor(
            graph.weight_domain,
            self.forward[state],
            state_data.final_weight,
            one(graph.weight_domain),
        )
    }
}

/// Bounded result of one distance-analysis poll.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DistancePoll {
    Pending,
    Complete,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DistanceStage {
    Indegrees,
    QueueSeeds,
    Topology,
    ForwardDag,
    BackwardDag,
    ForwardCyclic,
    BackwardCyclic,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DistanceMode {
    Semiring,
    ViterbiSuffix,
    UniformCount,
}

#[derive(Clone, Debug)]
enum DistanceTerminal {
    Complete,
    Cancelled,
    Failed(GraphAnalysisError),
}

/// A single-edge/vertex-step distance machine. Opening only allocates arrays
/// proportional to the already bounded exact graph; every poll executes at
/// most `work_per_call` graph-edge or graph-vertex transitions.
pub(crate) struct GraphDistanceCursor {
    graph: Arc<ScalarGraph>,
    mode: DistanceMode,
    max_work: usize,
    work_per_call: usize,
    work: usize,
    stage: DistanceStage,
    state_index: usize,
    arc_index: usize,
    indegree: Vec<usize>,
    ready: VecDeque<usize>,
    order: Vec<usize>,
    active: Option<usize>,
    pass: usize,
    changed: bool,
    forward: Vec<f64>,
    backward: Vec<f64>,
    terminal: Option<DistanceTerminal>,
}

impl GraphDistanceCursor {
    pub(crate) fn new(
        graph: Arc<ScalarGraph>,
        max_work: usize,
        work_per_call: usize,
    ) -> Result<Self, GraphAnalysisError> {
        Self::with_mode(graph, max_work, work_per_call, DistanceMode::Semiring)
    }

    /// Bounded preparation of best-completion costs for ranked path search.
    pub(crate) fn viterbi_suffix(
        graph: Arc<ScalarGraph>,
        max_work: usize,
        work_per_call: usize,
    ) -> Result<Self, GraphAnalysisError> {
        Self::with_mode(graph, max_work, work_per_call, DistanceMode::ViterbiSuffix)
    }

    /// Exact accepting-path counts on finite DAGs, ignoring scalar weights.
    pub(crate) fn uniform_path_counts(
        graph: Arc<ScalarGraph>,
        max_work: usize,
        work_per_call: usize,
    ) -> Result<Self, GraphAnalysisError> {
        Self::with_mode(graph, max_work, work_per_call, DistanceMode::UniformCount)
    }

    fn with_mode(
        graph: Arc<ScalarGraph>,
        max_work: usize,
        work_per_call: usize,
        mode: DistanceMode,
    ) -> Result<Self, GraphAnalysisError> {
        if max_work == 0 || work_per_call == 0 || graph.states.is_empty() {
            return Err(GraphAnalysisError::InvalidGraph);
        }
        let len = graph.states.len();
        let zero = scalar_zero(match mode {
            DistanceMode::ViterbiSuffix => VtWeightDomain::SignedTropicalF64,
            DistanceMode::UniformCount => VtWeightDomain::CountF64,
            DistanceMode::Semiring => graph.weight_domain,
        });
        Ok(Self {
            graph,
            mode,
            max_work,
            work_per_call,
            work: 0,
            stage: DistanceStage::Indegrees,
            state_index: 0,
            arc_index: 0,
            indegree: vec![0; len],
            ready: VecDeque::new(),
            order: Vec::with_capacity(len),
            active: None,
            pass: 0,
            changed: false,
            forward: vec![zero; len],
            backward: vec![zero; len],
            terminal: None,
        })
    }

    fn fail(&mut self, error: GraphAnalysisError) -> Result<DistancePoll, GraphAnalysisError> {
        self.terminal = Some(DistanceTerminal::Failed(error.clone()));
        Err(error)
    }

    fn target(&self, source: usize, arc_index: usize) -> Result<usize, GraphAnalysisError> {
        self.graph
            .local_ids
            .get(&self.graph.states[source].arcs[arc_index].target_state)
            .copied()
            .ok_or(GraphAnalysisError::InvalidGraph)
    }

    fn domain(&self) -> VtWeightDomain {
        match self.mode {
            DistanceMode::ViterbiSuffix => VtWeightDomain::SignedTropicalF64,
            DistanceMode::UniformCount => VtWeightDomain::CountF64,
            DistanceMode::Semiring => self.graph.weight_domain,
        }
    }

    fn weight(&self, raw: f64) -> Result<f64, GraphAnalysisError> {
        match self.mode {
            DistanceMode::ViterbiSuffix => rank_cost(self.graph.weight_domain, raw),
            DistanceMode::UniformCount => Ok(1.0),
            DistanceMode::Semiring => Ok(raw),
        }
    }

    /// One bounded graph-edge or graph-vertex transition.
    fn step(&mut self) -> Result<(), GraphAnalysisError> {
        let len = self.graph.states.len();
        let domain = self.domain();
        match self.stage {
            DistanceStage::Indegrees => {
                if self.state_index == len {
                    self.stage = DistanceStage::QueueSeeds;
                    self.state_index = 0;
                    return Ok(());
                }
                if self.arc_index == self.graph.states[self.state_index].arcs.len() {
                    self.state_index += 1;
                    self.arc_index = 0;
                    return Ok(());
                }
                let target = self.target(self.state_index, self.arc_index)?;
                self.indegree[target] = self.indegree[target]
                    .checked_add(1)
                    .ok_or(GraphAnalysisError::NumericFailure)?;
                self.arc_index += 1;
            }
            DistanceStage::QueueSeeds => {
                if self.state_index == len {
                    self.stage = DistanceStage::Topology;
                    return Ok(());
                }
                if self.indegree[self.state_index] == 0 {
                    self.ready.push_back(self.state_index);
                }
                self.state_index += 1;
            }
            DistanceStage::Topology => {
                if let Some(source) = self.active {
                    if self.arc_index == self.graph.states[source].arcs.len() {
                        self.active = None;
                        return Ok(());
                    }
                    let target = self.target(source, self.arc_index)?;
                    self.indegree[target] -= 1;
                    if self.indegree[target] == 0 {
                        self.ready.push_back(target);
                    }
                    self.arc_index += 1;
                } else if let Some(source) = self.ready.pop_front() {
                    self.order.push(source);
                    self.active = Some(source);
                    self.arc_index = 0;
                } else if self.order.len() == len {
                    self.stage = if self.mode != DistanceMode::Semiring {
                        DistanceStage::BackwardDag
                    } else {
                        DistanceStage::ForwardDag
                    };
                    self.state_index = 0;
                    self.arc_index = 0;
                    if self.mode == DistanceMode::Semiring {
                        self.forward[self.graph.start()] = one(domain);
                    }
                } else if self.mode != DistanceMode::UniformCount
                    && matches!(
                        domain,
                        VtWeightDomain::TropicalF64
                            | VtWeightDomain::SignedTropicalF64
                            | VtWeightDomain::ArcticF64
                            | VtWeightDomain::BooleanF64
                    )
                {
                    self.stage = if self.mode != DistanceMode::Semiring {
                        DistanceStage::BackwardCyclic
                    } else {
                        DistanceStage::ForwardCyclic
                    };
                    self.state_index = 0;
                    self.arc_index = 0;
                    if self.mode == DistanceMode::Semiring {
                        self.forward[self.graph.start()] = one(domain);
                    }
                    self.backward = self
                        .graph
                        .states
                        .iter()
                        .map(|state| {
                            if state.is_final {
                                self.weight(state.final_weight)
                            } else {
                                Ok(scalar_zero(domain))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                } else {
                    return Err(GraphAnalysisError::UnsupportedCycle);
                }
            }
            DistanceStage::ForwardDag => {
                if self.state_index == len {
                    self.stage = DistanceStage::BackwardDag;
                    self.state_index = 0;
                    self.arc_index = 0;
                    return Ok(());
                }
                let source = self.order[self.state_index];
                if self.arc_index == self.graph.states[source].arcs.len() {
                    self.state_index += 1;
                    self.arc_index = 0;
                    return Ok(());
                }
                let target = self.target(source, self.arc_index)?;
                let arc = &self.graph.states[source].arcs[self.arc_index];
                let contribution = times(domain, self.forward[source], self.weight(arc.weight)?)?;
                self.forward[target] = plus(domain, self.forward[target], contribution)?;
                self.arc_index += 1;
            }
            DistanceStage::BackwardDag => {
                if self.state_index == len {
                    self.stage = DistanceStage::Complete;
                    return Ok(());
                }
                let source = self.order[len - 1 - self.state_index];
                let state = &self.graph.states[source];
                if self.arc_index == 0 && state.is_final {
                    self.backward[source] = plus(
                        domain,
                        self.backward[source],
                        self.weight(state.final_weight)?,
                    )?;
                }
                if self.arc_index == state.arcs.len() {
                    self.state_index += 1;
                    self.arc_index = 0;
                    return Ok(());
                }
                let target = self.target(source, self.arc_index)?;
                let contribution = times(
                    domain,
                    self.weight(state.arcs[self.arc_index].weight)?,
                    self.backward[target],
                )?;
                self.backward[source] = plus(domain, self.backward[source], contribution)?;
                self.arc_index += 1;
            }
            DistanceStage::ForwardCyclic | DistanceStage::BackwardCyclic => {
                if self.state_index == len {
                    let was_forward = self.stage == DistanceStage::ForwardCyclic;
                    self.pass += 1;
                    if !self.changed {
                        if was_forward {
                            self.stage = DistanceStage::BackwardCyclic;
                            self.pass = 0;
                        } else {
                            self.stage = DistanceStage::Complete;
                        }
                    } else if self.pass >= len {
                        return Err(GraphAnalysisError::NonConvergent);
                    }
                    self.changed = false;
                    self.state_index = 0;
                    self.arc_index = 0;
                    return Ok(());
                }
                let source = self.state_index;
                if self.arc_index == self.graph.states[source].arcs.len() {
                    self.state_index += 1;
                    self.arc_index = 0;
                    return Ok(());
                }
                let target = self.target(source, self.arc_index)?;
                let arc = &self.graph.states[source].arcs[self.arc_index];
                let arc_weight = self.weight(arc.weight)?;
                let (from, to, distance) = if self.stage == DistanceStage::ForwardCyclic {
                    (source, target, &mut self.forward)
                } else {
                    (target, source, &mut self.backward)
                };
                let candidate = times(domain, distance[from], arc_weight)?;
                let combined = plus(domain, distance[to], candidate)?;
                if distance[to] != combined {
                    distance[to] = combined;
                    self.changed = true;
                }
                self.arc_index += 1;
            }
            DistanceStage::Complete => {}
        }
        Ok(())
    }

    pub(crate) fn poll(
        &mut self,
        cancelled: impl Fn() -> bool,
    ) -> Result<DistancePoll, GraphAnalysisError> {
        if let Some(terminal) = &self.terminal {
            return match terminal {
                DistanceTerminal::Complete => Ok(DistancePoll::Complete),
                DistanceTerminal::Cancelled => Ok(DistancePoll::Cancelled),
                DistanceTerminal::Failed(error) => Err(error.clone()),
            };
        }
        for _ in 0..self.work_per_call {
            if cancelled() {
                self.terminal = Some(DistanceTerminal::Cancelled);
                return Ok(DistancePoll::Cancelled);
            }
            if self.stage == DistanceStage::Complete {
                self.terminal = Some(DistanceTerminal::Complete);
                return Ok(DistancePoll::Complete);
            }
            if self.work == self.max_work {
                return self.fail(GraphAnalysisError::WorkLimit);
            }
            self.work += 1;
            if let Err(error) = self.step() {
                return self.fail(error);
            }
        }
        if self.stage == DistanceStage::Complete {
            self.terminal = Some(DistanceTerminal::Complete);
            Ok(DistancePoll::Complete)
        } else {
            Ok(DistancePoll::Pending)
        }
    }

    pub(crate) fn into_distances(self) -> Result<GraphDistances, GraphAnalysisError> {
        if !matches!(self.terminal, Some(DistanceTerminal::Complete)) {
            return Err(GraphAnalysisError::InvalidGraph);
        }
        Ok(GraphDistances {
            total: self.backward[self.graph.start()],
            forward: self.forward,
            backward: self.backward,
        })
    }

    pub(crate) fn is_complete(&self) -> bool {
        matches!(self.terminal, Some(DistanceTerminal::Complete))
    }

    pub(crate) fn graph_lease(&self) -> Arc<ScalarGraph> {
        Arc::clone(&self.graph)
    }

    pub(crate) fn work_done(&self) -> usize {
        self.work
    }
}

/// Cost projection for ordering individual paths rather than summing them.
/// Semiring zero is an impossible branch under every declared scalar domain.
pub(super) fn rank_cost(domain: VtWeightDomain, value: f64) -> Result<f64, GraphAnalysisError> {
    if value == scalar_zero(domain) {
        return Ok(f64::INFINITY);
    }
    let cost = match domain {
        VtWeightDomain::TropicalF64
        | VtWeightDomain::SignedTropicalF64
        | VtWeightDomain::LogF64 => value,
        VtWeightDomain::ArcticF64 => -value,
        VtWeightDomain::ProbabilityF64 => -value.ln(),
        VtWeightDomain::CountF64 => value.ln(),
        VtWeightDomain::BooleanF64 => 0.0,
    };
    // IEEE total ordering distinguishes -0 from +0 although these are the
    // same semiring cost. Canonicalize before provider-order tie breaking.
    checked(
        VtWeightDomain::SignedTropicalF64,
        if cost == 0.0 { 0.0 } else { cost },
    )
}

fn checked(domain: VtWeightDomain, value: f64) -> Result<f64, GraphAnalysisError> {
    valid_scalar_weight(domain, value)
        .then_some(value)
        .ok_or(GraphAnalysisError::NumericFailure)
}

pub(super) fn one(domain: VtWeightDomain) -> f64 {
    match domain {
        VtWeightDomain::TropicalF64
        | VtWeightDomain::LogF64
        | VtWeightDomain::ArcticF64
        | VtWeightDomain::SignedTropicalF64 => 0.0,
        VtWeightDomain::ProbabilityF64 | VtWeightDomain::CountF64 | VtWeightDomain::BooleanF64 => {
            1.0
        }
    }
}

pub(super) fn times(domain: VtWeightDomain, lhs: f64, rhs: f64) -> Result<f64, GraphAnalysisError> {
    let zero = scalar_zero(domain);
    if lhs == zero || rhs == zero {
        return Ok(zero);
    }
    if domain == VtWeightDomain::CountF64 {
        let product = (lhs as u128)
            .checked_mul(rhs as u128)
            .ok_or(GraphAnalysisError::NumericFailure)?;
        return (product <= 9_007_199_254_740_992)
            .then_some(product as f64)
            .ok_or(GraphAnalysisError::NumericFailure);
    }
    if domain == VtWeightDomain::ProbabilityF64 && lhs != 0.0 && rhs != 0.0 && lhs * rhs == 0.0 {
        return Err(GraphAnalysisError::NumericFailure);
    }
    checked(
        domain,
        match domain {
            VtWeightDomain::TropicalF64
            | VtWeightDomain::LogF64
            | VtWeightDomain::ArcticF64
            | VtWeightDomain::SignedTropicalF64 => lhs + rhs,
            VtWeightDomain::ProbabilityF64 | VtWeightDomain::CountF64 => lhs * rhs,
            VtWeightDomain::BooleanF64 => f64::from(lhs == 1.0 && rhs == 1.0),
        },
    )
}

fn plus(domain: VtWeightDomain, lhs: f64, rhs: f64) -> Result<f64, GraphAnalysisError> {
    let zero = scalar_zero(domain);
    if lhs == zero {
        return Ok(rhs);
    }
    if rhs == zero {
        return Ok(lhs);
    }
    if domain == VtWeightDomain::CountF64 {
        let sum = (lhs as u128)
            .checked_add(rhs as u128)
            .ok_or(GraphAnalysisError::NumericFailure)?;
        return (sum <= 9_007_199_254_740_992)
            .then_some(sum as f64)
            .ok_or(GraphAnalysisError::NumericFailure);
    }
    checked(
        domain,
        match domain {
            VtWeightDomain::TropicalF64 | VtWeightDomain::SignedTropicalF64 => lhs.min(rhs),
            VtWeightDomain::ArcticF64 => lhs.max(rhs),
            VtWeightDomain::BooleanF64 => f64::from(lhs == 1.0 || rhs == 1.0),
            VtWeightDomain::ProbabilityF64 | VtWeightDomain::CountF64 => lhs + rhs,
            VtWeightDomain::LogF64 => {
                // Stable negative-log sum: -ln(exp(-lhs) + exp(-rhs)).
                let smaller = lhs.min(rhs);
                smaller - (-(lhs.max(rhs) - smaller)).exp().ln_1p()
            }
        },
    )
}

// Independent eager oracle for small fixtures; production uses the bounded
// GraphDistanceCursor above and never performs this whole-graph computation
// in one call.
#[cfg(test)]
impl ScalarGraph {
    /// Stable Kahn order. The graph collector assigns deterministic local IDs
    /// and stores every arc in provider order, so this order is reproducible.
    pub(crate) fn topological_order(&self) -> Result<Option<Vec<usize>>, GraphAnalysisError> {
        let mut indegree = vec![0usize; self.states.len()];
        for state in &self.states {
            for arc in &state.arcs {
                let &target = self
                    .local_ids
                    .get(&arc.target_state)
                    .ok_or(GraphAnalysisError::InvalidGraph)?;
                indegree[target] = indegree[target]
                    .checked_add(1)
                    .ok_or(GraphAnalysisError::NumericFailure)?;
            }
        }
        let mut ready: VecDeque<_> = indegree
            .iter()
            .enumerate()
            .filter_map(|(id, &degree)| (degree == 0).then_some(id))
            .collect();
        let mut order = Vec::with_capacity(self.states.len());
        while let Some(source) = ready.pop_front() {
            order.push(source);
            for arc in &self.states[source].arcs {
                let target = self.local_ids[&arc.target_state];
                indegree[target] -= 1;
                if indegree[target] == 0 {
                    ready.push_back(target);
                }
            }
        }
        Ok((order.len() == self.states.len()).then_some(order))
    }

    /// Exact forward/backward semiring sums on a finite acyclic graph.
    ///
    /// A cycle is an explicit unsupported outcome here; tropical/arctic and
    /// convergent probabilistic cycle solvers are separate operations rather
    /// than silently approximating an exact forward/backward result.
    pub(crate) fn acyclic_distances(&self) -> Result<GraphDistances, GraphAnalysisError> {
        let order = self
            .topological_order()?
            .ok_or(GraphAnalysisError::UnsupportedCycle)?;
        let domain = self.weight_domain;
        let zero = scalar_zero(domain);
        let mut forward = vec![zero; self.states.len()];
        let mut backward = vec![zero; self.states.len()];
        forward[self.start()] = one(domain);
        for &source in &order {
            for arc in &self.states[source].arcs {
                let target = self.local_ids[&arc.target_state];
                let contribution = times(domain, forward[source], arc.weight)?;
                forward[target] = plus(domain, forward[target], contribution)?;
            }
        }
        for &source in order.iter().rev() {
            let state = &self.states[source];
            if state.is_final {
                backward[source] = plus(domain, backward[source], state.final_weight)?;
            }
            for arc in &state.arcs {
                let target = self.local_ids[&arc.target_state];
                let contribution = times(domain, arc.weight, backward[target])?;
                backward[source] = plus(domain, backward[source], contribution)?;
            }
        }
        let total = backward[self.start()];
        Ok(GraphDistances {
            forward,
            backward,
            total,
        })
    }

    /// Exact shortest/longest/reachability distances when a cyclic graph uses
    /// an idempotent semiring. A strictly improving cycle is nonconvergent; a
    /// zero-cost or nonproductive cycle is not incorrectly rejected.
    ///
    /// Non-idempotent cyclic sums require a convergence proof and a distinct
    /// solver. In particular, treating a finite `max_depth` prefix as the
    /// exact probability/log/count sum would be incorrect.
    pub(crate) fn distances(&self) -> Result<GraphDistances, GraphAnalysisError> {
        if self.topological_order()?.is_some() {
            return self.acyclic_distances();
        }
        let domain = self.weight_domain;
        if !matches!(
            domain,
            VtWeightDomain::TropicalF64
                | VtWeightDomain::SignedTropicalF64
                | VtWeightDomain::ArcticF64
                | VtWeightDomain::BooleanF64
        ) {
            return Err(GraphAnalysisError::UnsupportedCycle);
        }

        let zero = scalar_zero(domain);
        let mut forward = vec![zero; self.states.len()];
        forward[self.start()] = one(domain);
        let mut backward = self
            .states
            .iter()
            .map(|state| {
                if state.is_final {
                    state.final_weight
                } else {
                    zero
                }
            })
            .collect::<Vec<_>>();
        let relax = |distance: &mut Vec<f64>, reverse: bool| -> Result<bool, GraphAnalysisError> {
            let mut changed = false;
            for (source, state) in self.states.iter().enumerate() {
                for arc in &state.arcs {
                    let target = self.local_ids[&arc.target_state];
                    let (from, to) = if reverse {
                        (target, source)
                    } else {
                        (source, target)
                    };
                    let candidate = times(domain, distance[from], arc.weight)?;
                    let combined = plus(domain, distance[to], candidate)?;
                    if combined != distance[to] {
                        distance[to] = combined;
                        changed = true;
                    }
                }
            }
            Ok(changed)
        };
        // A simple path visits at most V states. In-place relaxation can only
        // converge sooner; an improving Vth sweep witnesses a productive cycle.
        for _ in 0..self.states.len().saturating_sub(1) {
            let forward_changed = relax(&mut forward, false)?;
            let backward_changed = relax(&mut backward, true)?;
            if !forward_changed && !backward_changed {
                break;
            }
        }
        if relax(&mut forward, false)? || relax(&mut backward, true)? {
            return Err(GraphAnalysisError::NonConvergent);
        }
        Ok(GraphDistances {
            total: backward[self.start()],
            forward,
            backward,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::graph::ScalarGraphState;
    use std::collections::HashMap;
    use vinary_tree_interop::{VtUnitDomain, VtWfstArc};

    fn arc(target: u64, weight: f64) -> VtWfstArc {
        VtWfstArc {
            target_state: target,
            weight,
            ..VtWfstArc::default()
        }
    }

    fn diamond(domain: VtWeightDomain, left: f64, right: f64, final_weight: f64) -> ScalarGraph {
        ScalarGraph {
            unit_domain: VtUnitDomain::Byte,
            weight_domain: domain,
            states: vec![
                ScalarGraphState {
                    raw_id: 10,
                    is_final: false,
                    final_weight: scalar_zero(domain),
                    arcs: vec![arc(20, left), arc(20, right)],
                },
                ScalarGraphState {
                    raw_id: 20,
                    is_final: true,
                    final_weight,
                    arcs: vec![],
                },
            ],
            local_ids: HashMap::from([(10, 0), (20, 1)]),
        }
    }

    #[test]
    fn all_scalar_domains_share_exact_acyclic_recurrences() {
        let cases = [
            (VtWeightDomain::TropicalF64, 3.0, 5.0, 2.0, 5.0),
            (VtWeightDomain::SignedTropicalF64, -3.0, 5.0, 2.0, -1.0),
            (VtWeightDomain::ArcticF64, 3.0, 5.0, 2.0, 7.0),
            (VtWeightDomain::ProbabilityF64, 0.2, 0.3, 0.5, 0.25),
            (VtWeightDomain::CountF64, 2.0, 3.0, 4.0, 20.0),
            (VtWeightDomain::BooleanF64, 1.0, 0.0, 1.0, 1.0),
            (
                VtWeightDomain::LogF64,
                -0.2f64.ln(),
                -0.3f64.ln(),
                -0.5f64.ln(),
                -0.25f64.ln(),
            ),
        ];
        for (domain, left, right, final_weight, expected) in cases {
            let distances = diamond(domain, left, right, final_weight)
                .acyclic_distances()
                .unwrap();
            assert!(
                (distances.total - expected).abs() < 1e-12,
                "{domain:?}: {} != {expected}",
                distances.total
            );
            assert_eq!(distances.backward[1], final_weight);
        }
    }

    #[test]
    fn cycles_are_explicit_and_count_overflow_is_rejected() {
        let mut cyclic = diamond(VtWeightDomain::BooleanF64, 1.0, 1.0, 1.0);
        cyclic.states[1].arcs.push(arc(10, 1.0));
        assert_eq!(cyclic.topological_order().unwrap(), None);
        assert_eq!(
            cyclic.acyclic_distances().unwrap_err(),
            GraphAnalysisError::UnsupportedCycle
        );
        let overflow = diamond(VtWeightDomain::CountF64, 9_007_199_254_740_992.0, 1.0, 1.0);
        assert_eq!(
            overflow.acyclic_distances().unwrap_err(),
            GraphAnalysisError::NumericFailure
        );
    }

    #[test]
    fn idempotent_cycles_converge_or_report_the_improving_cycle() {
        let mut tropical = diamond(VtWeightDomain::TropicalF64, 3.0, 5.0, 2.0);
        tropical.states[1].arcs.push(arc(10, 0.0));
        let distances = tropical.distances().unwrap();
        assert_eq!(distances.total, 5.0);
        assert_eq!(distances.forward, vec![0.0, 3.0]);
        assert_eq!(distances.backward, vec![5.0, 2.0]);

        tropical.states[1].arcs[0].weight = -4.0;
        assert_eq!(
            tropical.distances().unwrap_err(),
            GraphAnalysisError::NonConvergent
        );

        let mut arctic = diamond(VtWeightDomain::ArcticF64, 3.0, 5.0, 2.0);
        arctic.states[1].arcs.push(arc(10, -10.0));
        assert_eq!(arctic.distances().unwrap().total, 7.0);
        arctic.states[1].arcs[0].weight = 1.0;
        assert_eq!(
            arctic.distances().unwrap_err(),
            GraphAnalysisError::NonConvergent
        );

        let mut boolean = diamond(VtWeightDomain::BooleanF64, 1.0, 0.0, 1.0);
        boolean.states[1].arcs.push(arc(10, 1.0));
        assert_eq!(boolean.distances().unwrap().total, 1.0);
    }

    #[test]
    fn distance_machine_is_bounded_resumable_and_matches_exact_kernels() {
        for domain in [
            VtWeightDomain::TropicalF64,
            VtWeightDomain::ProbabilityF64,
            VtWeightDomain::LogF64,
            VtWeightDomain::BooleanF64,
        ] {
            let graph = Arc::new(diamond(domain, one(domain), one(domain), one(domain)));
            let expected = graph.distances().unwrap();
            let mut cursor = GraphDistanceCursor::new(Arc::clone(&graph), 100, 1).unwrap();
            let mut pending = 0;
            loop {
                match cursor.poll(|| false).unwrap() {
                    DistancePoll::Pending => pending += 1,
                    DistancePoll::Complete => break,
                    DistancePoll::Cancelled => panic!("unexpected cancellation"),
                }
            }
            assert!(pending > 10);
            assert_eq!(cursor.poll(|| false), Ok(DistancePoll::Complete));
            let actual = cursor.into_distances().unwrap();
            assert_eq!(actual.forward, expected.forward);
            assert_eq!(actual.backward, expected.backward);
            assert_eq!(actual.total, expected.total);
        }

        let graph = Arc::new(diamond(VtWeightDomain::TropicalF64, 1.0, 2.0, 3.0));
        let mut cursor = GraphDistanceCursor::new(Arc::clone(&graph), 1, 1).unwrap();
        assert_eq!(cursor.poll(|| false), Ok(DistancePoll::Pending));
        assert_eq!(cursor.poll(|| false), Err(GraphAnalysisError::WorkLimit));
        assert_eq!(cursor.poll(|| false), Err(GraphAnalysisError::WorkLimit));
        assert!(cursor.into_distances().is_err());

        let mut cursor = GraphDistanceCursor::new(graph, 100, 1).unwrap();
        assert_eq!(cursor.poll(|| true), Ok(DistancePoll::Cancelled));
        assert_eq!(cursor.poll(|| false), Ok(DistancePoll::Cancelled));
        assert!(cursor.into_distances().is_err());

        let mut cyclic = diamond(VtWeightDomain::TropicalF64, 3.0, 5.0, 2.0);
        cyclic.states[1].arcs.push(arc(10, 0.0));
        let exact = cyclic.distances().unwrap();
        let mut cursor = GraphDistanceCursor::new(Arc::new(cyclic.clone()), 100, 1).unwrap();
        while cursor.poll(|| false).unwrap() == DistancePoll::Pending {}
        let actual = cursor.into_distances().unwrap();
        assert_eq!(actual.forward, exact.forward);
        assert_eq!(actual.backward, exact.backward);
        assert_eq!(actual.total, exact.total);

        cyclic.states[1].arcs[0].weight = -4.0;
        let mut cursor = GraphDistanceCursor::new(Arc::new(cyclic), 100, 1).unwrap();
        loop {
            match cursor.poll(|| false) {
                Ok(DistancePoll::Pending) => continue,
                Err(error) => {
                    assert_eq!(error, GraphAnalysisError::NonConvergent);
                    assert_eq!(cursor.poll(|| false), Err(error));
                    break;
                }
                outcome => panic!("unexpected cyclic outcome: {outcome:?}"),
            }
        }
    }

    #[test]
    fn posterior_mass_preserves_parallel_arc_and_final_multiplicity() {
        for (domain, left, right, final_weight) in [
            (VtWeightDomain::ProbabilityF64, 0.2, 0.3, 0.5),
            (
                VtWeightDomain::LogF64,
                -0.2f64.ln(),
                -0.3f64.ln(),
                -0.5f64.ln(),
            ),
            (VtWeightDomain::CountF64, 2.0, 3.0, 4.0),
        ] {
            let graph = diamond(domain, left, right, final_weight);
            let distances = graph.distances().unwrap();
            assert!((distances.arc_posterior(&graph, 0, 0).unwrap() - 0.4).abs() < 1e-12);
            assert!((distances.arc_posterior(&graph, 0, 1).unwrap() - 0.6).abs() < 1e-12);
            assert!((distances.final_posterior(&graph, 1).unwrap() - 1.0).abs() < 1e-12);
            assert_eq!(distances.final_posterior(&graph, 0).unwrap(), 0.0);
        }
    }

    #[test]
    fn bounded_viterbi_suffix_is_distinct_from_semiring_path_sum() {
        let cases = [
            (VtWeightDomain::TropicalF64, 3.0, 5.0, 2.0, 5.0),
            (VtWeightDomain::SignedTropicalF64, -3.0, 5.0, 2.0, -1.0),
            (VtWeightDomain::ArcticF64, 3.0, 5.0, 2.0, -7.0),
            (VtWeightDomain::ProbabilityF64, 0.2, 0.3, 0.5, -0.15f64.ln()),
            (VtWeightDomain::CountF64, 2.0, 3.0, 4.0, 8.0f64.ln()),
            (VtWeightDomain::BooleanF64, 1.0, 0.0, 1.0, 0.0),
            (
                VtWeightDomain::LogF64,
                -0.2f64.ln(),
                -0.3f64.ln(),
                -0.5f64.ln(),
                -0.15f64.ln(),
            ),
        ];
        for (domain, left, right, terminal, expected) in cases {
            let graph = Arc::new(diamond(domain, left, right, terminal));
            let mut cursor = GraphDistanceCursor::viterbi_suffix(graph, 100, 1).unwrap();
            while cursor.poll(|| false).unwrap() == DistancePoll::Pending {}
            let cost = cursor.into_distances().unwrap().total;
            assert!(
                (cost - expected).abs() < 1e-12,
                "{domain:?}: {cost} != {expected}"
            );
        }

        let mut graph = diamond(VtWeightDomain::ProbabilityF64, 0.2, 0.3, 0.5);
        graph.states[1].arcs.push(arc(10, 0.25));
        let mut cursor =
            GraphDistanceCursor::viterbi_suffix(Arc::new(graph.clone()), 100, 1).unwrap();
        while cursor.poll(|| false).unwrap() == DistancePoll::Pending {}
        assert!((cursor.into_distances().unwrap().total + 0.15f64.ln()).abs() < 1e-12);

        graph.states[1].arcs[0].weight = 5.0;
        let mut cursor = GraphDistanceCursor::viterbi_suffix(Arc::new(graph), 100, 1).unwrap();
        loop {
            match cursor.poll(|| false) {
                Ok(DistancePoll::Pending) => continue,
                Err(error) => {
                    assert_eq!(error, GraphAnalysisError::NonConvergent);
                    break;
                }
                outcome => panic!("unexpected outcome: {outcome:?}"),
            }
        }
    }
}
