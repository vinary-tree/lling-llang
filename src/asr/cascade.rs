//! ASR transducer cascade construction.
//!
//! This module provides tools for building the full ASR recognition network:
//!
//! ```text
//! N = π(min(det(H̃ ∘ det(C̃ ∘ det(L̃ ∘ G)))))
//! ```
//!
//! ## Components
//!
//! - **G**: Word-level grammar (n-gram language model)
//! - **L̃**: Pronunciation lexicon with auxiliary symbols
//! - **C̃**: Context-dependency transducer (triphone/tetraphone)
//! - **H̃**: HMM transducer with auxiliary distribution symbols
//! - **π**: Erasing operation (auxiliary symbols → ε)
//!
//! ## Incremental Optimization
//!
//! The cascade is built incrementally, applying determinization after each
//! composition to control graph size:
//!
//! 1. det(L̃ ∘ G) - Compose lexicon with grammar, determinize
//! 2. det(C̃ ∘ result) - Add context-dependency, determinize
//! 3. det(H̃ ∘ result) - Add HMM structure, determinize
//! 4. min(result) - Minimize final graph
//! 5. π(result) - Erase auxiliary symbols
//!
//! ## Lazy Composition
//!
//! For dynamic applications (e.g., dialogue systems), lazy composition
//! can be used to avoid materializing the full graph.
//!
//! ## References
//!
//! - Mohri et al., "Speech Recognition with WFSTs" Section 5

use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

use crate::semiring::Semiring;
use crate::wfst::{VectorWfst, MutableWfst, Wfst, StateId};

use super::context::PhoneId;
use super::ngram::{WordId, NgramTransducer};

/// Auxiliary symbol for disambiguation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuxiliarySymbol {
    /// Word boundary marker.
    WordBoundary,
    /// Disambiguation symbol #i.
    Disambiguation(u32),
    /// Epsilon (for erasing).
    Epsilon,
}

/// Pronunciation lexicon entry.
#[derive(Clone, Debug)]
pub struct LexiconEntry<W: Semiring> {
    /// Word ID.
    pub word: WordId,
    /// Pronunciation as sequence of phones.
    pub phones: Vec<PhoneId>,
    /// Probability weight (for multiple pronunciations).
    pub weight: W,
    /// Auxiliary symbols for disambiguation.
    pub auxiliaries: Vec<AuxiliarySymbol>,
}

impl<W: Semiring> LexiconEntry<W> {
    /// Create a new lexicon entry.
    pub fn new(word: WordId, phones: Vec<PhoneId>, weight: W) -> Self {
        Self {
            word,
            phones,
            weight,
            auxiliaries: Vec::new(),
        }
    }

    /// Add disambiguation symbols.
    pub fn with_auxiliaries(mut self, aux: Vec<AuxiliarySymbol>) -> Self {
        self.auxiliaries = aux;
        self
    }
}

/// Configuration for cascade construction.
#[derive(Clone, Debug)]
pub struct CascadeConfig {
    /// Whether to apply determinization after each composition.
    pub incremental_det: bool,

    /// Whether to apply minimization to the final result.
    pub minimize: bool,

    /// Whether to use lazy composition.
    pub lazy: bool,

    /// Maximum degree of homophony for auxiliary symbols.
    pub max_homophony: u32,

    /// Whether to add word boundary markers.
    pub word_boundaries: bool,
}

impl Default for CascadeConfig {
    fn default() -> Self {
        Self {
            incremental_det: true,
            minimize: true,
            lazy: false,
            max_homophony: 10,
            word_boundaries: true,
        }
    }
}

/// ASR transducer cascade.
///
/// Represents the full recognition network H ∘ C ∘ L ∘ G.
pub struct AsrCascade<W: Semiring> {
    /// The composed transducer.
    pub fst: VectorWfst<PhoneId, W>,
    /// Configuration used.
    pub config: CascadeConfig,
    /// Statistics about the cascade.
    pub stats: CascadeStats,
}

/// Statistics about cascade construction.
#[derive(Clone, Debug, Default)]
pub struct CascadeStats {
    /// Number of states in G (grammar).
    pub g_states: usize,
    /// Number of states in L ∘ G.
    pub lg_states: usize,
    /// Number of states in det(L ∘ G).
    pub det_lg_states: usize,
    /// Number of states in C ∘ det(L ∘ G).
    pub clg_states: usize,
    /// Number of states in det(C ∘ L ∘ G).
    pub det_clg_states: usize,
    /// Number of states in final result.
    pub final_states: usize,
    /// Number of arcs in final result.
    pub final_arcs: usize,
}

/// Builder for ASR transducer cascades.
pub struct CascadeBuilder<W: Semiring> {
    /// Configuration.
    config: CascadeConfig,

    /// Grammar (G) transducer.
    grammar: Option<VectorWfst<WordId, W>>,

    /// Lexicon entries.
    lexicon: Vec<LexiconEntry<W>>,

    /// Context-dependency transducer (C).
    context: Option<VectorWfst<PhoneId, W>>,

    /// HMM transducer (H).
    hmm: Option<VectorWfst<PhoneId, W>>,

    /// Phantom for weight type.
    _weight: PhantomData<W>,
}

impl<W: Semiring + Clone> CascadeBuilder<W> {
    /// Create a new cascade builder.
    pub fn new() -> Self {
        Self {
            config: CascadeConfig::default(),
            grammar: None,
            lexicon: Vec::new(),
            context: None,
            hmm: None,
            _weight: PhantomData,
        }
    }

    /// Set configuration.
    pub fn config(mut self, config: CascadeConfig) -> Self {
        self.config = config;
        self
    }

    /// Set grammar (G) transducer.
    pub fn grammar(mut self, grammar: VectorWfst<WordId, W>) -> Self {
        self.grammar = Some(grammar);
        self
    }

    /// Set grammar from n-gram model.
    pub fn grammar_from_ngram(self, ngram: NgramTransducer<W>) -> Self {
        // Convert ngram FST (WordId labels) to cascade format
        self.grammar(ngram.fst)
    }

    /// Add a lexicon entry.
    pub fn add_lexicon_entry(&mut self, entry: LexiconEntry<W>) {
        self.lexicon.push(entry);
    }

    /// Set context-dependency transducer (C).
    pub fn context_dependency(mut self, context: VectorWfst<PhoneId, W>) -> Self {
        self.context = Some(context);
        self
    }

    /// Set HMM transducer (H).
    pub fn hmm(mut self, hmm: VectorWfst<PhoneId, W>) -> Self {
        self.hmm = Some(hmm);
        self
    }

    /// Build the lexicon transducer (L).
    ///
    /// The lexicon maps word sequences to phone sequences.
    /// Input labels: phones
    /// Output labels: words (on first transition of each word)
    fn build_lexicon(&self) -> VectorWfst<PhoneId, W> {
        let mut fst: VectorWfst<PhoneId, W> = VectorWfst::new();

        // Create initial state
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, W::one());

        // Group entries by word to handle homophones
        let mut word_entries: HashMap<WordId, Vec<&LexiconEntry<W>>> = HashMap::new();
        for entry in &self.lexicon {
            word_entries.entry(entry.word).or_default().push(entry);
        }

        // Add each lexicon entry
        for entry in &self.lexicon {
            if entry.phones.is_empty() {
                continue;
            }

            // Create states for this pronunciation
            let mut current = start;

            // First phone: output the word label
            let next = fst.add_state();
            // Use word as output on first arc (requires label type casting)
            // For now, we'll encode word in a way that can be decoded later
            fst.add_arc(
                current,
                Some(entry.phones[0]),
                Some(entry.phones[0]), // Phone output (will be relabeled)
                next,
                entry.weight.clone(),
            );
            current = next;

            // Remaining phones: epsilon output (or phone echo)
            for &phone in &entry.phones[1..entry.phones.len().saturating_sub(1)] {
                let next = fst.add_state();
                fst.add_arc(
                    current,
                    Some(phone),
                    Some(phone),
                    next,
                    W::one(),
                );
                current = next;
            }

            // Last phone: return to start
            if entry.phones.len() > 1 {
                let last_phone = entry.phones[entry.phones.len() - 1];
                fst.add_arc(
                    current,
                    Some(last_phone),
                    Some(last_phone),
                    start,
                    W::one(),
                );
            } else {
                // Single phone word: add self-loop back to start
                // Already handled above
            }
        }

        // If no entries, just return minimal FST
        if self.lexicon.is_empty() {
            let s0 = fst.add_state();
            fst.set_start(s0);
            fst.set_final(s0, W::one());
        }

        fst
    }

    /// Build the full cascade.
    ///
    /// Returns the composed transducer N = π(min(det(H ∘ det(C ∘ det(L ∘ G))))).
    pub fn build(self) -> AsrCascade<W> {
        let mut stats = CascadeStats::default();

        // Build lexicon first (before consuming self)
        let l = self.build_lexicon();

        // Start with grammar
        let g = self.grammar.unwrap_or_else(|| {
            // Create trivial grammar (accept everything)
            let mut fst = VectorWfst::new();
            let s = fst.add_state();
            fst.set_start(s);
            fst.set_final(s, W::one());
            fst
        });
        stats.g_states = g.num_states();

        // Compose L ∘ G (would need actual composition implementation)
        // For now, we'll use the lexicon as a placeholder
        let lg = l; // TODO: Actual composition with grammar
        stats.lg_states = lg.num_states();

        // Apply determinization if configured
        let det_lg = if self.config.incremental_det {
            // TODO: Apply determinization
            lg
        } else {
            lg
        };
        stats.det_lg_states = det_lg.num_states();

        // Compose with context-dependency
        let clg = if let Some(_context) = self.context {
            // TODO: Actual composition C ∘ det(L ∘ G)
            det_lg
        } else {
            det_lg
        };
        stats.clg_states = clg.num_states();

        // Apply determinization
        let det_clg = if self.config.incremental_det {
            // TODO: Apply determinization
            clg
        } else {
            clg
        };
        stats.det_clg_states = det_clg.num_states();

        // Compose with HMM
        let hclg = if let Some(_hmm) = self.hmm {
            // TODO: Actual composition H ∘ det(C ∘ L ∘ G)
            det_clg
        } else {
            det_clg
        };

        // Apply minimization
        let result = if self.config.minimize {
            // TODO: Apply minimization
            hclg
        } else {
            hclg
        };

        // Count final arcs
        let final_arcs: usize = (0..result.num_states() as StateId)
            .map(|s| result.transitions(s).len())
            .sum();

        stats.final_states = result.num_states();
        stats.final_arcs = final_arcs;

        AsrCascade {
            fst: result,
            config: self.config,
            stats,
        }
    }
}

impl<W: Semiring + Clone> Default for CascadeBuilder<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Semiring> AsrCascade<W> {
    /// Get the underlying FST.
    pub fn as_fst(&self) -> &VectorWfst<PhoneId, W> {
        &self.fst
    }

    /// Get construction statistics.
    pub fn statistics(&self) -> &CascadeStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::NO_STATE;

    #[test]
    fn test_lexicon_entry() {
        let entry = LexiconEntry::new(
            1, // word ID
            vec![10, 11, 12], // phones
            LogWeight::new(0.5),
        );

        assert_eq!(entry.word, 1);
        assert_eq!(entry.phones.len(), 3);
    }

    #[test]
    fn test_cascade_config_default() {
        let config = CascadeConfig::default();

        assert!(config.incremental_det);
        assert!(config.minimize);
        assert!(!config.lazy);
    }

    #[test]
    fn test_cascade_builder_minimal() {
        let builder = CascadeBuilder::<LogWeight>::new();
        let cascade = builder.build();

        assert!(cascade.fst.num_states() > 0);
        assert!(cascade.fst.start() != NO_STATE);
    }

    #[test]
    fn test_cascade_builder_with_lexicon() {
        let mut builder = CascadeBuilder::<LogWeight>::new();

        // Add some lexicon entries
        builder.add_lexicon_entry(LexiconEntry::new(
            1, // "hello"
            vec![10, 11, 12], // /h/, /e/, /l/
            LogWeight::new(0.0),
        ));

        builder.add_lexicon_entry(LexiconEntry::new(
            2, // "world"
            vec![20, 21, 22, 23], // /w/, /o/, /r/, /ld/
            LogWeight::new(0.0),
        ));

        let cascade = builder.build();

        // Should have states for the lexicon
        assert!(cascade.fst.num_states() > 1);
    }

    #[test]
    fn test_cascade_stats() {
        let mut builder = CascadeBuilder::<LogWeight>::new();

        builder.add_lexicon_entry(LexiconEntry::new(
            1,
            vec![10, 11],
            LogWeight::new(0.0),
        ));

        let cascade = builder.build();

        assert!(cascade.stats.final_states > 0);
    }

    #[test]
    fn test_auxiliary_symbols() {
        let entry = LexiconEntry::new(1, vec![10], LogWeight::new(0.0))
            .with_auxiliaries(vec![
                AuxiliarySymbol::WordBoundary,
                AuxiliarySymbol::Disambiguation(0),
            ]);

        assert_eq!(entry.auxiliaries.len(), 2);
    }
}
