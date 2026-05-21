//! LaTeX grammar definition for CFG-based filtering.
//!
//! Defines a context-free grammar for LaTeX document structure.

use crate::cfg::{Grammar, GrammarBuilder, GrammarError};
use std::sync::Arc;

/// Error type for LaTeX grammar construction.
#[derive(Debug, Clone)]
pub enum LatexGrammarError {
    /// Grammar construction failed.
    Construction(String),
    /// Invalid grammar configuration.
    Configuration(String),
}

impl std::fmt::Display for LatexGrammarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LatexGrammarError::Construction(msg) => {
                write!(f, "grammar construction error: {}", msg)
            }
            LatexGrammarError::Configuration(msg) => {
                write!(f, "grammar configuration error: {}", msg)
            }
        }
    }
}

impl std::error::Error for LatexGrammarError {}

impl From<GrammarError> for LatexGrammarError {
    fn from(e: GrammarError) -> Self {
        LatexGrammarError::Construction(format!("{:?}", e))
    }
}

/// LaTeX grammar for syntactic filtering.
///
/// Wraps a CFG grammar with LaTeX-specific terminal mappings.
#[derive(Clone)]
pub struct LatexGrammar {
    grammar: Arc<Grammar>,
}

impl std::fmt::Debug for LatexGrammar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LatexGrammar")
            .field("num_productions", &self.grammar.num_productions())
            .finish()
    }
}

impl LatexGrammar {
    /// Create a standard LaTeX grammar with common constructs.
    pub fn standard() -> Result<Self, LatexGrammarError> {
        LatexGrammarBuilder::new().build_standard()
    }

    /// Create a math-focused LaTeX grammar.
    pub fn math() -> Result<Self, LatexGrammarError> {
        LatexGrammarBuilder::new().build_math()
    }

    /// Create a minimal LaTeX grammar for fast parsing.
    pub fn minimal() -> Result<Self, LatexGrammarError> {
        LatexGrammarBuilder::new().build_minimal()
    }

    /// Get the underlying CFG grammar.
    pub fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    /// Get an Arc reference to the grammar for sharing.
    pub fn grammar_arc(&self) -> Arc<Grammar> {
        Arc::clone(&self.grammar)
    }
}

/// Builder for constructing LaTeX grammars.
pub struct LatexGrammarBuilder {
    include_amsmath: bool,
    include_environments: bool,
    include_commands: bool,
    include_math: bool,
}

impl Default for LatexGrammarBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LatexGrammarBuilder {
    /// Create a new grammar builder with default settings.
    pub fn new() -> Self {
        Self {
            include_amsmath: true,
            include_environments: true,
            include_commands: true,
            include_math: true,
        }
    }

    /// Include AMS math extensions.
    pub fn with_amsmath(mut self, include: bool) -> Self {
        self.include_amsmath = include;
        self
    }

    /// Include environment rules.
    pub fn with_environments(mut self, include: bool) -> Self {
        self.include_environments = include;
        self
    }

    /// Include command rules.
    pub fn with_commands(mut self, include: bool) -> Self {
        self.include_commands = include;
        self
    }

    /// Include math mode rules.
    pub fn with_math(mut self, include: bool) -> Self {
        self.include_math = include;
        self
    }

    /// Build the standard LaTeX grammar.
    pub fn build_standard(self) -> Result<LatexGrammar, LatexGrammarError> {
        let mut builder = GrammarBuilder::new().start("Document");

        // Document structure
        builder = builder
            .rule("Document", &["ContentList"])
            .rule("ContentList", &["Content", "ContentList"])
            .rule("ContentList", &["Content"])
            .epsilon_rule("ContentList");

        // Content types
        builder = builder
            .rule("Content", &["Environment"])
            .rule("Content", &["Command"])
            .rule("Content", &["MathMode"])
            .rule("Content", &["Group"])
            .rule("Content", &["Text"]);

        // Environment structure
        if self.include_environments {
            builder = self.add_environment_rules(builder);
        }

        // Command structure
        if self.include_commands {
            builder = self.add_command_rules(builder);
        }

        // Math mode
        if self.include_math {
            builder = self.add_math_rules(builder);
        }

        // AMS math extensions
        if self.include_amsmath {
            builder = self.add_amsmath_rules(builder);
        }

        // Groups and text
        builder = builder
            .rule("Group", &["lbrace", "ContentList", "rbrace"])
            .rule("Text", &["text_token"]);

        let grammar = builder.build()?;
        Ok(LatexGrammar {
            grammar: Arc::new(grammar),
        })
    }

    /// Build a math-focused grammar (optimized for equations).
    pub fn build_math(self) -> Result<LatexGrammar, LatexGrammarError> {
        let builder = GrammarBuilder::new()
            .start("MathDocument")
            // Math document is a sequence of math expressions
            .rule("MathDocument", &["MathExprList"])
            .rule("MathExprList", &["MathExpr", "MathExprList"])
            .rule("MathExprList", &["MathExpr"])
            .epsilon_rule("MathExprList")
            // Math expressions
            .rule("MathExpr", &["MathGroup"])
            .rule("MathExpr", &["Fraction"])
            .rule("MathExpr", &["Script"])
            .rule("MathExpr", &["Operator"])
            .rule("MathExpr", &["Symbol"])
            .rule("MathExpr", &["Identifier"])
            .rule("MathExpr", &["Number"])
            // Math groups
            .rule("MathGroup", &["lbrace", "MathExprList", "rbrace"])
            .rule("MathGroup", &["lparen", "MathExprList", "rparen"])
            .rule("MathGroup", &["lbracket", "MathExprList", "rbracket"])
            // Fractions
            .rule("Fraction", &["cmd_frac", "MathGroup", "MathGroup"])
            .rule("Fraction", &["cmd_dfrac", "MathGroup", "MathGroup"])
            .rule("Fraction", &["cmd_tfrac", "MathGroup", "MathGroup"])
            // Scripts (subscript/superscript)
            .rule("Script", &["MathExpr", "underscore", "MathGroup"])
            .rule("Script", &["MathExpr", "caret", "MathGroup"])
            .rule(
                "Script",
                &["MathExpr", "underscore", "MathGroup", "caret", "MathGroup"],
            )
            .rule(
                "Script",
                &["MathExpr", "caret", "MathGroup", "underscore", "MathGroup"],
            )
            // Sum, product, integral
            .rule("Operator", &["cmd_sum", "Limits"])
            .rule("Operator", &["cmd_prod", "Limits"])
            .rule("Operator", &["cmd_int", "Limits"])
            .rule("Operator", &["cmd_lim", "Limits"])
            // Limits (optional sub/superscripts)
            .rule("Limits", &["underscore", "MathGroup", "caret", "MathGroup"])
            .rule("Limits", &["underscore", "MathGroup"])
            .rule("Limits", &["caret", "MathGroup"])
            .epsilon_rule("Limits")
            // Primitives
            .rule("Symbol", &["greek_letter"])
            .rule("Symbol", &["math_symbol"])
            .rule("Identifier", &["identifier"])
            .rule("Number", &["number"]);

        let grammar = builder.build()?;
        Ok(LatexGrammar {
            grammar: Arc::new(grammar),
        })
    }

    /// Build a minimal grammar for fast parsing.
    pub fn build_minimal(self) -> Result<LatexGrammar, LatexGrammarError> {
        let builder = GrammarBuilder::new()
            .start("Document")
            .rule("Document", &["ContentList"])
            .rule("ContentList", &["Content", "ContentList"])
            .epsilon_rule("ContentList")
            .rule("Content", &["Group"])
            .rule("Content", &["Token"])
            .rule("Group", &["lbrace", "ContentList", "rbrace"])
            .rule("Token", &["any_token"]);

        let grammar = builder.build()?;
        Ok(LatexGrammar {
            grammar: Arc::new(grammar),
        })
    }

    fn add_environment_rules(&self, builder: GrammarBuilder) -> GrammarBuilder {
        builder
            // Generic environment structure
            .rule("Environment", &["BeginEnv", "ContentList", "EndEnv"])
            // Begin/end with environment name
            .rule(
                "BeginEnv",
                &["cmd_begin", "lbrace", "EnvName", "rbrace", "OptArgs"],
            )
            .rule("EndEnv", &["cmd_end", "lbrace", "EnvName", "rbrace"])
            // Common environment names
            .rule("EnvName", &["env_document"])
            .rule("EnvName", &["env_equation"])
            .rule("EnvName", &["env_align"])
            .rule("EnvName", &["env_figure"])
            .rule("EnvName", &["env_table"])
            .rule("EnvName", &["env_itemize"])
            .rule("EnvName", &["env_enumerate"])
            .rule("EnvName", &["env_tabular"])
            .rule("EnvName", &["env_theorem"])
            .rule("EnvName", &["env_proof"])
            .rule("EnvName", &["env_center"])
            // Optional arguments
            .rule(
                "OptArgs",
                &["lbracket", "ArgContent", "rbracket", "OptArgs"],
            )
            .epsilon_rule("OptArgs")
            .rule("ArgContent", &["arg_token"])
            .epsilon_rule("ArgContent")
    }

    fn add_command_rules(&self, builder: GrammarBuilder) -> GrammarBuilder {
        builder
            // Command structure
            .rule("Command", &["CommandName", "Arguments"])
            // Commands with different arities
            .rule("CommandName", &["cmd_section"])
            .rule("CommandName", &["cmd_subsection"])
            .rule("CommandName", &["cmd_label"])
            .rule("CommandName", &["cmd_ref"])
            .rule("CommandName", &["cmd_cite"])
            .rule("CommandName", &["cmd_textbf"])
            .rule("CommandName", &["cmd_textit"])
            .rule("CommandName", &["cmd_emph"])
            .rule("CommandName", &["cmd_usepackage"])
            .rule("CommandName", &["cmd_documentclass"])
            .rule("CommandName", &["cmd_include"])
            .rule("CommandName", &["cmd_input"])
            // Arguments (0 to 3 required args, plus optional)
            .rule("Arguments", &["OptArgs", "ReqArgs"])
            .rule("ReqArgs", &["Group", "ReqArgs"])
            .epsilon_rule("ReqArgs")
    }

    fn add_math_rules(&self, builder: GrammarBuilder) -> GrammarBuilder {
        builder
            // Math mode delimiters
            .rule("MathMode", &["InlineMath"])
            .rule("MathMode", &["DisplayMath"])
            // Inline math: $...$ or \(...\)
            .rule("InlineMath", &["dollar", "MathContent", "dollar"])
            .rule("InlineMath", &["cmd_lparen", "MathContent", "cmd_rparen"])
            // Display math: $$...$$ or \[...\]
            .rule("DisplayMath", &["ddollar", "MathContent", "ddollar"])
            .rule(
                "DisplayMath",
                &["cmd_lbracket", "MathContent", "cmd_rbracket"],
            )
            // Math content
            .rule("MathContent", &["MathExpr", "MathContent"])
            .epsilon_rule("MathContent")
            // Math expressions (simplified for standard grammar)
            .rule("MathExpr", &["MathGroup"])
            .rule("MathExpr", &["MathCommand"])
            .rule("MathExpr", &["MathToken"])
            // Math groups
            .rule("MathGroup", &["lbrace", "MathContent", "rbrace"])
            // Math commands
            .rule("MathCommand", &["cmd_frac", "MathGroup", "MathGroup"])
            .rule("MathCommand", &["cmd_sqrt", "OptArgs", "MathGroup"])
            .rule("MathCommand", &["greek_cmd"])
            .rule("MathCommand", &["relation_cmd"])
            .rule("MathCommand", &["operator_cmd"])
            // Math tokens
            .rule("MathToken", &["math_char"])
            .rule("MathToken", &["math_number"])
            .rule("MathToken", &["underscore"])
            .rule("MathToken", &["caret"])
    }

    fn add_amsmath_rules(&self, builder: GrammarBuilder) -> GrammarBuilder {
        builder
            // AMS environments
            .rule("EnvName", &["env_align_star"])
            .rule("EnvName", &["env_gather"])
            .rule("EnvName", &["env_gather_star"])
            .rule("EnvName", &["env_multline"])
            .rule("EnvName", &["env_split"])
            .rule("EnvName", &["env_cases"])
            .rule("EnvName", &["env_matrix"])
            .rule("EnvName", &["env_pmatrix"])
            .rule("EnvName", &["env_bmatrix"])
            .rule("EnvName", &["env_vmatrix"])
            // AMS commands
            .rule("MathCommand", &["cmd_text", "Group"])
            .rule("MathCommand", &["cmd_boldsymbol", "MathGroup"])
            .rule("MathCommand", &["cmd_binom", "MathGroup", "MathGroup"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_grammar() {
        let grammar = LatexGrammar::standard();
        assert!(
            grammar.is_ok(),
            "standard grammar should build: {:?}",
            grammar
        );
        let g = grammar.expect("layers/latex/grammar.rs: required value was None/Err");
        assert!(g.grammar().num_productions() > 0);
    }

    #[test]
    fn test_math_grammar() {
        let grammar = LatexGrammar::math();
        assert!(grammar.is_ok(), "math grammar should build: {:?}", grammar);
        let g = grammar.expect("layers/latex/grammar.rs: required value was None/Err");
        assert!(g.grammar().num_productions() > 0);
    }

    #[test]
    fn test_minimal_grammar() {
        let grammar = LatexGrammar::minimal();
        assert!(
            grammar.is_ok(),
            "minimal grammar should build: {:?}",
            grammar
        );
        let g = grammar.expect("layers/latex/grammar.rs: required value was None/Err");
        assert!(g.grammar().num_productions() > 0);
    }

    #[test]
    fn test_builder_customization() {
        let grammar = LatexGrammarBuilder::new()
            .with_amsmath(false)
            .with_environments(true)
            .with_math(true)
            .with_commands(true)
            .build_standard();

        assert!(grammar.is_ok());
    }

    #[test]
    fn test_grammar_arc() {
        let grammar =
            LatexGrammar::minimal().expect("layers/latex/grammar.rs: required value was None/Err");
        let arc1 = grammar.grammar_arc();
        let arc2 = grammar.grammar_arc();

        // Should be the same Arc
        assert!(Arc::ptr_eq(&arc1, &arc2));
    }
}
