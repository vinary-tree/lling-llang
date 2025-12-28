# Beam Search Optimization

This document covers optimization techniques for beam search decoding with WFSTs, particularly log-semiring weight pushing which provides **up to 18× speedup** in beam-pruned Viterbi decoding.

## Overview

Beam search is the standard inference algorithm for large WFST-based systems, pruning hypotheses that fall below a score threshold. The effectiveness of pruning depends critically on how weights are distributed through the transducer.

| Optimization | Description | Impact |
|--------------|-------------|--------|
| Log-Semiring Pushing | Stochastic normalization | Up to 18× speedup |
| Lookahead Tables | Future cost estimation | Improved pruning |
| Token Grouping | Lazy evaluation for composition | 10-20× fewer ops |
| N-gram Back-off | Compact LM representation | Avoids O(\|V\|²) |

## Log-Semiring Weight Pushing

This is the **most critical optimization** for beam search. The key insight from Mohri et al. is that weight pushing must be done in the **log semiring, NOT tropical**.

### Why Log Semiring?

**Tropical semiring pushing** uses the min-weight potential (best path):
- Can actually *harm* beam search by distorting relative scores
- "May slow down beam-pruned Viterbi decoding many fold"

**Log semiring pushing** uses the sum of all path probabilities:
- Creates a stochastic automaton (weights sum to 1 at each state)
- "Has a very large beneficial impact on pruning efficacy"
- Conjecture: "Optimal likelihood ratio test for pruning decisions"

### The Stochastic Property

After log-semiring pushing, at each state q:

```
Σ exp(-weight) = 1
    outgoing arcs + final

In log space: logadd(all outgoing weights + final weight) ≈ 0
```

This means transitions represent proper probability distributions, making beam pruning decisions statistically meaningful.

### Algorithm

1. **Compute backward potentials**: V(q) = -log(Σ exp(-path_weight)) for all paths from q to final
2. **Reweight transitions**: w'(e) = w(e) + V(target) - V(source)
3. **Normalize finals**: Set final weights to LogWeight::one()

```
Before pushing:
  0 --a/1.0--> 1 --b/2.0--> 2 (final)

  V(2) = 0.0 (final, log(1) = 0)
  V(1) = 2.0 (path 1→2 has weight 2.0)
  V(0) = 3.0 (path 0→2 has weight 1.0+2.0)

After pushing:
  Transition 0→1: w' = 1.0 + V(1) - V(0) = 1.0 + 2.0 - 3.0 = 0.0
  Transition 1→2: w' = 2.0 + V(2) - V(1) = 2.0 + 0.0 - 2.0 = 0.0

  Result: All path weight absorbed into initial state potential
```

### Core API

```rust
use lling_llang::optimization::{
    prepare_for_beam_search, LogPushConfig, BeamSearchPrepResult,
    compute_log_potentials, apply_log_push,
};

/// Configuration for log-semiring weight pushing
pub struct LogPushConfig {
    /// Verify stochasticity after pushing
    pub verify_stochastic: bool,
    /// Tolerance for stochasticity check
    pub stochastic_epsilon: f64,
    /// Normalize final weights to one
    pub normalize_finals: bool,
}

/// Result of preparing a WFST for beam search
pub struct BeamSearchPrepResult {
    /// Whether pushing succeeded
    pub pushed: bool,
    /// Total weight of original WFST
    pub total_weight: LogWeight,
    /// Stochasticity verification result
    pub is_stochastic: Option<bool>,
    /// Statistics
    pub num_states: usize,
    pub num_transitions: usize,
}
```

### Basic Usage

```rust
use lling_llang::optimization::{prepare_for_beam_search, LogPushConfig};
use lling_llang::semiring::LogWeight;
use lling_llang::wfst::VectorWfst;

// Build or load recognition WFST
let mut fst: VectorWfst<char, LogWeight> = build_recognition_wfst();

// Prepare for beam search
let result = prepare_for_beam_search(&mut fst, LogPushConfig::default())?;

println!("Total weight: {:?}", result.total_weight);
println!("States: {}, Transitions: {}", result.num_states, result.num_transitions);

// Now use fst with beam search for improved pruning
```

### Verified Stochasticity

```rust
use lling_llang::optimization::{prepare_for_beam_search, LogPushConfig};

// Enable verification that result is properly stochastic
let config = LogPushConfig::verified();

let result = prepare_for_beam_search(&mut fst, config)?;

if result.is_stochastic == Some(true) {
    println!("WFST is properly stochastic - optimal for beam search");
} else {
    println!("Warning: WFST may not be fully stochastic");
}
```

### Low-Level API

```rust
use lling_llang::optimization::{compute_log_potentials, apply_log_push};

// Step 1: Compute backward potentials
let potentials = compute_log_potentials(&fst)?;

// The potential at start state is the total weight
let total_weight = potentials[fst.start() as usize].clone();
println!("Total probability mass: exp(-{}) = {}",
         total_weight.value(),
         (-total_weight.value()).exp());

// Step 2: Apply the push transformation
apply_log_push(&mut fst, &potentials, true)?;

// WFST is now ready for beam search
```

## Lookahead Tables

Lookahead tables precompute future costs to improve pruning by making scores at different path stages comparable.

### Problem

During beam search, hypotheses at different positions have incomparable scores:
- A hypothesis that has processed 3 words naturally has higher accumulated cost
- Than one that has processed only 1 word

Without normalization, short paths look artificially "better."

### Solution

Add an estimate of remaining cost to each hypothesis:

```
normalized_score = accumulated_score + lookahead(current_state)

Where lookahead(q) = estimated cost to reach final from q
```

### API

```rust
use lling_llang::optimization::{
    LookaheadTable, build_lookahead_table, LookaheadConfig,
};

/// Precomputed lookahead table
pub struct LookaheadTable {
    potentials: Vec<LogWeight>,
    total_weight: LogWeight,
    num_reachable: usize,
}

impl LookaheadTable {
    /// Get lookahead score for a state
    pub fn get(&self, state: StateId) -> LogWeight;

    /// Get raw f64 value (INFINITY if unreachable)
    pub fn get_value(&self, state: StateId) -> f64;

    /// Check if state can reach a final state
    pub fn is_reachable(&self, state: StateId) -> bool;

    /// Normalize a score with lookahead
    pub fn normalize_score(&self, state: StateId, accumulated: &LogWeight) -> LogWeight;
}
```

### Usage

```rust
use lling_llang::optimization::{build_lookahead_table, LookaheadConfig};

// Build lookahead table (same computation as log-push potentials)
let table = build_lookahead_table(&fst, LookaheadConfig::default())?;

println!("Reachable states: {} / {}", table.num_reachable(), table.num_states());

// During beam search:
for hyp in hypotheses {
    // Normalize score for fair comparison
    let normalized = table.normalize_score(hyp.state, &hyp.accumulated_score);

    // Use normalized score for pruning
    if normalized.value() > beam_threshold {
        prune(hyp);
    }
}
```

### On-the-fly Lookahead

For single queries (not recommended for repeated access):

```rust
use lling_llang::optimization::lookahead::compute_lookahead_single;

let lookahead = compute_lookahead_single(&fst, state_id);
```

## Token Grouping (LET-Decoder)

For on-the-fly composition (e.g., HCLG ∘ G_r), token grouping reduces redundant operations by 10-20×.

### Problem

During on-the-fly composition with a residual grammar G_r:
- Many tokens share the same HCLG state but differ in grammar state
- Expanding all tokens independently wastes computation
- Most tokens get pruned before reaching word boundaries

### Solution: Lazy Evaluation

Group tokens by base-graph state and defer expansion until word boundaries:

```
Token Group = {tokens with same HCLG-state, different G_r-states}

Key insight: Only expand when a word label is emitted
Until then, just track the best forward probability for pruning
```

### Core Types

```rust
use lling_llang::optimization::{
    Token, TokenGroup, TokenGroupPool, TokenGroupManager,
    TokenGroupConfig, TokenGroupStats, BucketQueue,
};

/// A decoding token (hypothesis)
pub struct Token {
    pub base_state: StateId,     // State in HCLG
    pub grammar_state: StateId,  // State in G_r
    pub forward_prob: LogWeight, // Accumulated probability
    pub prev_token: Option<TokenId>,
    pub prev_arc: Option<ArcId>,
}

/// Group of tokens at same base-graph state
pub struct TokenGroup {
    pub base_state: StateId,
    pub best_forward_prob: LogWeight,  // Best among all tokens
    pub expanded: bool,                 // Tokens materialized?
    // ...
}

/// Configuration
pub struct TokenGroupConfig {
    pub max_tokens_per_group: usize,  // Default: 32
    pub max_groups: usize,            // Default: 10000
    pub num_buckets: usize,           // For histogram pruning
    pub lazy_evaluation: bool,        // Enable deferred expansion
}
```

### Usage

```rust
use lling_llang::optimization::{TokenGroupManager, TokenGroupConfig, Token};

let config = TokenGroupConfig {
    lazy_evaluation: true,
    max_tokens_per_group: 32,
    ..Default::default()
};

let mut manager = TokenGroupManager::new(config);

// Process tokens during decoding
for trans in transitions {
    let token = Token {
        base_state: trans.to,
        grammar_state: grammar_state,
        forward_prob: accumulated.times(&trans.weight),
        prev_token: Some(current_token_id),
        prev_arc: Some(trans.arc_id),
    };

    // is_word_arc forces expansion (for lattice generation)
    let group_id = manager.process_token(token, is_word_arc);
}

// Prune groups beyond threshold
let pruned = manager.prune(beam_threshold);

// Advance to next frame
let frame_result = manager.advance_frame();

// Check statistics
println!("Tokens processed: {}", manager.stats().tokens_processed);
println!("Ops saved: {}", manager.stats().ops_saved);
```

### BucketQueue for Histogram Pruning

BucketQueue organizes tokens by quantized forward probability for efficient pruning:

```rust
use lling_llang::optimization::BucketQueue;

// Create queue with 100 buckets for log probs in [0, 100]
let mut queue: BucketQueue<TokenGroupId> = BucketQueue::new(100, 0.0, 100.0);

// Insert tokens
queue.insert(score.value(), group_id);

// Pop best (lowest score = highest probability)
while let Some(group_id) = queue.pop() {
    process_group(group_id);
}

// Prune beyond threshold bucket
let pruned = queue.prune_beyond(max_bucket);

// Get histogram for analysis
let histogram = queue.histogram();
```

### Group Links for Back-tracing

For lazy back-tracing without materializing tokens:

```rust
// Add link between groups (for lazy path reconstruction)
manager.add_link(
    source_group_id,
    target_group_id,
    transition_weight,
    is_word_arc,
);

// Later: expand group when needed
manager.expand_group(group_id);
```

## N-gram Back-off Structure

For large vocabulary LMs, back-off structure avoids O(|V|²) transitions.

### Problem

Naively representing an n-gram LM:
- O(|V|^{n-1}) states for contexts
- O(|V|^n) arcs for all n-grams

For vocabulary of 100K words: 10^10 potential bigram arcs.

### Solution: Back-off

Only store *observed* n-grams, with back-off for unseen:

```
Seen bigram w₁w₂:   Direct transition from state w₁ to w₂
Unseen bigram w₁w₃: ε-transition from w₁ to backoff with weight -log(β(w₁))
                    then transition from backoff to w₃ with weight -log(P(w₃))
```

### Core API

```rust
use lling_llang::optimization::{
    NgramLmConfig, NgramLmBuilder, NgramStats,
    BigramLm, BigramStats,
    compute_size_reduction, SizeReduction,
};

/// N-gram LM builder
pub struct NgramLmBuilder {
    config: NgramLmConfig,
    // ...
}

impl NgramLmBuilder {
    pub fn new(config: NgramLmConfig) -> Self;

    /// Add n-gram: P(word | context)
    pub fn add_ngram(&mut self, context: &[VocabId], word: VocabId, log_prob: f64);

    /// Add back-off weight for context
    pub fn add_backoff(&mut self, context: &[VocabId], weight: f64);

    /// Build WFST with back-off structure
    pub fn build(self) -> VectorWfst<VocabId, LogWeight>;

    /// Get statistics
    pub fn stats(&self) -> NgramStats;
}
```

### Building a Bigram LM

```rust
use lling_llang::optimization::{BigramLm};

let mut lm = BigramLm::new(vocab_size);

// Set unigram probabilities
lm.set_unigram(word_id, log_prob);

// Set observed bigram probabilities
lm.set_bigram(w1, w2, log_prob);

// Set back-off weights
lm.set_backoff(w1, backoff_weight);

// Query (with automatic back-off for unseen)
let prob = lm.prob(w1, w2);

// Convert to WFST
let fst = lm.to_wfst();
```

### Building a Trigram LM

```rust
use lling_llang::optimization::{NgramLmConfig, NgramLmBuilder};

let config = NgramLmConfig {
    order: 3,  // Trigram
    use_backoff_symbol: true,  // Prevents ε-removal expansion
    vocab_size: 10000,
    prune_threshold: Some(5.0),  // Prune low-probability n-grams
};

let mut builder = NgramLmBuilder::new(config);

// Add trigrams: P(w3 | w1, w2)
builder.add_ngram(&[w1, w2], w3, log_prob);

// Add bigrams: P(w2 | w1)
builder.add_ngram(&[w1], w2, log_prob);

// Add unigrams: P(w)
builder.add_ngram(&[], w, log_prob);

// Add back-off weights
builder.add_backoff(&[w1, w2], backoff_weight);
builder.add_backoff(&[w1], backoff_weight);

// Build compact WFST
let fst = builder.build();

// Check statistics
let stats = builder.stats();
println!("Trigrams: {}", stats.order_counts[3]);
println!("Bigrams: {}", stats.order_counts[2]);
println!("Unigrams: {}", stats.order_counts[1]);
```

### Size Reduction Analysis

```rust
use lling_llang::optimization::compute_size_reduction;

// Compare dense vs sparse representation
let reduction = compute_size_reduction(
    vocab_size,     // e.g., 100000
    num_observed,   // e.g., 5000000 observed bigrams
    order,          // 2 for bigram
);

println!("Dense:  {} states, {} arcs", reduction.dense_states, reduction.dense_arcs);
println!("Sparse: {} states, {} arcs", reduction.sparse_states, reduction.sparse_arcs);
println!("Arc reduction: {:.1}%", reduction.arc_reduction * 100.0);
```

## Performance Comparison

### Log-Semiring Pushing Impact

| Configuration | Relative Speed |
|--------------|----------------|
| Unpushed WFST | 1.0× (baseline) |
| Tropical-pushed | 0.5-2.0× (can hurt!) |
| Log-pushed | 10-18× |

### Token Grouping Impact

| Metric | Without Grouping | With Grouping |
|--------|------------------|---------------|
| Composition ops | 100% | 5-10% |
| Memory usage | High | Reduced |
| Latency | Baseline | Improved |

### N-gram Back-off Impact

| Vocabulary | Dense Arcs | Sparse Arcs | Reduction |
|------------|------------|-------------|-----------|
| 1,000 | 1,000,000 | ~50,000 | 95% |
| 10,000 | 100,000,000 | ~500,000 | 99.5% |
| 100,000 | 10^10 | ~5,000,000 | 99.95% |

## Best Practices

### 1. Always Use Log-Semiring Pushing

```rust
// CORRECT: Log semiring pushing
prepare_for_beam_search(&mut fst, LogPushConfig::default())?;

// WRONG: Tropical pushing for beam search
// (Tropical is fine for Viterbi, but hurts beam pruning)
```

### 2. Precompute Lookahead for Large WFSTs

```rust
// For repeated decoding, precompute lookahead table
let lookahead = build_lookahead_table(&recognition_fst, LookaheadConfig::default())?;

// Use table for all utterances
for utterance in utterances {
    decode_with_lookahead(utterance, &recognition_fst, &lookahead);
}
```

### 3. Enable Token Grouping for On-the-fly Composition

```rust
// When composing with dynamic grammar (e.g., rescoring)
let config = TokenGroupConfig {
    lazy_evaluation: true,  // Critical for speedup
    ..Default::default()
};
```

### 4. Use Back-off for Large Vocabularies

```rust
// For vocab > 1000, back-off structure is essential
let config = NgramLmConfig {
    use_backoff_symbol: true,  // Prevents graph explosion
    prune_threshold: Some(10.0),  // Remove very rare n-grams
    ..Default::default()
};
```

## Theoretical Background

### Stochastic Automata

After log-semiring pushing, the WFST becomes a **stochastic automaton**:
- At each state, outgoing transition probabilities sum to 1
- Transitions represent proper conditional probabilities
- This is the optimal representation for beam pruning

### Optimal Pruning Conjecture

Mohri et al. conjecture that log-semiring pushing provides the **optimal likelihood ratio test** for pruning decisions:

> "The acoustic likelihoods and transducer probabilities are now synchronized to obtain the optimal likelihood ratio test for deciding whether to prune."

### α-Stable Property

Token grouping maintains the **α-stable property**:
- Updating unexpanded groups doesn't change their forward probability
- Enables correct lattice generation despite deferred expansion
- Guarantees exact results with lazy evaluation

## Complexity

### Log-Semiring Pushing

| Operation | Time | Space |
|-----------|------|-------|
| Compute potentials | O(\|Q\| + \|E\|) acyclic | O(\|Q\|) |
| Apply push | O(\|E\|) | O(\|E\|) |

### Lookahead Table

| Operation | Time | Space |
|-----------|------|-------|
| Build table | O(\|Q\| + \|E\|) | O(\|Q\|) |
| Query | O(1) | - |

### Token Grouping

| Operation | Time |
|-----------|------|
| Process token | O(1) amortized |
| Advance frame | O(groups) |
| Back-trace | O(path length) |

## References

1. Mohri, Pereira, Riley (2002): "WFSTs in Speech Recognition"
2. Mohri, Pereira, Riley (2008): "Speech Recognition with WFSTs" (Handbook)
3. Lv et al. (2023): "LET-Decoder: Lazy-evaluation Token-group Decoder"
4. Hannun et al. (2021): "Differentiable WFSTs" (ICLR)

## Next Steps

- [Log-Semiring](../architecture/semirings.md#log-semiring): Understanding the semiring
- [Weight Pushing](../algorithms/weight-pushing.md): General weight pushing algorithm
- [ASR Pipeline](asr-pipeline.md): Full speech recognition system
- [GPU Acceleration](gpu-acceleration.md): Hardware-accelerated decoding
