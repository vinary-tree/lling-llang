//! Bounded lazy enumeration of accepting paths by exact best-completion cost.
//!
//! The prelude runs a resumable min-plus suffix calculation over each scalar
//! domain's Viterbi cost projection. A finite `max_depth`, `max_frontier`,
//! `max_paths`, or `max_work` never silently masquerades as exact exhaustion.

use super::graph::ScalarGraph;
use super::graph_analysis::{
    one, rank_cost, times, DistancePoll, GraphAnalysisError, GraphDistanceCursor,
};
use super::path::{ScalarPath, ScalarPathStep};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use vinary_tree_interop::VtWeightDomain;

/// Explicit bounded search configuration; zero depth and paths are valid.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RankedPathConfig {
    pub max_work: usize,
    pub work_per_call: usize,
    pub max_depth: usize,
    pub max_paths: usize,
    pub max_frontier: usize,
}

impl RankedPathConfig {
    pub(crate) fn validate(self) -> Result<Self, GraphAnalysisError> {
        if self.max_work == 0 || self.work_per_call == 0 || self.max_frontier == 0 {
            return Err(GraphAnalysisError::InvalidGraph);
        }
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum RankedPoll {
    Path(ScalarPath),
    Pending,
    Exhausted,
    Truncated,
    Cancelled,
}

#[derive(Clone, Debug)]
enum RankedTerminal {
    Exhausted,
    Truncated,
    Cancelled,
    Failed(GraphAnalysisError),
}

#[derive(Clone, Debug)]
struct Candidate {
    state: usize,
    steps: Vec<(usize, usize)>,
    raw_weight: f64,
    rank_weight: f64,
    estimated: f64,
    terminal: bool,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max heap. Reverse cost, depth, and provider-order
        // arc sequence. At the same complete prefix, terminal precedes a
        // partial expansion, avoiding zero-cost-cycle starvation.
        other
            .estimated
            .total_cmp(&self.estimated)
            .then_with(|| other.steps.len().cmp(&self.steps.len()))
            .then_with(|| other.steps.cmp(&self.steps))
            .then_with(|| self.terminal.cmp(&other.terminal))
            .then_with(|| other.state.cmp(&self.state))
    }
}

struct Active {
    candidate: Candidate,
    final_checked: bool,
    next_arc: usize,
}

/// A bounded path-ranking cursor over one immutable, complete graph.
pub(crate) struct RankedPathCursor {
    graph: Arc<ScalarGraph>,
    config: RankedPathConfig,
    preparation: Option<GraphDistanceCursor>,
    suffix: Vec<f64>,
    heap: BinaryHeap<Candidate>,
    active: Option<Active>,
    work: usize,
    yielded: usize,
    saw_depth_truncation: bool,
    terminal: Option<RankedTerminal>,
}

impl RankedPathCursor {
    pub(crate) fn new(
        graph: Arc<ScalarGraph>,
        config: RankedPathConfig,
    ) -> Result<Self, GraphAnalysisError> {
        let config = config.validate()?;
        let preparation = GraphDistanceCursor::viterbi_suffix(
            Arc::clone(&graph),
            config.max_work,
            config.work_per_call,
        )?;
        Ok(Self {
            graph,
            config,
            preparation: Some(preparation),
            suffix: Vec::new(),
            heap: BinaryHeap::new(),
            active: None,
            work: 0,
            yielded: 0,
            saw_depth_truncation: false,
            terminal: None,
        })
    }

    fn fail(&mut self, error: GraphAnalysisError) -> Result<RankedPoll, GraphAnalysisError> {
        self.terminal = Some(RankedTerminal::Failed(error.clone()));
        Err(error)
    }

    fn push(&mut self, candidate: Candidate) -> Result<(), GraphAnalysisError> {
        if self.heap.len() >= self.config.max_frontier {
            return Err(GraphAnalysisError::FrontierLimit);
        }
        self.heap.push(candidate);
        Ok(())
    }

    fn path(&self, candidate: Candidate) -> ScalarPath {
        let steps = candidate
            .steps
            .into_iter()
            .map(|(source, index)| ScalarPathStep {
                from: self.graph.states[source].raw_id,
                arc: self.graph.states[source].arcs[index],
            })
            .collect();
        ScalarPath {
            steps,
            final_state: self.graph.states[candidate.state].raw_id,
            weight: candidate.raw_weight,
        }
    }

    /// Perform one queue pop, final-option check, arc evaluation, or cleanup.
    fn step(&mut self) -> Result<Option<RankedPoll>, GraphAnalysisError> {
        if self.active.is_none() {
            let Some(candidate) = self.heap.pop() else {
                self.terminal = Some(if self.saw_depth_truncation {
                    RankedTerminal::Truncated
                } else {
                    RankedTerminal::Exhausted
                });
                return Ok(Some(if self.saw_depth_truncation {
                    RankedPoll::Truncated
                } else {
                    RankedPoll::Exhausted
                }));
            };
            if candidate.terminal {
                if self.yielded == self.config.max_paths {
                    self.terminal = Some(RankedTerminal::Truncated);
                    return Ok(Some(RankedPoll::Truncated));
                }
                self.yielded += 1;
                return Ok(Some(RankedPoll::Path(self.path(candidate))));
            }
            self.active = Some(Active {
                candidate,
                final_checked: false,
                next_arc: 0,
            });
            return Ok(None);
        }

        let mut active = self.active.take().expect("checked active");
        let source = active.candidate.state;
        let domain = self.graph.weight_domain;
        if !active.final_checked {
            active.final_checked = true;
            let state = &self.graph.states[source];
            if state.is_final {
                let final_cost = rank_cost(domain, state.final_weight)?;
                if final_cost.is_finite() {
                    let total_cost = times(
                        VtWeightDomain::SignedTropicalF64,
                        active.candidate.rank_weight,
                        final_cost,
                    )?;
                    let raw_weight =
                        times(domain, active.candidate.raw_weight, state.final_weight)?;
                    let mut complete = active.candidate.clone();
                    complete.raw_weight = raw_weight;
                    complete.rank_weight = total_cost;
                    complete.estimated = total_cost;
                    complete.terminal = true;
                    self.push(complete)?;
                }
            }
            self.active = Some(active);
            return Ok(None);
        }

        let state = &self.graph.states[source];
        if active.next_arc == state.arcs.len() {
            return Ok(None);
        }
        let arc_index = active.next_arc;
        active.next_arc += 1;
        let arc = state.arcs[arc_index];
        let target = *self
            .graph
            .local_ids
            .get(&arc.target_state)
            .ok_or(GraphAnalysisError::InvalidGraph)?;
        let arc_cost = rank_cost(domain, arc.weight)?;
        if arc_cost.is_finite() && self.suffix[target].is_finite() {
            if active.candidate.steps.len() == self.config.max_depth {
                self.saw_depth_truncation = true;
            } else {
                let rank_weight = times(
                    VtWeightDomain::SignedTropicalF64,
                    active.candidate.rank_weight,
                    arc_cost,
                )?;
                let estimated = times(
                    VtWeightDomain::SignedTropicalF64,
                    rank_weight,
                    self.suffix[target],
                )?;
                let raw_weight = times(domain, active.candidate.raw_weight, arc.weight)?;
                let mut steps = active.candidate.steps.clone();
                steps.push((source, arc_index));
                self.push(Candidate {
                    state: target,
                    steps,
                    raw_weight,
                    rank_weight,
                    estimated,
                    terminal: false,
                })?;
            }
        }
        self.active = Some(active);
        Ok(None)
    }

    /// One call executes at most `work_per_call` native work transitions.
    pub(crate) fn poll(
        &mut self,
        cancelled: impl Fn() -> bool,
    ) -> Result<RankedPoll, GraphAnalysisError> {
        if let Some(terminal) = &self.terminal {
            return match terminal {
                RankedTerminal::Exhausted => Ok(RankedPoll::Exhausted),
                RankedTerminal::Truncated => Ok(RankedPoll::Truncated),
                RankedTerminal::Cancelled => Ok(RankedPoll::Cancelled),
                RankedTerminal::Failed(error) => Err(error.clone()),
            };
        }
        if let Some(preparation) = self.preparation.as_mut() {
            return match preparation.poll(|| cancelled()) {
                Ok(DistancePoll::Pending) => Ok(RankedPoll::Pending),
                Ok(DistancePoll::Cancelled) => {
                    self.terminal = Some(RankedTerminal::Cancelled);
                    Ok(RankedPoll::Cancelled)
                }
                Ok(DistancePoll::Complete) => {
                    let preparation = self.preparation.take().expect("preparation exists");
                    self.work = preparation.work_done();
                    self.suffix = preparation.into_distances()?.backward;
                    if self.suffix[self.graph.start()].is_finite() {
                        self.push(Candidate {
                            state: self.graph.start(),
                            steps: Vec::new(),
                            raw_weight: one(self.graph.weight_domain),
                            rank_weight: 0.0,
                            estimated: self.suffix[self.graph.start()],
                            terminal: false,
                        })?;
                    } else {
                        self.terminal = Some(RankedTerminal::Exhausted);
                        return Ok(RankedPoll::Exhausted);
                    }
                    Ok(RankedPoll::Pending)
                }
                Err(error) => self.fail(error),
            };
        }
        for _ in 0..self.config.work_per_call {
            if cancelled() {
                self.terminal = Some(RankedTerminal::Cancelled);
                return Ok(RankedPoll::Cancelled);
            }
            if self.active.is_none() && self.heap.is_empty() {
                self.terminal = Some(if self.saw_depth_truncation {
                    RankedTerminal::Truncated
                } else {
                    RankedTerminal::Exhausted
                });
                return Ok(if self.saw_depth_truncation {
                    RankedPoll::Truncated
                } else {
                    RankedPoll::Exhausted
                });
            }
            if self.work == self.config.max_work {
                return self.fail(GraphAnalysisError::WorkLimit);
            }
            self.work += 1;
            match self.step() {
                Ok(Some(result)) => return Ok(result),
                Ok(None) => {}
                Err(error) => return self.fail(error),
            }
        }
        Ok(RankedPoll::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::graph::ScalarGraphState;
    use std::collections::HashMap;
    use vinary_tree_interop::{VtUnitDomain, VtWfstArc};

    fn two_paths(domain: VtWeightDomain, left: f64, right: f64, terminal: f64) -> Arc<ScalarGraph> {
        Arc::new(ScalarGraph {
            unit_domain: VtUnitDomain::Byte,
            weight_domain: domain,
            states: vec![
                ScalarGraphState {
                    raw_id: 10,
                    is_final: false,
                    final_weight: super::super::scalar_zero(domain),
                    arcs: vec![
                        VtWfstArc {
                            input_label: 0,
                            has_input: 1,
                            target_state: 20,
                            weight: left,
                            ..VtWfstArc::default()
                        },
                        VtWfstArc {
                            input_label: 1,
                            has_input: 1,
                            target_state: 20,
                            weight: right,
                            ..VtWfstArc::default()
                        },
                    ],
                },
                ScalarGraphState {
                    raw_id: 20,
                    is_final: true,
                    final_weight: terminal,
                    arcs: vec![],
                },
            ],
            local_ids: HashMap::from([(10, 0), (20, 1)]),
        })
    }

    fn config(max_paths: usize) -> RankedPathConfig {
        RankedPathConfig {
            max_work: 100,
            work_per_call: 1,
            max_depth: 2,
            max_paths,
            max_frontier: 8,
        }
    }

    fn next_non_pending(cursor: &mut RankedPathCursor) -> RankedPoll {
        loop {
            match cursor.poll(|| false).unwrap() {
                RankedPoll::Pending => continue,
                result => return result,
            }
        }
    }

    #[test]
    fn ranks_parallel_paths_and_preserves_provider_ties() {
        for (domain, left, right, terminal, expected) in [
            (VtWeightDomain::TropicalF64, 3.0, 5.0, 2.0, (5.0, 7.0)),
            (VtWeightDomain::ProbabilityF64, 0.2, 0.3, 0.5, (0.15, 0.1)),
            (VtWeightDomain::ArcticF64, 3.0, 5.0, 2.0, (7.0, 5.0)),
            (VtWeightDomain::CountF64, 2.0, 3.0, 4.0, (8.0, 12.0)),
        ] {
            let graph = two_paths(domain, left, right, terminal);
            let mut cursor = RankedPathCursor::new(graph, config(3)).unwrap();
            let first = match next_non_pending(&mut cursor) {
                RankedPoll::Path(path) => path,
                other => panic!("expected first path, got {other:?}"),
            };
            let second = match next_non_pending(&mut cursor) {
                RankedPoll::Path(path) => path,
                other => panic!("expected second path, got {other:?}"),
            };
            assert!((first.weight - expected.0).abs() < 1e-12);
            assert!((second.weight - expected.1).abs() < 1e-12);
            assert!(matches!(
                next_non_pending(&mut cursor),
                RankedPoll::Exhausted
            ));
        }

        let graph = two_paths(VtWeightDomain::TropicalF64, 3.0, 3.0, 2.0);
        let mut cursor = RankedPathCursor::new(graph, config(3)).unwrap();
        let first = match next_non_pending(&mut cursor) {
            RankedPoll::Path(path) => path,
            _ => panic!(),
        };
        let second = match next_non_pending(&mut cursor) {
            RankedPoll::Path(path) => path,
            _ => panic!(),
        };
        assert_eq!(first.steps[0].arc.input_label, 0);
        assert_eq!(second.steps[0].arc.input_label, 1);
        // Distinct parallel arcs have the same weight but retain insertion order
        // as index-zero then index-one candidates in the native heap.
    }

    #[test]
    fn count_and_depth_limits_are_explicit_not_exact_exhaustion() {
        let graph = two_paths(VtWeightDomain::TropicalF64, 3.0, 5.0, 2.0);
        let mut cursor = RankedPathCursor::new(Arc::clone(&graph), config(1)).unwrap();
        assert!(matches!(next_non_pending(&mut cursor), RankedPoll::Path(_)));
        assert!(matches!(
            next_non_pending(&mut cursor),
            RankedPoll::Truncated
        ));
        assert!(matches!(cursor.poll(|| false), Ok(RankedPoll::Truncated)));

        let mut shallow = config(3);
        shallow.max_depth = 0;
        let mut cursor = RankedPathCursor::new(graph, shallow).unwrap();
        assert!(matches!(
            next_non_pending(&mut cursor),
            RankedPoll::Truncated
        ));
        assert!(matches!(cursor.poll(|| false), Ok(RankedPoll::Truncated)));
    }

    #[test]
    fn cancellation_and_work_failure_are_sticky() {
        let graph = two_paths(VtWeightDomain::TropicalF64, 3.0, 5.0, 2.0);
        let mut cursor = RankedPathCursor::new(Arc::clone(&graph), config(3)).unwrap();
        assert!(matches!(cursor.poll(|| true), Ok(RankedPoll::Cancelled)));
        assert!(matches!(cursor.poll(|| false), Ok(RankedPoll::Cancelled)));
        let mut limited = config(3);
        limited.max_work = 1;
        let mut cursor = RankedPathCursor::new(graph, limited).unwrap();
        assert!(matches!(cursor.poll(|| false), Ok(RankedPoll::Pending)));
        assert!(matches!(
            cursor.poll(|| false),
            Err(GraphAnalysisError::WorkLimit)
        ));
        assert!(matches!(
            cursor.poll(|| false),
            Err(GraphAnalysisError::WorkLimit)
        ));
    }

    #[test]
    fn zero_cost_cycles_emit_shorter_ties_before_depth_truncation() {
        let graph = Arc::new(ScalarGraph {
            unit_domain: VtUnitDomain::Byte,
            weight_domain: VtWeightDomain::TropicalF64,
            states: vec![ScalarGraphState {
                raw_id: 0,
                is_final: true,
                final_weight: 0.0,
                arcs: vec![VtWfstArc {
                    target_state: 0,
                    weight: 0.0,
                    ..VtWfstArc::default()
                }],
            }],
            local_ids: HashMap::from([(0, 0)]),
        });
        let mut limits = config(10);
        limits.max_depth = 2;
        let mut cursor = RankedPathCursor::new(graph, limits).unwrap();
        for expected_length in 0..=2 {
            match next_non_pending(&mut cursor) {
                RankedPoll::Path(path) => {
                    assert_eq!(path.steps.len(), expected_length);
                    assert_eq!(path.weight, 0.0);
                }
                other => panic!("expected a zero-cost accepting path, got {other:?}"),
            }
        }
        assert!(matches!(
            next_non_pending(&mut cursor),
            RankedPoll::Truncated
        ));
    }
}
