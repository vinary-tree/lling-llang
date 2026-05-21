# Top-Down Automatic Differentiation

Top-down automatic differentiation (autograd) computes gradients through WFST operations at the algorithm level rather than tracking individual primitive operations. This approach, pioneered by the k2 framework, offers better numerical stability and computational efficiency for sequence-level training.

## Background

### Bottom-Up vs. Top-Down Differentiation

**Bottom-Up (Traditional)**: PyTorch and TensorFlow track every primitive operation, building a computation graph that is traversed backward to compute gradients.

```
Forward:  x → op1 → y → op2 → z → op3 → loss
Backward: ∂L/∂x ← ∂op1 ← ∂L/∂y ← ∂op2 ← ∂L/∂z ← ∂op3 ← 1
```

**Top-Down (k2-style)**: For WFST operations, we compute gradients using mathematical properties of the algorithms (forward-backward scores) rather than tracking primitive operations.

```
Forward:  Compute α (forward) and β (backward) scores
Backward: Gradient = -posterior × output_grad
          where posterior = exp(α[src] + w + β[dst] - Z)
```

### Why Top-Down Works Better for WFSTs

1. **Numerical Stability**: Log-domain forward-backward avoids underflow/overflow
2. **Efficiency**: Single forward-backward pass computes all arc posteriors
3. **Sparsity**: Most arcs have zero gradient after pruning; sparse representation saves memory
4. **Pruning Compatibility**: Gradients naturally respect beam search decisions

## Core Concepts

### Forward-Backward Algorithm

The forward-backward algorithm computes the probability of each arc being used in any accepting path:

```
Forward Scores (α):
  α[start] = 0  (log domain: probability 1)
  α[s] = log Σ exp(α[prev] + w(prev→s))  for each incoming arc

Backward Scores (β):
  β[final] = final_weight
  β[s] = log Σ exp(w(s→next) + β[next])  for each outgoing arc

Total Log-Probability (Z):
  Z = α[start] + β[start]  (or α[any] + β[any] for acyclic)
```

Visually:

```
                    α[s]                      β[s]
                      │                         │
    ┌─────────────────┼─────────────────────────┼─────────────────┐
    │                 ▼                         ▼                 │
    │  (start) ════► (s) ─────w(arc)─────► (t) ══════► (final)   │
    │     │           │                     │             │       │
    │     └───────────┴─────────────────────┴─────────────┘       │
    │         Total path weight through arc:                       │
    │         α[s] + w(arc) + β[t]                                │
    └─────────────────────────────────────────────────────────────┘
```

### Arc Posteriors

The posterior probability of an arc is the probability it appears in a random path sampled according to path weights:

```
P(arc | observation) = exp(α[src] + w(arc) + β[dst] - Z)
```

Where:
- `α[src]` = log-probability of reaching the arc's source state
- `w(arc)` = arc weight (in log domain)
- `β[dst]` = log-probability of reaching a final state from destination
- `Z` = total log-probability (partition function)

For negative log-likelihood loss, the gradient with respect to arc weight is:

```
∂Loss/∂w(arc) = -P(arc | observation)
```

### Sparse Gradient Representation

After beam pruning, most arcs have zero posterior (they were pruned). Storing gradients sparsely saves memory:

```rust
pub struct SparseGradient {
    /// Map from arc ID to gradient value
    gradients: HashMap<usize, f64>,

    /// Total number of arcs (for sparsity calculation)
    num_arcs: usize,
}
```

Typical sparsity after beam search: 95-99% of arcs have zero gradient.

## API Reference

### Forward-Backward Scores

```rust
pub struct ForwardBackwardScores {
    /// Forward log-probabilities α[state]
    pub alpha: Vec<f64>,

    /// Backward log-probabilities β[state]
    pub beta: Vec<f64>,

    /// Total log-probability Z
    pub total_log_prob: f64,
}

impl ForwardBackwardScores {
    /// Compute arc posterior probability
    pub fn arc_posterior(&self, alpha_src: f64, arc_weight: f64, beta_dst: f64) -> f64 {
        (alpha_src + arc_weight + beta_dst - self.total_log_prob).exp()
    }
}
```

### Sparse Gradients

```rust
impl SparseGradient {
    /// Create empty gradient
    pub fn new(num_arcs: usize) -> Self;

    /// Set gradient for an arc (filters values below threshold)
    pub fn set(&mut self, arc_id: usize, value: f64);

    /// Get gradient (returns 0.0 for untracked arcs)
    pub fn get(&self, arc_id: usize) -> f64;

    /// Add another sparse gradient
    pub fn add_sparse(&mut self, other: &SparseGradient);

    /// Convert to dense vector
    pub fn to_dense(&self) -> Vec<f64>;

    /// Sparsity ratio (1 - nnz/total)
    pub fn sparsity(&self) -> f64;
}
```

### Composed WFST Gradients

When computing gradients through composed WFSTs, we need to distribute gradients back to both input WFSTs:

```rust
/// Information about a composed arc's origin
pub struct ComposedArcInfo {
    /// Source state in composed FST
    pub source: StateId,

    /// Destination state in composed FST
    pub dest: StateId,

    /// Combined weight (w1 + w2 in log domain)
    pub log_weight: f64,

    /// Arc index in first FST (None for epsilon)
    pub arc1: Option<usize>,

    /// Arc index in second FST (None for epsilon)
    pub arc2: Option<usize>,
}

/// Mapping from composed arcs to original arcs
pub struct ComposedArcMap {
    arc_origins: HashMap<usize, (Option<usize>, Option<usize>)>,
    arc_info: Vec<ComposedArcInfo>,
}

impl ComposedArcMap {
    /// Record a composed arc with full metadata
    pub fn add_with_info(
        &mut self,
        source: StateId,
        dest: StateId,
        log_weight: f64,
        arc1: Option<usize>,
        arc2: Option<usize>,
    );

    /// Iterate over arc information
    pub fn arc_infos(&self) -> impl Iterator<Item = &ComposedArcInfo>;

    /// Check if detailed arc info is available
    pub fn has_arc_info(&self) -> bool;
}
```

### Backward Pass Function

```rust
/// Compute gradients through a composed WFST
pub fn composed_backward(
    fst1: &impl Wfst,
    fst2: &impl Wfst,
    composed_fb: &ForwardBackwardScores,
    arc_map: &ComposedArcMap,
    output_grad: f64,
) -> ComposedBackwardResult {
    let mut grad1 = SparseGradient::new(fst1.num_arcs());
    let mut grad2 = SparseGradient::new(fst2.num_arcs());

    for arc_info in arc_map.arc_infos() {
        // Compute posterior for this composed arc
        let posterior = composed_fb.arc_posterior(
            composed_fb.alpha[arc_info.source as usize],
            arc_info.log_weight,
            composed_fb.beta[arc_info.dest as usize],
        );

        // Gradient = -posterior × output_grad
        let grad_value = -posterior * output_grad;

        // Distribute to original arcs
        if let Some(arc1) = arc_info.arc1 {
            grad1.add(arc1, grad_value);
        }
        if let Some(arc2) = arc_info.arc2 {
            grad2.add(arc2, grad_value);
        }
    }

    ComposedBackwardResult { grad1, grad2, stats }
}
```

## Examples

### Computing Gradients Through a Single WFST

```rust
use lling_llang::differentiable::{forward_backward, topdown_backward};
use lling_llang::wfst::VectorWfst;
use lling_llang::semiring::LogWeight;

// Build or load a WFST
let fst: VectorWfst<Label, LogWeight> = /* ... */;

// Compute forward-backward scores
let fb = forward_backward(&fst);

println!("Total log-prob: {}", fb.total_log_prob);
println!("Loss (NLL): {}", -fb.total_log_prob);

// Compute gradients (output_grad = 1.0 for NLL loss)
let gradients = topdown_backward(&fst, &fb, 1.0);

// Inspect gradient sparsity
println!("Gradient sparsity: {:.1}%", gradients.sparsity() * 100.0);

// Access individual arc gradients
for arc_id in 0..fst.num_arcs() {
    let grad = gradients.get(arc_id);
    if grad.abs() > 1e-6 {
        println!("Arc {}: gradient = {:.6}", arc_id, grad);
    }
}
```

### Gradients Through Composed WFSTs

```rust
use lling_llang::differentiable::{composed_backward, ComposedArcMap};
use lling_llang::composition::{compose, materialize};

// Two WFSTs to compose
let acoustic_fst: VectorWfst<Label, LogWeight> = /* from neural network */;
let language_model: VectorWfst<Label, LogWeight> = /* n-gram LM */;

// Compose with arc tracking
let (composed, arc_map) = compose_with_tracking(&acoustic_fst, &language_model);

// Forward-backward on composed result
let composed_fb = forward_backward(&composed);

// Compute gradients for both inputs
let result = composed_backward(
    &acoustic_fst,
    &language_model,
    &composed_fb,
    &arc_map,
    1.0,  // output_grad for NLL loss
);

// Gradients flow back to acoustic model
println!("Acoustic FST gradients: {} non-zero", result.grad1.num_nonzero());
println!("LM gradients: {} non-zero", result.grad2.num_nonzero());

// Use acoustic gradients to update neural network
let acoustic_grads = result.grad1.to_dense();
```

### Integration with Neural Network Training

```rust
// Pseudocode for training loop
fn train_step(
    encoder: &mut NeuralEncoder,
    decoder_fst: &VectorWfst<Label, LogWeight>,
    audio_features: &Tensor,
    targets: &[Label],
) -> f64 {
    // Forward pass through neural network
    let logits = encoder.forward(audio_features);

    // Build dense FSA from logits
    let acoustic_fsa = build_dense_fsa(&logits);

    // Compose with decoder (LM, lexicon, etc.)
    let (composed, arc_map) = compose_with_tracking(&acoustic_fsa, decoder_fst);

    // Intersect with target sequence
    let (final_fst, target_map) = intersect_with_tracking(&composed, targets);

    // Forward-backward for loss and posteriors
    let fb = forward_backward(&final_fst);
    let loss = -fb.total_log_prob;

    // Top-down backward pass
    let grads = composed_backward(&acoustic_fsa, decoder_fst, &fb, &arc_map, 1.0);

    // Convert sparse gradients to dense tensor
    let grad_tensor = grads.grad1.to_tensor(logits.shape());

    // Backward through neural network
    encoder.backward(&grad_tensor);

    loss
}
```

## Advanced Topics

### Pruned Search Backward

When using beam search, only paths within the beam survive. The backward pass should only compute gradients for surviving paths:

```rust
pub struct PrunedSearchResult {
    /// States that survived pruning
    pub surviving_states: HashSet<StateId>,

    /// Arcs that survived pruning
    pub surviving_arcs: HashSet<usize>,

    /// Forward scores (only for surviving states)
    pub alpha: HashMap<StateId, f64>,
}

/// Backward pass only through pruned subgraph
pub fn pruned_backward(
    fst: &impl Wfst,
    search_result: &PrunedSearchResult,
    output_grad: f64,
) -> SparseGradient {
    // Only compute β for surviving states
    // Only compute posteriors for surviving arcs
    // Massive memory savings for large vocabularies
}
```

### Numerical Stability Considerations

All computations use log-domain arithmetic:

```rust
/// Numerically stable log-addition
fn log_add(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY { return b; }
    if b == f64::NEG_INFINITY { return a; }

    let (max, min) = if a > b { (a, b) } else { (b, a) };
    max + (min - max).exp().ln_1p()
}

/// Numerically stable log-subtraction (when a > b)
fn log_sub(a: f64, b: f64) -> f64 {
    if b == f64::NEG_INFINITY { return a; }
    a + (-( (b - a).exp() )).ln_1p()
}
```

Key stability practices:
1. **Never exponentiate then log**: Work in log domain throughout
2. **Use log-sum-exp trick**: Factor out maximum before summing
3. **Threshold small posteriors**: Ignore arcs with posterior < 1e-10
4. **Check for infinities**: Handle -∞ (zero probability) gracefully

### Gradient Accumulation for Multiple Paths

When multiple composed arcs map to the same original arc, gradients accumulate:

```rust
// Multiple composed arcs might originate from the same arc in fst1
for arc_info in arc_map.arc_infos() {
    if let Some(arc1) = arc_info.arc1 {
        // Gradients ADD, not replace
        grad1.add(arc1, grad_value);  // += not =
    }
}
```

This correctly handles:
- Self-loops (same arc used multiple times)
- Epsilon-matching arcs (one arc matches multiple in other FST)
- Shared structure (determinized FSTs with merged states)

### Memory-Efficient Backward for Large Graphs

For very large composed graphs, compute gradients in chunks:

```rust
fn chunked_backward(
    arc_map: &ComposedArcMap,
    fb: &ForwardBackwardScores,
    chunk_size: usize,
) -> SparseGradient {
    let mut total_grad = SparseGradient::new(num_arcs);

    for chunk in arc_map.arc_infos().chunks(chunk_size) {
        let chunk_grad = compute_chunk_gradients(chunk, fb);
        total_grad.add_sparse(&chunk_grad);
    }

    total_grad
}
```

## Comparison with Bottom-Up Autograd

| Aspect | Bottom-Up | Top-Down |
|--------|-----------|----------|
| **Memory** | O(ops × state) | O(states + arcs) |
| **Numerical Stability** | Prone to underflow | Log-domain stable |
| **Pruning** | Awkward | Natural |
| **Implementation** | Framework-dependent | Algorithm-specific |
| **Debugging** | Tape inspection | Forward-backward scores |

## Related Documentation

- [Differentiable Operations](differentiable.md) - Overview of gradient computation
- [Weak Supervision](../training/weak-supervision.md) - WST training with bypass arcs
- [Path Sampling](../algorithms/path-sampling.md) - Monte Carlo gradient estimation
- [Composition](../algorithms/composition.md) - WFST composition algorithms

## References

- [k2-fsa/k2 GitHub](https://github.com/k2-fsa/k2) - Differentiable FSA/FST framework
- [k2 Documentation](https://k2-fsa.org/) - Official k2 documentation
