//! Structural validation for LaTeX documents.
//!
//! Provides validation beyond CFG parsing: brace matching, environment
//! pairing, math delimiter balance, and other structural constraints.

use std::collections::{HashSet, VecDeque};

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
    known_environments: HashSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CommandSpec {
    required_args: usize,
    optional_args: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TokenGroup {
    start: usize,
    end: usize,
    is_empty: bool,
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
        self.known_environments.insert(name.into());
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

        // Check command argument counts when requested
        if self.validate_arguments {
            self.validate_command_arguments(tokens, &mut result);
        }

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

    /// Validate required argument groups for known fixed-arity commands.
    fn validate_command_arguments(&self, tokens: &[&str], result: &mut ValidationResult) {
        let mut i = 0;

        while i < tokens.len() {
            let command = tokens[i];
            let Some(spec) = command_spec(command) else {
                i += 1;
                continue;
            };

            let mut cursor = i + 1;
            for _ in 0..spec.optional_args {
                let Some(group) = consume_group(tokens, cursor, "[", "]") else {
                    break;
                };
                cursor = group.end + 1;
            }

            for _ in 0..spec.required_args {
                let Some(group) = consume_group(tokens, cursor, "{", "}") else {
                    result.add_issue(ValidationIssue::error(
                        IssueKind::InvalidArgumentCount,
                        Some(i),
                        format!(
                            "Command '{}' at position {} expects {} required argument(s)",
                            command, i, spec.required_args
                        ),
                    ));
                    break;
                };

                if group.is_empty {
                    result.add_issue(ValidationIssue::error(
                        IssueKind::EmptyRequiredArgument,
                        Some(group.start),
                        format!(
                            "Command '{}' has an empty required argument at position {}",
                            command, group.start
                        ),
                    ));
                }

                cursor = group.end + 1;
            }

            i += 1;
        }
    }
}

fn command_spec(command: &str) -> Option<CommandSpec> {
    let required_args = match command {
        "\\frac" | "\\dfrac" | "\\tfrac" | "\\binom" | "\\href" => 2,
        "\\textcolor" => {
            return Some(CommandSpec {
                required_args: 2,
                optional_args: 1,
            });
        }
        "\\sqrt" => {
            return Some(CommandSpec {
                required_args: 1,
                optional_args: 1,
            });
        }
        "\\cite" => {
            return Some(CommandSpec {
                required_args: 1,
                optional_args: 2,
            });
        }
        "\\section" | "\\subsection" | "\\subsubsection" | "\\paragraph" | "\\subparagraph"
        | "\\chapter" | "\\part" | "\\includegraphics" | "\\documentclass" | "\\usepackage"
        | "\\color" => {
            return Some(CommandSpec {
                required_args: 1,
                optional_args: 1,
            });
        }
        "\\text" | "\\textrm" | "\\textbf" | "\\textit" | "\\texttt" | "\\emph" | "\\mathrm"
        | "\\mathbf" | "\\mathit" | "\\mathsf" | "\\mathtt" | "\\mathcal" | "\\mathbb"
        | "\\mathfrak" | "\\underline" | "\\overline" | "\\hat" | "\\bar" | "\\tilde" | "\\vec"
        | "\\dot" | "\\ddot" | "\\label" | "\\ref" | "\\url" => 1,
        _ => return None,
    };

    Some(CommandSpec {
        required_args,
        optional_args: 0,
    })
}

fn consume_group(tokens: &[&str], start: usize, open: &str, close: &str) -> Option<TokenGroup> {
    if tokens.get(start).copied() != Some(open) {
        return None;
    }

    let mut depth = 0usize;
    for (pos, token) in tokens.iter().enumerate().skip(start) {
        if *token == open {
            depth += 1;
        } else if *token == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(TokenGroup {
                    start,
                    end: pos,
                    is_empty: pos == start + 1,
                });
            }
        }
    }

    None
}

/// Default list of known LaTeX environments.
fn default_environments() -> HashSet<String> {
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

    #[test]
    fn test_valid_command_arguments() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\frac", "{", "1", "}", "{", "x", "}"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
        assert!(result
            .issues
            .iter()
            .all(|i| i.kind != IssueKind::InvalidArgumentCount));
    }

    #[test]
    fn test_missing_command_argument() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\frac", "{", "1", "}"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::InvalidArgumentCount));
    }

    #[test]
    fn test_empty_required_command_argument() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\textbf", "{", "}"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::EmptyRequiredArgument));
    }

    #[test]
    fn test_optional_command_argument() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\sqrt", "[", "3", "]", "{", "x", "}"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_multiple_optional_command_arguments() {
        let validator = LatexValidator::new();
        let tokens = vec![
            "\\cite", "[", "see", "]", "[", "p.", "3", "]", "{", "key", "}",
        ];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_color_optional_argument() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\color", "[", "rgb", "]", "{", "1,0,0", "}"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_argument_validation_can_be_disabled() {
        let validator = LatexValidator::new().with_argument_validation(false);
        let tokens = vec!["\\frac", "{", "1", "}"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
        assert!(result
            .issues
            .iter()
            .all(|i| i.kind != IssueKind::InvalidArgumentCount));
    }

    #[test]
    fn test_nested_command_arguments_are_validated() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\frac", "{", "\\textbf", "{", "}", "}", "{", "x", "}"];
        let result = validator.validate(&tokens);
        assert!(!result.is_valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::EmptyRequiredArgument));
    }

    #[test]
    fn test_unknown_commands_are_not_over_validated() {
        let validator = LatexValidator::new();
        let tokens = vec!["\\custommacro"];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
    }

    #[test]
    fn test_custom_environment_avoids_unknown_warning() {
        let validator = LatexValidator::new().add_environment("proofsketch");
        let tokens = vec![
            "\\begin",
            "{",
            "proofsketch",
            "}",
            "x",
            "\\end",
            "{",
            "proofsketch",
            "}",
        ];
        let result = validator.validate(&tokens);
        assert!(result.is_valid);
        assert!(result
            .issues
            .iter()
            .all(|i| i.kind != IssueKind::UnknownEnvironment));
    }
}
