//! Resumable, bounded constrained beam decoding.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;

use super::{BeamHypothesis, ConstrainedDecoder, TokenId};

/// Resource and scoring contract for one decoder session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeamSearchConfig {
    /// Maximum live hypotheses after each step.
    pub beam_width: usize,
    /// Maximum number of token steps.
    pub max_length: usize,
    /// Hard bound on materialized expansions in one step.
    pub max_candidates_per_step: usize,
    /// Hard bound on the scorer's dense vocabulary buffer.
    pub max_vocabulary: usize,
    /// Positive divisor applied to token log probabilities.
    pub temperature: f64,
    /// Nonnegative exponent applied to sequence length when ranking beams.
    pub length_penalty: f64,
}

impl BeamSearchConfig {
    /// Validate finite scoring parameters and hard work limits.
    pub fn validate(self) -> Result<(), &'static str> {
        if self.beam_width == 0 {
            return Err("beam_width must be positive");
        }
        if self.max_candidates_per_step == 0 {
            return Err("max_candidates_per_step must be positive");
        }
        if self.max_vocabulary == 0 {
            return Err("max_vocabulary must be positive");
        }
        if !self.temperature.is_finite() || self.temperature <= 0.0 {
            return Err("temperature must be finite and positive");
        }
        if !self.length_penalty.is_finite() || self.length_penalty < 0.0 {
            return Err("length_penalty must be finite and nonnegative");
        }
        Ok(())
    }
}

impl Default for BeamSearchConfig {
    fn default() -> Self {
        Self {
            beam_width: 8,
            max_length: 128,
            max_candidates_per_step: 65_536,
            max_vocabulary: 262_144,
            temperature: 1.0,
            length_penalty: 0.0,
        }
    }
}

/// Cumulative work counters. All counts saturate instead of wrapping.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BeamSearchStats {
    /// Successfully committed decode steps.
    pub steps: u64,
    /// Calls made to the model scorer.
    pub score_calls: u64,
    /// Legal token transitions considered.
    pub considered_tokens: u64,
    /// Finite scored transitions materialized.
    pub candidates: u64,
}

/// Result of a successful step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BeamStep {
    /// At least one hypothesis can still be extended.
    Advanced,
    /// Every surviving hypothesis accepts or the configured length was reached.
    Complete,
    /// No further transition is available.
    Exhausted,
}

/// A step failed without replacing the last committed hypotheses.
#[derive(Debug, PartialEq)]
pub enum BeamSearchError<E> {
    /// The scorer returned a failure.
    Scorer(E),
    /// The scorer returned a nonfinite score.
    InvalidScores(&'static str),
    /// An expansion would exceed the configured materialization bound.
    CandidateLimit,
    /// Cancellation was observed before committing the step.
    Cancelled,
}

impl<E: fmt::Display> fmt::Display for BeamSearchError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScores(message) => formatter.write_str(message),
            Self::Scorer(error) => write!(formatter, "model scorer failed: {error}"),
            Self::CandidateLimit => formatter.write_str("beam candidate limit exceeded"),
            Self::Cancelled => formatter.write_str("beam decoding cancelled"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BeamSearchError<E> {}

/// One exclusive decode session over a retained constraint decoder.
///
/// The session is single owner. A cloned cancellation flag may be set from
/// another thread while `step` is evaluating model scores; the next check
/// aborts without publishing partial hypotheses.
pub struct ConstrainedBeamSession<C: ConstrainedDecoder> {
    constraint: C,
    config: BeamSearchConfig,
    hypotheses: Vec<BeamHypothesis>,
    stats: BeamSearchStats,
    finished: bool,
    cancellation: Arc<AtomicBool>,
}

impl<C: ConstrainedDecoder> ConstrainedBeamSession<C> {
    /// Construct a session with one zero-length initial hypothesis.
    pub fn new(constraint: C, config: BeamSearchConfig) -> Result<Self, &'static str> {
        config.validate()?;
        if constraint.vocab_size() > u32::MAX as usize
            || constraint.vocab_size() > config.max_vocabulary
        {
            return Err("constraint vocabulary exceeds the configured limit");
        }
        let initial = constraint.initial_state();
        Ok(Self {
            constraint,
            config,
            hypotheses: vec![BeamHypothesis {
                tokens: Vec::new(),
                score: 0.0,
                state: initial,
            }],
            stats: BeamSearchStats::default(),
            finished: config.max_length == 0,
            cancellation: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Share a cancellation flag without sharing the mutable decoder itself.
    pub fn cancellation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancellation)
    }

    /// Request cancellation before the next hypothesis is committed.
    pub fn cancel(&self) {
        self.cancellation.store(true, AtomicOrdering::Release);
    }

    /// Return the last committed partial hypotheses in best-first order.
    pub fn hypotheses(&self) -> &[BeamHypothesis] {
        &self.hypotheses
    }

    /// Return only accepting hypotheses from the current beam.
    pub fn accepted(&self) -> Vec<BeamHypothesis> {
        self.hypotheses
            .iter()
            .filter(|hypothesis| self.constraint.is_accepting(&hypothesis.state))
            .cloned()
            .collect()
    }

    /// Test whether one current hypothesis accepts the constraint.
    pub fn is_accepting(&self, index: usize) -> Option<bool> {
        self.hypotheses
            .get(index)
            .map(|hypothesis| self.constraint.is_accepting(&hypothesis.state))
    }

    /// Return cumulative committed work counters.
    pub fn stats(&self) -> BeamSearchStats {
        self.stats
    }

    /// Whether decoding has reached a terminal condition.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Advance one token step using a fallible model callback.
    ///
    /// The callback fills a reusable `vocab_size`-element score buffer. The
    /// buffer is reset to negative infinity before each call, so untouched
    /// tokens remain excluded. NaN and positive infinity are rejected. A
    /// callback error, cancellation, or candidate limit leaves the previous
    /// beam and its counters unchanged.
    pub fn step<E, F>(&mut self, mut score: F) -> Result<BeamStep, BeamSearchError<E>>
    where
        F: FnMut(&[TokenId], &mut [f64]) -> Result<(), E>,
    {
        if self.cancellation.load(AtomicOrdering::Acquire) {
            return Err(BeamSearchError::Cancelled);
        }
        if self.finished {
            return Ok(if self.accepted().is_empty() {
                BeamStep::Exhausted
            } else {
                BeamStep::Complete
            });
        }

        let mut next = Vec::new();
        let mut scores = vec![f64::NEG_INFINITY; self.constraint.vocab_size()];
        let mut score_calls = 0u64;
        let mut considered = 0u64;
        let mut expansions = 0usize;
        for hypothesis in &self.hypotheses {
            if self.cancellation.load(AtomicOrdering::Acquire) {
                return Err(BeamSearchError::Cancelled);
            }
            if self.constraint.is_accepting(&hypothesis.state) {
                next.push(hypothesis.clone());
            }
            let mask = self.constraint.valid_tokens(&hypothesis.state);
            if mask.count_valid() == 0 {
                continue;
            }
            scores.fill(f64::NEG_INFINITY);
            score(&hypothesis.tokens, &mut scores).map_err(BeamSearchError::Scorer)?;
            score_calls = score_calls.saturating_add(1);
            for token in mask.iter_valid() {
                if self.cancellation.load(AtomicOrdering::Acquire) {
                    return Err(BeamSearchError::Cancelled);
                }
                considered = considered.saturating_add(1);
                let token_score = scores[token as usize];
                if token_score == f64::NEG_INFINITY {
                    continue;
                }
                if !token_score.is_finite() {
                    return Err(BeamSearchError::InvalidScores(
                        "model score must be finite or negative infinity",
                    ));
                }
                let Some(state) = self.constraint.advance(&hypothesis.state, token) else {
                    continue;
                };
                if expansions == self.config.max_candidates_per_step {
                    return Err(BeamSearchError::CandidateLimit);
                }
                expansions += 1;
                let cumulative = hypothesis.score + token_score / self.config.temperature;
                if !cumulative.is_finite() {
                    return Err(BeamSearchError::InvalidScores(
                        "cumulative model score is not finite",
                    ));
                }
                let mut tokens = hypothesis.tokens.clone();
                tokens.push(token);
                next.push(BeamHypothesis {
                    tokens,
                    score: cumulative,
                    state,
                });
            }
        }

        let candidate_count = expansions as u64;
        let step = if next.is_empty() {
            BeamStep::Exhausted
        } else {
            let length_penalty = self.config.length_penalty;
            next.sort_by(|left, right| {
                let rank = |hypothesis: &BeamHypothesis| {
                    hypothesis.score / (hypothesis.tokens.len() as f64).powf(length_penalty)
                };
                rank(right)
                    .total_cmp(&rank(left))
                    .then_with(|| left.tokens.cmp(&right.tokens))
            });
            next.truncate(self.config.beam_width);
            BeamStep::Advanced
        };
        if self.cancellation.load(AtomicOrdering::Acquire) {
            return Err(BeamSearchError::Cancelled);
        }
        if !next.is_empty() {
            self.hypotheses = next;
        }
        self.stats.steps = self.stats.steps.saturating_add(1);
        self.stats.score_calls = self.stats.score_calls.saturating_add(score_calls);
        self.stats.considered_tokens = self.stats.considered_tokens.saturating_add(considered);
        self.stats.candidates = self.stats.candidates.saturating_add(candidate_count);
        let terminal = step == BeamStep::Exhausted
            || expansions == 0
            || self.stats.steps as usize >= self.config.max_length
            || self.hypotheses.iter().all(|hypothesis| {
                self.constraint
                    .valid_tokens(&hypothesis.state)
                    .count_valid()
                    == 0
            });
        self.finished = terminal;
        Ok(if terminal {
            if self.accepted().is_empty() {
                BeamStep::Exhausted
            } else {
                BeamStep::Complete
            }
        } else {
            BeamStep::Advanced
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{from_regex, WfstConstraint};
    use crate::semiring::TropicalWeight;

    fn decoder() -> WfstConstraint<TropicalWeight> {
        from_regex::<TropicalWeight>("ab", 256).expect("valid regex")
    }

    #[test]
    fn streams_partial_hypotheses_and_accepts_only_complete_strings() {
        let mut session = ConstrainedBeamSession::new(
            decoder(),
            BeamSearchConfig {
                beam_width: 2,
                max_length: 4,
                max_candidates_per_step: 4,
                ..BeamSearchConfig::default()
            },
        )
        .unwrap();
        assert_eq!(
            session.step(|_, scores| {
                scores.fill(0.0);
                Ok::<_, ()>(())
            }),
            Ok(BeamStep::Advanced)
        );
        assert_eq!(session.hypotheses()[0].tokens, vec![b'a' as u32]);
        assert!(session.accepted().is_empty());
        assert_eq!(
            session.step(|_, scores| {
                scores.fill(0.0);
                Ok::<_, ()>(())
            }),
            Ok(BeamStep::Complete)
        );
        assert_eq!(session.accepted()[0].tokens, vec![b'a' as u32, b'b' as u32]);
        assert_eq!(session.stats().steps, 2);
        assert_eq!(session.stats().score_calls, 2);
    }

    #[test]
    fn failed_step_preserves_beam_and_counters() {
        let mut session =
            ConstrainedBeamSession::new(decoder(), BeamSearchConfig::default()).unwrap();
        assert!(matches!(
            session.step(|_, scores| {
                scores[b'a' as usize] = f64::NAN;
                Ok::<_, ()>(())
            }),
            Err(BeamSearchError::InvalidScores(_))
        ));
        assert_eq!(session.hypotheses()[0].tokens, Vec::<u32>::new());
        assert_eq!(session.stats(), BeamSearchStats::default());
        session.cancel();
        assert!(matches!(
            session.step(|_, _| Ok::<_, ()>(())),
            Err(BeamSearchError::Cancelled)
        ));
        assert_eq!(session.stats(), BeamSearchStats::default());
    }

    #[test]
    fn accepting_prefix_remains_available_while_longer_path_decodes() {
        let constraint = from_regex::<TropicalWeight>("a|ab", 256).expect("valid regex");
        let mut session = ConstrainedBeamSession::new(
            constraint,
            BeamSearchConfig {
                beam_width: 2,
                ..BeamSearchConfig::default()
            },
        )
        .unwrap();
        assert_eq!(
            session.step(|_, scores| {
                scores[b'a' as usize] = 0.0;
                Ok::<_, ()>(())
            }),
            Ok(BeamStep::Advanced)
        );
        assert_eq!(session.accepted()[0].tokens, vec![b'a' as u32]);
        assert_eq!(
            session.step(|_, scores| {
                scores[b'b' as usize] = 0.0;
                Ok::<_, ()>(())
            }),
            Ok(BeamStep::Advanced)
        );
        assert_eq!(session.accepted().len(), 2);
        assert_eq!(session.stats().candidates, 2);
        assert_eq!(session.step(|_, _| Ok::<_, ()>(())), Ok(BeamStep::Complete));
    }

    #[test]
    fn candidate_limit_is_transactional_and_cancellation_is_shareable() {
        let mut session = ConstrainedBeamSession::new(
            from_regex::<TropicalWeight>("[ab]", 256).expect("valid regex"),
            BeamSearchConfig {
                max_candidates_per_step: 1,
                ..BeamSearchConfig::default()
            },
        )
        .unwrap();
        assert!(matches!(
            session.step(|_, scores| {
                scores.fill(0.0);
                Ok::<_, ()>(())
            }),
            Err(BeamSearchError::CandidateLimit)
        ));
        assert_eq!(session.hypotheses()[0].tokens, Vec::<u32>::new());
        assert_eq!(session.stats(), BeamSearchStats::default());
        let flag = session.cancellation_flag();
        flag.store(true, AtomicOrdering::Release);
        assert!(matches!(
            session.step(|_, _| Ok::<_, ()>(())),
            Err(BeamSearchError::Cancelled)
        ));
    }
}
