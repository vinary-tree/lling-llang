//! Refinement and native-stack properties for the owned parser syntax tree.
//!
//! The shallow oracle deliberately uses the recursive equations that define the
//! public semantics.  Production code must instead refine those equations with
//! the formally verified explicit ordered-tree machines.  Deep tests include
//! construction, traversal, borrowed-to-owned conversion, lifecycle operations,
//! and teardown on a 256 KiB native stack.

use std::fmt;

use lling_llang::programming::{
    NodeKind, Position, Range, SimpleSyntaxNodeRef, SyntaxNode, SyntaxNodeRef,
};
use proptest::prelude::*;

const PROPERTY_CASES: u32 = 512;
const DEEP_INPUT_DEPTH: usize = 100_000;
const SMALL_NATIVE_STACK: usize = 256 * 1024;

fn recursive_text(node: &SyntaxNode) -> String {
    if let Some(text) = &node.text {
        text.clone()
    } else {
        node.children.iter().map(recursive_text).collect()
    }
}

fn recursive_has_error(node: &SyntaxNode) -> bool {
    node.is_error || node.children.iter().any(recursive_has_error)
}

fn recursive_error_count(node: &SyntaxNode) -> usize {
    usize::from(node.is_error)
        + node
            .children
            .iter()
            .map(recursive_error_count)
            .sum::<usize>()
}

fn recursive_depth(node: &SyntaxNode) -> usize {
    1 + node.children.iter().map(recursive_depth).max().unwrap_or(0)
}

fn recursive_node_count(node: &SyntaxNode) -> usize {
    1 + node
        .children
        .iter()
        .map(recursive_node_count)
        .sum::<usize>()
}

type NodeSignature = (String, Range, Option<String>, bool, bool);

fn signature(node: &SyntaxNode) -> NodeSignature {
    (
        node.kind.name().to_owned(),
        node.range,
        node.text.clone(),
        node.is_error,
        node.is_missing,
    )
}

fn recursive_errors(node: &SyntaxNode, output: &mut Vec<NodeSignature>) {
    if node.is_error {
        output.push(signature(node));
    }
    for child in &node.children {
        recursive_errors(child, output);
    }
}

fn recursive_find_all(node: &SyntaxNode, kind: &str, output: &mut Vec<NodeSignature>) {
    if node.kind.name() == kind {
        output.push(signature(node));
    }
    for child in &node.children {
        recursive_find_all(child, kind, output);
    }
}

struct RecursiveNodeDebug<'a>(&'a SyntaxNode);

struct RecursiveChildrenDebug<'a>(&'a [SyntaxNode]);

impl fmt::Debug for RecursiveNodeDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxNode")
            .field("kind", &self.0.kind)
            .field("range", &self.0.range)
            .field("text", &self.0.text)
            .field("children", &RecursiveChildrenDebug(&self.0.children))
            .field("is_error", &self.0.is_error)
            .field("is_missing", &self.0.is_missing)
            .finish()
    }
}

impl fmt::Debug for RecursiveChildrenDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.iter().map(RecursiveNodeDebug))
            .finish()
    }
}

fn position_strategy() -> impl Strategy<Value = Position> {
    (0usize..32, 0usize..64, 0usize..4096).prop_map(|(line, column, byte_offset)| Position {
        line,
        column,
        byte_offset,
    })
}

fn range_strategy() -> impl Strategy<Value = Range> {
    (position_strategy(), position_strategy()).prop_map(|(start, end)| Range { start, end })
}

fn node_fields_strategy() -> impl Strategy<Value = (NodeKind, Range, Option<String>, bool, bool)> {
    (
        "[a-zA-Z_][a-zA-Z0-9_]{0,7}",
        range_strategy(),
        prop::option::of("[a-zA-Z0-9 ]{0,12}"),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(kind, range, text, is_error, is_missing)| {
            (NodeKind::new(kind), range, text, is_error, is_missing)
        })
}

fn syntax_node_strategy() -> BoxedStrategy<SyntaxNode> {
    node_fields_strategy()
        .prop_map(|(kind, range, text, is_error, is_missing)| SyntaxNode {
            kind,
            range,
            text,
            children: Vec::new(),
            is_error,
            is_missing,
        })
        .prop_recursive(5, 96, 4, |inner| {
            (node_fields_strategy(), prop::collection::vec(inner, 0..=4)).prop_map(
                |((kind, range, text, is_error, is_missing), children)| SyntaxNode {
                    kind,
                    range,
                    text,
                    children,
                    is_error,
                    is_missing,
                },
            )
        })
        .boxed()
}

fn deep_chain(depth: usize) -> SyntaxNode {
    assert!(depth > 0);
    let mut node = SyntaxNode {
        kind: NodeKind::new("ERROR"),
        range: Range::default(),
        text: Some("leaf".to_owned()),
        children: Vec::new(),
        is_error: true,
        is_missing: false,
    };
    for index in 1..depth {
        node = SyntaxNode {
            kind: NodeKind::new(if index % 2 == 0 { "node" } else { "branch" }),
            range: Range::default(),
            text: None,
            children: vec![node],
            is_error: false,
            is_missing: index % 17 == 0,
        };
    }
    node
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: PROPERTY_CASES,
        max_shrink_iters: 20_000,
        .. ProptestConfig::default()
    })]

    #[test]
    fn syntax_node_ordered_operations_refine_recursive_equations(node in syntax_node_strategy()) {
        prop_assert_eq!(node.get_text(), recursive_text(&node));
        prop_assert_eq!(node.has_error(), recursive_has_error(&node));
        prop_assert_eq!(node.error_count(), recursive_error_count(&node));
        prop_assert_eq!(node.depth(), recursive_depth(&node));
        prop_assert_eq!(node.node_count(), recursive_node_count(&node));

        let actual_errors = node.find_errors().into_iter().map(signature).collect::<Vec<_>>();
        let mut expected_errors = Vec::new();
        recursive_errors(&node, &mut expected_errors);
        prop_assert_eq!(actual_errors, expected_errors);

        let query = node.kind.name();
        let actual_matches = node.find_all(query).into_iter().map(signature).collect::<Vec<_>>();
        let mut expected_matches = Vec::new();
        recursive_find_all(&node, query, &mut expected_matches);
        prop_assert_eq!(actual_matches, expected_matches);
    }

    #[test]
    fn syntax_node_lifecycle_preserves_derived_shape(node in syntax_node_strategy()) {
        let compact = format!("{node:?}");
        let expected_compact = format!("{:?}", RecursiveNodeDebug(&node));
        prop_assert_eq!(&compact, &expected_compact);

        let pretty = format!("{node:#?}");
        let expected_pretty = format!("{:#?}", RecursiveNodeDebug(&node));
        prop_assert_eq!(&pretty, &expected_pretty);

        let clone = node.clone();
        prop_assert_eq!(&clone, &node);
        prop_assert_eq!(format!("{clone:?}"), expected_compact);
    }
}

#[test]
fn deep_syntax_node_operations_use_constant_native_stack() {
    std::thread::Builder::new()
        .name("deep-syntax-node-operations".to_owned())
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let node = deep_chain(DEEP_INPUT_DEPTH);
            assert_eq!(node.get_text(), "leaf");
            assert!(node.has_error());
            assert_eq!(node.error_count(), 1);
            assert_eq!(node.find_errors().len(), 1);
            assert_eq!(node.find_all("node").len(), (DEEP_INPUT_DEPTH - 1) / 2);
            assert_eq!(node.depth(), DEEP_INPUT_DEPTH);
            assert_eq!(node.node_count(), DEEP_INPUT_DEPTH);
            drop(node);
        })
        .expect("the bounded-stack operation worker must spawn")
        .join()
        .expect("syntax-node operations must not overflow the native stack");
}

#[test]
fn deep_syntax_node_lifecycle_uses_constant_native_stack() {
    std::thread::Builder::new()
        .name("deep-syntax-node-lifecycle".to_owned())
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let node = deep_chain(DEEP_INPUT_DEPTH);
            let mut clone = node.clone();
            assert_eq!(clone, node);

            let compact = format!("{node:?}");
            assert!(compact.starts_with("SyntaxNode { kind: NodeKind("));
            assert!(compact.ends_with("is_missing: false }"));

            let mut deepest = &mut clone;
            while !deepest.children.is_empty() {
                deepest = &mut deepest.children[0];
            }
            deepest.is_error = false;
            assert_ne!(clone, node);

            drop(compact);
            drop(clone);
            drop(node);
        })
        .expect("the bounded-stack lifecycle worker must spawn")
        .join()
        .expect("syntax-node lifecycle must not overflow the native stack");
}

#[test]
fn deep_syntax_node_reference_conversion_uses_constant_native_stack() {
    std::thread::Builder::new()
        .name("deep-syntax-node-reference-conversion".to_owned())
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(|| {
            let node = deep_chain(DEEP_INPUT_DEPTH);
            let reference = SimpleSyntaxNodeRef::new(&node, "");
            let converted = reference.to_syntax_node();
            assert_eq!(converted.node_count(), DEEP_INPUT_DEPTH);
            assert_eq!(converted.get_text(), "");
            drop(converted);
            drop(node);
        })
        .expect("the bounded-stack conversion worker must spawn")
        .join()
        .expect("syntax-node conversion must not overflow the native stack");
}
