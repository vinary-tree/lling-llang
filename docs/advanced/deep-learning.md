# Deep Learning Integration

This module (`src/differentiable/`) extends the differentiable WFST (**W**eighted **F**inite-**S**tate **T**ransducer) framework with advanced features for integrating WFSTs into deep-learning pipelines, including convolutional WFST layers, token graph variants, marginalized word piece decompositions, and n-gram pruning with back-off. A WFST here is a *differentiable layer*: a forward pass computes a log-sum-exp score over all paths, and the backward pass returns the gradient of that score with respect to the arc weights ([Hannun 2020](../BIBLIOGRAPHY.md#ref-hannun2020)).

## Terms & symbols

Defined centrally in [`../NOTATION.md`](../NOTATION.md); repeated locally for the terms this doc uses.

| Symbol | Meaning |
|---|---|
| $`\oplus`$ / $`\otimes`$ | semiring *plus* (combine alternative paths; Log: $`\oplus_{\log}`$) / *times* (combine arcs along a path). |
| $`\circ`$ | composition — chains transducers; the output tape of one feeds the input tape of the next. |
| $`*`$ | Kleene closure of a transducer ($`(T_1 + \dots + T_C)^*`$: any number of token graphs in sequence). |
| $`\varepsilon`$ | epsilon — a transition that consumes/emits nothing (the CTC blank/back-off arc). |
| $`E`$, $`B`$, $`T_i`$, $`L`$, $`Y`$ | emissions · bigram graph · token graph for token $`i`$ · lexicon transducer · target sequence. |
| $`A`$, $`Z`$ | constrained alignment graph $`A = E \circ (B \circ ((T_1+\dots+T_C)^* \circ (L \circ Y)))`$ · unconstrained normalizer $`Z = E \circ B`$. |
| $`\lvert V\rvert`$ | vocabulary size; $`\lvert E\rvert`$ = arc count in complexity bounds. |
| $`k`$, $`c_i`$, $`c_o`$, $`w`$ | kernel width · input / output channels · receptive-field width. |
| **CTC** | **C**onnectionist **T**emporal **C**lassification (alignment-free sequence labeling, [Graves 2006](../BIBLIOGRAPHY.md#ref-graves2006)). |

## Overview

Deep learning integration enables:
1. **WFST as Neural Layers**: Use WFSTs as convolutional layers with 38× fewer parameters
2. **Flexible Token Graphs**: Different CTC variants for various training objectives
3. **Learned Decompositions**: Marginalize over word piece segmentations
4. **Scalable N-grams**: 87× speedup through pruning and back-off

The data flows neural-network → emissions $`E`$ → a stack of differentiable WFST layers → a scalar loss; the gradient of that loss flows back through the same graph to the network. The structural spine of the stack is the lexicon-marginalization alignment graph $`A = E \circ (B \circ ((T_1 + \dots + T_C)^* \circ (L \circ Y)))`$, scored against the unconstrained normalizer $`Z`$ to give $`\text{loss} = -\log P(Y \mid X) = \operatorname{forwardScore}(Z) - \operatorname{forwardScore}(A)`$.

![WFST as a differentiable neural-network layer: neural emissions E feed a token graph (CTC variant) with an epsilon blank/repeat self-loop, then the Kleene closure of per-token graphs composed with a lexicon L and the target Y form the alignment graph A; a forward log-sum-exp score over A and over the normalizer Z combine into the loss, whose backward pass sends arc-posterior gradients back to the network](../diagrams/advanced/deep-learning-layers.svg)

*Purple = deep-learning tier; the green-bold spine is the constrained path $`E \to \text{token graph} \to \text{closure} \to L \to Y`$ and its forward score; grey-dashed $`\varepsilon`$ arcs are the CTC blank/repeat self-loop and the unconstrained normalizer branch $`Z`$; the amber note marks the lexicon marginalization (sum over every valid word-piece decomposition of $`Y`$); the red-dashed arc is the gradient $`\partial\text{Loss}/\partial E`$ returning to the encoder.*

<details><summary>Text view</summary>

```text
Neural Network → Emissions → WFST Layer → Loss
                     ↓
              ┌──────┴──────┐
              │ Token Graph │ (CTC variant)
              └──────┬──────┘
                     ↓
              ┌──────┴──────┐
              │   Lexicon   │ (marginalization)
              └──────┬──────┘
                     ↓
              ┌──────┴──────┐
              │   N-gram    │ (pruned transitions)
              └─────────────┘
```

</details>

## WFST Convolutional Layers

WFST kernels can replace traditional convolutions with dramatic parameter reduction.

### Concept

Traditional convolution: $`\text{output}[t] = \sum_k W[k] \cdot \text{input}[t+k]`$

WFST convolution: $`\text{output}[t] = \bigoplus_{\log} \{\, \operatorname{score}(p) : p \in K \circ R_t \,\}`$ (a log-sum-exp over every path of the composed kernel-and-receptive-field automaton).

Where:
- $`K`$ = WFST kernel (encodes structural patterns)
- $`R_t`$ = receptive field (linear graph from hidden units at positions $`t`$ to $`t+k`$)
- The $`\oplus_{\log}`$ (log-sum-exp) aggregates all path scores

### API

```rust
use lling_llang::differentiable::{
    WfstKernel, WfstConvLayer, WfstConvConfig, ReceptiveField,
    wfst_conv_forward, wfst_conv_backward, PaddingMode,
};

// Create a WFST kernel: (vocabulary size, receptive-field width, initial weight)
let kernel = WfstKernel::<usize>::new(256, 3, 0.0);

// Configure the convolutional layer
let config = WfstConvConfig {
    input_channels: 256,
    output_channels: 32,
    kernel_size: 3,
    stride: 1,
    padding: PaddingMode::Same,
};

// Create the layer
let layer = WfstConvLayer::<usize>::new(config);

// Forward pass — input is [time × channels]
let input_sequence: Vec<Vec<f64>> = vec![/* hidden states */];
let output = wfst_conv_forward(&layer, &input_sequence);

// Backward pass — returns (input gradients, per-kernel gradients)
let output_grad: Vec<Vec<f64>> = vec![/* gradient from upstream */];
let (input_grad, kernel_grads) = wfst_conv_backward(&layer, &input_sequence, &output_grad);
```

### Receptive Field Construction

The receptive field is a linear graph where edge weights come from hidden units:

```
Position:     t      t+1     t+2
              ●──────●───────●
           h[t,0]  h[t+1,0] h[t+2,0]
              ●──────●───────●
           h[t,1]  h[t+1,1] h[t+2,1]
```

```rust
// Build receptive field from hidden states: a window of (label, weight) pairs.
// Each pair is one position's emitted label and its score; `start_pos` is the
// window's offset into the input sequence.
let hidden_states: Vec<(usize, f64)> = vec![
    (0, 1.2),  // position t   → label 0, weight 1.2
    (1, 0.7),  // position t+1 → label 1, weight 0.7
    (0, 0.4),  // position t+2 → label 0, weight 0.4
];
let start_pos = 5;
let receptive_field = ReceptiveField::from_hidden_states(&hidden_states, start_pos);

// Compose with kernel, then score the composed graph.
let composed = compose(&kernel.fst, &receptive_field.fst);
let output = forward_score(&GradientWfst::from_wfst(&composed));
```

### Parameter Efficiency

| Layer Type | Parameters | Operations |
|------------|------------|------------|
| WFST Conv ($`k=3`$, $`c_o=64`$) | 2,048 | $`O(k \times w \times c_o)`$ |
| Traditional Conv ($`k=3`$, $`c_i=256`$, $`c_o=64`$) | 79,000 | $`O(k \times c_i \times c_o)`$ |

38× **fewer parameters** with WFST convolution!

### Why It Works

1. **Structural sharing**: Kernel encodes pattern structure, not all input combinations
2. **Sparse coverage**: Only relevant patterns have non-zero weights
3. **Compositional**: Complex patterns built from simpler sub-patterns

## Token Graph Variants

Different CTC topologies for different training objectives.

### Token Graph Types

```rust
use lling_llang::differentiable::{
    TokenGraphType, TokenGraphConfig, build_token_graph,
    build_vocabulary_graph, build_blank_graph,
};

// Available types
pub enum TokenGraphType {
    Standard,                              // Classic CTC with blank and repeats
    Spike,                                 // Single emission per token (no repeats)
    DurationLimited { max_duration: usize }, // Limit token duration to n frames
    EquallySpaced { blank_count: usize },    // Fixed blank count between tokens
}
```

### Standard CTC

Full CTC with blank token and self-loops for repetition:

```
┌──ε──┐   ┌──a──┐
│     ▼   │     ▼
●────────►●────────►●
blank      a       blank
```

```rust
let config = TokenGraphConfig {
    graph_type: TokenGraphType::Standard,
    blank_id: 0,
    ..Default::default()
};

let token = 1; // the token ID this graph accepts
let token_graph = build_token_graph(token, &config);
```

### Spike CTC

Single repetition only—no self-loops on non-blank tokens:

```
●────────►●
 blank     a
```

Best for:
- Peaky acoustic models
- When blank dominance is expected
- Faster training convergence

```rust
let config = TokenGraphConfig {
    graph_type: TokenGraphType::Spike,
    blank_id: 0,
    ..Default::default()
};

let token = 1;
let spike_graph = build_token_graph(token, &config);
```

### Duration-Limited CTC

Limit token duration to 1-2 frames:

```
●───a───►●───a───►●
   1       2      (max)
```

Use when:
- You know maximum token duration
- Training with subsampled features
- Memory-constrained environments

```rust
let config = TokenGraphConfig {
    graph_type: TokenGraphType::DurationLimited { max_duration: 2 }, // Max 2 frames per token
    blank_id: 0,
    ..Default::default()
};

let token = 1;
let duration_limited = build_token_graph(token, &config);
```

### Equally Spaced CTC

Fixed distance between non-blank tokens:

```
●──blank──►●──blank──►●───a───►●
  spacing=2
```

Use for:
- Rhythmic sequences
- Fixed-rate output requirements
- Regularization

```rust
let config = TokenGraphConfig {
    graph_type: TokenGraphType::EquallySpaced { blank_count: 3 }, // Exactly 3 blanks between tokens
    blank_id: 0,
    ..Default::default()
};

let token = 1;
let equally_spaced = build_token_graph(token, &config);
```

### Vocabulary and Blank Graphs

```rust
// Build graph accepting any token from vocabulary
let vocab_graph = build_vocabulary_graph(256, None); // No blank

// Build blank-only graph (for spacing)
let blank_graph = build_blank_graph(0, 3); // blank_id=0, 3 repetitions
```

### Choosing a Token Graph

| Model Context | Recommended Graph |
|---------------|-------------------|
| Short context (Citrinet $`\gamma=0.25`$) | Standard (with self-loops) |
| Long context ($`\gamma=1.0`$) | Spike or Duration-Limited |
| Unlimited context (Conformer) | Spike |
| Fixed frame rate output | Equally Spaced |
| Memory-constrained training | Duration-Limited or Spike |

## Marginalized Word Piece Decompositions

Learn task-optimal tokenization by marginalizing over all valid decompositions.

### The Problem

Fixed word piece decomposition (from SentencePiece) may not be optimal for the task:
- Decomposition learned independently of ASR objective
- Same decomposition for all inputs regardless of context

### The Solution

Marginalize over all valid decompositions — the alignment graph is:

```math
A = E \circ (B \circ ((T_1 + \dots + T_C)^* \circ (L \circ Y)))
```

Where:
- $`E`$ = emissions from neural network
- $`B`$ = bigram transition graph
- $`T_i`$ = token graph for token $`i`$
- $`(\cdot)^{*}`$ = Kleene closure (any number of token graphs)
- $`L`$ = lexicon transducer (word piece → graphemes)
- $`Y`$ = target grapheme sequence

### API

```rust
use lling_llang::differentiable::{
    LexiconEntry, LexiconConfig, MarginalizationContext,
    build_lexicon_transducer, build_target_graph,
    marginalized_loss, build_identity_lexicon, build_character_lexicon,
};

// Define lexicon entries
let entries = vec![
    LexiconEntry::new(0, vec![104, 101, 108]),      // "hel" → [h, e, l]
    LexiconEntry::new(1, vec![108, 111]),           // "lo" → [l, o]
    LexiconEntry::with_weight(2, vec![104], -0.1), // "h" with bias
];

// Configure lexicon
let config = LexiconConfig {
    allow_multiple_decompositions: true,
    init_weight: 0.0,
    word_boundary: Some(32),  // space character as boundary
};

// Build lexicon transducer
let lexicon = build_lexicon_transducer(&entries, &config);

// Build target graph from grapheme sequence
let target = vec![104, 101, 108, 108, 111]; // "hello"
let target_graph = build_target_graph(&target);

// Compute marginalized loss
let emissions = /* from neural network */;
let loss = marginalized_loss(&emissions, &lexicon, &target);
```

### Pre-built Lexicon Helpers

```rust
// Identity lexicon: each word piece maps to itself
let identity = build_identity_lexicon(1000);

// Character lexicon: word pieces map to character sequences
let mut word_pieces = HashMap::new();
word_pieces.insert(0, "hello".to_string());
word_pieces.insert(1, "world".to_string());
let char_lexicon = build_character_lexicon(&word_pieces);
```

### Marginalization Context

For efficient batched computation:

```rust
let mut ctx = MarginalizationContext::new(
    1000,  // vocab_size
    256,   // grapheme_vocab_size
);

// Initialize with lexicon entries
ctx.initialize(&entries);

// Use for batched loss computation
```

### Benefits of Marginalization

1. **Adaptive Decomposition**: Model learns to use different segmentations based on input
2. **Better Generalization**: Handles rare words via character-level fallback
3. **Task Alignment**: Decomposition optimized for recognition, not compression

## N-gram Pruning with Back-off

Scale to large vocabularies with `87×` speedup.

### The Problem

Dense n-gram transitions:
- Bigram: $`O(\lvert V\rvert^2)`$ transitions for vocabulary size $`\lvert V\rvert`$
- Trigram: $`O(\lvert V\rvert^3)`$ transitions
- 1000 word pieces: 1 million bigram arcs!

### The Solution

Prune infrequent n-grams and use back-off:

```
Observed bigram a→b: direct transition
Unobserved bigram a→c: a → ε → backoff → c
                        ↑     ↑
                     backoff  unigram
                     weight   prob
```

### API

```rust
use lling_llang::differentiable::{
    PrunedNgramConfig, NgramCounts,
    build_pruned_bigram_graph, build_pruned_trigram_graph,
    PrunedNgramStats,
};

// Collect n-gram counts from training data
let mut counts = NgramCounts::new();
counts.add_sequence(&[1, 2, 3, 1, 2]);
counts.add_sequence(&[0, 1, 2, 3]);

// Configure pruning
let config = PrunedNgramConfig {
    order: 2,              // Bigram
    min_count: 2,          // Keep n-grams seen ≥2 times
    use_backoff: true,     // Use back-off for unseen
    backoff_weight: 1.0,   // -log(backoff_prob)
    smoothing: true,       // Apply smoothing
    discount: 0.5,         // Discount for smoothing
};

// Build pruned bigram graph
let bigram_fst = build_pruned_bigram_graph(1000, &counts, &config);

// Build pruned trigram graph
let trigram_config = PrunedNgramConfig { order: 3, ..config };
let trigram_fst = build_pruned_trigram_graph(1000, &counts, &trigram_config);
```

### N-gram Count Collection

```rust
let mut counts = NgramCounts::new();

// Add sequences from training data
for sentence in training_data {
    counts.add_sequence(&sentence);
}

// Query counts
let unigram = counts.unigram_count(token_id);
let bigram = counts.bigram_count(prev, curr);
let trigram = counts.trigram_count(prev2, prev1, curr);

// Query probabilities
let p_unigram = counts.unigram_prob(token_id);
let p_bigram = counts.bigram_prob(prev, curr);
```

### Graph Statistics

```rust
let stats = PrunedNgramStats::from_bigram_graph(&bigram_fst, 1000);

println!("States: {}", stats.num_states);
println!("Arcs: {}", stats.num_arcs);
println!("Dense would have: {}", stats.dense_arcs);  // 1,000,000
println!("Compression ratio: {:.1}x", stats.compression_ratio);
```

### Performance Results

| Vocabulary | Pruning | Training Time |
|------------|---------|---------------|
| 26 letters | None | 544 s/epoch |
| 26 letters | min_count=0 | 249 s/epoch |
| 1000 WP | None | 17,939 s/epoch |
| 1000 WP | min_count=10 | **204 s/epoch** |

87× **speedup** for 1000 word pieces with no accuracy loss!

### Smoothing Options

```rust
// Kneser-Ney style discounting
let config = PrunedNgramConfig {
    smoothing: true,
    discount: 0.75,  // Standard discount value
    ..Default::default()
};

// Without smoothing (raw MLE)
let config = PrunedNgramConfig {
    smoothing: false,
    ..Default::default()
};
```

## Second-Order Differentiation

Compute Hessians for advanced optimization.

### Use Cases

1. **Natural Gradient**: Uses Fisher information for better optimization
2. **Uncertainty Estimation**: Hessian diagonal approximates parameter uncertainty
3. **Second-order Optimization**: Newton's method and variants
4. **Influence Functions**: Understanding training data impact

### API

```rust
use lling_llang::differentiable::{
    SecondOrderConfig, SecondOrderWfst, HessianMatrix,
    compute_diagonal_hessian, hessian_vector_product,
    compute_fisher_information, compute_diagonal_fisher,
    natural_gradient, gradient_and_hessian, SecondOrderResult,
};

// Create second-order WFST
let so_wfst = SecondOrderWfst::from_wfst(&fst);

// Compute diagonal Hessian (efficient)
let hessian = compute_diagonal_hessian(&so_wfst);

// Or compute full Fisher information (O(|E|²))
let first_grads = backward(&so_wfst.first_order);
let fisher = compute_fisher_information(&first_grads);
```

### Hessian-Vector Products

Efficient $`O(\lvert E\rvert)`$ computation without materializing the full Hessian:

```rust
// Direction vector
let v: Vec<f64> = vec![1.0; num_arcs];

// Compute H·v using finite differences
let hvp = hessian_vector_product(&so_wfst, &v, 1e-4);
```

### Natural Gradient

Precondition gradient with inverse Fisher for faster convergence:

```rust
// Compute gradients
let gradients = backward(&so_wfst.first_order);

// Compute diagonal Fisher
let fisher = compute_diagonal_fisher(&gradients);

// Natural gradient = F⁻¹ · g
let nat_grad = natural_gradient(&gradients, &fisher, 1e-6);
```

### All-in-One Computation

```rust
let config = SecondOrderConfig {
    full_hessian: false,    // Use diagonal approximation
    diagonal_only: true,
    block_size: 0,          // No blocking
    damping: 1e-6,          // Numerical stability
};

let result: SecondOrderResult = gradient_and_hessian(&fst, &config);

println!("Gradients: {:?}", result.gradients);
println!("Hessian diagonal: {:?}", result.hessian.diagonal_elements());
println!("Natural gradient: {:?}", result.natural_grad);
```

### Hessian Matrix Operations

```rust
let mut h = HessianMatrix::diagonal(100);

// Set diagonal elements
h.set(0, 0, 1.5);
h.add(1, 1, 0.5);

// Get elements
let h_00 = h.get(0, 0);
let h_01 = h.get(0, 1);  // 0 for diagonal matrix

// Hessian-vector product
let v = vec![1.0; 100];
let hv = h.hvp(&v);

// Extract diagonal
let diag = h.diagonal_elements();
```

## Sequence-Level Loss Functions

Combine components for end-to-end training.

### ASG Loss (Auto Segmentation)

```rust
// Constrained graph: A = E ∘ (B ∘ target)
let constrained = compose(&emissions, &compose(&bigram, &target_graph));

// Normalization graph: Z = E ∘ B
let normalization = compose(&emissions, &bigram);

// Loss = -log P(Y|X) = Z - A
let a_score = forward_score(&GradientWfst::from_wfst(&constrained));
let z_score = forward_score(&GradientWfst::from_wfst(&normalization));

let loss = z_score.value() - a_score.value();
```

### CTC Loss with Token Graphs

```rust
// Build CTC topology
let ctc_config = TokenGraphConfig {
    graph_type: TokenGraphType::Standard,
    blank_id: 0,
    ..Default::default()
};
let token = 1; // token ID this CTC graph accepts
let ctc_graph = build_token_graph(token, &ctc_config);

// Compose: E ∘ CTC ∘ target
let constrained = compose(&emissions, &compose(&ctc_graph, &target));
let a_score = forward_score(&GradientWfst::from_wfst(&constrained));

// Normalization: E ∘ CTC ∘ Σ*
let normalization = compose(&emissions, &ctc_graph);
let z_score = forward_score(&GradientWfst::from_wfst(&normalization));

let loss = z_score.value() - a_score.value();
```

## Performance Tips

1. **Use Diagonal Hessian**: Full Hessian is $`O(\lvert E\rvert^2)`$, diagonal is $`O(\lvert E\rvert)`$

2. **Prune Aggressively**: `min_count=10` gives 87× speedup with no accuracy loss

3. **Choose Right Token Graph**:
   - Wide context models → Spike CTC
   - Short context → Standard CTC

4. **Batch Marginalization**: Use `MarginalizationContext` for efficient batching

5. **WFST Conv Position**: Best as first layer where input dimension is large

## Related Topics

- [Differentiable WFSTs](differentiable.md): Core gradient infrastructure
- [CTC Topologies](ctc-topologies.md): Graph construction details
- [ASR Pipeline](asr-pipeline.md): Full speech recognition system
- [Beam Optimization](beam-optimization.md): Efficient decoding

## References

- [Hannun 2020](../BIBLIOGRAPHY.md#ref-hannun2020) — *Differentiable Weighted Finite-State Transducers* (ICML 2020): the autograd-through-WFST framework this module implements — WFSTs as differentiable layers, convolutional WFST kernels, marginalized word-piece decompositions via the alignment graph $`A = E \circ (B \circ ((T_1+\dots+T_C)^* \circ (L \circ Y)))`$, the token-graph (CTC variant) topologies, and pruned n-gram transitions with back-off.
- [Graves 2006](../BIBLIOGRAPHY.md#ref-graves2006) — *Connectionist Temporal Classification*: the blank-augmented, alignment-free training objective whose WFST realization is the Standard token graph (and whose variants — Spike, Duration-Limited, Equally-Spaced — this module parameterizes); the $`\text{loss} = \operatorname{forwardScore}(Z) - \operatorname{forwardScore}(A)`$ log-sum-exp marginalization is the CTC forward computation expressed over composed transducers.
