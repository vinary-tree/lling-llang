# RRWM Algorithm

The Rational Randomized Weighted-Majority (RRWM) algorithm is an online learning method for ensemble prediction using WFST-based path experts ([Cortes 2015](../BIBLIOGRAPHY.md#ref-cortes2015)). It provides principled combination of multiple models with guaranteed regret bounds. (WFST = **W**eighted **F**inite-**S**tate **T**ransducer.)

## Terms & symbols

Defined centrally in [`../NOTATION.md`](../NOTATION.md); repeated locally for the terms this doc uses.

| Symbol | Meaning |
|---|---|
| $`\circ`$ | composition — combines the cumulative automaton with the round's loss transducer. |
| $`\oplus`$ / $`\otimes`$ | semiring *plus* / *times* over the $`\eta`$-power semiring. |
| $`\bar{1}`$ | the $`\otimes`$-identity ($`W_0`$ is the one-state automaton, all paths weight $`\bar{1}`$). |
| $`\eta`$ | power-semiring exponent — the online-learning temperature (smaller = more exploration). |
| $`W_t`$, $`V_t`$ | cumulative weight automaton / loss transducer at round $`t`$. |
| $`R_T`$ | total regret after $`T`$ rounds; $`M`$ = max loss/round, $`N`$ = number of experts. |
| $`\lvert W_t\rvert`$, $`\lvert V_t\rvert`$ | sizes used in the per-round composition bound (cardinality bar $`\lvert\cdot\rvert`$). |

## Concepts

### Online Learning Problem

In online learning, we face a sequence of rounds:

```text
Round 1: Receive input → Make prediction → Observe loss
Round 2: Receive input → Make prediction → Observe loss
...
Round T: Receive input → Make prediction → Observe loss
```

The goal is to minimize **regret**: the difference between our cumulative loss and the loss of the best single expert in hindsight.

### Path Experts

A **path expert** is a WFST that maps inputs to outputs. Given an input, each expert proposes a prediction (an accepting path). Different experts may propose different paths with different weights.

```
Expert 1: "teh" → "the" (weight 0.9)
Expert 2: "teh" → "tea" (weight 0.7)
Expert 3: "teh" → "ten" (weight 0.8)
```

RRWM combines these experts, learning which ones to trust over time.

### Algorithm Overview

RRWM maintains a **cumulative weight automaton** $`W_t`$ that tracks performance. Each
round predicts by sampling $`W_{t-1}`$, observes a loss transducer $`V_t`$, composes it
in, and weight-pushes to restore a stochastic automaton for the next sample.

![RRWM online learning loop: starting from W₀ (one-state, all paths weight 1̄), each round predicts by sampling Wₜ₋₁, builds a loss transducer Vₜ, composes Wₜ ← Wₜ₋₁ ∘ Vₜ, weight-pushes to stochastic, and accumulates loss until T rounds or a state cap triggers reset](../diagrams/algorithms/rrwm-loop.svg)

*Purple = the learner's predict step (sampling); algorithms-green = the WFST operations it drives (compose, weight-push); the loop runs until round $`T`$ or a state cap forces a reset to $`W_0`$. Regret bound $`E[R_T] \le 2M\sqrt{T \log N}`$.*

<details><summary>Text view</summary>

```text
RRWM(T rounds):
    W₀ ← one-state automaton (all paths weight 1̄)
    for t = 1 to T:
        1. Receive input and loss transducer Vₜ
        2. Compose: Wₜ ← Wₜ₋₁ ∘ Vₜ
        3. Push weights to make Wₜ stochastic
        4. Sample prediction from Wₜ
    return W_T
```

</details>

The literate chunks below name each phase of one round.

```text
⟨ initialize cumulative automaton ⟩ ≡
    W₀ ← one-state automaton over the η-power semiring   // all paths weight 1̄
```

```text
⟨ one RRWM round t ⟩ ≡
    ŷₜ ← sample(Wₜ₋₁)                 // predict: draw a path ∝ its weight
    Vₜ ← loss_transducer(observe yₜ)  // build losses for each candidate output
    Wₜ ← Wₜ₋₁ ∘ Vₜ                    // compose: fold losses into every path expert
    Wₜ ← weight_push(Wₜ)              // restore stochastic (out-weights sum to 1̄)
```

```text
⟨ RRWM online learning ⟩ ≡
    ⟨ initialize cumulative automaton ⟩
    for t = 1 to T:
        ⟨ one RRWM round t ⟩
        if states(Wₜ) > max_rounds:  Wₜ ← W₀     // cap growth
    return W_T
```

The key insight is that WFST composition efficiently combines all paths across all
experts in one structure, so the weighted-majority update is a single $`\circ`$-then-push.

### Regret Bound

RRWM achieves the theoretical regret bound $`E[R_T] \le 2M\sqrt{T \log N}`$:

```math
E[R_T] \le 2M\sqrt{T \log N}
```

Where:
- $`R_T`$: Total regret after $`T`$ rounds
- $`M`$: Maximum loss per round
- $`T`$: Number of rounds
- $`N`$: Number of path experts

This bound is **sublinear** in $`T`$, meaning average regret decreases over time ([Cortes 2015](../BIBLIOGRAPHY.md#ref-cortes2015)).

### Connection to Power Semiring

RRWM operates over the $`\eta`$-power semiring to enable:
- Smooth weight updates (vs hard thresholding)
- Proper probability distributions after weight pushing
- Rational loss functions (hence "Rational" in RRWM)

## Core API

### Configuration

```rust
use lling_llang::algorithms::{RrwmConfig, Rrwm};

let config = RrwmConfig::default()
    .eta(1.0)               // η parameter for power semiring
    .learning_rate(1.0)     // Learning rate multiplier
    .max_rounds(100_000)    // Maximum rounds before reset
    .with_statistics()      // Enable statistics tracking
    .seed(42);              // Random seed for reproducibility

let rrwm = Rrwm::<char>::new(config);
```

### Configuration Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `eta` | 1.0 | Power semiring $`\eta`$ (smaller = more exploration) |
| `learning_rate` | 1.0 | Multiplier for weight updates |
| `max_rounds` | 100,000 | Reset trigger (prevents unbounded automaton growth) |
| `track_statistics` | false | Enable detailed statistics |
| `seed` | None | Random seed for sampling |

### Main Methods

| Method | Description |
|--------|-------------|
| `observe(loss_transducer)` | Update weights with observed loss |
| `predict()` | Sample a prediction from current distribution |
| `regret_bound(max_loss, num_experts)` | Compute theoretical regret bound |
| `reset()` | Reset to initial state |
| `round()` | Current round number |
| `cumulative_weights()` | Access the cumulative automaton |
| `statistics()` | Get learning statistics (if enabled) |

### Error Handling

```rust
use lling_llang::algorithms::RrwmError;

match rrwm.observe(&loss_transducer) {
    Ok(loss) => println!("Observed loss: {}", loss),
    Err(RrwmError::MaxRoundsExceeded) => println!("Need to reset"),
    Err(RrwmError::PushFailed(msg)) => println!("Push failed: {}", msg),
    Err(RrwmError::SampleFailed(e)) => println!("Sampling failed: {:?}", e),
    Err(RrwmError::EmptyComposition) => println!("No valid paths"),
    Err(RrwmError::ConfigError(msg)) => println!("Config error: {}", msg),
}
```

### Statistics

```rust
let rrwm = Rrwm::<char>::new(RrwmConfig::default().with_statistics());

// After some observations...
if let Some(stats) = rrwm.statistics() {
    println!("Rounds: {}", stats.rounds);
    println!("Total loss: {}", stats.total_loss);
    println!("Average loss: {}", stats.average_loss);
    println!("Cumulative states: {}", stats.cumulative_states);
    println!("Loss history: {:?}", stats.loss_history);
}
```

## Examples

### Basic Online Learning Loop

```rust
use lling_llang::algorithms::{Rrwm, RrwmConfig};
use lling_llang::semiring::PowerWeight;
use lling_llang::wfst::{VectorWfst, MutableWfst};

// Create RRWM learner
let config = RrwmConfig::default()
    .eta(1.0)
    .with_statistics()
    .seed(42);
let mut rrwm = Rrwm::<char>::new(config);

// Simulate online learning
for round in 0..100 {
    // 1. Make prediction
    match rrwm.predict() {
        Ok(prediction) => {
            println!("Round {}: predicted {:?}", round, prediction.output_string());
        }
        Err(e) => {
            println!("Prediction failed: {:?}", e);
        }
    }

    // 2. Construct loss transducer based on actual outcome
    let loss_transducer = create_loss_transducer(/* actual outcome */);

    // 3. Observe loss and update
    match rrwm.observe(loss_transducer) {
        Ok(loss) => {
            println!("Round {}: loss = {:.4}", round, loss);
        }
        Err(e) => {
            println!("Observation failed: {:?}", e);
            break;
        }
    }
}

// Check final statistics
if let Some(stats) = rrwm.statistics() {
    println!("Final average loss: {:.4}", stats.average_loss);
}

fn create_loss_transducer() -> VectorWfst<char, PowerWeight> {
    let eta = 1.0;
    let mut wfst = VectorWfst::new();
    let s0 = wfst.add_state();
    let s1 = wfst.add_state();
    wfst.set_start(s0);
    wfst.set_final(s1, PowerWeight::one_with_eta(eta));
    wfst.add_arc(s0, Some('a'), Some('a'), s1,
        PowerWeight::from_probability(0.9, eta));
    wfst
}
```

### Setting Up Path Experts

```rust
use lling_llang::algorithms::{Rrwm, RrwmBuilder, RrwmConfig};
use lling_llang::semiring::PowerWeight;
use lling_llang::wfst::{VectorWfst, MutableWfst};

fn create_expert(label: char, weight: f64, eta: f64) -> VectorWfst<char, PowerWeight> {
    let mut wfst = VectorWfst::new();
    let s0 = wfst.add_state();
    let s1 = wfst.add_state();
    wfst.set_start(s0);
    wfst.set_final(s1, PowerWeight::one_with_eta(eta));
    wfst.add_arc(s0, Some('?'), Some(label), s1,
        PowerWeight::from_probability(weight, eta));
    wfst
}

// Create multiple path experts
let eta = 2.0;
let expert_a = create_expert('a', 0.8, eta);  // Predicts 'a' with weight 0.8
let expert_b = create_expert('b', 0.6, eta);  // Predicts 'b' with weight 0.6
let expert_c = create_expert('c', 0.9, eta);  // Predicts 'c' with weight 0.9

// Build RRWM with initial experts
let rrwm = RrwmBuilder::<char>::new()
    .eta(eta)
    .add_expert(expert_a)
    .add_expert(expert_b)
    .add_expert(expert_c)
    .build();

println!("RRWM initialized with η = {}", rrwm.eta());
```

### Computing Regret Bounds

```rust
use lling_llang::algorithms::{Rrwm, RrwmConfig};

let mut rrwm = Rrwm::<char>::new(RrwmConfig::default());

// Simulate 100 rounds
rrwm.round = 100;  // (normally incremented by observe())

let max_loss = 1.0;    // Maximum loss per round
let num_experts = 10;  // Number of path experts

let bound = rrwm.regret_bound(max_loss, num_experts);
println!("Regret bound after {} rounds: {:.2}", rrwm.round(), bound);

// The bound is: 2 * M * sqrt(T * ln(N))
// = 2 * 1.0 * sqrt(100 * ln(10)) ≈ 30.3
```

### Controlling Exploration vs Exploitation

```rust
use lling_llang::algorithms::{Rrwm, RrwmConfig};

// More exploration (smaller η)
let exploratory = Rrwm::<char>::new(
    RrwmConfig::default().eta(0.5)
);
println!("Exploratory η: {}", exploratory.eta());

// More exploitation (larger η)
let exploitative = Rrwm::<char>::new(
    RrwmConfig::default().eta(3.0)
);
println!("Exploitative η: {}", exploitative.eta());

// The η parameter affects:
// - How weights are combined (soft vs hard)
// - The sampling distribution (uniform-like vs greedy)
```

### Resetting the Learner

```rust
use lling_llang::algorithms::{Rrwm, RrwmConfig};

let mut rrwm = Rrwm::<char>::new(RrwmConfig::default().with_statistics());

// Run for a while...
// (rrwm.round increments with each observe())

// Reset to initial state
rrwm.reset();

assert_eq!(rrwm.round(), 0);
assert_eq!(rrwm.cumulative_weights().num_states(), 1);
if let Some(stats) = rrwm.statistics() {
    assert_eq!(stats.rounds, 0);
    assert_eq!(stats.total_loss, 0.0);
}
println!("RRWM reset to initial state");
```

### Accessing Cumulative Weights

```rust
use lling_llang::algorithms::{Rrwm, RrwmConfig};
use lling_llang::wfst::Wfst;

let rrwm = Rrwm::<char>::new(RrwmConfig::default());

// Access the cumulative weight automaton
let cumulative = rrwm.cumulative_weights();

println!("States: {}", cumulative.num_states());
println!("Start state: {}", cumulative.start());

// The cumulative automaton encodes learned expert weights
// and can be used for inspection or debugging
```

## Algorithm Details

### Weight Update Mechanism

At each round, weights are updated via WFST composition:

```math
W_t = \operatorname{WeightPush}(W_{t-1} \circ V_t)
```

Where:
- $`W_{t-1}`$ is the cumulative automaton from the previous round
- $`V_t`$ is the loss transducer for round $`t`$
- $`\circ`$ denotes WFST composition
- `WeightPush` makes the result stochastic for sampling

### Loss Transducer Construction

The loss transducer $`V_t`$ encodes the loss for each possible prediction:

```
Input: actual outcome y
Output: V_t such that V_t(ŷ) = loss(ŷ, y)

For correct prediction: low loss (high weight)
For incorrect prediction: high loss (low weight)
```

### Sampling Predictions

After weight pushing, the cumulative automaton is stochastic (outgoing weights sum to 1). Predictions are sampled proportional to weights:

```math
P(\text{path } \pi) \propto W_t(\pi)
```

Better-performing paths get higher probability over time.

## When to Use RRWM

**Choose RRWM when:**

| Scenario | Why RRWM? |
|----------|-----------|
| Combining multiple WFST models | Principled ensemble learning |
| Online adaptation | Updates incrementally, no batch retraining |
| Theoretical guarantees needed | Provable regret bounds |
| Non-additive losses | Handles edit distance, tropical losses |
| Streaming data | Constant memory per update |

**Consider alternatives when:**

| Scenario | Alternative |
|----------|-------------|
| Single best model known | Use that model directly |
| Batch training available | Train ensemble offline |
| Very few rounds | Limited learning opportunity |
| Memory constrained | Cumulative automaton grows |

## Relationship to Other Algorithms

```text
                    ┌─────────────────────┐
                    │      RRWM           │
                    │ (Online Learning)   │
                    └──────────┬──────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
              ▼                ▼                ▼
    ┌─────────────────┐ ┌─────────────┐ ┌─────────────────┐
    │ Power Semiring  │ │Path Sampling│ │ Weight Pushing  │
    │ (η-soft weights)│ │(predictions)│ │(make stochastic)│
    └─────────────────┘ └─────────────┘ └─────────────────┘
```

## Complexity Analysis

| Operation | Complexity |
|-----------|------------|
| `observe()` | $`O(\lvert W_{t-1}\rvert \times \lvert V_t\rvert)`$ composition |
| `predict()` | $`O(\text{path length})`$ sampling |
| `regret_bound()` | $`O(1)`$ |
| `reset()` | $`O(1)`$ |
| Space | $`O(\lvert W_t\rvert)`$ cumulative automaton |

## References

- [Cortes 2015](../BIBLIOGRAPHY.md#ref-cortes2015) — Cortes, C., Kuznetsov, V., Mohri, M., & Warmuth, M. K. (2015). *On-Line Learning Algorithms for Path Experts with Non-Additive Losses.* COLT 2015, PMLR 40:424–447. Figure 6 defines RRWM; the $`E[R_T] \le 2M\sqrt{T \log N}`$ regret bound and the compose-then-push update are from this work.
- [Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) — *Weighted Automata Algorithms*: the composition and weight-pushing primitives RRWM drives each round.

## Related Documentation

- [Power Semiring](../architecture/power-semiring.md) - The $`\eta`$-power semiring used by RRWM
- [Path Sampling](path-sampling.md) - How predictions are sampled
- [Weight Pushing](weight-pushing.md) - Making automata stochastic
- [Composition](composition.md) - How loss transducers are combined
