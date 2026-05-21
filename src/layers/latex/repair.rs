//! Syntax repair strategies for LaTeX documents.
//!
//! Provides repair suggestions for common LaTeX syntax errors.

use super::validator::{IssueKind, ValidationIssue};

/// A repair suggestion for a syntax issue.
#[derive(Debug, Clone)]
pub struct RepairSuggestion {
    /// The kind of repair.
    pub kind: RepairKind,
    /// Position where the repair should be applied.
    pub position: usize,
    /// Token(s) to insert/delete/replace.
    pub tokens: Vec<String>,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
    /// Human-readable description.
    pub description: String,
}

impl RepairSuggestion {
    /// Create an insertion repair.
    pub fn insert(
        position: usize,
        tokens: Vec<String>,
        confidence: f32,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind: RepairKind::Insert,
            position,
            tokens,
            confidence,
            description: description.into(),
        }
    }

    /// Create a deletion repair.
    pub fn delete(
        position: usize,
        count: usize,
        confidence: f32,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind: RepairKind::Delete { count },
            position,
            tokens: Vec::new(),
            confidence,
            description: description.into(),
        }
    }

    /// Create a replacement repair.
    pub fn replace(
        position: usize,
        count: usize,
        tokens: Vec<String>,
        confidence: f32,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind: RepairKind::Replace { count },
            position,
            tokens,
            confidence,
            description: description.into(),
        }
    }
}

/// Kind of repair operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairKind {
    /// Insert tokens at position.
    Insert,
    /// Delete tokens starting at position.
    Delete {
        /// Number of tokens to delete.
        count: usize,
    },
    /// Replace tokens starting at position.
    Replace {
        /// Number of tokens to replace.
        count: usize,
    },
}

/// Strategy for generating repairs.
pub trait RepairStrategy: Send + Sync {
    /// Generate repair suggestions for a validation issue.
    fn suggest(&self, issue: &ValidationIssue, context: &[&str]) -> Vec<RepairSuggestion>;

    /// Name of this strategy for diagnostics.
    fn name(&self) -> &str;
}

/// Repair strategy for brace matching issues.
pub struct BraceRepairStrategy;

impl BraceRepairStrategy {
    /// Create a new brace repair strategy.
    pub fn new() -> Self {
        Self
    }
}

impl Default for BraceRepairStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl RepairStrategy for BraceRepairStrategy {
    fn name(&self) -> &str {
        "brace-repair"
    }

    fn suggest(&self, issue: &ValidationIssue, context: &[&str]) -> Vec<RepairSuggestion> {
        let mut suggestions = Vec::new();
        let pos = issue.position.unwrap_or(0);

        match issue.kind {
            IssueKind::UnmatchedOpenBrace => {
                // Suggest inserting closing brace at the end
                suggestions.push(RepairSuggestion::insert(
                    context.len(),
                    vec!["}".to_string()],
                    0.7,
                    "Insert missing closing brace at end",
                ));

                // Suggest inserting after next logical break
                if let Some(break_pos) = find_logical_break(context, pos) {
                    suggestions.push(RepairSuggestion::insert(
                        break_pos,
                        vec!["}".to_string()],
                        0.8,
                        format!("Insert closing brace at position {}", break_pos),
                    ));
                }
            }
            IssueKind::UnmatchedCloseBrace => {
                // Suggest deleting the unmatched brace
                suggestions.push(RepairSuggestion::delete(
                    pos,
                    1,
                    0.6,
                    "Delete unmatched closing brace",
                ));

                // Suggest inserting opening brace at start
                suggestions.push(RepairSuggestion::insert(
                    0,
                    vec!["{".to_string()],
                    0.5,
                    "Insert matching opening brace at start",
                ));
            }
            IssueKind::UnmatchedOpenBracket => {
                suggestions.push(RepairSuggestion::insert(
                    context.len(),
                    vec!["]".to_string()],
                    0.7,
                    "Insert missing closing bracket at end",
                ));
            }
            IssueKind::UnmatchedCloseBracket => {
                suggestions.push(RepairSuggestion::delete(
                    pos,
                    1,
                    0.6,
                    "Delete unmatched closing bracket",
                ));
            }
            IssueKind::UnmatchedOpenParen => {
                suggestions.push(RepairSuggestion::insert(
                    context.len(),
                    vec![")".to_string()],
                    0.7,
                    "Insert missing closing parenthesis at end",
                ));
            }
            IssueKind::UnmatchedCloseParen => {
                suggestions.push(RepairSuggestion::delete(
                    pos,
                    1,
                    0.6,
                    "Delete unmatched closing parenthesis",
                ));
            }
            _ => {}
        }

        suggestions
    }
}

/// Repair strategy for environment matching issues.
pub struct EnvironmentRepairStrategy;

impl EnvironmentRepairStrategy {
    /// Create a new environment repair strategy.
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvironmentRepairStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl RepairStrategy for EnvironmentRepairStrategy {
    fn name(&self) -> &str {
        "environment-repair"
    }

    fn suggest(&self, issue: &ValidationIssue, context: &[&str]) -> Vec<RepairSuggestion> {
        let mut suggestions = Vec::new();
        let pos = issue.position.unwrap_or(0);

        match issue.kind {
            IssueKind::MissingEnvironmentEnd => {
                // Extract environment name from context
                if let Some(env_name) = extract_begin_env_name(context, pos) {
                    suggestions.push(RepairSuggestion::insert(
                        context.len(),
                        vec![
                            "\\end".to_string(),
                            "{".to_string(),
                            env_name.clone(),
                            "}".to_string(),
                        ],
                        0.9,
                        format!("Insert \\end{{{}}} at end", env_name),
                    ));
                }
            }
            IssueKind::ExtraEnvironmentEnd => {
                suggestions.push(RepairSuggestion::delete(
                    pos,
                    4, // \end { name }
                    0.6,
                    "Delete unmatched \\end",
                ));

                // Suggest inserting matching begin at start
                if let Some(env_name) = extract_end_env_name(context, pos) {
                    suggestions.push(RepairSuggestion::insert(
                        0,
                        vec![
                            "\\begin".to_string(),
                            "{".to_string(),
                            env_name.clone(),
                            "}".to_string(),
                        ],
                        0.5,
                        format!("Insert \\begin{{{}}} at start", env_name),
                    ));
                }
            }
            IssueKind::EnvironmentMismatch => {
                // Parse the mismatched names from the issue message
                if let (Some(begin_name), Some(end_pos)) = parse_mismatch_info(&issue.message) {
                    // Suggest replacing the end environment name with the begin name
                    suggestions.push(RepairSuggestion::replace(
                        end_pos + 2, // Position of the env name in \end { name }
                        1,
                        vec![begin_name.clone()],
                        0.85,
                        format!("Change \\end to match \\begin{{{}}}", begin_name),
                    ));
                }
            }
            _ => {}
        }

        suggestions
    }
}

/// Repair strategy for math delimiter issues.
pub struct MathRepairStrategy;

impl MathRepairStrategy {
    /// Create a new math repair strategy.
    pub fn new() -> Self {
        Self
    }
}

impl Default for MathRepairStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl RepairStrategy for MathRepairStrategy {
    fn name(&self) -> &str {
        "math-repair"
    }

    fn suggest(&self, issue: &ValidationIssue, context: &[&str]) -> Vec<RepairSuggestion> {
        let mut suggestions = Vec::new();
        let pos = issue.position.unwrap_or(0);

        match issue.kind {
            IssueKind::UnmatchedMathDelimiter => {
                // Determine which delimiter is unmatched
                if pos < context.len() {
                    let token = context[pos];
                    match token {
                        "$" => {
                            suggestions.push(RepairSuggestion::insert(
                                context.len(),
                                vec!["$".to_string()],
                                0.8,
                                "Insert closing $",
                            ));
                        }
                        "$$" => {
                            suggestions.push(RepairSuggestion::insert(
                                context.len(),
                                vec!["$$".to_string()],
                                0.8,
                                "Insert closing $$",
                            ));
                        }
                        "\\[" => {
                            suggestions.push(RepairSuggestion::insert(
                                context.len(),
                                vec!["\\]".to_string()],
                                0.9,
                                "Insert closing \\]",
                            ));
                        }
                        "\\(" => {
                            suggestions.push(RepairSuggestion::insert(
                                context.len(),
                                vec!["\\)".to_string()],
                                0.9,
                                "Insert closing \\)",
                            ));
                        }
                        "\\]" | "\\)" => {
                            suggestions.push(RepairSuggestion::delete(
                                pos,
                                1,
                                0.6,
                                format!("Delete unmatched {}", token),
                            ));
                        }
                        _ => {}
                    }
                } else {
                    // Generic suggestion when we can't determine the delimiter
                    suggestions.push(RepairSuggestion::insert(
                        context.len(),
                        vec!["$".to_string()],
                        0.5,
                        "Insert closing math delimiter",
                    ));
                }
            }
            IssueKind::NestedMathMode => {
                // Suggest closing the outer math mode before opening inner
                suggestions.push(RepairSuggestion::insert(
                    pos,
                    vec!["$".to_string()],
                    0.7,
                    "Close outer math mode before opening inner",
                ));

                // Suggest deleting the nested delimiter
                suggestions.push(RepairSuggestion::delete(
                    pos,
                    1,
                    0.6,
                    "Delete nested math delimiter",
                ));
            }
            _ => {}
        }

        suggestions
    }
}

/// Composite repair strategy that combines multiple strategies.
pub struct CompositeRepairStrategy {
    strategies: Vec<Box<dyn RepairStrategy>>,
}

impl CompositeRepairStrategy {
    /// Create a new composite strategy with all default strategies.
    pub fn all() -> Self {
        Self {
            strategies: vec![
                Box::new(BraceRepairStrategy::new()),
                Box::new(EnvironmentRepairStrategy::new()),
                Box::new(MathRepairStrategy::new()),
            ],
        }
    }
}

impl RepairStrategy for CompositeRepairStrategy {
    fn name(&self) -> &str {
        "composite-repair"
    }

    fn suggest(&self, issue: &ValidationIssue, context: &[&str]) -> Vec<RepairSuggestion> {
        let mut all_suggestions = Vec::new();

        for strategy in &self.strategies {
            all_suggestions.extend(strategy.suggest(issue, context));
        }

        // Sort by confidence (highest first)
        all_suggestions.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        all_suggestions
    }
}

// Helper functions

/// Find a logical break point after the given position.
fn find_logical_break(context: &[&str], start: usize) -> Option<usize> {
    // Look for newlines, end of environments, or semicolons
    for (i, token) in context.iter().enumerate().skip(start) {
        if *token == "\n" || *token == "\\end" || *token == ";" || *token == "." {
            return Some(i);
        }
    }
    None
}

/// Extract environment name from \begin at given position.
fn extract_begin_env_name(context: &[&str], pos: usize) -> Option<String> {
    if pos + 3 < context.len()
        && context[pos] == "\\begin"
        && context[pos + 1] == "{"
        && context[pos + 3] == "}"
    {
        Some(context[pos + 2].to_string())
    } else {
        None
    }
}

/// Extract environment name from \end at given position.
fn extract_end_env_name(context: &[&str], pos: usize) -> Option<String> {
    if pos + 3 < context.len()
        && context[pos] == "\\end"
        && context[pos + 1] == "{"
        && context[pos + 3] == "}"
    {
        Some(context[pos + 2].to_string())
    } else {
        None
    }
}

/// Parse mismatch info from error message.
fn parse_mismatch_info(message: &str) -> (Option<String>, Option<usize>) {
    // Try to extract "begin{X}" and end position from message
    // Message format: "Environment mismatch: \begin{X} at N closed by \end{Y} at M"
    let begin_start = message.find("\\begin{");
    let end_at = message.rfind(" at ");

    let begin_name = begin_start.and_then(|start| {
        let name_start = start + 7; // length of "\begin{"
        let name_end = message[name_start..].find('}')?;
        Some(message[name_start..name_start + name_end].to_string())
    });

    let end_pos = end_at.and_then(|at| {
        let pos_str = &message[at + 4..];
        pos_str.parse::<usize>().ok()
    });

    (begin_name, end_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brace_repair_unclosed() {
        let strategy = BraceRepairStrategy::new();
        let issue = ValidationIssue::error(
            IssueKind::UnmatchedOpenBrace,
            Some(0),
            "Unclosed '{' at position 0",
        );
        let context = vec!["{", "content"];

        let suggestions = strategy.suggest(&issue, &context);
        assert!(!suggestions.is_empty());
        assert!(suggestions
            .iter()
            .any(|s| matches!(s.kind, RepairKind::Insert)));
    }

    #[test]
    fn test_brace_repair_extra_close() {
        let strategy = BraceRepairStrategy::new();
        let issue = ValidationIssue::error(
            IssueKind::UnmatchedCloseBrace,
            Some(1),
            "Unmatched closing brace at position 1",
        );
        let context = vec!["content", "}"];

        let suggestions = strategy.suggest(&issue, &context);
        assert!(!suggestions.is_empty());
        // Should suggest delete or insert
        assert!(suggestions
            .iter()
            .any(|s| matches!(s.kind, RepairKind::Delete { .. })
                || matches!(s.kind, RepairKind::Insert)));
    }

    #[test]
    fn test_environment_repair_missing_end() {
        let strategy = EnvironmentRepairStrategy::new();
        let issue = ValidationIssue::error(
            IssueKind::MissingEnvironmentEnd,
            Some(0),
            "Unclosed environment 'equation' starting at position 0",
        );
        let context = vec!["\\begin", "{", "equation", "}", "x"];

        let suggestions = strategy.suggest(&issue, &context);
        assert!(!suggestions.is_empty());
        assert!(suggestions
            .iter()
            .any(|s| s.tokens.contains(&"\\end".to_string())));
    }

    #[test]
    fn test_math_repair_unclosed_dollar() {
        let strategy = MathRepairStrategy::new();
        let issue = ValidationIssue::error(
            IssueKind::UnmatchedMathDelimiter,
            Some(0),
            "Unclosed inline math starting at position 0",
        );
        let context = vec!["$", "x", "+", "y"];

        let suggestions = strategy.suggest(&issue, &context);
        assert!(!suggestions.is_empty());
        assert!(suggestions
            .iter()
            .any(|s| s.tokens.contains(&"$".to_string())));
    }

    #[test]
    fn test_composite_strategy() {
        let strategy = CompositeRepairStrategy::all();
        let issue =
            ValidationIssue::error(IssueKind::UnmatchedOpenBrace, Some(0), "Unclosed brace");
        let context = vec!["{", "x"];

        let suggestions = strategy.suggest(&issue, &context);
        assert!(!suggestions.is_empty());
        // Should be sorted by confidence
        for window in suggestions.windows(2) {
            assert!(window[0].confidence >= window[1].confidence);
        }
    }

    #[test]
    fn test_repair_suggestion_constructors() {
        let insert = RepairSuggestion::insert(5, vec!["}".to_string()], 0.9, "test");
        assert!(matches!(insert.kind, RepairKind::Insert));
        assert_eq!(insert.position, 5);

        let delete = RepairSuggestion::delete(3, 2, 0.7, "test");
        assert!(matches!(delete.kind, RepairKind::Delete { count: 2 }));

        let replace = RepairSuggestion::replace(1, 1, vec!["new".to_string()], 0.8, "test");
        assert!(matches!(replace.kind, RepairKind::Replace { count: 1 }));
    }
}
