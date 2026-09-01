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

    /// Extract the structurally best parse tree.
    ///
    /// Forest nodes do not carry parse probabilities, so ties are resolved by
    /// preferring shallower trees, then smaller trees, then stable identifiers.
    pub fn best_parse(&self) -> Option<ParseTree> {
        self.roots()
            .filter_map(|root| self.extract_tree(root).map(|tree| (root, tree)))
            .min_by_key(|(root, tree)| {
                (
                    tree.depth(),
                    tree.size(),
                    tree.start.0,
                    tree.end.0,
                    tree.rule.index(),
                    root.id(),
                )
            })
            .map(|(_, tree)| tree)
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
        #[derive(Clone, Copy)]
        enum Action {
            Child(ForestNodeId),
            Terminal(EdgeId),
        }

        struct Frame {
            id: ForestNodeId,
            tree: ParseTree,
            actions: Vec<Action>,
            next_action: usize,
        }

        fn frame(forest: &ParseForest, id: ForestNodeId) -> Option<Frame> {
            let node = forest.node(id)?;
            let mut actions = Vec::new();
            for child in &node.children {
                match child {
                    ForestChild::Derivation(kids) => {
                        actions.extend(kids.iter().copied().map(Action::Child));
                        break;
                    }
                    ForestChild::Terminal(edge) => actions.push(Action::Terminal(*edge)),
                }
            }
            Some(Frame {
                id,
                tree: ParseTree {
                    rule: node.rule,
                    start: node.start,
                    end: node.end,
                    children: Vec::with_capacity(actions.len()),
                },
                actions,
                next_action: 0,
            })
        }

        let mut on_path = FxHashSet::default();
        on_path.insert(root);
        let mut frames = vec![frame(self, root)?];
        loop {
            let current = frames
                .last_mut()
                .expect("the root extraction frame remains until completion");
            if let Some(action) = current.actions.get(current.next_action).copied() {
                current.next_action += 1;
                match action {
                    Action::Terminal(edge) => {
                        current.tree.children.push(ParseTreeChild::Terminal(edge));
                    }
                    Action::Child(id) => {
                        if on_path.contains(&id) {
                            return None;
                        }
                        if let Some(child) = frame(self, id) {
                            on_path.insert(id);
                            frames.push(child);
                        }
                    }
                }
                continue;
            }

            let completed = frames
                .pop()
                .expect("a completed extraction frame is present");
            on_path.remove(&completed.id);
            if let Some(parent) = frames.last_mut() {
                parent
                    .tree
                    .children
                    .push(ParseTreeChild::Tree(Box::new(completed.tree)));
            } else {
                return Some(completed.tree);
            }
        }
    }

    /// Collect all edges used in any parse.
    pub fn collect_used_edges(&self) -> FxHashSet<EdgeId> {
        let mut edges = FxHashSet::default();
        let mut visited = FxHashSet::default();
        let mut pending: Vec<ForestNodeId> = self.roots().collect();
        while let Some(node_id) = pending.pop() {
            if !visited.insert(node_id) {
                continue;
            }
            if let Some(node) = self.node(node_id) {
                for child in &node.children {
                    match child {
                        ForestChild::Derivation(kids) => pending.extend(kids.iter().copied()),
                        ForestChild::Terminal(edge) => {
                            edges.insert(*edge);
                        }
                    }
                }
            }
        }

        edges
    }
}

/// A single parse tree.
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
pub enum ParseTreeChild {
    /// A subtree.
    Tree(Box<ParseTree>),
    /// A terminal (edge in the lattice).
    Terminal(EdgeId),
}

impl Clone for ParseTree {
    fn clone(&self) -> Self {
        struct Frame<'a> {
            source: &'a ParseTree,
            next_child: usize,
            children: Vec<ParseTreeChild>,
        }
        let mut frames = vec![Frame {
            source: self,
            next_child: 0,
            children: Vec::with_capacity(self.children.len()),
        }];
        loop {
            let frame = frames
                .last_mut()
                .expect("the root parse-tree clone frame remains until completion");
            if let Some(child) = frame.source.children.get(frame.next_child) {
                frame.next_child += 1;
                match child {
                    ParseTreeChild::Terminal(edge) => {
                        frame.children.push(ParseTreeChild::Terminal(*edge));
                    }
                    ParseTreeChild::Tree(subtree) => frames.push(Frame {
                        source: subtree,
                        next_child: 0,
                        children: Vec::with_capacity(subtree.children.len()),
                    }),
                }
                continue;
            }
            let completed = frames
                .pop()
                .expect("a completed parse-tree clone frame is present");
            let cloned = ParseTree {
                rule: completed.source.rule,
                start: completed.source.start,
                end: completed.source.end,
                children: completed.children,
            };
            if let Some(parent) = frames.last_mut() {
                parent.children.push(ParseTreeChild::Tree(Box::new(cloned)));
            } else {
                return cloned;
            }
        }
    }
}

impl Clone for ParseTreeChild {
    fn clone(&self) -> Self {
        match self {
            ParseTreeChild::Tree(tree) => ParseTreeChild::Tree(Box::new((**tree).clone())),
            ParseTreeChild::Terminal(edge) => ParseTreeChild::Terminal(*edge),
        }
    }
}

enum ParseTreeDebugRoot<'a> {
    Tree(&'a ParseTree),
    Child(&'a ParseTreeChild),
}

enum ParseTreeDebugEvent<'a> {
    CompactTree(&'a ParseTree),
    CompactChild(&'a ParseTreeChild),
    PrettyTree(&'a ParseTree, usize),
    PrettyChild(&'a ParseTreeChild, usize),
    PrettyTreeSuffix(usize),
    PrettyTupleSuffix(usize),
    Indent(usize),
    Text(&'static str),
}

fn write_parse_tree_indent(formatter: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    for _ in 0..depth {
        formatter.write_str("    ")?;
    }
    Ok(())
}

fn write_pretty_u32_tuple(
    formatter: &mut fmt::Formatter<'_>,
    name: &str,
    value: u32,
    field_depth: usize,
) -> fmt::Result {
    writeln!(formatter, "{name}(")?;
    write_parse_tree_indent(formatter, field_depth + 1)?;
    writeln!(formatter, "{value},")?;
    write_parse_tree_indent(formatter, field_depth)?;
    formatter.write_str(")")
}

fn debug_parse_tree(
    root: ParseTreeDebugRoot<'_>,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    let mut events = Vec::with_capacity(64);
    match (formatter.alternate(), root) {
        (false, ParseTreeDebugRoot::Tree(tree)) => {
            events.push(ParseTreeDebugEvent::CompactTree(tree));
        }
        (false, ParseTreeDebugRoot::Child(child)) => {
            events.push(ParseTreeDebugEvent::CompactChild(child));
        }
        (true, ParseTreeDebugRoot::Tree(tree)) => {
            events.push(ParseTreeDebugEvent::PrettyTree(tree, 0));
        }
        (true, ParseTreeDebugRoot::Child(child)) => {
            events.push(ParseTreeDebugEvent::PrettyChild(child, 0));
        }
    }

    while let Some(event) = events.pop() {
        match event {
            ParseTreeDebugEvent::Text(text) => formatter.write_str(text)?,
            ParseTreeDebugEvent::Indent(depth) => {
                write_parse_tree_indent(formatter, depth)?;
            }
            ParseTreeDebugEvent::CompactTree(tree) => {
                write!(
                    formatter,
                    "ParseTree {{ rule: {:?}, start: {:?}, end: {:?}, children: [",
                    tree.rule, tree.start, tree.end
                )?;
                events.push(ParseTreeDebugEvent::Text("] }"));
                for (index, child) in tree.children.iter().enumerate().rev() {
                    if index + 1 < tree.children.len() {
                        events.push(ParseTreeDebugEvent::Text(", "));
                    }
                    events.push(ParseTreeDebugEvent::CompactChild(child));
                }
            }
            ParseTreeDebugEvent::CompactChild(child) => match child {
                ParseTreeChild::Tree(tree) => {
                    formatter.write_str("Tree(")?;
                    events.push(ParseTreeDebugEvent::Text(")"));
                    events.push(ParseTreeDebugEvent::CompactTree(tree));
                }
                ParseTreeChild::Terminal(edge) => write!(formatter, "Terminal({edge:?})")?,
            },
            ParseTreeDebugEvent::PrettyTree(tree, depth) => {
                formatter.write_str("ParseTree {\n")?;
                write_parse_tree_indent(formatter, depth + 1)?;
                formatter.write_str("rule: ")?;
                write_pretty_u32_tuple(formatter, "RuleId", tree.rule.0, depth + 1)?;
                formatter.write_str(",\n")?;
                write_parse_tree_indent(formatter, depth + 1)?;
                formatter.write_str("start: ")?;
                write_pretty_u32_tuple(formatter, "NodeId", tree.start.0, depth + 1)?;
                formatter.write_str(",\n")?;
                write_parse_tree_indent(formatter, depth + 1)?;
                formatter.write_str("end: ")?;
                write_pretty_u32_tuple(formatter, "NodeId", tree.end.0, depth + 1)?;
                formatter.write_str(",\n")?;
                write_parse_tree_indent(formatter, depth + 1)?;
                if tree.children.is_empty() {
                    formatter.write_str("children: [],\n")?;
                    write_parse_tree_indent(formatter, depth)?;
                    formatter.write_str("}")?;
                } else {
                    formatter.write_str("children: [\n")?;
                    events.push(ParseTreeDebugEvent::PrettyTreeSuffix(depth));
                    for child in tree.children.iter().rev() {
                        events.push(ParseTreeDebugEvent::Text(",\n"));
                        events.push(ParseTreeDebugEvent::PrettyChild(child, depth + 2));
                        events.push(ParseTreeDebugEvent::Indent(depth + 2));
                    }
                }
            }
            ParseTreeDebugEvent::PrettyChild(child, depth) => match child {
                ParseTreeChild::Tree(tree) => {
                    formatter.write_str("Tree(\n")?;
                    events.push(ParseTreeDebugEvent::PrettyTupleSuffix(depth));
                    events.push(ParseTreeDebugEvent::PrettyTree(tree, depth + 1));
                    events.push(ParseTreeDebugEvent::Indent(depth + 1));
                }
                ParseTreeChild::Terminal(edge) => {
                    formatter.write_str("Terminal(\n")?;
                    write_parse_tree_indent(formatter, depth + 1)?;
                    write_pretty_u32_tuple(formatter, "EdgeId", edge.0, depth + 1)?;
                    formatter.write_str(",\n")?;
                    write_parse_tree_indent(formatter, depth)?;
                    formatter.write_str(")")?;
                }
            },
            ParseTreeDebugEvent::PrettyTreeSuffix(depth) => {
                write_parse_tree_indent(formatter, depth + 1)?;
                formatter.write_str("],\n")?;
                write_parse_tree_indent(formatter, depth)?;
                formatter.write_str("}")?;
            }
            ParseTreeDebugEvent::PrettyTupleSuffix(depth) => {
                formatter.write_str(",\n")?;
                write_parse_tree_indent(formatter, depth)?;
                formatter.write_str(")")?;
            }
        }
    }
    Ok(())
}

impl fmt::Debug for ParseTree {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_parse_tree(ParseTreeDebugRoot::Tree(self), formatter)
    }
}

impl fmt::Debug for ParseTreeChild {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_parse_tree(ParseTreeDebugRoot::Child(self), formatter)
    }
}

impl Drop for ParseTree {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        for child in std::mem::take(&mut self.children) {
            if let ParseTreeChild::Tree(tree) = child {
                pending.push(tree);
            }
        }
        while let Some(mut tree) = pending.pop() {
            for child in std::mem::take(&mut tree.children) {
                if let ParseTreeChild::Tree(subtree) = child {
                    pending.push(subtree);
                }
            }
        }
    }
}

impl ParseTree {
    /// Get the depth of the tree.
    pub fn depth(&self) -> usize {
        let mut maximum = 1usize;
        let mut pending = vec![(self, 1usize)];
        while let Some((tree, depth)) = pending.pop() {
            maximum = maximum.max(depth);
            pending.extend(tree.children.iter().rev().filter_map(|child| match child {
                ParseTreeChild::Tree(subtree) => Some((subtree.as_ref(), depth + 1)),
                ParseTreeChild::Terminal(_) => None,
            }));
        }
        maximum
    }

    /// Get the number of nodes in the tree.
    pub fn size(&self) -> usize {
        let mut size = 0usize;
        let mut pending = vec![self];
        while let Some(tree) = pending.pop() {
            size += 1;
            for child in &tree.children {
                match child {
                    ParseTreeChild::Tree(subtree) => pending.push(subtree),
                    ParseTreeChild::Terminal(_) => size += 1,
                }
            }
        }
        size
    }

    /// Collect all edges in this tree.
    pub fn edges(&self) -> Vec<EdgeId> {
        let mut result = Vec::new();
        self.collect_edges(&mut result);
        result
    }

    fn collect_edges(&self, result: &mut Vec<EdgeId>) {
        enum Item<'a> {
            Tree(&'a ParseTree),
            Edge(EdgeId),
        }
        let mut pending = vec![Item::Tree(self)];
        while let Some(item) = pending.pop() {
            match item {
                Item::Tree(tree) => {
                    pending.extend(tree.children.iter().rev().map(|child| match child {
                        ParseTreeChild::Tree(subtree) => Item::Tree(subtree),
                        ParseTreeChild::Terminal(edge) => Item::Edge(*edge),
                    }))
                }
                Item::Edge(edge) => result.push(edge),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const PROPERTY_CASES: u32 = 512;
    const DEEP_PARSE_TREE_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    struct RecursiveTreeDebug<'a>(&'a ParseTree);

    struct RecursiveChildDebug<'a>(&'a ParseTreeChild);

    struct RecursiveChildrenDebug<'a>(&'a [ParseTreeChild]);

    impl std::fmt::Debug for RecursiveTreeDebug<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("ParseTree")
                .field("rule", &self.0.rule)
                .field("start", &self.0.start)
                .field("end", &self.0.end)
                .field("children", &RecursiveChildrenDebug(&self.0.children))
                .finish()
        }
    }

    impl std::fmt::Debug for RecursiveChildDebug<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.0 {
                ParseTreeChild::Tree(tree) => formatter
                    .debug_tuple("Tree")
                    .field(&RecursiveTreeDebug(tree))
                    .finish(),
                ParseTreeChild::Terminal(edge) => {
                    formatter.debug_tuple("Terminal").field(edge).finish()
                }
            }
        }
    }

    impl std::fmt::Debug for RecursiveChildrenDebug<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_list()
                .entries(self.0.iter().map(RecursiveChildDebug))
                .finish()
        }
    }

    fn recursive_depth(tree: &ParseTree) -> usize {
        1 + tree
            .children
            .iter()
            .filter_map(|child| match child {
                ParseTreeChild::Tree(subtree) => Some(recursive_depth(subtree)),
                ParseTreeChild::Terminal(_) => None,
            })
            .max()
            .unwrap_or(0)
    }

    fn recursive_size(tree: &ParseTree) -> usize {
        1 + tree
            .children
            .iter()
            .map(|child| match child {
                ParseTreeChild::Tree(subtree) => recursive_size(subtree),
                ParseTreeChild::Terminal(_) => 1,
            })
            .sum::<usize>()
    }

    fn recursive_edges(tree: &ParseTree, output: &mut Vec<EdgeId>) {
        for child in &tree.children {
            match child {
                ParseTreeChild::Tree(subtree) => recursive_edges(subtree, output),
                ParseTreeChild::Terminal(edge) => output.push(*edge),
            }
        }
    }

    fn parse_tree_strategy() -> BoxedStrategy<ParseTree> {
        (
            0u32..32,
            0u32..64,
            0u32..64,
            prop::collection::vec(0u32..128, 0..=4),
        )
            .prop_map(|(rule, start, end, terminals)| ParseTree {
                rule: RuleId::new(rule),
                start: NodeId(start),
                end: NodeId(end),
                children: terminals
                    .into_iter()
                    .map(|edge| ParseTreeChild::Terminal(EdgeId(edge)))
                    .collect(),
            })
            .prop_recursive(5, 96, 4, |inner| {
                (
                    0u32..32,
                    0u32..64,
                    0u32..64,
                    prop::collection::vec(
                        prop_oneof![
                            3 => inner
                                .clone()
                                .prop_map(|tree| ParseTreeChild::Tree(Box::new(tree))),
                            1 => (0u32..128)
                                .prop_map(|edge| ParseTreeChild::Terminal(EdgeId(edge))),
                        ],
                        0..=4,
                    ),
                )
                    .prop_map(|(rule, start, end, children)| ParseTree {
                        rule: RuleId::new(rule),
                        start: NodeId(start),
                        end: NodeId(end),
                        children,
                    })
            })
            .boxed()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: PROPERTY_CASES,
            max_shrink_iters: 20_000,
            .. ProptestConfig::default()
        })]

        #[test]
        fn parse_tree_machine_refines_recursive_shape(tree in parse_tree_strategy()) {
            prop_assert_eq!(tree.depth(), recursive_depth(&tree));
            prop_assert_eq!(tree.size(), recursive_size(&tree));
            let mut expected_edges = Vec::new();
            recursive_edges(&tree, &mut expected_edges);
            prop_assert_eq!(tree.edges(), expected_edges);

            let compact = format!("{tree:?}");
            prop_assert_eq!(&compact, &format!("{:?}", RecursiveTreeDebug(&tree)));
            let pretty = format!("{tree:#?}");
            prop_assert_eq!(&pretty, &format!("{:#?}", RecursiveTreeDebug(&tree)));

            let clone = tree.clone();
            prop_assert_eq!(format!("{clone:?}"), compact);
        }

        #[test]
        fn forest_extraction_refines_linear_derivation(depth in 1usize..=128) {
            let forest = linear_parse_forest(depth);
            let best = forest.best_parse().expect("the linear forest has one parse");
            prop_assert_eq!(best.depth(), depth);
            prop_assert_eq!(best.size(), depth + 1);
            prop_assert_eq!(best.edges(), vec![EdgeId(0)]);

            let parses = forest.all_parses(1);
            prop_assert_eq!(parses.len(), 1);
            prop_assert_eq!(parses[0].depth(), depth);
        }
    }

    fn deep_parse_tree(depth: usize) -> ParseTree {
        assert!(depth > 0);
        let mut tree = ParseTree {
            rule: RuleId::new(0),
            start: NodeId(0),
            end: NodeId(1),
            children: vec![ParseTreeChild::Terminal(EdgeId(0))],
        };
        for index in 1..depth {
            tree = ParseTree {
                rule: RuleId::new((index % u32::MAX as usize) as u32),
                start: NodeId(index as u32),
                end: NodeId(index.saturating_add(1) as u32),
                children: vec![ParseTreeChild::Tree(Box::new(tree))],
            };
        }
        tree
    }

    fn linear_parse_forest(depth: usize) -> ParseForest {
        assert!(depth > 0);
        let mut forest = ParseForest::new();
        let mut leaf = ForestNode::new(RuleId::new(0), NodeId(0), NodeId(1));
        leaf.add_terminal(EdgeId(0));
        let mut child = forest.add_node(leaf);
        for index in 1..depth {
            let mut parent = ForestNode::new(
                RuleId::new((index % u32::MAX as usize) as u32),
                NodeId(index as u32),
                NodeId(index.saturating_add(1) as u32),
            );
            parent.add_derivation(smallvec::smallvec![child]);
            child = forest.add_node(parent);
        }
        forest.add_root(child);
        forest
    }

    #[test]
    fn deep_parse_tree_operations_and_lifecycle_use_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-parse-tree-lifecycle".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let tree = deep_parse_tree(DEEP_PARSE_TREE_DEPTH);
                assert_eq!(tree.depth(), DEEP_PARSE_TREE_DEPTH);
                assert_eq!(tree.size(), DEEP_PARSE_TREE_DEPTH + 1);
                assert_eq!(tree.edges(), vec![EdgeId(0)]);
                let clone = tree.clone();
                assert_eq!(clone.depth(), DEEP_PARSE_TREE_DEPTH);
                assert!(format!("{tree:?}").starts_with("ParseTree {"));
                drop(clone);
                drop(tree);
            })
            .expect("the bounded-stack parse-tree worker must spawn")
            .join()
            .expect("parse-tree operations and lifecycle must not overflow the native stack");
    }

    #[test]
    fn deep_forest_extraction_uses_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-forest-extraction".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let forest = linear_parse_forest(DEEP_PARSE_TREE_DEPTH);
                let best = forest
                    .best_parse()
                    .expect("the linear forest has one parse");
                assert_eq!(best.depth(), DEEP_PARSE_TREE_DEPTH);
                assert_eq!(best.size(), DEEP_PARSE_TREE_DEPTH + 1);
                let parses = forest.all_parses(1);
                assert_eq!(parses.len(), 1);
                assert_eq!(parses[0].depth(), DEEP_PARSE_TREE_DEPTH);
                drop(parses);
                drop(best);
                drop(forest);
            })
            .expect("the bounded-stack forest worker must spawn")
            .join()
            .expect("forest extraction must not overflow the native stack");
    }

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
    fn test_best_parse_prefers_shallow_structural_parse() {
        let mut forest = ParseForest::new();

        let mut child = ForestNode::new(RuleId::new(2), NodeId(0), NodeId(1));
        child.add_terminal(EdgeId(0));
        let child_id = forest.add_node(child);

        let mut deep_root = ForestNode::new(RuleId::new(1), NodeId(0), NodeId(1));
        deep_root.add_derivation(smallvec::smallvec![child_id]);
        let deep_root_id = forest.add_node(deep_root);

        let mut shallow_root = ForestNode::new(RuleId::new(0), NodeId(0), NodeId(1));
        shallow_root.add_terminal(EdgeId(0));
        let shallow_root_id = forest.add_node(shallow_root);

        forest.add_root(deep_root_id);
        forest.add_root(shallow_root_id);

        let tree = forest.best_parse().expect("should have parse");
        assert_eq!(tree.rule, RuleId::new(0));
        assert_eq!(tree.depth(), 1);
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
