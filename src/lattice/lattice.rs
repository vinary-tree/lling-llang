//! Lattice data structure implementation.

use crate::backend::{LatticeBackend, VocabId};
use crate::semiring::Semiring;
use super::types::{NodeId, EdgeId, Node, Edge};

/// A weighted directed acyclic graph (DAG) representing correction alternatives.
///
/// A lattice provides:
/// - Efficient storage of multiple correction paths
/// - Weight-based path ranking
/// - Vocabulary interning via backend
/// - Topological ordering for DP algorithms
///
/// # Type Parameters
///
/// - `W`: Weight type (semiring)
/// - `B`: Backend for vocabulary storage
///
/// # Example
///
/// ```rust
/// use lling_llang::lattice::{LatticeBuilder, EdgeMetadata};
/// use lling_llang::backend::HashMapBackend;
/// use lling_llang::semiring::TropicalWeight;
///
/// let backend = HashMapBackend::new();
/// let mut builder = LatticeBuilder::<TropicalWeight, _>::new(backend);
///
/// // Build lattice for "teh" with corrections
/// builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
/// builder.add_correction(0, 1, "teh", TropicalWeight::new(0.0), EdgeMetadata::original());
///
/// let lattice = builder.build(1);
/// assert_eq!(lattice.num_nodes(), 2);
/// assert_eq!(lattice.num_edges(), 2);
/// ```
#[derive(Clone, Debug)]
pub struct Lattice<W: Semiring, B: LatticeBackend> {
    /// All nodes in the lattice.
    nodes: Vec<Node>,
    /// All edges in the lattice.
    edges: Vec<Edge<W>>,
    /// Start node ID.
    start: NodeId,
    /// End node ID.
    end: NodeId,
    /// Backend for vocabulary storage.
    backend: B,
    /// Cached topological order (computed on demand).
    topo_order: Option<Vec<NodeId>>,
}

impl<W: Semiring, B: LatticeBackend> Lattice<W, B> {
    /// Create a new lattice with the given nodes, edges, start, end, and backend.
    ///
    /// This is typically called by `LatticeBuilder::build()`.
    pub(crate) fn new(
        nodes: Vec<Node>,
        edges: Vec<Edge<W>>,
        start: NodeId,
        end: NodeId,
        backend: B,
    ) -> Self {
        Self {
            nodes,
            edges,
            start,
            end,
            backend,
            topo_order: None,
        }
    }

    /// Get the start node ID.
    #[inline]
    pub fn start(&self) -> NodeId {
        self.start
    }

    /// Get the end node ID.
    #[inline]
    pub fn end(&self) -> NodeId {
        self.end
    }

    /// Get the number of nodes.
    #[inline]
    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges.
    #[inline]
    pub fn num_edges(&self) -> usize {
        self.edges.len()
    }

    /// Check if the lattice is empty (no edges).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Get a node by ID.
    #[inline]
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0 as usize)
    }

    /// Get an edge by ID.
    #[inline]
    pub fn edge(&self, id: EdgeId) -> Option<&Edge<W>> {
        self.edges.get(id.0 as usize)
    }

    /// Get all nodes.
    #[inline]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Get all edges.
    #[inline]
    pub fn edges(&self) -> &[Edge<W>] {
        &self.edges
    }

    /// Get the outgoing edges from a node.
    pub fn outgoing_edges(&self, node: NodeId) -> impl Iterator<Item = &Edge<W>> {
        self.nodes
            .get(node.0 as usize)
            .into_iter()
            .flat_map(|n| n.outgoing.iter())
            .filter_map(|&eid| self.edges.get(eid.0 as usize))
    }

    /// Get the incoming edges to a node.
    pub fn incoming_edges(&self, node: NodeId) -> impl Iterator<Item = &Edge<W>> {
        self.nodes
            .get(node.0 as usize)
            .into_iter()
            .flat_map(|n| n.incoming.iter())
            .filter_map(|&eid| self.edges.get(eid.0 as usize))
    }

    /// Look up a word by vocabulary ID.
    #[inline]
    pub fn word(&self, id: VocabId) -> Option<&str> {
        self.backend.lookup(id)
    }

    /// Get the edge label as a string.
    #[inline]
    pub fn edge_word(&self, edge: &Edge<W>) -> Option<&str> {
        self.word(edge.label)
    }

    /// Get the underlying backend.
    #[inline]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Get mutable access to the backend.
    #[inline]
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Get the topological order of nodes.
    ///
    /// Computes and caches the order on first call. Returns `None` if the
    /// graph contains a cycle.
    pub fn topological_order(&mut self) -> Option<&[NodeId]> {
        if self.topo_order.is_none() {
            self.topo_order = super::algorithms::topological_sort(&self.nodes, &self.edges);
        }
        self.topo_order.as_deref()
    }

    /// Check if the lattice is acyclic.
    ///
    /// A proper lattice should always be acyclic.
    pub fn is_acyclic(&self) -> bool {
        super::algorithms::is_acyclic(&self.nodes, &self.edges)
    }

    /// Count the number of distinct paths from start to end.
    ///
    /// Uses dynamic programming for efficiency. May return `None` if
    /// the count would overflow.
    pub fn path_count(&mut self) -> Option<usize> {
        super::algorithms::count_paths(self)
    }

    /// Shrink internal storage to fit current size.
    pub fn shrink_to_fit(&mut self) {
        self.nodes.shrink_to_fit();
        self.edges.shrink_to_fit();
        for node in &mut self.nodes {
            node.outgoing.shrink_to_fit();
            node.incoming.shrink_to_fit();
        }
    }

    /// Iterate over node IDs in the lattice.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        (0..self.nodes.len() as u32).map(NodeId)
    }

    /// Iterate over edge IDs in the lattice.
    pub fn edge_ids(&self) -> impl Iterator<Item = EdgeId> + '_ {
        (0..self.edges.len() as u32).map(EdgeId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;
    use crate::lattice::builder::LatticeBuilder;
    use crate::lattice::types::EdgeMetadata;
    use crate::semiring::TropicalWeight;

    fn sample_lattice() -> Lattice<TropicalWeight, HashMapBackend> {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        // Token 0: "teh" -> "the" (edit 1), "teh" (original)
        builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
        builder.add_correction(0, 1, "teh", TropicalWeight::new(0.0), EdgeMetadata::original());

        // Token 1: "quik" -> "quick" (edit 1), "quik" (original)
        builder.add_correction(1, 2, "quick", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
        builder.add_correction(1, 2, "quik", TropicalWeight::new(0.0), EdgeMetadata::original());

        builder.build(2)
    }

    #[test]
    fn test_lattice_structure() {
        let lattice = sample_lattice();

        assert_eq!(lattice.num_nodes(), 3); // 0, 1, 2
        assert_eq!(lattice.num_edges(), 4); // 2 edges per position
        assert_eq!(lattice.start(), NodeId::new(0));
        assert_eq!(lattice.end(), NodeId::new(2));
    }

    #[test]
    fn test_node_access() {
        let lattice = sample_lattice();

        let start = lattice.node(NodeId::new(0)).expect("start node exists");
        assert_eq!(start.out_degree(), 2);
        assert_eq!(start.in_degree(), 0);

        let middle = lattice.node(NodeId::new(1)).expect("middle node exists");
        assert_eq!(middle.out_degree(), 2);
        assert_eq!(middle.in_degree(), 2);

        let end = lattice.node(NodeId::new(2)).expect("end node exists");
        assert_eq!(end.out_degree(), 0);
        assert_eq!(end.in_degree(), 2);
    }

    #[test]
    fn test_word_lookup() {
        let lattice = sample_lattice();

        // Look up edge labels
        for edge in lattice.edges() {
            let word = lattice.word(edge.label);
            assert!(word.is_some());
        }
    }

    #[test]
    fn test_outgoing_edges() {
        let lattice = sample_lattice();

        let edges: Vec<_> = lattice.outgoing_edges(NodeId::new(0)).collect();
        assert_eq!(edges.len(), 2);

        let words: Vec<_> = edges.iter()
            .filter_map(|e| lattice.word(e.label))
            .collect();
        assert!(words.contains(&"the"));
        assert!(words.contains(&"teh"));
    }

    #[test]
    fn test_is_acyclic() {
        let lattice = sample_lattice();
        assert!(lattice.is_acyclic());
    }

    #[test]
    fn test_topological_order() {
        let mut lattice = sample_lattice();

        let order = lattice.topological_order().expect("acyclic lattice");
        assert_eq!(order.len(), 3);

        // Start should come before middle, middle before end
        let start_pos = order.iter().position(|&n| n == NodeId::new(0)).unwrap();
        let middle_pos = order.iter().position(|&n| n == NodeId::new(1)).unwrap();
        let end_pos = order.iter().position(|&n| n == NodeId::new(2)).unwrap();

        assert!(start_pos < middle_pos);
        assert!(middle_pos < end_pos);
    }

    #[test]
    fn test_path_count() {
        let mut lattice = sample_lattice();

        // 2 choices at position 0, 2 choices at position 1 = 4 paths
        let count = lattice.path_count();
        assert_eq!(count, Some(4));
    }

    #[test]
    fn test_empty_lattice() {
        let backend = HashMapBackend::new();
        let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
        let lattice = builder.build(0);

        assert!(lattice.is_empty());
        assert_eq!(lattice.num_nodes(), 1); // Just start/end node
    }

    #[test]
    fn test_node_ids_iterator() {
        let lattice = sample_lattice();
        let ids: Vec<_> = lattice.node_ids().collect();
        assert_eq!(ids, vec![NodeId::new(0), NodeId::new(1), NodeId::new(2)]);
    }

    #[test]
    fn test_edge_ids_iterator() {
        let lattice = sample_lattice();
        let ids: Vec<_> = lattice.edge_ids().collect();
        assert_eq!(ids.len(), 4);
    }
}
