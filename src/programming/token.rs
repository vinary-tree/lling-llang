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

impl Clone for TokenPredicate {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Predicate(&'a TokenPredicate),
            Any(usize),
            All(usize),
            Not,
        }
        let mut tasks = vec![Task::Predicate(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Predicate(predicate) => match predicate {
                    TokenPredicate::Any => values.push(TokenPredicate::Any),
                    TokenPredicate::Text(text) => {
                        values.push(TokenPredicate::Text(text.clone()));
                    }
                    TokenPredicate::TextCaseInsensitive(text) => {
                        values.push(TokenPredicate::TextCaseInsensitive(text.clone()));
                    }
                    TokenPredicate::Kind(kind) => values.push(TokenPredicate::Kind(*kind)),
                    TokenPredicate::KindAndText(kind, text) => {
                        values.push(TokenPredicate::KindAndText(*kind, text.clone()));
                    }
                    TokenPredicate::StartsWith(prefix) => {
                        values.push(TokenPredicate::StartsWith(prefix.clone()));
                    }
                    TokenPredicate::EndsWith(suffix) => {
                        values.push(TokenPredicate::EndsWith(suffix.clone()));
                    }
                    TokenPredicate::Contains(fragment) => {
                        values.push(TokenPredicate::Contains(fragment.clone()));
                    }
                    TokenPredicate::Regex(pattern) => {
                        values.push(TokenPredicate::Regex(pattern.clone()));
                    }
                    TokenPredicate::Any_(predicates) | TokenPredicate::All(predicates) => {
                        tasks.push(if matches!(predicate, TokenPredicate::Any_(_)) {
                            Task::Any(predicates.len())
                        } else {
                            Task::All(predicates.len())
                        });
                        tasks.extend(predicates.iter().rev().map(Task::Predicate));
                    }
                    TokenPredicate::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Predicate(inner));
                    }
                },
                Task::Any(length) | Task::All(length) => {
                    let offset = values
                        .len()
                        .checked_sub(length)
                        .expect("all token-predicate children are cloned");
                    let children = values.split_off(offset);
                    values.push(if matches!(task, Task::Any(_)) {
                        TokenPredicate::Any_(children)
                    } else {
                        TokenPredicate::All(children)
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated token predicate is cloned");
                    values.push(TokenPredicate::Not(Box::new(inner)));
                }
            }
        }
        values
            .pop()
            .expect("the root token predicate produces one clone")
    }
}

impl Debug for TokenPredicate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Predicate(&'a TokenPredicate),
            Text(&'static str),
        }
        let mut events = vec![Event::Predicate(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => formatter.write_str(text)?,
                Event::Predicate(predicate) => match predicate {
                    TokenPredicate::Any => formatter.write_str("Any")?,
                    TokenPredicate::Text(text) => write!(formatter, "Text({text:?})")?,
                    TokenPredicate::TextCaseInsensitive(text) => {
                        write!(formatter, "TextCaseInsensitive({text:?})")?;
                    }
                    TokenPredicate::Kind(kind) => write!(formatter, "Kind({kind:?})")?,
                    TokenPredicate::KindAndText(kind, text) => {
                        write!(formatter, "KindAndText({kind:?}, {text:?})")?;
                    }
                    TokenPredicate::StartsWith(prefix) => {
                        write!(formatter, "StartsWith({prefix:?})")?;
                    }
                    TokenPredicate::EndsWith(suffix) => {
                        write!(formatter, "EndsWith({suffix:?})")?;
                    }
                    TokenPredicate::Contains(fragment) => {
                        write!(formatter, "Contains({fragment:?})")?;
                    }
                    TokenPredicate::Regex(pattern) => {
                        write!(formatter, "Regex({pattern:?})")?;
                    }
                    TokenPredicate::Any_(predicates) | TokenPredicate::All(predicates) => {
                        formatter.write_str(if matches!(predicate, TokenPredicate::Any_(_)) {
                            "Any_(["
                        } else {
                            "All(["
                        })?;
                        events.push(Event::Text("])"));
                        for (index, child) in predicates.iter().enumerate().rev() {
                            if index + 1 < predicates.len() {
                                events.push(Event::Text(", "));
                            }
                            events.push(Event::Predicate(child));
                        }
                    }
                    TokenPredicate::Not(inner) => {
                        formatter.write_str("Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Predicate(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for TokenPredicate {
    fn drop(&mut self) {
        fn drain(predicate: &mut TokenPredicate, pending: &mut Vec<TokenPredicate>) {
            match predicate {
                TokenPredicate::Any_(children) | TokenPredicate::All(children) => {
                    pending.append(children);
                }
                TokenPredicate::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, TokenPredicate::Any));
                }
                TokenPredicate::Any
                | TokenPredicate::Text(_)
                | TokenPredicate::TextCaseInsensitive(_)
                | TokenPredicate::Kind(_)
                | TokenPredicate::KindAndText(_, _)
                | TokenPredicate::StartsWith(_)
                | TokenPredicate::EndsWith(_)
                | TokenPredicate::Contains(_)
                | TokenPredicate::Regex(_) => {}
            }
        }

        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain(&mut predicate, &mut pending);
        }
    }
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
        enum Frame<'a> {
            Eval(&'a TokenPredicate),
            Not,
            Any {
                predicates: &'a [TokenPredicate],
                next: usize,
            },
            All {
                predicates: &'a [TokenPredicate],
                next: usize,
            },
        }
        let mut frames = vec![Frame::Eval(self)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    TokenPredicate::Any => result = Some(true),
                    TokenPredicate::Text(text) => result = Some(token.text == *text),
                    TokenPredicate::TextCaseInsensitive(text) => {
                        result = Some(token.text.eq_ignore_ascii_case(text));
                    }
                    TokenPredicate::Kind(kind) => result = Some(token.kind == *kind),
                    TokenPredicate::KindAndText(kind, text) => {
                        result = Some(token.kind == *kind && token.text == *text);
                    }
                    TokenPredicate::StartsWith(prefix) => {
                        result = Some(token.text.starts_with(prefix));
                    }
                    TokenPredicate::EndsWith(suffix) => {
                        result = Some(token.text.ends_with(suffix));
                    }
                    TokenPredicate::Contains(fragment) => {
                        result = Some(token.text.contains(fragment));
                    }
                    TokenPredicate::Regex(pattern) => {
                        result = Some(regex_matches(&token.text, pattern));
                    }
                    TokenPredicate::Any_(predicates) => {
                        if let Some(first) = predicates.first() {
                            frames.push(Frame::Any {
                                predicates,
                                next: 1,
                            });
                            frames.push(Frame::Eval(first));
                        } else {
                            result = Some(false);
                        }
                    }
                    TokenPredicate::All(predicates) => {
                        if let Some(first) = predicates.first() {
                            frames.push(Frame::All {
                                predicates,
                                next: 1,
                            });
                            frames.push(Frame::Eval(first));
                        } else {
                            result = Some(true);
                        }
                    }
                    TokenPredicate::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => {
                    result = Some(!result.take().expect("negated token predicate is evaluated"));
                }
                Frame::Any { predicates, next } => {
                    if result.take().expect("Any child is evaluated") {
                        result = Some(true);
                    } else if let Some(predicate) = predicates.get(next) {
                        frames.push(Frame::Any {
                            predicates,
                            next: next + 1,
                        });
                        frames.push(Frame::Eval(predicate));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::All { predicates, next } => {
                    if result.take().expect("All child is evaluated") {
                        if let Some(predicate) = predicates.get(next) {
                            frames.push(Frame::All {
                                predicates,
                                next: next + 1,
                            });
                            frames.push(Frame::Eval(predicate));
                        } else {
                            result = Some(true);
                        }
                    } else {
                        result = Some(false);
                    }
                }
            }
        }
        result.expect("the root token predicate produces one Boolean result")
    }
}

/// Regex matching with invalid patterns treated as non-matches.
fn regex_matches(text: &str, pattern: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|regex| regex.is_match(text))
        .unwrap_or(false)
}

/// A pattern for matching sequences of tokens.
pub struct TokenPattern {
    /// Elements in the pattern.
    pub elements: Vec<PatternElement>,
    /// Pattern name for debugging.
    pub name: String,
}

/// Element in a token pattern.
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

enum PatternCloneTask<'a> {
    Pattern(&'a TokenPattern),
    FinishPattern { element_count: usize, name: &'a str },
    Element(&'a PatternElement),
    FinishAlternative(usize),
}

enum PatternCloneValue {
    Pattern(TokenPattern),
    Element(PatternElement),
}

fn clone_pattern_syntax(root: PatternCloneTask<'_>) -> PatternCloneValue {
    let mut tasks = vec![root];
    let mut values = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            PatternCloneTask::Pattern(pattern) => {
                tasks.push(PatternCloneTask::FinishPattern {
                    element_count: pattern.elements.len(),
                    name: &pattern.name,
                });
                tasks.extend(pattern.elements.iter().rev().map(PatternCloneTask::Element));
            }
            PatternCloneTask::FinishPattern {
                element_count,
                name,
            } => {
                let offset = values
                    .len()
                    .checked_sub(element_count)
                    .expect("all token-pattern elements are cloned");
                let elements = values
                    .split_off(offset)
                    .into_iter()
                    .map(|value| match value {
                        PatternCloneValue::Element(element) => element,
                        PatternCloneValue::Pattern(_) => {
                            unreachable!("a pattern clone contains only element values")
                        }
                    })
                    .collect();
                values.push(PatternCloneValue::Pattern(TokenPattern {
                    elements,
                    name: name.to_owned(),
                }));
            }
            PatternCloneTask::Element(element) => match element {
                PatternElement::Single(predicate) => values.push(PatternCloneValue::Element(
                    PatternElement::Single(predicate.clone()),
                )),
                PatternElement::ZeroOrMore(predicate) => values.push(PatternCloneValue::Element(
                    PatternElement::ZeroOrMore(predicate.clone()),
                )),
                PatternElement::OneOrMore(predicate) => values.push(PatternCloneValue::Element(
                    PatternElement::OneOrMore(predicate.clone()),
                )),
                PatternElement::Optional(predicate) => values.push(PatternCloneValue::Element(
                    PatternElement::Optional(predicate.clone()),
                )),
                PatternElement::Capture(name, predicate) => {
                    values.push(PatternCloneValue::Element(PatternElement::Capture(
                        name.clone(),
                        predicate.clone(),
                    )))
                }
                PatternElement::Alternative(alternatives) => {
                    tasks.push(PatternCloneTask::FinishAlternative(alternatives.len()));
                    tasks.extend(alternatives.iter().rev().map(PatternCloneTask::Pattern));
                }
                PatternElement::LookAhead(predicate) => values.push(PatternCloneValue::Element(
                    PatternElement::LookAhead(predicate.clone()),
                )),
                PatternElement::NegativeLookAhead(predicate) => {
                    values.push(PatternCloneValue::Element(
                        PatternElement::NegativeLookAhead(predicate.clone()),
                    ))
                }
            },
            PatternCloneTask::FinishAlternative(pattern_count) => {
                let offset = values
                    .len()
                    .checked_sub(pattern_count)
                    .expect("all alternative token patterns are cloned");
                let alternatives = values
                    .split_off(offset)
                    .into_iter()
                    .map(|value| match value {
                        PatternCloneValue::Pattern(pattern) => pattern,
                        PatternCloneValue::Element(_) => {
                            unreachable!("an alternative clone contains only pattern values")
                        }
                    })
                    .collect();
                values.push(PatternCloneValue::Element(PatternElement::Alternative(
                    alternatives,
                )));
            }
        }
    }
    values
        .pop()
        .expect("the root token-pattern syntax produces one clone")
}

impl Clone for TokenPattern {
    fn clone(&self) -> Self {
        match clone_pattern_syntax(PatternCloneTask::Pattern(self)) {
            PatternCloneValue::Pattern(pattern) => pattern,
            PatternCloneValue::Element(_) => {
                unreachable!("a token-pattern clone produces a pattern")
            }
        }
    }
}

impl Clone for PatternElement {
    fn clone(&self) -> Self {
        match clone_pattern_syntax(PatternCloneTask::Element(self)) {
            PatternCloneValue::Element(element) => element,
            PatternCloneValue::Pattern(_) => {
                unreachable!("a pattern-element clone produces an element")
            }
        }
    }
}

enum PatternDebugEvent<'a> {
    Pattern(&'a TokenPattern),
    Element(&'a PatternElement),
    PatternSuffix(&'a str),
    Text(&'static str),
}

fn debug_pattern_syntax(
    root: PatternDebugEvent<'_>,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    let mut events = vec![root];
    while let Some(event) = events.pop() {
        match event {
            PatternDebugEvent::Text(text) => formatter.write_str(text)?,
            PatternDebugEvent::PatternSuffix(name) => {
                write!(formatter, "], name: {name:?} }}")?;
            }
            PatternDebugEvent::Pattern(pattern) => {
                formatter.write_str("TokenPattern { elements: [")?;
                events.push(PatternDebugEvent::PatternSuffix(&pattern.name));
                for (index, element) in pattern.elements.iter().enumerate().rev() {
                    if index + 1 < pattern.elements.len() {
                        events.push(PatternDebugEvent::Text(", "));
                    }
                    events.push(PatternDebugEvent::Element(element));
                }
            }
            PatternDebugEvent::Element(element) => match element {
                PatternElement::Single(predicate) => {
                    write!(formatter, "Single({predicate:?})")?;
                }
                PatternElement::ZeroOrMore(predicate) => {
                    write!(formatter, "ZeroOrMore({predicate:?})")?;
                }
                PatternElement::OneOrMore(predicate) => {
                    write!(formatter, "OneOrMore({predicate:?})")?;
                }
                PatternElement::Optional(predicate) => {
                    write!(formatter, "Optional({predicate:?})")?;
                }
                PatternElement::Capture(name, predicate) => {
                    write!(formatter, "Capture({name:?}, {predicate:?})")?;
                }
                PatternElement::Alternative(alternatives) => {
                    formatter.write_str("Alternative([")?;
                    events.push(PatternDebugEvent::Text("])"));
                    for (index, pattern) in alternatives.iter().enumerate().rev() {
                        if index + 1 < alternatives.len() {
                            events.push(PatternDebugEvent::Text(", "));
                        }
                        events.push(PatternDebugEvent::Pattern(pattern));
                    }
                }
                PatternElement::LookAhead(predicate) => {
                    write!(formatter, "LookAhead({predicate:?})")?;
                }
                PatternElement::NegativeLookAhead(predicate) => {
                    write!(formatter, "NegativeLookAhead({predicate:?})")?;
                }
            },
        }
    }
    Ok(())
}

impl Debug for TokenPattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_pattern_syntax(PatternDebugEvent::Pattern(self), formatter)
    }
}

impl Debug for PatternElement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_pattern_syntax(PatternDebugEvent::Element(self), formatter)
    }
}

enum OwnedPatternSyntax {
    Pattern(TokenPattern),
    Element(PatternElement),
}

fn drain_pattern_element(element: &mut PatternElement, pending: &mut Vec<OwnedPatternSyntax>) {
    if let PatternElement::Alternative(alternatives) = element {
        pending.extend(
            std::mem::take(alternatives)
                .into_iter()
                .map(OwnedPatternSyntax::Pattern),
        );
    }
}

fn drain_owned_pattern_syntax(pending: &mut Vec<OwnedPatternSyntax>) {
    while let Some(mut syntax) = pending.pop() {
        match &mut syntax {
            OwnedPatternSyntax::Pattern(pattern) => pending.extend(
                std::mem::take(&mut pattern.elements)
                    .into_iter()
                    .map(OwnedPatternSyntax::Element),
            ),
            OwnedPatternSyntax::Element(element) => drain_pattern_element(element, pending),
        }
    }
}

impl Drop for TokenPattern {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.elements)
            .into_iter()
            .map(OwnedPatternSyntax::Element)
            .collect::<Vec<_>>();
        drain_owned_pattern_syntax(&mut pending);
    }
}

impl Drop for PatternElement {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        drain_pattern_element(self, &mut pending);
        drain_owned_pattern_syntax(&mut pending);
    }
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
        let start = self
            .tokens
            .first()
            .expect("programming/token.rs: required value was None/Err")
            .range
            .start;
        let end = self
            .tokens
            .last()
            .expect("programming/token.rs: required value was None/Err")
            .range
            .end;
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
        struct MatchState {
            pos: usize,
            matched_tokens: Vec<Token>,
            captures: std::collections::HashMap<String, Vec<Token>>,
        }

        struct Cursor<'a> {
            pattern: &'a TokenPattern,
            next_element: usize,
            state: MatchState,
        }

        struct AlternativeFrame<'a> {
            alternatives: &'a [TokenPattern],
            next_alternative: usize,
            parent: Cursor<'a>,
        }

        fn fresh_cursor(pattern: &TokenPattern, pos: usize) -> Cursor<'_> {
            Cursor {
                pattern,
                next_element: 0,
                state: MatchState {
                    pos,
                    matched_tokens: Vec::new(),
                    captures: std::collections::HashMap::new(),
                },
            }
        }

        fn resume_after_failure<'a>(
            alternatives: &mut Vec<AlternativeFrame<'a>>,
        ) -> Option<Cursor<'a>> {
            while let Some(mut frame) = alternatives.pop() {
                if let Some(pattern) = frame.alternatives.get(frame.next_alternative) {
                    frame.next_alternative += 1;
                    let pos = frame.parent.state.pos;
                    alternatives.push(frame);
                    return Some(fresh_cursor(pattern, pos));
                }
            }
            None
        }

        let mut cursor = fresh_cursor(pattern, start);
        let mut alternatives: Vec<AlternativeFrame<'_>> = Vec::new();

        loop {
            let Some(element) = cursor.pattern.elements.get(cursor.next_element) else {
                if let Some(frame) = alternatives.pop() {
                    let completed = cursor.state;
                    cursor = frame.parent;
                    cursor.state.pos = completed.pos;
                    cursor.state.matched_tokens.extend(completed.matched_tokens);
                    for (name, captured) in completed.captures {
                        cursor
                            .state
                            .captures
                            .entry(name)
                            .or_insert_with(Vec::new)
                            .extend(captured);
                    }
                    continue;
                }

                return Some(PatternMatch {
                    tokens: cursor.state.matched_tokens,
                    captures: cursor.state.captures,
                    start_index: start,
                    end_index: cursor.state.pos,
                });
            };
            cursor.next_element += 1;

            let matched = match element {
                PatternElement::Single(pred) => {
                    if cursor.state.pos >= tokens.len() || !pred.matches(&tokens[cursor.state.pos])
                    {
                        false
                    } else {
                        cursor
                            .state
                            .matched_tokens
                            .push(tokens[cursor.state.pos].clone());
                        cursor.state.pos += 1;
                        true
                    }
                }
                PatternElement::Optional(pred) => {
                    if cursor.state.pos < tokens.len() && pred.matches(&tokens[cursor.state.pos]) {
                        cursor
                            .state
                            .matched_tokens
                            .push(tokens[cursor.state.pos].clone());
                        cursor.state.pos += 1;
                    }
                    true
                }
                PatternElement::ZeroOrMore(pred) => {
                    while cursor.state.pos < tokens.len() && pred.matches(&tokens[cursor.state.pos])
                    {
                        cursor
                            .state
                            .matched_tokens
                            .push(tokens[cursor.state.pos].clone());
                        cursor.state.pos += 1;
                    }
                    true
                }
                PatternElement::OneOrMore(pred) => {
                    if cursor.state.pos >= tokens.len() || !pred.matches(&tokens[cursor.state.pos])
                    {
                        false
                    } else {
                        while cursor.state.pos < tokens.len()
                            && pred.matches(&tokens[cursor.state.pos])
                        {
                            cursor
                                .state
                                .matched_tokens
                                .push(tokens[cursor.state.pos].clone());
                            cursor.state.pos += 1;
                        }
                        true
                    }
                }
                PatternElement::Capture(name, pred) => {
                    if cursor.state.pos >= tokens.len() || !pred.matches(&tokens[cursor.state.pos])
                    {
                        false
                    } else {
                        cursor
                            .state
                            .captures
                            .entry(name.clone())
                            .or_insert_with(Vec::new)
                            .push(tokens[cursor.state.pos].clone());
                        cursor
                            .state
                            .matched_tokens
                            .push(tokens[cursor.state.pos].clone());
                        cursor.state.pos += 1;
                        true
                    }
                }
                PatternElement::Alternative(candidates) => {
                    if let Some(first) = candidates.first() {
                        let pos = cursor.state.pos;
                        alternatives.push(AlternativeFrame {
                            alternatives: candidates,
                            next_alternative: 1,
                            parent: cursor,
                        });
                        cursor = fresh_cursor(first, pos);
                        continue;
                    }
                    false
                }
                PatternElement::LookAhead(pred) => {
                    cursor.state.pos < tokens.len() && pred.matches(&tokens[cursor.state.pos])
                }
                PatternElement::NegativeLookAhead(pred) => {
                    cursor.state.pos >= tokens.len() || !pred.matches(&tokens[cursor.state.pos])
                }
            };

            if !matched {
                cursor = resume_after_failure(&mut alternatives)?;
            }
        }
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
    use proptest::prelude::*;

    const DEEP_PATTERN_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    fn shallow_text() -> BoxedStrategy<String> {
        prop::sample::select(vec!["a".to_string(), "b".to_string(), "c".to_string()]).boxed()
    }

    fn shallow_predicate() -> BoxedStrategy<TokenPredicate> {
        shallow_text().prop_map(TokenPredicate::text).boxed()
    }

    fn shallow_primitive_element() -> BoxedStrategy<PatternElement> {
        prop_oneof![
            4 => shallow_predicate().prop_map(PatternElement::Single),
            2 => shallow_predicate().prop_map(PatternElement::ZeroOrMore),
            2 => shallow_predicate().prop_map(PatternElement::OneOrMore),
            2 => shallow_predicate().prop_map(PatternElement::Optional),
            2 => (shallow_text(), shallow_predicate())
                .prop_map(|(name, predicate)| PatternElement::Capture(name, predicate)),
            1 => shallow_predicate().prop_map(PatternElement::LookAhead),
            1 => shallow_predicate().prop_map(PatternElement::NegativeLookAhead),
        ]
        .boxed()
    }

    fn shallow_pattern() -> BoxedStrategy<TokenPattern> {
        prop::collection::vec(shallow_primitive_element(), 0..5)
            .prop_map(|elements| TokenPattern {
                elements,
                name: "leaf".to_string(),
            })
            .prop_recursive(4, 64, 4, |inner| {
                (
                    prop::collection::vec(shallow_primitive_element(), 0..3),
                    prop::collection::vec(inner, 0..4),
                    prop::collection::vec(shallow_primitive_element(), 0..3),
                )
                    .prop_map(|(mut prefix, alternatives, suffix)| {
                        prefix.push(PatternElement::Alternative(alternatives));
                        prefix.extend(suffix);
                        TokenPattern {
                            elements: prefix,
                            name: "nested".to_string(),
                        }
                    })
            })
            .boxed()
    }

    fn shallow_tokens() -> BoxedStrategy<Vec<Token>> {
        prop::collection::vec(
            shallow_text().prop_map(|text| Token::simple(TokenKind::Identifier, text)),
            0..12,
        )
        .boxed()
    }

    fn recursive_try_match_at(
        pattern: &TokenPattern,
        tokens: &[Token],
        start: usize,
    ) -> Option<PatternMatch> {
        let mut pos = start;
        let mut matched_tokens = Vec::new();
        let mut captures = std::collections::HashMap::new();

        for element in &pattern.elements {
            match element {
                PatternElement::Single(predicate) => {
                    if pos >= tokens.len() || !predicate.matches(&tokens[pos]) {
                        return None;
                    }
                    matched_tokens.push(tokens[pos].clone());
                    pos += 1;
                }
                PatternElement::Optional(predicate) => {
                    if pos < tokens.len() && predicate.matches(&tokens[pos]) {
                        matched_tokens.push(tokens[pos].clone());
                        pos += 1;
                    }
                }
                PatternElement::ZeroOrMore(predicate) => {
                    while pos < tokens.len() && predicate.matches(&tokens[pos]) {
                        matched_tokens.push(tokens[pos].clone());
                        pos += 1;
                    }
                }
                PatternElement::OneOrMore(predicate) => {
                    if pos >= tokens.len() || !predicate.matches(&tokens[pos]) {
                        return None;
                    }
                    while pos < tokens.len() && predicate.matches(&tokens[pos]) {
                        matched_tokens.push(tokens[pos].clone());
                        pos += 1;
                    }
                }
                PatternElement::Capture(name, predicate) => {
                    if pos >= tokens.len() || !predicate.matches(&tokens[pos]) {
                        return None;
                    }
                    captures
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(tokens[pos].clone());
                    matched_tokens.push(tokens[pos].clone());
                    pos += 1;
                }
                PatternElement::Alternative(alternatives) => {
                    let mut matched = None;
                    for alternative in alternatives {
                        if let Some(candidate) = recursive_try_match_at(alternative, tokens, pos) {
                            matched = Some(candidate);
                            break;
                        }
                    }
                    let candidate = matched?;
                    matched_tokens.extend(candidate.tokens);
                    for (name, captured) in candidate.captures {
                        captures
                            .entry(name)
                            .or_insert_with(Vec::new)
                            .extend(captured);
                    }
                    pos = candidate.end_index;
                }
                PatternElement::LookAhead(predicate) => {
                    if pos >= tokens.len() || !predicate.matches(&tokens[pos]) {
                        return None;
                    }
                }
                PatternElement::NegativeLookAhead(predicate) => {
                    if pos < tokens.len() && predicate.matches(&tokens[pos]) {
                        return None;
                    }
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
    fn test_token_predicate_regex() {
        let token = Token::simple(TokenKind::Identifier, "foo123");

        assert!(TokenPredicate::Regex(r"^[a-z]+\d+$".to_string()).matches(&token));
        assert!(!TokenPredicate::Regex(r"^\d+$".to_string()).matches(&token));
        assert!(!TokenPredicate::Regex("[".to_string()).matches(&token));
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
        assert_eq!(
            result
                .expect("programming/token.rs: required value was None/Err")
                .tokens
                .len(),
            2
        );
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
        assert_eq!(
            result
                .expect("programming/token.rs: required value was None/Err")
                .tokens
                .len(),
            1
        );
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
        assert_eq!(
            result
                .expect("programming/token.rs: required value was None/Err")
                .tokens
                .len(),
            3
        );
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

        let m = result.expect("programming/token.rs: required value was None/Err");
        assert!(m.get("name").is_some());
        assert_eq!(
            m.get_one("name")
                .expect("programming/token.rs: required value was None/Err")
                .text,
            "x"
        );
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

    #[test]
    fn iterative_debug_matches_derived_shape_and_clone() {
        let predicate = TokenPredicate::Any_(vec![
            TokenPredicate::text("a"),
            TokenPredicate::not(TokenPredicate::kind(TokenKind::Keyword)),
        ]);
        assert_eq!(
            format!("{predicate:?}"),
            "Any_([Text(\"a\"), Not(Kind(Keyword))])",
        );

        let pattern = TokenPattern {
            elements: vec![
                PatternElement::Single(TokenPredicate::text("a")),
                PatternElement::Alternative(vec![TokenPattern {
                    elements: vec![PatternElement::Optional(TokenPredicate::kind(
                        TokenKind::Identifier,
                    ))],
                    name: "nested".to_string(),
                }]),
            ],
            name: "root".to_string(),
        };
        let expected = "TokenPattern { elements: [Single(Text(\"a\")), Alternative([TokenPattern { elements: [Optional(Kind(Identifier))], name: \"nested\" }])], name: \"root\" }";
        assert_eq!(format!("{pattern:?}"), expected);
        assert_eq!(format!("{:?}", pattern.clone()), expected);
    }

    #[test]
    fn alternative_restores_entry_state_and_continues_parent() {
        let preferred = TokenPattern::exact_sequence("preferred", &["x", "a"]);
        let fallback = TokenPattern::exact_sequence("fallback", &["x", "b"]);
        let pattern = TokenPattern {
            elements: vec![
                PatternElement::Alternative(vec![preferred, fallback]),
                PatternElement::Single(TokenPredicate::text("z")),
            ],
            name: "parent".to_string(),
        };
        let tokens = ["x", "b", "z"]
            .into_iter()
            .map(|text| Token::simple(TokenKind::Identifier, text))
            .collect::<Vec<_>>();
        let matched = PatternMatcher::new()
            .try_match_at(&pattern, &tokens, 0)
            .expect("fallback must restart at the alternative's entry state");
        assert_eq!(matched.end_index, tokens.len());
        assert_eq!(
            matched
                .tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>(),
            vec!["x", "b", "z"],
        );
    }

    #[test]
    fn first_success_commits_before_parent_continuation() {
        let preferred = TokenPattern::exact_sequence("preferred", &["x"]);
        let fallback = TokenPattern::exact_sequence("fallback", &["x", "b"]);
        let pattern = TokenPattern {
            elements: vec![
                PatternElement::Alternative(vec![preferred, fallback]),
                PatternElement::Single(TokenPredicate::text("z")),
            ],
            name: "parent".to_string(),
        };
        let tokens = ["x", "b", "z"]
            .into_iter()
            .map(|text| Token::simple(TokenKind::Identifier, text))
            .collect::<Vec<_>>();
        assert!(PatternMatcher::new()
            .try_match_at(&pattern, &tokens, 0)
            .is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn ordered_alternative_selects_the_first_matching_branch(
            token_text in "[a-c]",
            preferred_text in "[a-c]",
            fallback_text in "[a-c]",
            suffix_text in "[d-f]",
        ) {
            let preferred = TokenPattern::exact_sequence("preferred", &[&preferred_text]);
            let fallback = TokenPattern::exact_sequence("fallback", &[&fallback_text]);
            let pattern = TokenPattern {
                elements: vec![
                    PatternElement::Alternative(vec![preferred, fallback]),
                    PatternElement::Single(TokenPredicate::text(&suffix_text)),
                ],
                name: "parent".to_string(),
            };
            let tokens = vec![
                Token::simple(TokenKind::Identifier, &token_text),
                Token::simple(TokenKind::Identifier, &suffix_text),
            ];
            let actual = PatternMatcher::new().try_match_at(&pattern, &tokens, 0);
            let expected = token_text == preferred_text || token_text == fallback_text;
            prop_assert_eq!(actual.is_some(), expected);
            if let Some(matched) = actual {
                prop_assert_eq!(matched.end_index, 2);
            }
        }

        #[test]
        fn shallow_pattern_machine_matches_recursive_reference(
            pattern in shallow_pattern(),
            tokens in shallow_tokens(),
            start in 0usize..16,
        ) {
            let expected = recursive_try_match_at(&pattern, &tokens, start);
            let actual = PatternMatcher::new().try_match_at(&pattern, &tokens, start);
            match (actual, expected) {
                (Some(actual), Some(expected)) => {
                    prop_assert_eq!(actual.tokens, expected.tokens);
                    prop_assert_eq!(actual.captures, expected.captures);
                    prop_assert_eq!(actual.start_index, expected.start_index);
                    prop_assert_eq!(actual.end_index, expected.end_index);
                }
                (None, None) => {}
                (actual, expected) => {
                    prop_assert!(false, "iterative={actual:?}, recursive={expected:?}");
                }
            }
        }
    }

    #[test]
    fn deep_pattern_alternative_lifecycle_uses_constant_native_stack() {
        std::thread::Builder::new()
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let mut pattern = TokenPattern::exact_sequence("leaf", &["x"]);
                for _ in 0..DEEP_PATTERN_DEPTH {
                    pattern = TokenPattern {
                        elements: vec![PatternElement::Alternative(vec![pattern])],
                        name: "nested".to_string(),
                    };
                }
                let tokens = vec![Token::simple(TokenKind::Identifier, "x")];
                let matched = PatternMatcher::new()
                    .try_match_at(&pattern, &tokens, 0)
                    .expect("the unique nested alternative must match");
                assert_eq!(matched.end_index, 1);
                let cloned = pattern.clone();
                assert!(format!("{pattern:?}").starts_with("TokenPattern {"));
                drop(cloned);
                drop(pattern);
            })
            .expect("small-stack worker must spawn")
            .join()
            .expect("token-pattern lifecycle must not overflow the native stack");
    }
}
