//! Token representation and pattern matching for syntax repair.

use std::fmt::{self, Debug, Display};
use std::hash::Hash;

use super::traits::Range;

/// Token kind classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// Keywords (if, while, function, etc.).
    Keyword,
    /// Identifiers (variable names, function names).
    Identifier,
    /// Operators (+, -, *, /, etc.).
    Operator,
    /// Punctuation ({, }, (, ), ;, etc.).
    Punctuation,
    /// String literals.
    String,
    /// Numeric literals.
    Number,
    /// Comments.
    Comment,
    /// Whitespace.
    Whitespace,
    /// End of file.
    Eof,
    /// Error token.
    Error,
    /// Other/unknown token type.
    Other,
}

impl TokenKind {
    /// Check if this is a significant token (not whitespace or comment).
    pub fn is_significant(&self) -> bool {
        !matches!(self, TokenKind::Whitespace | TokenKind::Comment)
    }
}

impl Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Keyword => write!(f, "keyword"),
            TokenKind::Identifier => write!(f, "identifier"),
            TokenKind::Operator => write!(f, "operator"),
            TokenKind::Punctuation => write!(f, "punctuation"),
            TokenKind::String => write!(f, "string"),
            TokenKind::Number => write!(f, "number"),
            TokenKind::Comment => write!(f, "comment"),
            TokenKind::Whitespace => write!(f, "whitespace"),
            TokenKind::Eof => write!(f, "eof"),
            TokenKind::Error => write!(f, "error"),
            TokenKind::Other => write!(f, "other"),
        }
    }
}

/// A token in source code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    /// The token kind.
    pub kind: TokenKind,
    /// The token text.
    pub text: String,
    /// Range in source.
    pub range: Range,
}

impl Token {
    /// Create a new token.
    pub fn new(kind: TokenKind, text: impl Into<String>, range: Range) -> Self {
        Self {
            kind,
            text: text.into(),
            range,
        }
    }

    /// Create a simple token without range info.
    pub fn simple(kind: TokenKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            range: Range::default(),
        }
    }

    /// Check if this token matches a predicate.
    pub fn matches(&self, predicate: &TokenPredicate) -> bool {
        predicate.matches(self)
    }
}

impl Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.kind, self.text)
    }
}

/// Predicate for matching tokens.
#[derive(Debug, Clone)]
pub enum TokenPredicate {
    /// Match any token.
    Any,
    /// Match by exact text.
    Text(String),
    /// Match by text (case-insensitive).
    TextCaseInsensitive(String),
    /// Match by token kind.
    Kind(TokenKind),
    /// Match by kind and text.
    KindAndText(TokenKind, String),
    /// Match if text starts with prefix.
    StartsWith(String),
    /// Match if text ends with suffix.
    EndsWith(String),
    /// Match if text contains substring.
    Contains(String),
    /// Match by regex pattern.
    Regex(String),
    /// Match any of several predicates.
    Any_(Vec<TokenPredicate>),
    /// Match all of several predicates.
    All(Vec<TokenPredicate>),
    /// Negation of a predicate.
    Not(Box<TokenPredicate>),
}

impl TokenPredicate {
    /// Create a text predicate.
    pub fn text(s: impl Into<String>) -> Self {
        TokenPredicate::Text(s.into())
    }

    /// Create a kind predicate.
    pub fn kind(k: TokenKind) -> Self {
        TokenPredicate::Kind(k)
    }

    /// Create an "any of" predicate.
    pub fn any_of(predicates: Vec<TokenPredicate>) -> Self {
        TokenPredicate::Any_(predicates)
    }

    /// Create a "not" predicate.
    pub fn not(predicate: TokenPredicate) -> Self {
        TokenPredicate::Not(Box::new(predicate))
    }

    /// Check if a token matches this predicate.
    pub fn matches(&self, token: &Token) -> bool {
        match self {
            TokenPredicate::Any => true,
            TokenPredicate::Text(t) => token.text == *t,
            TokenPredicate::TextCaseInsensitive(t) => token.text.eq_ignore_ascii_case(t),
            TokenPredicate::Kind(k) => token.kind == *k,
            TokenPredicate::KindAndText(k, t) => token.kind == *k && token.text == *t,
            TokenPredicate::StartsWith(prefix) => token.text.starts_with(prefix),
            TokenPredicate::EndsWith(suffix) => token.text.ends_with(suffix),
            TokenPredicate::Contains(sub) => token.text.contains(sub),
            TokenPredicate::Regex(pattern) => {
                // Simple regex matching - in production, use regex crate
                regex_matches(&token.text, pattern)
            }
            TokenPredicate::Any_(preds) => preds.iter().any(|p| p.matches(token)),
            TokenPredicate::All(preds) => preds.iter().all(|p| p.matches(token)),
            TokenPredicate::Not(pred) => !pred.matches(token),
        }
    }
}

/// Simple regex matching (placeholder - use regex crate in production).
fn regex_matches(text: &str, pattern: &str) -> bool {
    // Very simple pattern matching for common cases
    if pattern == ".*" {
        return true;
    }
    if pattern.starts_with('^') && pattern.ends_with('$') {
        let inner = &pattern[1..pattern.len() - 1];
        return text == inner;
    }
    if pattern.starts_with('^') {
        let prefix = &pattern[1..];
        return text.starts_with(prefix);
    }
    if pattern.ends_with('$') {
        let suffix = &pattern[..pattern.len() - 1];
        return text.ends_with(suffix);
    }
    text.contains(pattern)
}

/// A pattern for matching sequences of tokens.
#[derive(Debug, Clone)]
pub struct TokenPattern {
    /// Elements in the pattern.
    pub elements: Vec<PatternElement>,
    /// Pattern name for debugging.
    pub name: String,
}

/// Element in a token pattern.
#[derive(Debug, Clone)]
pub enum PatternElement {
    /// Match a single token.
    Single(TokenPredicate),
    /// Match zero or more tokens.
    ZeroOrMore(TokenPredicate),
    /// Match one or more tokens.
    OneOrMore(TokenPredicate),
    /// Match zero or one token.
    Optional(TokenPredicate),
    /// Capture a token by name.
    Capture(String, TokenPredicate),
    /// Alternative patterns.
    Alternative(Vec<TokenPattern>),
    /// Look-ahead (doesn't consume).
    LookAhead(TokenPredicate),
    /// Negative look-ahead.
    NegativeLookAhead(TokenPredicate),
}

impl TokenPattern {
    /// Create a new empty pattern.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            elements: Vec::new(),
            name: name.into(),
        }
    }

    /// Add a single-token predicate.
    pub fn then(mut self, pred: TokenPredicate) -> Self {
        self.elements.push(PatternElement::Single(pred));
        self
    }

    /// Add an optional element.
    pub fn optional(mut self, pred: TokenPredicate) -> Self {
        self.elements.push(PatternElement::Optional(pred));
        self
    }

    /// Add a zero-or-more element.
    pub fn zero_or_more(mut self, pred: TokenPredicate) -> Self {
        self.elements.push(PatternElement::ZeroOrMore(pred));
        self
    }

    /// Add a one-or-more element.
    pub fn one_or_more(mut self, pred: TokenPredicate) -> Self {
        self.elements.push(PatternElement::OneOrMore(pred));
        self
    }

    /// Add a capture.
    pub fn capture(mut self, name: impl Into<String>, pred: TokenPredicate) -> Self {
        self.elements
            .push(PatternElement::Capture(name.into(), pred));
        self
    }

    /// Add a look-ahead.
    pub fn look_ahead(mut self, pred: TokenPredicate) -> Self {
        self.elements.push(PatternElement::LookAhead(pred));
        self
    }

    /// Build pattern for exact text sequence.
    pub fn exact_sequence(name: &str, texts: &[&str]) -> Self {
        let mut pattern = Self::new(name);
        for text in texts {
            pattern
                .elements
                .push(PatternElement::Single(TokenPredicate::text(*text)));
        }
        pattern
    }
}

/// Pattern matching result.
#[derive(Debug, Clone)]
pub struct PatternMatch {
    /// Matched tokens.
    pub tokens: Vec<Token>,
    /// Captured groups.
    pub captures: std::collections::HashMap<String, Vec<Token>>,
    /// Start position in token stream.
    pub start_index: usize,
    /// End position in token stream (exclusive).
    pub end_index: usize,
}

impl PatternMatch {
    /// Get captured tokens by name.
    pub fn get(&self, name: &str) -> Option<&[Token]> {
        self.captures.get(name).map(|v| v.as_slice())
    }

    /// Get the first captured token by name.
    pub fn get_one(&self, name: &str) -> Option<&Token> {
        self.captures.get(name).and_then(|v| v.first())
    }

    /// Get the range of matched tokens.
    pub fn range(&self) -> Option<Range> {
        if self.tokens.is_empty() {
            return None;
        }
        let start = self.tokens.first().unwrap().range.start;
        let end = self.tokens.last().unwrap().range.end;
        Some(Range::new(start, end))
    }
}

/// Pattern matcher for token streams.
#[derive(Debug, Clone)]
pub struct PatternMatcher {
    /// Patterns to match.
    patterns: Vec<TokenPattern>,
}

impl PatternMatcher {
    /// Create a new pattern matcher.
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    /// Add a pattern.
    pub fn add_pattern(&mut self, pattern: TokenPattern) {
        self.patterns.push(pattern);
    }

    /// Find all matches of all patterns in a token stream.
    pub fn find_all_matches(&self, tokens: &[Token]) -> Vec<(String, PatternMatch)> {
        let mut results = Vec::new();

        for pattern in &self.patterns {
            let matches = self.find_pattern(pattern, tokens);
            for m in matches {
                results.push((pattern.name.clone(), m));
            }
        }

        results
    }

    /// Find all matches of a specific pattern.
    fn find_pattern(&self, pattern: &TokenPattern, tokens: &[Token]) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        for start in 0..tokens.len() {
            if let Some(m) = self.try_match_at(pattern, tokens, start) {
                matches.push(m);
            }
        }

        matches
    }

    /// Try to match a pattern at a specific position.
    fn try_match_at(
        &self,
        pattern: &TokenPattern,
        tokens: &[Token],
        start: usize,
    ) -> Option<PatternMatch> {
        let mut pos = start;
        let mut matched_tokens = Vec::new();
        let mut captures = std::collections::HashMap::new();

        for element in &pattern.elements {
            match element {
                PatternElement::Single(pred) => {
                    if pos >= tokens.len() || !pred.matches(&tokens[pos]) {
                        return None;
                    }
                    matched_tokens.push(tokens[pos].clone());
                    pos += 1;
                }
                PatternElement::Optional(pred) => {
                    if pos < tokens.len() && pred.matches(&tokens[pos]) {
                        matched_tokens.push(tokens[pos].clone());
                        pos += 1;
                    }
                }
                PatternElement::ZeroOrMore(pred) => {
                    while pos < tokens.len() && pred.matches(&tokens[pos]) {
                        matched_tokens.push(tokens[pos].clone());
                        pos += 1;
                    }
                }
                PatternElement::OneOrMore(pred) => {
                    if pos >= tokens.len() || !pred.matches(&tokens[pos]) {
                        return None;
                    }
                    while pos < tokens.len() && pred.matches(&tokens[pos]) {
                        matched_tokens.push(tokens[pos].clone());
                        pos += 1;
                    }
                }
                PatternElement::Capture(name, pred) => {
                    if pos >= tokens.len() || !pred.matches(&tokens[pos]) {
                        return None;
                    }
                    captures
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(tokens[pos].clone());
                    matched_tokens.push(tokens[pos].clone());
                    pos += 1;
                }
                PatternElement::Alternative(alts) => {
                    let mut found = false;
                    for alt in alts {
                        if let Some(sub_match) = self.try_match_at(alt, tokens, pos) {
                            matched_tokens.extend(sub_match.tokens);
                            for (k, v) in sub_match.captures {
                                captures.entry(k).or_insert_with(Vec::new).extend(v);
                            }
                            pos = sub_match.end_index;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return None;
                    }
                }
                PatternElement::LookAhead(pred) => {
                    if pos >= tokens.len() || !pred.matches(&tokens[pos]) {
                        return None;
                    }
                    // Don't consume
                }
                PatternElement::NegativeLookAhead(pred) => {
                    if pos < tokens.len() && pred.matches(&tokens[pos]) {
                        return None;
                    }
                    // Don't consume
                }
            }
        }

        Some(PatternMatch {
            tokens: matched_tokens,
            captures,
            start_index: start,
            end_index: pos,
        })
    }
}

impl Default for PatternMatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Replacement action for tokens.
#[derive(Debug, Clone)]
pub enum ReplacementAction {
    /// Delete the matched tokens.
    Delete,
    /// Replace with specific text.
    Replace(String),
    /// Replace with tokens.
    ReplaceTokens(Vec<Token>),
    /// Insert text before.
    InsertBefore(String),
    /// Insert text after.
    InsertAfter(String),
    /// Apply a transform function (by name for serialization).
    Transform(String),
}

/// Token replacement rule.
#[derive(Debug, Clone)]
pub struct TokenReplacement {
    /// Pattern to match.
    pub pattern: TokenPattern,
    /// Replacement action.
    pub action: ReplacementAction,
    /// Cost of this replacement.
    pub cost: f64,
    /// Description for diagnostics.
    pub description: String,
}

impl TokenReplacement {
    /// Create a new replacement rule.
    pub fn new(
        pattern: TokenPattern,
        action: ReplacementAction,
        cost: f64,
        description: impl Into<String>,
    ) -> Self {
        Self {
            pattern,
            action,
            cost,
            description: description.into(),
        }
    }

    /// Create a deletion rule.
    pub fn delete(pattern: TokenPattern, cost: f64, description: &str) -> Self {
        Self::new(pattern, ReplacementAction::Delete, cost, description)
    }

    /// Create a substitution rule.
    pub fn substitute(from: &str, to: &str, cost: f64, description: &str) -> Self {
        let pattern =
            TokenPattern::new(format!("substitute_{}", from)).then(TokenPredicate::text(from));

        Self::new(
            pattern,
            ReplacementAction::Replace(to.to_string()),
            cost,
            description,
        )
    }

    /// Create an insertion rule.
    pub fn insert_after(after: &str, insert: &str, cost: f64, description: &str) -> Self {
        let pattern =
            TokenPattern::new(format!("insert_after_{}", after)).then(TokenPredicate::text(after));

        Self::new(
            pattern,
            ReplacementAction::InsertAfter(insert.to_string()),
            cost,
            description,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_creation() {
        let token = Token::simple(TokenKind::Keyword, "function");
        assert_eq!(token.kind, TokenKind::Keyword);
        assert_eq!(token.text, "function");
    }

    #[test]
    fn test_token_predicate_text() {
        let token = Token::simple(TokenKind::Keyword, "function");
        let pred = TokenPredicate::text("function");
        assert!(pred.matches(&token));

        let pred2 = TokenPredicate::text("class");
        assert!(!pred2.matches(&token));
    }

    #[test]
    fn test_token_predicate_kind() {
        let token = Token::simple(TokenKind::Identifier, "foo");
        assert!(TokenPredicate::kind(TokenKind::Identifier).matches(&token));
        assert!(!TokenPredicate::kind(TokenKind::Keyword).matches(&token));
    }

    #[test]
    fn test_token_predicate_any() {
        let token = Token::simple(TokenKind::Identifier, "test");
        assert!(TokenPredicate::Any.matches(&token));
    }

    #[test]
    fn test_token_predicate_case_insensitive() {
        let token = Token::simple(TokenKind::Keyword, "FUNCTION");
        let pred = TokenPredicate::TextCaseInsensitive("function".to_string());
        assert!(pred.matches(&token));
    }

    #[test]
    fn test_token_predicate_starts_with() {
        let token = Token::simple(TokenKind::Identifier, "fooBar");
        assert!(TokenPredicate::StartsWith("foo".to_string()).matches(&token));
        assert!(!TokenPredicate::StartsWith("bar".to_string()).matches(&token));
    }

    #[test]
    fn test_token_predicate_any_of() {
        let token = Token::simple(TokenKind::Keyword, "if");
        let pred = TokenPredicate::any_of(vec![
            TokenPredicate::text("if"),
            TokenPredicate::text("while"),
            TokenPredicate::text("for"),
        ]);
        assert!(pred.matches(&token));

        let token2 = Token::simple(TokenKind::Keyword, "else");
        assert!(!pred.matches(&token2));
    }

    #[test]
    fn test_token_predicate_not() {
        let token = Token::simple(TokenKind::Identifier, "foo");
        let pred = TokenPredicate::not(TokenPredicate::kind(TokenKind::Keyword));
        assert!(pred.matches(&token));

        let keyword = Token::simple(TokenKind::Keyword, "if");
        assert!(!pred.matches(&keyword));
    }

    #[test]
    fn test_pattern_single() {
        let tokens = vec![
            Token::simple(TokenKind::Keyword, "function"),
            Token::simple(TokenKind::Identifier, "foo"),
        ];

        let pattern = TokenPattern::new("function_decl")
            .then(TokenPredicate::text("function"))
            .then(TokenPredicate::kind(TokenKind::Identifier));

        let matcher = PatternMatcher::new();
        let result = matcher.try_match_at(&pattern, &tokens, 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tokens.len(), 2);
    }

    #[test]
    fn test_pattern_optional() {
        let tokens = vec![Token::simple(TokenKind::Keyword, "return")];

        let pattern = TokenPattern::new("return_stmt")
            .then(TokenPredicate::text("return"))
            .optional(TokenPredicate::kind(TokenKind::Identifier));

        let matcher = PatternMatcher::new();
        let result = matcher.try_match_at(&pattern, &tokens, 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tokens.len(), 1);
    }

    #[test]
    fn test_pattern_zero_or_more() {
        let tokens = vec![
            Token::simple(TokenKind::Identifier, "a"),
            Token::simple(TokenKind::Identifier, "b"),
            Token::simple(TokenKind::Identifier, "c"),
            Token::simple(TokenKind::Punctuation, ";"),
        ];

        let pattern = TokenPattern::new("identifiers")
            .zero_or_more(TokenPredicate::kind(TokenKind::Identifier));

        let matcher = PatternMatcher::new();
        let result = matcher.try_match_at(&pattern, &tokens, 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tokens.len(), 3);
    }

    #[test]
    fn test_pattern_capture() {
        let tokens = vec![
            Token::simple(TokenKind::Keyword, "let"),
            Token::simple(TokenKind::Identifier, "x"),
            Token::simple(TokenKind::Operator, "="),
        ];

        let pattern = TokenPattern::new("let_binding")
            .then(TokenPredicate::text("let"))
            .capture("name", TokenPredicate::kind(TokenKind::Identifier));

        let matcher = PatternMatcher::new();
        let result = matcher.try_match_at(&pattern, &tokens, 0);
        assert!(result.is_some());

        let m = result.unwrap();
        assert!(m.get("name").is_some());
        assert_eq!(m.get_one("name").unwrap().text, "x");
    }

    #[test]
    fn test_pattern_matcher_find_all() {
        let tokens = vec![
            Token::simple(TokenKind::Keyword, "if"),
            Token::simple(TokenKind::Punctuation, "("),
            Token::simple(TokenKind::Identifier, "x"),
            Token::simple(TokenKind::Punctuation, ")"),
            Token::simple(TokenKind::Keyword, "if"),
            Token::simple(TokenKind::Punctuation, "("),
        ];

        let pattern = TokenPattern::new("if_stmt")
            .then(TokenPredicate::text("if"))
            .then(TokenPredicate::text("("));

        let mut matcher = PatternMatcher::new();
        matcher.add_pattern(pattern);

        let matches = matcher.find_all_matches(&tokens);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].1.start_index, 0);
        assert_eq!(matches[1].1.start_index, 4);
    }

    #[test]
    fn test_token_replacement_substitute() {
        let replacement = TokenReplacement::substitute(
            "funciton",
            "function",
            0.1,
            "Fix typo in function keyword",
        );

        assert!(matches!(replacement.action, ReplacementAction::Replace(_)));
        assert!((replacement.cost - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_token_replacement_insert_after() {
        let replacement =
            TokenReplacement::insert_after("}", ";", 0.5, "Insert missing semicolon after block");

        assert!(matches!(
            replacement.action,
            ReplacementAction::InsertAfter(_)
        ));
    }

    #[test]
    fn test_exact_sequence() {
        let pattern = TokenPattern::exact_sequence("arrow_function", &["(", ")", "=>"]);

        assert_eq!(pattern.elements.len(), 3);
        assert_eq!(pattern.name, "arrow_function");
    }
}
