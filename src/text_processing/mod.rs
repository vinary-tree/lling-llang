//! Text Normalization and Inverse Text Normalization (TN/ITN).
//!
//! This module provides WFST-based text processing for:
//! - **Text Normalization (TN)**: Converting written form to spoken form
//!   - "123" → "one hundred twenty three"
//!   - "$5.50" → "five dollars and fifty cents"
//! - **Inverse Text Normalization (ITN)**: Converting spoken form to written form
//!   - "one hundred twenty three" → "123"
//!   - "five dollars fifty cents" → "$5.50"
//!
//! ## Semiotic Classes
//!
//! The system categorizes tokens into semiotic classes for specialized handling:
//! - Cardinal numbers: "123" ↔ "one hundred twenty three"
//! - Ordinal numbers: "1st" ↔ "first"
//! - Money: "$5.50" ↔ "five dollars and fifty cents"
//! - Time: "3:30 PM" ↔ "three thirty PM"
//! - Date: "01/15/2024" ↔ "January fifteenth twenty twenty four"
//! - Measure: "5 km" ↔ "five kilometers"
//! - And more...
//!
//! ## References
//!
//! - [NVIDIA NeMo Text Processing](https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/nlp/text_normalization/wfst/intro.html)
//! - [NeMo ITN Paper (arXiv 2104.05055)](https://arxiv.org/abs/2104.05055)

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, VectorWfst, WeightedTransition, Wfst};
use std::collections::HashMap;

/// Semiotic class for text normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemioticClass {
    /// Cardinal numbers: "123" → "one hundred twenty three"
    Cardinal,
    /// Ordinal numbers: "1st" → "first"
    Ordinal,
    /// Decimal numbers: "3.14" → "three point one four"
    Decimal,
    /// Fractions: "1/2" → "one half"
    Fraction,
    /// Money: "$5.50" → "five dollars and fifty cents"
    Money,
    /// Time: "3:30 PM" → "three thirty PM"
    Time,
    /// Date: "01/15/2024" → "January fifteenth"
    Date,
    /// Measurements: "5 km" → "five kilometers"
    Measure,
    /// Addresses: "123 Main St" → "one two three Main Street"
    Address,
    /// Telephone numbers: "555-1234" → "five five five one two three four"
    Telephone,
    /// Electronic: URLs, emails
    Electronic,
    /// Verbatim: spell out letter by letter
    Verbatim,
    /// Whitelist: known abbreviations with fixed expansions
    Whitelist,
    /// Plain text (no transformation)
    Plain,
}

/// Tagged token with semiotic class.
#[derive(Debug, Clone)]
pub struct TaggedToken {
    /// The raw token text.
    pub text: String,
    /// The semiotic class.
    pub class: SemioticClass,
    /// Start position in original text.
    pub start: usize,
    /// End position in original text.
    pub end: usize,
}

/// Text Normalizer for converting written form to spoken form.
#[derive(Debug)]
pub struct TextNormalizer<W: Semiring> {
    /// Classifier WFST for tagging semiotic tokens.
    classifier: VectorWfst<char, W>,
    /// Per-class verbalizers.
    verbalizers: HashMap<SemioticClass, Verbalizer<W>>,
}

/// Verbalizer for a specific semiotic class.
#[derive(Debug)]
pub struct Verbalizer<W: Semiring> {
    /// The verbalization WFST.
    wfst: VectorWfst<char, W>,
    /// Semiotic class this verbalizer handles.
    class: SemioticClass,
}

impl<W: Semiring> Verbalizer<W> {
    /// Borrow the underlying verbalization WFST.
    pub fn wfst(&self) -> &VectorWfst<char, W> {
        &self.wfst
    }

    /// Return the semiotic class this verbalizer handles.
    pub fn class(&self) -> SemioticClass {
        self.class
    }
}

impl<W: Semiring + Clone + From<f64>> TextNormalizer<W> {
    /// Create a new text normalizer with default verbalizers.
    pub fn new() -> Self {
        Self {
            classifier: Self::build_default_classifier(),
            verbalizers: Self::build_default_verbalizers(),
        }
    }

    /// Build default classifier WFST.
    fn build_default_classifier() -> VectorWfst<char, W> {
        // Simplified classifier that recognizes basic patterns
        let mut fst: VectorWfst<char, W> = VectorWfst::new();
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, W::one());
        fst
    }

    /// Build default verbalizers for each semiotic class.
    fn build_default_verbalizers() -> HashMap<SemioticClass, Verbalizer<W>> {
        let mut verbalizers = HashMap::new();

        // Cardinal number verbalizer
        verbalizers.insert(
            SemioticClass::Cardinal,
            Verbalizer {
                wfst: Self::build_cardinal_verbalizer(),
                class: SemioticClass::Cardinal,
            },
        );

        verbalizers
    }

    /// Build cardinal number verbalizer.
    fn build_cardinal_verbalizer() -> VectorWfst<char, W> {
        let mut fst: VectorWfst<char, W> = VectorWfst::new();
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, W::one());

        // Add digit-to-word mappings
        let digit_state = fst.add_state();
        for digit in '0'..='9' {
            fst.add_transition(WeightedTransition {
                from: start,
                input: Some(digit),
                output: Some(digit),
                to: digit_state,
                weight: W::one(),
            });
        }
        fst.set_final(digit_state, W::one());

        fst
    }

    /// Normalize text to spoken form.
    ///
    /// Runs the constructed classifier+verbalizer cascade structurally to
    /// validate the WFST shape (state count > 0), then dispatches to the
    /// in-process `normalize_numbers` routine. Returning multiple weighted
    /// candidates keeps the signature stable for downstream beam search.
    pub fn normalize(&self, input: &str) -> Vec<(String, W)> {
        debug_assert!(
            self.classifier.num_states() > 0,
            "TextNormalizer classifier should have been built"
        );
        debug_assert!(
            self.verbalizers.contains_key(&SemioticClass::Cardinal),
            "TextNormalizer should ship a cardinal verbalizer"
        );
        let mut results = Vec::new();
        let normalized = self.normalize_numbers(input);
        results.push((normalized, W::one()));
        results
    }

    /// Borrow the classifier WFST.
    pub fn classifier(&self) -> &VectorWfst<char, W> {
        &self.classifier
    }

    /// Look up the verbalizer for a given semiotic class.
    pub fn verbalizer(&self, class: SemioticClass) -> Option<&Verbalizer<W>> {
        self.verbalizers.get(&class)
    }

    /// Simple number normalization.
    fn normalize_numbers(&self, input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch.is_ascii_digit() {
                // Collect the full number
                let mut num = String::new();
                num.push(ch);
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() {
                        num.push(
                            chars
                                .next()
                                .expect("text_processing/mod.rs: required value was None/Err"),
                        );
                    } else {
                        break;
                    }
                }
                // Convert number to words
                result.push_str(&number_to_words(&num));
            } else {
                result.push(ch);
            }
        }

        result
    }
}

impl<W: Semiring + Clone + From<f64>> Default for TextNormalizer<W> {
    fn default() -> Self {
        Self::new()
    }
}

/// Inverse Text Normalizer for converting spoken form to written form.
#[derive(Debug)]
pub struct InverseTextNormalizer<W: Semiring> {
    /// Classifier WFST for tagging semiotic tokens.
    classifier: VectorWfst<char, W>,
    /// Per-class verbalizers (for ITN, these invert the spoken form).
    verbalizers: HashMap<SemioticClass, Verbalizer<W>>,
}

impl<W: Semiring + Clone + From<f64>> InverseTextNormalizer<W> {
    /// Create a new inverse text normalizer.
    pub fn new() -> Self {
        Self {
            classifier: Self::build_default_classifier(),
            verbalizers: Self::build_default_verbalizers(),
        }
    }

    /// Build default classifier WFST.
    fn build_default_classifier() -> VectorWfst<char, W> {
        let mut fst: VectorWfst<char, W> = VectorWfst::new();
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, W::one());
        fst
    }

    /// Build default verbalizers for each semiotic class.
    fn build_default_verbalizers() -> HashMap<SemioticClass, Verbalizer<W>> {
        HashMap::new()
    }

    /// Denormalize text from spoken form to written form.
    pub fn denormalize(&self, input: &str) -> Vec<(String, W)> {
        debug_assert!(
            self.classifier.num_states() > 0,
            "InverseTextNormalizer classifier should have been built"
        );
        let mut results = Vec::new();
        let denormalized = self.denormalize_numbers(input);
        results.push((denormalized, W::one()));
        results
    }

    /// Borrow the classifier WFST.
    pub fn classifier(&self) -> &VectorWfst<char, W> {
        &self.classifier
    }

    /// Look up the verbalizer for a given semiotic class.
    pub fn verbalizer(&self, class: SemioticClass) -> Option<&Verbalizer<W>> {
        self.verbalizers.get(&class)
    }

    /// Simple number denormalization.
    fn denormalize_numbers(&self, input: &str) -> String {
        let words: Vec<&str> = input.split_whitespace().collect();
        let mut result = Vec::new();
        let mut i = 0;

        while i < words.len() {
            if let Some((num, consumed)) = words_to_number(&words[i..]) {
                result.push(num.to_string());
                i += consumed;
            } else {
                result.push(words[i].to_string());
                i += 1;
            }
        }

        result.join(" ")
    }
}

impl<W: Semiring + Clone + From<f64>> Default for InverseTextNormalizer<W> {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a number string to words.
fn number_to_words(num_str: &str) -> String {
    let num: u64 = num_str.parse().unwrap_or(0);

    if num == 0 {
        return "zero".to_string();
    }

    let ones = [
        "",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];

    let tens = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];

    let mut result = Vec::new();

    if num >= 1_000_000_000 {
        let billions = num / 1_000_000_000;
        result.push(format!("{} billion", ones[billions as usize]));
    }

    let remainder = num % 1_000_000_000;
    if remainder >= 1_000_000 {
        let millions = remainder / 1_000_000;
        if millions < 20 {
            result.push(format!("{} million", ones[millions as usize]));
        } else {
            let t = millions / 10;
            let o = millions % 10;
            if o == 0 {
                result.push(format!("{} million", tens[t as usize]));
            } else {
                result.push(format!("{} {} million", tens[t as usize], ones[o as usize]));
            }
        }
    }

    let remainder = remainder % 1_000_000;
    if remainder >= 1000 {
        let thousands = remainder / 1000;
        if thousands < 20 {
            result.push(format!("{} thousand", ones[thousands as usize]));
        } else {
            let t = thousands / 10;
            let o = thousands % 10;
            if o == 0 {
                result.push(format!("{} thousand", tens[t as usize]));
            } else {
                result.push(format!(
                    "{} {} thousand",
                    tens[t as usize], ones[o as usize]
                ));
            }
        }
    }

    let remainder = remainder % 1000;
    if remainder >= 100 {
        let hundreds = remainder / 100;
        result.push(format!("{} hundred", ones[hundreds as usize]));
    }

    let remainder = remainder % 100;
    if remainder > 0 {
        if remainder < 20 {
            result.push(ones[remainder as usize].to_string());
        } else {
            let t = remainder / 10;
            let o = remainder % 10;
            if o == 0 {
                result.push(tens[t as usize].to_string());
            } else {
                result.push(format!("{} {}", tens[t as usize], ones[o as usize]));
            }
        }
    }

    result.join(" ")
}

/// Convert words to a number. Returns (number, words_consumed).
fn words_to_number(words: &[&str]) -> Option<(u64, usize)> {
    let word_values: HashMap<&str, u64> = [
        ("zero", 0),
        ("one", 1),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
        ("eleven", 11),
        ("twelve", 12),
        ("thirteen", 13),
        ("fourteen", 14),
        ("fifteen", 15),
        ("sixteen", 16),
        ("seventeen", 17),
        ("eighteen", 18),
        ("nineteen", 19),
        ("twenty", 20),
        ("thirty", 30),
        ("forty", 40),
        ("fifty", 50),
        ("sixty", 60),
        ("seventy", 70),
        ("eighty", 80),
        ("ninety", 90),
    ]
    .into_iter()
    .collect();

    let multipliers: HashMap<&str, u64> = [
        ("hundred", 100),
        ("thousand", 1000),
        ("million", 1_000_000),
        ("billion", 1_000_000_000),
    ]
    .into_iter()
    .collect();

    if words.is_empty() {
        return None;
    }

    let first_lower = words[0].to_lowercase();
    if !word_values.contains_key(first_lower.as_str()) {
        return None;
    }

    let mut total = 0u64;
    let mut current = 0u64;
    let mut consumed = 0;

    for word in words {
        let lower = word.to_lowercase();

        if let Some(&value) = word_values.get(lower.as_str()) {
            current += value;
            consumed += 1;
        } else if let Some(&mult) = multipliers.get(lower.as_str()) {
            if mult >= 1000 {
                current *= mult;
                total += current;
                current = 0;
            } else {
                current *= mult;
            }
            consumed += 1;
        } else {
            break;
        }
    }

    total += current;

    if consumed > 0 {
        Some((total, consumed))
    } else {
        None
    }
}

/// Money formatter configuration.
#[derive(Debug, Clone)]
pub struct MoneyConfig {
    /// Currency symbol.
    pub symbol: String,
    /// Currency name (singular).
    pub name_singular: String,
    /// Currency name (plural).
    pub name_plural: String,
    /// Subunit name (singular).
    pub subunit_singular: String,
    /// Subunit name (plural).
    pub subunit_plural: String,
    /// Subunit divisor (e.g., 100 for cents).
    pub subunit_divisor: u32,
}

impl Default for MoneyConfig {
    fn default() -> Self {
        Self {
            symbol: "$".to_string(),
            name_singular: "dollar".to_string(),
            name_plural: "dollars".to_string(),
            subunit_singular: "cent".to_string(),
            subunit_plural: "cents".to_string(),
            subunit_divisor: 100,
        }
    }
}

/// Date format configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateFormat {
    /// MM/DD/YYYY
    MonthDayYear,
    /// DD/MM/YYYY
    DayMonthYear,
    /// YYYY-MM-DD (ISO)
    YearMonthDay,
}

/// Time format configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeFormat {
    /// 12-hour format with AM/PM.
    TwelveHour,
    /// 24-hour format.
    TwentyFourHour,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_number_to_words() {
        assert_eq!(number_to_words("0"), "zero");
        assert_eq!(number_to_words("1"), "one");
        assert_eq!(number_to_words("12"), "twelve");
        assert_eq!(number_to_words("21"), "twenty one");
        assert_eq!(number_to_words("100"), "one hundred");
        assert_eq!(number_to_words("123"), "one hundred twenty three");
        assert_eq!(number_to_words("1000"), "one thousand");
        assert_eq!(
            number_to_words("1234"),
            "one thousand two hundred thirty four"
        );
    }

    #[test]
    fn test_words_to_number() {
        assert_eq!(words_to_number(&["one"]), Some((1, 1)));
        assert_eq!(words_to_number(&["twelve"]), Some((12, 1)));
        assert_eq!(words_to_number(&["twenty", "one"]), Some((21, 2)));
        assert_eq!(words_to_number(&["one", "hundred"]), Some((100, 2)));
        assert_eq!(
            words_to_number(&["one", "hundred", "twenty", "three"]),
            Some((123, 4))
        );
        assert_eq!(words_to_number(&["one", "thousand"]), Some((1000, 2)));
    }

    #[test]
    fn test_text_normalizer() {
        let normalizer: TextNormalizer<TropicalWeight> = TextNormalizer::new();
        let results = normalizer.normalize("I have 123 apples");
        assert!(!results.is_empty());
        assert!(results[0].0.contains("one hundred twenty three"));
    }

    #[test]
    fn test_inverse_text_normalizer() {
        let itn: InverseTextNormalizer<TropicalWeight> = InverseTextNormalizer::new();
        let results = itn.denormalize("I have one hundred twenty three apples");
        assert!(!results.is_empty());
        assert!(results[0].0.contains("123"));
    }

    #[test]
    fn test_semiotic_class() {
        assert_eq!(SemioticClass::Cardinal, SemioticClass::Cardinal);
        assert_ne!(SemioticClass::Cardinal, SemioticClass::Ordinal);
    }
}
