//! Seeded, bounded draws from exact accepting-path distributions.
//!
//! A resumable native backward analysis supplies conditional continuation
//! masses. Each draw then scans at most one stop/arc option per work unit.
//! The generator never follows a dead-end arc or silently truncates a path.

use super::graph::ScalarGraph;
use super::graph_analysis::{one, times, DistancePoll, GraphAnalysisError, GraphDistanceCursor};
use super::path::{ScalarPath, ScalarPathStep};
use std::sync::Arc;
use vinary_tree_interop::VtWeightDomain;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SampleStrategy {
    Uniform,
    Proportional,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SamplePathConfig {
    pub max_work: usize,
    pub work_per_call: usize,
    pub max_depth: usize,
    pub max_samples: usize,
    pub strategy: SampleStrategy,
    pub seed: u64,
}

#[derive(Clone, Debug)]
pub(crate) enum SamplePoll {
    Path(ScalarPath),
    Pending,
    Exhausted,
    Truncated,
    Cancelled,
}

#[derive(Clone, Debug)]
enum Terminal {
    Exhausted,
    Truncated,
    Cancelled,
    Failed(GraphAnalysisError),
}

#[derive(Clone, Copy, Debug)]
enum Choice {
    Final,
    Arc(usize),
}

/// SplitMix64 fixes the exact seed-to-draw mapping across Rust/rand versions.
#[derive(Clone, Copy, Debug)]
struct StableRng(u64);

impl StableRng {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut bits = self.0;
        bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        bits ^= bits >> 31;
        ((bits >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
    }
}

/// One mutable sample stream over an immutable complete graph.
pub(crate) struct SamplePathCursor {
    graph: Arc<ScalarGraph>,
    config: SamplePathConfig,
    preparation: Option<GraphDistanceCursor>,
    backward: Vec<f64>,
    rng: StableRng,
    current: usize,
    steps: Vec<(usize, usize)>,
    raw_weight: f64,
    threshold: Option<f64>,
    cumulative: f64,
    option: usize,
    fallback: Option<Choice>,
    work: usize,
    yielded: usize,
    terminal: Option<Terminal>,
}

impl SamplePathCursor {
    pub(crate) fn new(
        graph: Arc<ScalarGraph>,
        config: SamplePathConfig,
    ) -> Result<Self, GraphAnalysisError> {
        if config.max_work == 0 || config.work_per_call == 0 || config.max_samples == 0 {
            return Err(GraphAnalysisError::InvalidGraph);
        }
        let preparation = match config.strategy {
            SampleStrategy::Uniform => GraphDistanceCursor::uniform_path_counts(
                Arc::clone(&graph),
                config.max_work,
                config.work_per_call,
            )?,
            SampleStrategy::Proportional => {
                if !matches!(
                    graph.weight_domain,
                    VtWeightDomain::ProbabilityF64
                        | VtWeightDomain::LogF64
                        | VtWeightDomain::CountF64
                ) {
                    return Err(GraphAnalysisError::UnsupportedDomain);
                }
                GraphDistanceCursor::new(Arc::clone(&graph), config.max_work, config.work_per_call)?
            }
        };
        let raw_weight = one(graph.weight_domain);
        Ok(Self {
            graph,
            config,
            preparation: Some(preparation),
            backward: Vec::new(),
            rng: StableRng(config.seed),
            current: 0,
            steps: Vec::new(),
            raw_weight,
            threshold: None,
            cumulative: 0.0,
            option: 0,
            fallback: None,
            work: 0,
            yielded: 0,
            terminal: None,
        })
    }

    fn fail(&mut self, error: GraphAnalysisError) -> Result<SamplePoll, GraphAnalysisError> {
        self.terminal = Some(Terminal::Failed(error.clone()));
        Err(error)
    }

    fn reset_choice(&mut self) {
        self.threshold = None;
        self.cumulative = 0.0;
        self.option = 0;
        self.fallback = None;
    }

    fn conditional_mass(&self, raw: f64, target: Option<usize>) -> Result<f64, GraphAnalysisError> {
        let denominator = self.backward[self.current];
        let domain = self.graph.weight_domain;
        let result = match self.config.strategy {
            SampleStrategy::Uniform => {
                let count = target.map_or(1.0, |id| self.backward[id]);
                count / denominator
            }
            SampleStrategy::Proportional => {
                if domain == VtWeightDomain::LogF64 {
                    let suffix = target.map_or(0.0, |id| self.backward[id]);
                    if raw == f64::INFINITY || suffix == f64::INFINITY {
                        return Ok(0.0);
                    }
                    let cost = times(domain, raw, suffix)?;
                    let mass = (denominator - cost).exp();
                    if mass == 0.0 {
                        return Err(GraphAnalysisError::NumericFailure);
                    }
                    mass
                } else {
                    let suffix = target.map_or(1.0, |id| self.backward[id]);
                    let numerator = times(domain, raw, suffix)?;
                    numerator / denominator
                }
            }
        };
        if !result.is_finite() || result < 0.0 || result > 1.0 + 1e-9 {
            return Err(GraphAnalysisError::NumericFailure);
        }
        Ok(result)
    }

    fn select(&mut self, choice: Choice) -> Result<Option<SamplePoll>, GraphAnalysisError> {
        let domain = self.graph.weight_domain;
        match choice {
            Choice::Final => {
                let final_weight = self.graph.states[self.current].final_weight;
                let weight = times(domain, self.raw_weight, final_weight)?;
                let steps = std::mem::take(&mut self.steps)
                    .into_iter()
                    .map(|(source, index)| ScalarPathStep {
                        from: self.graph.states[source].raw_id,
                        arc: self.graph.states[source].arcs[index],
                    })
                    .collect();
                let path = ScalarPath {
                    steps,
                    final_state: self.graph.states[self.current].raw_id,
                    weight,
                };
                self.yielded += 1;
                self.current = self.graph.start();
                self.raw_weight = one(domain);
                self.reset_choice();
                Ok(Some(SamplePoll::Path(path)))
            }
            Choice::Arc(index) => {
                if self.steps.len() == self.config.max_depth {
                    self.terminal = Some(Terminal::Truncated);
                    return Ok(Some(SamplePoll::Truncated));
                }
                let arc = self.graph.states[self.current].arcs[index];
                let target = *self
                    .graph
                    .local_ids
                    .get(&arc.target_state)
                    .ok_or(GraphAnalysisError::InvalidGraph)?;
                self.raw_weight = times(domain, self.raw_weight, arc.weight)?;
                self.steps.push((self.current, index));
                self.current = target;
                self.reset_choice();
                Ok(None)
            }
        }
    }

    /// Exactly one random draw or one final/arc candidate is processed.
    fn step(&mut self) -> Result<Option<SamplePoll>, GraphAnalysisError> {
        if self.threshold.is_none() {
            self.threshold = Some(self.rng.unit());
            return Ok(None);
        }
        let state = &self.graph.states[self.current];
        if self.option == 0 {
            self.option = 1;
            if state.is_final {
                let mass = self.conditional_mass(state.final_weight, None)?;
                if mass > 0.0 {
                    self.fallback = Some(Choice::Final);
                    self.cumulative += mass;
                    if self.threshold.unwrap() < self.cumulative {
                        return self.select(Choice::Final);
                    }
                }
            }
        } else if self.option <= state.arcs.len() {
            let index = self.option - 1;
            self.option += 1;
            let arc = state.arcs[index];
            let target = *self
                .graph
                .local_ids
                .get(&arc.target_state)
                .ok_or(GraphAnalysisError::InvalidGraph)?;
            let mass = self.conditional_mass(arc.weight, Some(target))?;
            if mass > 0.0 {
                self.fallback = Some(Choice::Arc(index));
                self.cumulative += mass;
                if self.threshold.unwrap() < self.cumulative {
                    return self.select(Choice::Arc(index));
                }
            }
        }
        if self.option > state.arcs.len() {
            return self.select(self.fallback.ok_or(GraphAnalysisError::NumericFailure)?);
        }
        Ok(None)
    }

    /// Run no more than `work_per_call` decisions, including RNG draws.
    pub(crate) fn poll(
        &mut self,
        cancelled: impl Fn() -> bool,
    ) -> Result<SamplePoll, GraphAnalysisError> {
        if let Some(terminal) = &self.terminal {
            return match terminal {
                Terminal::Exhausted => Ok(SamplePoll::Exhausted),
                Terminal::Truncated => Ok(SamplePoll::Truncated),
                Terminal::Cancelled => Ok(SamplePoll::Cancelled),
                Terminal::Failed(error) => Err(error.clone()),
            };
        }
        if let Some(preparation) = self.preparation.as_mut() {
            return match preparation.poll(|| cancelled()) {
                Ok(DistancePoll::Pending) => Ok(SamplePoll::Pending),
                Ok(DistancePoll::Cancelled) => {
                    self.terminal = Some(Terminal::Cancelled);
                    Ok(SamplePoll::Cancelled)
                }
                Ok(DistancePoll::Complete) => {
                    let preparation = self.preparation.take().expect("preparation exists");
                    self.work = preparation.work_done();
                    self.backward = preparation.into_distances()?.backward;
                    let total = self.backward[self.graph.start()];
                    if total == 0.0 || total == f64::INFINITY {
                        self.terminal = Some(Terminal::Exhausted);
                        Ok(SamplePoll::Exhausted)
                    } else {
                        Ok(SamplePoll::Pending)
                    }
                }
                Err(error) => self.fail(error),
            };
        }
        for _ in 0..self.config.work_per_call {
            if cancelled() {
                self.terminal = Some(Terminal::Cancelled);
                return Ok(SamplePoll::Cancelled);
            }
            if self.yielded == self.config.max_samples {
                self.terminal = Some(Terminal::Truncated);
                return Ok(SamplePoll::Truncated);
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
        Ok(SamplePoll::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::graph::ScalarGraphState;
    use std::collections::HashMap;
    use vinary_tree_interop::{VtUnitDomain, VtWfstArc};

    fn two_choices(domain: VtWeightDomain, left: f64, right: f64) -> Arc<ScalarGraph> {
        Arc::new(ScalarGraph {
            unit_domain: VtUnitDomain::Byte,
            weight_domain: domain,
            states: vec![
                ScalarGraphState {
                    raw_id: 0,
                    is_final: false,
                    final_weight: super::super::scalar_zero(domain),
                    arcs: vec![
                        VtWfstArc {
                            input_label: 0,
                            has_input: 1,
                            target_state: 1,
                            weight: left,
                            ..VtWfstArc::default()
                        },
                        VtWfstArc {
                            input_label: 1,
                            has_input: 1,
                            target_state: 2,
                            weight: right,
                            ..VtWfstArc::default()
                        },
                    ],
                },
                ScalarGraphState {
                    raw_id: 1,
                    is_final: true,
                    final_weight: one(domain),
                    arcs: vec![],
                },
                ScalarGraphState {
                    raw_id: 2,
                    is_final: true,
                    final_weight: one(domain),
                    arcs: vec![],
                },
            ],
            local_ids: HashMap::from([(0, 0), (1, 1), (2, 2)]),
        })
    }

    fn config(strategy: SampleStrategy, per_call: usize) -> SamplePathConfig {
        SamplePathConfig {
            max_work: 20_000,
            work_per_call: per_call,
            max_depth: 3,
            max_samples: 200,
            strategy,
            seed: 0x1234_5678,
        }
    }

    fn next(cursor: &mut SamplePathCursor) -> SamplePoll {
        loop {
            match cursor.poll(|| false).unwrap() {
                SamplePoll::Pending => {}
                other => return other,
            }
        }
    }

    #[test]
    fn uniform_seed_is_independent_of_poll_slice_and_weights() {
        let graph = two_choices(VtWeightDomain::TropicalF64, 20.0, -5.0);
        let mut slow =
            SamplePathCursor::new(Arc::clone(&graph), config(SampleStrategy::Uniform, 1)).unwrap();
        let mut fast = SamplePathCursor::new(graph, config(SampleStrategy::Uniform, 64)).unwrap();
        for _ in 0..200 {
            let left = match next(&mut slow) {
                SamplePoll::Path(path) => path,
                other => panic!("expected sample, got {other:?}"),
            };
            let right = match next(&mut fast) {
                SamplePoll::Path(path) => path,
                other => panic!("expected sample, got {other:?}"),
            };
            assert_eq!(
                left.steps[0].arc.input_label,
                right.steps[0].arc.input_label
            );
        }
        assert!(matches!(next(&mut slow), SamplePoll::Truncated));
        assert!(matches!(next(&mut fast), SamplePoll::Truncated));
    }

    #[test]
    fn proportional_draws_favor_mass_and_reject_unsupported_domains() {
        let graph = two_choices(VtWeightDomain::ProbabilityF64, 0.25, 0.75);
        let mut cursor =
            SamplePathCursor::new(graph, config(SampleStrategy::Proportional, 3)).unwrap();
        let mut right = 0;
        for _ in 0..200 {
            match next(&mut cursor) {
                SamplePoll::Path(path) => {
                    right += usize::from(path.steps[0].arc.input_label == 1);
                }
                other => panic!("expected sample, got {other:?}"),
            }
        }
        assert!(right > 120 && right < 180, "right mass count {right}");
        assert!(matches!(next(&mut cursor), SamplePoll::Truncated));
        assert!(matches!(
            SamplePathCursor::new(
                two_choices(VtWeightDomain::TropicalF64, 1.0, 2.0),
                config(SampleStrategy::Proportional, 1)
            ),
            Err(GraphAnalysisError::UnsupportedDomain)
        ));
    }

    #[test]
    fn cancellation_and_depth_truncation_are_sticky() {
        let graph = two_choices(VtWeightDomain::BooleanF64, 1.0, 1.0);
        let mut cancelled =
            SamplePathCursor::new(Arc::clone(&graph), config(SampleStrategy::Uniform, 1)).unwrap();
        assert!(matches!(cancelled.poll(|| true), Ok(SamplePoll::Cancelled)));
        assert!(matches!(
            cancelled.poll(|| false),
            Ok(SamplePoll::Cancelled)
        ));
        let mut limits = config(SampleStrategy::Uniform, 1);
        limits.max_depth = 0;
        let mut cursor = SamplePathCursor::new(graph, limits).unwrap();
        assert!(matches!(next(&mut cursor), SamplePoll::Truncated));
        assert!(matches!(next(&mut cursor), SamplePoll::Truncated));
    }

    #[test]
    fn empty_language_exhausts_and_cycle_is_rejected_without_a_partial_sample() {
        let mut empty = two_choices(VtWeightDomain::BooleanF64, 1.0, 1.0);
        for state in &mut Arc::get_mut(&mut empty).unwrap().states {
            state.is_final = false;
            state.final_weight = 0.0;
        }
        let mut cursor = SamplePathCursor::new(empty, config(SampleStrategy::Uniform, 1)).unwrap();
        assert!(matches!(next(&mut cursor), SamplePoll::Exhausted));
        assert!(matches!(next(&mut cursor), SamplePoll::Exhausted));

        let mut cyclic = two_choices(VtWeightDomain::BooleanF64, 1.0, 1.0);
        Arc::get_mut(&mut cyclic).unwrap().states[1]
            .arcs
            .push(VtWfstArc {
                target_state: 1,
                weight: 1.0,
                ..VtWfstArc::default()
            });
        let mut cursor = SamplePathCursor::new(cyclic, config(SampleStrategy::Uniform, 1)).unwrap();
        loop {
            match cursor.poll(|| false) {
                Ok(SamplePoll::Pending) => continue,
                Err(GraphAnalysisError::UnsupportedCycle) => break,
                other => panic!("cycle must fail before yielding a sample: {other:?}"),
            }
        }
        assert!(matches!(
            cursor.poll(|| false),
            Err(GraphAnalysisError::UnsupportedCycle)
        ));
    }
}
