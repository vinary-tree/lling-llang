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
use super::token::{PatternMatch, PatternMatcher, Token, TokenPattern, TokenPredicate};
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

impl Clone for RepairAction {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a RepairAction),
            Multiple(usize),
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(action) => match action {
                    RepairAction::NoOp => values.push(RepairAction::NoOp),
                    RepairAction::Insert { position, text } => {
                        values.push(RepairAction::Insert {
                            position: *position,
                            text: text.clone(),
                        });
                    }
                    RepairAction::Delete { range } => {
                        values.push(RepairAction::Delete { range: *range });
                    }
                    RepairAction::Replace { range, replacement } => {
                        values.push(RepairAction::Replace {
                            range: *range,
                            replacement: replacement.clone(),
                        });
                    }
                    RepairAction::Multiple(actions) => {
                        tasks.push(Task::Multiple(actions.len()));
                        tasks.extend(actions.iter().rev().map(Task::Clone));
                    }
                },
                Task::Multiple(count) => {
                    let first = values
                        .len()
                        .checked_sub(count)
                        .expect("all nested actions are cloned before their group");
                    let actions = values.split_off(first);
                    values.push(RepairAction::Multiple(actions));
                }
            }
        }
        values.pop().expect("the root action produces one clone")
    }
}

impl PartialEq for RepairAction {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (RepairAction::NoOp, RepairAction::NoOp) => {}
                (
                    RepairAction::Insert {
                        position: lp,
                        text: lt,
                    },
                    RepairAction::Insert {
                        position: rp,
                        text: rt,
                    },
                ) if lp == rp && lt == rt => {}
                (RepairAction::Delete { range: left }, RepairAction::Delete { range: right })
                    if left == right => {}
                (
                    RepairAction::Replace {
                        range: lr,
                        replacement: lv,
                    },
                    RepairAction::Replace {
                        range: rr,
                        replacement: rv,
                    },
                ) if lr == rr && lv == rv => {}
                (RepairAction::Multiple(left), RepairAction::Multiple(right))
                    if left.len() == right.len() =>
                {
                    pending.extend(left.iter().zip(right).rev());
                }
                _ => return false,
            }
        }
        true
    }
}

impl fmt::Debug for RepairAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Action(&'a RepairAction),
            Multiple(&'a [RepairAction], usize),
        }
        let mut events = vec![Event::Action(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Action(action) => match action {
                    RepairAction::NoOp => write!(f, "NoOp")?,
                    RepairAction::Insert { position, text } => {
                        write!(f, "Insert {{ position: {position:?}, text: {text:?} }}")?;
                    }
                    RepairAction::Delete { range } => {
                        write!(f, "Delete {{ range: {range:?} }}")?;
                    }
                    RepairAction::Replace { range, replacement } => write!(
                        f,
                        "Replace {{ range: {range:?}, replacement: {replacement:?} }}"
                    )?,
                    RepairAction::Multiple(actions) => {
                        write!(f, "Multiple([")?;
                        events.push(Event::Multiple(actions, 0));
                    }
                },
                Event::Multiple(actions, index) => {
                    if index == actions.len() {
                        write!(f, "])")?;
                    } else {
                        if index > 0 {
                            write!(f, ", ")?;
                        }
                        events.push(Event::Multiple(actions, index + 1));
                        events.push(Event::Action(&actions[index]));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for RepairAction {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        if let RepairAction::Multiple(actions) = self {
            pending.append(actions);
        }
        while let Some(mut action) = pending.pop() {
            if let RepairAction::Multiple(actions) = &mut action {
                pending.append(actions);
            }
        }
    }
}

impl RepairAction {
    /// Get the cost of this action based on costs.
    pub fn cost(&self, costs: &SyntaxRepairCosts) -> f64 {
        let mut total = 0.0;
        let mut pending = vec![self];
        while let Some(action) = pending.pop() {
            total += match action {
                RepairAction::NoOp | RepairAction::Multiple(_) => 0.0,
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
            };
            if let RepairAction::Multiple(actions) = action {
                pending.extend(actions.iter().rev());
            }
        }
        total
    }

    /// Apply this action to source text.
    pub fn apply(&self, source: &str) -> String {
        let mut result = source.to_string();
        let mut pending = vec![self];
        while let Some(action) = pending.pop() {
            match action {
                RepairAction::NoOp => {}
                RepairAction::Insert { position, text } => {
                    let offset = position.byte_offset.min(result.len());
                    let mut next = String::with_capacity(result.len() + text.len());
                    next.push_str(&result[..offset]);
                    next.push_str(text);
                    next.push_str(&result[offset..]);
                    result = next;
                }
                RepairAction::Delete { range } => {
                    let mut next = String::with_capacity(result.len());
                    next.push_str(&result[..range.start.byte_offset.min(result.len())]);
                    next.push_str(&result[range.end.byte_offset.min(result.len())..]);
                    result = next;
                }
                RepairAction::Replace { range, replacement } => {
                    let mut next = String::with_capacity(result.len() + replacement.len());
                    next.push_str(&result[..range.start.byte_offset.min(result.len())]);
                    next.push_str(replacement);
                    next.push_str(&result[range.end.byte_offset.min(result.len())..]);
                    result = next;
                }
                RepairAction::Multiple(actions) => {
                    let mut sorted: Vec<_> = actions.iter().collect();
                    sorted
                        .sort_by(|left, right| action_position(right).cmp(&action_position(left)));
                    pending.extend(sorted.into_iter().rev());
                }
            }
        }
        result
    }
}

fn action_position(action: &RepairAction) -> usize {
    let mut maximum = 0usize;
    let mut pending = vec![action];
    while let Some(current) = pending.pop() {
        match current {
            RepairAction::NoOp => {}
            RepairAction::Insert { position, .. } => maximum = maximum.max(position.byte_offset),
            RepairAction::Delete { range } | RepairAction::Replace { range, .. } => {
                maximum = maximum.max(range.start.byte_offset);
            }
            RepairAction::Multiple(actions) => pending.extend(actions.iter()),
        }
    }
    maximum
}

fn is_punctuation(text: &str) -> bool {
    text.chars().all(|c| "{}();,.:[]".contains(c))
}

impl Display for RepairAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Action(&'a RepairAction),
            List(&'a [RepairAction], usize),
        }
        let mut events = vec![Event::Action(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Action(action) => match action {
                    RepairAction::NoOp => write!(f, "no-op")?,
                    RepairAction::Insert { position, text } => {
                        write!(f, "insert '{}' at {}", text, position)?;
                    }
                    RepairAction::Delete { range } => write!(f, "delete {}", range)?,
                    RepairAction::Replace { range, replacement } => {
                        write!(f, "replace {} with '{}'", range, replacement)?;
                    }
                    RepairAction::Multiple(actions) => {
                        write!(f, "[")?;
                        events.push(Event::List(actions, 0));
                    }
                },
                Event::List(actions, index) => {
                    if index == actions.len() {
                        write!(f, "]")?;
                    } else {
                        if index > 0 {
                            write!(f, ", ")?;
                        }
                        events.push(Event::List(actions, index + 1));
                        events.push(Event::Action(&actions[index]));
                    }
                }
            }
        }
        Ok(())
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
        let mut first = &action;
        let position = loop {
            match first {
                RepairAction::Insert { position, .. } => break *position,
                RepairAction::Delete { range } | RepairAction::Replace { range, .. } => {
                    break range.start;
                }
                RepairAction::Multiple(actions) => match actions.first() {
                    Some(action) => first = action,
                    None => break Position::default(),
                },
                RepairAction::NoOp => break Position::default(),
            }
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
            RepairPattern::TokenPattern(pattern) => {
                let mut matcher = PatternMatcher::new();
                matcher.add_pattern(pattern.clone());

                for (_, matched) in matcher.find_all_matches(tokens) {
                    if let Some(action) = self.generate_action_from_match(rule, &matched) {
                        candidates.push(RepairCandidate::new(
                            action,
                            rule.cost,
                            rule.description.clone(),
                        ));
                    }
                }
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

    /// Generate a repair action from a token-pattern match.
    fn generate_action_from_match(
        &self,
        rule: &SyntaxRepairRule,
        matched: &PatternMatch,
    ) -> Option<RepairAction> {
        let range = matched.range()?;

        match &rule.action_template {
            RepairActionTemplate::Insert(text) => Some(RepairAction::Insert {
                position: range.end,
                text: text.clone(),
            }),
            RepairActionTemplate::Delete => Some(RepairAction::Delete { range }),
            RepairActionTemplate::Replace(replacement) => Some(RepairAction::Replace {
                range,
                replacement: replacement.clone(),
            }),
            RepairActionTemplate::ReplaceWithCapture(capture_name, template) => {
                let capture_text = capture_text(matched.get(capture_name)?);
                Some(RepairAction::Replace {
                    range,
                    replacement: render_capture_template(template, capture_name, &capture_text),
                })
            }
            RepairActionTemplate::InsertCapture(capture_name) => {
                let capture_text = capture_text(matched.get(capture_name)?);
                Some(RepairAction::Insert {
                    position: range.end,
                    text: capture_text,
                })
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
        let mut minimum = usize::MAX;
        let mut maximum = 0usize;
        let mut pending = vec![action];
        while let Some(current) = pending.pop() {
            let range = match current {
                RepairAction::NoOp => Some((0, 0)),
                RepairAction::Insert { position, .. } => {
                    Some((position.byte_offset, position.byte_offset))
                }
                RepairAction::Delete { range } | RepairAction::Replace { range, .. } => {
                    Some((range.start.byte_offset, range.end.byte_offset))
                }
                RepairAction::Multiple(actions) if actions.is_empty() => Some((0, 0)),
                RepairAction::Multiple(actions) => {
                    pending.extend(actions.iter());
                    None
                }
            };
            if let Some((start, end)) = range {
                minimum = minimum.min(start);
                maximum = maximum.max(end);
            }
        }
        (if minimum == usize::MAX { 0 } else { minimum }, maximum)
    }
}

fn capture_text(tokens: &[Token]) -> String {
    let Some(first) = tokens.first() else {
        return String::new();
    };

    let mut result = String::new();
    let mut previous_end = first.range.start.byte_offset;

    for token in tokens {
        if !result.is_empty() && token.range.start.byte_offset > previous_end {
            result.push(' ');
        }
        result.push_str(&token.text);
        previous_end = token.range.end.byte_offset;
    }

    result
}

fn render_capture_template(template: &str, capture_name: &str, capture_text: &str) -> String {
    if template.is_empty() {
        return capture_text.to_string();
    }

    let braced = format!("{{{}}}", capture_name);
    let dollar = format!("${}", capture_name);
    template
        .replace("{}", capture_text)
        .replace(&braced, capture_text)
        .replace(&dollar, capture_text)
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

    const DEEP_REPAIR_ACTION_DEPTH: usize = 100_000;
    const SMALL_NATIVE_STACK: usize = 256 * 1024;

    fn nested_repair_action(depth: usize, position: Position) -> RepairAction {
        let mut action = RepairAction::Insert {
            position,
            text: "x".to_owned(),
        };
        for _ in 0..depth {
            action = RepairAction::Multiple(vec![action]);
        }
        action
    }

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
    fn test_find_repairs_token_pattern_replace_with_capture() {
        let pattern = TokenPattern::new("let_binding")
            .then(TokenPredicate::text("let"))
            .capture("name", TokenPredicate::kind(TokenKind::Identifier));
        let rule = SyntaxRepairRule::new(
            RepairPattern::TokenPattern(pattern),
            RepairActionTemplate::ReplaceWithCapture(
                "name".to_string(),
                "const {name}".to_string(),
            ),
            0.2,
            "Promote let binding to const",
        );
        let transducer: SyntaxRepairTransducer<TropicalWeight> =
            SyntaxRepairBuilder::new().add_rule(rule).build();
        let tokens = vec![
            Token::new(
                TokenKind::Keyword,
                "let",
                Range::new(Position::start(), Position::new(0, 3, 3)),
            ),
            Token::new(
                TokenKind::Identifier,
                "answer",
                Range::new(Position::new(0, 4, 4), Position::new(0, 10, 10)),
            ),
        ];

        let repairs = transducer.find_repairs(&tokens);
        assert_eq!(repairs.len(), 1);
        assert_eq!(repairs[0].apply("let answer"), "const answer");
    }

    #[test]
    fn test_find_repairs_token_pattern_insert_capture() {
        let pattern = TokenPattern::new("duplicate_identifier")
            .capture("name", TokenPredicate::kind(TokenKind::Identifier));
        let rule = SyntaxRepairRule::new(
            RepairPattern::TokenPattern(pattern),
            RepairActionTemplate::InsertCapture("name".to_string()),
            0.4,
            "Duplicate identifier",
        );
        let transducer: SyntaxRepairTransducer<TropicalWeight> =
            SyntaxRepairBuilder::new().add_rule(rule).build();
        let tokens = vec![Token::new(
            TokenKind::Identifier,
            "value",
            Range::new(Position::start(), Position::new(0, 5, 5)),
        )];

        let repairs = transducer.find_repairs(&tokens);
        assert_eq!(repairs.len(), 1);
        assert_eq!(repairs[0].apply("value"), "valuevalue");
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

    #[test]
    fn deep_repair_candidate_and_range_use_constant_native_stack() {
        std::thread::Builder::new()
            .name("deep-repair-action".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let position = Position::new(7, 11, 13);
                let action = nested_repair_action(DEEP_REPAIR_ACTION_DEPTH, position);
                let transducer: SyntaxRepairTransducer<TropicalWeight> =
                    SyntaxRepairTransducer::new();
                assert_eq!(
                    transducer.action_range(&action),
                    (position.byte_offset, position.byte_offset),
                );
                let candidate = RepairCandidate::new(action, 0.25, "deep".to_owned());
                assert_eq!(candidate.position, position);
                drop(candidate);
            })
            .expect("the bounded-stack repair worker must spawn")
            .join()
            .expect("repair action traversal and lifecycle must not overflow the native stack");
    }

    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        fn repair_action_strategy() -> BoxedStrategy<RepairAction> {
            let leaf = prop_oneof![
                Just(RepairAction::NoOp),
                (0usize..256).prop_map(|offset| RepairAction::Insert {
                    position: Position::new(0, offset, offset),
                    text: "x".to_owned(),
                }),
                (0usize..256, 0usize..32).prop_map(|(start, width)| {
                    RepairAction::Delete {
                        range: Range::new(
                            Position::new(0, start, start),
                            Position::new(0, start + width, start + width),
                        ),
                    }
                }),
                (0usize..256, 0usize..32).prop_map(|(start, width)| {
                    RepairAction::Replace {
                        range: Range::new(
                            Position::new(0, start, start),
                            Position::new(0, start + width, start + width),
                        ),
                        replacement: "r".to_owned(),
                    }
                }),
            ];
            leaf.prop_recursive(6, 192, 4, |inner| {
                prop::collection::vec(inner, 0..=4).prop_map(RepairAction::Multiple)
            })
            .boxed()
        }

        fn recursive_first_position(action: &RepairAction) -> Position {
            match action {
                RepairAction::Insert { position, .. } => *position,
                RepairAction::Delete { range } | RepairAction::Replace { range, .. } => range.start,
                RepairAction::Multiple(actions) => actions
                    .first()
                    .map(recursive_first_position)
                    .unwrap_or_default(),
                RepairAction::NoOp => Position::default(),
            }
        }

        fn recursive_action_range(action: &RepairAction) -> (usize, usize) {
            match action {
                RepairAction::NoOp => (0, 0),
                RepairAction::Insert { position, .. } => {
                    (position.byte_offset, position.byte_offset)
                }
                RepairAction::Delete { range } | RepairAction::Replace { range, .. } => {
                    (range.start.byte_offset, range.end.byte_offset)
                }
                RepairAction::Multiple(actions) if actions.is_empty() => (0, 0),
                RepairAction::Multiple(actions) => actions
                    .iter()
                    .map(recursive_action_range)
                    .fold((usize::MAX, 0), |(minimum, maximum), (start, end)| {
                        (minimum.min(start), maximum.max(end))
                    }),
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(512))]

            #[test]
            fn repair_candidate_and_range_refine_recursive_oracles(
                action in repair_action_strategy(),
            ) {
                let expected_position = recursive_first_position(&action);
                let expected_range = recursive_action_range(&action);
                let transducer: SyntaxRepairTransducer<TropicalWeight> =
                    SyntaxRepairTransducer::new();
                prop_assert_eq!(transducer.action_range(&action), expected_range);
                let candidate = RepairCandidate::new(action, 0.5, "property".to_owned());
                prop_assert_eq!(candidate.position, expected_position);
            }

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
