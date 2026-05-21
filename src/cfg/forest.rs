//! Parse forest for representing ambiguous parses.
//!
//! A parse forest compactly represents all possible parse trees for
//! ambiguous input. It uses shared structure where possible.

use std::fmt;

use rustc_hash::FxHashSet;
use smallvec::SmallVec;

use super::types::RuleId;
use crate::lattice::{EdgeId, NodeId};

/// Forest node identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ForestNodeId(pub u32);

impl ForestNodeId {
    /// Create a new forest node ID.
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the ID value.
    pub fn id(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for ForestNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "F{}", self.0)
    }
}

/// A node in the parse forest.
#[derive(Clone, Debug)]
pub struct ForestNode {
    /// The production rule that created this node.
    pub rule: RuleId,
    /// Start position in the lattice.
    pub start: NodeId,
    /// End position in the lattice.
    pub end: NodeId,
    /// Children (for packed forests, multiple alternatives).
    pub children: SmallVec<[ForestChild; 4]>,
}

impl ForestNode {
    /// Create a new forest node.
    pub fn new(rule: RuleId, start: NodeId, end: NodeId) -> Self {
        Self {
            rule,
            start,
            end,
            children: SmallVec::new(),
        }
    }

    /// Add a child node.
    pub fn add_child(&mut self, child: ForestChild) {
        self.children.push(child);
    }

    /// Add children for a derivation.
    pub fn add_derivation(&mut self, children: SmallVec<[ForestNodeId; 4]>) {
        self.children.push(ForestChild::Derivation(children));
    }

    /// Add a terminal child (edge in the lattice).
    pub fn add_terminal(&mut self, edge: EdgeId) {
        self.children.push(ForestChild::Terminal(edge));
    }
}

/// A child in the parse forest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForestChild {
    /// A derivation (sequence of child forest nodes).
    Derivation(SmallVec<[ForestNodeId; 4]>),
    /// A terminal (edge in the lattice).
    Terminal(EdgeId),
}

/// A parse forest representing all parses of a lattice.
#[derive(Clone, Debug, Default)]
pub struct ParseForest {
    /// Nodes in the forest.
    nodes: Vec<ForestNode>,
    /// Root nodes (complete parses).
    roots: FxHashSet<ForestNodeId>,
}

impl ParseForest {
    /// Create a new empty forest.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node to the forest.
    pub fn add_node(&mut self, node: ForestNode) -> ForestNodeId {
        let id = ForestNodeId::new(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    /// Add a root node.
    pub fn add_root(&mut self, id: ForestNodeId) {
        self.roots.insert(id);
    }

    /// Get a node by ID.
    pub fn node(&self, id: ForestNodeId) -> Option<&ForestNode> {
        self.nodes.get(id.0 as usize)
    }

    /// Get a mutable node by ID.
    pub fn node_mut(&mut self, id: ForestNodeId) -> Option<&mut ForestNode> {
        self.nodes.get_mut(id.0 as usize)
    }

    /// Get all root nodes.
    pub fn roots(&self) -> impl Iterator<Item = ForestNodeId> + '_ {
        self.roots.iter().copied()
    }

    /// Check if the forest is empty (no parses).
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Get the number of nodes in the forest.
    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of root nodes (complete parses).
    pub fn num_roots(&self) -> usize {
        self.roots.len()
    }

    /// Extract the best parse tree.
    ///
    /// Returns the first root (for now - could be extended with weights).
    pub fn best_parse(&self) -> Option<ParseTree> {
        self.roots().next().and_then(|root| self.extract_tree(root))
    }

    /// Extract all parse trees (up to a limit).
    pub fn all_parses(&self, limit: usize) -> Vec<ParseTree> {
        let mut trees = Vec::new();

        for root in self.roots() {
            if trees.len() >= limit {
                break;
            }
            if let Some(tree) = self.extract_tree(root) {
                trees.push(tree);
            }
        }

        trees
    }

    /// Extract a single parse tree from a forest node.
    fn extract_tree(&self, root: ForestNodeId) -> Option<ParseTree> {
        let node = self.node(root)?;

        let mut tree = ParseTree {
            rule: node.rule,
            start: node.start,
            end: node.end,
            children: Vec::new(),
        };

        // Extract first derivation
        for child in &node.children {
            match child {
                ForestChild::Derivation(kids) => {
                    for &kid_id in kids {
                        if let Some(kid_tree) = self.extract_tree(kid_id) {
                            tree.children.push(ParseTreeChild::Tree(Box::new(kid_tree)));
                        }
                    }
                    break; // Take first derivation
                }
                ForestChild::Terminal(edge) => {
                    tree.children.push(ParseTreeChild::Terminal(*edge));
                }
            }
        }

        Some(tree)
    }

    /// Collect all edges used in any parse.
    pub fn collect_used_edges(&self) -> FxHashSet<EdgeId> {
        let mut edges = FxHashSet::default();

        fn collect(forest: &ParseForest, node_id: ForestNodeId, edges: &mut FxHashSet<EdgeId>) {
            if let Some(node) = forest.node(node_id) {
                for child in &node.children {
                    match child {
                        ForestChild::Derivation(kids) => {
                            for &kid_id in kids {
                                collect(forest, kid_id, edges);
                            }
                        }
                        ForestChild::Terminal(edge) => {
                            edges.insert(*edge);
                        }
                    }
                }
            }
        }

        for root in self.roots() {
            collect(self, root, &mut edges);
        }

        edges
    }
}

/// A single parse tree.
#[derive(Clone, Debug)]
pub struct ParseTree {
    /// The production rule at this node.
    pub rule: RuleId,
    /// Start position in lattice.
    pub start: NodeId,
    /// End position in lattice.
    pub end: NodeId,
    /// Children of this node.
    pub children: Vec<ParseTreeChild>,
}

/// A child in a parse tree.
#[derive(Clone, Debug)]
pub enum ParseTreeChild {
    /// A subtree.
    Tree(Box<ParseTree>),
    /// A terminal (edge in the lattice).
    Terminal(EdgeId),
}

impl ParseTree {
    /// Get the depth of the tree.
    pub fn depth(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(|c| match c {
                ParseTreeChild::Tree(t) => t.depth(),
                ParseTreeChild::Terminal(_) => 0,
            })
            .max()
            .unwrap_or(0)
    }

    /// Get the number of nodes in the tree.
    pub fn size(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(|c| match c {
                ParseTreeChild::Tree(t) => t.size(),
                ParseTreeChild::Terminal(_) => 1,
            })
            .sum::<usize>()
    }

    /// Collect all edges in this tree.
    pub fn edges(&self) -> Vec<EdgeId> {
        let mut result = Vec::new();
        self.collect_edges(&mut result);
        result
    }

    fn collect_edges(&self, result: &mut Vec<EdgeId>) {
        for child in &self.children {
            match child {
                ParseTreeChild::Tree(t) => t.collect_edges(result),
                ParseTreeChild::Terminal(e) => result.push(*e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forest_node_id() {
        let id = ForestNodeId::new(5);
        assert_eq!(id.id(), 5);
        assert_eq!(format!("{}", id), "F5");
    }

    #[test]
    fn test_forest_node() {
        let mut node = ForestNode::new(RuleId::new(0), NodeId(0), NodeId(2));
        assert_eq!(node.rule, RuleId::new(0));
        assert!(node.children.is_empty());

        node.add_terminal(EdgeId(1));
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn test_parse_forest_creation() {
        let mut forest = ParseForest::new();
        assert!(forest.is_empty());

        let node = ForestNode::new(RuleId::new(0), NodeId(0), NodeId(1));
        let id = forest.add_node(node);
        forest.add_root(id);

        assert!(!forest.is_empty());
        assert_eq!(forest.num_nodes(), 1);
        assert_eq!(forest.num_roots(), 1);
    }

    #[test]
    fn test_best_parse() {
        let mut forest = ParseForest::new();

        // Create a simple tree: S → a b
        let mut root = ForestNode::new(RuleId::new(0), NodeId(0), NodeId(2));
        root.add_terminal(EdgeId(0));
        root.add_terminal(EdgeId(1));

        let root_id = forest.add_node(root);
        forest.add_root(root_id);

        let tree = forest.best_parse().expect("should have parse");
        assert_eq!(tree.rule, RuleId::new(0));
        assert_eq!(tree.children.len(), 2);
    }

    #[test]
    fn test_parse_tree_metrics() {
        let tree = ParseTree {
            rule: RuleId::new(0),
            start: NodeId(0),
            end: NodeId(3),
            children: vec![
                ParseTreeChild::Tree(Box::new(ParseTree {
                    rule: RuleId::new(1),
                    start: NodeId(0),
                    end: NodeId(1),
                    children: vec![ParseTreeChild::Terminal(EdgeId(0))],
                })),
                ParseTreeChild::Terminal(EdgeId(1)),
            ],
        };

        assert_eq!(tree.depth(), 2);
        assert_eq!(tree.size(), 4); // root + child tree + 2 terminals
        assert_eq!(tree.edges().len(), 2);
    }

    #[test]
    fn test_collect_used_edges() {
        let mut forest = ParseForest::new();

        let mut root = ForestNode::new(RuleId::new(0), NodeId(0), NodeId(2));
        root.add_terminal(EdgeId(0));
        root.add_terminal(EdgeId(1));

        let root_id = forest.add_node(root);
        forest.add_root(root_id);

        let edges = forest.collect_used_edges();
        assert_eq!(edges.len(), 2);
        assert!(edges.contains(&EdgeId(0)));
        assert!(edges.contains(&EdgeId(1)));
    }

    #[test]
    fn test_all_parses() {
        let mut forest = ParseForest::new();

        // Add two root nodes
        let root1 = ForestNode::new(RuleId::new(0), NodeId(0), NodeId(1));
        let root2 = ForestNode::new(RuleId::new(1), NodeId(0), NodeId(1));

        let id1 = forest.add_node(root1);
        let id2 = forest.add_node(root2);
        forest.add_root(id1);
        forest.add_root(id2);

        let trees = forest.all_parses(10);
        assert_eq!(trees.len(), 2);
    }
}
