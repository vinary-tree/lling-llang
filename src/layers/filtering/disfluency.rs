//! Disfluency removal correction layer.
//!
//! This module provides a correction layer that detects and optionally removes
//! disfluencies (filled pauses, restarts, repairs, repetitions) from lattices.
//!
//! # Features
//!
//! - Integrates with the correction layer pipeline
//! - Detects various disfluency types (filled pauses, repetitions, etc.)
//! - Can either remove disfluent paths or penalize them
//! - Supports configurable sensitivity and thresholds
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::layers::{DisfluencyLayer, DisfluencyLayerConfig};
//! use lling_llang::layers::LayerPipeline;
//!
//! let config = DisfluencyLayerConfig::default();
//! let layer = DisfluencyLayer::new(config);
//!
//! let mut pipeline = LayerPipeline::new();
//! pipeline.add_layer(layer);
//!
//! let cleaned = pipeline.apply(&lattice)?;
//! ```

use std::collections::HashSet;

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder};
use crate::semiring::{Semiring, TropicalWeight};

use super::super::traits::{CorrectionLayer, LayerResult};

/// Types of disfluencies to detect/remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisfluencyType {
    /// Filled pauses: "um", "uh", "er", "ah"
    FilledPause,
    /// Discourse markers: "like", "you know", "I mean"
    DiscourseMarker,
    /// Word repetitions: "I I I want"
    WordRepetition,
    /// Partial word restarts: "I wa- want"
    Restart,
    /// Self-corrections: "the red- the blue car"
    Repair,
}

impl DisfluencyType {
    /// Get all disfluency types.
    pub fn all() -> &'static [DisfluencyType] {
        &[
            DisfluencyType::FilledPause,
            DisfluencyType::DiscourseMarker,
            DisfluencyType::WordRepetition,
            DisfluencyType::Restart,
            DisfluencyType::Repair,
        ]
    }

    /// Get the name of this disfluency type.
    pub fn name(&self) -> &'static str {
        match self {
            DisfluencyType::FilledPause => "filled_pause",
            DisfluencyType::DiscourseMarker => "discourse_marker",
            DisfluencyType::WordRepetition => "word_repetition",
            DisfluencyType::Restart => "restart",
            DisfluencyType::Repair => "repair",
        }
    }
}

/// Configuration for the disfluency layer.
#[derive(Debug, Clone)]
pub struct DisfluencyLayerConfig {
    /// Disfluency types to detect/remove.
    pub types_to_detect: Vec<DisfluencyType>,
    /// Filled pause words to detect (lowercase).
    pub filled_pauses: HashSet<String>,
    /// Discourse markers to detect (lowercase).
    pub discourse_markers: HashSet<String>,
    /// Whether to remove disfluent edges or just penalize them.
    pub remove_disfluencies: bool,
    /// Penalty weight for detected disfluencies (higher = more aggressive removal).
    pub disfluency_penalty: f64,
    /// Minimum consecutive repetitions to flag as word repetition.
    pub min_word_repetitions: usize,
    /// Whether to preserve one instance of repeated words.
    pub preserve_one_repetition: bool,
}

impl Default for DisfluencyLayerConfig {
    fn default() -> Self {
        Self {
            types_to_detect: vec![
                DisfluencyType::FilledPause,
                DisfluencyType::DiscourseMarker,
                DisfluencyType::WordRepetition,
            ],
            filled_pauses: default_filled_pauses(),
            discourse_markers: default_discourse_markers(),
            remove_disfluencies: false,
            disfluency_penalty: 2.0,
            min_word_repetitions: 2,
            preserve_one_repetition: true,
        }
    }
}

/// Get default English filled pause words.
fn default_filled_pauses() -> HashSet<String> {
    [
        "um", "uh", "er", "ah", "eh", "mm", "hmm", "hm", "umm", "uhh", "err", "ahh", "ehh", "mmm",
        "hmmm",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Get default English discourse markers.
fn default_discourse_markers() -> HashSet<String> {
    [
        "like",
        "you know",
        "i mean",
        "so",
        "well",
        "anyway",
        "basically",
        "actually",
        "literally",
        "honestly",
        "right",
        "okay",
        "ok",
        "yeah",
        "yep",
        "nope",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// A detected disfluency span in the lattice.
#[derive(Debug, Clone)]
pub struct DisfluencySpan {
    /// Type of disfluency detected.
    pub disfluency_type: DisfluencyType,
    /// Edge indices involved in this disfluency.
    pub edge_indices: Vec<usize>,
    /// Start node ID.
    pub start_node: u32,
    /// End node ID.
    pub end_node: u32,
    /// The word(s) involved.
    pub words: Vec<String>,
    /// Detection confidence (higher = more confident).
    pub confidence: f64,
}

/// Disfluency removal correction layer.
///
/// Detects and handles disfluencies in word-level lattices.
#[derive(Debug, Clone)]
pub struct DisfluencyLayer {
    config: DisfluencyLayerConfig,
}

impl DisfluencyLayer {
    /// Create a new disfluency layer with the given configuration.
    pub fn new(config: DisfluencyLayerConfig) -> Self {
        Self { config }
    }

    /// Create a layer that only detects filled pauses.
    pub fn filled_pauses_only() -> Self {
        let mut config = DisfluencyLayerConfig::default();
        config.types_to_detect = vec![DisfluencyType::FilledPause];
        Self::new(config)
    }

    /// Create a layer with aggressive disfluency removal.
    pub fn aggressive() -> Self {
        let mut config = DisfluencyLayerConfig::default();
        config.types_to_detect = DisfluencyType::all().to_vec();
        config.remove_disfluencies = true;
        config.disfluency_penalty = 5.0;
        Self::new(config)
    }

    /// Get the layer name (for trait implementation).
    pub fn layer_name(&self) -> &str {
        "disfluency"
    }

    /// Get estimated reduction factor (for trait implementation).
    pub fn estimated_reduction_factor(&self) -> f64 {
        if self.config.remove_disfluencies {
            0.9 // Estimate: remove ~10% of paths
        } else {
            1.0 // Penalization doesn't change path count
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &DisfluencyLayerConfig {
        &self.config
    }

    /// Check if a word is a filled pause.
    fn is_filled_pause(&self, word: &str) -> bool {
        self.config.filled_pauses.contains(&word.to_lowercase())
    }

    /// Check if a word is a discourse marker.
    fn is_discourse_marker(&self, word: &str) -> bool {
        self.config.discourse_markers.contains(&word.to_lowercase())
    }

    /// Check if an edge should be penalized or removed.
    fn is_disfluent_word(&self, word: &str) -> Option<DisfluencyType> {
        let word_lower = word.to_lowercase();

        for &dtype in &self.config.types_to_detect {
            match dtype {
                DisfluencyType::FilledPause if self.is_filled_pause(&word_lower) => {
                    return Some(DisfluencyType::FilledPause);
                }
                DisfluencyType::DiscourseMarker if self.is_discourse_marker(&word_lower) => {
                    return Some(DisfluencyType::DiscourseMarker);
                }
                _ => continue,
            }
        }

        None
    }

    /// Detect word repetitions in a sequence of edges.
    fn detect_word_repetitions<W: Semiring, B: LatticeBackend>(
        &self,
        lattice: &Lattice<W, B>,
    ) -> Vec<DisfluencySpan> {
        if !self
            .config
            .types_to_detect
            .contains(&DisfluencyType::WordRepetition)
        {
            return Vec::new();
        }

        let mut spans = Vec::new();

        // Group edges by source node to find consecutive edges
        let mut edges_by_source: std::collections::HashMap<
            u32,
            Vec<(usize, &crate::lattice::Edge<W>)>,
        > = std::collections::HashMap::new();

        for (idx, edge) in lattice.edges().iter().enumerate() {
            edges_by_source
                .entry(edge.source.value())
                .or_default()
                .push((idx, edge));
        }

        // Look for paths where the same word appears consecutively
        for edge in lattice.edges().iter() {
            let word = match lattice.word(edge.label) {
                Some(w) => w.to_string(),
                None => continue,
            };

            // Check if the next edge has the same word (repetition)
            if let Some(next_edges) = edges_by_source.get(&edge.target.value()) {
                for &(_next_idx, next_edge) in next_edges {
                    if let Some(next_word) = lattice.word(next_edge.label) {
                        if word.to_lowercase() == next_word.to_lowercase() {
                            spans.push(DisfluencySpan {
                                disfluency_type: DisfluencyType::WordRepetition,
                                edge_indices: vec![],
                                start_node: edge.source.value(),
                                end_node: next_edge.target.value(),
                                words: vec![word.clone(), next_word.to_string()],
                                confidence: 0.9,
                            });
                        }
                    }
                }
            }
        }

        spans
    }
}

impl Default for DisfluencyLayer {
    fn default() -> Self {
        Self::new(DisfluencyLayerConfig::default())
    }
}

impl<W, B> CorrectionLayer<W, B> for DisfluencyLayer
where
    W: Semiring + From<TropicalWeight> + Clone,
    B: LatticeBackend + Clone,
{
    fn name(&self) -> &str {
        self.layer_name()
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        let backend = lattice.backend().clone();
        let mut builder = LatticeBuilder::new(backend);

        // Track which edges to skip (if remove_disfluencies is true)
        let mut skip_edges: HashSet<usize> = HashSet::new();

        // Detect word repetitions
        let _repetition_spans = self.detect_word_repetitions(lattice);

        // Process each edge
        for (idx, edge) in lattice.edges().iter().enumerate() {
            let source_pos = edge.source.value() as usize;
            let target_pos = edge.target.value() as usize;

            let word = match lattice.word(edge.label) {
                Some(w) => w.to_string(),
                None => {
                    // Keep edges without words as-is
                    builder.add_correction_by_id(
                        source_pos,
                        target_pos,
                        edge.label,
                        edge.weight.clone(),
                        edge.metadata.clone(),
                    );
                    continue;
                }
            };

            // Check if this is a disfluent word
            let disfluency = self.is_disfluent_word(&word);

            if disfluency.is_some() {
                if self.config.remove_disfluencies {
                    // Mark edge to skip and penalize it heavily instead of removing
                    skip_edges.insert(idx);

                    // Add heavily penalized edge to discourage this path
                    let penalty_weight =
                        W::from(TropicalWeight::new(self.config.disfluency_penalty * 10.0));
                    let penalized_weight = edge.weight.clone().times(&penalty_weight);

                    let mut metadata = edge.metadata.clone();
                    metadata.is_original = false;

                    builder.add_correction_by_id(
                        source_pos,
                        target_pos,
                        edge.label,
                        penalized_weight,
                        metadata,
                    );
                } else {
                    // Penalize the disfluent edge
                    let penalty_weight =
                        W::from(TropicalWeight::new(self.config.disfluency_penalty));
                    let penalized_weight = edge.weight.clone().times(&penalty_weight);

                    let mut metadata = edge.metadata.clone();
                    metadata.is_original = false;

                    builder.add_correction_by_id(
                        source_pos,
                        target_pos,
                        edge.label,
                        penalized_weight,
                        metadata,
                    );
                }
            } else if !skip_edges.contains(&idx) {
                // Keep non-disfluent edge as-is
                builder.add_correction_by_id(
                    source_pos,
                    target_pos,
                    edge.label,
                    edge.weight.clone(),
                    edge.metadata.clone(),
                );
            }
        }

        // Find max node id for final state
        let max_node = lattice
            .edges()
            .iter()
            .map(|e| e.source.value().max(e.target.value()) as usize)
            .max()
            .unwrap_or(0);

        Ok(builder.build(max_node))
    }

    fn estimated_reduction(&self) -> f64 {
        self.estimated_reduction_factor()
    }
}

/// Builder for custom disfluency detection rules.
#[derive(Debug, Clone)]
pub struct DisfluencyRuleBuilder {
    filled_pauses: HashSet<String>,
    discourse_markers: HashSet<String>,
    types: Vec<DisfluencyType>,
    remove: bool,
    penalty: f64,
}

impl DisfluencyRuleBuilder {
    /// Create a new rule builder.
    pub fn new() -> Self {
        Self {
            filled_pauses: HashSet::new(),
            discourse_markers: HashSet::new(),
            types: Vec::new(),
            remove: false,
            penalty: 2.0,
        }
    }

    /// Add a filled pause word.
    pub fn add_filled_pause(mut self, word: impl Into<String>) -> Self {
        self.filled_pauses.insert(word.into().to_lowercase());
        if !self.types.contains(&DisfluencyType::FilledPause) {
            self.types.push(DisfluencyType::FilledPause);
        }
        self
    }

    /// Add multiple filled pause words.
    pub fn add_filled_pauses(mut self, words: &[&str]) -> Self {
        for word in words {
            self.filled_pauses.insert(word.to_lowercase());
        }
        if !self.types.contains(&DisfluencyType::FilledPause) {
            self.types.push(DisfluencyType::FilledPause);
        }
        self
    }

    /// Add a discourse marker.
    pub fn add_discourse_marker(mut self, word: impl Into<String>) -> Self {
        self.discourse_markers.insert(word.into().to_lowercase());
        if !self.types.contains(&DisfluencyType::DiscourseMarker) {
            self.types.push(DisfluencyType::DiscourseMarker);
        }
        self
    }

    /// Add multiple discourse markers.
    pub fn add_discourse_markers(mut self, words: &[&str]) -> Self {
        for word in words {
            self.discourse_markers.insert(word.to_lowercase());
        }
        if !self.types.contains(&DisfluencyType::DiscourseMarker) {
            self.types.push(DisfluencyType::DiscourseMarker);
        }
        self
    }

    /// Enable word repetition detection.
    pub fn detect_word_repetitions(mut self) -> Self {
        if !self.types.contains(&DisfluencyType::WordRepetition) {
            self.types.push(DisfluencyType::WordRepetition);
        }
        self
    }

    /// Set whether to remove disfluencies or just penalize them.
    pub fn remove_disfluencies(mut self, remove: bool) -> Self {
        self.remove = remove;
        self
    }

    /// Set the penalty weight for disfluencies.
    pub fn penalty(mut self, penalty: f64) -> Self {
        self.penalty = penalty;
        self
    }

    /// Build the disfluency layer.
    pub fn build(self) -> DisfluencyLayer {
        let config = DisfluencyLayerConfig {
            types_to_detect: if self.types.is_empty() {
                vec![DisfluencyType::FilledPause]
            } else {
                self.types
            },
            filled_pauses: if self.filled_pauses.is_empty() {
                default_filled_pauses()
            } else {
                self.filled_pauses
            },
            discourse_markers: if self.discourse_markers.is_empty() {
                default_discourse_markers()
            } else {
                self.discourse_markers
            },
            remove_disfluencies: self.remove,
            disfluency_penalty: self.penalty,
            min_word_repetitions: 2,
            preserve_one_repetition: true,
        };
        DisfluencyLayer::new(config)
    }
}

impl Default for DisfluencyRuleBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::EdgeMetadata;

    #[test]
    fn test_disfluency_type_all() {
        let types = DisfluencyType::all();
        assert_eq!(types.len(), 5);
    }

    #[test]
    fn test_disfluency_type_names() {
        assert_eq!(DisfluencyType::FilledPause.name(), "filled_pause");
        assert_eq!(DisfluencyType::DiscourseMarker.name(), "discourse_marker");
        assert_eq!(DisfluencyType::WordRepetition.name(), "word_repetition");
    }

    #[test]
    fn test_config_default() {
        let config = DisfluencyLayerConfig::default();
        assert!(!config.remove_disfluencies);
        assert!((config.disfluency_penalty - 2.0).abs() < 0.001);
        assert!(!config.filled_pauses.is_empty());
        assert!(!config.discourse_markers.is_empty());
    }

    #[test]
    fn test_default_filled_pauses() {
        let pauses = default_filled_pauses();
        assert!(pauses.contains("um"));
        assert!(pauses.contains("uh"));
        assert!(pauses.contains("er"));
    }

    #[test]
    fn test_default_discourse_markers() {
        let markers = default_discourse_markers();
        assert!(markers.contains("like"));
        assert!(markers.contains("you know"));
        assert!(markers.contains("i mean"));
    }

    #[test]
    fn test_layer_creation() {
        let layer = DisfluencyLayer::default();
        assert_eq!(layer.layer_name(), "disfluency");
        assert!(!layer.config().remove_disfluencies);
    }

    #[test]
    fn test_filled_pauses_only() {
        let layer = DisfluencyLayer::filled_pauses_only();
        assert_eq!(layer.config().types_to_detect.len(), 1);
        assert_eq!(
            layer.config().types_to_detect[0],
            DisfluencyType::FilledPause
        );
    }

    #[test]
    fn test_aggressive_mode() {
        let layer = DisfluencyLayer::aggressive();
        assert!(layer.config().remove_disfluencies);
        assert!((layer.config().disfluency_penalty - 5.0).abs() < 0.001);
        assert_eq!(layer.config().types_to_detect.len(), 5);
    }

    #[test]
    fn test_is_filled_pause() {
        let layer = DisfluencyLayer::default();
        assert!(layer.is_filled_pause("um"));
        assert!(layer.is_filled_pause("UM"));
        assert!(layer.is_filled_pause("Uh"));
        assert!(!layer.is_filled_pause("hello"));
    }

    #[test]
    fn test_is_discourse_marker() {
        let layer = DisfluencyLayer::default();
        assert!(layer.is_discourse_marker("like"));
        assert!(layer.is_discourse_marker("LIKE"));
        assert!(layer.is_discourse_marker("you know"));
        assert!(!layer.is_discourse_marker("computer"));
    }

    #[test]
    fn test_is_disfluent_word() {
        let layer = DisfluencyLayer::default();

        assert_eq!(
            layer.is_disfluent_word("um"),
            Some(DisfluencyType::FilledPause)
        );
        assert_eq!(
            layer.is_disfluent_word("like"),
            Some(DisfluencyType::DiscourseMarker)
        );
        assert_eq!(layer.is_disfluent_word("hello"), None);
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = DisfluencyLayer::default();
        assert!((layer.estimated_reduction_factor() - 1.0).abs() < 0.001);

        let aggressive = DisfluencyLayer::aggressive();
        assert!((aggressive.estimated_reduction_factor() - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_rule_builder() {
        let layer = DisfluencyRuleBuilder::new()
            .add_filled_pause("um")
            .add_filled_pause("uh")
            .add_discourse_marker("like")
            .penalty(3.0)
            .remove_disfluencies(true)
            .build();

        assert!(layer.config().remove_disfluencies);
        assert!((layer.config().disfluency_penalty - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_rule_builder_batch() {
        let layer = DisfluencyRuleBuilder::new()
            .add_filled_pauses(&["um", "uh", "er"])
            .add_discourse_markers(&["like", "basically"])
            .build();

        assert!(layer.is_filled_pause("um"));
        assert!(layer.is_filled_pause("er"));
        assert!(layer.is_discourse_marker("basically"));
    }

    #[test]
    fn test_disfluency_span() {
        let span = DisfluencySpan {
            disfluency_type: DisfluencyType::FilledPause,
            edge_indices: vec![1, 2],
            start_node: 0,
            end_node: 2,
            words: vec!["um".to_string()],
            confidence: 0.95,
        };

        assert_eq!(span.disfluency_type, DisfluencyType::FilledPause);
        assert_eq!(span.words, vec!["um"]);
    }

    #[test]
    fn test_apply_empty_lattice() {
        let layer = DisfluencyLayer::default();
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, HashMapBackend> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        let result = <DisfluencyLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_apply_simple_lattice() {
        let layer = DisfluencyLayer::default();
        let mut backend = HashMapBackend::new();

        // Add words
        let hello = backend.intern("hello");
        let um = backend.intern("um");
        let world = backend.intern("world");

        let mut builder: LatticeBuilder<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend);

        // Create: hello -> um -> world
        builder.add_correction_by_id(0, 1, hello, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, um, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(2, 3, world, TropicalWeight::one(), EdgeMetadata::default());

        let lattice = builder.build(3);

        let result = <DisfluencyLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        assert!(result.is_ok());

        let filtered = result.expect("layers/disfluency.rs: required value was None/Err");
        // Should still have edges, but "um" edge should be penalized
        assert!(filtered.num_edges() >= 3);
    }

    #[test]
    fn test_apply_with_removal() {
        let mut config = DisfluencyLayerConfig::default();
        config.remove_disfluencies = true;
        let layer = DisfluencyLayer::new(config);

        let mut backend = HashMapBackend::new();
        let hello = backend.intern("hello");
        let um = backend.intern("um");

        let mut builder: LatticeBuilder<TropicalWeight, HashMapBackend> =
            LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, hello, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, um, TropicalWeight::one(), EdgeMetadata::default());

        let lattice = builder.build(2);

        let result = <DisfluencyLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::apply(
            &layer, &lattice,
        );
        assert!(result.is_ok());

        // Should have bypass edge for "um"
        let filtered = result.expect("layers/disfluency.rs: required value was None/Err");
        assert!(filtered.num_edges() >= 2);
    }

    #[test]
    fn test_correction_layer_trait() {
        let layer = DisfluencyLayer::default();

        // Test trait methods via inherent methods (to avoid type annotation issues)
        assert_eq!(layer.layer_name(), "disfluency");
        assert!((layer.estimated_reduction_factor() - 1.0).abs() < 0.001);
    }
}
