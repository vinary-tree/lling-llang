//! Traits for parser backend abstraction.
//!
//! This module defines the `ParserBackend` trait that provides a unified interface
//! for integrating different parsing technologies (tree-sitter, LALRPOP, pest, etc.).

use std::fmt::{self, Debug, Display};
use std::hash::Hash;

/// Position in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// Zero-indexed line number.
    pub line: usize,
    /// Zero-indexed column number (in UTF-8 code units).
    pub column: usize,
    /// Byte offset from start of input.
    pub byte_offset: usize,
}

impl Position {
    /// Create a new position.
    pub fn new(line: usize, column: usize, byte_offset: usize) -> Self {
        Self {
            line,
            column,
            byte_offset,
        }
    }

    /// Position at start of input.
    pub fn start() -> Self {
        Self::default()
    }
}

impl Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.column + 1)
    }
}

/// A range in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Range {
    /// Start position (inclusive).
    pub start: Position,
    /// End position (exclusive).
    pub end: Position,
}

impl Range {
    /// Create a new range.
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// Single-point range.
    pub fn point(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// Byte length of this range.
    pub fn byte_len(&self) -> usize {
        self.end.byte_offset.saturating_sub(self.start.byte_offset)
    }

    /// Check if this range contains a position.
    pub fn contains(&self, pos: Position) -> bool {
        pos.byte_offset >= self.start.byte_offset && pos.byte_offset < self.end.byte_offset
    }

    /// Check if this range overlaps with another.
    pub fn overlaps(&self, other: &Range) -> bool {
        self.start.byte_offset < other.end.byte_offset
            && other.start.byte_offset < self.end.byte_offset
    }

    /// Extend this range to include another.
    pub fn extend(&self, other: &Range) -> Range {
        Range {
            start: if self.start.byte_offset <= other.start.byte_offset {
                self.start
            } else {
                other.start
            },
            end: if self.end.byte_offset >= other.end.byte_offset {
                self.end
            } else {
                other.end
            },
        }
    }
}

impl Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

/// Kind of syntax node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeKind(pub String);

impl NodeKind {
    /// Create a new node kind.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Get the name of this node kind.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Check if this is an error node.
    pub fn is_error(&self) -> bool {
        self.0 == "ERROR" || self.0.starts_with("error")
    }

    /// Check if this is a missing node (inserted by error recovery).
    pub fn is_missing(&self) -> bool {
        self.0 == "MISSING" || self.0.starts_with("missing")
    }
}

impl Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<S: Into<String>> From<S> for NodeKind {
    fn from(s: S) -> Self {
        Self::new(s)
    }
}

/// A syntax node in a parse tree.
///
/// This is an owned representation suitable for storage and manipulation.
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxNode {
    /// Kind of this node.
    pub kind: NodeKind,
    /// Range in source.
    pub range: Range,
    /// Text content (for leaf nodes).
    pub text: Option<String>,
    /// Child nodes.
    pub children: Vec<SyntaxNode>,
    /// Whether this node represents an error.
    pub is_error: bool,
    /// Whether this node was inserted by error recovery.
    pub is_missing: bool,
}

impl SyntaxNode {
    /// Create a new syntax node.
    pub fn new(kind: NodeKind, range: Range) -> Self {
        Self {
            is_error: kind.is_error(),
            is_missing: kind.is_missing(),
            kind,
            range,
            text: None,
            children: Vec::new(),
        }
    }

    /// Create a leaf node with text.
    pub fn leaf(kind: NodeKind, range: Range, text: impl Into<String>) -> Self {
        Self {
            is_error: kind.is_error(),
            is_missing: kind.is_missing(),
            kind,
            range,
            text: Some(text.into()),
            children: Vec::new(),
        }
    }

    /// Create an error node.
    pub fn error(range: Range, text: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::new("ERROR"),
            range,
            text: Some(text.into()),
            children: Vec::new(),
            is_error: true,
            is_missing: false,
        }
    }

    /// Create a missing node (inserted by error recovery).
    pub fn missing(kind: NodeKind, pos: Position) -> Self {
        Self {
            is_error: false,
            is_missing: true,
            kind,
            range: Range::point(pos),
            text: None,
            children: Vec::new(),
        }
    }

    /// Add a child node.
    pub fn add_child(&mut self, child: SyntaxNode) {
        self.children.push(child);
    }

    /// Set the text content.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = Some(text.into());
    }

    /// Get text content, recursively collecting from children if needed.
    pub fn get_text(&self) -> String {
        if let Some(ref text) = self.text {
            text.clone()
        } else {
            self.children.iter().map(|c| c.get_text()).collect()
        }
    }

    /// Check if this node or any descendant has an error.
    pub fn has_error(&self) -> bool {
        self.is_error || self.children.iter().any(|c| c.has_error())
    }

    /// Count error nodes in this tree.
    pub fn error_count(&self) -> usize {
        let self_count = if self.is_error { 1 } else { 0 };
        self_count + self.children.iter().map(|c| c.error_count()).sum::<usize>()
    }

    /// Find all error nodes in this tree.
    pub fn find_errors(&self) -> Vec<&SyntaxNode> {
        let mut errors = Vec::new();
        self.collect_errors(&mut errors);
        errors
    }

    fn collect_errors<'a>(&'a self, errors: &mut Vec<&'a SyntaxNode>) {
        if self.is_error {
            errors.push(self);
        }
        for child in &self.children {
            child.collect_errors(errors);
        }
    }

    /// Get named children (non-anonymous nodes).
    pub fn named_children(&self) -> impl Iterator<Item = &SyntaxNode> {
        self.children
            .iter()
            .filter(|c| !c.kind.name().starts_with('_'))
    }

    /// Find first child with given kind.
    pub fn find_child(&self, kind: &str) -> Option<&SyntaxNode> {
        self.children.iter().find(|c| c.kind.name() == kind)
    }

    /// Find all descendants with given kind.
    pub fn find_all(&self, kind: &str) -> Vec<&SyntaxNode> {
        let mut results = Vec::new();
        self.collect_by_kind(kind, &mut results);
        results
    }

    fn collect_by_kind<'a>(&'a self, kind: &str, results: &mut Vec<&'a SyntaxNode>) {
        if self.kind.name() == kind {
            results.push(self);
        }
        for child in &self.children {
            child.collect_by_kind(kind, results);
        }
    }

    /// Get depth of this tree.
    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            1
        } else {
            1 + self.children.iter().map(|c| c.depth()).max().unwrap_or(0)
        }
    }

    /// Count total nodes in this tree.
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }
}

/// A reference to a syntax node (borrowed).
///
/// This is a lightweight view that avoids copying node data.
pub trait SyntaxNodeRef<'a>: Clone + Debug {
    /// Get the kind of this node.
    fn kind(&self) -> NodeKind;

    /// Get the range of this node in source.
    fn range(&self) -> Range;

    /// Get the text content of this node.
    fn text(&self) -> &'a str;

    /// Check if this is an error node.
    fn is_error(&self) -> bool;

    /// Check if this is a missing node.
    fn is_missing(&self) -> bool;

    /// Get child nodes.
    fn children(&self) -> impl Iterator<Item = Self>;

    /// Get the number of children.
    fn child_count(&self) -> usize;

    /// Get child at index.
    fn child(&self, index: usize) -> Option<Self>;

    /// Get the parent node.
    fn parent(&self) -> Option<Self>;

    /// Convert to owned SyntaxNode.
    fn to_syntax_node(&self) -> SyntaxNode {
        let mut node = SyntaxNode::new(self.kind(), self.range());
        node.is_error = self.is_error();
        node.is_missing = self.is_missing();

        // For leaf nodes, store text directly
        if self.child_count() == 0 {
            node.text = Some(self.text().to_string());
        }

        // Recursively convert children
        for child in self.children() {
            node.children.push(child.to_syntax_node());
        }

        node
    }
}

/// Error from parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct ParserError {
    /// Error message.
    pub message: String,
    /// Location of error.
    pub range: Range,
    /// Severity level.
    pub severity: ErrorSeverity,
    /// Expected tokens/constructs.
    pub expected: Vec<String>,
    /// Actual token found.
    pub found: Option<String>,
}

impl ParserError {
    /// Create a new parser error.
    pub fn new(message: impl Into<String>, range: Range) -> Self {
        Self {
            message: message.into(),
            range,
            severity: ErrorSeverity::Error,
            expected: Vec::new(),
            found: None,
        }
    }

    /// Set expected tokens.
    pub fn with_expected(mut self, expected: Vec<String>) -> Self {
        self.expected = expected;
        self
    }

    /// Set the found token.
    pub fn with_found(mut self, found: impl Into<String>) -> Self {
        self.found = Some(found.into());
        self
    }

    /// Set severity.
    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = severity;
        self
    }
}

impl Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.range, self.message)?;
        if !self.expected.is_empty() {
            write!(f, " (expected: {})", self.expected.join(", "))?;
        }
        if let Some(ref found) = self.found {
            write!(f, " (found: {})", found)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParserError {}

/// Error severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorSeverity {
    /// Informational hint.
    Hint,
    /// Warning (code may work but is suspicious).
    Warning,
    /// Error (code will not work).
    Error,
    /// Fatal error (cannot continue parsing).
    Fatal,
}

/// Result of parsing.
#[derive(Debug, Clone)]
pub struct ParseResult<N> {
    /// The syntax tree (may be partial if errors occurred).
    pub tree: N,
    /// Errors encountered during parsing.
    pub errors: Vec<ParserError>,
}

impl<N> ParseResult<N> {
    /// Create a successful parse result.
    pub fn success(tree: N) -> Self {
        Self {
            tree,
            errors: Vec::new(),
        }
    }

    /// Create a parse result with errors.
    pub fn with_errors(tree: N, errors: Vec<ParserError>) -> Self {
        Self { tree, errors }
    }

    /// Check if parsing succeeded without errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Check if parsing had errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get the error count.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Map the tree to a different type.
    pub fn map<F, M>(self, f: F) -> ParseResult<M>
    where
        F: FnOnce(N) -> M,
    {
        ParseResult {
            tree: f(self.tree),
            errors: self.errors,
        }
    }
}

/// Trait for parser backends.
///
/// This provides a unified interface for different parsing technologies,
/// allowing WFST-based syntax repair to work with any parser.
pub trait ParserBackend: Send + Sync {
    /// The node reference type for this backend.
    type NodeRef<'a>: SyntaxNodeRef<'a>
    where
        Self: 'a;

    /// Parse input and return a result.
    ///
    /// If the parser supports error recovery, partial trees should be returned
    /// along with error information.
    fn parse<'a>(&'a self, input: &'a str) -> ParseResult<Self::NodeRef<'a>>;

    /// Get the root node of the last successful parse.
    fn root<'a>(&'a self) -> Option<Self::NodeRef<'a>>;

    /// Get the language name.
    fn language(&self) -> &str;

    /// Check if this parser supports incremental parsing.
    fn supports_incremental(&self) -> bool {
        false
    }

    /// Check if this parser supports error recovery.
    fn supports_error_recovery(&self) -> bool {
        true
    }

    /// Convert a node reference to an owned SyntaxNode.
    fn to_syntax_node_tree<'a>(&'a self, node: &Self::NodeRef<'a>) -> SyntaxNode {
        node.to_syntax_node()
    }

    /// Parse and return an owned tree.
    fn parse_owned(&self, input: &str) -> ParseResult<SyntaxNode> {
        let result = self.parse(input);
        ParseResult {
            tree: result.tree.to_syntax_node(),
            errors: result.errors,
        }
    }
}

/// A simple parser backend that wraps owned SyntaxNode trees.
///
/// This is useful for testing or when a full parser isn't available.
#[derive(Debug, Clone)]
pub struct SimpleParserBackend {
    language: String,
    last_tree: Option<SyntaxNode>,
    last_source: String,
    error_tree: SyntaxNode,
}

impl SimpleParserBackend {
    /// Create a new simple parser backend.
    pub fn new(language: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            last_tree: None,
            last_source: String::new(),
            error_tree: SyntaxNode::error(Range::default(), "no tree available"),
        }
    }

    /// Return the language identifier this backend was constructed for.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Set the tree directly (for testing).
    pub fn set_tree(&mut self, tree: SyntaxNode) {
        self.last_tree = Some(tree);
        self.last_source.clear();
    }

    /// Set the tree and source text directly.
    pub fn set_tree_with_source(&mut self, tree: SyntaxNode, source: impl Into<String>) {
        self.last_tree = Some(tree);
        self.last_source = source.into();
    }
}

/// Reference wrapper for SyntaxNode.
#[derive(Debug, Clone)]
pub struct SimpleSyntaxNodeRef<'a> {
    node: &'a SyntaxNode,
    source: &'a str,
}

impl<'a> SimpleSyntaxNodeRef<'a> {
    /// Create a new reference.
    pub fn new(node: &'a SyntaxNode, source: &'a str) -> Self {
        Self { node, source }
    }
}

impl<'a> SyntaxNodeRef<'a> for SimpleSyntaxNodeRef<'a> {
    fn kind(&self) -> NodeKind {
        self.node.kind.clone()
    }

    fn range(&self) -> Range {
        self.node.range
    }

    fn text(&self) -> &'a str {
        let start = self.node.range.start.byte_offset.min(self.source.len());
        let end = self.node.range.end.byte_offset.min(self.source.len());
        if start <= end {
            &self.source[start..end]
        } else {
            ""
        }
    }

    fn is_error(&self) -> bool {
        self.node.is_error
    }

    fn is_missing(&self) -> bool {
        self.node.is_missing
    }

    fn children(&self) -> impl Iterator<Item = Self> {
        self.node.children.iter().map(|c| SimpleSyntaxNodeRef {
            node: c,
            source: self.source,
        })
    }

    fn child_count(&self) -> usize {
        self.node.children.len()
    }

    fn child(&self, index: usize) -> Option<Self> {
        self.node.children.get(index).map(|c| SimpleSyntaxNodeRef {
            node: c,
            source: self.source,
        })
    }

    fn parent(&self) -> Option<Self> {
        None
    }
}

impl SimpleParserBackend {
    /// Parse input and return a parse result with the simple backend.
    pub fn parse_simple<'a>(&'a self, input: &'a str) -> ParseResult<SimpleSyntaxNodeRef<'a>> {
        if let Some(ref tree) = self.last_tree {
            ParseResult::success(SimpleSyntaxNodeRef::new(tree, input))
        } else {
            // Return an error node for missing tree
            ParseResult::with_errors(
                SimpleSyntaxNodeRef::new(&self.error_tree, input),
                vec![ParserError::new("No tree available", Range::default())],
            )
        }
    }

    /// Get root of last parse.
    pub fn root_simple<'a>(&'a self, source: &'a str) -> Option<SimpleSyntaxNodeRef<'a>> {
        self.last_tree
            .as_ref()
            .map(|t| SimpleSyntaxNodeRef::new(t, source))
    }
}

impl ParserBackend for SimpleParserBackend {
    type NodeRef<'a> = SimpleSyntaxNodeRef<'a>;

    fn parse<'a>(&'a self, input: &'a str) -> ParseResult<Self::NodeRef<'a>> {
        self.parse_simple(input)
    }

    fn root<'a>(&'a self) -> Option<Self::NodeRef<'a>> {
        self.last_tree
            .as_ref()
            .map(|tree| SimpleSyntaxNodeRef::new(tree, &self.last_source))
    }

    fn language(&self) -> &str {
        &self.language
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position() {
        let pos = Position::new(5, 10, 100);
        assert_eq!(pos.line, 5);
        assert_eq!(pos.column, 10);
        assert_eq!(pos.byte_offset, 100);
        assert_eq!(format!("{}", pos), "6:11"); // 1-indexed display
    }

    #[test]
    fn test_range() {
        let start = Position::new(0, 0, 0);
        let end = Position::new(0, 5, 5);
        let range = Range::new(start, end);

        assert_eq!(range.byte_len(), 5);
        assert!(range.contains(Position::new(0, 2, 2)));
        assert!(!range.contains(Position::new(0, 5, 5)));
    }

    #[test]
    fn test_range_overlaps() {
        let r1 = Range::new(Position::new(0, 0, 0), Position::new(0, 5, 5));
        let r2 = Range::new(Position::new(0, 3, 3), Position::new(0, 8, 8));
        let r3 = Range::new(Position::new(0, 10, 10), Position::new(0, 15, 15));

        assert!(r1.overlaps(&r2));
        assert!(r2.overlaps(&r1));
        assert!(!r1.overlaps(&r3));
    }

    #[test]
    fn test_node_kind() {
        let kind = NodeKind::new("function_definition");
        assert_eq!(kind.name(), "function_definition");
        assert!(!kind.is_error());
        assert!(!kind.is_missing());

        let error_kind = NodeKind::new("ERROR");
        assert!(error_kind.is_error());

        let missing_kind = NodeKind::new("MISSING");
        assert!(missing_kind.is_missing());
    }

    #[test]
    fn test_syntax_node() {
        let mut root = SyntaxNode::new(
            NodeKind::new("program"),
            Range::new(Position::start(), Position::new(0, 10, 10)),
        );

        let child = SyntaxNode::leaf(
            NodeKind::new("identifier"),
            Range::new(Position::start(), Position::new(0, 3, 3)),
            "foo",
        );

        root.add_child(child);

        assert_eq!(root.node_count(), 2);
        assert_eq!(root.depth(), 2);
        assert!(!root.has_error());
    }

    #[test]
    fn test_syntax_node_errors() {
        let mut root = SyntaxNode::new(
            NodeKind::new("program"),
            Range::new(Position::start(), Position::new(0, 10, 10)),
        );

        root.add_child(SyntaxNode::error(
            Range::new(Position::new(0, 5, 5), Position::new(0, 8, 8)),
            "unexpected token",
        ));

        assert!(root.has_error());
        assert_eq!(root.error_count(), 1);
        assert_eq!(root.find_errors().len(), 1);
    }

    #[test]
    fn test_syntax_node_find() {
        let mut root = SyntaxNode::new(NodeKind::new("program"), Range::default());

        let mut func = SyntaxNode::new(NodeKind::new("function"), Range::default());

        func.add_child(SyntaxNode::leaf(
            NodeKind::new("identifier"),
            Range::default(),
            "foo",
        ));

        root.add_child(func);

        assert!(root.find_child("function").is_some());
        assert!(root.find_child("class").is_none());
        assert_eq!(root.find_all("identifier").len(), 1);
    }

    #[test]
    fn test_parser_error() {
        let err = ParserError::new("unexpected token", Range::default())
            .with_expected(vec!["identifier".to_string(), "number".to_string()])
            .with_found("operator");

        assert!(err.message.contains("unexpected"));
        assert_eq!(err.expected.len(), 2);
        assert_eq!(err.found.as_deref(), Some("operator"));
    }

    #[test]
    fn test_parse_result() {
        let tree = SyntaxNode::new(NodeKind::new("program"), Range::default());
        let result: ParseResult<SyntaxNode> = ParseResult::success(tree);

        assert!(result.is_ok());
        assert!(!result.has_errors());
        assert_eq!(result.error_count(), 0);
    }

    #[test]
    fn test_parse_result_with_errors() {
        let tree = SyntaxNode::new(NodeKind::new("program"), Range::default());
        let errors = vec![ParserError::new("test error", Range::default())];
        let result = ParseResult::with_errors(tree, errors);

        assert!(!result.is_ok());
        assert!(result.has_errors());
        assert_eq!(result.error_count(), 1);
    }

    #[test]
    fn test_simple_parser_backend() {
        let mut backend = SimpleParserBackend::new("test");

        let tree = SyntaxNode::leaf(
            NodeKind::new("program"),
            Range::new(Position::start(), Position::new(0, 5, 5)),
            "hello",
        );

        backend.set_tree(tree);

        let source = "hello";
        let result = backend.parse_simple(source);
        assert!(result.is_ok());
        assert_eq!(result.tree.text(), "hello");
    }
}
