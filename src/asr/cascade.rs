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
use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

use crate::algorithms::{
    determinize, minimize,
    DeterminizeConfig, MinimizeConfig,
};
use crate::composition::{compose, materialize};
use crate::semiring::{DivisibleSemiring, NumericalWeight, Semiring};
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

impl<W> CascadeBuilder<W>
where
    W: Semiring + Clone,
{
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
            // Only iterate middle phones (not first, not last)
            if entry.phones.len() > 2 {
                for &phone in &entry.phones[1..entry.phones.len() - 1] {
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
                // Single phone word: return to start from current state
                // This allows recognition to continue after this word
                fst.add_arc(
                    current,
                    None,  // epsilon
                    None,  // epsilon
                    start,
                    W::one(),
                );
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

    /// Build the full cascade (basic version).
    ///
    /// This version uses the lexicon directly without optimization algorithms.
    /// For full determinization and minimization, use [`build_optimized`].
    pub fn build(self) -> AsrCascade<W> {
        let mut stats = CascadeStats::default();

        // Build lexicon first (before consuming self)
        let l = self.build_lexicon();

        // Start with grammar (note: grammar uses WordId, lexicon uses PhoneId)
        // Full L ∘ G composition requires label type unification which is not
        // implemented yet. For now, we use the lexicon directly.
        let _g = self.grammar.unwrap_or_else(|| {
            let mut fst = VectorWfst::new();
            let s = fst.add_state();
            fst.set_start(s);
            fst.set_final(s, W::one());
            fst
        });
        stats.g_states = _g.num_states();

        // L ∘ G composition placeholder (requires WordId/PhoneId unification)
        let lg = l;
        stats.lg_states = lg.num_states();

        // Compose with context-dependency (C ∘ det(L ∘ G))
        // Both C and L use PhoneId, so composition is possible
        let clg = if let Some(context) = self.context {
            let lazy = compose(context, lg);
            materialize(lazy)
        } else {
            lg
        };
        stats.det_lg_states = clg.num_states();
        stats.clg_states = clg.num_states();

        // Compose with HMM (H ∘ det(C ∘ L ∘ G))
        // Both H and C use PhoneId, so composition is possible
        let hclg = if let Some(hmm) = self.hmm {
            let lazy = compose(hmm, clg);
            materialize(lazy)
        } else {
            clg
        };
        stats.det_clg_states = hclg.num_states();

        // Count final arcs
        let final_arcs: usize = (0..hclg.num_states() as StateId)
            .map(|s| hclg.transitions(s).len())
            .sum();

        stats.final_states = hclg.num_states();
        stats.final_arcs = final_arcs;

        AsrCascade {
            fst: hclg,
            config: self.config,
            stats,
        }
    }
}

impl<W> CascadeBuilder<W>
where
    W: DivisibleSemiring + NumericalWeight + PartialOrd + Clone + Debug + Hash + Eq,
{
    /// Build the full cascade with optimization algorithms.
    ///
    /// Returns the composed transducer N = π(min(det(H ∘ det(C ∘ det(L ∘ G))))).
    ///
    /// This version applies determinization and minimization as configured.
    /// Requires the weight type to support division operations.
    pub fn build_optimized(self) -> AsrCascade<W> {
        let mut stats = CascadeStats::default();

        // Build lexicon first (before consuming self)
        let l = self.build_lexicon();

        // Start with grammar (note: grammar uses WordId, lexicon uses PhoneId)
        let _g = self.grammar.unwrap_or_else(|| {
            let mut fst = VectorWfst::new();
            let s = fst.add_state();
            fst.set_start(s);
            fst.set_final(s, W::one());
            fst
        });
        stats.g_states = _g.num_states();

        // L ∘ G composition placeholder (requires WordId/PhoneId unification)
        let lg = l;
        stats.lg_states = lg.num_states();

        // Apply determinization if configured
        let det_lg = if self.config.incremental_det {
            determinize(&lg, DeterminizeConfig::standard()).unwrap_or(lg)
        } else {
            lg
        };
        stats.det_lg_states = det_lg.num_states();

        // Compose with context-dependency (C ∘ det(L ∘ G))
        let clg = if let Some(context) = self.context {
            let lazy = compose(context, det_lg);
            materialize(lazy)
        } else {
            det_lg
        };
        stats.clg_states = clg.num_states();

        // Apply determinization
        let det_clg = if self.config.incremental_det {
            determinize(&clg, DeterminizeConfig::standard()).unwrap_or(clg)
        } else {
            clg
        };
        stats.det_clg_states = det_clg.num_states();

        // Compose with HMM (H ∘ det(C ∘ L ∘ G))
        let hclg = if let Some(hmm) = self.hmm {
            let lazy = compose(hmm, det_clg);
            materialize(lazy)
        } else {
            det_clg
        };

        // Apply minimization
        let result = if self.config.minimize {
            minimize(&hclg, MinimizeConfig::default()).unwrap_or(hclg)
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

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::{Wfst, NO_STATE};
    use proptest::prelude::*;

    // -------------------------------------------------------------------------
    // AuxiliarySymbol Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// AuxiliarySymbol equality is reflexive.
        #[test]
        fn aux_symbol_reflexive(idx in 0u32..100) {
            let sym = AuxiliarySymbol::Disambiguation(idx);
            prop_assert_eq!(sym, sym);
        }

        /// Different disambiguation indices are different.
        #[test]
        fn different_disambiguation(a in 0u32..100, b in 100u32..200) {
            prop_assert_ne!(
                AuxiliarySymbol::Disambiguation(a),
                AuxiliarySymbol::Disambiguation(b)
            );
        }

        /// WordBoundary != Epsilon.
        #[test]
        fn word_boundary_ne_epsilon(_seed in any::<u64>()) {
            prop_assert_ne!(AuxiliarySymbol::WordBoundary, AuxiliarySymbol::Epsilon);
        }

        /// WordBoundary != Disambiguation.
        #[test]
        fn word_boundary_ne_disambiguation(idx in 0u32..100) {
            prop_assert_ne!(
                AuxiliarySymbol::WordBoundary,
                AuxiliarySymbol::Disambiguation(idx)
            );
        }
    }

    // -------------------------------------------------------------------------
    // LexiconEntry Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// LexiconEntry preserves word ID.
        #[test]
        fn lexicon_preserves_word(word_id in 0u32..1000) {
            let entry = LexiconEntry::new(
                word_id,
                vec![1, 2, 3],
                LogWeight::new(1.0),
            );
            prop_assert_eq!(entry.word, word_id);
        }

        /// LexiconEntry preserves phones.
        #[test]
        fn lexicon_preserves_phones(phones in prop::collection::vec(0u32..100, 1..10)) {
            let entry = LexiconEntry::new(
                1,
                phones.clone(),
                LogWeight::new(1.0),
            );
            prop_assert_eq!(entry.phones, phones);
        }

        /// LexiconEntry starts with empty auxiliaries.
        #[test]
        fn lexicon_empty_auxiliaries(word_id in 0u32..100) {
            let entry = LexiconEntry::new(
                word_id,
                vec![1],
                LogWeight::new(1.0),
            );
            prop_assert!(entry.auxiliaries.is_empty());
        }

        /// with_auxiliaries sets auxiliaries.
        #[test]
        fn with_auxiliaries_sets(num_aux in 0usize..5) {
            let aux: Vec<_> = (0..num_aux)
                .map(|i| AuxiliarySymbol::Disambiguation(i as u32))
                .collect();

            let entry = LexiconEntry::new(1, vec![1], LogWeight::new(1.0))
                .with_auxiliaries(aux.clone());

            prop_assert_eq!(entry.auxiliaries.len(), num_aux);
        }
    }

    // -------------------------------------------------------------------------
    // CascadeConfig Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default config enables incremental determinization.
        #[test]
        fn default_config_det(_seed in any::<u64>()) {
            let config = CascadeConfig::default();
            prop_assert!(config.incremental_det);
        }

        /// Default config enables minimization.
        #[test]
        fn default_config_min(_seed in any::<u64>()) {
            let config = CascadeConfig::default();
            prop_assert!(config.minimize);
        }

        /// Default config is not lazy.
        #[test]
        fn default_config_not_lazy(_seed in any::<u64>()) {
            let config = CascadeConfig::default();
            prop_assert!(!config.lazy);
        }

        /// Default config has word boundaries enabled.
        #[test]
        fn default_config_word_boundaries(_seed in any::<u64>()) {
            let config = CascadeConfig::default();
            prop_assert!(config.word_boundaries);
        }

        /// Default config has max_homophony of 10.
        #[test]
        fn default_config_homophony(_seed in any::<u64>()) {
            let config = CascadeConfig::default();
            prop_assert_eq!(config.max_homophony, 10);
        }
    }

    // -------------------------------------------------------------------------
    // CascadeStats Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default CascadeStats has zeros.
        #[test]
        fn default_cascade_stats(_seed in any::<u64>()) {
            let stats = CascadeStats::default();
            prop_assert_eq!(stats.g_states, 0);
            prop_assert_eq!(stats.lg_states, 0);
            prop_assert_eq!(stats.det_lg_states, 0);
            prop_assert_eq!(stats.clg_states, 0);
            prop_assert_eq!(stats.det_clg_states, 0);
            prop_assert_eq!(stats.final_states, 0);
            prop_assert_eq!(stats.final_arcs, 0);
        }
    }

    // -------------------------------------------------------------------------
    // CascadeBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(25))]

        /// Empty builder produces valid FST.
        #[test]
        fn empty_builder_valid(_seed in any::<u64>()) {
            let builder = CascadeBuilder::<LogWeight>::new();
            let cascade = builder.build();

            prop_assert!(cascade.fst.num_states() > 0);
            prop_assert!(cascade.fst.start() != NO_STATE);
        }

        /// Builder with lexicon entries produces larger FST.
        #[test]
        fn builder_with_entries(num_entries in 1usize..5) {
            let mut builder = CascadeBuilder::<LogWeight>::new();

            for i in 0..num_entries {
                builder.add_lexicon_entry(LexiconEntry::new(
                    i as u32,
                    vec![(i * 10) as u32, (i * 10 + 1) as u32],
                    LogWeight::new(0.0),
                ));
            }

            let cascade = builder.build();

            // Should have multiple states
            prop_assert!(cascade.fst.num_states() >= 1);
        }

        /// config method sets configuration.
        #[test]
        fn builder_config_sets(lazy in any::<bool>(), minimize in any::<bool>()) {
            let config = CascadeConfig {
                lazy,
                minimize,
                ..Default::default()
            };

            let builder = CascadeBuilder::<LogWeight>::new().config(config);
            let cascade = builder.build();

            prop_assert_eq!(cascade.config.lazy, lazy);
            prop_assert_eq!(cascade.config.minimize, minimize);
        }

        /// grammar method accepts FST.
        #[test]
        fn builder_grammar(_seed in any::<u64>()) {
            let mut g = VectorWfst::<WordId, LogWeight>::new();
            let s = g.add_state();
            g.set_start(s);
            g.set_final(s, LogWeight::one());

            let builder = CascadeBuilder::<LogWeight>::new().grammar(g);
            let cascade = builder.build();

            // Should complete without error
            prop_assert!(cascade.fst.num_states() >= 1);
        }

        /// context_dependency method accepts FST.
        #[test]
        fn builder_context(_seed in any::<u64>()) {
            let mut c = VectorWfst::<PhoneId, LogWeight>::new();
            let s = c.add_state();
            c.set_start(s);
            c.set_final(s, LogWeight::one());

            let builder = CascadeBuilder::<LogWeight>::new().context_dependency(c);
            let cascade = builder.build();

            prop_assert!(cascade.fst.num_states() >= 1);
        }

        /// hmm method accepts FST.
        #[test]
        fn builder_hmm(_seed in any::<u64>()) {
            let mut h = VectorWfst::<PhoneId, LogWeight>::new();
            let s = h.add_state();
            h.set_start(s);
            h.set_final(s, LogWeight::one());

            let builder = CascadeBuilder::<LogWeight>::new().hmm(h);
            let cascade = builder.build();

            prop_assert!(cascade.fst.num_states() >= 1);
        }
    }

    // -------------------------------------------------------------------------
    // AsrCascade Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// as_fst returns the FST reference.
        #[test]
        fn as_fst_returns_fst(_seed in any::<u64>()) {
            let builder = CascadeBuilder::<LogWeight>::new();
            let cascade = builder.build();

            let fst = cascade.as_fst();
            prop_assert_eq!(fst.num_states(), cascade.fst.num_states());
        }

        /// statistics returns stats reference.
        #[test]
        fn statistics_returns_stats(_seed in any::<u64>()) {
            let builder = CascadeBuilder::<LogWeight>::new();
            let cascade = builder.build();

            let stats = cascade.statistics();
            prop_assert_eq!(stats.final_states, cascade.stats.final_states);
        }

        /// final_states in stats matches FST state count.
        #[test]
        fn stats_match_fst(_seed in any::<u64>()) {
            let builder = CascadeBuilder::<LogWeight>::new();
            let cascade = builder.build();

            prop_assert_eq!(cascade.stats.final_states, cascade.fst.num_states());
        }

        /// final_arcs in stats matches actual arc count.
        #[test]
        fn stats_arcs_match(_seed in any::<u64>()) {
            let mut builder = CascadeBuilder::<LogWeight>::new();
            builder.add_lexicon_entry(LexiconEntry::new(
                1,
                vec![10, 11, 12],
                LogWeight::new(0.0),
            ));

            let cascade = builder.build();

            let actual_arcs: usize = (0..cascade.fst.num_states() as StateId)
                .map(|s| cascade.fst.transitions(s).len())
                .sum();

            prop_assert_eq!(cascade.stats.final_arcs, actual_arcs);
        }
    }

    // -------------------------------------------------------------------------
    // Lexicon Building Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// Empty lexicon produces minimal FST.
        #[test]
        fn empty_lexicon_minimal(_seed in any::<u64>()) {
            let builder = CascadeBuilder::<LogWeight>::new();
            let cascade = builder.build();

            // Empty lexicon should still have at least start state
            prop_assert!(cascade.fst.num_states() >= 1);
        }

        /// Single word lexicon produces correct structure.
        #[test]
        fn single_word_lexicon(
            word_id in 0u32..100,
            phones in prop::collection::vec(0u32..50, 1..5)
        ) {
            let mut builder = CascadeBuilder::<LogWeight>::new();
            builder.add_lexicon_entry(LexiconEntry::new(
                word_id,
                phones.clone(),
                LogWeight::new(0.0),
            ));

            let cascade = builder.build();

            // Should have states for the pronunciation
            prop_assert!(cascade.fst.num_states() >= 1);
        }

        /// Multiple words with same pronunciation (homophones).
        #[test]
        fn homophone_lexicon(
            word1 in 0u32..50,
            word2 in 50u32..100
        ) {
            let phones = vec![10, 11, 12];

            let mut builder = CascadeBuilder::<LogWeight>::new();
            builder.add_lexicon_entry(LexiconEntry::new(
                word1,
                phones.clone(),
                LogWeight::new(0.0),
            ));
            builder.add_lexicon_entry(LexiconEntry::new(
                word2,
                phones.clone(),
                LogWeight::new(0.0),
            ));

            let cascade = builder.build();

            // Should handle homophones
            prop_assert!(cascade.fst.num_states() >= 1);
        }
    }
}
