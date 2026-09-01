//! Tree data structures for tree transducers.

use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};

/// A tree node with a label and children.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TreeNode<L> {
    /// The node label.
    pub label: L,
    /// Child nodes (empty for leaves).
    pub children: Vec<Tree<L>>,
}

/// A tree structure for tree transducers.
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
        let mut maximum = 0usize;
        let mut pending = vec![(self, 1usize)];
        while let Some((node, depth)) = pending.pop() {
            maximum = maximum.max(depth);
            pending.extend(node.children().iter().rev().map(|child| (child, depth + 1)));
        }
        maximum
    }

    /// Get the total number of nodes in the tree.
    pub fn size(&self) -> usize {
        let mut size = 0usize;
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            size += 1;
            pending.extend(node.children().iter().rev());
        }
        size
    }

    /// Map a function over all labels.
    pub fn map<F, M>(&self, f: &F) -> Tree<M>
    where
        F: Fn(&L) -> M,
    {
        struct Frame<'a, L, M> {
            source: &'a Tree<L>,
            next_child: usize,
            mapped_label: Option<M>,
            mapped_children: Vec<Tree<M>>,
        }

        impl<'a, L, M> Frame<'a, L, M> {
            fn new<F>(source: &'a Tree<L>, f: &F) -> Self
            where
                F: Fn(&L) -> M,
            {
                Self {
                    source,
                    next_child: 0,
                    mapped_label: Some(f(source.label())),
                    mapped_children: Vec::with_capacity(source.arity()),
                }
            }
        }

        let mut frames = vec![Frame::new(self, f)];
        loop {
            let frame = frames
                .last_mut()
                .expect("the root mapping frame remains until a result is produced");
            if let Some(child) = frame.source.children().get(frame.next_child) {
                frame.next_child += 1;
                frames.push(Frame::new(child, f));
                continue;
            }

            let mut completed = frames.pop().expect("a completed mapping frame is present");
            let mapped = Tree::node(
                completed
                    .mapped_label
                    .take()
                    .expect("each mapping frame owns one mapped label"),
                completed.mapped_children,
            );
            if let Some(parent) = frames.last_mut() {
                parent.mapped_children.push(mapped);
            } else {
                return mapped;
            }
        }
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
        let mut node = self;
        for &child_index in path {
            node = node.children().get(child_index)?;
        }
        Some(node.clone())
    }

    /// Replace the subtree at the given path.
    pub fn replace(&self, path: &[usize], replacement: Tree<L>) -> Option<Tree<L>> {
        if path.is_empty() {
            return Some(replacement);
        }

        let mut ancestors = Vec::with_capacity(path.len());
        let mut node = self;
        for &child_index in path {
            if child_index >= node.arity() {
                return None;
            }
            ancestors.push((node, child_index));
            node = &node.children()[child_index];
        }

        let mut rebuilt = Some(replacement);
        while let Some((ancestor, replaced_index)) = ancestors.pop() {
            let mut children = Vec::with_capacity(ancestor.arity());
            for (index, child) in ancestor.children().iter().enumerate() {
                if index == replaced_index {
                    children.push(
                        rebuilt
                            .take()
                            .expect("exactly one child is replaced at each path level"),
                    );
                } else {
                    children.push(child.clone());
                }
            }
            rebuilt = Some(Tree::node(ancestor.label().clone(), children));
        }
        rebuilt
    }
}

impl<L: Clone> Clone for Tree<L> {
    fn clone(&self) -> Self {
        self.map(&L::clone)
    }
}

impl<L: PartialEq> PartialEq for Tree<L> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            if left.label() != right.label() || left.arity() != right.arity() {
                return false;
            }
            pending.extend(left.children().iter().zip(right.children()).rev());
        }
        true
    }
}

impl<L: Eq> Eq for Tree<L> {}

impl<L: Hash> Hash for Tree<L> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            node.label().hash(state);
            node.arity().hash(state);
            pending.extend(node.children().iter().rev());
        }
    }
}

impl<L: Debug> Debug for Tree<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, L> {
            Node(&'a Tree<L>),
            Children(&'a [Tree<L>], usize),
        }

        let mut events = vec![Event::Node(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Node(node) => {
                    write!(f, "Tree(TreeNode {{ label: {:?}, children: [", node.label())?;
                    events.push(Event::Children(node.children(), 0));
                }
                Event::Children(children, index) => {
                    if index == children.len() {
                        write!(f, "] }})")?;
                    } else {
                        if index > 0 {
                            write!(f, ", ")?;
                        }
                        events.push(Event::Children(children, index + 1));
                        events.push(Event::Node(&children[index]));
                    }
                }
            }
        }
        Ok(())
    }
}

impl<L> Drop for Tree<L> {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.0.children);
        while let Some(mut child) = pending.pop() {
            pending.append(&mut child.0.children);
        }
    }
}

impl<L: Display> Display for Tree<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, L> {
            Node(&'a Tree<L>),
            Children(&'a [Tree<L>], usize),
        }

        let mut events = vec![Event::Node(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Node(node) => {
                    write!(f, "{}", node.label())?;
                    if !node.is_leaf() {
                        write!(f, "(")?;
                        events.push(Event::Children(node.children(), 0));
                    }
                }
                Event::Children(children, index) => {
                    if index == children.len() {
                        write!(f, ")")?;
                    } else {
                        if index > 0 {
                            write!(f, ", ")?;
                        }
                        events.push(Event::Children(children, index + 1));
                        events.push(Event::Node(&children[index]));
                    }
                }
            }
        }
        Ok(())
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
    use proptest::prelude::*;

    const DEEP_TREE_PATH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    fn unary_tree(edge_depth: usize) -> Tree<usize> {
        let mut tree = Tree::leaf(0);
        for label in 1..=edge_depth {
            tree = Tree::node(label, vec![tree]);
        }
        tree
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn subtree_and_replace_refine_unary_path_semantics(
            (edge_depth, path_depth) in (0usize..128).prop_flat_map(|edge_depth| {
                (Just(edge_depth), 0usize..=edge_depth)
            }),
        ) {
            let tree = unary_tree(edge_depth);
            let path = vec![0; path_depth];
            let subtree = tree.subtree(&path).expect("the unary path is valid");
            prop_assert_eq!(*subtree.label(), edge_depth - path_depth);

            let replacement_label = edge_depth + 1;
            let replaced = tree
                .replace(&path, Tree::leaf(replacement_label))
                .expect("the unary replacement path is valid");
            let inserted = replaced
                .subtree(&path)
                .expect("the replaced path remains valid");
            prop_assert_eq!(*inserted.label(), replacement_label);
            prop_assert_eq!(replaced.depth(), path_depth + 1);
        }
    }

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

        let subtree = tree
            .subtree(&[0])
            .expect("tree_transducers/tree.rs: required value was None/Err");
        assert_eq!(subtree.label(), &"NP");

        let leaf = tree
            .subtree(&[0, 1])
            .expect("tree_transducers/tree.rs: required value was None/Err");
        assert_eq!(leaf.label(), &"cat");

        assert!(tree.subtree(&[5]).is_none());
    }

    #[test]
    fn test_replace() {
        let tree = Tree::node("S", vec![Tree::leaf("NP"), Tree::leaf("VP")]);

        let replaced = tree
            .replace(&[0], Tree::leaf("PP"))
            .expect("tree_transducers/tree.rs: required value was None/Err");
        assert_eq!(replaced.children()[0].label(), &"PP");
    }

    #[test]
    fn deep_subtree_and_replace_use_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-tree-path".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let tree = unary_tree(DEEP_TREE_PATH);
                let path = vec![0; DEEP_TREE_PATH];
                let subtree = tree.subtree(&path).expect("the deep unary path is valid");
                assert_eq!(*subtree.label(), 0);
                let replaced = tree
                    .replace(&path, Tree::leaf(DEEP_TREE_PATH + 1))
                    .expect("the deep unary replacement path is valid");
                assert_eq!(replaced.depth(), DEEP_TREE_PATH + 1);
                assert_eq!(
                    *replaced
                        .subtree(&path)
                        .expect("the replacement is present")
                        .label(),
                    DEEP_TREE_PATH + 1,
                );
                drop(replaced);
                drop(subtree);
                drop(tree);
            })
            .expect("the bounded-stack tree-path worker must spawn")
            .join()
            .expect("tree path operations and lifecycle must not overflow the native stack");
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
