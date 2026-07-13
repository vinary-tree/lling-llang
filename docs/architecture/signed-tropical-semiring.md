# Signed Tropical Semiring

The signed tropical semiring extends the standard tropical semiring to allow negative weights, enabling representation of **rewards** (negative costs) alongside **penalties** (positive costs).

## Terms & symbols

Symbols link to [`NOTATION.md`](../NOTATION.md); conventions in [`STYLE.md`](../STYLE.md).

| Symbol / term | Meaning |
|---|---|
| **Signed tropical** | The semiring $`(\mathbb{R} \cup \{\pm\infty\}, \min, +, +\infty, 0)`$ over the full real line. |
| $`\oplus`$ | Semiring *plus*: $`\min(a, b)`$ — pick the better (lower) weight. |
| $`\otimes`$ | Semiring *times*: $`a + b`$ — accumulate cost/reward along a path. |
| $`\bar{0}`$ / $`\bar{1}`$ | The identities $`+\infty`$ ($`\oplus`$) and $`0`$ ($`\otimes`$). |
| $`w^*`$ | Kleene closure $`\min(0, w, 2w, 3w, \dots)`$ (diverges to $`-\infty`$ for $`w < 0`$). |
| **Reward** | A negative weight ($`< 0`$) — a bonus that $`\oplus = \min`$ prefers. |

## Concepts

### What is the Signed Tropical Semiring?

The **signed tropical semiring** $`(\mathbb{R} \cup \{\pm\infty\}, \min, +, +\infty, 0)`$ operates over the full real number line, unlike the standard tropical semiring which is restricted to non-negative values.

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| $`\oplus`$ | $`\min(a, b)`$ | Pick the better (lower) weight |
| $`\otimes`$ | $`a + b`$ | Accumulate costs/rewards |
| $`\bar{0}`$ | $`+\infty`$ | Unreachable (infinite cost) |
| $`\bar{1}`$ | $`0`$ | Neutral (no cost, no reward) |

The figure below places rewards (negative) and costs (positive) on the real line and marks the star-convergence boundary at $`0`$:

![Signed-tropical semiring figure: the signature (ℝ∪{±∞}, min, +, +∞, 0) with a⊕b=min(a,b) and a⊗b=a+b over a number line from −∞ to +∞=0̄, with a green brace marking rewards (<0, fluency bonus / preferred path) on the negative side and an orange brace marking costs (>0, edit distance / penalty) on the positive side, plus a red star-rule note: a≥0 ⇒ a*=1̄=0 (converges), a<0 ⇒ a*=−∞ (diverges → FallibleStarSemiring).](../diagrams/architecture/signed-tropical.svg)

*Blue = the signature/axioms; green = the reward region ($`< 0`$) and the converging-star case; orange = the cost region ($`> 0`$); red = the star-divergence boundary at $`0`$ and the fallible-closure note.*

<details><summary>Text view</summary>

```text
  −∞ ───────────── rewards (<0) ───── 0=1̄ ───── costs (>0) ───────────── +∞=0̄
                  fluency bonus,                edit distance,
                  preferred path                penalty
  star a* = min(0, a, 2a, …):  a ≥ 0 ⇒ a* = 1̄ = 0 (converges)
                               a < 0 ⇒ a* = −∞       (diverges → FallibleStarSemiring)
```

</details>

### Why Signed Tropical?

The standard `TropicalWeight` restricts weights to non-negative values. This limitation prevents modeling scenarios where you want to **reward** certain behaviors:

| Weight Type | Meaning | Example |
|-------------|---------|---------|
| Positive ($`+`$) | Cost/Penalty | Edit distance, error penalty |
| Zero ($`0`$) | Neutral | Free operation |
| Negative ($`-`$) | Reward/Bonus | Fluency bonus, preferred path |

**Use cases:**
- **Language model scoring**: Bonuses for fluent phrases
- **Preference modeling**: Rewards for user-preferred outputs
- **Bidirectional optimization**: Balance costs and rewards in a single framework
- **Game-theoretic applications**: Minimax-style scoring
- **Reinforcement learning**: Combine costs and rewards on WFST paths

### Comparison with Standard Tropical

Standard tropical lives on the non-negative half-line $`(\mathbb{R}_+ \cup \{\infty\}, \min, +, \infty, 0)`$; the signed variant $`(\mathbb{R} \cup \{\pm\infty\}, \min, +, +\infty, 0)`$ opens it to the full real line so negatives can encode rewards:

```text
Standard Tropical:  (ℝ₊ ∪ {∞}, min, +, ∞, 0)
Signed Tropical:    (ℝ ∪ {±∞}, min, +, +∞, 0)
                        ↑
                   Full real line (allows negatives)
```

| Feature | `TropicalWeight` | `SignedTropicalWeight` |
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

The star operation $`w^* = \bar{1} \oplus w \oplus w^2 \oplus \dots`$ computes the Kleene closure. For signed tropical this is:

```math
w^* = \min(0, w, 2w, 3w, \dots)
```

- **If $`w \ge 0`$**: Sequence is non-decreasing, minimum is $`0`$ → $`w^* = 0`$ (converges)
- **If $`w < 0`$**: Sequence decreases without bound → $`w^* = -\infty`$ (diverges!)

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
| `IdempotentSemiring` | $`a \oplus a = a`$ | Shortest path algorithms work correctly |
| `CommutativeTimesSemiring` | $`a \otimes b = b \otimes a`$ | Order of path composition doesn't matter |
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
| Shortest path (Dijkstra) | Only if all weights $`\ge 0`$ |
| Shortest path (Bellman-Ford) | Yes (handles negatives) |
| Determinization | Yes |
| Minimization | Yes |
| Weight pushing | Yes |
| Epsilon removal | Only if loop weights $`\ge 0`$ |

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

## References

Full entries — including DOIs — are in [`BIBLIOGRAPHY.md`](../BIBLIOGRAPHY.md).

- [**Mohri 2009**](../BIBLIOGRAPHY.md#ref-mohri2009) — Mohri, *Weighted Automata Algorithms*: closure/star convergence conditions and the divisibility properties this semiring exposes (motivating `FallibleStarSemiring` when $`w < 0`$). [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6)
- [**Mohri 2002**](../BIBLIOGRAPHY.md#ref-mohri2002) — Mohri, Pereira & Riley, *Weighted Finite-State Transducers in Speech Recognition*: tropical weights for shortest-path scoring, here generalized to carry rewards as negative costs. [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184)

## Related Topics

- [Semirings](semirings.md): Overview of all semiring types
- [Tropical Weight](semirings.md#tropicalweight): Standard (non-negative) tropical semiring
- [Weight Pushing](../algorithms/weight-pushing.md): Weight distribution algorithms
- [Epsilon Removal](../algorithms/epsilon-removal.md): Uses star operation
- [Shortest Distance](../algorithms/shortest-distance.md): Path algorithms with different semirings
