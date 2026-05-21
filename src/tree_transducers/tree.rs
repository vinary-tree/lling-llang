//! Tree data structures for tree transducers.

use std::fmt::{self, Display};
use std::hash::Hash;

/// A tree node with a label and children.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TreeNode<L> {
    /// The node label.
    pub label: L,
    /// Child nodes (empty for leaves).
    pub children: Vec<Tree<L>>,
}

/// A tree structure for tree transducers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tree<L>(pub TreeNode<L>);

impl<L> Tree<L> {
    /// Create a leaf node (no children).
    pub fn leaf(label: L) -> Self {
        Tree(TreeNode {
            label,
            children: Vec::new(),
        })
    }

    /// Create an internal node with children.
    pub fn node(label: L, children: Vec<Tree<L>>) -> Self {
        Tree(TreeNode { label, children })
    }

    /// Get the label of the root node.
    pub fn label(&self) -> &L {
        &self.0.label
    }

    /// Get the children of the root node.
    pub fn children(&self) -> &[Tree<L>] {
        &self.0.children
    }

    /// Get mutable access to children.
    pub fn children_mut(&mut self) -> &mut Vec<Tree<L>> {
        &mut self.0.children
    }

    /// Check if this is a leaf node.
    pub fn is_leaf(&self) -> bool {
        self.0.children.is_empty()
    }

    /// Get the arity (number of children).
    pub fn arity(&self) -> usize {
        self.0.children.len()
    }

    /// Get the depth of the tree.
    pub fn depth(&self) -> usize {
        if self.is_leaf() {
            1
        } else {
            1 + self.children().iter().map(|c| c.depth()).max().unwrap_or(0)
        }
    }

    /// Get the total number of nodes in the tree.
    pub fn size(&self) -> usize {
        1 + self.children().iter().map(|c| c.size()).sum::<usize>()
    }

    /// Map a function over all labels.
    pub fn map<F, M>(&self, f: &F) -> Tree<M>
    where
        F: Fn(&L) -> M,
    {
        Tree::node(
            f(&self.0.label),
            self.children().iter().map(|c| c.map(f)).collect(),
        )
    }

    /// Iterate over all nodes in pre-order.
    pub fn preorder(&self) -> impl Iterator<Item = &Tree<L>> {
        PreorderIterator::new(self)
    }
}

impl<L: Clone> Tree<L> {
    /// Create a copy of the subtree at the given path.
    ///
    /// The path is a sequence of child indices.
    pub fn subtree(&self, path: &[usize]) -> Option<Tree<L>> {
        if path.is_empty() {
            return Some(self.clone());
        }

        let first = path[0];
        if first >= self.arity() {
            return None;
        }

        self.children()[first].subtree(&path[1..])
    }

    /// Replace the subtree at the given path.
    pub fn replace(&self, path: &[usize], replacement: Tree<L>) -> Option<Tree<L>> {
        if path.is_empty() {
            return Some(replacement);
        }

        let first = path[0];
        if first >= self.arity() {
            return None;
        }

        let mut new_children = self.children().to_vec();
        new_children[first] = self.children()[first].replace(&path[1..], replacement)?;

        Some(Tree::node(self.label().clone(), new_children))
    }
}

impl<L: Display> Display for Tree<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_leaf() {
            write!(f, "{}", self.label())
        } else {
            write!(f, "{}(", self.label())?;
            for (i, child) in self.children().iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", child)?;
            }
            write!(f, ")")
        }
    }
}

/// Pre-order iterator over tree nodes.
struct PreorderIterator<'a, L> {
    stack: Vec<&'a Tree<L>>,
}

impl<'a, L> PreorderIterator<'a, L> {
    fn new(root: &'a Tree<L>) -> Self {
        Self { stack: vec![root] }
    }
}

impl<'a, L> Iterator for PreorderIterator<'a, L> {
    type Item = &'a Tree<L>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;

        // Push children in reverse order so leftmost is processed first
        for child in node.children().iter().rev() {
            self.stack.push(child);
        }

        Some(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_creation() {
        let leaf: Tree<&str> = Tree::leaf("a");
        assert!(leaf.is_leaf());
        assert_eq!(leaf.arity(), 0);
        assert_eq!(leaf.label(), &"a");
    }

    #[test]
    fn test_node_creation() {
        let tree = Tree::node("S", vec![Tree::leaf("NP"), Tree::leaf("VP")]);

        assert!(!tree.is_leaf());
        assert_eq!(tree.arity(), 2);
        assert_eq!(tree.label(), &"S");
    }

    #[test]
    fn test_depth() {
        let leaf: Tree<&str> = Tree::leaf("a");
        assert_eq!(leaf.depth(), 1);

        let tree = Tree::node(
            "S",
            vec![
                Tree::node("NP", vec![Tree::leaf("Det"), Tree::leaf("N")]),
                Tree::leaf("VP"),
            ],
        );
        assert_eq!(tree.depth(), 3);
    }

    #[test]
    fn test_size() {
        let leaf: Tree<&str> = Tree::leaf("a");
        assert_eq!(leaf.size(), 1);

        let tree = Tree::node(
            "S",
            vec![
                Tree::node("NP", vec![Tree::leaf("Det"), Tree::leaf("N")]),
                Tree::leaf("VP"),
            ],
        );
        assert_eq!(tree.size(), 5);
    }

    #[test]
    fn test_map() {
        let tree = Tree::node("abc", vec![Tree::leaf("de"), Tree::leaf("f")]);

        let mapped = tree.map(&|s: &&str| s.len());

        assert_eq!(mapped.label(), &3);
        assert_eq!(mapped.children()[0].label(), &2);
        assert_eq!(mapped.children()[1].label(), &1);
    }

    #[test]
    fn test_subtree() {
        let tree = Tree::node(
            "S",
            vec![
                Tree::node("NP", vec![Tree::leaf("the"), Tree::leaf("cat")]),
                Tree::leaf("VP"),
            ],
        );

        let subtree = tree.subtree(&[0]).unwrap();
        assert_eq!(subtree.label(), &"NP");

        let leaf = tree.subtree(&[0, 1]).unwrap();
        assert_eq!(leaf.label(), &"cat");

        assert!(tree.subtree(&[5]).is_none());
    }

    #[test]
    fn test_replace() {
        let tree = Tree::node("S", vec![Tree::leaf("NP"), Tree::leaf("VP")]);

        let replaced = tree.replace(&[0], Tree::leaf("PP")).unwrap();
        assert_eq!(replaced.children()[0].label(), &"PP");
    }

    #[test]
    fn test_display() {
        let leaf: Tree<&str> = Tree::leaf("x");
        assert_eq!(format!("{}", leaf), "x");

        let tree = Tree::node("S", vec![Tree::leaf("NP"), Tree::leaf("VP")]);
        assert_eq!(format!("{}", tree), "S(NP, VP)");
    }

    #[test]
    fn test_preorder() {
        let tree = Tree::node("S", vec![Tree::leaf("NP"), Tree::leaf("VP")]);

        let labels: Vec<_> = tree.preorder().map(|t| t.label()).collect();
        assert_eq!(labels, vec![&"S", &"NP", &"VP"]);
    }
}
