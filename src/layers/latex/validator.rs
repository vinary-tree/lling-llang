//! Structural validation for LaTeX documents.
//!
//! Provides validation beyond CFG parsing: brace matching, environment
//! pairing, math delimiter balance, and other structural constraints.

use std::collections::VecDeque;

/// Result of validating a LaTeX token sequence.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether the sequence is valid.
    pub is_valid: bool,
    /// List of issues found.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Create a valid result with no issues.
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            issues: Vec::new(),
        }
    }

    /// Create an invalid result with issues.
    pub fn invalid(issues: Vec<ValidationIssue>) -> Self {
        Self {
            is_valid: false,
            issues,
        }
    }

    /// Add an issue to the result.
    pub fn add_issue(&mut self, issue: ValidationIssue) {
        if issue.severity == IssueSeverity::Error {
            self.is_valid = false;
        }
        self.issues.push(issue);
    }

    /// Check if there are any errors (not just warnings).
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error)
    }

    /// Get only the error issues.
    pub fn errors(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
    }

    /// Get only the warning issues.
    pub fn warnings(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
    }
}

/// A validation issue found in the document.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Severity of the issue.
    pub severity: IssueSeverity,
    /// Type of issue.
    pub kind: IssueKind,
    /// Position in the token sequence (if applicable).
    pub position: Option<usize>,
    /// Human-readable message.
    pub message: String,
}

impl ValidationIssue {
    /// Create an error issue.
    pub fn error(kind: IssueKind, position: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            kind,
            position,
            message: message.into(),
        }
    }

    /// Create a warning issue.
    pub fn warning(kind: IssueKind, position: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            kind,
            position,
            message: message.into(),
        }
    }
}

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// An error that makes the document invalid.
    Error,
    /// A warning that may indicate a problem.
    Warning,
}

/// Type of validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// Unmatched opening brace.
    UnmatchedOpenBrace,
    /// Unmatched closing brace.
    UnmatchedCloseBrace,
    /// Unmatched opening bracket.
    UnmatchedOpenBracket,
    /// Unmatched closing bracket.
    UnmatchedCloseBracket,
    /// Unmatched opening parenthesis.
    UnmatchedOpenParen,
    /// Unmatched closing parenthesis.
    UnmatchedCloseParen,
    /// Mismatched environment begin/end.
    EnvironmentMismatch,
    /// Missing environment end.
    MissingEnvironmentEnd,
    /// Extra environment end.
    ExtraEnvironmentEnd,
    /// Unmatched math delimiter.
    UnmatchedMathDelimiter,
    /// Nested math mode (e.g., $ inside $).
    NestedMathMode,
    /// Invalid command argument count.
    InvalidArgumentCount,
    /// Unknown environment.
    UnknownEnvironment,
    /// Empty required argument.
    EmptyRequiredArgument,
}

/// Validator for LaTeX structural constraints.
pub struct LatexValidator {
    /// Whether to validate environment names.
    validate_environments: bool,
    /// Whether to validate command argument counts.
    validate_arguments: bool,
    /// Whether to allow nested math modes.
    allow_nested_math: bool,
    /// Known environment names.
    known_environments: Vec<String>,
}

impl Default for LatexValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl LatexValidator {
    /// Create a new validator with default settings.
    pub fn new() -> Self {
        Self {
            validate_environments: true,
            validate_arguments: true,
            allow_nested_math: false,
            known_environments: default_environments(),
        }
    }

    /// Configure whether to validate environment names.
    pub fn with_environment_validation(mut self, validate: bool) -> Self {
        self.validate_environments = validate;
        self
    }

    /// Configure whether to validate command arguments.
    pub fn with_argument_validation(mut self, validate: bool) -> Self {
        self.validate_arguments = validate;
        self
    }

    /// Configure whether to allow nested math modes.
    pub fn with_nested_math(mut self, allow: bool) -> Self {
        self.allow_nested_math = allow;
        self
    }

    /// Add a known environment name.
    pub fn add_environment(mut self, name: impl Into<String>) -> Self {
        self.known_environments.push(name.into());
        self
    }

    /// Validate a sequence of LaTeX tokens represented as strings.
    pub fn validate(&self, tokens: &[&str]) -> ValidationResult {
        let mut result = ValidationResult::valid();

        // Check brace balance
        self.validate_braces(tokens, &mut result);

        // Check environment matching
        self.validate_environments(tokens, &mut result);

        // Check math delimiter matching
        self.validate_math_delimiters(tokens, &mut result);

        result
    }

    /// Validate brace, bracket, and parenthesis matching.
    fn validate_braces(&self, tokens: &[&str], result: &mut ValidationResult) {
        let mut stack: Vec<(char, usize)> = Vec::new();

        for (pos, token) in tokens.iter().enumerate() {
            match *token {
                "{" => stack.push(('{', pos)),
                "}" => {
                    if let Some((open, _)) = stack.pop() {
                        if open != '{' {
                            result.add_issue(ValidationIssue::error(
                                IssueKind::UnmatchedCloseBrace,
                                Some(pos),
                                format!(
                                    "Closing brace at position {} doesn't match opening '{}'",
                                    pos, open
                                ),
                            ));
                        }
                    } else {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::UnmatchedCloseBrace,
                            Some(pos),
                            format!("Unmatched closing brace at position {}", pos),
                        ));
                    }
                }
                "[" => stack.push(('[', pos)),
                "]" => {
                    if let Some((open, _)) = stack.pop() {
                        if open != '[' {
                            result.add_issue(ValidationIssue::error(
                                IssueKind::UnmatchedCloseBracket,
                                Some(pos),
                                format!(
                                    "Closing bracket at position {} doesn't match opening '{}'",
                                    pos, open
                                ),
                            ));
                        }
                    } else {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::UnmatchedCloseBracket,
                            Some(pos),
                            format!("Unmatched closing bracket at position {}", pos),
                        ));
                    }
                }
                "(" => stack.push(('(', pos)),
                ")" => {
                    if let Some((open, _)) = stack.pop() {
                        if open != '(' {
                            result.add_issue(ValidationIssue::error(
                                IssueKind::UnmatchedCloseParen,
                                Some(pos),
                                format!(
                                    "Closing paren at position {} doesn't match opening '{}'",
                                    pos, open
                                ),
                            ));
                        }
                    } else {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::UnmatchedCloseParen,
                            Some(pos),
                            format!("Unmatched closing parenthesis at position {}", pos),
                        ));
                    }
                }
                _ => {}
            }
        }

        // Check for unclosed delimiters
        for (open, pos) in stack {
            let kind = match open {
                '{' => IssueKind::UnmatchedOpenBrace,
                '[' => IssueKind::UnmatchedOpenBracket,
                '(' => IssueKind::UnmatchedOpenParen,
                _ => continue,
            };
            result.add_issue(ValidationIssue::error(
                kind,
                Some(pos),
                format!("Unclosed '{}' at position {}", open, pos),
            ));
        }
    }

    /// Validate environment begin/end matching.
    fn validate_environments(&self, tokens: &[&str], result: &mut ValidationResult) {
        let mut env_stack: VecDeque<(String, usize)> = VecDeque::new();
        let mut i = 0;

        while i < tokens.len() {
            if tokens[i] == "\\begin" && i + 3 < tokens.len() {
                // Look for pattern: \begin { envname }
                if tokens[i + 1] == "{" && tokens[i + 3] == "}" {
                    let env_name = tokens[i + 2].to_string();

                    // Check for unknown environment
                    if self.validate_environments && !self.known_environments.contains(&env_name) {
                        result.add_issue(ValidationIssue::warning(
                            IssueKind::UnknownEnvironment,
                            Some(i),
                            format!("Unknown environment '{}' at position {}", env_name, i),
                        ));
                    }

                    env_stack.push_back((env_name, i));
                    i += 4;
                    continue;
                }
            }

            if tokens[i] == "\\end" && i + 3 < tokens.len() {
                // Look for pattern: \end { envname }
                if tokens[i + 1] == "{" && tokens[i + 3] == "}" {
                    let env_name = tokens[i + 2].to_string();

                    if let Some((open_name, open_pos)) = env_stack.pop_back() {
                        if open_name != env_name {
                            result.add_issue(ValidationIssue::error(
                                IssueKind::EnvironmentMismatch,
                                Some(i),
                                format!(
                                    "Environment mismatch: \\begin{{{}}} at {} closed by \\end{{{}}} at {}",
                                    open_name, open_pos, env_name, i
                                ),
                            ));
                        }
                    } else {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::ExtraEnvironmentEnd,
                            Some(i),
                            format!(
                                "Extra \\end{{{}}} at position {} without matching \\begin",
                                env_name, i
                            ),
                        ));
                    }

                    i += 4;
                    continue;
                }
            }

            i += 1;
        }

        // Check for unclosed environments
        while let Some((name, pos)) = env_stack.pop_front() {
            result.add_issue(ValidationIssue::error(
                IssueKind::MissingEnvironmentEnd,
                Some(pos),
                format!(
                    "Unclosed environment '{}' starting at position {}",
                    name, pos
                ),
            ));
        }
    }

    /// Validate math delimiter matching.
    fn validate_math_delimiters(&self, tokens: &[&str], result: &mut ValidationResult) {
        let mut in_inline_math = false;
        let mut in_display_math = false;
        let mut inline_start: Option<usize> = None;
        let mut display_start: Option<usize> = None;

        for (pos, token) in tokens.iter().enumerate() {
            match *token {
                "$" => {
                    if in_display_math {
                        // Check if this might be closing $$
                        continue;
                    }
                    if in_inline_math {
                        in_inline_math = false;
                        inline_start = None;
                    } else {
                        if !self.allow_nested_math && in_display_math {
                            result.add_issue(ValidationIssue::error(
                                IssueKind::NestedMathMode,
                                Some(pos),
                                format!("Nested math mode at position {}", pos),
                            ));
                        }
                        in_inline_math = true;
                        inline_start = Some(pos);
                    }
                }
                "$$" => {
                    if in_display_math {
                        in_display_math = false;
                        display_start = None;
                    } else {
                        if !self.allow_nested_math && in_inline_math {
                            result.add_issue(ValidationIssue::error(
                                IssueKind::NestedMathMode,
                                Some(pos),
                                format!("Nested display math mode at position {}", pos),
                            ));
                        }
                        in_display_math = true;
                        display_start = Some(pos);
                    }
                }
                "\\[" => {
                    if !self.allow_nested_math && (in_inline_math || in_display_math) {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::NestedMathMode,
                            Some(pos),
                            format!("Nested display math mode at position {}", pos),
                        ));
                    }
                    in_display_math = true;
                    display_start = Some(pos);
                }
                "\\]" => {
                    if in_display_math {
                        in_display_math = false;
                        display_start = None;
                    } else {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::UnmatchedMathDelimiter,
                            Some(pos),
                            format!("Unmatched \\] at position {}", pos),
                        ));
                    }
                }
                "\\(" => {
                    if !self.allow_nested_math && (in_inline_math || in_display_math) {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::NestedMathMode,
                            Some(pos),
                            format!("Nested inline math mode at position {}", pos),
                        ));
                    }
                    in_inline_math = true;
                    inline_start = Some(pos);
                }
                "\\)" => {
                    if in_inline_math {
                        in_inline_math = false;
                        inline_start = None;
                    } else {
                        result.add_issue(ValidationIssue::error(
                            IssueKind::UnmatchedMathDelimiter,
                            Some(pos),
                            format!("Unmatched \\) at position {}", pos),
                        ));
                    }
                }
                _ => {}
            }
        }

        // Check for unclosed math modes
        if let Some(pos) = inline_start {
            result.add_issue(ValidationIssue::error(
                IssueKind::UnmatchedMathDelimiter,
                Some(pos),
                format!("Unclosed inline math starting at position {}", pos),
            ));
        }
        if let Some(pos) = display_start {
            result.add_issue(ValidationIssue::error(
                IssueKind::UnmatchedMathDelimiter,
                Some(pos),
                format!("Unclosed display math starting at position {}", pos),
            ));
        }
    }
}

/// Default list of known LaTeX environments.
fn default_environments() -> Vec<String> {
    vec![
        // Document structure
        "document",
        "abstract",
        "titlepage",
        // Sectioning
        "part",
        "chapter",
        "section",
        "subsection",
        // Lists
        "itemize",
        "enumerate",
        "description",
        // Math
        "equation",
        "equation*",
        "align",
        "align*",
        "gather",
        "gather*",
        "multline",
        "multline*",
        "split",
        "cases",
        "aligned",
        "gathered",
        // Matrices
        "matrix",
        "pmatrix",
        "bmatrix",
        "vmatrix",
        "Vmatrix",
        "Bmatrix",
        // Floats
        "figure",
        "figure*",
        "table",
        "table*",
        // Tables
        "tabular",
        "tabular*",
        "array",
        "tabularx",
        // Theorems
        "theorem",
        "lemma",
        "corollary",
        "proposition",
        "definition",
        "example",
        "remark",
        "proof",
        // Formatting
        "center",
        "flushleft",
        "flushright",
        "quote",
        "quotation",
        "verse",
        // Code
        "verbatim",
        "lstlisting",
        // Bibliography
        "thebibliography",
        // Misc
        "minipage",
        "picture",
        "tikzpicture",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_braces() {
        let validator = LatexValidator::new();
        let tokens = vec!["{", "content", "}"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_unmatched_open_brace() {
        let validator = LatexValidator::new();
        let tokens = vec!["{", "content"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::UnmatchedOpenBrace));
    }

    #[test]
    fn test_unmatched_close_brace() {
        let validator = LatexValidator::new();
        let tokens = vec!["content", "}"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::UnmatchedCloseBrace));
    }

    #[test]
    fn test_valid_environment() {
        let validator = LatexValidator::new();
        let tokens = vec![
            "\\begin", "{", "equation", "}", "x", "\\end", "{", "equation", "}",
        ];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_mismatched_environment() {
        let validator = LatexValidator::new();
        let tokens = vec![
            "\\begin", "{", "equation", "}", "x", "\\end", "{", "align", "}",
        ];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::EnvironmentMismatch));
    }

    #[test]
    fn test_unclosed_environment() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\begin", "{", "equation", "}", "x"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::MissingEnvironmentEnd));
    }

    #[test]
    fn test_valid_inline_math() {
        let validator = LatexValidator::new();
        let tokens = vec!["$", "x", "$"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_unclosed_inline_math() {
        let validator = LatexValidator::new();
        let tokens = vec!["$", "x"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::UnmatchedMathDelimiter));
    }

    #[test]
    fn test_unknown_environment_warning() {
        let validator = LatexValidator::new();
        let tokens = vec![
            "\\begin", "{", "myenv", "}", "x", "\\end", "{", "myenv", "}",
        ];
        let result = validator.validate(&tokens);
        // Should be valid but have a warning
        assert!(result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::UnknownEnvironment));
    }

    #[test]
    fn test_nested_brackets() {
        let validator = LatexValidator::new();
        let tokens = vec!["{", "[", "(", ")", "]", "}"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_mismatched_brackets() {
        let validator = LatexValidator::new();
        let tokens = vec!["{", "[", "}", "]"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
    }
}
