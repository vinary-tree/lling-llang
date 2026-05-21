//! Core traits for the correction layer system.

use std::fmt;

use crate::backend::LatticeBackend;
use crate::lattice::Lattice;
use crate::semiring::Semiring;

/// Error type for layer operations.
#[derive(Clone, Debug)]
pub enum LayerError {
    /// The layer cannot process this lattice.
    CannotApply(String),
    /// Parse error during CFG filtering.
    ParseError(String),
    /// Configuration error.
    ConfigError(String),
    /// Resource error (e.g., model not loaded).
    ResourceError(String),
    /// Generic error with message.
    Other(String),
}

impl fmt::Display for LayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayerError::CannotApply(msg) => write!(f, "cannot apply layer: {}", msg),
            LayerError::ParseError(msg) => write!(f, "parse error: {}", msg),
            LayerError::ConfigError(msg) => write!(f, "configuration error: {}", msg),
            LayerError::ResourceError(msg) => write!(f, "resource error: {}", msg),
            LayerError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for LayerError {}

/// Result type for layer operations.
pub type LayerResult<T> = Result<T, LayerError>;

/// Statistics from applying a layer.
#[derive(Clone, Debug, Default)]
pub struct LayerStats {
    /// Number of input paths (or estimate).
    pub input_paths: usize,
    /// Number of output paths (or estimate).
    pub output_paths: usize,
    /// Number of edges in input lattice.
    pub input_edges: usize,
    /// Number of edges in output lattice.
    pub output_edges: usize,
    /// Time taken in microseconds.
    pub time_us: u64,
}

impl LayerStats {
    /// Calculate reduction ratio (0.0 = no paths remain, 1.0 = all paths remain).
    pub fn reduction_ratio(&self) -> f64 {
        if self.input_paths == 0 {
            1.0
        } else {
            self.output_paths as f64 / self.input_paths as f64
        }
    }
}

/// Trait for correction layers that filter/rerank lattice paths.
///
/// Each layer receives a lattice and returns a (typically smaller) lattice.
/// Layers can be composed sequentially: Layer1 → Layer2 → ... → LayerN
///
/// # Implementation Notes
///
/// - Layers should be stateless or use interior mutability for caching
/// - The `apply` method should not modify the input lattice
/// - Use `can_apply` to check preconditions before applying
/// - Implement `estimated_reduction` to help pipeline optimization
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::{CorrectionLayer, LayerError, LayerResult};
/// use lling_llang::lattice::Lattice;
/// use lling_llang::semiring::Semiring;
/// use lling_llang::backend::LatticeBackend;
///
/// struct MyLayer {
///     threshold: f64,
/// }
///
/// impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for MyLayer {
///     fn name(&self) -> &str { "my-layer" }
///
///     fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
///         // Filter or transform the lattice
///         Ok(lattice.clone())
///     }
/// }
/// ```
pub trait CorrectionLayer<W: Semiring, B: LatticeBackend>: Send + Sync {
    /// Human-readable layer name for diagnostics.
    fn name(&self) -> &str;

    /// Apply this layer's corrections/filtering to the input lattice.
    ///
    /// Returns a new lattice with paths filtered or reweighted.
    fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>>;

    /// Pre-check if this layer can process the given lattice.
    ///
    /// Override this to add precondition checks. Default returns true.
    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true
    }

    /// Estimated reduction factor (e.g., 0.1 = reduces to 10% of paths).
    ///
    /// Used by the pipeline for optimization decisions. Default returns 1.0 (no reduction).
    fn estimated_reduction(&self) -> f64 {
        1.0
    }

    /// Apply and return statistics about the operation.
    fn apply_with_stats(
        &self,
        lattice: &Lattice<W, B>,
    ) -> LayerResult<(Lattice<W, B>, LayerStats)> {
        let start = std::time::Instant::now();
        let input_edges = lattice.num_edges();

        let result = self.apply(lattice)?;

        let output_edges = result.num_edges();
        let elapsed = start.elapsed();

        let stats = LayerStats {
            input_paths: 0, // Would need path counting
            output_paths: 0,
            input_edges,
            output_edges,
            time_us: elapsed.as_micros() as u64,
        };

        Ok((result, stats))
    }
}

/// Layer pipeline for composing multiple correction layers.
///
/// Layers are applied in order: input → layer1 → layer2 → ... → output
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::{LayerPipeline, CfgFilterLayer};
///
/// let mut pipeline = LayerPipeline::new();
/// pipeline.add_layer(CfgFilterLayer::new(&grammar));
///
/// let result = pipeline.apply(&lattice)?;
/// ```
pub struct LayerPipeline<W: Semiring, B: LatticeBackend> {
    layers: Vec<Box<dyn CorrectionLayer<W, B>>>,
}

impl<W: Semiring, B: LatticeBackend> LayerPipeline<W, B> {
    /// Create a new empty pipeline.
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Add a layer to the end of the pipeline.
    pub fn add_layer<L: CorrectionLayer<W, B> + 'static>(&mut self, layer: L) {
        self.layers.push(Box::new(layer));
    }

    /// Get the number of layers in the pipeline.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Check if the pipeline is empty.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Get layer names for diagnostics.
    pub fn layer_names(&self) -> Vec<&str> {
        self.layers.iter().map(|l| l.name()).collect()
    }

    /// Apply all layers in sequence.
    pub fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
        let mut current = lattice.clone();

        for layer in &self.layers {
            if !layer.can_apply(&current) {
                return Err(LayerError::CannotApply(format!(
                    "layer '{}' cannot process lattice",
                    layer.name()
                )));
            }
            current = layer.apply(&current)?;
        }

        Ok(current)
    }

    /// Apply all layers and collect statistics.
    pub fn apply_with_stats(
        &self,
        lattice: &Lattice<W, B>,
    ) -> LayerResult<(Lattice<W, B>, Vec<LayerStats>)> {
        let mut current = lattice.clone();
        let mut all_stats = Vec::with_capacity(self.layers.len());

        for layer in &self.layers {
            if !layer.can_apply(&current) {
                return Err(LayerError::CannotApply(format!(
                    "layer '{}' cannot process lattice",
                    layer.name()
                )));
            }
            let (result, stats) = layer.apply_with_stats(&current)?;
            current = result;
            all_stats.push(stats);
        }

        Ok((current, all_stats))
    }

    /// Get estimated total reduction factor.
    pub fn estimated_reduction(&self) -> f64 {
        self.layers
            .iter()
            .map(|l| l.estimated_reduction())
            .product()
    }
}

impl<W: Semiring, B: LatticeBackend> Default for LayerPipeline<W, B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Semiring, B: LatticeBackend> fmt::Debug for LayerPipeline<W, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayerPipeline")
            .field("layers", &self.layer_names())
            .finish()
    }
}

/// Builder for constructing layer pipelines.
///
/// Provides a fluent API for adding layers.
///
/// # Example
///
/// ```ignore
/// use lling_llang::layers::LayerPipelineBuilder;
///
/// let pipeline = LayerPipelineBuilder::new()
///     .with_cfg(&grammar)
///     .build();
/// ```
pub struct LayerPipelineBuilder<W: Semiring, B: LatticeBackend> {
    layers: Vec<Box<dyn CorrectionLayer<W, B>>>,
}

impl<W: Semiring, B: LatticeBackend> LayerPipelineBuilder<W, B> {
    /// Create a new builder.
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Add a custom layer.
    pub fn add_layer<L: CorrectionLayer<W, B> + 'static>(mut self, layer: L) -> Self {
        self.layers.push(Box::new(layer));
        self
    }

    /// Build the pipeline.
    pub fn build(self) -> LayerPipeline<W, B> {
        LayerPipeline {
            layers: self.layers,
        }
    }
}

impl<W: Semiring, B: LatticeBackend> Default for LayerPipelineBuilder<W, B> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::{EdgeMetadata, LatticeBuilder};
    use crate::semiring::TropicalWeight;

    /// Simple pass-through layer for testing.
    struct IdentityLayer;

    impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for IdentityLayer {
        fn name(&self) -> &str {
            "identity"
        }

        fn apply(&self, lattice: &Lattice<W, B>) -> LayerResult<Lattice<W, B>> {
            Ok(lattice.clone())
        }
    }

    /// Layer that nominally marks edges with metadata; tests only exercise
    /// pipeline plumbing, so this is a pass-through.
    struct MarkingLayer;

    impl CorrectionLayer<TropicalWeight, HashMapBackend> for MarkingLayer {
        fn name(&self) -> &str {
            "marking"
        }

        fn apply(
            &self,
            lattice: &Lattice<TropicalWeight, HashMapBackend>,
        ) -> LayerResult<Lattice<TropicalWeight, HashMapBackend>> {
            Ok(lattice.clone())
        }

        fn estimated_reduction(&self) -> f64 {
            0.5
        }
    }

    fn build_test_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
        let mut backend = HashMapBackend::new();
        let hello = backend.intern("hello");
        let world = backend.intern("world");

        let mut builder = LatticeBuilder::new(backend);
        builder.add_correction_by_id(0, 1, hello, TropicalWeight::one(), EdgeMetadata::default());
        builder.add_correction_by_id(1, 2, world, TropicalWeight::one(), EdgeMetadata::default());
        builder.build(2)
    }

    #[test]
    fn test_layer_error_display() {
        let err = LayerError::CannotApply("test".to_string());
        assert!(err.to_string().contains("cannot apply"));

        let err = LayerError::ParseError("syntax".to_string());
        assert!(err.to_string().contains("parse error"));
    }

    #[test]
    fn test_layer_stats() {
        let stats = LayerStats {
            input_paths: 100,
            output_paths: 10,
            input_edges: 50,
            output_edges: 5,
            time_us: 1000,
        };

        assert!((stats.reduction_ratio() - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_identity_layer() {
        let lattice = build_test_lattice();
        let layer = IdentityLayer;

        // Use explicit trait method calls with concrete types
        type Layer = IdentityLayer;
        type W = TropicalWeight;
        type B = HashMapBackend;

        assert_eq!(<Layer as CorrectionLayer<W, B>>::name(&layer), "identity");
        assert!(<Layer as CorrectionLayer<W, B>>::can_apply(
            &layer, &lattice
        ));
        assert!(
            (<Layer as CorrectionLayer<W, B>>::estimated_reduction(&layer) - 1.0).abs() < 0.001
        );

        let result =
            <Layer as CorrectionLayer<W, B>>::apply(&layer, &lattice).expect("should apply");
        assert_eq!(result.num_edges(), lattice.num_edges());
    }

    #[test]
    fn test_pipeline_empty() {
        let pipeline: LayerPipeline<TropicalWeight, HashMapBackend> = LayerPipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.len(), 0);

        let lattice = build_test_lattice();
        let result = pipeline.apply(&lattice).expect("should apply");
        assert_eq!(result.num_edges(), lattice.num_edges());
    }

    #[test]
    fn test_pipeline_single_layer() {
        let mut pipeline: LayerPipeline<TropicalWeight, HashMapBackend> = LayerPipeline::new();
        pipeline.add_layer(IdentityLayer);

        assert_eq!(pipeline.len(), 1);
        assert_eq!(pipeline.layer_names(), vec!["identity"]);

        let lattice = build_test_lattice();
        let result = pipeline.apply(&lattice).expect("should apply");
        assert_eq!(result.num_edges(), lattice.num_edges());
    }

    #[test]
    fn test_pipeline_multiple_layers() {
        let mut pipeline: LayerPipeline<TropicalWeight, HashMapBackend> = LayerPipeline::new();
        pipeline.add_layer(IdentityLayer);
        pipeline.add_layer(MarkingLayer);

        assert_eq!(pipeline.len(), 2);
        assert_eq!(pipeline.layer_names(), vec!["identity", "marking"]);

        // Estimated reduction: 1.0 * 0.5 = 0.5
        assert!((pipeline.estimated_reduction() - 0.5).abs() < 0.001);

        let lattice = build_test_lattice();
        let result = pipeline.apply(&lattice).expect("should apply");
        assert_eq!(result.num_edges(), lattice.num_edges());
    }

    #[test]
    fn test_pipeline_with_stats() {
        let mut pipeline: LayerPipeline<TropicalWeight, HashMapBackend> = LayerPipeline::new();
        pipeline.add_layer(IdentityLayer);

        let lattice = build_test_lattice();
        let (result, stats) = pipeline.apply_with_stats(&lattice).expect("should apply");

        assert_eq!(result.num_edges(), lattice.num_edges());
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].input_edges, 2);
        assert_eq!(stats[0].output_edges, 2);
    }

    #[test]
    fn test_pipeline_builder() {
        let pipeline: LayerPipeline<TropicalWeight, HashMapBackend> = LayerPipelineBuilder::new()
            .add_layer(IdentityLayer)
            .add_layer(MarkingLayer)
            .build();

        assert_eq!(pipeline.len(), 2);
    }

    #[test]
    fn test_pipeline_debug() {
        let mut pipeline: LayerPipeline<TropicalWeight, HashMapBackend> = LayerPipeline::new();
        pipeline.add_layer(IdentityLayer);

        let debug_str = format!("{:?}", pipeline);
        assert!(debug_str.contains("identity"));
    }
}
