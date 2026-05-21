//! Core types for lattice representation.

use smallvec::SmallVec;

use crate::backend::VocabId;
use crate::semiring::Semiring;

/// A node identifier in a lattice.
///
/// Nodes represent positions in the input sequence. Node 0 is typically
/// the start node, and the last node is the end node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Create a new node ID.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw ID value.
    #[inline]
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl From<u32> for NodeId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<usize> for NodeId {
    fn from(id: usize) -> Self {
        Self(id as u32)
    }
}

/// An edge identifier in a lattice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeId(pub u32);

impl EdgeId {
    /// Create a new edge ID.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw ID value.
    #[inline]
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl From<u32> for EdgeId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<usize> for EdgeId {
    fn from(id: usize) -> Self {
        Self(id as u32)
    }
}

/// A node in a lattice.
///
/// Each node represents a position in the input sequence and tracks
/// its incoming and outgoing edges.
#[derive(Clone, Debug)]
pub struct Node {
    /// The node's identifier.
    pub id: NodeId,
    /// Outgoing edges from this node.
    pub outgoing: SmallVec<[EdgeId; 8]>,
    /// Incoming edges to this node.
    pub incoming: SmallVec<[EdgeId; 8]>,
    /// Position in the input sequence (if known).
    ///
    /// For lattices built from token sequences, this is the token index.
    /// May be `None` for nodes created by epsilon transitions or composition.
    pub position: Option<usize>,
}

impl Node {
    /// Create a new node with no edges.
    #[inline]
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            outgoing: SmallVec::new(),
            incoming: SmallVec::new(),
            position: None,
        }
    }

    /// Create a new node with a position.
    #[inline]
    pub fn with_position(id: NodeId, position: usize) -> Self {
        Self {
            id,
            outgoing: SmallVec::new(),
            incoming: SmallVec::new(),
            position: Some(position),
        }
    }

    /// Check if this node has any outgoing edges.
    #[inline]
    pub fn has_outgoing(&self) -> bool {
        !self.outgoing.is_empty()
    }

    /// Check if this node has any incoming edges.
    #[inline]
    pub fn has_incoming(&self) -> bool {
        !self.incoming.is_empty()
    }

    /// Get the number of outgoing edges.
    #[inline]
    pub fn out_degree(&self) -> usize {
        self.outgoing.len()
    }

    /// Get the number of incoming edges.
    #[inline]
    pub fn in_degree(&self) -> usize {
        self.incoming.len()
    }
}

/// An edge in a lattice.
///
/// Each edge represents a token alternative with an associated weight.
/// The label references interned vocabulary via `VocabId`.
#[derive(Clone, Debug)]
pub struct Edge<W: Semiring> {
    /// The edge's identifier.
    pub id: EdgeId,
    /// Source node.
    pub source: NodeId,
    /// Target node.
    pub target: NodeId,
    /// Label (vocabulary ID referencing the word).
    pub label: VocabId,
    /// Weight of this edge.
    pub weight: W,
    /// Additional metadata about this edge.
    pub metadata: EdgeMetadata,
}

impl<W: Semiring> Edge<W> {
    /// Create a new edge.
    #[inline]
    pub fn new(
        id: EdgeId,
        source: NodeId,
        target: NodeId,
        label: VocabId,
        weight: W,
        metadata: EdgeMetadata,
    ) -> Self {
        Self {
            id,
            source,
            target,
            label,
            weight,
            metadata,
        }
    }

    /// Create an edge with default metadata.
    #[inline]
    pub fn simple(id: EdgeId, source: NodeId, target: NodeId, label: VocabId, weight: W) -> Self {
        Self::new(id, source, target, label, weight, EdgeMetadata::default())
    }
}

/// Metadata associated with a lattice edge.
///
/// Provides additional information about how an edge was created and
/// what kind of correction it represents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EdgeMetadata {
    /// Edit distance from original token (if computed).
    pub edit_distance: Option<u8>,
    /// Whether this edge was generated via phonetic matching.
    pub is_phonetic: bool,
    /// Rule ID that generated this edge (for grammar-based corrections).
    pub rule_id: Option<u32>,
    /// Whether this is the original token (no correction).
    pub is_original: bool,
    /// Source layer that produced this edge.
    pub source_layer: Option<u8>,
}

impl EdgeMetadata {
    /// Create metadata for an original (uncorrected) token.
    #[inline]
    pub fn original() -> Self {
        Self {
            is_original: true,
            edit_distance: Some(0),
            ..Default::default()
        }
    }

    /// Create metadata for a correction with edit distance.
    #[inline]
    pub fn correction(edit_distance: u8) -> Self {
        Self {
            edit_distance: Some(edit_distance),
            is_original: false,
            ..Default::default()
        }
    }

    /// Create metadata for a phonetic match.
    #[inline]
    pub fn phonetic() -> Self {
        Self {
            is_phonetic: true,
            is_original: false,
            ..Default::default()
        }
    }

    /// Create metadata for a grammar rule application.
    #[inline]
    pub fn grammar_rule(rule_id: u32) -> Self {
        Self {
            rule_id: Some(rule_id),
            is_original: false,
            ..Default::default()
        }
    }

    /// Set the source layer.
    #[inline]
    pub fn with_layer(mut self, layer: u8) -> Self {
        self.source_layer = Some(layer);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_node_id() {
        let id = NodeId::new(42);
        assert_eq!(id.value(), 42);
        assert_eq!(id, NodeId::from(42u32));
        assert_eq!(id, NodeId::from(42usize));
    }

    #[test]
    fn test_edge_id() {
        let id = EdgeId::new(42);
        assert_eq!(id.value(), 42);
        assert_eq!(id, EdgeId::from(42u32));
        assert_eq!(id, EdgeId::from(42usize));
    }

    #[test]
    fn test_node_creation() {
        let node = Node::new(NodeId::new(0));
        assert_eq!(node.id, NodeId::new(0));
        assert!(!node.has_outgoing());
        assert!(!node.has_incoming());
        assert_eq!(node.out_degree(), 0);
        assert_eq!(node.in_degree(), 0);
        assert_eq!(node.position, None);
    }

    #[test]
    fn test_node_with_position() {
        let node = Node::with_position(NodeId::new(1), 5);
        assert_eq!(node.id, NodeId::new(1));
        assert_eq!(node.position, Some(5));
    }

    #[test]
    fn test_edge_creation() {
        let edge: Edge<TropicalWeight> = Edge::new(
            EdgeId::new(0),
            NodeId::new(0),
            NodeId::new(1),
            42,
            TropicalWeight::new(1.0),
            EdgeMetadata::default(),
        );

        assert_eq!(edge.id, EdgeId::new(0));
        assert_eq!(edge.source, NodeId::new(0));
        assert_eq!(edge.target, NodeId::new(1));
        assert_eq!(edge.label, 42);
        assert_eq!(edge.weight.value(), 1.0);
    }

    #[test]
    fn test_edge_simple() {
        let edge: Edge<TropicalWeight> = Edge::simple(
            EdgeId::new(0),
            NodeId::new(0),
            NodeId::new(1),
            42,
            TropicalWeight::new(1.0),
        );

        assert!(!edge.metadata.is_original);
        assert_eq!(edge.metadata.edit_distance, None);
    }

    #[test]
    fn test_edge_metadata_original() {
        let meta = EdgeMetadata::original();
        assert!(meta.is_original);
        assert_eq!(meta.edit_distance, Some(0));
        assert!(!meta.is_phonetic);
    }

    #[test]
    fn test_edge_metadata_correction() {
        let meta = EdgeMetadata::correction(2);
        assert!(!meta.is_original);
        assert_eq!(meta.edit_distance, Some(2));
    }

    #[test]
    fn test_edge_metadata_phonetic() {
        let meta = EdgeMetadata::phonetic();
        assert!(!meta.is_original);
        assert!(meta.is_phonetic);
    }

    #[test]
    fn test_edge_metadata_grammar_rule() {
        let meta = EdgeMetadata::grammar_rule(42);
        assert!(!meta.is_original);
        assert_eq!(meta.rule_id, Some(42));
    }

    #[test]
    fn test_edge_metadata_with_layer() {
        let meta = EdgeMetadata::correction(1).with_layer(2);
        assert_eq!(meta.edit_distance, Some(1));
        assert_eq!(meta.source_layer, Some(2));
    }
}
