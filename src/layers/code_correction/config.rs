//! Configuration types for code correction layers.

use std::collections::HashSet;
use std::sync::Arc;

/// Supported programming languages for code correction.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CodeCorrectionLanguage {
    /// Python language
    Python,
    /// Rust language
    Rust,
    /// JavaScript language
    JavaScript,
    /// TypeScript language
    TypeScript,
    /// Go language
    Go,
    /// Java language
    Java,
    /// C language
    C,
    /// C++ language
    Cpp,
    /// Rholang (F1R3FLY.io)
    Rholang,
    /// MeTTa (F1R3FLY.io)
    MeTTa,
    /// Generic (language-agnostic)
    Generic,
    /// Custom language with name
    Custom(String),
}

impl CodeCorrectionLanguage {
    /// Parse a language name string.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "python" | "py" => Self::Python,
            "rust" | "rs" => Self::Rust,
            "javascript" | "js" => Self::JavaScript,
            "typescript" | "ts" => Self::TypeScript,
            "go" | "golang" => Self::Go,
            "java" => Self::Java,
            "c" => Self::C,
            "cpp" | "c++" | "cxx" => Self::Cpp,
            "rholang" | "rho" => Self::Rholang,
            "metta" => Self::MeTTa,
            "generic" | "" => Self::Generic,
            other => Self::Custom(other.to_string()),
        }
    }

    /// Get the language name as a string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Python => "python",
            Self::Rust => "rust",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Rholang => "rholang",
            Self::MeTTa => "metta",
            Self::Generic => "generic",
            Self::Custom(s) => s,
        }
    }

    /// Get common keywords for this language.
    pub fn keywords(&self) -> &[&str] {
        match self {
            Self::Python => &[
                "def", "class", "if", "elif", "else", "for", "while", "try", "except", "finally",
                "with", "as", "import", "from", "return", "yield", "lambda", "and", "or", "not",
                "in", "is", "None", "True", "False", "pass", "break", "continue", "raise",
                "assert", "global", "nonlocal", "async", "await",
            ],
            Self::Rust => &[
                "fn", "let", "mut", "const", "static", "struct", "enum", "impl", "trait", "type",
                "where", "if", "else", "match", "loop", "while", "for", "in", "return", "break",
                "continue", "use", "mod", "pub", "crate", "self", "super", "as", "unsafe", "async",
                "await", "move", "ref", "dyn", "box",
            ],
            Self::JavaScript | Self::TypeScript => &[
                "function",
                "var",
                "let",
                "const",
                "if",
                "else",
                "for",
                "while",
                "do",
                "switch",
                "case",
                "default",
                "break",
                "continue",
                "return",
                "try",
                "catch",
                "finally",
                "throw",
                "new",
                "delete",
                "typeof",
                "instanceof",
                "in",
                "of",
                "class",
                "extends",
                "static",
                "get",
                "set",
                "import",
                "export",
                "from",
                "async",
                "await",
                "yield",
                "null",
                "undefined",
                "true",
                "false",
                "this",
            ],
            Self::Go => &[
                "func",
                "var",
                "const",
                "type",
                "struct",
                "interface",
                "map",
                "chan",
                "if",
                "else",
                "for",
                "range",
                "switch",
                "case",
                "default",
                "select",
                "break",
                "continue",
                "return",
                "goto",
                "fallthrough",
                "defer",
                "go",
                "package",
                "import",
                "nil",
                "true",
                "false",
                "iota",
            ],
            Self::Java => &[
                "class",
                "interface",
                "enum",
                "extends",
                "implements",
                "public",
                "private",
                "protected",
                "static",
                "final",
                "abstract",
                "synchronized",
                "volatile",
                "if",
                "else",
                "for",
                "while",
                "do",
                "switch",
                "case",
                "default",
                "break",
                "continue",
                "return",
                "try",
                "catch",
                "finally",
                "throw",
                "throws",
                "new",
                "import",
                "package",
                "this",
                "super",
                "null",
                "true",
                "false",
                "void",
            ],
            Self::C | Self::Cpp => &[
                "if", "else", "for", "while", "do", "switch", "case", "default", "break",
                "continue", "return", "goto", "sizeof", "typedef", "struct", "union", "enum",
                "static", "extern", "const", "volatile", "void", "int", "char", "float", "double",
                "long", "short", "unsigned", "signed", "auto", "register",
            ],
            Self::Rholang => &[
                "new",
                "contract",
                "for",
                "match",
                "if",
                "else",
                "select",
                "Nil",
                "true",
                "false",
                "bundle",
                "with",
                "stdout",
                "stdoutAck",
                "stderr",
                "stderrAck",
            ],
            Self::MeTTa => &[
                "!",
                "=",
                ":",
                "->",
                "let",
                "let*",
                "if",
                "match",
                "case",
                "import",
                "atom",
                "symbol",
                "expression",
                "grounded",
                "type",
                "function",
            ],
            Self::Generic | Self::Custom(_) => &[],
        }
    }

    /// Get common syntax tokens (brackets, operators, etc.) for this language.
    pub fn syntax_tokens(&self) -> &[&str] {
        match self {
            Self::Python => &[
                "(", ")", "[", "]", "{", "}", ":", ",", ".", ";", "=", "==", "!=", "<", ">", "<=",
                ">=", "+", "-", "*", "/", "//", "%", "**", "@", "->", "...", "|", "&", "^", "~",
                "<<", ">>",
            ],
            Self::Rust => &[
                "(", ")", "[", "]", "{", "}", "<", ">", ";", ",", ".", "::", ":", "=", "==", "!=",
                "<=", ">=", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "<<", ">>", "&&",
                "||", "->", "=>", "..", "..=", "?", "#", "'", "@",
            ],
            Self::JavaScript | Self::TypeScript => &[
                "(", ")", "[", "]", "{", "}", ";", ",", ".", ":", "=", "==", "===", "!=", "!==",
                "<", ">", "<=", ">=", "+", "-", "*", "/", "%", "**", "&", "|", "^", "~", "<<",
                ">>", ">>>", "&&", "||", "!", "?", ":", "=>", "...", "?.", "??",
            ],
            Self::Go => &[
                "(", ")", "[", "]", "{", "}", ";", ",", ".", ":", ":=", "=", "==", "!=", "<", ">",
                "<=", ">=", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "<<", ">>", "&&",
                "||", "<-", "...",
            ],
            Self::Java => &[
                "(", ")", "[", "]", "{", "}", ";", ",", ".", ":", "=", "==", "!=", "<", ">", "<=",
                ">=", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "<<", ">>", ">>>", "&&",
                "||", "?", "::", "->", "@",
            ],
            Self::C | Self::Cpp => &[
                "(", ")", "[", "]", "{", "}", ";", ",", ".", "->", "::", ":", "=", "==", "!=", "<",
                ">", "<=", ">=", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "<<", ">>",
                "&&", "||", "?", "#", "##",
            ],
            Self::Rholang => &[
                "(", ")", "[", "]", "{", "}", "|", ";", ",", ".", "!", "?", "*", "@", "~", "<<",
                ">>", "<=", "=>", "/\\", "\\/", "==", "!=", "<", ">", "+", "-", "/", "%", "++",
                "--",
            ],
            Self::MeTTa => &[
                "(", ")", "[", "]", "{", "}", "!", "=", ":", "->", ",", ".", "@", "$", "?", "*",
                "+",
            ],
            Self::Generic | Self::Custom(_) => {
                &["(", ")", "[", "]", "{", "}", ";", ",", ".", ":", "="]
            }
        }
    }

    /// Check if this is a bracket-sensitive language (Python uses indentation).
    pub fn uses_braces(&self) -> bool {
        !matches!(self, Self::Python | Self::MeTTa)
    }

    /// Check if this is a semicolon-terminated language.
    pub fn uses_semicolons(&self) -> bool {
        matches!(
            self,
            Self::Rust
                | Self::JavaScript
                | Self::TypeScript
                | Self::Go
                | Self::Java
                | Self::C
                | Self::Cpp
                | Self::Rholang
        )
    }
}

/// Configuration for code correction layers.
#[derive(Clone, Debug)]
pub struct CodeCorrectionConfig {
    /// Target programming language.
    pub language: CodeCorrectionLanguage,

    /// Maximum number of corrections to generate per token.
    pub max_corrections_per_token: usize,

    /// Maximum edit distance for token corrections.
    pub max_edit_distance: usize,

    /// Cost per edit operation.
    pub edit_cost: f64,

    /// Cost for inserting a missing token.
    pub insertion_cost: f64,

    /// Cost for deleting an unexpected token.
    pub deletion_cost: f64,

    /// Boost (negative cost) for exact keyword matches.
    pub keyword_boost: f64,

    /// Syntax recovery configuration (optional).
    pub syntax_config: Option<super::SyntaxRecoveryConfig>,

    /// Pattern-aware configuration (optional).
    pub pattern_config: Option<super::PatternAwareConfig>,

    /// Token vocabulary (keywords + syntax tokens).
    pub vocabulary: HashSet<Arc<str>>,

    /// Whether to preserve original tokens in the lattice.
    pub keep_original: bool,

    /// Minimum token length for edit distance corrections.
    pub min_token_length: usize,
}

impl CodeCorrectionConfig {
    /// Create a new configuration for the given language.
    pub fn new(language: &str) -> Self {
        let lang = CodeCorrectionLanguage::from_str(language);

        // Build vocabulary from keywords and syntax tokens
        let mut vocabulary = HashSet::new();
        for kw in lang.keywords() {
            vocabulary.insert(Arc::from(*kw));
        }
        for tok in lang.syntax_tokens() {
            vocabulary.insert(Arc::from(*tok));
        }

        Self {
            language: lang,
            max_corrections_per_token: 5,
            max_edit_distance: 2,
            edit_cost: 1.0,
            insertion_cost: 2.0,
            deletion_cost: 1.5,
            keyword_boost: 0.5,
            syntax_config: Some(super::SyntaxRecoveryConfig::default()),
            pattern_config: None,
            vocabulary,
            keep_original: true,
            min_token_length: 2,
        }
    }

    /// Set maximum corrections per token.
    pub fn with_max_corrections(mut self, max: usize) -> Self {
        self.max_corrections_per_token = max;
        self
    }

    /// Set maximum edit distance.
    pub fn with_max_edit_distance(mut self, distance: usize) -> Self {
        self.max_edit_distance = distance;
        self
    }

    /// Set edit cost.
    pub fn with_edit_cost(mut self, cost: f64) -> Self {
        self.edit_cost = cost;
        self
    }

    /// Set insertion cost.
    pub fn with_insertion_cost(mut self, cost: f64) -> Self {
        self.insertion_cost = cost;
        self
    }

    /// Set deletion cost.
    pub fn with_deletion_cost(mut self, cost: f64) -> Self {
        self.deletion_cost = cost;
        self
    }

    /// Set keyword boost.
    pub fn with_keyword_boost(mut self, boost: f64) -> Self {
        self.keyword_boost = boost;
        self
    }

    /// Set syntax recovery configuration.
    pub fn with_syntax_recovery(mut self, config: super::SyntaxRecoveryConfig) -> Self {
        self.syntax_config = Some(config);
        self
    }

    /// Disable syntax recovery.
    pub fn without_syntax_recovery(mut self) -> Self {
        self.syntax_config = None;
        self
    }

    /// Set pattern-aware configuration.
    pub fn with_pattern_aware(mut self, config: super::PatternAwareConfig) -> Self {
        self.pattern_config = Some(config);
        self
    }

    /// Add vocabulary words.
    pub fn with_vocabulary<I, S>(mut self, words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for word in words {
            self.vocabulary.insert(Arc::from(word.as_ref()));
        }
        self
    }

    /// Set whether to keep original tokens.
    pub fn with_keep_original(mut self, keep: bool) -> Self {
        self.keep_original = keep;
        self
    }

    /// Set minimum token length for corrections.
    pub fn with_min_token_length(mut self, len: usize) -> Self {
        self.min_token_length = len;
        self
    }

    /// Check if a token is a keyword for this language.
    pub fn is_keyword(&self, token: &str) -> bool {
        self.language.keywords().contains(&token)
    }

    /// Check if a token is in the vocabulary.
    pub fn is_in_vocabulary(&self, token: &str) -> bool {
        self.vocabulary.contains(token)
    }
}

impl Default for CodeCorrectionConfig {
    fn default() -> Self {
        Self::new("generic")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_from_str() {
        assert_eq!(
            CodeCorrectionLanguage::from_str("python"),
            CodeCorrectionLanguage::Python
        );
        assert_eq!(
            CodeCorrectionLanguage::from_str("py"),
            CodeCorrectionLanguage::Python
        );
        assert_eq!(
            CodeCorrectionLanguage::from_str("RUST"),
            CodeCorrectionLanguage::Rust
        );
        assert_eq!(
            CodeCorrectionLanguage::from_str("rs"),
            CodeCorrectionLanguage::Rust
        );
        assert_eq!(
            CodeCorrectionLanguage::from_str("rholang"),
            CodeCorrectionLanguage::Rholang
        );
        assert_eq!(
            CodeCorrectionLanguage::from_str("metta"),
            CodeCorrectionLanguage::MeTTa
        );
        assert!(matches!(
            CodeCorrectionLanguage::from_str("unknown"),
            CodeCorrectionLanguage::Custom(_)
        ));
    }

    #[test]
    fn test_language_as_str() {
        assert_eq!(CodeCorrectionLanguage::Python.as_str(), "python");
        assert_eq!(CodeCorrectionLanguage::Rust.as_str(), "rust");
        assert_eq!(CodeCorrectionLanguage::Rholang.as_str(), "rholang");
        assert_eq!(
            CodeCorrectionLanguage::Custom("mylan".to_string()).as_str(),
            "mylan"
        );
    }

    #[test]
    fn test_language_keywords() {
        assert!(CodeCorrectionLanguage::Python.keywords().contains(&"def"));
        assert!(CodeCorrectionLanguage::Rust.keywords().contains(&"fn"));
        assert!(CodeCorrectionLanguage::Rholang.keywords().contains(&"new"));
        assert!(CodeCorrectionLanguage::MeTTa.keywords().contains(&"match"));
    }

    #[test]
    fn test_language_syntax_tokens() {
        assert!(CodeCorrectionLanguage::Python
            .syntax_tokens()
            .contains(&"("));
        assert!(CodeCorrectionLanguage::Rust.syntax_tokens().contains(&"::"));
        assert!(CodeCorrectionLanguage::Rholang
            .syntax_tokens()
            .contains(&"|"));
    }

    #[test]
    fn test_language_uses_braces() {
        assert!(!CodeCorrectionLanguage::Python.uses_braces());
        assert!(CodeCorrectionLanguage::Rust.uses_braces());
        assert!(CodeCorrectionLanguage::JavaScript.uses_braces());
        assert!(!CodeCorrectionLanguage::MeTTa.uses_braces());
    }

    #[test]
    fn test_language_uses_semicolons() {
        assert!(!CodeCorrectionLanguage::Python.uses_semicolons());
        assert!(CodeCorrectionLanguage::Rust.uses_semicolons());
        assert!(CodeCorrectionLanguage::JavaScript.uses_semicolons());
        assert!(CodeCorrectionLanguage::Rholang.uses_semicolons());
    }

    #[test]
    fn test_config_new() {
        let config = CodeCorrectionConfig::new("python");
        assert_eq!(config.language, CodeCorrectionLanguage::Python);
        assert!(config.vocabulary.contains("def"));
        assert!(config.vocabulary.contains("("));
    }

    #[test]
    fn test_config_builder() {
        let config = CodeCorrectionConfig::new("rust")
            .with_max_corrections(10)
            .with_max_edit_distance(3)
            .with_edit_cost(0.5)
            .with_keyword_boost(1.0)
            .with_keep_original(false);

        assert_eq!(config.max_corrections_per_token, 10);
        assert_eq!(config.max_edit_distance, 3);
        assert!((config.edit_cost - 0.5).abs() < 0.001);
        assert!((config.keyword_boost - 1.0).abs() < 0.001);
        assert!(!config.keep_original);
    }

    #[test]
    fn test_config_is_keyword() {
        let config = CodeCorrectionConfig::new("python");
        assert!(config.is_keyword("def"));
        assert!(config.is_keyword("class"));
        assert!(!config.is_keyword("notakeyword"));
    }

    #[test]
    fn test_config_is_in_vocabulary() {
        let config =
            CodeCorrectionConfig::new("rust").with_vocabulary(vec!["my_function", "my_struct"]);

        assert!(config.is_in_vocabulary("fn"));
        assert!(config.is_in_vocabulary("my_function"));
        assert!(!config.is_in_vocabulary("unknown_token"));
    }

    #[test]
    fn test_config_default() {
        let config = CodeCorrectionConfig::default();
        assert_eq!(config.language, CodeCorrectionLanguage::Generic);
    }
}
