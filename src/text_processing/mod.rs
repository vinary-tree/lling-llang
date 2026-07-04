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
//! - [NeMo Inverse Text Normalization: From Development to Production (arXiv 2104.05055)](https://arxiv.org/abs/2104.05055)

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};
use std::collections::HashMap;

const WHITELIST_TOKENS: &[&str] = &[
    "dr.", "mr.", "mrs.", "ms.", "prof.", "st.", "ave.", "rd.", "blvd.", "inc.", "ltd.",
];

const MEASURE_UNITS: &[&str] = &[
    "mm", "cm", "m", "km", "in", "ft", "yd", "mi", "mg", "g", "kg", "lb", "oz", "ml", "l", "hz",
    "khz", "mhz", "gb", "mb", "kb", "tb", "mph", "kmh", "c", "f",
];

const AM_PM: &[&str] = &["am", "pm", "a.m.", "p.m."];

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
        let mut fst: VectorWfst<char, W> = VectorWfst::new();
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, W::one());

        add_cardinal_decimal_time_date_paths(&mut fst, start);
        add_ordinal_paths(&mut fst, start);
        add_money_paths(&mut fst, start);
        add_electronic_paths(&mut fst, start);

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

    /// Tag non-whitespace spans with their semiotic class.
    pub fn tag(&self, input: &str) -> Vec<TaggedToken> {
        tag_text(input)
    }

    /// Number normalization.
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
        TextNormalizer::<W>::build_default_classifier()
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

    /// Tag non-whitespace spans with their semiotic class.
    pub fn tag(&self, input: &str) -> Vec<TaggedToken> {
        tag_text(input)
    }

    /// Number denormalization.
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

fn add_transition<W: Semiring>(
    fst: &mut VectorWfst<char, W>,
    from: StateId,
    input: char,
    to: StateId,
) {
    fst.add_transition(WeightedTransition {
        from,
        input: Some(input),
        output: Some(input),
        to,
        weight: W::one(),
    });
}

fn add_digit_transitions<W: Semiring>(fst: &mut VectorWfst<char, W>, from: StateId, to: StateId) {
    for digit in '0'..='9' {
        add_transition(fst, from, digit, to);
    }
}

fn add_ascii_word_transitions<W: Semiring>(
    fst: &mut VectorWfst<char, W>,
    from: StateId,
    to: StateId,
) {
    for ch in 'a'..='z' {
        add_transition(fst, from, ch, to);
    }
    for ch in 'A'..='Z' {
        add_transition(fst, from, ch, to);
    }
    for ch in '0'..='9' {
        add_transition(fst, from, ch, to);
    }
    for ch in ['.', '-', '_', '+', '/', ':'] {
        add_transition(fst, from, ch, to);
    }
}

fn add_cardinal_decimal_time_date_paths<W: Semiring>(
    fst: &mut VectorWfst<char, W>,
    start: StateId,
) {
    let number = fst.add_state();
    fst.set_final(number, W::one());
    add_digit_transitions(fst, start, number);
    add_digit_transitions(fst, number, number);

    let decimal_dot = fst.add_state();
    let decimal = fst.add_state();
    fst.set_final(decimal, W::one());
    add_transition(fst, number, '.', decimal_dot);
    add_digit_transitions(fst, decimal_dot, decimal);
    add_digit_transitions(fst, decimal, decimal);

    let colon = fst.add_state();
    let minute_first = fst.add_state();
    let minute_second = fst.add_state();
    fst.set_final(minute_second, W::one());
    add_transition(fst, number, ':', colon);
    add_digit_transitions(fst, colon, minute_first);
    add_digit_transitions(fst, minute_first, minute_second);

    let slash = fst.add_state();
    let slash_number = fst.add_state();
    let date_second_slash = fst.add_state();
    let date_year = fst.add_state();
    fst.set_final(slash_number, W::one());
    fst.set_final(date_year, W::one());
    add_transition(fst, number, '/', slash);
    add_digit_transitions(fst, slash, slash_number);
    add_digit_transitions(fst, slash_number, slash_number);
    add_transition(fst, slash_number, '/', date_second_slash);
    add_digit_transitions(fst, date_second_slash, date_year);
    add_digit_transitions(fst, date_year, date_year);

    let dash = fst.add_state();
    let dash_number = fst.add_state();
    let date_second_dash = fst.add_state();
    fst.set_final(dash_number, W::one());
    add_transition(fst, number, '-', dash);
    add_digit_transitions(fst, dash, dash_number);
    add_digit_transitions(fst, dash_number, dash_number);
    add_transition(fst, dash_number, '-', date_second_dash);
    add_digit_transitions(fst, date_second_dash, date_year);
}

fn add_ordinal_paths<W: Semiring>(fst: &mut VectorWfst<char, W>, start: StateId) {
    let number = fst.add_state();
    add_digit_transitions(fst, start, number);
    add_digit_transitions(fst, number, number);

    for (first, second) in [('s', 't'), ('n', 'd'), ('r', 'd'), ('t', 'h')] {
        let suffix_first = fst.add_state();
        let ordinal = fst.add_state();
        fst.set_final(ordinal, W::one());
        add_transition(fst, number, first, suffix_first);
        add_transition(fst, number, first.to_ascii_uppercase(), suffix_first);
        add_transition(fst, suffix_first, second, ordinal);
        add_transition(fst, suffix_first, second.to_ascii_uppercase(), ordinal);
    }
}

fn add_money_paths<W: Semiring>(fst: &mut VectorWfst<char, W>, start: StateId) {
    let prefix = fst.add_state();
    let amount = fst.add_state();
    let cents_dot = fst.add_state();
    let cents = fst.add_state();
    fst.set_final(amount, W::one());
    fst.set_final(cents, W::one());

    for symbol in ['$', '£', '€', '¥'] {
        add_transition(fst, start, symbol, prefix);
    }
    add_digit_transitions(fst, prefix, amount);
    add_digit_transitions(fst, amount, amount);
    add_transition(fst, amount, '.', cents_dot);
    add_digit_transitions(fst, cents_dot, cents);
    add_digit_transitions(fst, cents, cents);
}

fn add_electronic_paths<W: Semiring>(fst: &mut VectorWfst<char, W>, start: StateId) {
    let user = fst.add_state();
    let at = fst.add_state();
    let domain = fst.add_state();
    let dot = fst.add_state();
    let suffix = fst.add_state();
    fst.set_final(suffix, W::one());

    add_ascii_word_transitions(fst, start, user);
    add_ascii_word_transitions(fst, user, user);
    add_transition(fst, user, '@', at);
    add_ascii_word_transitions(fst, at, domain);
    add_ascii_word_transitions(fst, domain, domain);
    add_transition(fst, domain, '.', dot);
    add_ascii_word_transitions(fst, dot, suffix);
    add_ascii_word_transitions(fst, suffix, suffix);
}

fn tag_text(input: &str) -> Vec<TaggedToken> {
    let spans = lexical_spans(input);
    spans
        .iter()
        .enumerate()
        .map(|(idx, span)| {
            let class = classify_span(input, &spans, idx);
            TaggedToken {
                text: input[span.start..span.end].to_string(),
                class,
                start: span.start,
                end: span.end,
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct TextSpan {
    start: usize,
    end: usize,
}

fn lexical_spans(input: &str) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut start = None;

    for (idx, ch) in input.char_indices() {
        if ch.is_whitespace() {
            if let Some(span_start) = start.take() {
                spans.push(TextSpan {
                    start: span_start,
                    end: idx,
                });
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }

    if let Some(span_start) = start {
        spans.push(TextSpan {
            start: span_start,
            end: input.len(),
        });
    }

    spans
}

fn classify_span(input: &str, spans: &[TextSpan], idx: usize) -> SemioticClass {
    let token = strip_outer_punctuation(&input[spans[idx].start..spans[idx].end]);
    let lower = token.to_ascii_lowercase();
    let next_lower = spans
        .get(idx + 1)
        .map(|span| strip_outer_punctuation(&input[span.start..span.end]).to_ascii_lowercase());

    if token.is_empty() {
        return SemioticClass::Plain;
    }
    if WHITELIST_TOKENS.contains(&lower.as_str()) {
        return SemioticClass::Whitelist;
    }
    if is_electronic_token(&lower) {
        return SemioticClass::Electronic;
    }
    if is_money_token(token) {
        return SemioticClass::Money;
    }
    if is_time_token(token)
        || next_lower
            .as_deref()
            .is_some_and(|next| AM_PM.contains(&next))
    {
        if contains_ascii_digit(token) && token.contains(':') {
            return SemioticClass::Time;
        }
    }
    if is_date_token(token) {
        return SemioticClass::Date;
    }
    if is_telephone_token(token) {
        return SemioticClass::Telephone;
    }
    if is_ordinal_token(token) {
        return SemioticClass::Ordinal;
    }
    if is_decimal_token(token) {
        return SemioticClass::Decimal;
    }
    if is_fraction_token(token) {
        return SemioticClass::Fraction;
    }
    if is_measure_token(token, next_lower.as_deref()) {
        return SemioticClass::Measure;
    }
    if token.chars().all(|ch| ch.is_ascii_digit()) {
        return SemioticClass::Cardinal;
    }
    if is_verbatim_token(token) {
        return SemioticClass::Verbatim;
    }

    SemioticClass::Plain
}

fn strip_outer_punctuation(token: &str) -> &str {
    token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    })
}

fn contains_ascii_digit(token: &str) -> bool {
    token.chars().any(|ch| ch.is_ascii_digit())
}

fn is_electronic_token(lower: &str) -> bool {
    lower.contains('@')
        || lower.contains("://")
        || lower.starts_with("www.")
        || lower.ends_with(".com")
        || lower.ends_with(".org")
        || lower.ends_with(".net")
}

fn is_money_token(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, '$' | '£' | '€' | '¥') && is_decimal_or_integer(chars.as_str())
}

fn is_time_token(token: &str) -> bool {
    let Some((hour, minute)) = token.split_once(':') else {
        return false;
    };
    let minute = minute.trim_end_matches(|ch: char| ch.is_ascii_alphabetic() || ch == '.');
    if !hour.chars().all(|ch| ch.is_ascii_digit())
        || minute.len() != 2
        || !minute.chars().all(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    let hour = hour.parse::<u8>().ok();
    let minute = minute.parse::<u8>().ok();
    matches!((hour, minute), (Some(0..=23), Some(0..=59)))
}

fn is_date_token(token: &str) -> bool {
    for separator in ['/', '-'] {
        let parts: Vec<_> = token.split(separator).collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        {
            return true;
        }
    }
    false
}

fn is_telephone_token(token: &str) -> bool {
    let digit_count = token.chars().filter(|ch| ch.is_ascii_digit()).count();
    digit_count >= 7
        && token
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '(' | ')' | '.' | '+'))
}

fn is_ordinal_token(token: &str) -> bool {
    if token.len() < 3 {
        return false;
    }
    let (digits, suffix) = token.split_at(token.len() - 2);
    digits.chars().all(|ch| ch.is_ascii_digit())
        && matches!(
            suffix.to_ascii_lowercase().as_str(),
            "st" | "nd" | "rd" | "th"
        )
}

fn is_decimal_token(token: &str) -> bool {
    token
        .split_once('.')
        .is_some_and(|(lhs, rhs)| is_ascii_digits(lhs) && is_ascii_digits(rhs))
}

fn is_fraction_token(token: &str) -> bool {
    token
        .split_once('/')
        .is_some_and(|(lhs, rhs)| is_ascii_digits(lhs) && is_ascii_digits(rhs))
}

fn is_measure_token(token: &str, next_lower: Option<&str>) -> bool {
    let split_at = token
        .char_indices()
        .find_map(|(idx, ch)| ch.is_ascii_alphabetic().then_some(idx));
    if let Some(idx) = split_at {
        let (value, unit) = token.split_at(idx);
        return is_decimal_or_integer(value)
            && MEASURE_UNITS.contains(&unit.to_ascii_lowercase().as_str());
    }

    token.chars().all(|ch| ch.is_ascii_digit())
        && next_lower
            .map(|next| MEASURE_UNITS.contains(&next))
            .unwrap_or(false)
}

fn is_verbatim_token(token: &str) -> bool {
    token.len() <= 5
        && token.chars().any(|ch| ch.is_ascii_alphabetic())
        && token
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch == '.' || ch == '-')
}

fn is_decimal_or_integer(token: &str) -> bool {
    is_ascii_digits(token)
        || token
            .split_once('.')
            .is_some_and(|(lhs, rhs)| is_ascii_digits(lhs) && is_ascii_digits(rhs))
}

fn is_ascii_digits(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|ch| ch.is_ascii_digit())
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
    fn test_text_normalizer_classifier_has_pattern_structure() {
        let normalizer: TextNormalizer<TropicalWeight> = TextNormalizer::new();
        assert!(normalizer.classifier().num_states() > 1);
        assert!(!normalizer.classifier().transitions(0).is_empty());
    }

    #[test]
    fn test_text_normalizer_tags_common_semiotic_classes() {
        let normalizer: TextNormalizer<TropicalWeight> = TextNormalizer::new();
        let tagged = normalizer.tag("Dr. Smith paid $12.50 on 01/15/2024 at 3:30 PM for 5 km");
        let classes: Vec<_> = tagged.iter().map(|token| token.class).collect();

        assert!(classes.contains(&SemioticClass::Whitelist));
        assert!(classes.contains(&SemioticClass::Money));
        assert!(classes.contains(&SemioticClass::Date));
        assert!(classes.contains(&SemioticClass::Time));
        assert!(classes.contains(&SemioticClass::Measure));
    }

    #[test]
    fn test_text_normalizer_tags_numeric_variants() {
        let normalizer: TextNormalizer<TropicalWeight> = TextNormalizer::new();
        let tagged = normalizer.tag("Call 555-1234 after the 21st with 3.14 or 1/2");

        assert_eq!(tagged[1].class, SemioticClass::Telephone);
        assert_eq!(tagged[4].class, SemioticClass::Ordinal);
        assert_eq!(tagged[6].class, SemioticClass::Decimal);
        assert_eq!(tagged[8].class, SemioticClass::Fraction);
    }

    #[test]
    fn test_inverse_text_normalizer() {
        let itn: InverseTextNormalizer<TropicalWeight> = InverseTextNormalizer::new();
        let results = itn.denormalize("I have one hundred twenty three apples");
        assert!(!results.is_empty());
        assert!(results[0].0.contains("123"));
    }

    #[test]
    fn test_inverse_text_normalizer_reuses_classifier() {
        let itn: InverseTextNormalizer<TropicalWeight> = InverseTextNormalizer::new();
        let tagged = itn.tag("www.example.com support@example.com NASA");

        assert_eq!(tagged[0].class, SemioticClass::Electronic);
        assert_eq!(tagged[1].class, SemioticClass::Electronic);
        assert_eq!(tagged[2].class, SemioticClass::Verbatim);
    }

    #[test]
    fn test_semiotic_class() {
        assert_eq!(SemioticClass::Cardinal, SemioticClass::Cardinal);
        assert_ne!(SemioticClass::Cardinal, SemioticClass::Ordinal);
    }
}
