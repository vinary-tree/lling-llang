//! Constrained LLM Decoding.
//!
//! This module provides WFST/FSM-based constraint enforcement for Large Language Model outputs.
//! Grammar-constrained decoding (GCD) ensures LLM outputs match specified rules.
//!
//! ## Key Techniques
//!
//! 1. **CFG → PDA → Token Masking**: Compile grammar to pushdown automaton
//! 2. **Compressed FSM**: Batch multiple token transitions
//! 3. **Lookahead Tables**: Precompute valid continuations
//!
//! ## References
//!
//! - [Flexible Grammar-Constrained Decoding (arXiv 2502.05111)](https://arxiv.org/abs/2502.05111)
//! - [Grammar-Constrained Decoding for Logical Parsing (ACL 2025)](https://aclanthology.org/2025.acl-industry.34/)
//! - [vLLM Structured Decoding](https://blog.vllm.ai/2025/01/14/struct-decode-intro.html)

use crate::semiring::{Semiring, TropicalWeight};
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};
use std::collections::{HashMap, HashSet};

/// Token ID type for LLM vocabulary.
pub type TokenId = u32;

/// Bit vector for efficient token masking.
#[derive(Debug, Clone)]
pub struct TokenMask {
    /// Bits indicating which tokens are valid.
    bits: Vec<u64>,
    /// Vocabulary size.
    vocab_size: usize,
}

impl TokenMask {
    /// Create a new token mask with all tokens invalid.
    pub fn new(vocab_size: usize) -> Self {
        let num_words = (vocab_size + 63) / 64;
        Self {
            bits: vec![0; num_words],
            vocab_size,
        }
    }

    /// Create a mask with all tokens valid.
    pub fn all_valid(vocab_size: usize) -> Self {
        let num_words = (vocab_size + 63) / 64;
        let mut bits = vec![u64::MAX; num_words];
        // Clear bits beyond vocab_size
        let remaining = vocab_size % 64;
        if remaining > 0 && !bits.is_empty() {
            bits[num_words - 1] = (1u64 << remaining) - 1;
        }
        Self { bits, vocab_size }
    }

    /// Set a token as valid.
    #[inline]
    pub fn set(&mut self, token_id: TokenId) {
        let idx = token_id as usize;
        if idx < self.vocab_size {
            self.bits[idx / 64] |= 1u64 << (idx % 64);
        }
    }

    /// Set a token as invalid.
    #[inline]
    pub fn unset(&mut self, token_id: TokenId) {
        let idx = token_id as usize;
        if idx < self.vocab_size {
            self.bits[idx / 64] &= !(1u64 << (idx % 64));
        }
    }

    /// Check if a token is valid.
    #[inline]
    pub fn is_valid(&self, token_id: TokenId) -> bool {
        let idx = token_id as usize;
        if idx >= self.vocab_size {
            return false;
        }
        (self.bits[idx / 64] & (1u64 << (idx % 64))) != 0
    }

    /// Count number of valid tokens.
    pub fn count_valid(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Get an iterator over valid token IDs.
    pub fn iter_valid(&self) -> impl Iterator<Item = TokenId> + '_ {
        (0..self.vocab_size as TokenId).filter(move |&t| self.is_valid(t))
    }

    /// Union with another mask (OR).
    pub fn union(&mut self, other: &TokenMask) {
        for (a, b) in self.bits.iter_mut().zip(other.bits.iter()) {
            *a |= *b;
        }
    }

    /// Intersection with another mask (AND).
    pub fn intersection(&mut self, other: &TokenMask) {
        for (a, b) in self.bits.iter_mut().zip(other.bits.iter()) {
            *a &= *b;
        }
    }
}

/// Decoder state for constraint tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecoderState {
    /// Current state in the constraint automaton.
    pub automaton_state: StateId,
    /// Stack for PDA (empty for FSM).
    pub stack: Vec<u32>,
}

impl Default for DecoderState {
    fn default() -> Self {
        Self {
            automaton_state: 0,
            stack: Vec::new(),
        }
    }
}

/// Trait for constrained decoders.
pub trait ConstrainedDecoder {
    /// Get valid tokens given current state.
    fn valid_tokens(&self, state: &DecoderState) -> TokenMask;

    /// Advance state with a token.
    fn advance(&self, state: &DecoderState, token: TokenId) -> Option<DecoderState>;

    /// Check if current state is accepting.
    fn is_accepting(&self, state: &DecoderState) -> bool;

    /// Get initial state.
    fn initial_state(&self) -> DecoderState;

    /// Get vocabulary size.
    fn vocab_size(&self) -> usize;
}

/// WFST-based constraint for LLM decoding.
pub struct WfstConstraint<W: Semiring> {
    /// The constraint automaton.
    automaton: VectorWfst<TokenId, W>,
    /// Cached valid tokens per state.
    valid_token_cache: HashMap<StateId, TokenMask>,
    /// Vocabulary size.
    vocab_size: usize,
}

impl<W: Semiring + Clone> WfstConstraint<W> {
    /// Create a new WFST constraint.
    pub fn new(automaton: VectorWfst<TokenId, W>, vocab_size: usize) -> Self {
        let mut constraint = Self {
            automaton,
            valid_token_cache: HashMap::new(),
            vocab_size,
        };
        constraint.build_cache();
        constraint
    }

    /// Build valid token cache for all states.
    fn build_cache(&mut self) {
        for state in 0..self.automaton.num_states() {
            let state_id = state as StateId;
            let mut mask = TokenMask::new(self.vocab_size);

            for tr in self.automaton.transitions(state_id) {
                if let Some(label) = tr.input {
                    if (label as usize) < self.vocab_size {
                        mask.set(label);
                    }
                }
            }

            self.valid_token_cache.insert(state_id, mask);
        }
    }

    /// Get the underlying automaton.
    pub fn automaton(&self) -> &VectorWfst<TokenId, W> {
        &self.automaton
    }
}

impl<W: Semiring + Clone> ConstrainedDecoder for WfstConstraint<W> {
    fn valid_tokens(&self, state: &DecoderState) -> TokenMask {
        self.valid_token_cache
            .get(&state.automaton_state)
            .cloned()
            .unwrap_or_else(|| TokenMask::new(self.vocab_size))
    }

    fn advance(&self, state: &DecoderState, token: TokenId) -> Option<DecoderState> {
        for tr in self.automaton.transitions(state.automaton_state) {
            if tr.input == Some(token) {
                return Some(DecoderState {
                    automaton_state: tr.to,
                    stack: state.stack.clone(),
                });
            }
        }
        None
    }

    fn is_accepting(&self, state: &DecoderState) -> bool {
        self.automaton.is_final(state.automaton_state)
    }

    fn initial_state(&self) -> DecoderState {
        DecoderState {
            automaton_state: self.automaton.start(),
            stack: Vec::new(),
        }
    }

    fn vocab_size(&self) -> usize {
        self.vocab_size
    }
}

/// Build WFST constraint from a regular expression.
///
/// This compiles a regex pattern into a DFA, then converts to WFST.
pub fn from_regex<W: Semiring + From<f64>>(
    pattern: &str,
    vocab_size: usize,
) -> Option<WfstConstraint<W>> {
    // Simplified regex to FSM compilation
    // A full implementation would use a proper regex engine

    let mut fst: VectorWfst<TokenId, W> = VectorWfst::new();
    let mut current_state = fst.add_state();
    fst.set_start(current_state);

    let mut char_iter = pattern.chars().peekable();

    while let Some(ch) = char_iter.next() {
        match ch {
            '.' => {
                // Any single character
                let next_state = fst.add_state();
                for token in 0..vocab_size as TokenId {
                    fst.add_transition(WeightedTransition {
                        from: current_state,
                        input: Some(token),
                        output: Some(token),
                        to: next_state,
                        weight: W::one(),
                    });
                }
                current_state = next_state;
            }
            '*' => {
                // Kleene star on previous state - make it a self-loop
                // This is simplified; real impl would track previous element
                for token in 0..vocab_size as TokenId {
                    fst.add_transition(WeightedTransition {
                        from: current_state,
                        input: Some(token),
                        output: Some(token),
                        to: current_state,
                        weight: W::one(),
                    });
                }
            }
            '\\' => {
                // Escape sequence
                if let Some(escaped) = char_iter.next() {
                    let next_state = fst.add_state();
                    let token = escaped as u32;
                    if token < vocab_size as u32 {
                        fst.add_transition(WeightedTransition {
                            from: current_state,
                            input: Some(token),
                            output: Some(token),
                            to: next_state,
                            weight: W::one(),
                        });
                    }
                    current_state = next_state;
                }
            }
            _ => {
                // Literal character
                let next_state = fst.add_state();
                let token = ch as u32;
                if token < vocab_size as u32 {
                    fst.add_transition(WeightedTransition {
                        from: current_state,
                        input: Some(token),
                        output: Some(token),
                        to: next_state,
                        weight: W::one(),
                    });
                }
                current_state = next_state;
            }
        }
    }

    fst.set_final(current_state, W::one());

    Some(WfstConstraint::new(fst, vocab_size))
}

/// Compressed FSM for faster decoding.
///
/// Precomputes valid token sets and batch transitions for efficiency.
#[derive(Debug)]
pub struct CompressedFsmConstraint {
    /// Transitions: (state, token) -> next_state
    transitions: HashMap<(StateId, TokenId), StateId>,
    /// Valid tokens per state.
    valid_tokens: HashMap<StateId, TokenMask>,
    /// Final states.
    final_states: HashSet<StateId>,
    /// Start state.
    start_state: StateId,
    /// Vocabulary size.
    vocab_size: usize,
}

impl CompressedFsmConstraint {
    /// Create from a WFST.
    pub fn from_wfst<W: Semiring + Clone>(
        wfst: &VectorWfst<TokenId, W>,
        vocab_size: usize,
    ) -> Self {
        let mut transitions = HashMap::new();
        let mut valid_tokens = HashMap::new();
        let mut final_states = HashSet::new();

        for state in 0..wfst.num_states() {
            let state_id = state as StateId;
            let mut mask = TokenMask::new(vocab_size);

            for tr in wfst.transitions(state_id) {
                if let Some(label) = tr.input {
                    transitions.insert((state_id, label), tr.to);
                    if (label as usize) < vocab_size {
                        mask.set(label);
                    }
                }
            }

            valid_tokens.insert(state_id, mask);

            if wfst.is_final(state_id) {
                final_states.insert(state_id);
            }
        }

        Self {
            transitions,
            valid_tokens,
            final_states,
            start_state: wfst.start(),
            vocab_size,
        }
    }
}

impl ConstrainedDecoder for CompressedFsmConstraint {
    fn valid_tokens(&self, state: &DecoderState) -> TokenMask {
        self.valid_tokens
            .get(&state.automaton_state)
            .cloned()
            .unwrap_or_else(|| TokenMask::new(self.vocab_size))
    }

    fn advance(&self, state: &DecoderState, token: TokenId) -> Option<DecoderState> {
        self.transitions
            .get(&(state.automaton_state, token))
            .map(|&next| DecoderState {
                automaton_state: next,
                stack: state.stack.clone(),
            })
    }

    fn is_accepting(&self, state: &DecoderState) -> bool {
        self.final_states.contains(&state.automaton_state)
    }

    fn initial_state(&self) -> DecoderState {
        DecoderState {
            automaton_state: self.start_state,
            stack: Vec::new(),
        }
    }

    fn vocab_size(&self) -> usize {
        self.vocab_size
    }
}

/// JSON schema constraint for structured output.
#[derive(Debug, Clone)]
pub struct JsonSchemaConstraint {
    /// Allowed field names.
    pub field_names: Vec<String>,
    /// Required fields.
    pub required_fields: HashSet<String>,
    /// Field type constraints.
    pub field_types: HashMap<String, JsonType>,
}

/// JSON type for schema constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonType {
    /// String type.
    String,
    /// Number type.
    Number,
    /// Integer type.
    Integer,
    /// Boolean type.
    Boolean,
    /// Null type.
    Null,
    /// Array type.
    Array(Box<JsonType>),
    /// Object type.
    Object,
    /// Any type.
    Any,
}

impl JsonSchemaConstraint {
    /// Create an empty schema constraint.
    pub fn new() -> Self {
        Self {
            field_names: Vec::new(),
            required_fields: HashSet::new(),
            field_types: HashMap::new(),
        }
    }

    /// Add a field with type.
    pub fn add_field(&mut self, name: &str, field_type: JsonType, required: bool) {
        self.field_names.push(name.to_string());
        self.field_types.insert(name.to_string(), field_type);
        if required {
            self.required_fields.insert(name.to_string());
        }
    }
}

impl Default for JsonSchemaConstraint {
    fn default() -> Self {
        Self::new()
    }
}

/// Vocabulary mapper for converting between LLM tokens and WFST labels.
#[derive(Debug)]
pub struct VocabMapper {
    /// Token to label mapping.
    token_to_label: HashMap<TokenId, u32>,
    /// Label to token mapping.
    label_to_token: HashMap<u32, TokenId>,
    /// Vocabulary size.
    vocab_size: usize,
}

impl VocabMapper {
    /// Create identity mapping.
    pub fn identity(vocab_size: usize) -> Self {
        let mut token_to_label = HashMap::new();
        let mut label_to_token = HashMap::new();

        for i in 0..vocab_size as u32 {
            token_to_label.insert(i, i);
            label_to_token.insert(i, i);
        }

        Self {
            token_to_label,
            label_to_token,
            vocab_size,
        }
    }

    /// Create from explicit mapping.
    pub fn from_mapping(token_to_label: HashMap<TokenId, u32>) -> Self {
        let label_to_token: HashMap<u32, TokenId> =
            token_to_label.iter().map(|(&t, &l)| (l, t)).collect();
        let vocab_size = token_to_label.len();

        Self {
            token_to_label,
            label_to_token,
            vocab_size,
        }
    }

    /// Map token to label.
    pub fn to_label(&self, token: TokenId) -> Option<u32> {
        self.token_to_label.get(&token).copied()
    }

    /// Map label to token.
    pub fn to_token(&self, label: u32) -> Option<TokenId> {
        self.label_to_token.get(&label).copied()
    }
}

/// Constrained beam search for LLM decoding.
pub struct ConstrainedBeamSearch<C: ConstrainedDecoder> {
    /// The constraint decoder.
    constraint: C,
    /// Beam width.
    beam_width: usize,
    /// Maximum sequence length.
    max_length: usize,
}

/// Beam hypothesis.
#[derive(Debug, Clone)]
pub struct BeamHypothesis {
    /// Generated tokens.
    pub tokens: Vec<TokenId>,
    /// Cumulative log probability.
    pub score: f64,
    /// Constraint state.
    pub state: DecoderState,
}

impl<C: ConstrainedDecoder> ConstrainedBeamSearch<C> {
    /// Create a new constrained beam search.
    pub fn new(constraint: C, beam_width: usize, max_length: usize) -> Self {
        Self {
            constraint,
            beam_width,
            max_length,
        }
    }

    /// Run beam search with the given log probability function.
    ///
    /// The `get_log_probs` function takes the current tokens and returns
    /// log probabilities over the vocabulary.
    pub fn search<F>(&self, get_log_probs: F) -> Vec<BeamHypothesis>
    where
        F: Fn(&[TokenId]) -> Vec<f64>,
    {
        let initial_state = self.constraint.initial_state();
        let mut beams = vec![BeamHypothesis {
            tokens: Vec::new(),
            score: 0.0,
            state: initial_state,
        }];

        for _ in 0..self.max_length {
            let mut candidates = Vec::new();

            for beam in &beams {
                // Get valid tokens from constraint
                let valid_mask = self.constraint.valid_tokens(&beam.state);

                // Get log probabilities from LLM
                let log_probs = get_log_probs(&beam.tokens);

                // Expand with valid tokens
                for token in valid_mask.iter_valid() {
                    if let Some(new_state) = self.constraint.advance(&beam.state, token) {
                        let token_score = log_probs
                            .get(token as usize)
                            .copied()
                            .unwrap_or(f64::NEG_INFINITY);
                        let new_score = beam.score + token_score;

                        let mut new_tokens = beam.tokens.clone();
                        new_tokens.push(token);

                        candidates.push(BeamHypothesis {
                            tokens: new_tokens,
                            score: new_score,
                            state: new_state,
                        });
                    }
                }
            }

            if candidates.is_empty() {
                break;
            }

            // Sort and keep top beam_width
            candidates.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            candidates.truncate(self.beam_width);
            beams = candidates;

            // Check if all beams are in accepting states
            if beams.iter().all(|b| self.constraint.is_accepting(&b.state)) {
                break;
            }
        }

        // Filter to only accepting hypotheses
        beams
            .into_iter()
            .filter(|b| self.constraint.is_accepting(&b.state))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_mask() {
        let mut mask = TokenMask::new(100);
        assert!(!mask.is_valid(0));

        mask.set(5);
        mask.set(10);
        assert!(mask.is_valid(5));
        assert!(mask.is_valid(10));
        assert!(!mask.is_valid(7));

        mask.unset(5);
        assert!(!mask.is_valid(5));

        assert_eq!(mask.count_valid(), 1);
    }

    #[test]
    fn test_token_mask_all_valid() {
        let mask = TokenMask::all_valid(100);
        assert!(mask.is_valid(0));
        assert!(mask.is_valid(50));
        assert!(mask.is_valid(99));
        assert!(!mask.is_valid(100));
    }

    #[test]
    fn test_decoder_state() {
        let state = DecoderState::default();
        assert_eq!(state.automaton_state, 0);
        assert!(state.stack.is_empty());
    }

    #[test]
    fn test_json_schema_constraint() {
        let mut schema = JsonSchemaConstraint::new();
        schema.add_field("name", JsonType::String, true);
        schema.add_field("age", JsonType::Integer, false);

        assert_eq!(schema.field_names.len(), 2);
        assert!(schema.required_fields.contains("name"));
        assert!(!schema.required_fields.contains("age"));
    }

    #[test]
    fn test_vocab_mapper() {
        let mapper = VocabMapper::identity(100);
        assert_eq!(mapper.to_label(50), Some(50));
        assert_eq!(mapper.to_token(50), Some(50));
    }

    #[test]
    fn test_wfst_constraint() {
        let mut fst: VectorWfst<TokenId, TropicalWeight> = VectorWfst::new();

        // Simple FSM: 0 --1--> 1 --2--> 2 (final)
        let s0 = fst.add_state();
        let s1 = fst.add_state();
        let s2 = fst.add_state();

        fst.set_start(s0);
        fst.set_final(s2, TropicalWeight::one());

        fst.add_transition(WeightedTransition {
            from: s0,
            input: Some(1),
            output: Some(1),
            to: s1,
            weight: TropicalWeight::one(),
        });

        fst.add_transition(WeightedTransition {
            from: s1,
            input: Some(2),
            output: Some(2),
            to: s2,
            weight: TropicalWeight::one(),
        });

        let constraint = WfstConstraint::new(fst, 10);
        let state0 = constraint.initial_state();

        let valid0 = constraint.valid_tokens(&state0);
        assert!(valid0.is_valid(1));
        assert!(!valid0.is_valid(2));

        let state1 = constraint.advance(&state0, 1).expect("should advance");
        let valid1 = constraint.valid_tokens(&state1);
        assert!(!valid1.is_valid(1));
        assert!(valid1.is_valid(2));

        let state2 = constraint.advance(&state1, 2).expect("should advance");
        assert!(constraint.is_accepting(&state2));
    }
}
