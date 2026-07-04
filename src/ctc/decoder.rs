//! CTC decoder with WFST composition.
//!
//! This module provides CTC decoding by composing CTC topologies with
//! language model WFSTs. It supports multiple decoding strategies:
//!
//! - **Greedy decoding**: Fast, single-best path
//! - **Beam search**: Configurable beam width for n-best
//! - **Lattice generation**: Full lattice for downstream processing
//!
//! ## Decoding Pipeline
//!
//! ```text
//! Posteriors → ObservationFst → CTC ∘ L ∘ G → Best Path
//!     │              │              │            │
//!     │              │              │            └─ Viterbi/Beam search
//!     │              │              └─ WFST composition
//!     │              └─ Frame-by-frame posterior FST
//!     └─ Neural network output (log probs)
//! ```
//!
//! ## Example
//!
//! ```ignore
//! use lling_llang::ctc::{CtcDecoder, CtcDecoderConfig, compact_ctc};
//! use lling_llang::asr::NgramTransducer;
//! use lling_llang::semiring::LogWeight;
//!
//! // Create decoder with CTC topology and language model
//! let ctc = compact_ctc::<LogWeight>(vocab_size);
//! let decoder = CtcDecoder::new(ctc)
//!     .with_language_model(lm_fst)
//!     .with_config(CtcDecoderConfig {
//!         beam_width: 10.0,
//!         max_active: 5000,
//!         ..Default::default()
//!     });
//!
//! // Decode posteriors to text
//! let result = decoder.decode(&posteriors)?;
//! println!("Decoded: {:?}", result.words);
//! ```
//!
//! ## References
//!
//! - Graves et al., "Connectionist Temporal Classification" (ICML 2006)
//! - Miao et al., "EESEN: End-to-end speech recognition using deep RNN" (ASRU 2015)

use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use crate::composition::{compose, materialize};
use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst, NO_STATE};

use super::topologies::{CtcLabel, CtcTopology, BLANK};

/// Configuration for CTC decoding.
#[derive(Clone, Debug)]
pub struct CtcDecoderConfig {
    /// Beam width for pruning (in log space).
    /// Paths worse than best + beam_width are pruned.
    pub beam_width: f64,

    /// Maximum number of active tokens per frame.
    pub max_active: usize,

    /// Minimum number of active tokens per frame.
    pub min_active: usize,

    /// Acoustic model scale (weight for AM scores).
    pub acoustic_scale: f64,

    /// Language model scale (weight for LM scores).
    pub lm_scale: f64,

    /// Word insertion penalty (added for each word).
    pub word_insertion_penalty: f64,

    /// Whether to use greedy decoding (ignores beam settings).
    pub greedy: bool,

    /// Whether to generate a lattice for n-best extraction.
    pub generate_lattice: bool,

    /// Maximum frames to decode (0 = unlimited).
    pub max_frames: usize,
}

impl Default for CtcDecoderConfig {
    fn default() -> Self {
        Self {
            beam_width: 16.0,
            max_active: 7000,
            min_active: 200,
            acoustic_scale: 1.0,
            lm_scale: 1.0,
            word_insertion_penalty: 0.0,
            greedy: false,
            generate_lattice: false,
            max_frames: 0,
        }
    }
}

impl CtcDecoderConfig {
    /// Create config for greedy decoding.
    pub fn greedy() -> Self {
        Self {
            greedy: true,
            ..Default::default()
        }
    }

    /// Create config for beam search with specified width.
    pub fn beam(width: f64) -> Self {
        Self {
            beam_width: width,
            greedy: false,
            ..Default::default()
        }
    }
}

/// Result of CTC decoding.
#[derive(Clone, Debug)]
pub struct DecodingResult {
    /// Decoded label sequence (without blanks).
    pub labels: Vec<CtcLabel>,

    /// Total score (acoustic + language model).
    pub score: f64,

    /// Acoustic model score component.
    pub am_score: f64,

    /// Language model score component.
    pub lm_score: f64,

    /// Number of frames processed.
    pub num_frames: usize,

    /// Decoding statistics.
    pub stats: DecodingStats,
}

/// Statistics from the decoding process.
#[derive(Clone, Debug, Default)]
pub struct DecodingStats {
    /// Total tokens created during decoding.
    pub tokens_created: usize,

    /// Maximum active tokens at any frame.
    pub max_active_reached: usize,

    /// Number of tokens pruned.
    pub tokens_pruned: usize,

    /// Time spent in decoding (microseconds).
    pub decode_time_us: u64,
}

/// Observation FST representing frame posteriors.
///
/// This FST has one state per frame with arcs for each label,
/// weighted by the log posterior probability.
#[derive(Clone, Debug)]
pub struct ObservationFst<W: Semiring> {
    /// The underlying WFST (linear chain).
    pub fst: VectorWfst<CtcLabel, W>,
    /// Number of frames.
    pub num_frames: usize,
    /// Vocabulary size (including blank).
    pub vocab_size: usize,
}

/// Error returned by fallible [`ObservationFst`] constructors.
#[derive(Clone, Debug, PartialEq)]
pub enum ObservationFstError {
    /// No posterior frames were provided.
    EmptyInput,
    /// Posterior frames exist, but they contain no labels.
    EmptyVocabulary,
    /// The number of frames cannot be represented as concrete WFST states.
    FrameCountExceedsStateSpace {
        /// Requested number of posterior frames.
        num_frames: usize,
        /// Maximum number of frames representable by concrete WFST state IDs.
        max_frames: usize,
    },
    /// The vocabulary cannot be represented by the CTC label type.
    VocabSizeExceedsLabelSpace {
        /// Requested vocabulary size.
        vocab_size: usize,
        /// Maximum vocabulary size representable by CTC labels.
        max_vocab_size: usize,
    },
    /// A posterior frame has a different vocabulary width from the first frame.
    InconsistentFrameSize {
        /// Zero-based frame index.
        frame: usize,
        /// Number of labels in this frame.
        actual: usize,
        /// Expected number of labels.
        expected: usize,
    },
    /// Scaling a posterior produced a value outside the verified log-weight domain.
    InvalidLogWeight {
        /// Zero-based frame index.
        frame: usize,
        /// Zero-based label index.
        label: usize,
        /// Raw weight value that failed validation.
        value: f64,
    },
}

impl fmt::Display for ObservationFstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "Posteriors cannot be empty"),
            Self::EmptyVocabulary => write!(f, "Posteriors must contain at least one label"),
            Self::FrameCountExceedsStateSpace {
                num_frames,
                max_frames,
            } => write!(
                f,
                "Posterior frame count {} exceeds maximum concrete WFST frames {}",
                num_frames, max_frames
            ),
            Self::VocabSizeExceedsLabelSpace {
                vocab_size,
                max_vocab_size,
            } => write!(
                f,
                "Posterior vocabulary size {} exceeds maximum CTC labels {}",
                vocab_size, max_vocab_size
            ),
            Self::InconsistentFrameSize {
                frame,
                actual,
                expected,
            } => write!(
                f,
                "Frame {} has {} labels, expected {}",
                frame, actual, expected
            ),
            Self::InvalidLogWeight {
                frame,
                label,
                value,
            } => write!(
                f,
                "Posterior frame {} label {} produced invalid log weight {}",
                frame, label, value
            ),
        }
    }
}

impl std::error::Error for ObservationFstError {}

impl ObservationFst<LogWeight> {
    /// Create observation FST from log posteriors.
    ///
    /// # Arguments
    ///
    /// * `posteriors` - Frame posteriors as `[num_frames][vocab_size]` log probabilities.
    ///                  Each inner vector should have `vocab_size` elements.
    ///
    /// # Panics
    ///
    /// Panics if posteriors cannot form a rectangular, non-empty observation
    /// tensor. Use [`Self::try_from_posteriors`] when invalid input should be
    /// handled without panicking.
    pub fn from_posteriors(posteriors: &[Vec<f32>]) -> Self {
        Self::try_from_posteriors(posteriors).unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create an observation FST from log posteriors.
    ///
    /// The posterior tensor must be rectangular with shape
    /// `[num_frames][vocab_size]`, `num_frames >= 1`, and `vocab_size >= 1`.
    pub fn try_from_posteriors(posteriors: &[Vec<f32>]) -> Result<Self, ObservationFstError> {
        Self::try_from_posteriors_scaled(posteriors, 1.0)
    }

    /// Create observation FST with acoustic scaling.
    ///
    /// Applies `weight = acoustic_scale * (-log_prob)` to all arcs.
    ///
    /// # Panics
    ///
    /// Panics if posteriors cannot form a rectangular, non-empty observation
    /// tensor or scaling produces an invalid [`LogWeight`]. Use
    /// [`Self::try_from_posteriors_scaled`] when invalid input should be handled
    /// without panicking.
    pub fn from_posteriors_scaled(posteriors: &[Vec<f32>], acoustic_scale: f64) -> Self {
        Self::try_from_posteriors_scaled(posteriors, acoustic_scale)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Try to create an observation FST with acoustic scaling.
    ///
    /// Applies `weight = acoustic_scale * (-log_prob)` to every frame-label arc
    /// after validating tensor shape and log-weight domain membership.
    pub fn try_from_posteriors_scaled(
        posteriors: &[Vec<f32>],
        acoustic_scale: f64,
    ) -> Result<Self, ObservationFstError> {
        let Some(first_frame) = posteriors.first() else {
            return Err(ObservationFstError::EmptyInput);
        };

        let num_frames = posteriors.len();
        let max_frames = (NO_STATE as usize).saturating_sub(1);
        if num_frames > max_frames {
            return Err(ObservationFstError::FrameCountExceedsStateSpace {
                num_frames,
                max_frames,
            });
        }

        let vocab_size = first_frame.len();
        if vocab_size == 0 {
            return Err(ObservationFstError::EmptyVocabulary);
        }

        let max_vocab_size = NO_STATE as usize;
        if vocab_size > max_vocab_size {
            return Err(ObservationFstError::VocabSizeExceedsLabelSpace {
                vocab_size,
                max_vocab_size,
            });
        }

        // Create FST with linear chain structure: s0 -> s1 -> ... -> sN
        let mut fst: VectorWfst<CtcLabel, LogWeight> = VectorWfst::with_capacity(num_frames + 1);

        // Add states (one per frame + final state)
        for _ in 0..=num_frames {
            fst.add_state();
        }

        fst.set_start(0);
        fst.set_final(num_frames as StateId, LogWeight::one());

        // Add arcs for each frame
        for (frame_idx, frame_posteriors) in posteriors.iter().enumerate() {
            if frame_posteriors.len() != vocab_size {
                return Err(ObservationFstError::InconsistentFrameSize {
                    frame: frame_idx,
                    actual: frame_posteriors.len(),
                    expected: vocab_size,
                });
            }

            let from_state = frame_idx as StateId;
            let to_state = (frame_idx + 1) as StateId;

            // Pre-allocate transitions for this state
            fst.reserve_transitions(from_state, vocab_size);

            // Add arc for each label
            for (label, &log_prob) in frame_posteriors.iter().enumerate() {
                let label = label as CtcLabel;
                // Output epsilon for blank, otherwise output the label
                let output = if label == BLANK { None } else { Some(label) };
                // Weight is negative log probability (higher prob = lower weight)
                let raw_weight = acoustic_scale * (-(log_prob as f64));
                let weight = LogWeight::try_new(raw_weight).ok_or(
                    ObservationFstError::InvalidLogWeight {
                        frame: frame_idx,
                        label: label as usize,
                        value: raw_weight,
                    },
                )?;
                fst.add_arc(from_state, Some(label), output, to_state, weight);
            }
        }

        Ok(Self {
            fst,
            num_frames,
            vocab_size,
        })
    }
}

/// CTC decoder combining CTC topology with language model.
pub struct CtcDecoder<W: Semiring> {
    /// CTC topology FST.
    ctc_topology: Arc<CtcTopology<W>>,

    /// Language model FST (optional).
    language_model: Option<Arc<VectorWfst<CtcLabel, W>>>,

    /// Subword lexicon FST for subword→word mapping (optional).
    lexicon: Option<Arc<VectorWfst<CtcLabel, W>>>,

    /// Decoder configuration.
    config: CtcDecoderConfig,

    /// Composed FST (CTC ∘ L ∘ G) - cached for reuse.
    composed_fst: Option<Arc<VectorWfst<CtcLabel, W>>>,
}

impl<W: Semiring + Clone> CtcDecoder<W> {
    /// Create a new CTC decoder with the given topology.
    pub fn new(ctc_topology: CtcTopology<W>) -> Self {
        Self {
            ctc_topology: Arc::new(ctc_topology),
            language_model: None,
            lexicon: None,
            config: CtcDecoderConfig::default(),
            composed_fst: None,
        }
    }

    /// Set the language model FST.
    pub fn with_language_model(mut self, lm: VectorWfst<CtcLabel, W>) -> Self {
        self.language_model = Some(Arc::new(lm));
        self.composed_fst = None; // Invalidate cache
        self
    }

    /// Set the lexicon FST for subword→word mapping.
    pub fn with_lexicon(mut self, lexicon: VectorWfst<CtcLabel, W>) -> Self {
        self.lexicon = Some(Arc::new(lexicon));
        self.composed_fst = None; // Invalidate cache
        self
    }

    /// Set decoder configuration.
    pub fn with_config(mut self, config: CtcDecoderConfig) -> Self {
        self.config = config;
        self
    }

    /// Get the CTC topology.
    pub fn ctc_topology(&self) -> &CtcTopology<W> {
        &self.ctc_topology
    }

    /// Get the configuration.
    pub fn config(&self) -> &CtcDecoderConfig {
        &self.config
    }
}

/// Scale all weights in a WFST by a constant factor (in log space).
///
/// In the log semiring, this multiplies all negative log probabilities by `scale`,
/// effectively raising the underlying probabilities to the power `scale`:
/// - `scaled_weight = scale * weight = scale * (-log(p)) = -log(p^scale)`
///
/// # Arguments
/// * `fst` - The input WFST
/// * `scale` - The scaling factor (1.0 = no change)
///
/// # Returns
/// A new WFST with scaled weights
fn scale_weights<L: Clone + Send + Sync>(
    fst: &VectorWfst<L, LogWeight>,
    scale: f64,
) -> VectorWfst<L, LogWeight> {
    use crate::wfst::StateId;

    let mut scaled = VectorWfst::with_capacity(fst.num_states());

    // Add all states
    for _ in 0..fst.num_states() {
        scaled.add_state();
    }

    // Set start state
    scaled.set_start(fst.start());

    // Copy and scale transitions and final weights
    for state_id in 0..fst.num_states() as StateId {
        // Scale final weight
        if fst.is_final(state_id) {
            let final_w = fst.final_weight(state_id);
            let scaled_final = LogWeight::new(final_w.value() * scale);
            scaled.set_final(state_id, scaled_final);
        }

        // Scale arc weights
        for arc in fst.transitions(state_id) {
            let scaled_weight = LogWeight::new(arc.weight.value() * scale);
            scaled.add_arc(
                arc.from,
                arc.input.clone(),
                arc.output.clone(),
                arc.to,
                scaled_weight,
            );
        }
    }

    scaled
}

impl CtcDecoder<LogWeight> {
    /// Decode acoustic posteriors to a label sequence.
    ///
    /// # Arguments
    ///
    /// * `posteriors` - Log posterior probabilities `[num_frames][vocab_size]`
    ///
    /// # Returns
    ///
    /// Decoding result with best label sequence and scores.
    pub fn decode(&self, posteriors: &[Vec<f32>]) -> Result<DecodingResult, DecodingError> {
        if posteriors.is_empty() {
            return Err(DecodingError::EmptyInput);
        }

        let start_time = std::time::Instant::now();

        // Build observation FST from posteriors
        let obs_fst =
            ObservationFst::try_from_posteriors_scaled(posteriors, self.config.acoustic_scale)
                .map_err(DecodingError::InvalidPosteriors)?;

        // Check vocabulary size compatibility
        if obs_fst.vocab_size != self.ctc_topology.vocab_size() {
            return Err(DecodingError::VocabMismatch {
                posterior_vocab: obs_fst.vocab_size,
                ctc_vocab: self.ctc_topology.vocab_size(),
            });
        }

        let num_frames = obs_fst.num_frames;

        // Compose observation with CTC topology
        let lazy_obs_ctc = compose(obs_fst.fst, self.ctc_topology.fst().clone());
        let obs_ctc = materialize(lazy_obs_ctc);

        // Compose with language model if available
        let search_fst = if let Some(ref lm) = self.language_model {
            // Scale LM weights by lm_scale (skip if scale is 1.0)
            let scaled_lm = if (self.config.lm_scale - 1.0).abs() > f64::EPSILON {
                scale_weights(&**lm, self.config.lm_scale)
            } else {
                (**lm).clone()
            };
            let lazy_composed = compose(obs_ctc, scaled_lm);
            materialize(lazy_composed)
        } else {
            obs_ctc
        };

        // Find best path
        let result = if self.config.greedy {
            self.greedy_decode(&search_fst)?
        } else {
            self.beam_decode(&search_fst)?
        };

        let decode_time_us = start_time.elapsed().as_micros() as u64;

        Ok(DecodingResult {
            labels: result.labels,
            score: result.score,
            am_score: result.am_score,
            lm_score: result.lm_score,
            num_frames,
            stats: DecodingStats {
                decode_time_us,
                ..result.stats
            },
        })
    }

    /// Greedy decoding using Viterbi on WFST.
    ///
    /// Finds the single best path through the FST using dynamic programming.
    fn greedy_decode(
        &self,
        fst: &VectorWfst<CtcLabel, LogWeight>,
    ) -> Result<DecodingResult, DecodingError> {
        if fst.num_states() == 0 {
            return Err(DecodingError::NoPath);
        }

        let start = fst.start();
        let num_states = fst.num_states();
        if !fst.is_valid_state(start) {
            return Err(DecodingError::NoPath);
        }

        // Forward pass: compute best score to each state
        // (best_score, backpointer_state, backpointer_arc)
        let mut best: Vec<Option<(LogWeight, StateId, usize)>> = vec![None; num_states];
        best[start as usize] = Some((LogWeight::one(), start, 0));

        // Process in state order (assuming topological order for acyclic FST)
        for state in 0..num_states as StateId {
            if best[state as usize].is_none() {
                continue;
            }
            let Some((current_score, _, _)) = best[state as usize] else {
                continue;
            };

            let transitions = fst.transitions(state);
            for (arc_idx, arc) in transitions.iter().enumerate() {
                if !fst.is_valid_state(arc.to) {
                    continue;
                }

                let new_score = current_score.times(&arc.weight);
                let target = arc.to as usize;

                let update = match &best[target] {
                    None => true,
                    Some((existing_score, _, _)) => new_score.value() < existing_score.value(),
                };

                if update {
                    best[target] = Some((new_score, state, arc_idx));
                }
            }
        }

        // Find best final state
        let mut best_final: Option<(LogWeight, StateId)> = None;
        for state in 0..num_states as StateId {
            if fst.is_final(state) {
                if let Some((score, _, _)) = &best[state as usize] {
                    let final_weight = fst.final_weight(state);
                    let total = score.times(&final_weight);

                    let update = match &best_final {
                        None => true,
                        Some((existing, _)) => total.value() < existing.value(),
                    };

                    if update {
                        best_final = Some((total, state));
                    }
                }
            }
        }

        let (final_score, end_state) = match best_final {
            Some(result) => result,
            None => return Err(DecodingError::NoPath),
        };

        // Backward pass: reconstruct path
        let mut labels = Vec::with_capacity(num_states);
        let mut current = end_state;
        let mut visited_backtrace = vec![false; num_states];

        while current != start {
            let current_idx = current as usize;
            let Some(visited) = visited_backtrace.get_mut(current_idx) else {
                return Err(DecodingError::NoPath);
            };
            if *visited {
                return Err(DecodingError::NoPath);
            }
            *visited = true;

            let Some((_, prev_state, arc_idx)) =
                best.get(current_idx).and_then(|entry| entry.as_ref())
            else {
                return Err(DecodingError::NoPath);
            };

            let Some(arc) = fst.transitions(*prev_state).get(*arc_idx) else {
                return Err(DecodingError::NoPath);
            };
            if let Some(output) = arc.output {
                labels.push(output);
            }
            current = *prev_state;
        }

        // Reverse since we traced backward
        labels.reverse();

        Ok(DecodingResult {
            labels,
            score: final_score.value(),
            am_score: final_score.value(),
            lm_score: 0.0,
            num_frames: 0,
            stats: DecodingStats::default(),
        })
    }

    /// Beam search decoding on WFST.
    ///
    /// Uses beam pruning to limit active hypotheses for efficiency.
    fn beam_decode(
        &self,
        fst: &VectorWfst<CtcLabel, LogWeight>,
    ) -> Result<DecodingResult, DecodingError> {
        if fst.num_states() == 0 {
            return Err(DecodingError::NoPath);
        }

        let start = fst.start();
        if !fst.is_valid_state(start) {
            return Err(DecodingError::NoPath);
        }

        let beam_width = self.config.beam_width;
        let max_active = self.config.max_active;

        // Token: (state, score, path of output labels)
        #[derive(Clone)]
        struct Token {
            state: StateId,
            score: LogWeight,
            labels: Vec<CtcLabel>,
        }

        // Initialize with start state
        let mut active = vec![Token {
            state: start,
            score: LogWeight::one(),
            labels: Vec::new(),
        }];

        let mut stats = DecodingStats::default();
        let mut best_completed: Option<Token> = None;

        // Process until no more active tokens
        while !active.is_empty() {
            let capacity_hint = active.len().saturating_mul(2).min(max_active).max(1);
            let mut next_active: Vec<Token> = Vec::with_capacity(capacity_hint);
            let mut state_best: HashMap<StateId, (LogWeight, usize)> =
                HashMap::with_capacity(capacity_hint);

            for token in &active {
                let transitions = fst.transitions(token.state);

                for arc in transitions.iter() {
                    if !fst.is_valid_state(arc.to) {
                        continue;
                    }

                    let new_score = token.score.times(&arc.weight);

                    let mut new_labels = token.labels.clone();
                    if let Some(output) = arc.output {
                        new_labels.push(output);
                    }

                    // Token recombination: keep best per state
                    let idx = next_active.len();
                    if let Some((existing_score, existing_idx)) = state_best.get(&arc.to) {
                        if new_score.value() < existing_score.value() {
                            next_active[*existing_idx] = Token {
                                state: arc.to,
                                score: new_score.clone(),
                                labels: new_labels,
                            };
                            state_best.insert(arc.to, (new_score, *existing_idx));
                        }
                    } else {
                        next_active.push(Token {
                            state: arc.to,
                            score: new_score.clone(),
                            labels: new_labels,
                        });
                        state_best.insert(arc.to, (new_score, idx));
                    }
                }

                // Check if this is a final state
                if fst.is_final(token.state) {
                    let final_weight = fst.final_weight(token.state);
                    let total = token.score.times(&final_weight);

                    let update = match &best_completed {
                        None => true,
                        Some(existing) => total.value() < existing.score.value(),
                    };

                    if update {
                        best_completed = Some(Token {
                            state: token.state,
                            score: total,
                            labels: token.labels.clone(),
                        });
                    }
                }
            }

            stats.tokens_created += next_active.len();

            // Beam pruning
            if !next_active.is_empty() {
                // Find best score
                let best_score = next_active
                    .iter()
                    .map(|t| t.score.value())
                    .fold(f64::INFINITY, f64::min);

                let threshold = best_score + beam_width;

                // Prune tokens outside beam
                let before_prune = next_active.len();
                next_active.retain(|t| t.score.value() <= threshold);
                stats.tokens_pruned += before_prune - next_active.len();

                // Limit max active
                if next_active.len() > max_active {
                    next_active.sort_by(|a, b| {
                        a.score
                            .value()
                            .partial_cmp(&b.score.value())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    next_active.truncate(max_active);
                }

                stats.max_active_reached = stats.max_active_reached.max(next_active.len());
            }

            active = next_active;
        }

        // Return best completed path
        match best_completed {
            Some(token) => Ok(DecodingResult {
                labels: token.labels,
                score: token.score.value(),
                am_score: token.score.value(),
                lm_score: 0.0,
                num_frames: 0,
                stats,
            }),
            None => Err(DecodingError::NoPath),
        }
    }

    /// Decode and collapse repeated labels.
    ///
    /// CTC allows repeated labels on consecutive frames. This method
    /// collapses them to produce the final output sequence.
    pub fn decode_and_collapse(
        &self,
        posteriors: &[Vec<f32>],
    ) -> Result<DecodingResult, DecodingError> {
        let mut result = self.decode(posteriors)?;

        // Collapse consecutive duplicates
        let mut collapsed = Vec::with_capacity(result.labels.len());
        let mut prev_label: Option<CtcLabel> = None;

        for label in &result.labels {
            if Some(*label) != prev_label {
                collapsed.push(*label);
            }
            prev_label = Some(*label);
        }

        result.labels = collapsed;
        Ok(result)
    }
}

/// Errors that can occur during decoding.
#[derive(Clone, Debug)]
pub enum DecodingError {
    /// Empty input posteriors.
    EmptyInput,
    /// Vocabulary size mismatch between posteriors and CTC topology.
    VocabMismatch {
        /// Vocabulary size in posteriors.
        posterior_vocab: usize,
        /// Vocabulary size in CTC topology.
        ctc_vocab: usize,
    },
    /// Posterior tensor shape or values are invalid.
    InvalidPosteriors(ObservationFstError),
    /// No valid path found.
    NoPath,
    /// Composition failed.
    CompositionError(String),
}

impl std::fmt::Display for DecodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "Empty input posteriors"),
            Self::VocabMismatch {
                posterior_vocab,
                ctc_vocab,
            } => {
                write!(
                    f,
                    "Vocabulary size mismatch: posteriors have {} labels, CTC has {}",
                    posterior_vocab, ctc_vocab
                )
            }
            Self::InvalidPosteriors(err) => write!(f, "Invalid posteriors: {}", err),
            Self::NoPath => write!(f, "No valid path found during decoding"),
            Self::CompositionError(msg) => write!(f, "Composition error: {}", msg),
        }
    }
}

impl std::error::Error for DecodingError {}

/// Token for frame-synchronous beam search.
///
/// Represents an active hypothesis during decoding.
#[derive(Clone, Debug)]
pub struct DecoderToken<W: Semiring> {
    /// Current state in the search FST.
    pub state: StateId,
    /// Accumulated score.
    pub score: W,
    /// Backpointer to previous token (for traceback).
    pub backpointer: Option<usize>,
    /// Label that led to this token.
    pub label: Option<CtcLabel>,
    /// Word sequence (for LM integration).
    pub words: Vec<u32>,
}

/// Backtrace node for streaming CTC hypotheses.
#[derive(Clone, Debug)]
struct TraceNode {
    /// Previous node in the hypothesis trace.
    prev: Option<usize>,
    /// Raw CTC output symbol emitted by this step.
    ///
    /// `None` means the transition consumed no frame and emitted no symbol.
    /// `Some(None)` means a blank frame was consumed. `Some(Some(label))`
    /// means a non-blank label was emitted.
    emitted: Option<Option<CtcLabel>>,
}

impl<W: Semiring + Clone> DecoderToken<W> {
    /// Create initial token at start state.
    pub fn initial(state: StateId) -> Self {
        Self {
            state,
            score: W::one(),
            backpointer: None,
            label: None,
            words: Vec::new(),
        }
    }

    /// Extend token along an arc.
    pub fn extend(
        &self,
        to_state: StateId,
        weight: W,
        label: Option<CtcLabel>,
        token_idx: usize,
    ) -> Self {
        Self {
            state: to_state,
            score: self.score.clone().times(&weight),
            backpointer: Some(token_idx),
            label,
            words: self.words.clone(),
        }
    }
}

/// Frame-synchronous decoder for streaming applications.
///
/// This decoder processes frames one at a time, maintaining active
/// hypotheses between frames for real-time decoding.
pub struct StreamingCtcDecoder<W: Semiring> {
    /// CTC decoder (for topology and config).
    decoder: CtcDecoder<W>,
    /// Active tokens at current frame.
    active_tokens: Vec<DecoderToken<W>>,
    /// Token history for traceback.
    token_history: Vec<Vec<DecoderToken<W>>>,
    /// Compact traceback arena for active hypotheses.
    trace_nodes: Vec<TraceNode>,
    /// Accumulated decoding statistics.
    stats: DecodingStats,
    /// Current frame index.
    current_frame: usize,
    /// State to token index mapping for recombination.
    state_map: HashMap<StateId, usize>,
}

impl<W> StreamingCtcDecoder<W>
where
    W: Semiring + Clone + PartialOrd + From<f64> + Into<f64>,
{
    /// Create a new streaming decoder.
    pub fn new(decoder: CtcDecoder<W>) -> Self {
        Self {
            decoder,
            active_tokens: Vec::new(),
            token_history: Vec::new(),
            trace_nodes: Vec::new(),
            stats: DecodingStats::default(),
            current_frame: 0,
            state_map: HashMap::new(),
        }
    }

    /// Reset the decoder to initial state.
    pub fn reset(&mut self) {
        self.active_tokens.clear();
        self.token_history.clear();
        self.trace_nodes.clear();
        self.stats = DecodingStats::default();
        self.current_frame = 0;
        self.state_map.clear();

        // Initialize with start token
        let start = self.decoder.ctc_topology.fst().start();
        self.trace_nodes.push(TraceNode {
            prev: None,
            emitted: None,
        });
        let mut initial_token = DecoderToken::initial(start);
        initial_token.backpointer = Some(0);
        self.active_tokens.push(initial_token);
        self.state_map.insert(start, 0);
    }

    /// Process a single frame of posteriors.
    ///
    /// # Arguments
    ///
    /// * `posteriors` - Log posteriors for this frame `[vocab_size]`
    pub fn process_frame(&mut self, posteriors: &[f32]) {
        if self.active_tokens.is_empty() {
            return;
        }

        let vocab_size = posteriors.len();
        let capacity_hint = self
            .active_tokens
            .len()
            .saturating_mul(2)
            .min(self.decoder.config.max_active)
            .max(1);
        let mut new_tokens: Vec<DecoderToken<W>> = Vec::with_capacity(capacity_hint);
        let mut new_state_map: HashMap<StateId, usize> = HashMap::with_capacity(capacity_hint);
        let mut tokens_created = 0usize;

        let active = self.epsilon_closure(self.active_tokens.clone(), &mut tokens_created);

        // Extend each active token
        for token in &active {
            let state = token.state;
            let transitions = self.decoder.ctc_topology.fst().transitions(state).to_vec();

            // Get transitions from this state
            for trans in transitions {
                if let Some(input_label) = trans.input {
                    if (input_label as usize) < vocab_size {
                        let acoustic_weight =
                            self.posterior_weight(posteriors[input_label as usize]);
                        let arc_weight = trans.weight.times(&acoustic_weight);
                        let score = token.score.times(&arc_weight);
                        let trace_idx = self.push_trace(token.backpointer, Some(trans.output));
                        let new_token = DecoderToken {
                            state: trans.to,
                            score,
                            backpointer: Some(trace_idx),
                            label: trans.output,
                            words: token.words.clone(),
                        };

                        if Self::insert_or_update_token(
                            &mut new_tokens,
                            &mut new_state_map,
                            new_token,
                        )
                        .is_some()
                        {
                            tokens_created += 1;
                        }
                    }
                }
            }
        }

        let mut new_tokens = self.epsilon_closure(new_tokens, &mut tokens_created);
        self.prune_active_tokens(&mut new_tokens);
        self.rebuild_state_map(&new_tokens);

        self.active_tokens = new_tokens;
        self.token_history.push(self.active_tokens.clone());
        self.stats.tokens_created += tokens_created;
        self.stats.max_active_reached = self.stats.max_active_reached.max(self.active_tokens.len());
        self.current_frame += 1;
    }

    /// Get the current best hypothesis.
    pub fn best_hypothesis(&self) -> Vec<CtcLabel> {
        self.best_token_index()
            .map(|idx| self.labels_for_token(&self.active_tokens[idx]))
            .unwrap_or_default()
    }

    /// Finalize decoding and get the best result.
    pub fn finalize(&self) -> DecodingResult {
        let best = self
            .best_token_index()
            .map(|idx| {
                let token = &self.active_tokens[idx];
                let score = self.token_score_with_final(token);
                (self.labels_for_token(token), score.into())
            })
            .unwrap_or_else(|| (Vec::new(), f64::INFINITY));

        DecodingResult {
            labels: best.0,
            score: best.1,
            am_score: best.1,
            lm_score: 0.0,
            num_frames: self.current_frame,
            stats: self.stats.clone(),
        }
    }

    fn posterior_weight(&self, log_posterior: f32) -> W {
        W::from(self.decoder.config.acoustic_scale * (-(log_posterior as f64)))
    }

    fn push_trace(&mut self, prev: Option<usize>, emitted: Option<Option<CtcLabel>>) -> usize {
        let idx = self.trace_nodes.len();
        self.trace_nodes.push(TraceNode { prev, emitted });
        idx
    }

    fn epsilon_closure(
        &mut self,
        seeds: Vec<DecoderToken<W>>,
        tokens_created: &mut usize,
    ) -> Vec<DecoderToken<W>> {
        let mut tokens = Vec::with_capacity(seeds.len().max(1));
        let mut state_map = HashMap::with_capacity(seeds.len().max(1));
        let mut queue = VecDeque::with_capacity(seeds.len().max(1));

        for token in seeds {
            if let Some(idx) = Self::insert_or_update_token(&mut tokens, &mut state_map, token) {
                queue.push_back(idx);
            }
        }

        while let Some(idx) = queue.pop_front() {
            if idx >= tokens.len() {
                continue;
            }
            let token = tokens[idx].clone();
            let transitions = self
                .decoder
                .ctc_topology
                .fst()
                .transitions(token.state)
                .to_vec();

            for trans in transitions {
                if trans.input.is_some() {
                    continue;
                }

                let score = token.score.times(&trans.weight);
                let backpointer = if trans.output.is_some() {
                    Some(self.push_trace(token.backpointer, Some(trans.output)))
                } else {
                    token.backpointer
                };
                let epsilon_token = DecoderToken {
                    state: trans.to,
                    score,
                    backpointer,
                    label: trans.output,
                    words: token.words.clone(),
                };

                if let Some(updated_idx) =
                    Self::insert_or_update_token(&mut tokens, &mut state_map, epsilon_token)
                {
                    *tokens_created += 1;
                    queue.push_back(updated_idx);
                }
            }
        }

        tokens
    }

    fn insert_or_update_token(
        tokens: &mut Vec<DecoderToken<W>>,
        state_map: &mut HashMap<StateId, usize>,
        token: DecoderToken<W>,
    ) -> Option<usize> {
        if let Some(&existing_idx) = state_map.get(&token.state) {
            if Self::is_better_score(&token.score, &tokens[existing_idx].score) {
                tokens[existing_idx] = token;
                return Some(existing_idx);
            }
            None
        } else {
            let idx = tokens.len();
            state_map.insert(token.state, idx);
            tokens.push(token);
            Some(idx)
        }
    }

    fn is_better_score(candidate: &W, incumbent: &W) -> bool {
        candidate
            .natural_less(incumbent)
            .unwrap_or_else(|| matches!(candidate.partial_cmp(incumbent), Some(Ordering::Less)))
    }

    fn compare_scores(left: &W, right: &W) -> Ordering {
        if Self::is_better_score(left, right) {
            Ordering::Less
        } else if Self::is_better_score(right, left) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }

    fn prune_active_tokens(&mut self, tokens: &mut Vec<DecoderToken<W>>) {
        if tokens.is_empty() {
            return;
        }

        tokens.sort_by(|a, b| Self::compare_scores(&a.score, &b.score));

        let max_active = self.decoder.config.max_active.max(1);
        let min_active = self.decoder.config.min_active.min(max_active);

        let before_max = tokens.len();
        if tokens.len() > max_active {
            tokens.truncate(max_active);
        }
        self.stats.tokens_pruned += before_max - tokens.len();

        let best_score: f64 = tokens[0].score.into();
        let beam_cutoff = best_score + self.decoder.config.beam_width;
        let before_beam = tokens.len();
        let mut kept = Vec::with_capacity(tokens.len());
        for (idx, token) in tokens.drain(..).enumerate() {
            let score: f64 = token.score.into();
            if idx < min_active || score <= beam_cutoff {
                kept.push(token);
            }
        }
        self.stats.tokens_pruned += before_beam - kept.len();
        *tokens = kept;
    }

    fn rebuild_state_map(&mut self, tokens: &[DecoderToken<W>]) {
        self.state_map.clear();
        self.state_map.reserve(tokens.len());
        for (idx, token) in tokens.iter().enumerate() {
            self.state_map.insert(token.state, idx);
        }
    }

    fn best_token_index(&self) -> Option<usize> {
        let mut best_final: Option<(usize, W)> = None;
        let mut best_any: Option<(usize, W)> = None;

        for (idx, token) in self.active_tokens.iter().enumerate() {
            let raw_score = token.score;
            if best_any
                .as_ref()
                .map(|(_, best)| Self::is_better_score(&raw_score, best))
                .unwrap_or(true)
            {
                best_any = Some((idx, raw_score));
            }

            if self.decoder.ctc_topology.fst().is_final(token.state) {
                let final_score = self.token_score_with_final(token);
                if best_final
                    .as_ref()
                    .map(|(_, best)| Self::is_better_score(&final_score, best))
                    .unwrap_or(true)
                {
                    best_final = Some((idx, final_score));
                }
            }
        }

        best_final.or(best_any).map(|(idx, _)| idx)
    }

    fn token_score_with_final(&self, token: &DecoderToken<W>) -> W {
        if self.decoder.ctc_topology.fst().is_final(token.state) {
            token
                .score
                .times(&self.decoder.ctc_topology.fst().final_weight(token.state))
        } else {
            token.score
        }
    }

    fn labels_for_token(&self, token: &DecoderToken<W>) -> Vec<CtcLabel> {
        let mut raw = Vec::new();
        let mut current = token.backpointer;

        while let Some(idx) = current {
            if let Some(node) = self.trace_nodes.get(idx) {
                if let Some(emitted) = node.emitted {
                    raw.push(emitted);
                }
                current = node.prev;
            } else {
                break;
            }
        }

        raw.reverse();
        collapse_ctc_symbols(raw)
    }
}

fn collapse_ctc_symbols<I>(symbols: I) -> Vec<CtcLabel>
where
    I: IntoIterator<Item = Option<CtcLabel>>,
{
    let mut labels = Vec::new();
    let mut previous_raw: Option<CtcLabel> = None;

    for symbol in symbols {
        match symbol {
            Some(label) => {
                if label != BLANK && Some(label) != previous_raw {
                    labels.push(label);
                }
                previous_raw = Some(label);
            }
            None => {
                previous_raw = None;
            }
        }
    }

    labels
}

#[cfg(test)]
mod tests {
    use super::super::topologies::{compact_ctc, minimal_ctc};
    use super::*;

    #[test]
    fn test_decoder_config_default() {
        let config = CtcDecoderConfig::default();
        assert_eq!(config.beam_width, 16.0);
        assert_eq!(config.max_active, 7000);
        assert!(!config.greedy);
    }

    #[test]
    fn test_decoder_config_greedy() {
        let config = CtcDecoderConfig::greedy();
        assert!(config.greedy);
    }

    #[test]
    fn test_decoder_config_beam() {
        let config = CtcDecoderConfig::beam(8.0);
        assert_eq!(config.beam_width, 8.0);
        assert!(!config.greedy);
    }

    #[test]
    fn test_observation_fst_structure() {
        // 3 frames, 4 labels (including blank)
        let posteriors = vec![
            vec![-0.1, -1.0, -2.0, -3.0], // Frame 0
            vec![-0.5, -0.5, -2.0, -2.0], // Frame 1
            vec![-2.0, -2.0, -0.1, -2.0], // Frame 2
        ];

        let obs_fst = ObservationFst::from_posteriors(&posteriors);

        assert_eq!(obs_fst.num_frames, 3);
        assert_eq!(obs_fst.vocab_size, 4);

        // Should have 4 states (3 frames + final)
        assert_eq!(obs_fst.fst.num_states(), 4);

        // Each frame state should have 4 outgoing arcs
        for frame in 0..3 {
            assert_eq!(obs_fst.fst.transitions(frame as StateId).len(), 4);
        }

        // Final state should have no outgoing arcs
        assert_eq!(obs_fst.fst.transitions(3).len(), 0);
    }

    #[test]
    fn test_observation_fst_blank_epsilon() {
        let posteriors = vec![vec![-0.1, -1.0, -2.0]];
        let obs_fst = ObservationFst::from_posteriors(&posteriors);

        // Find blank arc (label 0)
        let blank_arc = obs_fst
            .fst
            .transitions(0)
            .iter()
            .find(|t| t.input == Some(0))
            .expect("Should have blank arc");

        assert_eq!(blank_arc.output, None, "Blank should output epsilon");
    }

    #[test]
    fn test_observation_fst_scaled() {
        let posteriors = vec![vec![-1.0, -2.0]];
        let scale = 0.5;

        let scaled = ObservationFst::from_posteriors_scaled(&posteriors, scale);
        let unscaled = ObservationFst::from_posteriors(&posteriors);

        // Scaled weights should be half of unscaled
        let scaled_weight = scaled.fst.transitions(0)[0].weight.value();
        let unscaled_weight = unscaled.fst.transitions(0)[0].weight.value();

        assert!((scaled_weight - unscaled_weight * scale).abs() < 1e-6);
    }

    #[test]
    fn test_observation_fst_try_rejects_empty_input() {
        let result = ObservationFst::try_from_posteriors(&[]);

        assert!(matches!(result, Err(ObservationFstError::EmptyInput)));
    }

    #[test]
    fn test_observation_fst_try_rejects_empty_vocabulary() {
        let posteriors = vec![Vec::new()];
        let result = ObservationFst::try_from_posteriors(&posteriors);

        assert!(matches!(result, Err(ObservationFstError::EmptyVocabulary)));
    }

    #[test]
    fn test_observation_fst_try_rejects_inconsistent_frame_sizes() {
        let posteriors = vec![vec![-0.1, -1.0], vec![-0.2]];
        let result = ObservationFst::try_from_posteriors(&posteriors);

        assert!(matches!(
            result,
            Err(ObservationFstError::InconsistentFrameSize {
                frame: 1,
                actual: 1,
                expected: 2
            })
        ));
    }

    #[test]
    fn test_observation_fst_try_rejects_invalid_log_weight() {
        let posteriors = vec![vec![f32::INFINITY]];
        let result = ObservationFst::try_from_posteriors(&posteriors);

        assert!(matches!(
            result,
            Err(ObservationFstError::InvalidLogWeight {
                frame: 0,
                label: 0,
                value
            }) if value.is_infinite() && value.is_sign_negative()
        ));
    }

    #[test]
    #[should_panic(expected = "Frame 1 has 1 labels, expected 2")]
    fn test_observation_fst_infallible_constructor_preserves_panic_contract() {
        let posteriors = vec![vec![-0.1, -1.0], vec![-0.2]];

        let _ = ObservationFst::from_posteriors(&posteriors);
    }

    #[test]
    fn test_scale_weights() {
        // Create a simple FST with known weights
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s2, LogWeight::new(3.0));

        // Add arcs with weights 1.0 and 2.0
        fst.add_arc(s0, Some(1), Some(1), s1, LogWeight::new(1.0));
        fst.add_arc(s1, Some(2), Some(2), s2, LogWeight::new(2.0));

        // Scale by 0.5
        let scaled = scale_weights(&fst, 0.5);

        // Verify structure preserved
        assert_eq!(scaled.num_states(), fst.num_states());
        assert_eq!(scaled.start(), fst.start());
        assert!(scaled.is_final(s2));

        // Verify weights are scaled
        assert!((scaled.transitions(s0)[0].weight.value() - 0.5).abs() < 1e-10);
        assert!((scaled.transitions(s1)[0].weight.value() - 1.0).abs() < 1e-10);
        assert!((scaled.final_weight(s2).value() - 1.5).abs() < 1e-10);
    }

    #[test]
    fn test_scale_weights_identity() {
        // Scale by 1.0 should preserve weights
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s1, LogWeight::new(2.0));
        fst.add_arc(s0, Some(1), Some(1), s1, LogWeight::new(3.0));

        let scaled = scale_weights(&fst, 1.0);

        assert!((scaled.transitions(s0)[0].weight.value() - 3.0).abs() < 1e-10);
        assert!((scaled.final_weight(s1).value() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_scale_weights_zero() {
        // Scale by 0.0 should make all weights zero (log prob of 1)
        let mut fst: VectorWfst<u32, LogWeight> = VectorWfst::new();
        let s0 = fst.add_state();
        let s1 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s1, LogWeight::new(5.0));
        fst.add_arc(s0, Some(1), Some(1), s1, LogWeight::new(10.0));

        let scaled = scale_weights(&fst, 0.0);

        assert!((scaled.transitions(s0)[0].weight.value()).abs() < 1e-10);
        assert!((scaled.final_weight(s1).value()).abs() < 1e-10);
    }

    #[test]
    fn test_ctc_decoder_creation() {
        let ctc = compact_ctc::<LogWeight>(10);
        let decoder = CtcDecoder::new(ctc);

        assert_eq!(decoder.ctc_topology().vocab_size(), 10);
        assert!(decoder.language_model.is_none());
    }

    #[test]
    fn test_ctc_decoder_with_config() {
        let ctc = compact_ctc::<LogWeight>(5);
        let config = CtcDecoderConfig {
            beam_width: 8.0,
            greedy: true,
            ..Default::default()
        };

        let decoder = CtcDecoder::new(ctc).with_config(config);

        assert_eq!(decoder.config().beam_width, 8.0);
        assert!(decoder.config().greedy);
    }

    #[test]
    fn test_decode_empty_error() {
        let ctc = compact_ctc::<LogWeight>(5);
        let decoder = CtcDecoder::new(ctc);

        let result = decoder.decode(&[]);
        assert!(matches!(result, Err(DecodingError::EmptyInput)));
    }

    #[test]
    fn test_decode_vocab_mismatch() {
        let ctc = compact_ctc::<LogWeight>(5);
        let decoder = CtcDecoder::new(ctc);

        // 10 labels but CTC expects 5
        let posteriors = vec![vec![-1.0; 10]];
        let result = decoder.decode(&posteriors);

        assert!(matches!(result, Err(DecodingError::VocabMismatch { .. })));
    }

    #[test]
    fn test_decode_inconsistent_posteriors_returns_error() {
        let ctc = compact_ctc::<LogWeight>(2);
        let decoder = CtcDecoder::new(ctc);
        let posteriors = vec![vec![-0.1, -1.0], vec![-0.2]];

        let result = decoder.decode(&posteriors);

        assert!(matches!(
            result,
            Err(DecodingError::InvalidPosteriors(
                ObservationFstError::InconsistentFrameSize {
                    frame: 1,
                    actual: 1,
                    expected: 2
                }
            ))
        ));
    }

    #[test]
    fn test_greedy_decode_simple() {
        let ctc = minimal_ctc::<LogWeight>(4);
        let decoder = CtcDecoder::new(ctc).with_config(CtcDecoderConfig::greedy());

        // Frame posteriors: [blank, 1, 2, 3]
        // Frame 0: label 1 is best
        // Frame 1: blank is best
        // Frame 2: label 2 is best
        let posteriors = vec![
            vec![-1.0, -0.1, -2.0, -3.0], // Label 1 wins
            vec![-0.1, -1.0, -2.0, -3.0], // Blank wins
            vec![-2.0, -2.0, -0.1, -3.0], // Label 2 wins
        ];

        let result = decoder.decode(&posteriors);
        assert!(result.is_ok());

        let decoded = result.expect("ctc/decoder.rs: required value was None/Err");
        assert_eq!(decoded.num_frames, 3);
    }

    #[test]
    fn test_decode_rejects_missing_start_state() {
        let ctc = minimal_ctc::<LogWeight>(2);
        let decoder = CtcDecoder::new(ctc);

        let mut fst: VectorWfst<CtcLabel, LogWeight> = VectorWfst::new();
        let state = fst.add_state();
        fst.set_final(state, LogWeight::one());

        assert!(matches!(
            decoder.greedy_decode(&fst),
            Err(DecodingError::NoPath)
        ));
        assert!(matches!(
            decoder.beam_decode(&fst),
            Err(DecodingError::NoPath)
        ));
    }

    #[test]
    fn test_decode_skips_invalid_arc_targets() {
        let ctc = minimal_ctc::<LogWeight>(2);
        let decoder = CtcDecoder::new(ctc);

        let mut fst: VectorWfst<CtcLabel, LogWeight> = VectorWfst::new();
        let start = fst.add_state();
        let final_state = fst.add_state();
        fst.set_start(start);
        fst.set_final(final_state, LogWeight::one());
        fst.add_arc(start, Some(9), Some(9), 99, LogWeight::new(0.0));
        fst.add_arc(start, Some(1), Some(1), final_state, LogWeight::new(1.0));

        let greedy = decoder
            .greedy_decode(&fst)
            .expect("valid arc should remain decodable");
        assert_eq!(greedy.labels, vec![1]);
        assert_eq!(greedy.score, 1.0);

        let beam = decoder
            .beam_decode(&fst)
            .expect("valid arc should remain decodable");
        assert_eq!(beam.labels, vec![1]);
        assert_eq!(beam.score, 1.0);
    }

    #[test]
    fn test_greedy_decode_rejects_cyclic_backtrace() {
        let ctc = minimal_ctc::<LogWeight>(2);
        let decoder = CtcDecoder::new(ctc);

        let mut fst: VectorWfst<CtcLabel, LogWeight> = VectorWfst::new();
        let start = fst.add_state();
        let final_state = fst.add_state();
        fst.set_start(start);
        fst.set_final(final_state, LogWeight::one());
        fst.add_arc(start, Some(1), Some(1), final_state, LogWeight::new(1.0));
        fst.add_arc(
            final_state,
            Some(2),
            Some(2),
            final_state,
            LogWeight::new(-2.0),
        );

        assert!(matches!(
            decoder.greedy_decode(&fst),
            Err(DecodingError::NoPath)
        ));
    }

    #[test]
    fn test_decode_and_collapse() {
        let ctc = minimal_ctc::<LogWeight>(3);
        let decoder = CtcDecoder::new(ctc).with_config(CtcDecoderConfig::greedy());

        // Sequence with repeated labels: 1, 1, 2, 2, 1
        // Should collapse to: 1, 2, 1
        let posteriors = vec![
            vec![-2.0, -0.1, -2.0], // 1
            vec![-2.0, -0.1, -2.0], // 1
            vec![-2.0, -2.0, -0.1], // 2
            vec![-2.0, -2.0, -0.1], // 2
            vec![-2.0, -0.1, -2.0], // 1
        ];

        let result = decoder.decode_and_collapse(&posteriors);
        assert!(result.is_ok());
    }

    #[test]
    fn test_decoding_result_structure() {
        let result = DecodingResult {
            labels: vec![1, 2, 3],
            score: -5.0,
            am_score: -3.0,
            lm_score: -2.0,
            num_frames: 10,
            stats: DecodingStats::default(),
        };

        assert_eq!(result.labels.len(), 3);
        assert_eq!(result.num_frames, 10);
    }

    #[test]
    fn test_decoder_token_initial() {
        let token: DecoderToken<LogWeight> = DecoderToken::initial(0);

        assert_eq!(token.state, 0);
        assert_eq!(token.score, LogWeight::one());
        assert!(token.backpointer.is_none());
    }

    #[test]
    fn test_streaming_decoder_reset() {
        let ctc = compact_ctc::<LogWeight>(5);
        let decoder = CtcDecoder::new(ctc);
        let mut streaming = StreamingCtcDecoder::new(decoder);

        streaming.reset();

        assert!(!streaming.active_tokens.is_empty());
        assert_eq!(streaming.current_frame, 0);
    }

    #[test]
    fn test_streaming_decoder_uses_posterior_scores_for_recombination() {
        let ctc = minimal_ctc::<LogWeight>(3);
        let decoder = CtcDecoder::new(ctc);
        let mut streaming = StreamingCtcDecoder::new(decoder);

        streaming.reset();
        streaming.process_frame(&[-5.0, -0.1, -4.0]);

        let result = streaming.finalize();
        assert_eq!(result.labels, vec![1]);
    }

    #[test]
    fn test_streaming_decoder_blank_separates_repeated_labels() {
        let ctc = minimal_ctc::<LogWeight>(2);
        let decoder = CtcDecoder::new(ctc);
        let mut streaming = StreamingCtcDecoder::new(decoder);

        streaming.reset();
        streaming.process_frame(&[-2.0, -0.1]);
        streaming.process_frame(&[-0.1, -2.0]);
        streaming.process_frame(&[-2.0, -0.1]);

        let result = streaming.finalize();
        assert_eq!(result.labels, vec![1, 1]);
    }

    #[test]
    fn test_streaming_decoder_score_accumulates_posteriors() {
        let ctc = minimal_ctc::<LogWeight>(3);
        let decoder = CtcDecoder::new(ctc);
        let mut streaming = StreamingCtcDecoder::new(decoder);

        streaming.reset();
        streaming.process_frame(&[-3.0, -0.1, -3.0]);
        streaming.process_frame(&[-3.0, -3.0, -0.1]);

        let result = streaming.finalize();
        assert_eq!(result.labels, vec![1, 2]);
        assert!((result.score - 0.2).abs() < 1e-6);
    }

    #[test]
    fn test_streaming_decoder_compact_ctc_uses_epsilon_closure() {
        let ctc = compact_ctc::<LogWeight>(3);
        let decoder = CtcDecoder::new(ctc);
        let mut streaming = StreamingCtcDecoder::new(decoder);

        streaming.reset();
        streaming.process_frame(&[-3.0, -0.1, -3.0]);
        streaming.process_frame(&[-3.0, -3.0, -0.1]);

        let result = streaming.finalize();
        assert_eq!(result.labels, vec![1, 2]);
    }

    #[test]
    fn test_decoding_error_display() {
        let err = DecodingError::EmptyInput;
        assert_eq!(format!("{}", err), "Empty input posteriors");

        let err = DecodingError::VocabMismatch {
            posterior_vocab: 10,
            ctc_vocab: 5,
        };
        assert!(format!("{}", err).contains("mismatch"));

        let err = DecodingError::InvalidPosteriors(ObservationFstError::EmptyVocabulary);
        assert!(format!("{}", err).contains("Invalid posteriors"));

        let err = DecodingError::NoPath;
        assert!(format!("{}", err).contains("No valid path"));
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::super::topologies::{compact_ctc, minimal_ctc};
    use super::*;
    use proptest::prelude::*;

    // -------------------------------------------------------------------------
    // CtcDecoderConfig Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        #[test]
        fn decoder_config_beam_width_positive(width in 0.1f64..100.0) {
            let config = CtcDecoderConfig::beam(width);
            prop_assert!((config.beam_width - width).abs() < 1e-10);
        }

        #[test]
        fn decoder_config_default_values(_seed in any::<u64>()) {
            let config = CtcDecoderConfig::default();
            prop_assert!(config.beam_width > 0.0);
            prop_assert!(config.max_active > 0);
            prop_assert!(config.acoustic_scale > 0.0);
            prop_assert!(config.lm_scale > 0.0);
        }
    }

    // -------------------------------------------------------------------------
    // ObservationFst Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        #[test]
        fn obs_fst_correct_num_states(
            num_frames in 1usize..20,
            vocab_size in 2usize..10
        ) {
            let posteriors: Vec<Vec<f32>> = (0..num_frames)
                .map(|_| vec![-1.0; vocab_size])
                .collect();

            let obs_fst = ObservationFst::from_posteriors(&posteriors);

            prop_assert_eq!(obs_fst.fst.num_states(), num_frames + 1);
        }

        #[test]
        fn obs_fst_correct_num_arcs(
            num_frames in 1usize..10,
            vocab_size in 2usize..8
        ) {
            let posteriors: Vec<Vec<f32>> = (0..num_frames)
                .map(|_| vec![-1.0; vocab_size])
                .collect();

            let obs_fst = ObservationFst::from_posteriors(&posteriors);

            // Total arcs = num_frames * vocab_size
            let total_arcs: usize = (0..num_frames)
                .map(|s| obs_fst.fst.transitions(s as StateId).len())
                .sum();

            prop_assert_eq!(total_arcs, num_frames * vocab_size);
        }

        #[test]
        fn obs_fst_blank_always_epsilon(
            num_frames in 1usize..10,
            vocab_size in 2usize..8
        ) {
            let posteriors: Vec<Vec<f32>> = (0..num_frames)
                .map(|_| vec![-1.0; vocab_size])
                .collect();

            let obs_fst = ObservationFst::from_posteriors(&posteriors);

            // Check all frames
            for frame in 0..num_frames {
                let blank_arc = obs_fst.fst.transitions(frame as StateId)
                    .iter()
                    .find(|t| t.input == Some(BLANK));

                if let Some(arc) = blank_arc {
                    prop_assert_eq!(arc.output, None, "Blank should output epsilon");
                }
            }
        }

        #[test]
        fn obs_fst_linear_chain(
            num_frames in 1usize..15,
            vocab_size in 2usize..6
        ) {
            let posteriors: Vec<Vec<f32>> = (0..num_frames)
                .map(|_| vec![-1.0; vocab_size])
                .collect();

            let obs_fst = ObservationFst::from_posteriors(&posteriors);

            // All transitions from frame i should go to frame i+1
            for frame in 0..num_frames {
                let expected_to = (frame + 1) as StateId;
                for trans in obs_fst.fst.transitions(frame as StateId) {
                    prop_assert_eq!(trans.to, expected_to,
                        "Frame {} should transition to frame {}", frame, frame + 1);
                }
            }
        }

        #[test]
        fn obs_fst_start_and_final(
            num_frames in 1usize..20,
            vocab_size in 2usize..10
        ) {
            let posteriors: Vec<Vec<f32>> = (0..num_frames)
                .map(|_| vec![-1.0; vocab_size])
                .collect();

            let obs_fst = ObservationFst::from_posteriors(&posteriors);

            prop_assert_eq!(obs_fst.fst.start(), 0);
            prop_assert!(obs_fst.fst.is_final(num_frames as StateId));
        }
    }

    // -------------------------------------------------------------------------
    // CtcDecoder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        #[test]
        fn decoder_preserves_vocab_size(vocab_size in 2usize..50) {
            let ctc = compact_ctc::<LogWeight>(vocab_size);
            let decoder = CtcDecoder::new(ctc);

            prop_assert_eq!(decoder.ctc_topology().vocab_size(), vocab_size);
        }

        #[test]
        fn decoder_config_preserved(
            beam in 1.0f64..50.0,
            max_active in 100usize..10000
        ) {
            let ctc = minimal_ctc::<LogWeight>(5);
            let config = CtcDecoderConfig {
                beam_width: beam,
                max_active,
                ..Default::default()
            };

            let decoder = CtcDecoder::new(ctc).with_config(config);

            prop_assert!((decoder.config().beam_width - beam).abs() < 1e-10);
            prop_assert_eq!(decoder.config().max_active, max_active);
        }

        #[test]
        fn decode_rejects_wrong_vocab(
            ctc_vocab in 5usize..20,
            post_vocab in 21usize..40
        ) {
            let ctc = minimal_ctc::<LogWeight>(ctc_vocab);
            let decoder = CtcDecoder::new(ctc);

            let posteriors = vec![vec![-1.0; post_vocab]];
            let result = decoder.decode(&posteriors);

            let is_vocab_mismatch = matches!(result, Err(DecodingError::VocabMismatch { .. }));
            prop_assert!(is_vocab_mismatch, "Expected VocabMismatch error");
        }
    }

    // -------------------------------------------------------------------------
    // Token Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        #[test]
        fn token_initial_at_state(state in 0u32..100) {
            let token: DecoderToken<LogWeight> = DecoderToken::initial(state);
            prop_assert_eq!(token.state, state);
            prop_assert_eq!(token.score, LogWeight::one());
        }

        #[test]
        fn token_extend_updates_state(
            from in 0u32..50,
            to in 0u32..50,
            idx in 0usize..100
        ) {
            let token: DecoderToken<LogWeight> = DecoderToken::initial(from);
            let extended = token.extend(to, LogWeight::one(), Some(1), idx);

            prop_assert_eq!(extended.state, to);
            prop_assert_eq!(extended.backpointer, Some(idx));
        }
    }

    // -------------------------------------------------------------------------
    // DecodingResult Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        #[test]
        fn decoding_result_preserves_labels(
            labels in prop::collection::vec(0u32..100, 0..20)
        ) {
            let result = DecodingResult {
                labels: labels.clone(),
                score: 0.0,
                am_score: 0.0,
                lm_score: 0.0,
                num_frames: labels.len(),
                stats: DecodingStats::default(),
            };

            prop_assert_eq!(result.labels, labels);
        }

        #[test]
        fn decoding_result_score_components(
            am in -100.0f64..0.0,
            lm in -100.0f64..0.0
        ) {
            let result = DecodingResult {
                labels: vec![],
                score: am + lm,
                am_score: am,
                lm_score: lm,
                num_frames: 0,
                stats: DecodingStats::default(),
            };

            prop_assert!((result.score - (am + lm)).abs() < 1e-10);
        }
    }

    // -------------------------------------------------------------------------
    // Streaming Decoder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        #[test]
        fn streaming_reset_clears_state(vocab_size in 3usize..20) {
            let ctc = minimal_ctc::<LogWeight>(vocab_size);
            let decoder = CtcDecoder::new(ctc);
            let mut streaming = StreamingCtcDecoder::new(decoder);

            // Process some frames
            for _ in 0..5 {
                streaming.process_frame(&vec![-1.0; vocab_size]);
            }

            // Reset
            streaming.reset();

            prop_assert_eq!(streaming.current_frame, 0);
            prop_assert!(streaming.token_history.is_empty());
        }

        #[test]
        fn streaming_frame_count_increments(
            vocab_size in 3usize..10,
            num_frames in 1usize..20
        ) {
            let ctc = minimal_ctc::<LogWeight>(vocab_size);
            let decoder = CtcDecoder::new(ctc);
            let mut streaming = StreamingCtcDecoder::new(decoder);
            streaming.reset();

            for _ in 0..num_frames {
                streaming.process_frame(&vec![-1.0; vocab_size]);
            }

            prop_assert_eq!(streaming.current_frame, num_frames);
        }
    }
}
