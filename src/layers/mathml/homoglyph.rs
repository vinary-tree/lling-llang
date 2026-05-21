//! Homoglyph disambiguation for mathematical symbols.
//!
//! Handles cases where visually similar characters have different meanings:
//! - `x` (variable) vs `×` (multiplication)
//! - `-` (minus) vs `−` (minus sign) vs `–` (en-dash)
//! - `0` (zero) vs `O` (capital O) vs `o` (lowercase o)

use std::collections::HashMap;

/// A meaning that a glyph can have in context.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GlyphMeaning {
    /// Multiplication operator.
    Multiplication,
    /// Variable identifier.
    Variable(String),
    /// Subtraction operator.
    Subtraction,
    /// Minus sign (unary negation).
    UnaryMinus,
    /// Addition operator.
    Addition,
    /// Equals sign.
    Equals,
    /// Digit.
    Digit(u8),
    /// Letter (alphabetic).
    Letter(char),
    /// Prime symbol.
    Prime,
    /// Apostrophe.
    Apostrophe,
    /// Quotation mark.
    Quote,
    /// Division operator.
    Division,
    /// Colon (ratio).
    Ratio,
    /// Period (decimal point).
    DecimalPoint,
    /// Period (sentence end).
    SentenceEnd,
    /// Comma (separator).
    Separator,
    /// Comma (decimal separator in some locales).
    DecimalSeparator,
    /// Parenthesis (grouping).
    Grouping,
    /// Parenthesis (function application).
    FunctionApplication,
    /// Unknown/ambiguous.
    Unknown,
}

/// A set of confusable glyphs.
#[derive(Debug, Clone)]
pub struct HomoglyphSet {
    /// The glyphs in this confusion set.
    pub glyphs: Vec<char>,
    /// Possible meanings for these glyphs.
    pub meanings: Vec<GlyphMeaning>,
    /// Canonical form (preferred glyph).
    pub canonical: char,
}

impl HomoglyphSet {
    /// Create a new homoglyph set.
    pub fn new(glyphs: Vec<char>, meanings: Vec<GlyphMeaning>, canonical: char) -> Self {
        Self {
            glyphs,
            meanings,
            canonical,
        }
    }

    /// Check if a glyph is in this set.
    pub fn contains(&self, glyph: char) -> bool {
        self.glyphs.contains(&glyph)
    }
}

/// Context for disambiguating homoglyphs.
#[derive(Debug, Clone, Default)]
pub struct MathContext {
    /// Whether we're in math mode.
    pub in_math_mode: bool,
    /// Whether we're in text mode.
    pub in_text_mode: bool,
    /// Previous token (if any).
    pub prev_token: Option<String>,
    /// Next token (if any).
    pub next_token: Option<String>,
    /// Current nesting depth for parentheses.
    pub paren_depth: i32,
    /// Whether the previous token was an operator.
    pub prev_was_operator: bool,
    /// Whether the previous token was a number.
    pub prev_was_number: bool,
    /// Topic/domain hint.
    pub domain: Option<MathDomain>,
}

/// Mathematical domain for context hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathDomain {
    /// General mathematics.
    General,
    /// Pure algebra.
    Algebra,
    /// Calculus/analysis.
    Analysis,
    /// Linear algebra.
    LinearAlgebra,
    /// Number theory.
    NumberTheory,
    /// Statistics/probability.
    Statistics,
    /// Physics.
    Physics,
    /// Computer science.
    ComputerScience,
}

/// Disambiguator for homoglyphs.
pub struct HomoglyphDisambiguator {
    /// Confusion sets indexed by character.
    confusion_sets: HashMap<char, HomoglyphSet>,
    /// Configuration.
    config: DisambiguatorConfig,
}

/// Configuration for the disambiguator.
#[derive(Debug, Clone)]
pub struct DisambiguatorConfig {
    /// Weight for context-based disambiguation.
    pub context_weight: f32,
    /// Weight for frequency-based disambiguation.
    pub frequency_weight: f32,
    /// Whether to normalize to canonical forms.
    pub normalize: bool,
}

impl Default for DisambiguatorConfig {
    fn default() -> Self {
        Self {
            context_weight: 0.7,
            frequency_weight: 0.3,
            normalize: true,
        }
    }
}

impl Default for HomoglyphDisambiguator {
    fn default() -> Self {
        Self::new()
    }
}

impl HomoglyphDisambiguator {
    /// Create a new disambiguator with standard confusion sets.
    pub fn new() -> Self {
        let mut disambiguator = Self {
            confusion_sets: HashMap::new(),
            config: DisambiguatorConfig::default(),
        };
        disambiguator.register_standard_confusions();
        disambiguator
    }

    /// Create with custom configuration.
    pub fn with_config(config: DisambiguatorConfig) -> Self {
        let mut disambiguator = Self {
            confusion_sets: HashMap::new(),
            config,
        };
        disambiguator.register_standard_confusions();
        disambiguator
    }

    /// Register standard confusion sets.
    fn register_standard_confusions(&mut self) {
        // Multiplication vs letter x
        self.register(HomoglyphSet::new(
            vec!['x', 'X', '×', '✕', '✖', '⨯'],
            vec![
                GlyphMeaning::Variable("x".to_string()),
                GlyphMeaning::Multiplication,
            ],
            'x',
        ));

        // Minus signs and dashes
        self.register(HomoglyphSet::new(
            vec!['-', '−', '–', '—', '‐', '‑'],
            vec![GlyphMeaning::Subtraction, GlyphMeaning::UnaryMinus],
            '-',
        ));

        // Zero vs letter O
        self.register(HomoglyphSet::new(
            vec!['0', 'O', 'o', 'Ο', 'ο', '০'],
            vec![
                GlyphMeaning::Digit(0),
                GlyphMeaning::Variable("O".to_string()),
                GlyphMeaning::Variable("o".to_string()),
            ],
            '0',
        ));

        // One vs letter l vs pipe
        self.register(HomoglyphSet::new(
            vec!['1', 'l', 'I', '|', 'ǀ', 'ⅼ'],
            vec![
                GlyphMeaning::Digit(1),
                GlyphMeaning::Variable("l".to_string()),
                GlyphMeaning::Variable("I".to_string()),
            ],
            '1',
        ));

        // Two vs letter Z
        self.register(HomoglyphSet::new(
            vec!['2', 'Z', 'z', 'Ζ', 'ζ'],
            vec![
                GlyphMeaning::Digit(2),
                GlyphMeaning::Variable("Z".to_string()),
            ],
            '2',
        ));

        // Five vs letter S
        self.register(HomoglyphSet::new(
            vec!['5', 'S', 's', 'Ѕ', 'ѕ'],
            vec![
                GlyphMeaning::Digit(5),
                GlyphMeaning::Variable("S".to_string()),
            ],
            '5',
        ));

        // Prime symbols
        self.register(HomoglyphSet::new(
            vec!['\'', '′', 'ʹ', 'ˈ', '\u{2019}'], // U+2019 is right single quotation mark
            vec![GlyphMeaning::Prime, GlyphMeaning::Apostrophe],
            '\'',
        ));

        // Division symbols
        self.register(HomoglyphSet::new(
            vec!['/', '÷', '∕', '⁄'],
            vec![GlyphMeaning::Division],
            '/',
        ));

        // Colon vs ratio
        self.register(HomoglyphSet::new(
            vec![':', '∶', '꞉'],
            vec![GlyphMeaning::Ratio],
            ':',
        ));

        // Period ambiguities
        self.register(HomoglyphSet::new(
            vec!['.', '·', '⋅', '∙'],
            vec![
                GlyphMeaning::DecimalPoint,
                GlyphMeaning::Multiplication,
                GlyphMeaning::SentenceEnd,
            ],
            '.',
        ));

        // Comma ambiguities
        self.register(HomoglyphSet::new(
            vec![',', '，'],
            vec![GlyphMeaning::Separator, GlyphMeaning::DecimalSeparator],
            ',',
        ));

        // Plus signs
        self.register(HomoglyphSet::new(
            vec!['+', '＋', '➕'],
            vec![GlyphMeaning::Addition],
            '+',
        ));

        // Equals signs
        self.register(HomoglyphSet::new(
            vec!['=', '＝', '⩵', '₌'],
            vec![GlyphMeaning::Equals],
            '=',
        ));

        // Greek/Latin confusions
        self.register(HomoglyphSet::new(
            vec!['A', 'Α'], // Latin A vs Greek Alpha
            vec![GlyphMeaning::Variable("A".to_string())],
            'A',
        ));

        self.register(HomoglyphSet::new(
            vec!['B', 'Β', 'В'], // Latin B vs Greek Beta vs Cyrillic Ve
            vec![GlyphMeaning::Variable("B".to_string())],
            'B',
        ));

        self.register(HomoglyphSet::new(
            vec!['E', 'Ε', 'Е'], // Latin E vs Greek Epsilon vs Cyrillic Ie
            vec![GlyphMeaning::Variable("E".to_string())],
            'E',
        ));

        self.register(HomoglyphSet::new(
            vec!['H', 'Η', 'Н'], // Latin H vs Greek Eta vs Cyrillic En
            vec![GlyphMeaning::Variable("H".to_string())],
            'H',
        ));

        self.register(HomoglyphSet::new(
            vec!['K', 'Κ', 'К'], // Latin K vs Greek Kappa vs Cyrillic Ka
            vec![GlyphMeaning::Variable("K".to_string())],
            'K',
        ));

        self.register(HomoglyphSet::new(
            vec!['M', 'Μ', 'М'], // Latin M vs Greek Mu vs Cyrillic Em
            vec![GlyphMeaning::Variable("M".to_string())],
            'M',
        ));

        self.register(HomoglyphSet::new(
            vec!['N', 'Ν'], // Latin N vs Greek Nu
            vec![GlyphMeaning::Variable("N".to_string())],
            'N',
        ));

        self.register(HomoglyphSet::new(
            vec!['P', 'Ρ', 'Р'], // Latin P vs Greek Rho vs Cyrillic Er
            vec![GlyphMeaning::Variable("P".to_string())],
            'P',
        ));

        self.register(HomoglyphSet::new(
            vec!['T', 'Τ', 'Т'], // Latin T vs Greek Tau vs Cyrillic Te
            vec![GlyphMeaning::Variable("T".to_string())],
            'T',
        ));

        self.register(HomoglyphSet::new(
            vec!['Y', 'Υ', 'У'], // Latin Y vs Greek Upsilon vs Cyrillic U
            vec![GlyphMeaning::Variable("Y".to_string())],
            'Y',
        ));
    }

    /// Register a confusion set.
    pub fn register(&mut self, set: HomoglyphSet) {
        for &glyph in &set.glyphs {
            self.confusion_sets.insert(glyph, set.clone());
        }
    }

    /// Check if a character is potentially ambiguous.
    pub fn is_ambiguous(&self, c: char) -> bool {
        self.confusion_sets.contains_key(&c)
    }

    /// Get the confusion set for a character.
    pub fn get_confusion_set(&self, c: char) -> Option<&HomoglyphSet> {
        self.confusion_sets.get(&c)
    }

    /// Disambiguate a glyph based on context.
    pub fn disambiguate(&self, glyph: char, context: &MathContext) -> GlyphMeaning {
        let Some(set) = self.confusion_sets.get(&glyph) else {
            return GlyphMeaning::Unknown;
        };

        // If only one meaning, return it
        if set.meanings.len() == 1 {
            return set.meanings[0].clone();
        }

        // Use context to disambiguate
        self.disambiguate_with_context(glyph, set, context)
    }

    /// Disambiguate using context.
    fn disambiguate_with_context(
        &self,
        glyph: char,
        set: &HomoglyphSet,
        context: &MathContext,
    ) -> GlyphMeaning {
        // Score each possible meaning
        let mut best_meaning = set.meanings[0].clone();
        let mut best_score = 0.0f32;

        for meaning in &set.meanings {
            let score = self.score_meaning(glyph, meaning, context);
            if score > best_score {
                best_score = score;
                best_meaning = meaning.clone();
            }
        }

        best_meaning
    }

    /// Score a meaning based on context.
    fn score_meaning(&self, glyph: char, meaning: &GlyphMeaning, context: &MathContext) -> f32 {
        let mut score = 0.5; // Base score

        match meaning {
            GlyphMeaning::Multiplication => {
                // More likely after a number or closing paren
                if context.prev_was_number {
                    score += 0.3;
                }
                // More likely in math mode
                if context.in_math_mode {
                    score += 0.2;
                }
                // Check for specific glyphs
                if glyph == '×' || glyph == '⋅' || glyph == '∙' {
                    score += 0.4; // These are explicitly multiplication
                }
            }
            GlyphMeaning::Variable(_) => {
                // More likely after operators
                if context.prev_was_operator {
                    score += 0.3;
                }
                // Check for letter-like glyphs
                if glyph.is_alphabetic() {
                    score += 0.2;
                }
            }
            GlyphMeaning::Subtraction => {
                // Binary minus: after number or closing paren
                if context.prev_was_number {
                    score += 0.4;
                }
            }
            GlyphMeaning::UnaryMinus => {
                // Unary minus: at start or after operator
                if context.prev_was_operator || context.prev_token.is_none() {
                    score += 0.4;
                }
            }
            GlyphMeaning::Digit(_) => {
                // More likely if previous was a digit or decimal point
                if let Some(ref prev) = context.prev_token {
                    if prev.chars().all(|c| c.is_ascii_digit() || c == '.') {
                        score += 0.4;
                    }
                }
            }
            GlyphMeaning::DecimalPoint => {
                // More likely between digits
                if context.prev_was_number {
                    if let Some(ref next) = context.next_token {
                        if next
                            .chars()
                            .next()
                            .map(|c| c.is_ascii_digit())
                            .unwrap_or(false)
                        {
                            score += 0.5;
                        }
                    }
                }
            }
            GlyphMeaning::Prime => {
                // More likely after a variable in math mode
                if context.in_math_mode && !context.prev_was_number && !context.prev_was_operator {
                    score += 0.4;
                }
            }
            _ => {}
        }

        score
    }

    /// Normalize a string by replacing homoglyphs with canonical forms.
    pub fn normalize(&self, input: &str) -> String {
        if !self.config.normalize {
            return input.to_string();
        }

        input
            .chars()
            .map(|c| {
                if let Some(set) = self.confusion_sets.get(&c) {
                    set.canonical
                } else {
                    c
                }
            })
            .collect()
    }

    /// Get all characters that could be confused with the given character.
    pub fn get_confusables(&self, c: char) -> Vec<char> {
        self.confusion_sets
            .get(&c)
            .map(|set| set.glyphs.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disambiguator_creation() {
        let disambiguator = HomoglyphDisambiguator::new();
        assert!(disambiguator.is_ambiguous('x'));
        assert!(disambiguator.is_ambiguous('×'));
        assert!(disambiguator.is_ambiguous('-'));
        assert!(disambiguator.is_ambiguous('0'));
    }

    #[test]
    fn test_get_confusion_set() {
        let disambiguator = HomoglyphDisambiguator::new();

        let set = disambiguator.get_confusion_set('x');
        assert!(set.is_some());
        let set = set.unwrap();
        assert!(set.contains('×'));
    }

    #[test]
    fn test_disambiguate_x_after_number() {
        let disambiguator = HomoglyphDisambiguator::new();

        let context = MathContext {
            in_math_mode: true,
            prev_was_number: true,
            ..Default::default()
        };

        let meaning = disambiguator.disambiguate('x', &context);
        // After a number, x is more likely multiplication
        assert!(matches!(
            meaning,
            GlyphMeaning::Multiplication | GlyphMeaning::Variable(_)
        ));
    }

    #[test]
    fn test_disambiguate_x_after_operator() {
        let disambiguator = HomoglyphDisambiguator::new();

        let context = MathContext {
            in_math_mode: true,
            prev_was_operator: true,
            ..Default::default()
        };

        let meaning = disambiguator.disambiguate('x', &context);
        // After an operator, x is more likely a variable
        assert!(matches!(meaning, GlyphMeaning::Variable(_)));
    }

    #[test]
    fn test_disambiguate_minus() {
        let disambiguator = HomoglyphDisambiguator::new();

        // After number = subtraction
        let context = MathContext {
            prev_was_number: true,
            ..Default::default()
        };
        let meaning = disambiguator.disambiguate('-', &context);
        assert!(matches!(meaning, GlyphMeaning::Subtraction));

        // After operator = unary minus
        let context = MathContext {
            prev_was_operator: true,
            ..Default::default()
        };
        let meaning = disambiguator.disambiguate('-', &context);
        assert!(matches!(meaning, GlyphMeaning::UnaryMinus));
    }

    #[test]
    fn test_normalize() {
        let disambiguator = HomoglyphDisambiguator::new();

        // Normalize multiplication sign
        let normalized = disambiguator.normalize("2×3");
        assert_eq!(normalized, "2x3");

        // Normalize minus sign
        let normalized = disambiguator.normalize("a−b");
        assert_eq!(normalized, "a-b");
    }

    #[test]
    fn test_get_confusables() {
        let disambiguator = HomoglyphDisambiguator::new();

        let confusables = disambiguator.get_confusables('x');
        assert!(confusables.contains(&'×'));
        assert!(confusables.contains(&'X'));
    }

    #[test]
    fn test_non_ambiguous_char() {
        let disambiguator = HomoglyphDisambiguator::new();

        assert!(!disambiguator.is_ambiguous('q'));
        assert!(disambiguator.get_confusion_set('q').is_none());
    }

    #[test]
    fn test_greek_latin_confusion() {
        let disambiguator = HomoglyphDisambiguator::new();

        // Greek Alpha vs Latin A
        assert!(disambiguator.is_ambiguous('Α'));
        let set = disambiguator.get_confusion_set('Α').unwrap();
        assert!(set.contains('A'));
    }

    #[test]
    fn test_zero_o_confusion() {
        let disambiguator = HomoglyphDisambiguator::new();

        let set = disambiguator.get_confusion_set('0').unwrap();
        assert!(set.contains('O'));
        assert!(set.contains('o'));
    }

    #[test]
    fn test_homoglyph_set() {
        let set = HomoglyphSet::new(
            vec!['a', 'ɑ', 'α'],
            vec![GlyphMeaning::Variable("a".to_string())],
            'a',
        );

        assert!(set.contains('a'));
        assert!(set.contains('α'));
        assert!(!set.contains('b'));
        assert_eq!(set.canonical, 'a');
    }

    #[test]
    fn test_config() {
        let config = DisambiguatorConfig {
            normalize: false,
            ..Default::default()
        };
        let disambiguator = HomoglyphDisambiguator::with_config(config);

        // With normalize=false, should return original
        let result = disambiguator.normalize("2×3");
        assert_eq!(result, "2×3");
    }
}
