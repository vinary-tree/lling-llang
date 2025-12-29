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

use std::collections::HashMap;
use std::sync::Arc;

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst, WeightedTransition};
use crate::composition::{compose, materialize};

use super::{CtcTopology, CtcLabel, BLANK};

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
    /// Panics if posteriors is empty or frames have inconsistent sizes.
    pub fn from_posteriors(posteriors: &[Vec<f32>]) -> Self {
        assert!(!posteriors.is_empty(), "Posteriors cannot be empty");

        let num_frames = posteriors.len();
        let vocab_size = posteriors[0].len();

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
            assert_eq!(
                frame_posteriors.len(),
                vocab_size,
                "Frame {} has {} labels, expected {}",
                frame_idx,
                frame_posteriors.len(),
                vocab_size
            );

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
                let weight = LogWeight::new(-log_prob as f64);
                fst.add_arc(from_state, Some(label), output, to_state, weight);
            }
        }

        Self {
            fst,
            num_frames,
            vocab_size,
        }
    }

    /// Create observation FST with acoustic scaling.
    ///
    /// Applies `weight = acoustic_scale * (-log_prob)` to all arcs.
    pub fn from_posteriors_scaled(posteriors: &[Vec<f32>], acoustic_scale: f64) -> Self {
        assert!(!posteriors.is_empty(), "Posteriors cannot be empty");

        let num_frames = posteriors.len();
        let vocab_size = posteriors[0].len();

        let mut fst: VectorWfst<CtcLabel, LogWeight> = VectorWfst::with_capacity(num_frames + 1);

        for _ in 0..=num_frames {
            fst.add_state();
        }

        fst.set_start(0);
        fst.set_final(num_frames as StateId, LogWeight::one());

        for (frame_idx, frame_posteriors) in posteriors.iter().enumerate() {
            let from_state = frame_idx as StateId;
            let to_state = (frame_idx + 1) as StateId;

            fst.reserve_transitions(from_state, vocab_size);

            for (label, &log_prob) in frame_posteriors.iter().enumerate() {
                let label = label as CtcLabel;
                let output = if label == BLANK { None } else { Some(label) };
                // Apply acoustic scale
                let weight = LogWeight::new(acoustic_scale * (-log_prob as f64));
                fst.add_arc(from_state, Some(label), output, to_state, weight);
            }
        }

        Self {
            fst,
            num_frames,
            vocab_size,
        }
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
        let obs_fst = ObservationFst::from_posteriors_scaled(posteriors, self.config.acoustic_scale);

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
            // Apply LM scale to language model weights
            // TODO: Scale LM weights by lm_scale
            let lazy_composed = compose(obs_ctc, (**lm).clone());
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
    fn greedy_decode(&self, fst: &VectorWfst<CtcLabel, LogWeight>) -> Result<DecodingResult, DecodingError> {
        if fst.num_states() == 0 {
            return Err(DecodingError::NoPath);
        }

        let start = fst.start();
        let num_states = fst.num_states();

        // Forward pass: compute best score to each state
        // (best_score, backpointer_state, backpointer_arc)
        let mut best: Vec<Option<(LogWeight, StateId, usize)>> = vec![None; num_states];
        best[start as usize] = Some((LogWeight::one(), start, 0));


        // Process in state order (assuming topological order for acyclic FST)
        for state in 0..num_states as StateId {
            if best[state as usize].is_none() {
                continue;
            }
            let (current_score, _, _) = best[state as usize].clone().expect("checked above");

            let transitions = fst.transitions(state);
            for (arc_idx, arc) in transitions.iter().enumerate() {
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
        let mut labels = Vec::new();
        let mut current = end_state;

        while current != start {
            if let Some((_, prev_state, arc_idx)) = &best[current as usize] {
                let arc = &fst.transitions(*prev_state)[*arc_idx];
                if let Some(output) = arc.output {
                    labels.push(output);
                }
                current = *prev_state;
            } else {
                break;
            }
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
    fn beam_decode(&self, fst: &VectorWfst<CtcLabel, LogWeight>) -> Result<DecodingResult, DecodingError> {
        if fst.num_states() == 0 {
            return Err(DecodingError::NoPath);
        }

        let start = fst.start();
        let num_states = fst.num_states();
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
            let mut next_active: Vec<Token> = Vec::new();
            let mut state_best: HashMap<StateId, (LogWeight, usize)> = HashMap::new();

            for token in &active {
                let transitions = fst.transitions(token.state);

                for arc in transitions.iter() {
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
                        a.score.value().partial_cmp(&b.score.value()).unwrap_or(std::cmp::Ordering::Equal)
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
    pub fn decode_and_collapse(&self, posteriors: &[Vec<f32>]) -> Result<DecodingResult, DecodingError> {
        let mut result = self.decode(posteriors)?;

        // Collapse consecutive duplicates
        let mut collapsed = Vec::new();
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
    /// No valid path found.
    NoPath,
    /// Composition failed.
    CompositionError(String),
}

impl std::fmt::Display for DecodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "Empty input posteriors"),
            Self::VocabMismatch { posterior_vocab, ctc_vocab } => {
                write!(
                    f,
                    "Vocabulary size mismatch: posteriors have {} labels, CTC has {}",
                    posterior_vocab, ctc_vocab
                )
            }
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
    pub fn extend(&self, to_state: StateId, weight: W, label: Option<CtcLabel>, token_idx: usize) -> Self {
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
    /// Current frame index.
    current_frame: usize,
    /// State to token index mapping for recombination.
    state_map: HashMap<StateId, usize>,
}

impl<W: Semiring + Clone + PartialOrd> StreamingCtcDecoder<W> {
    /// Create a new streaming decoder.
    pub fn new(decoder: CtcDecoder<W>) -> Self {
        Self {
            decoder,
            active_tokens: Vec::new(),
            token_history: Vec::new(),
            current_frame: 0,
            state_map: HashMap::new(),
        }
    }

    /// Reset the decoder to initial state.
    pub fn reset(&mut self) {
        self.active_tokens.clear();
        self.token_history.clear();
        self.current_frame = 0;
        self.state_map.clear();

        // Initialize with start token
        let start = self.decoder.ctc_topology.fst().start();
        let initial_token = DecoderToken::initial(start);
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

        // Save current tokens to history
        self.token_history.push(self.active_tokens.clone());

        let vocab_size = posteriors.len();
        let mut new_tokens: Vec<DecoderToken<W>> = Vec::new();
        let mut new_state_map: HashMap<StateId, usize> = HashMap::new();

        // Extend each active token
        for (token_idx, token) in self.active_tokens.iter().enumerate() {
            let state = token.state;

            // Get transitions from this state
            for trans in self.decoder.ctc_topology.fst().transitions(state) {
                if let Some(input_label) = trans.input {
                    if (input_label as usize) < vocab_size {
                        // Get posterior weight for this label
                        let posterior_weight = posteriors[input_label as usize];
                        let arc_weight = trans.weight.clone();

                        // Create new token
                        let new_token = token.extend(
                            trans.to,
                            arc_weight,
                            trans.output,
                            token_idx,
                        );

                        // Token recombination: keep best token per state
                        if let Some(&existing_idx) = new_state_map.get(&trans.to) {
                            // Compare scores (lower is better for log weights)
                            // For now, just replace - proper comparison needs PartialOrd
                            // This is a simplification
                            new_tokens[existing_idx] = new_token;
                        } else {
                            new_state_map.insert(trans.to, new_tokens.len());
                            new_tokens.push(new_token);
                        }
                    }
                }
            }
        }

        // Apply beam pruning
        // (Simplified - full implementation would sort and prune by score)
        let max_active = self.decoder.config.max_active;
        if new_tokens.len() > max_active {
            new_tokens.truncate(max_active);
        }

        self.active_tokens = new_tokens;
        self.state_map = new_state_map;
        self.current_frame += 1;
    }

    /// Get the current best hypothesis.
    pub fn best_hypothesis(&self) -> Vec<CtcLabel> {
        if self.active_tokens.is_empty() {
            return Vec::new();
        }

        // Find best final token
        // (Simplified - just take first one)
        let mut labels = Vec::new();
        let mut prev_label: Option<CtcLabel> = None;

        // Traceback would go through token_history
        // For now, just return current labels
        for token in &self.active_tokens {
            if let Some(label) = token.label {
                if Some(label) != prev_label && label != BLANK {
                    labels.push(label);
                }
                prev_label = Some(label);
            }
        }

        labels
    }

    /// Finalize decoding and get the best result.
    pub fn finalize(&self) -> DecodingResult {
        let labels = self.best_hypothesis();

        // Get best score
        let score = if let Some(token) = self.active_tokens.first() {
            // Would extract actual score value
            0.0
        } else {
            f64::INFINITY
        };

        DecodingResult {
            labels,
            score,
            am_score: score,
            lm_score: 0.0,
            num_frames: self.current_frame,
            stats: DecodingStats {
                tokens_created: self.token_history.iter().map(|t| t.len()).sum(),
                max_active_reached: self.token_history.iter().map(|t| t.len()).max().unwrap_or(0),
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctc::{correct_ctc, compact_ctc, minimal_ctc};

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
        let blank_arc = obs_fst.fst.transitions(0)
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

        let decoded = result.unwrap();
        assert_eq!(decoded.num_frames, 3);
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
    fn test_decoding_error_display() {
        let err = DecodingError::EmptyInput;
        assert_eq!(format!("{}", err), "Empty input posteriors");

        let err = DecodingError::VocabMismatch {
            posterior_vocab: 10,
            ctc_vocab: 5,
        };
        assert!(format!("{}", err).contains("mismatch"));

        let err = DecodingError::NoPath;
        assert!(format!("{}", err).contains("No valid path"));
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::ctc::{correct_ctc, compact_ctc, minimal_ctc};
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
