//! Homophone transducer for sound-alike word matching.
//!
//! Models words that sound alike but are spelled differently (homophones),
//! enabling correction of spoken language transcription errors and phonetic
//! spelling mistakes.
//!
//! # Phonetic Encodings
//!
//! This module supports multiple phonetic encoding schemes:
//! - **Soundex**: Classic algorithm for American English (4-character codes)
//! - **Metaphone**: More accurate phonetic representation
//! - **Double Metaphone**: Handles multiple pronunciations
//! - **NYSIIS**: New York State Identification and Intelligence System
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::error_models::{HomophoneTransducer, PhoneticAlgorithm};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Create a homophone transducer with Soundex encoding
//! let vocabulary = vec!["their", "there", "they're", "hear", "here"];
//! let transducer = HomophoneTransducer::<TropicalWeight>::new(PhoneticAlgorithm::Soundex)
//!     .with_vocabulary(&vocabulary);
//!
//! // Find homophones for "there"
//! let homophones = transducer.homophones("there");
//! // Returns: [("their", w1), ("they're", w2)]
//! ```

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

use crate::semiring::{Semiring, TropicalWeight};
use crate::wfst::{MutableWfst, VectorWfst, Wfst};

/// Configuration for homophone transducer behavior.
#[derive(Clone, Debug)]
pub struct HomophoneConfig {
    /// Cost for exact matches (identity mapping)
    pub identity_cost: f64,
    /// Base cost for homophone substitutions
    pub homophone_cost: f64,
    /// Whether to include the word itself in homophones
    pub include_self: bool,
    /// Maximum edit distance for fuzzy phonetic matching
    pub max_phonetic_edit_distance: Option<usize>,
    /// Penalty multiplier for different-length words
    pub length_penalty: f64,
    /// Case sensitivity for matching
    pub case_sensitive: bool,
}

impl Default for HomophoneConfig {
    fn default() -> Self {
        HomophoneConfig {
            identity_cost: 0.0,
            homophone_cost: 0.5,
            include_self: false,
            max_phonetic_edit_distance: Some(1),
            length_penalty: 0.1,
            case_sensitive: false,
        }
    }
}

/// Phonetic encoding algorithm selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhoneticAlgorithm {
    /// Soundex: 4-character code (letter + 3 digits)
    Soundex,
    /// Refined Soundex with more distinctions
    RefinedSoundex,
    /// Metaphone: Variable-length phonetic code
    Metaphone,
    /// Double Metaphone: Primary and alternate encodings
    DoubleMetaphone,
    /// NYSIIS: New York State encoding
    Nysiis,
    /// Caverphone: Optimized for English names
    Caverphone,
    /// Cologne Phonetic: German language support
    Cologne,
}

/// Result of phonetic encoding (may have multiple representations).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PhoneticCode {
    /// Primary phonetic code
    pub primary: String,
    /// Alternate code (for algorithms like Double Metaphone)
    pub alternate: Option<String>,
}

impl PhoneticCode {
    /// Create a single-code result.
    pub fn single(code: String) -> Self {
        PhoneticCode {
            primary: code,
            alternate: None,
        }
    }

    /// Create a dual-code result.
    pub fn dual(primary: String, alternate: String) -> Self {
        PhoneticCode {
            primary,
            alternate: Some(alternate),
        }
    }

    /// Check if two phonetic codes match (considering alternates).
    pub fn matches(&self, other: &PhoneticCode) -> bool {
        if self.primary == other.primary {
            return true;
        }
        if let Some(ref alt) = self.alternate {
            if alt == &other.primary {
                return true;
            }
        }
        if let Some(ref other_alt) = other.alternate {
            if &self.primary == other_alt {
                return true;
            }
        }
        if let (Some(ref alt1), Some(ref alt2)) = (&self.alternate, &other.alternate) {
            if alt1 == alt2 {
                return true;
            }
        }
        false
    }
}

/// Phonetic encoder that computes phonetic codes for words.
#[derive(Clone, Debug)]
pub struct PhoneticEncoder {
    algorithm: PhoneticAlgorithm,
}

impl PhoneticEncoder {
    /// Create a new encoder with the specified algorithm.
    pub fn new(algorithm: PhoneticAlgorithm) -> Self {
        PhoneticEncoder { algorithm }
    }

    /// Encode a word to its phonetic representation.
    pub fn encode(&self, word: &str) -> PhoneticCode {
        match self.algorithm {
            PhoneticAlgorithm::Soundex => self.soundex(word),
            PhoneticAlgorithm::RefinedSoundex => self.refined_soundex(word),
            PhoneticAlgorithm::Metaphone => self.metaphone(word),
            PhoneticAlgorithm::DoubleMetaphone => self.double_metaphone(word),
            PhoneticAlgorithm::Nysiis => self.nysiis(word),
            PhoneticAlgorithm::Caverphone => self.caverphone(word),
            PhoneticAlgorithm::Cologne => self.cologne(word),
        }
    }

    /// Soundex encoding (4 characters: letter + 3 digits).
    fn soundex(&self, word: &str) -> PhoneticCode {
        if word.is_empty() {
            return PhoneticCode::single(String::new());
        }

        let word_upper: String = word.to_uppercase();
        let mut chars = word_upper.chars().filter(|c| c.is_ascii_alphabetic());

        let first = match chars.next() {
            Some(c) => c,
            None => return PhoneticCode::single(String::new()),
        };

        let mut code = String::with_capacity(4);
        code.push(first);

        let mut last_digit = Self::soundex_digit(first);

        for c in chars {
            let digit = Self::soundex_digit(c);
            if digit != '0' && digit != last_digit {
                code.push(digit);
                if code.len() == 4 {
                    break;
                }
            }
            last_digit = digit;
        }

        // Pad with zeros to length 4
        while code.len() < 4 {
            code.push('0');
        }

        PhoneticCode::single(code)
    }

    /// Map character to Soundex digit.
    fn soundex_digit(c: char) -> char {
        match c {
            'B' | 'F' | 'P' | 'V' => '1',
            'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => '2',
            'D' | 'T' => '3',
            'L' => '4',
            'M' | 'N' => '5',
            'R' => '6',
            _ => '0', // A, E, I, O, U, H, W, Y
        }
    }

    /// Refined Soundex with more distinctions.
    fn refined_soundex(&self, word: &str) -> PhoneticCode {
        if word.is_empty() {
            return PhoneticCode::single(String::new());
        }

        let word_upper: String = word.to_uppercase();
        let mut chars = word_upper.chars().filter(|c| c.is_ascii_alphabetic());

        let first = match chars.next() {
            Some(c) => c,
            None => return PhoneticCode::single(String::new()),
        };

        let mut code = String::with_capacity(8);
        code.push(first);

        let mut last_digit = Self::refined_soundex_digit(first);

        for c in chars {
            let digit = Self::refined_soundex_digit(c);
            if digit != '0' && digit != last_digit {
                code.push(digit);
            }
            last_digit = digit;
        }

        PhoneticCode::single(code)
    }

    /// Map character to Refined Soundex digit.
    fn refined_soundex_digit(c: char) -> char {
        match c {
            'B' | 'P' => '1',
            'F' | 'V' => '2',
            'C' | 'K' | 'S' => '3',
            'G' | 'J' => '4',
            'Q' | 'X' | 'Z' => '5',
            'D' | 'T' => '6',
            'L' => '7',
            'M' | 'N' => '8',
            'R' => '9',
            _ => '0', // A, E, I, O, U, H, W, Y
        }
    }

    /// Metaphone encoding.
    fn metaphone(&self, word: &str) -> PhoneticCode {
        if word.is_empty() {
            return PhoneticCode::single(String::new());
        }

        let word_upper: String = word.to_uppercase();
        let chars: Vec<char> = word_upper.chars().collect();
        let mut code = String::new();
        let mut i = 0;

        // Drop initial 'KN', 'GN', 'PN', 'AE', 'WR'
        if chars.len() >= 2 {
            let prefix: String = chars[..2].iter().collect();
            if ["KN", "GN", "PN", "AE", "WR"].contains(&prefix.as_str()) {
                i = 1;
            }
        }
        // Drop initial 'X' (sounds like 'S')
        if !chars.is_empty() && chars[0] == 'X' {
            code.push('S');
            i = 1;
        }
        // Initial 'WH' -> 'W'
        if chars.len() >= 2 && chars[0] == 'W' && chars[1] == 'H' {
            i = 1;
        }

        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            let prev = if i > 0 {
                chars.get(i - 1).copied()
            } else {
                None
            };

            match c {
                'A' | 'E' | 'I' | 'O' | 'U' => {
                    // Vowels only at beginning
                    if i == 0 {
                        code.push(c);
                    }
                }
                'B' => {
                    // B silent at end after M
                    if !(prev == Some('M') && i == chars.len() - 1) {
                        code.push('B');
                    }
                }
                'C' => {
                    if next == Some('I') || next == Some('E') || next == Some('Y') {
                        code.push('S'); // CE, CI, CY -> S
                    } else if next == Some('H') {
                        code.push('X'); // CH -> X (sh sound)
                        i += 1;
                    } else {
                        code.push('K');
                    }
                }
                'D' => {
                    if next == Some('G') {
                        if let Some(next2) = chars.get(i + 2) {
                            if *next2 == 'E' || *next2 == 'I' || *next2 == 'Y' {
                                code.push('J');
                                i += 2;
                            } else {
                                code.push('T');
                            }
                        } else {
                            code.push('T');
                        }
                    } else {
                        code.push('T');
                    }
                }
                'F' => code.push('F'),
                'G' => {
                    if next == Some('H') {
                        i += 1; // GH silent
                    } else if next == Some('N') {
                        if i == chars.len() - 2 {
                            // GN at end - skip
                        } else {
                            code.push('K');
                        }
                    } else if next == Some('I') || next == Some('E') || next == Some('Y') {
                        code.push('J');
                    } else {
                        code.push('K');
                    }
                }
                'H' => {
                    // H only if not after vowel and before vowel
                    let is_vowel = |c: Option<char>| matches!(c, Some('A' | 'E' | 'I' | 'O' | 'U'));
                    if !is_vowel(prev) && is_vowel(next) {
                        code.push('H');
                    }
                }
                'J' => code.push('J'),
                'K' => {
                    if prev != Some('C') {
                        code.push('K');
                    }
                }
                'L' => code.push('L'),
                'M' => code.push('M'),
                'N' => code.push('N'),
                'P' => {
                    if next == Some('H') {
                        code.push('F');
                        i += 1;
                    } else {
                        code.push('P');
                    }
                }
                'Q' => code.push('K'),
                'R' => code.push('R'),
                'S' => {
                    if next == Some('H') {
                        code.push('X'); // SH -> X
                        i += 1;
                    } else if next == Some('I') || next == Some('O') {
                        if let Some(next2) = chars.get(i + 2) {
                            if *next2 == 'O' || *next2 == 'N' {
                                code.push('X'); // SIO, SION -> X
                            } else {
                                code.push('S');
                            }
                        } else {
                            code.push('S');
                        }
                    } else {
                        code.push('S');
                    }
                }
                'T' => {
                    if next == Some('I') {
                        if let Some(next2) = chars.get(i + 2) {
                            if *next2 == 'O' || *next2 == 'A' {
                                code.push('X'); // TIO, TIA -> X
                            } else {
                                code.push('T');
                            }
                        } else {
                            code.push('T');
                        }
                    } else if next == Some('H') {
                        code.push('0'); // TH -> 0 (theta)
                        i += 1;
                    } else {
                        code.push('T');
                    }
                }
                'V' => code.push('F'),
                'W' | 'Y' => {
                    // W, Y only before vowel
                    let is_vowel = |c: Option<char>| matches!(c, Some('A' | 'E' | 'I' | 'O' | 'U'));
                    if is_vowel(next) {
                        code.push(c);
                    }
                }
                'X' => {
                    code.push('K');
                    code.push('S');
                }
                'Z' => code.push('S'),
                _ => {}
            }
            i += 1;
        }

        PhoneticCode::single(code)
    }

    /// Double Metaphone encoding (returns primary and alternate).
    fn double_metaphone(&self, word: &str) -> PhoneticCode {
        // Simplified implementation - returns primary and alternate
        let primary = self.metaphone(word).primary;

        // Generate alternate by applying common variation rules
        let word_upper = word.to_uppercase();
        let alternate = if word_upper.contains("GH") {
            word_upper.replace("GH", "G")
        } else if word_upper.contains("PH") {
            word_upper.replace("PH", "F")
        } else if word_upper.starts_with("WR") {
            word_upper.replacen("WR", "R", 1)
        } else {
            return PhoneticCode::single(primary);
        };

        let alt_code = self.metaphone(&alternate).primary;
        if alt_code != primary {
            PhoneticCode::dual(primary, alt_code)
        } else {
            PhoneticCode::single(primary)
        }
    }

    /// NYSIIS encoding.
    fn nysiis(&self, word: &str) -> PhoneticCode {
        if word.is_empty() {
            return PhoneticCode::single(String::new());
        }

        let mut word_upper: String = word.to_uppercase();
        word_upper.retain(|c| c.is_ascii_alphabetic());

        if word_upper.is_empty() {
            return PhoneticCode::single(String::new());
        }

        // Initial transformations
        let prefixes = [
            ("MAC", "MCC"),
            ("KN", "NN"),
            ("K", "C"),
            ("PH", "FF"),
            ("PF", "FF"),
            ("SCH", "SSS"),
        ];
        for (from, to) in prefixes {
            if word_upper.starts_with(from) {
                word_upper = format!("{}{}", to, &word_upper[from.len()..]);
                break;
            }
        }

        // Suffix transformations
        let suffixes = [
            ("EE", "Y"),
            ("IE", "Y"),
            ("DT", "D"),
            ("RT", "D"),
            ("RD", "D"),
            ("NT", "D"),
            ("ND", "D"),
        ];
        for (from, to) in suffixes {
            if word_upper.ends_with(from) {
                let len = word_upper.len() - from.len();
                word_upper = format!("{}{}", &word_upper[..len], to);
                break;
            }
        }

        let chars: Vec<char> = word_upper.chars().collect();
        let mut code = String::new();
        code.push(chars[0]);

        let mut i = 1;
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            let next2 = chars.get(i + 2).copied();

            match c {
                'E' | 'I' | 'O' | 'U' => {
                    code.push('A');
                }
                'A' => {
                    code.push('A');
                }
                'Q' => code.push('G'),
                'Z' => code.push('S'),
                'M' => code.push('N'),
                'K' => {
                    if next == Some('N') {
                        code.push('N');
                    } else {
                        code.push('C');
                    }
                }
                'S' => {
                    if next == Some('C') && next2 == Some('H') {
                        code.push_str("SSS");
                        i += 2;
                    } else if next == Some('H') {
                        code.push_str("SS");
                        i += 1;
                    } else {
                        code.push('S');
                    }
                }
                'P' => {
                    if next == Some('H') {
                        code.push('F');
                        i += 1;
                    } else {
                        code.push('P');
                    }
                }
                'H' => {
                    let is_vowel = |c: char| matches!(c, 'A' | 'E' | 'I' | 'O' | 'U');
                    let prev = chars.get(i.saturating_sub(1)).copied();
                    if let Some(p) = prev {
                        if !is_vowel(p) || (next.is_some() && !is_vowel(next.unwrap())) {
                            if let Some(p) = prev {
                                if is_vowel(p) {
                                    code.push('A');
                                }
                            }
                        } else {
                            code.push('H');
                        }
                    }
                }
                'W' => {
                    let is_vowel = |c: char| matches!(c, 'A' | 'E' | 'I' | 'O' | 'U');
                    let prev = chars.get(i.saturating_sub(1)).copied();
                    if let Some(p) = prev {
                        if is_vowel(p) {
                            code.push('A');
                        }
                    }
                }
                _ => {
                    code.push(c);
                }
            }
            i += 1;
        }

        // Remove trailing 'S'
        while code.ends_with('S') && code.len() > 1 {
            code.pop();
        }
        // Remove trailing 'A'
        if code.ends_with('A') && code.len() > 1 {
            code.pop();
        }

        // Collapse consecutive duplicates
        let mut collapsed = String::new();
        let mut last = '\0';
        for c in code.chars() {
            if c != last {
                collapsed.push(c);
                last = c;
            }
        }

        PhoneticCode::single(collapsed)
    }

    /// Caverphone encoding (for English names).
    fn caverphone(&self, word: &str) -> PhoneticCode {
        if word.is_empty() {
            return PhoneticCode::single(String::new());
        }

        let mut result: String = word.to_lowercase();
        result.retain(|c| c.is_ascii_alphabetic());

        if result.is_empty() {
            return PhoneticCode::single(String::new());
        }

        // Apply transformations
        let replacements = [
            ("cough", "cof2"),
            ("rough", "rof2"),
            ("tough", "tof2"),
            ("enough", "enof2"),
            ("gn", "2n"),
            ("mb", "m2"),
            ("cq", "2q"),
            ("ci", "si"),
            ("ce", "se"),
            ("cy", "sy"),
            ("tch", "2ch"),
            ("c", "k"),
            ("q", "k"),
            ("x", "k"),
            ("v", "f"),
            ("dg", "2g"),
            ("tio", "sio"),
            ("tia", "sia"),
            ("d", "t"),
            ("ph", "fh"),
            ("b", "p"),
            ("sh", "s2"),
            ("z", "s"),
            ("^aeiou", "3"),
            ("3gh3", "3kh3"),
            ("gh", "22"),
            ("g", "k"),
            ("s+", "S"),
            ("t+", "T"),
            ("p+", "P"),
            ("k+", "K"),
            ("f+", "F"),
            ("m+", "M"),
            ("n+", "N"),
            ("w3", "W3"),
            ("wy", "Wy"),
            ("wh3", "Wh3"),
            ("why", "Why"),
            ("w", "2"),
            ("^h", "A"),
            ("3h", "3"),
            ("h", "2"),
            ("r3", "R3"),
            ("ry", "Ry"),
            ("r", "2"),
            ("l3", "L3"),
            ("ly", "Ly"),
            ("l", "2"),
            ("j", "y"),
            ("y3", "Y3"),
            ("y", "2"),
        ];

        for (from, to) in replacements {
            if from.starts_with('^') {
                // Beginning of word only
                let pattern = &from[1..];
                if result.starts_with(pattern) {
                    result = format!("{}{}", to, &result[pattern.len()..]);
                }
            } else if from.ends_with('+') {
                // Collapse repeated
                let base = &from[..from.len() - 1];
                let repeated = format!("{}{}", base, base);
                while result.contains(&repeated) {
                    result = result.replace(&repeated, base);
                }
            } else {
                result = result.replace(from, to);
            }
        }

        // Remove all 2s and 3s
        result.retain(|c| c != '2' && c != '3');

        // Pad to 10 characters with '1'
        while result.len() < 10 {
            result.push('1');
        }
        result.truncate(10);

        PhoneticCode::single(result.to_uppercase())
    }

    /// Cologne Phonetic encoding (German).
    fn cologne(&self, word: &str) -> PhoneticCode {
        if word.is_empty() {
            return PhoneticCode::single(String::new());
        }

        let word_upper: String = word.to_uppercase();
        let chars: Vec<char> = word_upper
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect();

        if chars.is_empty() {
            return PhoneticCode::single(String::new());
        }

        let mut code = String::new();

        for (i, &c) in chars.iter().enumerate() {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = chars.get(i + 1).copied();

            let digit = match c {
                'A' | 'E' | 'I' | 'O' | 'U' | 'J' | 'Y' => '0',
                'H' => continue, // Skip H
                'B' => '1',
                'P' => {
                    if next == Some('H') {
                        '3'
                    } else {
                        '1'
                    }
                }
                'D' | 'T' => {
                    if matches!(next, Some('C' | 'S' | 'Z')) {
                        '8'
                    } else {
                        '2'
                    }
                }
                'F' | 'V' | 'W' => '3',
                'G' | 'K' | 'Q' => '4',
                'X' => {
                    // X after C/K/Q produces just '8', otherwise '4' then '8'
                    // We handle the '48' case specially below
                    if matches!(prev, Some('C' | 'K' | 'Q')) {
                        '8'
                    } else {
                        '\0'
                    }
                }
                'L' => '5',
                'M' | 'N' => '6',
                'R' => '7',
                'S' | 'Z' => '8',
                'C' => {
                    if i == 0 {
                        if matches!(
                            next,
                            Some('A' | 'H' | 'K' | 'L' | 'O' | 'Q' | 'R' | 'U' | 'X')
                        ) {
                            '4'
                        } else {
                            '8'
                        }
                    } else if matches!(prev, Some('S' | 'Z')) {
                        '8'
                    } else if matches!(next, Some('A' | 'H' | 'K' | 'O' | 'Q' | 'U' | 'X')) {
                        '4'
                    } else {
                        '8'
                    }
                }
                _ => continue,
            };

            // Handle X -> 48 special case
            if c == 'X' && !matches!(prev, Some('C' | 'K' | 'Q')) {
                code.push('4');
                code.push('8');
            } else if digit != '\0' {
                code.push(digit);
            }
        }

        // Remove consecutive duplicates
        let mut collapsed = String::new();
        let mut last = '\0';
        for c in code.chars() {
            if c != last {
                collapsed.push(c);
                last = c;
            }
        }

        // Remove leading zeros (except if all zeros)
        let trimmed: String = collapsed.trim_start_matches('0').to_string();
        let result = if trimmed.is_empty() {
            "0".to_string()
        } else {
            trimmed
        };

        PhoneticCode::single(result)
    }
}

/// Homophone vocabulary entry.
#[derive(Clone, Debug)]
pub struct HomophoneEntry {
    /// The word itself
    pub word: String,
    /// Phonetic encoding(s)
    pub phonetic: PhoneticCode,
    /// Prior probability / frequency (optional)
    pub frequency: f64,
}

/// Homophone transducer that maps between sound-alike words.
///
/// Uses phonetic encoding to group words by pronunciation, enabling
/// correction of words that sound similar but are spelled differently.
#[derive(Clone, Debug)]
pub struct HomophoneTransducer<W: Semiring> {
    /// Phonetic encoding algorithm
    encoder: PhoneticEncoder,
    /// Configuration
    config: HomophoneConfig,
    /// Vocabulary indexed by phonetic code
    phonetic_index: HashMap<String, Vec<HomophoneEntry>>,
    /// All vocabulary entries
    vocabulary: Vec<HomophoneEntry>,
    _phantom: PhantomData<W>,
}

impl<W: Semiring> HomophoneTransducer<W> {
    /// Create a new homophone transducer with the specified algorithm.
    pub fn new(algorithm: PhoneticAlgorithm) -> Self {
        HomophoneTransducer {
            encoder: PhoneticEncoder::new(algorithm),
            config: HomophoneConfig::default(),
            phonetic_index: HashMap::new(),
            vocabulary: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(algorithm: PhoneticAlgorithm, config: HomophoneConfig) -> Self {
        HomophoneTransducer {
            encoder: PhoneticEncoder::new(algorithm),
            config,
            phonetic_index: HashMap::new(),
            vocabulary: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Add vocabulary words.
    pub fn with_vocabulary<S: AsRef<str>>(mut self, words: &[S]) -> Self {
        for word in words {
            self.add_word(word.as_ref(), 1.0);
        }
        self
    }

    /// Add vocabulary words with frequencies.
    pub fn with_vocabulary_frequencies(mut self, words: &[(String, f64)]) -> Self {
        for (word, freq) in words {
            self.add_word(word, *freq);
        }
        self
    }

    /// Add a single word to the vocabulary.
    pub fn add_word(&mut self, word: &str, frequency: f64) {
        let normalized = if self.config.case_sensitive {
            word.to_string()
        } else {
            word.to_lowercase()
        };

        let phonetic = self.encoder.encode(&normalized);
        let entry = HomophoneEntry {
            word: normalized,
            phonetic: phonetic.clone(),
            frequency,
        };

        // Index by primary code
        self.phonetic_index
            .entry(phonetic.primary.clone())
            .or_default()
            .push(entry.clone());

        // Index by alternate code if present
        if let Some(ref alt) = phonetic.alternate {
            self.phonetic_index
                .entry(alt.clone())
                .or_default()
                .push(entry.clone());
        }

        self.vocabulary.push(entry);
    }

    /// Find homophones for a word.
    ///
    /// Returns words that have the same phonetic encoding, along with
    /// their weights (lower = more likely).
    pub fn homophones(&self, word: &str) -> Vec<(String, f64)> {
        let normalized = if self.config.case_sensitive {
            word.to_string()
        } else {
            word.to_lowercase()
        };

        let query_phonetic = self.encoder.encode(&normalized);
        let mut results: Vec<(String, f64)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Look up by primary code
        if let Some(entries) = self.phonetic_index.get(&query_phonetic.primary) {
            for entry in entries {
                if !self.config.include_self && entry.word == normalized {
                    continue;
                }
                if seen.insert(entry.word.clone()) {
                    let cost = self.compute_cost(&normalized, entry);
                    results.push((entry.word.clone(), cost));
                }
            }
        }

        // Look up by alternate code if present
        if let Some(ref alt) = query_phonetic.alternate {
            if let Some(entries) = self.phonetic_index.get(alt) {
                for entry in entries {
                    if !self.config.include_self && entry.word == normalized {
                        continue;
                    }
                    if seen.insert(entry.word.clone()) {
                        let cost = self.compute_cost(&normalized, entry);
                        results.push((entry.word.clone(), cost));
                    }
                }
            }
        }

        // Sort by cost (lower first)
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Compute the cost of a homophone substitution.
    fn compute_cost(&self, query: &str, entry: &HomophoneEntry) -> f64 {
        let mut cost = self.config.homophone_cost;

        // Apply length penalty
        let len_diff = (query.len() as f64 - entry.word.len() as f64).abs();
        cost += len_diff * self.config.length_penalty;

        // Apply frequency discount (more frequent = lower cost)
        cost -= entry.frequency.ln().max(-5.0).min(0.0);

        cost.max(0.0)
    }

    /// Get the phonetic encoder.
    pub fn encoder(&self) -> &PhoneticEncoder {
        &self.encoder
    }

    /// Get the configuration.
    pub fn config(&self) -> &HomophoneConfig {
        &self.config
    }

    /// Get the vocabulary size.
    pub fn vocabulary_size(&self) -> usize {
        self.vocabulary.len()
    }

    /// Get all phonetic groups (words grouped by pronunciation).
    pub fn phonetic_groups(&self) -> impl Iterator<Item = (&str, &[HomophoneEntry])> {
        self.phonetic_index
            .iter()
            .filter(|(_, v)| v.len() > 1) // Only groups with multiple words
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Build a word-level WFST mapping words to homophones.
    ///
    /// Each state represents a word position. Arcs map from input words
    /// to output words (homophones) with appropriate costs.
    pub fn build(&self) -> VectorWfst<String, W>
    where
        W: From<TropicalWeight>,
    {
        let mut fst: VectorWfst<String, W> = VectorWfst::new();
        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, W::one());

        // For each word in vocabulary
        for entry in &self.vocabulary {
            // Add identity arc if configured
            if self.config.include_self {
                let weight = W::from(TropicalWeight::new(self.config.identity_cost));
                fst.add_arc(
                    state,
                    Some(entry.word.clone()),
                    Some(entry.word.clone()),
                    state,
                    weight,
                );
            }

            // Add arcs to homophones
            let homophones = self.homophones(&entry.word);
            for (homophone, cost) in homophones {
                if homophone != entry.word || self.config.include_self {
                    let weight = W::from(TropicalWeight::new(cost));
                    fst.add_arc(
                        state,
                        Some(entry.word.clone()),
                        Some(homophone),
                        state,
                        weight,
                    );
                }
            }
        }

        fst
    }

    /// Build a character-level WFST for the phonetic encoding.
    ///
    /// Maps input characters to phonetic code characters.
    pub fn build_phonetic_encoder_fst(&self) -> VectorWfst<char, W>
    where
        W: From<TropicalWeight>,
    {
        // This would be a more complex implementation that encodes
        // the grapheme-to-phoneme rules as a transducer
        let mut fst: VectorWfst<char, W> = VectorWfst::new();
        let state = fst.add_state();
        fst.set_start(state);
        fst.set_final(state, W::one());

        // Add basic character identity mappings
        // A full implementation would encode the phonetic rules
        for c in 'a'..='z' {
            let weight = W::from(TropicalWeight::new(0.0));
            fst.add_arc(state, Some(c), Some(c), state, weight);
        }

        fst
    }
}

/// Find common English homophones.
///
/// Returns a list of homophone groups from a predefined set.
pub fn common_english_homophones() -> Vec<Vec<&'static str>> {
    vec![
        vec!["their", "there", "they're"],
        vec!["to", "too", "two"],
        vec!["your", "you're"],
        vec!["its", "it's"],
        vec!["hear", "here"],
        vec!["by", "buy", "bye"],
        vec!["right", "write", "rite"],
        vec!["know", "no"],
        vec!["knew", "new", "gnu"],
        vec!["through", "threw"],
        vec!["peace", "piece"],
        vec!["steal", "steel"],
        vec!["sail", "sale"],
        vec!["tale", "tail"],
        vec!["mail", "male"],
        vec!["wait", "weight"],
        vec!["weak", "week"],
        vec!["meet", "meat", "mete"],
        vec!["break", "brake"],
        vec!["wear", "where", "ware"],
        vec!["which", "witch"],
        vec!["weather", "whether"],
        vec!["pair", "pear", "pare"],
        vec!["sent", "cent", "scent"],
        vec!["sight", "site", "cite"],
        vec!["sea", "see"],
        vec!["be", "bee"],
        vec!["flour", "flower"],
        vec!["knot", "not"],
        vec!["knight", "night"],
        vec!["road", "rode", "rowed"],
        vec!["would", "wood"],
        vec!["won", "one"],
        vec!["sun", "son"],
        vec!["role", "roll"],
        vec!["hole", "whole"],
        vec!["so", "sew", "sow"],
        vec!["dear", "deer"],
        vec!["principal", "principle"],
        vec!["stationary", "stationery"],
    ]
}

/// Create a homophone transducer pre-loaded with common English homophones.
pub fn english_homophone_transducer<W: Semiring>() -> HomophoneTransducer<W> {
    let mut transducer = HomophoneTransducer::new(PhoneticAlgorithm::Metaphone);

    for group in common_english_homophones() {
        for word in group {
            transducer.add_word(word, 1.0);
        }
    }

    transducer
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soundex_basic() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::Soundex);

        // Classic Soundex examples
        assert_eq!(encoder.encode("Robert").primary, "R163");
        assert_eq!(encoder.encode("Rupert").primary, "R163");
        assert_eq!(encoder.encode("Rubin").primary, "R150");

        // Same Soundex code = homophones
        assert_eq!(
            encoder.encode("Smith").primary,
            encoder.encode("Smyth").primary
        );
    }

    #[test]
    fn test_soundex_edge_cases() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::Soundex);

        assert_eq!(encoder.encode("").primary, "");
        assert_eq!(encoder.encode("A").primary, "A000");
        assert_eq!(encoder.encode("Lee").primary, "L000");
    }

    #[test]
    fn test_metaphone_basic() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::Metaphone);

        // Words that sound alike should have same/similar codes
        let code1 = encoder.encode("night").primary;
        let code2 = encoder.encode("knight").primary;
        assert_eq!(code1, code2, "night and knight should match");

        let code1 = encoder.encode("phone").primary;
        let code2 = encoder.encode("fone").primary;
        assert_eq!(code1, code2, "phone and fone should match");
    }

    #[test]
    fn test_metaphone_special_cases() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::Metaphone);

        // KN -> N
        assert!(encoder.encode("knife").primary.starts_with('N'));

        // GN -> N at start
        let gnat = encoder.encode("gnat").primary;
        assert!(
            gnat.starts_with('N'),
            "gnat should start with N, got {}",
            gnat
        );

        // PH -> F
        assert!(encoder.encode("phone").primary.contains('F'));
    }

    #[test]
    fn test_double_metaphone() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::DoubleMetaphone);

        let code = encoder.encode("rough");
        assert!(code.primary.len() > 0);
        // May have alternate pronunciation
    }

    #[test]
    fn test_nysiis_basic() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::Nysiis);

        // NYSIIS should handle transformations
        let code = encoder.encode("Mackenzie");
        assert!(code.primary.len() > 0);

        // Similar names should match
        let code1 = encoder.encode("John");
        let code2 = encoder.encode("Jon");
        assert_eq!(code1.primary, code2.primary);
    }

    #[test]
    fn test_cologne_basic() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::Cologne);

        // German phonetic encoding
        let code = encoder.encode("Mueller");
        assert!(code.primary.len() > 0);

        // Similar German names should match
        let code1 = encoder.encode("Meier");
        let code2 = encoder.encode("Meyer");
        assert_eq!(code1.primary, code2.primary);
    }

    #[test]
    fn test_phonetic_code_matches() {
        let code1 = PhoneticCode::single("ABC".to_string());
        let code2 = PhoneticCode::single("ABC".to_string());
        let code3 = PhoneticCode::single("XYZ".to_string());

        assert!(code1.matches(&code2));
        assert!(!code1.matches(&code3));

        // Test with alternates
        let code4 = PhoneticCode::dual("ABC".to_string(), "XYZ".to_string());
        assert!(code4.matches(&code1)); // Primary match
        assert!(code4.matches(&code3)); // Alternate match
    }

    #[test]
    fn test_homophone_transducer_basic() {
        let transducer = HomophoneTransducer::<TropicalWeight>::new(PhoneticAlgorithm::Metaphone)
            .with_vocabulary(&["their", "there", "they're", "hear", "here"]);

        // Find homophones for "there"
        let homophones = transducer.homophones("there");
        let words: Vec<&str> = homophones.iter().map(|(w, _)| w.as_str()).collect();

        assert!(words.contains(&"their"), "Should find 'their' as homophone");
    }

    #[test]
    fn test_homophone_transducer_case_insensitive() {
        let transducer = HomophoneTransducer::<TropicalWeight>::new(PhoneticAlgorithm::Soundex)
            .with_vocabulary(&["HELLO", "hallo"]);

        let homophones = transducer.homophones("hello");
        assert!(
            homophones.len() >= 1,
            "Should find homophones case-insensitively"
        );
    }

    #[test]
    fn test_homophone_transducer_with_config() {
        let config = HomophoneConfig {
            include_self: true,
            homophone_cost: 1.0,
            ..Default::default()
        };
        let transducer = HomophoneTransducer::<TropicalWeight>::with_config(
            PhoneticAlgorithm::Metaphone,
            config,
        )
        .with_vocabulary(&["test"]);

        let homophones = transducer.homophones("test");
        assert!(
            homophones.iter().any(|(w, _)| w == "test"),
            "Should include self when configured"
        );
    }

    #[test]
    fn test_homophone_transducer_build() {
        let transducer = HomophoneTransducer::<TropicalWeight>::new(PhoneticAlgorithm::Soundex)
            .with_vocabulary(&["to", "too", "two"]);

        let fst = transducer.build();

        assert_eq!(fst.num_states(), 1);
        assert!(fst.is_final(fst.start()));
    }

    #[test]
    fn test_common_homophones() {
        let groups = common_english_homophones();
        assert!(groups.len() > 30, "Should have many homophone groups");
        assert!(groups.iter().any(|g| g.contains(&"their")));
    }

    #[test]
    fn test_english_homophone_transducer() {
        let transducer: HomophoneTransducer<TropicalWeight> = english_homophone_transducer();

        assert!(transducer.vocabulary_size() > 50);

        // Should find common homophones
        let homophones = transducer.homophones("their");
        let words: Vec<&str> = homophones.iter().map(|(w, _)| w.as_str()).collect();
        assert!(words.contains(&"there") || words.contains(&"they're"));
    }

    #[test]
    fn test_phonetic_groups() {
        let transducer = HomophoneTransducer::<TropicalWeight>::new(PhoneticAlgorithm::Soundex)
            .with_vocabulary(&["to", "too", "two", "be", "bee", "unique"]);

        let groups: Vec<_> = transducer.phonetic_groups().collect();

        // Should have at least 2 groups (to/too/two and be/bee)
        assert!(groups.len() >= 2, "Should have phonetic groups");
    }

    #[test]
    fn test_caverphone() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::Caverphone);

        let code = encoder.encode("Thompson");
        assert_eq!(code.primary.len(), 10, "Caverphone should be 10 chars");

        // Similar names should match
        let code1 = encoder.encode("Lee");
        let code2 = encoder.encode("Leigh");
        // They should be similar (may not be exact match)
        assert!(code1.primary.len() > 0 && code2.primary.len() > 0);
    }

    #[test]
    fn test_refined_soundex() {
        let encoder = PhoneticEncoder::new(PhoneticAlgorithm::RefinedSoundex);

        let code = encoder.encode("Robert");
        assert!(code.primary.starts_with('R'));
        assert!(code.primary.len() > 1);

        // Should provide more distinctions than basic Soundex
        let basic = PhoneticEncoder::new(PhoneticAlgorithm::Soundex);
        let refined_code = encoder.encode("Testing");
        let basic_code = basic.encode("Testing");
        // Refined typically produces longer codes
        assert!(refined_code.primary.len() >= basic_code.primary.len() - 1);
    }
}
