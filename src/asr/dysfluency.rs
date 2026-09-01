//! Zero-shot dysfluency detection using WFST composition.
//!
//! This module implements "Dysfluent WFST" for detecting speech dysfluencies
//! (stutters, repetitions, prolongations) without labeled training data.
//!
//! # Approach
//!
//! Zero-shot detection works by:
//! 1. Building pattern WFSTs that recognize specific dysfluency types
//! 2. Composing these patterns with ASR lattices
//! 3. Extracting dysfluency spans from successful alignments
//!
//! # Dysfluency Types
//!
//! - **Sound Repetition**: "b-b-b-book" - initial sound repeated
//! - **Syllable Repetition**: "ba-ba-basket" - initial syllable repeated
//! - **Word Repetition**: "I I I want" - whole word repeated
//! - **Block**: Silent pause within a word (broken phonation)
//! - **Prolongation**: "ssssnake" - sound stretched abnormally
//! - **Interjection**: "um", "uh", "er" - filler sounds
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::asr::dysfluency::{DysfluencyDetector, DysfluencyPattern};
//! use lling_llang::semiring::TropicalWeight;
//!
//! let detector = DysfluencyDetector::<TropicalWeight>::default();
//! let spans = detector.detect(&lattice);
//!
//! for span in spans {
//!     println!("{:?}: frames {}-{}", span.pattern, span.start_frame, span.end_frame);
//! }
//! ```
//!
//! # References
//!
//! - "Dysfluent WFST" (arXiv 2505.16351) - Zero-shot dysfluency detection

use std::collections::{HashMap, HashSet};

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};

/// Phone identifier for phoneme-level patterns.
pub type PhoneId = u32;

/// Frame index in the audio.
pub type FrameIndex = usize;

/// Types of dysfluency patterns that can be detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DysfluencyPattern {
    /// Sound repetition: "b-b-b-book"
    /// Initial phoneme is repeated 2+ times.
    SoundRepetition,

    /// Syllable repetition: "ba-ba-basket"
    /// Initial syllable is repeated 2+ times.
    SyllableRepetition,

    /// Word repetition: "I I I want"
    /// Whole word is repeated 2+ times.
    WordRepetition,

    /// Block/pause within word.
    /// Abnormal silence breaking phonation.
    Block,

    /// Prolongation: "ssssnake"
    /// Sound is stretched abnormally long.
    Prolongation,

    /// Interjection: "um", "uh", "er"
    /// Filler sounds between words.
    Interjection,
}

impl DysfluencyPattern {
    /// Get all pattern types.
    pub fn all() -> &'static [DysfluencyPattern] {
        &[
            DysfluencyPattern::SoundRepetition,
            DysfluencyPattern::SyllableRepetition,
            DysfluencyPattern::WordRepetition,
            DysfluencyPattern::Block,
            DysfluencyPattern::Prolongation,
            DysfluencyPattern::Interjection,
        ]
    }

    /// Human-readable name for the pattern.
    pub fn name(&self) -> &'static str {
        match self {
            DysfluencyPattern::SoundRepetition => "sound_repetition",
            DysfluencyPattern::SyllableRepetition => "syllable_repetition",
            DysfluencyPattern::WordRepetition => "word_repetition",
            DysfluencyPattern::Block => "block",
            DysfluencyPattern::Prolongation => "prolongation",
            DysfluencyPattern::Interjection => "interjection",
        }
    }
}

/// A detected dysfluency span in the audio.
#[derive(Debug, Clone)]
pub struct DysfluencySpan {
    /// Type of dysfluency detected.
    pub pattern: DysfluencyPattern,
    /// Start frame (inclusive).
    pub start_frame: FrameIndex,
    /// End frame (exclusive).
    pub end_frame: FrameIndex,
    /// Phone sequence involved in the dysfluency.
    pub phones: Vec<PhoneId>,
    /// Detection confidence (lower = more confident for tropical).
    pub score: f64,
    /// Number of repetitions (for repetition patterns).
    pub repetition_count: Option<usize>,
}

impl DysfluencySpan {
    /// Duration in frames.
    pub fn duration(&self) -> usize {
        self.end_frame.saturating_sub(self.start_frame)
    }
}

/// Configuration for dysfluency detection.
#[derive(Debug, Clone)]
pub struct DysfluencyConfig {
    /// Minimum repetitions to count as repetition dysfluency.
    pub min_repetitions: usize,
    /// Maximum repetitions to detect.
    pub max_repetitions: usize,
    /// Minimum frames for prolongation detection.
    pub min_prolongation_frames: usize,
    /// Cost penalty for detecting a dysfluency (higher = less sensitive).
    pub detection_penalty: f64,
    /// Cost for block/pause detection.
    pub block_cost: f64,
    /// Interjection phone IDs (e.g., for "um", "uh").
    pub interjection_phones: Vec<PhoneId>,
    /// Silence phone ID for block detection.
    pub silence_phone: PhoneId,
}

impl Default for DysfluencyConfig {
    fn default() -> Self {
        Self {
            min_repetitions: 2,
            max_repetitions: 5,
            min_prolongation_frames: 3,
            detection_penalty: 1.0,
            block_cost: 2.0,
            interjection_phones: vec![],
            silence_phone: 0,
        }
    }
}

/// Zero-shot dysfluency detector using WFST composition.
///
/// Detects dysfluencies by composing pattern WFSTs with input lattices.
#[derive(Debug)]
pub struct DysfluencyDetector<W: Semiring> {
    /// Pattern WFSTs for each dysfluency type.
    patterns: HashMap<DysfluencyPattern, VectorWfst<PhoneId, W>>,
    /// Configuration.
    config: DysfluencyConfig,
    /// Phone inventory size.
    vocab_size: usize,
}

struct PatternScan<'a, W: Semiring> {
    lattice: &'a VectorWfst<PhoneId, W>,
    pattern: &'a VectorWfst<PhoneId, W>,
    pattern_type: DysfluencyPattern,
}

struct PatternScanState {
    lattice_state: StateId,
    pattern_state: StateId,
    frame: FrameIndex,
    phones: Vec<PhoneId>,
}

#[derive(Clone, Copy)]
struct PatternScanCursor {
    lattice_state: StateId,
    pattern_state: StateId,
    frame: FrameIndex,
}

enum PatternScanWork {
    Enter {
        cursor: PatternScanCursor,
        phone: Option<PhoneId>,
    },
    Exit {
        frame: FrameIndex,
        pattern_state: StateId,
        phone_length: usize,
    },
}

impl<W: Semiring + From<f64> + Clone> DysfluencyDetector<W> {
    /// Create a new detector with given vocabulary size and config.
    pub fn new(vocab_size: usize, config: DysfluencyConfig) -> Self {
        let mut detector = Self {
            patterns: HashMap::new(),
            config,
            vocab_size,
        };
        detector.build_patterns();
        detector
    }

    /// Create with default configuration.
    pub fn with_vocab_size(vocab_size: usize) -> Self {
        Self::new(vocab_size, DysfluencyConfig::default())
    }

    /// Build all pattern WFSTs.
    fn build_patterns(&mut self) {
        self.patterns.insert(
            DysfluencyPattern::SoundRepetition,
            self.build_sound_repetition_pattern(),
        );
        self.patterns.insert(
            DysfluencyPattern::Prolongation,
            self.build_prolongation_pattern(),
        );
        self.patterns
            .insert(DysfluencyPattern::Block, self.build_block_pattern());
        self.patterns.insert(
            DysfluencyPattern::Interjection,
            self.build_interjection_pattern(),
        );
        // Syllable and word repetition require higher-level information
        // and are detected through composition with lexicon
    }

    /// Build pattern for sound repetition (phone repeated 2+ times).
    ///
    /// Pattern: p p+ where p is any phone (except silence).
    /// Accepts sequences like: a a, b b b, c c c c
    fn build_sound_repetition_pattern(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // States:
        // 0: Start
        // 1: Saw first phone
        // 2: Saw repetition (accepting)
        fst.add_states(3);
        fst.set_start(0);
        fst.set_final(2, W::one());

        let penalty = W::from(self.config.detection_penalty);

        // For each phone (except silence), create repetition pattern
        for phone in 1..self.vocab_size as PhoneId {
            if phone == self.config.silence_phone {
                continue;
            }

            // State 0 -> State 1: First occurrence of phone
            fst.add_transition(WeightedTransition {
                from: 0,
                input: Some(phone),
                output: Some(phone),
                to: 1,
                weight: W::one(),
            });

            // State 1 -> State 2: Second occurrence (repetition detected)
            fst.add_transition(WeightedTransition {
                from: 1,
                input: Some(phone),
                output: Some(phone),
                to: 2,
                weight: penalty.clone(),
            });

            // State 2 -> State 2: Additional repetitions
            fst.add_transition(WeightedTransition {
                from: 2,
                input: Some(phone),
                output: Some(phone),
                to: 2,
                weight: W::one(),
            });
        }

        // Allow transitions back to start for next potential repetition
        // State 2 -> State 0: End of repetition, look for next
        fst.add_transition(WeightedTransition {
            from: 2,
            input: None, // epsilon
            output: None,
            to: 0,
            weight: W::one(),
        });

        fst
    }

    /// Build pattern for prolongation (same phone for many frames).
    ///
    /// This is frame-level: detects when a phone spans more than
    /// `min_prolongation_frames` consecutive frames.
    fn build_prolongation_pattern(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // Create states for counting consecutive frames
        let num_states = self.config.min_prolongation_frames + 2;
        fst.add_states(num_states);
        fst.set_start(0);
        fst.set_final((num_states - 1) as StateId, W::one());

        let penalty = W::from(self.config.detection_penalty);

        for phone in 1..self.vocab_size as PhoneId {
            if phone == self.config.silence_phone {
                continue;
            }

            // Chain of states counting consecutive same-phone frames
            for i in 0..self.config.min_prolongation_frames {
                let from_state = i as StateId;
                let to_state = (i + 1) as StateId;

                fst.add_transition(WeightedTransition {
                    from: from_state,
                    input: Some(phone),
                    output: Some(phone),
                    to: to_state,
                    weight: W::one(),
                });
            }

            // Transition to accepting state with penalty
            let penultimate = self.config.min_prolongation_frames as StateId;
            let final_state = (num_states - 1) as StateId;

            fst.add_transition(WeightedTransition {
                from: penultimate,
                input: Some(phone),
                output: Some(phone),
                to: final_state,
                weight: penalty.clone(),
            });

            // Self-loop on accepting state
            fst.add_transition(WeightedTransition {
                from: final_state,
                input: Some(phone),
                output: Some(phone),
                to: final_state,
                weight: W::one(),
            });
        }

        fst
    }

    /// Build pattern for block (unexpected silence within word).
    ///
    /// Pattern: non-silence, silence+, non-silence
    fn build_block_pattern(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // States:
        // 0: Start (in word)
        // 1: Saw silence (potential block)
        // 2: Block detected (accepting)
        fst.add_states(3);
        fst.set_start(0);
        fst.set_final(2, W::one());

        let block_cost = W::from(self.config.block_cost);
        let silence = self.config.silence_phone;

        // Non-silence self-loop at start
        for phone in 1..self.vocab_size as PhoneId {
            if phone == silence {
                continue;
            }

            fst.add_transition(WeightedTransition {
                from: 0,
                input: Some(phone),
                output: Some(phone),
                to: 0,
                weight: W::one(),
            });
        }

        // Silence transition (entering potential block)
        fst.add_transition(WeightedTransition {
            from: 0,
            input: Some(silence),
            output: Some(silence),
            to: 1,
            weight: W::one(),
        });

        // More silence (staying in block)
        fst.add_transition(WeightedTransition {
            from: 1,
            input: Some(silence),
            output: Some(silence),
            to: 1,
            weight: W::one(),
        });

        // Exit block with non-silence (block confirmed)
        for phone in 1..self.vocab_size as PhoneId {
            if phone == silence {
                continue;
            }

            fst.add_transition(WeightedTransition {
                from: 1,
                input: Some(phone),
                output: Some(phone),
                to: 2,
                weight: block_cost.clone(),
            });
        }

        // Continue after block
        for phone in 1..self.vocab_size as PhoneId {
            fst.add_transition(WeightedTransition {
                from: 2,
                input: Some(phone),
                output: Some(phone),
                to: 2,
                weight: W::one(),
            });
        }

        fst
    }

    /// Build pattern for interjections ("um", "uh", etc.).
    fn build_interjection_pattern(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // Simple pattern: accept interjection phones with penalty
        fst.add_states(2);
        fst.set_start(0);
        fst.set_final(1, W::one());

        let penalty = W::from(self.config.detection_penalty);

        for &phone in &self.config.interjection_phones {
            fst.add_transition(WeightedTransition {
                from: 0,
                input: Some(phone),
                output: Some(phone),
                to: 1,
                weight: penalty.clone(),
            });

            // Self-loop for longer interjections
            fst.add_transition(WeightedTransition {
                from: 1,
                input: Some(phone),
                output: Some(phone),
                to: 1,
                weight: W::one(),
            });
        }

        fst
    }

    /// Detect dysfluencies in a phone-level lattice.
    ///
    /// Returns detected dysfluency spans with their locations and types.
    pub fn detect(&self, lattice: &VectorWfst<PhoneId, W>) -> Vec<DysfluencySpan>
    where
        W: Into<f64>,
    {
        let mut spans = Vec::new();

        for (&pattern_type, pattern_fst) in &self.patterns {
            let detected = self.detect_pattern(lattice, pattern_fst, pattern_type);
            spans.extend(detected);
        }

        // Sort by start frame
        spans.sort_by_key(|s| s.start_frame);

        spans
    }

    /// Detect a specific pattern in the lattice.
    fn detect_pattern(
        &self,
        lattice: &VectorWfst<PhoneId, W>,
        pattern: &VectorWfst<PhoneId, W>,
        pattern_type: DysfluencyPattern,
    ) -> Vec<DysfluencySpan>
    where
        W: Into<f64>,
    {
        let mut spans = Vec::new();

        let scan = PatternScan {
            lattice,
            pattern,
            pattern_type,
        };
        let initial_state = PatternScanState {
            lattice_state: lattice.start(),
            pattern_state: pattern.start(),
            frame: 0,
            phones: Vec::new(),
        };
        self.scan_for_pattern(&scan, initial_state, &mut spans);

        spans
    }

    /// Scan for pattern matches with an explicit depth-first machine.
    fn scan_for_pattern(
        &self,
        scan: &PatternScan<'_, W>,
        state: PatternScanState,
        spans: &mut Vec<DysfluencySpan>,
    ) where
        W: Into<f64>,
    {
        let PatternScanState {
            lattice_state,
            pattern_state,
            frame,
            mut phones,
        } = state;
        phones.reserve(20usize.saturating_sub(phones.len()));

        let pattern_state_count = scan.pattern.num_states();
        let mut work = Vec::with_capacity(pattern_state_count.saturating_add(1));
        let mut epsilon_on_path = HashSet::with_capacity(pattern_state_count);
        work.push(PatternScanWork::Enter {
            cursor: PatternScanCursor {
                lattice_state,
                pattern_state,
                frame,
            },
            phone: None,
        });

        while let Some(item) = work.pop() {
            let (cursor, phone) = match item {
                PatternScanWork::Exit {
                    frame,
                    pattern_state,
                    phone_length,
                } => {
                    let removed = epsilon_on_path.remove(&(frame, pattern_state));
                    debug_assert!(removed, "every entered scan state has one exit frame");
                    phones.truncate(phone_length);
                    continue;
                }
                PatternScanWork::Enter { cursor, phone } => (cursor, phone),
            };

            let phone_length = phones.len();
            if let Some(phone) = phone {
                phones.push(phone);
            }
            let path_key = (cursor.frame, cursor.pattern_state);
            if !epsilon_on_path.insert(path_key) {
                phones.truncate(phone_length);
                continue;
            }
            work.push(PatternScanWork::Exit {
                frame: cursor.frame,
                pattern_state: cursor.pattern_state,
                phone_length,
            });

            if scan.pattern.is_final(cursor.pattern_state) && !phones.is_empty() {
                let score: f64 = scan.pattern.final_weight(cursor.pattern_state).into();
                spans.push(DysfluencySpan {
                    pattern: scan.pattern_type,
                    start_frame: cursor.frame.saturating_sub(phones.len()),
                    end_frame: cursor.frame,
                    phones: phones.clone(),
                    score,
                    repetition_count: self.count_repetitions(&phones),
                });
            }
            if cursor.frame > 1000 {
                continue;
            }

            let pattern_transitions = scan.pattern.transitions(cursor.pattern_state);

            // Pattern-only epsilon successors follow every matching-pair
            // successor exactly once. Push them first in reverse order because
            // the explicit work tape is LIFO.
            for pattern_transition in pattern_transitions.iter().rev() {
                if pattern_transition.input.is_none() {
                    work.push(PatternScanWork::Enter {
                        cursor: PatternScanCursor {
                            lattice_state: cursor.lattice_state,
                            pattern_state: pattern_transition.to,
                            frame: cursor.frame,
                        },
                        phone: None,
                    });
                }
            }

            // Preserve the recursive oracle's lattice-major, pattern-minor
            // order by scheduling matching pairs in the reverse nesting order.
            for lattice_transition in scan.lattice.transitions(cursor.lattice_state).iter().rev() {
                for pattern_transition in pattern_transitions.iter().rev() {
                    let labels_match = match (lattice_transition.input, pattern_transition.input) {
                        (Some(left), Some(right)) => left == right,
                        (None, None) => true,
                        _ => false,
                    };
                    if !labels_match
                        || phones.len() + usize::from(lattice_transition.input.is_some()) > 20
                    {
                        continue;
                    }
                    work.push(PatternScanWork::Enter {
                        cursor: PatternScanCursor {
                            lattice_state: lattice_transition.to,
                            pattern_state: pattern_transition.to,
                            frame: cursor.frame + 1,
                        },
                        phone: lattice_transition.input,
                    });
                }
            }
        }
    }

    /// Count repetitions in a phone sequence.
    fn count_repetitions(&self, phones: &[PhoneId]) -> Option<usize> {
        if phones.is_empty() {
            return None;
        }

        let first = phones[0];
        let mut count = 0;
        for &p in phones {
            if p == first {
                count += 1;
            } else {
                break;
            }
        }

        if count >= 2 {
            Some(count)
        } else {
            None
        }
    }

    /// Get a specific pattern WFST.
    pub fn get_pattern(&self, pattern: DysfluencyPattern) -> Option<&VectorWfst<PhoneId, W>> {
        self.patterns.get(&pattern)
    }

    /// Get configuration.
    pub fn config(&self) -> &DysfluencyConfig {
        &self.config
    }
}

impl<W: Semiring + From<f64> + Clone> Default for DysfluencyDetector<W> {
    fn default() -> Self {
        Self::with_vocab_size(100) // Default phone vocabulary
    }
}

/// Builder for word-level repetition pattern.
///
/// This requires lexicon information to detect word boundaries.
#[derive(Debug)]
pub struct WordRepetitionBuilder<W: Semiring> {
    /// Word IDs to detect repetition of.
    words: Vec<u32>,
    /// Minimum repetitions.
    min_reps: usize,
    /// Maximum repetitions.
    max_reps: usize,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring + From<f64> + Clone> WordRepetitionBuilder<W> {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            words: Vec::new(),
            min_reps: 2,
            max_reps: 5,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a word to detect repetitions of.
    pub fn add_word(mut self, word_id: u32) -> Self {
        self.words.push(word_id);
        self
    }

    /// Set minimum repetitions.
    pub fn min_repetitions(mut self, min: usize) -> Self {
        self.min_reps = min;
        self
    }

    /// Set maximum repetitions.
    pub fn max_repetitions(mut self, max: usize) -> Self {
        self.max_reps = max;
        self
    }

    /// Build the word repetition pattern WFST.
    pub fn build(self) -> VectorWfst<u32, W> {
        let mut fst: VectorWfst<u32, W> = VectorWfst::new();

        // Create states for counting repetitions
        // 0: start, 1: saw once, 2: saw twice (accepting), ...
        let num_states = self.max_reps + 1;
        fst.add_states(num_states);
        fst.set_start(0);

        // Final states from min_reps onwards
        for i in self.min_reps..=self.max_reps {
            fst.set_final(i as StateId, W::one());
        }

        // For each word, create the repetition chain
        for &word in &self.words {
            for i in 0..self.max_reps {
                fst.add_transition(WeightedTransition {
                    from: i as StateId,
                    input: Some(word),
                    output: Some(word),
                    to: (i + 1) as StateId,
                    weight: W::one(),
                });
            }
        }

        fst
    }
}

impl<W: Semiring + From<f64> + Clone> Default for WordRepetitionBuilder<W> {
    fn default() -> Self {
        Self::new()
    }
}

/// Syllable repetition pattern builder.
///
/// Detects patterns like "ba-ba-basket" where an initial syllable is repeated.
#[derive(Debug)]
pub struct SyllableRepetitionBuilder<W: Semiring> {
    /// Syllable phone sequences to detect.
    syllables: Vec<Vec<PhoneId>>,
    /// Minimum repetitions.
    min_reps: usize,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring + From<f64> + Clone> SyllableRepetitionBuilder<W> {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            syllables: Vec::new(),
            min_reps: 2,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a syllable (as phone sequence) to detect.
    pub fn add_syllable(mut self, phones: Vec<PhoneId>) -> Self {
        self.syllables.push(phones);
        self
    }

    /// Set minimum repetitions.
    pub fn min_repetitions(mut self, min: usize) -> Self {
        self.min_reps = min;
        self
    }

    /// Build the syllable repetition pattern WFST.
    pub fn build(self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // For each syllable, create a pattern that accepts repetitions
        let mut state_counter: StateId = 0;

        fst.add_state(); // State 0: start
        fst.set_start(0);
        state_counter += 1;

        for syllable in &self.syllables {
            if syllable.is_empty() {
                continue;
            }

            // Create chain for first occurrence
            let first_start = state_counter;
            for &phone in syllable {
                fst.add_state();
                fst.add_transition(WeightedTransition {
                    from: if state_counter == first_start {
                        0
                    } else {
                        state_counter - 1
                    },
                    input: Some(phone),
                    output: Some(phone),
                    to: state_counter,
                    weight: W::one(),
                });
                state_counter += 1;
            }

            // Create chain for second occurrence (detecting repetition)
            let repeat_start = state_counter - 1;
            for (i, &phone) in syllable.iter().enumerate() {
                fst.add_state();
                let from = if i == 0 {
                    repeat_start
                } else {
                    state_counter - 1
                };
                fst.add_transition(WeightedTransition {
                    from,
                    input: Some(phone),
                    output: Some(phone),
                    to: state_counter,
                    weight: W::one(),
                });
                state_counter += 1;
            }

            // Mark final state
            fst.set_final(state_counter - 1, W::one());
        }

        fst
    }
}

impl<W: Semiring + From<f64> + Clone> Default for SyllableRepetitionBuilder<W> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;
    use proptest::prelude::*;

    const DEEP_EPSILON_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    #[derive(Clone, Debug)]
    struct ShallowGraph {
        states: usize,
        edges: Vec<(usize, Option<PhoneId>, usize)>,
        finals: Vec<bool>,
    }

    fn shallow_graph(acyclic: bool) -> BoxedStrategy<ShallowGraph> {
        (1usize..6)
            .prop_flat_map(move |states| {
                (
                    Just(states),
                    prop::collection::vec(
                        (0usize..states, prop::option::of(0u32..3), 0usize..states),
                        0..10,
                    ),
                    prop::collection::vec(any::<bool>(), states),
                )
            })
            .prop_map(move |(states, mut edges, finals)| {
                if acyclic {
                    edges.retain(|(from, _, to)| from < to);
                }
                ShallowGraph {
                    states,
                    edges,
                    finals,
                }
            })
            .boxed()
    }

    fn build_graph(spec: &ShallowGraph) -> VectorWfst<PhoneId, TropicalWeight> {
        let mut graph = VectorWfst::new();
        graph.add_states(spec.states);
        graph.set_start(0);
        for (state, is_final) in spec.finals.iter().copied().enumerate() {
            if is_final {
                graph.set_final(state as StateId, TropicalWeight::one());
            }
        }
        for &(from, label, to) in &spec.edges {
            graph.add_transition(WeightedTransition {
                from: from as StateId,
                input: label,
                output: label,
                to: to as StateId,
                weight: TropicalWeight::one(),
            });
        }
        graph
    }

    fn recursive_scan_reference(
        detector: &DysfluencyDetector<TropicalWeight>,
        scan: &PatternScan<'_, TropicalWeight>,
        state: PatternScanState,
        epsilon_path: &mut Vec<StateId>,
        spans: &mut Vec<DysfluencySpan>,
    ) {
        if scan.pattern.is_final(state.pattern_state) && !state.phones.is_empty() {
            let score: f64 = scan.pattern.final_weight(state.pattern_state).into();
            spans.push(DysfluencySpan {
                pattern: scan.pattern_type,
                start_frame: state.frame.saturating_sub(state.phones.len()),
                end_frame: state.frame,
                phones: state.phones.clone(),
                score,
                repetition_count: detector.count_repetitions(&state.phones),
            });
        }
        if state.frame > 1000 {
            return;
        }

        for lattice_transition in scan.lattice.transitions(state.lattice_state) {
            for pattern_transition in scan.pattern.transitions(state.pattern_state) {
                let labels_match = match (lattice_transition.input, pattern_transition.input) {
                    (Some(left), Some(right)) => left == right,
                    (None, None) => true,
                    _ => false,
                };
                if !labels_match {
                    continue;
                }

                let mut phones = state.phones.clone();
                if let Some(phone) = lattice_transition.input {
                    phones.push(phone);
                }
                if phones.len() <= 20 {
                    let mut child_epsilon_path = vec![pattern_transition.to];
                    recursive_scan_reference(
                        detector,
                        scan,
                        PatternScanState {
                            lattice_state: lattice_transition.to,
                            pattern_state: pattern_transition.to,
                            frame: state.frame + 1,
                            phones,
                        },
                        &mut child_epsilon_path,
                        spans,
                    );
                }
            }
        }

        for pattern_transition in scan.pattern.transitions(state.pattern_state) {
            if pattern_transition.input.is_some() || epsilon_path.contains(&pattern_transition.to) {
                continue;
            }
            epsilon_path.push(pattern_transition.to);
            recursive_scan_reference(
                detector,
                scan,
                PatternScanState {
                    lattice_state: state.lattice_state,
                    pattern_state: pattern_transition.to,
                    frame: state.frame,
                    phones: state.phones.clone(),
                },
                epsilon_path,
                spans,
            );
            epsilon_path.pop();
        }
    }

    fn recursive_detect_pattern(
        detector: &DysfluencyDetector<TropicalWeight>,
        lattice: &VectorWfst<PhoneId, TropicalWeight>,
        pattern: &VectorWfst<PhoneId, TropicalWeight>,
        pattern_type: DysfluencyPattern,
    ) -> Vec<DysfluencySpan> {
        let scan = PatternScan {
            lattice,
            pattern,
            pattern_type,
        };
        let initial = PatternScanState {
            lattice_state: lattice.start(),
            pattern_state: pattern.start(),
            frame: 0,
            phones: Vec::new(),
        };
        let mut epsilon_path = vec![pattern.start()];
        let mut spans = Vec::new();
        recursive_scan_reference(detector, &scan, initial, &mut epsilon_path, &mut spans);
        spans
    }

    fn span_signature(
        span: &DysfluencySpan,
    ) -> (
        DysfluencyPattern,
        FrameIndex,
        FrameIndex,
        Vec<PhoneId>,
        u64,
        Option<usize>,
    ) {
        (
            span.pattern,
            span.start_frame,
            span.end_frame,
            span.phones.clone(),
            span.score.to_bits(),
            span.repetition_count,
        )
    }

    #[test]
    fn test_dysfluency_pattern_all() {
        let patterns = DysfluencyPattern::all();
        assert_eq!(patterns.len(), 6);
    }

    #[test]
    fn test_dysfluency_detector_creation() {
        let detector = DysfluencyDetector::<TropicalWeight>::with_vocab_size(50);
        assert!(detector
            .get_pattern(DysfluencyPattern::SoundRepetition)
            .is_some());
        assert!(detector
            .get_pattern(DysfluencyPattern::Prolongation)
            .is_some());
        assert!(detector.get_pattern(DysfluencyPattern::Block).is_some());
    }

    #[test]
    fn test_sound_repetition_pattern() {
        let detector = DysfluencyDetector::<TropicalWeight>::with_vocab_size(10);
        let pattern = detector
            .get_pattern(DysfluencyPattern::SoundRepetition)
            .expect("asr/dysfluency.rs: required value was None/Err");

        // Pattern should have states for: start, first-phone, repetition-detected
        assert!(pattern.num_states() >= 3);
        assert_eq!(pattern.start(), 0);
    }

    #[test]
    fn test_prolongation_pattern() {
        let config = DysfluencyConfig {
            min_prolongation_frames: 3,
            ..Default::default()
        };
        let detector = DysfluencyDetector::<TropicalWeight>::new(10, config);
        let pattern = detector
            .get_pattern(DysfluencyPattern::Prolongation)
            .expect("asr/dysfluency.rs: required value was None/Err");

        // Should have states for counting frames
        assert!(pattern.num_states() >= 4);
    }

    #[test]
    fn test_word_repetition_builder() {
        let fst: VectorWfst<u32, TropicalWeight> = WordRepetitionBuilder::new()
            .add_word(1)
            .add_word(2)
            .min_repetitions(2)
            .max_repetitions(3)
            .build();

        // Should have 4 states (0, 1, 2, 3) for up to 3 repetitions
        assert_eq!(fst.num_states(), 4);
        // States 2 and 3 should be final
        assert!(fst.is_final(2));
        assert!(fst.is_final(3));
    }

    #[test]
    fn test_syllable_repetition_builder() {
        let fst: VectorWfst<PhoneId, TropicalWeight> = SyllableRepetitionBuilder::new()
            .add_syllable(vec![1, 2]) // "ba" as phones 1, 2
            .min_repetitions(2)
            .build();

        assert!(fst.num_states() > 0);
        assert_eq!(fst.start(), 0);
    }

    #[test]
    fn test_dysfluency_span() {
        let span = DysfluencySpan {
            pattern: DysfluencyPattern::SoundRepetition,
            start_frame: 10,
            end_frame: 15,
            phones: vec![1, 1, 1],
            score: 0.5,
            repetition_count: Some(3),
        };

        assert_eq!(span.duration(), 5);
        assert_eq!(span.repetition_count, Some(3));
    }

    #[test]
    fn test_config_default() {
        let config = DysfluencyConfig::default();
        assert_eq!(config.min_repetitions, 2);
        assert_eq!(config.max_repetitions, 5);
        assert_eq!(config.min_prolongation_frames, 3);
    }

    #[test]
    fn test_detect_empty_lattice() {
        let detector = DysfluencyDetector::<TropicalWeight>::with_vocab_size(10);
        let lattice: VectorWfst<PhoneId, TropicalWeight> = VectorWfst::new();

        let spans = detector.detect(&lattice);
        assert!(spans.is_empty());
    }

    #[test]
    fn pattern_epsilon_is_scheduled_once_independent_of_lattice_out_degree() {
        let detector = DysfluencyDetector::<TropicalWeight>::with_vocab_size(4);

        let mut lattice = VectorWfst::new();
        lattice.add_states(4);
        lattice.set_start(0);
        lattice.add_transition(WeightedTransition {
            from: 0,
            input: Some(1),
            output: Some(1),
            to: 1,
            weight: TropicalWeight::one(),
        });
        for to in [2, 3] {
            lattice.add_transition(WeightedTransition {
                from: 1,
                input: Some(2),
                output: Some(2),
                to,
                weight: TropicalWeight::one(),
            });
        }

        let mut pattern = VectorWfst::new();
        pattern.add_states(3);
        pattern.set_start(0);
        pattern.set_final(2, TropicalWeight::one());
        pattern.add_transition(WeightedTransition {
            from: 0,
            input: Some(1),
            output: Some(1),
            to: 1,
            weight: TropicalWeight::one(),
        });
        pattern.add_transition(WeightedTransition::epsilon(1, 2, TropicalWeight::one()));

        let spans = detector.detect_pattern(&lattice, &pattern, DysfluencyPattern::SoundRepetition);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].phones, vec![1]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn shallow_scan_matches_cycle_safe_recursive_reference(
            lattice_spec in shallow_graph(true),
            pattern_spec in shallow_graph(false),
        ) {
            let detector = DysfluencyDetector::<TropicalWeight>::with_vocab_size(4);
            let lattice = build_graph(&lattice_spec);
            let pattern = build_graph(&pattern_spec);
            let expected = recursive_detect_pattern(
                &detector,
                &lattice,
                &pattern,
                DysfluencyPattern::SoundRepetition,
            );
            let actual = detector.detect_pattern(
                &lattice,
                &pattern,
                DysfluencyPattern::SoundRepetition,
            );
            prop_assert_eq!(
                actual.iter().map(span_signature).collect::<Vec<_>>(),
                expected.iter().map(span_signature).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn deep_pattern_epsilon_scan_uses_constant_native_stack() {
        std::thread::Builder::new()
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let detector = DysfluencyDetector::<TropicalWeight>::with_vocab_size(4);

                let mut lattice = VectorWfst::new();
                lattice.add_states(2);
                lattice.set_start(0);
                lattice.add_transition(WeightedTransition {
                    from: 0,
                    input: Some(1),
                    output: Some(1),
                    to: 1,
                    weight: TropicalWeight::one(),
                });

                let mut pattern = VectorWfst::new();
                pattern.add_states(DEEP_EPSILON_DEPTH + 1);
                pattern.set_start(0);
                for state in 0..DEEP_EPSILON_DEPTH {
                    pattern.add_transition(WeightedTransition::epsilon(
                        state as StateId,
                        (state + 1) as StateId,
                        TropicalWeight::one(),
                    ));
                }

                assert!(detector
                    .detect_pattern(&lattice, &pattern, DysfluencyPattern::SoundRepetition,)
                    .is_empty());
            })
            .expect("small-stack worker must spawn")
            .join()
            .expect("dysfluency epsilon scan must not overflow the native stack");
    }
}
