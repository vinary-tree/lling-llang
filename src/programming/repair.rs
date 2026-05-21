//! Syntax error repair using WFST techniques.
//!
//! This module provides WFST-based syntax repair that can model and fix common
//! syntax errors in programming languages.

use std::fmt::{self, Display};

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, VectorWfst, WeightedTransition};

#[cfg(test)]
use crate::wfst::Wfst;

#[cfg(test)]
use super::token::TokenKind;
use super::token::{PatternMatcher, Token, TokenPattern, TokenPredicate};
use super::traits::{Position, Range};

/// Costs for syntax repair operations.
#[derive(Debug, Clone)]
pub struct SyntaxRepairCosts {
    /// Cost to insert a token.
    pub insert: f64,
    /// Cost to delete a token.
    pub delete: f64,
    /// Cost to substitute a token.
    pub substitute: f64,
    /// Cost for known typo fixes (lower than generic substitute).
    pub typo_fix: f64,
    /// Cost for missing punctuation (lower than generic insert).
    pub missing_punctuation: f64,
}

impl Default for SyntaxRepairCosts {
    fn default() -> Self {
        Self {
            insert: 1.0,
            delete: 1.0,
            substitute: 1.0,
            typo_fix: 0.2,
            missing_punctuation: 0.3,
        }
    }
}

impl SyntaxRepairCosts {
    /// Create costs optimized for typo correction.
    pub fn typo_focused() -> Self {
        Self {
            insert: 1.5,
            delete: 1.5,
            substitute: 0.8,
            typo_fix: 0.1,
            missing_punctuation: 0.4,
        }
    }

    /// Create costs optimized for punctuation errors.
    pub fn punctuation_focused() -> Self {
        Self {
            insert: 0.5,
            delete: 0.5,
            substitute: 1.0,
            typo_fix: 0.3,
            missing_punctuation: 0.1,
        }
    }
}

/// Action taken to repair syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum RepairAction {
    /// No repair needed.
    NoOp,
    /// Insert a token.
    Insert {
        /// Position at which to insert.
        position: Position,
        /// Text to insert at the position.
        text: String,
    },
    /// Delete a range of text.
    Delete {
        /// Range of text to delete.
        range: Range,
    },
    /// Replace text in a range.
    Replace {
        /// Range of text to replace.
        range: Range,
        /// Replacement text to substitute into the range.
        replacement: String,
    },
    /// Multiple repairs.
    Multiple(Vec<RepairAction>),
}

impl RepairAction {
    /// Get the cost of this action based on costs.
    pub fn cost(&self, costs: &SyntaxRepairCosts) -> f64 {
        match self {
            RepairAction::NoOp => 0.0,
            RepairAction::Insert { text, .. } => {
                if is_punctuation(text) {
                    costs.missing_punctuation
                } else {
                    costs.insert
                }
            }
            RepairAction::Delete { .. } => costs.delete,
            RepairAction::Replace { replacement, .. } => {
                if replacement.len() <= 2 {
                    costs.typo_fix
                } else {
                    costs.substitute
                }
            }
            RepairAction::Multiple(actions) => actions.iter().map(|a| a.cost(costs)).sum(),
        }
    }

    /// Apply this action to source text.
    pub fn apply(&self, source: &str) -> String {
        match self {
            RepairAction::NoOp => source.to_string(),
            RepairAction::Insert { position, text } => {
                let mut result = String::with_capacity(source.len() + text.len());
                result.push_str(&source[..position.byte_offset.min(source.len())]);
                result.push_str(text);
                result.push_str(&source[position.byte_offset.min(source.len())..]);
                result
            }
            RepairAction::Delete { range } => {
                let mut result = String::with_capacity(source.len());
                result.push_str(&source[..range.start.byte_offset.min(source.len())]);
                result.push_str(&source[range.end.byte_offset.min(source.len())..]);
                result
            }
            RepairAction::Replace { range, replacement } => {
                let mut result = String::with_capacity(source.len() + replacement.len());
                result.push_str(&source[..range.start.byte_offset.min(source.len())]);
                result.push_str(replacement);
                result.push_str(&source[range.end.byte_offset.min(source.len())..]);
                result
            }
            RepairAction::Multiple(actions) => {
                // Apply actions in reverse order (later positions first)
                // to maintain correct offsets
                let mut sorted_actions: Vec<_> = actions.iter().collect();
                sorted_actions.sort_by(|a, b| {
                    let pos_a = action_position(a);
                    let pos_b = action_position(b);
                    pos_b.cmp(&pos_a)
                });

                let mut result = source.to_string();
                for action in sorted_actions {
                    result = action.apply(&result);
                }
                result
            }
        }
    }
}

fn action_position(action: &RepairAction) -> usize {
    match action {
        RepairAction::NoOp => 0,
        RepairAction::Insert { position, .. } => position.byte_offset,
        RepairAction::Delete { range } => range.start.byte_offset,
        RepairAction::Replace { range, .. } => range.start.byte_offset,
        RepairAction::Multiple(actions) => actions.iter().map(action_position).max().unwrap_or(0),
    }
}

fn is_punctuation(text: &str) -> bool {
    text.chars().all(|c| "{}();,.:[]".contains(c))
}

impl Display for RepairAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepairAction::NoOp => write!(f, "no-op"),
            RepairAction::Insert { position, text } => {
                write!(f, "insert '{}' at {}", text, position)
            }
            RepairAction::Delete { range } => {
                write!(f, "delete {}", range)
            }
            RepairAction::Replace { range, replacement } => {
                write!(f, "replace {} with '{}'", range, replacement)
            }
            RepairAction::Multiple(actions) => {
                write!(f, "[")?;
                for (i, a) in actions.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, "]")
            }
        }
    }
}

/// A syntax repair rule.
#[derive(Debug, Clone)]
pub struct SyntaxRepairRule {
    /// Pattern to match (token-based or text-based).
    pub pattern: RepairPattern,
    /// Repair action to take.
    pub action_template: RepairActionTemplate,
    /// Base cost for this repair.
    pub cost: f64,
    /// Human-readable description.
    pub description: String,
    /// Language(s) this rule applies to.
    pub languages: Vec<String>,
}

/// Pattern for matching repair locations.
#[derive(Debug, Clone)]
pub enum RepairPattern {
    /// Match a token pattern.
    TokenPattern(TokenPattern),
    /// Match exact text.
    ExactText(String),
    /// Match text case-insensitively.
    TextCaseInsensitive(String),
    /// Match after a specific token.
    AfterToken(TokenPredicate),
    /// Match before a specific token.
    BeforeToken(TokenPredicate),
    /// Match in error node.
    InErrorNode,
    /// Match missing node.
    MissingNode(String),
}

/// Template for generating repair actions.
#[derive(Debug, Clone)]
pub enum RepairActionTemplate {
    /// Insert fixed text.
    Insert(String),
    /// Delete matched content.
    Delete,
    /// Replace with fixed text.
    Replace(String),
    /// Replace using captures from pattern.
    ReplaceWithCapture(String, String), // (capture name, template)
    /// Insert text from captures.
    InsertCapture(String),
}

impl SyntaxRepairRule {
    /// Create a new repair rule.
    pub fn new(
        pattern: RepairPattern,
        action_template: RepairActionTemplate,
        cost: f64,
        description: impl Into<String>,
    ) -> Self {
        Self {
            pattern,
            action_template,
            cost,
            description: description.into(),
            languages: Vec::new(),
        }
    }

    /// Add language constraints.
    pub fn for_languages(mut self, languages: &[&str]) -> Self {
        self.languages = languages.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Create a rule for missing semicolon after closing brace.
    pub fn missing_semicolon_after_brace(cost: f64) -> Self {
        Self::new(
            RepairPattern::AfterToken(TokenPredicate::text("}")),
            RepairActionTemplate::Insert(";".to_string()),
            cost,
            "Insert missing semicolon after closing brace",
        )
        .for_languages(&["javascript", "typescript", "java", "c", "cpp", "rust"])
    }

    /// Create a rule for typo substitution.
    pub fn typo_substitute(from: &str, to: &str, cost: f64) -> Self {
        Self::new(
            RepairPattern::ExactText(from.to_string()),
            RepairActionTemplate::Replace(to.to_string()),
            cost,
            format!("Fix typo: '{}' -> '{}'", from, to),
        )
    }

    /// Create a rule for missing opening brace.
    pub fn missing_opening_brace_after_paren(cost: f64) -> Self {
        Self::new(
            RepairPattern::AfterToken(TokenPredicate::text(")")),
            RepairActionTemplate::Insert(" {".to_string()),
            cost,
            "Insert missing opening brace after closing parenthesis",
        )
        .for_languages(&["javascript", "typescript", "java", "c", "cpp"])
    }

    /// Create a rule for missing closing brace.
    pub fn missing_closing_brace(cost: f64) -> Self {
        Self::new(
            RepairPattern::MissingNode("block".to_string()),
            RepairActionTemplate::Insert("}".to_string()),
            cost,
            "Insert missing closing brace",
        )
    }

    /// Check if this rule applies to a language.
    pub fn applies_to(&self, language: &str) -> bool {
        self.languages.is_empty() || self.languages.iter().any(|l| l == language)
    }
}

/// Common typo fixes for programming languages.
pub fn common_keyword_typos() -> Vec<SyntaxRepairRule> {
    vec![
        // JavaScript/TypeScript
        SyntaxRepairRule::typo_substitute("funciton", "function", 0.1),
        SyntaxRepairRule::typo_substitute("funtion", "function", 0.1),
        SyntaxRepairRule::typo_substitute("fucntion", "function", 0.1),
        SyntaxRepairRule::typo_substitute("functoin", "function", 0.1),
        SyntaxRepairRule::typo_substitute("retrun", "return", 0.1),
        SyntaxRepairRule::typo_substitute("reutrn", "return", 0.1),
        SyntaxRepairRule::typo_substitute("cosnt", "const", 0.1),
        SyntaxRepairRule::typo_substitute("conts", "const", 0.1),
        SyntaxRepairRule::typo_substitute("improt", "import", 0.1),
        SyntaxRepairRule::typo_substitute("exoprt", "export", 0.1),
        // Python
        SyntaxRepairRule::typo_substitute("pritn", "print", 0.1),
        SyntaxRepairRule::typo_substitute("prnit", "print", 0.1),
        SyntaxRepairRule::typo_substitute("defien", "define", 0.1),
        SyntaxRepairRule::typo_substitute("calss", "class", 0.1),
        // Rust
        SyntaxRepairRule::typo_substitute("mactch", "match", 0.1),
        SyntaxRepairRule::typo_substitute("strcut", "struct", 0.1),
        SyntaxRepairRule::typo_substitute("implm", "impl", 0.1),
        // General
        SyntaxRepairRule::typo_substitute("flase", "false", 0.1),
        SyntaxRepairRule::typo_substitute("ture", "true", 0.1),
        SyntaxRepairRule::typo_substitute("nul", "null", 0.1),
        SyntaxRepairRule::typo_substitute("nill", "nil", 0.1),
    ]
}

/// Common punctuation repair rules.
pub fn common_punctuation_repairs() -> Vec<SyntaxRepairRule> {
    vec![
        SyntaxRepairRule::missing_semicolon_after_brace(0.3),
        SyntaxRepairRule::missing_opening_brace_after_paren(0.5),
        SyntaxRepairRule::missing_closing_brace(0.5),
    ]
}

/// A repair candidate.
#[derive(Debug, Clone)]
pub struct RepairCandidate {
    /// The repair action to apply.
    pub action: RepairAction,
    /// Cost/weight of this repair.
    pub cost: f64,
    /// Rule that generated this candidate.
    pub rule_description: String,
    /// Position in source where repair applies.
    pub position: Position,
}

impl RepairCandidate {
    /// Create a new repair candidate.
    pub fn new(action: RepairAction, cost: f64, rule_description: String) -> Self {
        let position = match &action {
            RepairAction::Insert { position, .. } => *position,
            RepairAction::Delete { range } => range.start,
            RepairAction::Replace { range, .. } => range.start,
            RepairAction::Multiple(actions) => actions
                .first()
                .map(|a| RepairCandidate::new(a.clone(), 0.0, String::new()).position)
                .unwrap_or_default(),
            RepairAction::NoOp => Position::default(),
        };

        Self {
            action,
            cost,
            rule_description,
            position,
        }
    }

    /// Apply this repair candidate to source.
    pub fn apply(&self, source: &str) -> String {
        self.action.apply(source)
    }
}

/// Builder for syntax repair transducer.
#[derive(Debug, Clone)]
pub struct SyntaxRepairBuilder {
    rules: Vec<SyntaxRepairRule>,
    costs: SyntaxRepairCosts,
    language: Option<String>,
}

impl SyntaxRepairBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            costs: SyntaxRepairCosts::default(),
            language: None,
        }
    }

    /// Set the target language.
    pub fn language(mut self, language: &str) -> Self {
        self.language = Some(language.to_string());
        self
    }

    /// Set repair costs.
    pub fn costs(mut self, costs: SyntaxRepairCosts) -> Self {
        self.costs = costs;
        self
    }

    /// Add a repair rule.
    pub fn add_rule(mut self, rule: SyntaxRepairRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add multiple rules.
    pub fn add_rules(mut self, rules: Vec<SyntaxRepairRule>) -> Self {
        self.rules.extend(rules);
        self
    }

    /// Add common typo fixes.
    pub fn with_common_typos(self) -> Self {
        self.add_rules(common_keyword_typos())
    }

    /// Add common punctuation repairs.
    pub fn with_punctuation_repairs(self) -> Self {
        self.add_rules(common_punctuation_repairs())
    }

    /// Build the repair transducer.
    pub fn build<W: Semiring + Clone>(self) -> SyntaxRepairTransducer<W> {
        SyntaxRepairTransducer {
            rules: self.rules,
            costs: self.costs,
            language: self.language,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl Default for SyntaxRepairBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// WFST-based syntax repair transducer.
///
/// This transducer generates repair candidates for syntax errors by applying
/// weighted rules to token streams or syntax trees.
#[derive(Debug, Clone)]
pub struct SyntaxRepairTransducer<W: Semiring> {
    rules: Vec<SyntaxRepairRule>,
    costs: SyntaxRepairCosts,
    language: Option<String>,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring + Clone> SyntaxRepairTransducer<W> {
    /// Create a new repair transducer with default settings.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            costs: SyntaxRepairCosts::default(),
            language: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the number of rules.
    pub fn num_rules(&self) -> usize {
        self.rules.len()
    }

    /// Get the repair costs.
    pub fn costs(&self) -> &SyntaxRepairCosts {
        &self.costs
    }

    /// Find repair candidates for a token stream.
    pub fn find_repairs(&self, tokens: &[Token]) -> Vec<RepairCandidate> {
        let mut candidates = Vec::new();

        for rule in &self.rules {
            if let Some(ref lang) = self.language {
                if !rule.applies_to(lang) {
                    continue;
                }
            }

            self.apply_rule(rule, tokens, &mut candidates);
        }

        // Sort by cost
        candidates.sort_by(|a, b| {
            a.cost
                .partial_cmp(&b.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Apply a single rule to find candidates.
    fn apply_rule(
        &self,
        rule: &SyntaxRepairRule,
        tokens: &[Token],
        candidates: &mut Vec<RepairCandidate>,
    ) {
        match &rule.pattern {
            RepairPattern::ExactText(text) => {
                for token in tokens.iter() {
                    if token.text == *text {
                        if let Some(action) = self.generate_action(rule, &token.range) {
                            candidates.push(RepairCandidate::new(
                                action,
                                rule.cost,
                                rule.description.clone(),
                            ));
                        }
                    }
                }
            }
            RepairPattern::TextCaseInsensitive(text) => {
                for token in tokens.iter() {
                    if token.text.eq_ignore_ascii_case(text) {
                        if let Some(action) = self.generate_action(rule, &token.range) {
                            candidates.push(RepairCandidate::new(
                                action,
                                rule.cost,
                                rule.description.clone(),
                            ));
                        }
                    }
                }
            }
            RepairPattern::AfterToken(pred) => {
                for token in tokens.iter() {
                    if pred.matches(token) {
                        let pos = token.range.end;
                        if let Some(action) = self.generate_action_at_position(rule, pos) {
                            candidates.push(RepairCandidate::new(
                                action,
                                rule.cost,
                                rule.description.clone(),
                            ));
                        }
                    }
                }
            }
            RepairPattern::BeforeToken(pred) => {
                for token in tokens.iter() {
                    if pred.matches(token) {
                        let pos = token.range.start;
                        if let Some(action) = self.generate_action_at_position(rule, pos) {
                            candidates.push(RepairCandidate::new(
                                action,
                                rule.cost,
                                rule.description.clone(),
                            ));
                        }
                    }
                }
            }
            RepairPattern::TokenPattern(_pattern) => {
                let _matcher = PatternMatcher::new();
                // Add pattern and find matches
                // This is simplified - full implementation would use the matcher
            }
            RepairPattern::InErrorNode => {
                // Would need syntax tree access
            }
            RepairPattern::MissingNode(_node_kind) => {
                // Would need syntax tree access
            }
        }
    }

    /// Generate a repair action from a rule.
    fn generate_action(&self, rule: &SyntaxRepairRule, range: &Range) -> Option<RepairAction> {
        match &rule.action_template {
            RepairActionTemplate::Insert(text) => Some(RepairAction::Insert {
                position: range.end,
                text: text.clone(),
            }),
            RepairActionTemplate::Delete => Some(RepairAction::Delete { range: *range }),
            RepairActionTemplate::Replace(replacement) => Some(RepairAction::Replace {
                range: *range,
                replacement: replacement.clone(),
            }),
            RepairActionTemplate::ReplaceWithCapture(_, _) => {
                // Would need capture data from pattern match
                None
            }
            RepairActionTemplate::InsertCapture(_) => {
                // Would need capture data from pattern match
                None
            }
        }
    }

    /// Generate a repair action at a specific position.
    fn generate_action_at_position(
        &self,
        rule: &SyntaxRepairRule,
        pos: Position,
    ) -> Option<RepairAction> {
        match &rule.action_template {
            RepairActionTemplate::Insert(text) => Some(RepairAction::Insert {
                position: pos,
                text: text.clone(),
            }),
            _ => None,
        }
    }

    /// Build a WFST for token-level repair.
    ///
    /// The transducer accepts token sequences and outputs repaired sequences
    /// with appropriate weights for edit operations.
    pub fn build_token_wfst(&self, alphabet: &[String]) -> VectorWfst<String, W>
    where
        W: Clone,
    {
        let mut fst = VectorWfst::new();

        // Single state that loops on all tokens
        let s0 = fst.add_state();
        fst.set_start(s0);
        fst.set_final(s0, W::one());

        // Identity transitions (copy tokens unchanged)
        for token in alphabet {
            fst.add_transition(WeightedTransition::new(
                s0,
                Some(token.clone()),
                Some(token.clone()),
                s0,
                W::one(),
            ));
        }

        // Add repair transitions based on rules
        for rule in &self.rules {
            match &rule.pattern {
                RepairPattern::ExactText(from) => {
                    if let RepairActionTemplate::Replace(to) = &rule.action_template {
                        // Weight based on cost (convert to semiring)
                        // For tropical semiring, this would be the cost directly
                        fst.add_transition(WeightedTransition::new(
                            s0,
                            Some(from.clone()),
                            Some(to.clone()),
                            s0,
                            W::one(), // Would convert rule.cost to weight
                        ));
                    }
                }
                _ => {
                    // Other patterns require more complex FST construction
                }
            }
        }

        fst
    }

    /// Repair source text, returning the repaired text and applied repairs.
    pub fn repair(&self, source: &str, tokens: &[Token]) -> (String, Vec<RepairCandidate>) {
        let candidates = self.find_repairs(tokens);

        if candidates.is_empty() {
            return (source.to_string(), vec![]);
        }

        // Apply the best (lowest cost) non-overlapping repairs
        let selected = self.select_non_overlapping(&candidates);
        let mut repaired = source.to_string();

        // Sort by position descending to maintain correct offsets
        let mut sorted: Vec<_> = selected.iter().collect();
        sorted.sort_by(|a, b| b.position.byte_offset.cmp(&a.position.byte_offset));

        for candidate in &sorted {
            repaired = candidate.apply(&repaired);
        }

        (repaired, selected)
    }

    /// Select non-overlapping repairs.
    fn select_non_overlapping(&self, candidates: &[RepairCandidate]) -> Vec<RepairCandidate> {
        if candidates.is_empty() {
            return vec![];
        }

        let mut selected = Vec::new();
        let mut used_positions: Vec<(usize, usize)> = Vec::new();

        for candidate in candidates {
            let (start, end) = self.action_range(&candidate.action);

            // Check if this overlaps with any already selected
            let overlaps = used_positions.iter().any(|(s, e)| start < *e && end > *s);

            if !overlaps {
                used_positions.push((start, end));
                selected.push(candidate.clone());
            }
        }

        selected
    }

    /// Get the byte range affected by an action.
    fn action_range(&self, action: &RepairAction) -> (usize, usize) {
        match action {
            RepairAction::NoOp => (0, 0),
            RepairAction::Insert { position, .. } => (position.byte_offset, position.byte_offset),
            RepairAction::Delete { range } => (range.start.byte_offset, range.end.byte_offset),
            RepairAction::Replace { range, .. } => (range.start.byte_offset, range.end.byte_offset),
            RepairAction::Multiple(actions) => {
                let starts: Vec<_> = actions.iter().map(|a| self.action_range(a).0).collect();
                let ends: Vec<_> = actions.iter().map(|a| self.action_range(a).1).collect();
                (
                    starts.into_iter().min().unwrap_or(0),
                    ends.into_iter().max().unwrap_or(0),
                )
            }
        }
    }
}

impl<W: Semiring + Clone> Default for SyntaxRepairTransducer<W> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_repair_costs_default() {
        let costs = SyntaxRepairCosts::default();
        assert!((costs.insert - 1.0).abs() < f64::EPSILON);
        assert!((costs.delete - 1.0).abs() < f64::EPSILON);
        assert!((costs.substitute - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_repair_action_insert() {
        let action = RepairAction::Insert {
            position: Position::new(0, 5, 5),
            text: ";".to_string(),
        };

        let result = action.apply("hello world");
        assert_eq!(result, "hello; world");
    }

    #[test]
    fn test_repair_action_delete() {
        let action = RepairAction::Delete {
            range: Range::new(Position::new(0, 0, 0), Position::new(0, 5, 5)),
        };

        let result = action.apply("hello world");
        assert_eq!(result, " world");
    }

    #[test]
    fn test_repair_action_replace() {
        let action = RepairAction::Replace {
            range: Range::new(Position::new(0, 0, 0), Position::new(0, 5, 5)),
            replacement: "goodbye".to_string(),
        };

        let result = action.apply("hello world");
        assert_eq!(result, "goodbye world");
    }

    #[test]
    fn test_syntax_repair_rule_typo() {
        let rule = SyntaxRepairRule::typo_substitute("funciton", "function", 0.1);
        assert!(rule.description.contains("funciton"));
        assert!((rule.cost - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_syntax_repair_rule_language_filter() {
        let rule = SyntaxRepairRule::missing_semicolon_after_brace(0.3);

        assert!(rule.applies_to("javascript"));
        assert!(rule.applies_to("rust"));
        assert!(!rule.applies_to("python"));
    }

    #[test]
    fn test_common_keyword_typos() {
        let typos = common_keyword_typos();
        assert!(!typos.is_empty());

        // Check that "funciton" is in the list
        let has_function_typo = typos.iter().any(|r| {
            if let RepairPattern::ExactText(text) = &r.pattern {
                text == "funciton"
            } else {
                false
            }
        });
        assert!(has_function_typo);
    }

    #[test]
    fn test_repair_candidate() {
        let action = RepairAction::Replace {
            range: Range::new(Position::new(0, 0, 0), Position::new(0, 8, 8)),
            replacement: "function".to_string(),
        };

        let candidate = RepairCandidate::new(action, 0.1, "Fix typo".to_string());
        assert!((candidate.cost - 0.1).abs() < f64::EPSILON);
        assert_eq!(candidate.position.byte_offset, 0);
    }

    #[test]
    fn test_syntax_repair_builder() {
        let transducer: SyntaxRepairTransducer<TropicalWeight> = SyntaxRepairBuilder::new()
            .language("javascript")
            .with_common_typos()
            .with_punctuation_repairs()
            .build();

        assert!(transducer.num_rules() > 0);
    }

    #[test]
    fn test_find_repairs_typo() {
        let transducer: SyntaxRepairTransducer<TropicalWeight> = SyntaxRepairBuilder::new()
            .add_rule(SyntaxRepairRule::typo_substitute(
                "funciton", "function", 0.1,
            ))
            .build();

        let tokens = vec![
            Token::new(
                TokenKind::Keyword,
                "funciton",
                Range::new(Position::start(), Position::new(0, 8, 8)),
            ),
            Token::new(
                TokenKind::Identifier,
                "foo",
                Range::new(Position::new(0, 9, 9), Position::new(0, 12, 12)),
            ),
        ];

        let repairs = transducer.find_repairs(&tokens);
        assert!(!repairs.is_empty());
        assert!((repairs[0].cost - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_repair_source() {
        let transducer: SyntaxRepairTransducer<TropicalWeight> = SyntaxRepairBuilder::new()
            .add_rule(SyntaxRepairRule::typo_substitute(
                "funciton", "function", 0.1,
            ))
            .build();

        let source = "funciton foo() {}";
        let tokens = vec![
            Token::new(
                TokenKind::Keyword,
                "funciton",
                Range::new(Position::start(), Position::new(0, 8, 8)),
            ),
            Token::new(
                TokenKind::Identifier,
                "foo",
                Range::new(Position::new(0, 9, 9), Position::new(0, 12, 12)),
            ),
            Token::new(
                TokenKind::Punctuation,
                "(",
                Range::new(Position::new(0, 12, 12), Position::new(0, 13, 13)),
            ),
            Token::new(
                TokenKind::Punctuation,
                ")",
                Range::new(Position::new(0, 13, 13), Position::new(0, 14, 14)),
            ),
            Token::new(
                TokenKind::Punctuation,
                "{",
                Range::new(Position::new(0, 15, 15), Position::new(0, 16, 16)),
            ),
            Token::new(
                TokenKind::Punctuation,
                "}",
                Range::new(Position::new(0, 16, 16), Position::new(0, 17, 17)),
            ),
        ];

        let (repaired, repairs) = transducer.repair(source, &tokens);
        assert_eq!(repaired, "function foo() {}");
        assert_eq!(repairs.len(), 1);
    }

    #[test]
    fn test_build_token_wfst() {
        let transducer: SyntaxRepairTransducer<TropicalWeight> = SyntaxRepairBuilder::new()
            .add_rule(SyntaxRepairRule::typo_substitute("if", "IF", 0.1))
            .build();

        let alphabet = vec!["if".to_string(), "IF".to_string(), "then".to_string()];
        let fst = transducer.build_token_wfst(&alphabet);

        assert!(fst.num_states() > 0);
        // Has identity transitions plus repair transition
        assert!(fst.total_transitions() >= alphabet.len());
    }

    #[test]
    fn test_non_overlapping_selection() {
        let transducer: SyntaxRepairTransducer<TropicalWeight> = SyntaxRepairBuilder::new().build();

        let candidates = vec![
            RepairCandidate::new(
                RepairAction::Replace {
                    range: Range::new(Position::new(0, 0, 0), Position::new(0, 5, 5)),
                    replacement: "hello".to_string(),
                },
                0.1,
                "repair 1".to_string(),
            ),
            RepairCandidate::new(
                RepairAction::Replace {
                    range: Range::new(Position::new(0, 3, 3), Position::new(0, 8, 8)),
                    replacement: "world".to_string(),
                },
                0.2,
                "repair 2".to_string(),
            ),
            RepairCandidate::new(
                RepairAction::Replace {
                    range: Range::new(Position::new(0, 10, 10), Position::new(0, 15, 15)),
                    replacement: "test".to_string(),
                },
                0.15,
                "repair 3".to_string(),
            ),
        ];

        let selected = transducer.select_non_overlapping(&candidates);

        // Should select repair 1 (lowest cost) and repair 3 (non-overlapping)
        // Repair 2 overlaps with repair 1
        assert_eq!(selected.len(), 2);
    }

    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(48))]

            /// NoOp.apply(s) == s for any source.
            #[test]
            fn noop_preserves_source(s in ".{0,100}") {
                let action = RepairAction::NoOp;
                prop_assert_eq!(action.apply(&s), s);
            }

            /// Inserting then deleting the inserted span returns the original.
            #[test]
            fn insert_then_delete_roundtrips(
                prefix in "[a-z]{0,20}",
                insert in "[A-Z]{1,10}",
                suffix in "[a-z]{0,20}",
            ) {
                let source: String = format!("{}{}", prefix, suffix);
                let insert_pos = Position::new(0, prefix.len(), prefix.len());
                let with_insert = RepairAction::Insert {
                    position: insert_pos,
                    text: insert.clone(),
                }
                .apply(&source);
                prop_assert_eq!(with_insert.len(), source.len() + insert.len());

                let delete = RepairAction::Delete {
                    range: Range::new(
                        Position::new(0, prefix.len(), prefix.len()),
                        Position::new(
                            0,
                            prefix.len() + insert.len(),
                            prefix.len() + insert.len(),
                        ),
                    ),
                };
                prop_assert_eq!(delete.apply(&with_insert), source);
            }

            /// Cost of NoOp is always zero regardless of the configured costs.
            #[test]
            fn noop_cost_is_zero(
                insert in 0.0f64..10.0,
                delete in 0.0f64..10.0,
                substitute in 0.0f64..10.0,
            ) {
                let costs = SyntaxRepairCosts {
                    insert,
                    delete,
                    substitute,
                    typo_fix: substitute,
                    missing_punctuation: insert,
                };
                prop_assert!(RepairAction::NoOp.cost(&costs).abs() < 1e-12);
            }
        }
    }
}
