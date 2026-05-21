# Signed Tropical Semiring

The signed tropical semiring extends the standard tropical semiring to allow negative weights, enabling representation of **rewards** (negative costs) alongside **penalties** (positive costs).

## Concepts

### What is the Signed Tropical Semiring?

The **signed tropical semiring** (ℝ ∪ {±∞}, min, +, +∞, 0) operates over the full real number line, unlike the standard tropical semiring which is restricted to non-negative values.

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| ⊕ | min(a, b) | Pick the better (lower) weight |
| ⊗ | a + b | Accumulate costs/rewards |
| 0̄ | +∞ | Unreachable (infinite cost) |
| 1̄ | 0 | Neutral (no cost, no reward) |

### Why Signed Tropical?

The standard `TropicalWeight` restricts weights to non-negative values. This limitation prevents modeling scenarios where you want to **reward** certain behaviors:

| Weight Type | Meaning | Example |
|-------------|---------|---------|
| Positive (+) | Cost/Penalty | Edit distance, error penalty |
| Zero (0) | Neutral | Free operation |
| Negative (-) | Reward/Bonus | Fluency bonus, preferred path |

**Use cases:**
- **Language model scoring**: Bonuses for fluent phrases
- **Preference modeling**: Rewards for user-preferred outputs
- **Bidirectional optimization**: Balance costs and rewards in a single framework
- **Game-theoretic applications**: Minimax-style scoring
- **Reinforcement learning**: Combine costs and rewards on WFST paths

### Comparison with Standard Tropical

```
Standard Tropical:  (ℝ₊ ∪ {∞}, min, +, ∞, 0)
Signed Tropical:    (ℝ ∪ {±∞}, min, +, +∞, 0)
                        ↑
                   Full real line (allows negatives)
```

| Feature | TropicalWeight | SignedTropicalWeight |
|---------|---------------|----------------------|
| Positive weights | Yes | Yes |
| Negative weights | No | Yes |
| Star operation | Always converges | Diverges for negatives |
| Dijkstra-safe | Yes | No (for negative weights) |
| Conversion | → SignedTropical always | → Tropical fails if negative |

## Core API

### SignedTropicalWeight

```rust
use lling_llang::semiring::{Semiring, SignedTropicalWeight};

// Create weights
let cost = SignedTropicalWeight::new(2.5);      // Positive: cost
let reward = SignedTropicalWeight::new(-1.0);   // Negative: reward
let neutral = SignedTropicalWeight::new(0.0);   // Zero: neutral

// Special values
let unreachable = SignedTropicalWeight::infinity();      // +∞
let infinite_reward = SignedTropicalWeight::neg_infinity(); // -∞

// Query properties
assert!(cost.is_nonnegative());
assert!(reward.is_negative());
assert!(cost.is_finite());
assert!(unreachable.is_pos_infinite());
```

### Semiring Operations

```rust
use lling_llang::semiring::{Semiring, SignedTropicalWeight};

let a = SignedTropicalWeight::new(2.0);
let b = SignedTropicalWeight::new(3.0);
let c = SignedTropicalWeight::new(-1.0);  // Reward

// Plus (⊕): minimum (pick best weight)
assert_eq!(a.plus(&b), a);  // min(2, 3) = 2
assert_eq!(a.plus(&c), c);  // min(2, -1) = -1 (reward wins!)

// Times (⊗): addition (accumulate along path)
assert_eq!(a.times(&b), SignedTropicalWeight::new(5.0));  // 2 + 3 = 5
assert_eq!(a.times(&c), SignedTropicalWeight::new(1.0));  // 2 + (-1) = 1

// Identity elements
assert_eq!(a.plus(&SignedTropicalWeight::zero()), a);   // a ⊕ +∞ = a
assert_eq!(a.times(&SignedTropicalWeight::one()), a);   // a ⊗ 0 = a
```

### The Divergence Problem

The star operation w* = 1 ⊕ w ⊕ w² ⊕ ... computes the Kleene closure. For signed tropical:

```
w* = min(0, w, 2w, 3w, ...)
```

- **If w ≥ 0**: Sequence is non-decreasing, minimum is 0 → w* = 0 (converges)
- **If w < 0**: Sequence decreases without bound → w* = -∞ (diverges!)

```rust
use lling_llang::semiring::{SignedTropicalWeight, FallibleStarSemiring};

let positive = SignedTropicalWeight::new(2.0);
let negative = SignedTropicalWeight::new(-1.0);

// Positive: star converges
assert!(positive.star_defined());
assert_eq!(positive.star_checked(), Some(SignedTropicalWeight::one()));

// Negative: star diverges
assert!(!negative.star_defined());
assert_eq!(negative.star_checked(), None);
```

### FallibleStarSemiring Trait

Because star may diverge, `SignedTropicalWeight` implements `FallibleStarSemiring` instead of `StarSemiring`:

```rust
use lling_llang::semiring::{SignedTropicalWeight, FallibleStarSemiring, StarDivergenceError};

/// Trait for semirings where star may fail.
pub trait FallibleStarSemiring: Semiring {
    type Error;
    fn try_star(&self) -> Result<Self, Self::Error>;
}

// Usage
let w = SignedTropicalWeight::new(-1.0);

match w.try_star() {
    Ok(star) => println!("Star converged: {}", star),
    Err(StarDivergenceError) => println!("Star diverges for negative weights"),
}
```

### Algebraic Properties

`SignedTropicalWeight` implements these marker traits:

| Trait | Meaning | Implications |
|-------|---------|--------------|
| `IdempotentSemiring` | a ⊕ a = a | Shortest path algorithms work correctly |
| `CommutativeTimesSemiring` | a ⊗ b = b ⊗ a | Order of path composition doesn't matter |
| `TotallyOrderedSemiring` | Total ordering exists | Can use in determinization |
| `DivisibleSemiring` | Division defined | Weight pushing possible |
| `QuantizableSemiring` | Can quantize to integers | Minimization with floating-point tolerance |

### Division and Weight Operations

```rust
use lling_llang::semiring::{Semiring, DivisibleSemiring, SignedTropicalWeight};

let a = SignedTropicalWeight::new(5.0);
let b = SignedTropicalWeight::new(3.0);

// Division: subtraction in tropical semiring
// a ÷ b = c where c ⊗ b = a, i.e., c + b = a, so c = a - b
let quotient = a.divide(&b);
assert_eq!(quotient, Some(SignedTropicalWeight::new(2.0)));  // 5 - 3 = 2

// Negate: flip cost to reward
let neg = a.negate();
assert_eq!(neg, SignedTropicalWeight::new(-5.0));

// Absolute value
let abs = neg.abs();
assert_eq!(abs, a);

// Clamp to range
let clamped = a.clamp(-10.0, 3.0);
assert_eq!(clamped, SignedTropicalWeight::new(3.0));  // Clamped to max
```

## Examples

### Basic Path Cost with Rewards

A common use case: finding the best path where some edges have costs and others provide rewards.

```rust
use lling_llang::semiring::{Semiring, SignedTropicalWeight};

// Path 1: cost → reward → cost
//   Edge 1: cost 2.0
//   Edge 2: reward -1.5 (bonus for preferred transition)
//   Edge 3: cost 1.0
let edge1 = SignedTropicalWeight::new(2.0);
let edge2 = SignedTropicalWeight::new(-1.5);  // Reward!
let edge3 = SignedTropicalWeight::new(1.0);

let path1_total = edge1.times(&edge2).times(&edge3);
// 2.0 + (-1.5) + 1.0 = 1.5
assert_eq!(path1_total, SignedTropicalWeight::new(1.5));

// Path 2: all costs
let path2_total = SignedTropicalWeight::new(3.0);

// Best path: minimum weight
let best = path1_total.plus(&path2_total);
assert_eq!(best, path1_total);  // 1.5 < 3.0, path with reward wins!
```

### Language Model Scoring

Model fluent phrases as rewards:

```rust
use lling_llang::semiring::{Semiring, SignedTropicalWeight};

// Base edit costs
let substitution_cost = SignedTropicalWeight::new(1.0);
let insertion_cost = SignedTropicalWeight::new(0.8);

// Fluency rewards (negative = good)
let common_phrase_bonus = SignedTropicalWeight::new(-0.3);  // "going to"
let rare_word_penalty = SignedTropicalWeight::new(0.5);     // Unusual word

// Path: substitute + use common phrase
let path_with_bonus = substitution_cost.times(&common_phrase_bonus);
// 1.0 + (-0.3) = 0.7

// Path: substitute + rare word
let path_with_penalty = substitution_cost.times(&rare_word_penalty);
// 1.0 + 0.5 = 1.5

// Fluent path is preferred
assert!(path_with_bonus.value() < path_with_penalty.value());
```

### Conversion Between Tropical and Signed Tropical

```rust
use lling_llang::semiring::{TropicalWeight, SignedTropicalWeight};

// TropicalWeight → SignedTropicalWeight: Always succeeds
let tropical = TropicalWeight::new(2.5);
let signed: SignedTropicalWeight = tropical.into();
assert_eq!(signed.value(), 2.5);

// SignedTropicalWeight → TropicalWeight: May fail for negatives
let positive = SignedTropicalWeight::new(3.0);
let negative = SignedTropicalWeight::new(-1.0);

let result1: Result<TropicalWeight, _> = positive.try_into();
assert!(result1.is_ok());

let result2: Result<TropicalWeight, _> = negative.try_into();
assert!(result2.is_err());  // Cannot convert negative to TropicalWeight
```

### Handling Star Operation Safely

```rust
use lling_llang::semiring::{SignedTropicalWeight, FallibleStarSemiring};

fn compute_closure(weights: &[SignedTropicalWeight]) -> Result<Vec<SignedTropicalWeight>, &'static str> {
    let mut closures = Vec::new();

    for w in weights {
        match w.try_star() {
            Ok(star) => closures.push(star),
            Err(_) => return Err("Cannot compute closure: negative weight detected"),
        }
    }

    Ok(closures)
}

// Mixed weights
let weights = vec![
    SignedTropicalWeight::new(1.0),
    SignedTropicalWeight::new(0.0),
    SignedTropicalWeight::new(-0.5),
];

let result = compute_closure(&weights);
assert!(result.is_err());  // Failed due to negative weight
```

### Quantization for Hash-Based Algorithms

```rust
use lling_llang::semiring::{SignedTropicalWeight, QuantizableSemiring};

// Quantization converts floating-point to integer buckets
let w1 = SignedTropicalWeight::new(2.7);
let w2 = SignedTropicalWeight::new(2.8);
let w3 = SignedTropicalWeight::new(-1.3);

// With epsilon = 1.0, values are rounded to nearest integer
assert_eq!(w1.quantize(1.0), 3);   // 2.7 → 3
assert_eq!(w2.quantize(1.0), 3);   // 2.8 → 3 (same bucket!)
assert_eq!(w3.quantize(1.0), -1);  // -1.3 → -1

// With finer epsilon, more buckets
assert_eq!(w1.quantize(0.5), 5);   // 2.7 / 0.5 = 5.4 → 5
assert_eq!(w2.quantize(0.5), 6);   // 2.8 / 0.5 = 5.6 → 6 (different!)

// Special values
assert_eq!(SignedTropicalWeight::infinity().quantize(1.0), i64::MAX);
assert_eq!(SignedTropicalWeight::neg_infinity().quantize(1.0), i64::MIN + 1);
```

## Performance Considerations

### When to Use Signed Tropical

Use `SignedTropicalWeight` when:
- You need rewards (negative costs) in your model
- You're doing preference-based optimization
- You have bidirectional scoring (costs and bonuses)

Use standard `TropicalWeight` when:
- All weights are non-negative
- You need Dijkstra's algorithm guarantees
- You need the star operation (epsilon removal)

### Algorithm Compatibility

| Algorithm | SignedTropicalWeight Support |
|-----------|------------------------------|
| Shortest path (Dijkstra) | Only if all weights ≥ 0 |
| Shortest path (Bellman-Ford) | Yes (handles negatives) |
| Determinization | Yes |
| Minimization | Yes |
| Weight pushing | Yes |
| Epsilon removal | Only if loop weights ≥ 0 |

### Memory Layout

`SignedTropicalWeight` is a thin wrapper over `OrderedFloat<f64>`:

```rust
#[repr(transparent)]
pub struct SignedTropicalWeight(pub OrderedFloat<f64>);
```

This means:
- **Size**: 8 bytes (same as `f64`)
- **Alignment**: 8 bytes
- **Copy semantics**: Cheap to copy
- **Ordering**: Total ordering via `OrderedFloat`

## Related Topics

- [Semirings](semirings.md): Overview of all semiring types
- [Tropical Weight](semirings.md#tropicalweight): Standard (non-negative) tropical semiring
- [Weight Pushing](../algorithms/weight-pushing.md): Weight distribution algorithms
- [Epsilon Removal](../algorithms/epsilon-removal.md): Uses star operation
- [Shortest Distance](../algorithms/shortest-distance.md): Path algorithms with different semirings
