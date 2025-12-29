//! POS tagging layer for part-of-speech based filtering.
//!
//! This layer filters lattice paths based on part-of-speech tag sequences.
//! It requires an external POS tagger implementation.
//!
//! # Feature Gate
//!
//! This module is only available when the `pos-tagging` feature is enabled.

use rustc_hash::FxHashSet;

use crate::backend::LatticeBackend;
use crate::lattice::{Lattice, LatticeBuilder, EdgeMetadata, LatticePathExt};
use crate::semiring::Semiring;

use super::traits::{CorrectionLayer, LayerError, LayerResult};

/// Part-of-speech tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PosTag(pub String);

impl PosTag {
    /// Create a new POS tag.
    pub fn new(tag: impl Into<String>) -> Self {
        Self(tag.into())
    }
}

/// Trait for POS tagging models.
///
/// Implement this trait to provide POS tagging functionality.
pub trait PosModel: Send + Sync {
    /// Tag a sequence of tokens.
    fn tag(&self, tokens: &[&str]) -> Vec<PosTag>;

    /// Tag multiple token sequences (for batch processing).
    fn tag_batch(&self, sequences: &[Vec<&str>]) -> Vec<Vec<PosTag>> {
        sequences.iter().map(|seq| self.tag(seq)).collect()
    }
}

/// POS tagging layer for filtering by part-of-speech patterns.
///
/// This layer filters lattice paths to only those whose POS tag sequences
/// match the specified constraints.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::PosTaggingLayer;
///
/// let layer = PosTaggingLayer::new(Box::new(my_pos_model))
///     .with_required_pattern(&["DET", "NOUN", "VERB"]);
/// let filtered = layer.apply(&lattice)?;
/// ```
pub struct PosTaggingLayer {
    model: Box<dyn PosModel>,
    /// Optional required POS tag patterns.
    required_patterns: Vec<Vec<PosTag>>,
    /// Optional forbidden POS tag sequences.
    forbidden_sequences: Vec<Vec<PosTag>>,
}

impl PosTaggingLayer {
    /// Create a new POS tagging layer with the given model.
    pub fn new(model: Box<dyn PosModel>) -> Self {
        Self {
            model,
            required_patterns: Vec::new(),
            forbidden_sequences: Vec::new(),
        }
    }

    /// Add a required POS tag pattern.
    ///
    /// Paths must match at least one required pattern (if any are specified).
    pub fn with_required_pattern(mut self, pattern: &[&str]) -> Self {
        self.required_patterns.push(pattern.iter().map(|&s| PosTag::new(s)).collect());
        self
    }

    /// Add a forbidden POS tag sequence.
    ///
    /// Paths containing this sequence will be filtered out.
    pub fn with_forbidden_sequence(mut self, sequence: &[&str]) -> Self {
        self.forbidden_sequences.push(sequence.iter().map(|&s| PosTag::new(s)).collect());
        self
    }

    /// Get the POS model.
    pub fn model(&self) -> &dyn PosModel {
        self.model.as_ref()
    }

    /// Check if tags match any required pattern.
    ///
    /// Returns true if no required patterns are specified,
    /// or if the tags match at least one required pattern.
    fn matches_required(&self, tags: &[PosTag]) -> bool {
        if self.required_patterns.is_empty() {
            return true;
        }
        self.required_patterns.iter().any(|pattern| {
            // Check if pattern matches (exact match or subsequence)
            if pattern.len() == tags.len() {
                pattern == tags
            } else if pattern.len() < tags.len() {
                // Check if pattern appears as a subsequence
                tags.windows(pattern.len()).any(|w| w == pattern)
            } else {
                false
            }
        })
    }

    /// Check if tags contain any forbidden sequence.
    ///
    /// Returns true if the tags contain any forbidden sequence.
    fn contains_forbidden(&self, tags: &[PosTag]) -> bool {
        self.forbidden_sequences.iter().any(|seq| {
            if seq.len() <= tags.len() {
                tags.windows(seq.len()).any(|w| w == seq)
            } else {
                false
            }
        })
    }

    /// Check if a tag sequence passes all constraints.
    fn passes_constraints(&self, tags: &[PosTag]) -> bool {
        self.matches_required(tags) && !self.contains_forbidden(tags)
    }
}

impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for PosTaggingLayer {
    fn name(&self) -> &str {
        "pos-tagging"
    }

    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        if lattice.is_empty() {
            return Ok(lattice.clone());
        }

        // Collect edge IDs from paths that pass POS constraints
        let mut used_edges: FxHashSet<crate::lattice::EdgeId> = FxHashSet::default();

        // Iterate over all paths in the lattice
        for path in lattice.paths() {
            // Get words from the path
            let words: Vec<&str> = path.words(lattice).collect();

            if words.is_empty() {
                continue;
            }

            // Tag the path using the POS model
            let tags = self.model.tag(&words);

            // Check if the path passes constraints
            if self.passes_constraints(&tags) {
                // Add all edges from this path to the used set
                for edge_id in &path.edges {
                    used_edges.insert(*edge_id);
                }
            }
        }

        // If no paths passed, return error
        if used_edges.is_empty() {
            return Err(LayerError::Other(
                "no paths passed POS constraints".to_string()
            ));
        }

        // Build a new lattice with only the used edges
        let mut new_builder = LatticeBuilder::new(lattice.backend().clone());

        for edge in lattice.edges() {
            if used_edges.contains(&edge.id) {
                new_builder.add_correction_by_id(
                    edge.source.0 as usize,
                    edge.target.0 as usize,
                    edge.label,
                    edge.weight,
                    edge.metadata.clone(),
                );
            }
        }

        // Build with the same end position
        let end_pos = lattice.end().0 as usize;
        Ok(new_builder.build(end_pos))
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true
    }

    fn estimated_reduction(&self) -> f64 {
        // POS filtering typically provides moderate reduction
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::semiring::TropicalWeight;

    struct MockPosModel;

    impl PosModel for MockPosModel {
        fn tag(&self, tokens: &[&str]) -> Vec<PosTag> {
            tokens.iter().map(|_| PosTag::new("NOUN")).collect()
        }
    }

    #[test]
    fn test_pos_tag_creation() {
        let tag = PosTag::new("NOUN");
        assert_eq!(tag.0, "NOUN");
    }

    #[test]
    fn test_mock_model() {
        let model = MockPosModel;
        let tags = model.tag(&["the", "dog", "runs"]);
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn test_layer_name() {
        let layer = PosTaggingLayer::new(Box::new(MockPosModel));
        // Use explicit trait method call with concrete types
        let name = <PosTaggingLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::name(&layer);
        assert_eq!(name, "pos-tagging");
    }

    #[test]
    fn test_layer_builder() {
        let layer = PosTaggingLayer::new(Box::new(MockPosModel))
            .with_required_pattern(&["DET", "NOUN", "VERB"])
            .with_forbidden_sequence(&["NOUN", "NOUN", "NOUN"]);

        assert_eq!(layer.required_patterns.len(), 1);
        assert_eq!(layer.forbidden_sequences.len(), 1);
    }

    #[test]
    fn test_estimated_reduction() {
        let layer = PosTaggingLayer::new(Box::new(MockPosModel));
        let reduction = <PosTaggingLayer as CorrectionLayer<TropicalWeight, HashMapBackend>>::estimated_reduction(&layer);
        assert!((reduction - 0.5).abs() < 0.001);
    }
}
