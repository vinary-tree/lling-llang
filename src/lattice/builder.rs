//! Lattice construction builder.

use rustc_hash::FxHashMap;

use super::lattice::Lattice;
use super::types::{Edge, EdgeId, EdgeMetadata, Node, NodeId};
use crate::backend::LatticeBackend;
use crate::semiring::Semiring;

/// Builder for constructing lattices incrementally.
///
/// The builder handles:
/// - Creating nodes for positions automatically
/// - Interning vocabulary via the backend
/// - Tracking edge connectivity
///
/// # Example
///
/// ```rust
/// use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
/// use lling_llang::backend::HashMapBackend;
/// use lling_llang::semiring::TropicalWeight;
///
/// let backend = HashMapBackend::new();
/// let mut builder = LatticeBuilder::new(backend);
///
/// // Add corrections for position 0 -> 1
/// builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
/// builder.add_correction(0, 1, "teh", TropicalWeight::new(0.0), EdgeMetadata::original());
///
/// // Add corrections for position 1 -> 2
/// builder.add_correction(1, 2, "quick", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
///
/// let lattice = builder.build(2);
/// ```
#[derive(Clone, Debug)]
pub struct LatticeBuilder<W: Semiring, B: LatticeBackend> {
    /// Nodes indexed by ID.
    nodes: Vec<Node>,
    /// Edges.
    edges: Vec<Edge<W>>,
    /// Backend for vocabulary interning.
    backend: B,
    /// Map from position to node ID.
    position_map: FxHashMap<usize, NodeId>,
}

impl<W: Semiring, B: LatticeBackend> LatticeBuilder<W, B> {
    /// Create a new builder with the given backend.
    pub fn new(backend: B) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            backend,
            position_map: FxHashMap::default(),
        }
    }

    /// Create a new builder with pre-allocated capacity.
    pub fn with_capacity(backend: B, num_positions: usize, edges_per_position: usize) -> Self {
        let estimated_nodes = num_positions + 1;
        let estimated_edges = num_positions * edges_per_position;

        Self {
            nodes: Vec::with_capacity(estimated_nodes),
            edges: Vec::with_capacity(estimated_edges),
            backend,
            position_map: FxHashMap::with_capacity_and_hasher(estimated_nodes, Default::default()),
        }
    }

    /// Get or create a node for a position.
    fn get_or_create_node(&mut self, position: usize) -> NodeId {
        if let Some(&node_id) = self.position_map.get(&position) {
            return node_id;
        }

        let node_id = NodeId::new(self.nodes.len() as u32);
        self.nodes.push(Node::with_position(node_id, position));
        self.position_map.insert(position, node_id);
        node_id
    }

    /// Add a correction edge from one position to another.
    ///
    /// # Arguments
    ///
    /// * `start_pos` - Starting position in the input sequence
    /// * `end_pos` - Ending position in the input sequence
    /// * `word` - The correction word
    /// * `weight` - Edge weight
    /// * `metadata` - Edge metadata
    ///
    /// # Returns
    ///
    /// The ID of the created edge.
    pub fn add_correction(
        &mut self,
        start_pos: usize,
        end_pos: usize,
        word: &str,
        weight: W,
        metadata: EdgeMetadata,
    ) -> EdgeId {
        let source = self.get_or_create_node(start_pos);
        let target = self.get_or_create_node(end_pos);
        let label = self.backend.intern(word);
        let edge_id = EdgeId::new(self.edges.len() as u32);

        let edge = Edge::new(edge_id, source, target, label, weight, metadata);
        self.edges.push(edge);

        // Update node adjacency
        self.nodes[source.0 as usize].outgoing.push(edge_id);
        self.nodes[target.0 as usize].incoming.push(edge_id);

        edge_id
    }

    /// Add an edge by vocabulary ID (if word is already interned).
    pub fn add_correction_by_id(
        &mut self,
        start_pos: usize,
        end_pos: usize,
        label: crate::backend::VocabId,
        weight: W,
        metadata: EdgeMetadata,
    ) -> EdgeId {
        let source = self.get_or_create_node(start_pos);
        let target = self.get_or_create_node(end_pos);
        let edge_id = EdgeId::new(self.edges.len() as u32);

        let edge = Edge::new(edge_id, source, target, label, weight, metadata);
        self.edges.push(edge);

        // Update node adjacency
        self.nodes[source.0 as usize].outgoing.push(edge_id);
        self.nodes[target.0 as usize].incoming.push(edge_id);

        edge_id
    }

    /// Get access to the backend for vocabulary operations.
    #[inline]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Get mutable access to the backend.
    #[inline]
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Pre-intern multiple words for later use.
    pub fn intern_words<'a>(&mut self, words: impl IntoIterator<Item = &'a str>) {
        for word in words {
            self.backend.intern(word);
        }
    }

    /// Get the number of positions (nodes) currently in the builder.
    #[inline]
    pub fn num_positions(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges currently in the builder.
    #[inline]
    pub fn num_edges(&self) -> usize {
        self.edges.len()
    }

    /// Build the final lattice.
    ///
    /// # Arguments
    ///
    /// * `end_pos` - The final position (end node will be created if needed)
    ///
    /// # Returns
    ///
    /// The constructed lattice.
    pub fn build(mut self, end_pos: usize) -> Lattice<W, B> {
        // Ensure start and end nodes exist
        let start = self.get_or_create_node(0);
        let end = self.get_or_create_node(end_pos);

        // Sort nodes by position for proper topological order
        // (positions are already assigned, so we can use them directly)
        self.nodes.sort_by_key(|n| n.position.unwrap_or(usize::MAX));

        // Reassign node IDs after sorting
        let mut old_to_new: FxHashMap<NodeId, NodeId> = FxHashMap::default();
        for (new_id, node) in self.nodes.iter_mut().enumerate() {
            old_to_new.insert(node.id, NodeId::new(new_id as u32));
            node.id = NodeId::new(new_id as u32);
        }

        // Update edge source/target references
        for edge in &mut self.edges {
            edge.source = *old_to_new.get(&edge.source).expect("source exists");
            edge.target = *old_to_new.get(&edge.target).expect("target exists");
        }

        // Update start and end with new IDs
        let new_start = *old_to_new.get(&start).expect("start exists");
        let new_end = *old_to_new.get(&end).expect("end exists");

        Lattice::new(self.nodes, self.edges, new_start, new_end, self.backend)
    }

    /// Reserve capacity for additional edges.
    #[inline]
    pub fn reserve_edges(&mut self, additional: usize) {
        self.edges.reserve(additional);
    }

    /// Reserve capacity for additional positions.
    #[inline]
    pub fn reserve_positions(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_builder_new() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        assert_eq!(builder.num_positions(), 0);
        assert_eq!(builder.num_edges(), 0);
    }

    #[test]
    fn test_add_correction() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        let edge_id = builder.add_correction(
            0,
            1,
            "hello",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );

        assert_eq!(edge_id, EdgeId::new(0));
        assert_eq!(builder.num_positions(), 2); // 0 and 1
        assert_eq!(builder.num_edges(), 1);
    }

    #[test]
    fn test_multiple_corrections_same_position() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        builder.add_correction(
            0,
            1,
            "the",
            TropicalWeight::new(0.5),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            0,
            1,
            "teh",
            TropicalWeight::new(0.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            0,
            1,
            "tea",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );

        assert_eq!(builder.num_positions(), 2);
        assert_eq!(builder.num_edges(), 3);
    }

    #[test]
    fn test_build_simple() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        builder.add_correction(
            0,
            1,
            "hello",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "world",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );

        let lattice = builder.build(2);

        assert_eq!(lattice.num_nodes(), 3);
        assert_eq!(lattice.num_edges(), 2);
        assert_eq!(lattice.start(), NodeId::new(0));
        assert_eq!(lattice.end(), NodeId::new(2));
    }

    #[test]
    fn test_vocabulary_interning() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        builder.add_correction(
            0,
            1,
            "hello",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );
        builder.add_correction(
            1,
            2,
            "hello",
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );

        let lattice = builder.build(2);

        // Both edges should have the same label (interned)
        let labels: Vec<_> = lattice.edges().iter().map(|e| e.label).collect();
        assert_eq!(labels[0], labels[1]);

        // Backend should only have one entry
        assert_eq!(lattice.backend().vocab_size(), 1);
    }

    #[test]
    fn test_intern_words() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        builder.intern_words(["hello", "world", "test"]);

        assert_eq!(builder.backend().vocab_size(), 3);
    }

    #[test]
    fn test_add_correction_by_id() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        let id = builder.backend_mut().intern("hello");
        builder.add_correction_by_id(0, 1, id, TropicalWeight::new(1.0), EdgeMetadata::default());

        assert_eq!(builder.num_edges(), 1);
    }

    #[test]
    fn test_with_capacity() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> =
            LatticeBuilder::with_capacity(backend, 10, 5);

        // Should not panic with reserved capacity
        for i in 0..10 {
            for _ in 0..5 {
                builder.add_correction(
                    i,
                    i + 1,
                    "word",
                    TropicalWeight::new(1.0),
                    EdgeMetadata::default(),
                );
            }
        }

        assert_eq!(builder.num_positions(), 11);
        assert_eq!(builder.num_edges(), 50);
    }

    #[test]
    fn test_empty_build() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        assert_eq!(lattice.num_nodes(), 1);
        assert_eq!(lattice.num_edges(), 0);
        assert_eq!(lattice.start(), lattice.end());
    }

    #[test]
    fn test_node_adjacency() {
        let backend = HashMapBackend::new();
        let mut builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);

        builder.add_correction(0, 1, "a", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(0, 1, "b", TropicalWeight::new(1.0), EdgeMetadata::default());
        builder.add_correction(1, 2, "c", TropicalWeight::new(1.0), EdgeMetadata::default());

        let lattice = builder.build(2);

        // Start node has 2 outgoing, 0 incoming
        let start = lattice
            .node(NodeId::new(0))
            .expect("lattice/builder.rs: required value was None/Err");
        assert_eq!(start.out_degree(), 2);
        assert_eq!(start.in_degree(), 0);

        // Middle node has 2 incoming (from start), 1 outgoing
        let middle = lattice
            .node(NodeId::new(1))
            .expect("lattice/builder.rs: required value was None/Err");
        assert_eq!(middle.in_degree(), 2);
        assert_eq!(middle.out_degree(), 1);

        // End node has 1 incoming, 0 outgoing
        let end = lattice
            .node(NodeId::new(2))
            .expect("lattice/builder.rs: required value was None/Err");
        assert_eq!(end.in_degree(), 1);
        assert_eq!(end.out_degree(), 0);
    }
}
