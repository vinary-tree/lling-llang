# Semirings

Semirings provide the algebraic foundation for all weight computations in lling-llang. This document explains the theory behind semirings and describes the implementations available.

## Concepts

### What is a Semiring?

A **semiring** is an algebraic structure that generalizes addition and multiplication. Formally, a semiring (K, ⊕, ⊗, 0̄, 1̄) consists of:

- A set K of elements (weights)
- An addition operation ⊕ (called "plus")
- A multiplication operation ⊗ (called "times")
- An additive identity 0̄ (called "zero")
- A multiplicative identity 1̄ (called "one")

### Why Semirings?

In path-finding problems, we often want to:
1. **Combine parallel alternatives** (e.g., pick the shorter of two paths)
2. **Combine sequential steps** (e.g., add the costs of consecutive edges)

Different problems have different combination rules:
- **Shortest path**: min for parallel, + for sequential
- **Probability**: + for parallel (sum), × for sequential (product)
- **Reachability**: OR for parallel, AND for sequential

Semirings unify these operations under a common interface, allowing the same algorithms to work with different optimization objectives.

### Semiring Axioms

A semiring must satisfy these axioms:

**1. Additive Monoid** (K, ⊕, 0̄):
```
a ⊕ b = b ⊕ a                    (commutativity)
(a ⊕ b) ⊕ c = a ⊕ (b ⊕ c)        (associativity)
a ⊕ 0̄ = a                        (identity)
```

**2. Multiplicative Monoid** (K, ⊗, 1̄):
```
(a ⊗ b) ⊗ c = a ⊗ (b ⊗ c)        (associativity)
a ⊗ 1̄ = 1̄ ⊗ a = a               (identity)
```

**3. Distributivity**:
```
a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)  (left)
(a ⊕ b) ⊗ c = (a ⊗ c) ⊕ (b ⊗ c)  (right)
```

**4. Annihilation**:
```
a ⊗ 0̄ = 0̄ ⊗ a = 0̄               (zero annihilates)
```

### Semantic Interpretation

In the context of WFSTs and lattices:

| Operation | Meaning | Example (Tropical) |
|-----------|---------|-------------------|
| ⊕ (plus) | Combine parallel path weights | min(2, 3) = 2 |
| ⊗ (times) | Combine sequential edge weights | 2 + 3 = 5 |
| 0̄ (zero) | Identity for ⊕, worst possible weight | ∞ |
| 1̄ (one) | Identity for ⊗, neutral weight | 0 |

## The Semiring Trait

The core trait in lling-llang:

```rust
pub trait Semiring: Clone + Copy + Debug + PartialEq + Send + Sync + 'static {
    /// Additive identity (0̄).
    fn zero() -> Self;

    /// Multiplicative identity (1̄).
    fn one() -> Self;

    /// Addition (⊕): combines parallel path weights.
    fn plus(&self, other: &Self) -> Self;

    /// Multiplication (⊗): combines sequential transition weights.
    fn times(&self, other: &Self) -> Self;

    /// Check if this weight is the additive identity.
    fn is_zero(&self) -> bool;

    /// Check if this weight is the multiplicative identity.
    fn is_one(&self) -> bool;

    /// Approximate equality for floating-point weights.
    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool;

    /// Natural ordering: is self "better" than other?
    fn natural_less(&self, other: &Self) -> Option<bool>;

    /// Convert to bytes for hashing/serialization.
    fn to_bytes(&self) -> Vec<u8>;
}
```

### Extended Traits

**DivisibleSemiring**: Supports division (needed for weight pushing):

```rust
pub trait DivisibleSemiring: Semiring {
    fn divide(&self, other: &Self) -> Option<Self>;
}
```

**StarSemiring**: Supports Kleene closure (needed for epsilon removal):

```rust
pub trait StarSemiring: Semiring {
    /// Computes a* = Σ_{n=0}^∞ aⁿ
    fn star(&self) -> Option<Self>;
}
```

### Algebraic Property Marker Traits

These marker traits document algebraic properties of semirings, enabling compile-time verification of algorithm requirements:

**IdempotentSemiring**: Addition is idempotent (`a ⊕ a = a`):
```rust
pub trait IdempotentSemiring: Semiring {}
```
Implementations: `TropicalWeight`, `BoolWeight`, `ProductWeight<S1, S2>` (when both are idempotent)

**KClosedSemiring**: Star operation converges in bounded iterations:
```rust
pub trait KClosedSemiring: Semiring {
    /// Returns k such that star converges in k iterations, or None if value-dependent.
    fn closure_bound() -> Option<usize>;
}
```
Implementations: `TropicalWeight` (k=0), `BoolWeight` (k=0), `LogWeight` (None), `ProbabilityWeight` (None), `ExpectationWeight` (None), `PowerWeight` (None)

**ZeroSumFreeSemiring**: `a ⊕ b = 0̄` implies `a = b = 0̄`:
```rust
pub trait ZeroSumFreeSemiring: Semiring {}
```
Implementations: `TropicalWeight`, `LogWeight`, `ProbabilityWeight`, `BoolWeight`, `ExpectationWeight`, `PowerWeight`, `ProductWeight<S1, S2>` (when both are zero-sum-free)

**WeaklyLeftDivisibleSemiring**: Left quotient exists for sums:
```rust
pub trait WeaklyLeftDivisibleSemiring: Semiring {
    /// Computes c such that c ⊗ divisor = self, if possible.
    fn left_divide(&self, divisor: &Self) -> Option<Self>;
}
```
Implementations: `TropicalWeight`, `LogWeight`, `ProbabilityWeight`, `ExpectationWeight`, `ProductWeight<S1, S2>` (when both are weakly left divisible)

**CommutativeTimesSemiring**: Multiplication is commutative (`a ⊗ b = b ⊗ a`):
```rust
pub trait CommutativeTimesSemiring: Semiring {}
```
Implementations: `TropicalWeight`, `LogWeight`, `ProbabilityWeight`, `BoolWeight`, `ExpectationWeight`, `PowerWeight`, `ProductWeight<S1, S2>` (when both are commutative)

### Algorithm Requirement Traits

These traits encode requirements that specific algorithms need from their weight types. They provide compile-time enforcement of correctness conditions.

**TotallyOrderedSemiring**: Weights have a total order (required for determinization):
```rust
pub trait TotallyOrderedSemiring: Semiring + Ord {
    /// Safe comparison without unwrap_or(Equal) fallbacks.
    fn total_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp(other)
    }
}
```
Used by `determinize` to safely compute minimum weights in weighted subsets.

**NonnegativeSemiring**: All weights are non-negative (required for Dijkstra's algorithm):
```rust
pub trait NonnegativeSemiring: Semiring {}
```
Required for `ShortestFirstQueue` correctness—Dijkstra's algorithm relies on the property that once a state is popped, its distance is final.

**QuantizableSemiring**: Weights can be quantized for hashing (required for minimization):
```rust
pub trait QuantizableSemiring: Semiring {
    /// Convert weight to an integer bucket for approximate equality.
    fn quantize(&self, epsilon: f64) -> i64;
}
```
Used by `minimize` for HashMap-based partition refinement with floating-point weights.

**StochasticSemiring**: Weights can be interpreted as probabilities (required for sampling):
```rust
pub trait StochasticSemiring: Semiring {
    /// Convert weight to a probability value for sampling.
    fn to_probability(&self) -> f64;
}
```
Used by `sample_path` for proportional path sampling.

### Semiring Property Summary

| Semiring | Idempotent | K-Closed | Zero-Sum-Free | Weakly Left Divisible | Commutative ⊗ | TotallyOrdered | Nonnegative | Quantizable | Stochastic |
|----------|------------|----------|---------------|----------------------|---------------|----------------|-------------|-------------|------------|
| TropicalWeight | Yes | k=0 | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| LogWeight | No | None | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| ProbabilityWeight | No | None | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| BoolWeight | Yes | k=0 | Yes | No | Yes | No | No | No | No |
| ExpectationWeight | No | None | Yes | Yes | Yes | Yes | No | Yes | No |
| PowerWeight | No | None | Yes | No | Yes | Yes | Yes | Yes | Yes |
| ProductWeight | Cond. | Cond. | Cond. | Cond. | Cond. | Cond. | Cond. | Cond. | No |
| String Semirings | No | No | No | No | No | No | No | No | No |

*Cond.: Inherits property from component semirings.*

## Built-in Semirings

### TropicalWeight

The **tropical semiring** (ℝ ∪ {∞}, min, +, ∞, 0) is the standard choice for shortest-path problems.

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| ⊕ | min(a, b) | Pick the shorter path |
| ⊗ | a + b | Accumulate costs |
| 0̄ | ∞ | Unreachable |
| 1̄ | 0 | Free (zero cost) |

```rust
use lling_llang::semiring::{Semiring, TropicalWeight};

let a = TropicalWeight::new(2.0);
let b = TropicalWeight::new(3.0);

// Parallel paths: take the minimum
assert_eq!(a.plus(&b), TropicalWeight::new(2.0));   // min(2, 3) = 2

// Sequential edges: add the costs
assert_eq!(a.times(&b), TropicalWeight::new(5.0));  // 2 + 3 = 5

// Identity elements
assert_eq!(a.plus(&TropicalWeight::zero()), a);     // a ⊕ ∞ = a
assert_eq!(a.times(&TropicalWeight::one()), a);     // a ⊗ 0 = a
```

**When to use**: Most common choice. Use when you want to find the minimum-cost path.

### LogWeight

The **log semiring** (ℝ ∪ {∞}, log-add, +, ∞, 0) operates in negative log probability space for numerical stability.

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| ⊕ | -log(exp(-a) + exp(-b)) | Sum probabilities |
| ⊗ | a + b | Multiply probabilities |
| 0̄ | ∞ | Probability 0 |
| 1̄ | 0 | Probability 1 |

```rust
use lling_llang::semiring::{Semiring, LogWeight};

// Create from probabilities
let a = LogWeight::from_probability(0.3);
let b = LogWeight::from_probability(0.5);

// Plus sums probabilities: 0.3 + 0.5 = 0.8
let sum = a.plus(&b);
assert!((sum.to_probability() - 0.8).abs() < 1e-10);

// Times multiplies probabilities: 0.3 × 0.5 = 0.15
let prod = a.times(&b);
assert!((prod.to_probability() - 0.15).abs() < 1e-10);
```

**Why negative log?** Using negative log probabilities means:
- Lower values = higher probability (consistent with costs)
- Avoids underflow when multiplying small probabilities
- Arithmetic is done in log space (numerically stable)

**When to use**: Probabilistic models, language models, HMMs.

### BoolWeight

The **boolean semiring** ({0, 1}, ∨, ∧, 0, 1) for reachability queries.

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| ⊕ | a ∨ b (OR) | Path exists from either |
| ⊗ | a ∧ b (AND) | Path exists through both |
| 0̄ | false | Unreachable |
| 1̄ | true | Reachable |

```rust
use lling_llang::semiring::{Semiring, BoolWeight};

let t = BoolWeight::new(true);
let f = BoolWeight::new(false);

assert_eq!(t.plus(&f), BoolWeight::new(true));   // true OR false = true
assert_eq!(t.times(&f), BoolWeight::new(false)); // true AND false = false
```

**When to use**: Checking if any valid path exists, without caring about weights.

### ProductWeight

The **product semiring** combines two semirings component-wise. This is useful when you want to optimize for multiple objectives simultaneously.

| Operation | Definition |
|-----------|------------|
| ⊕ | (a₁ ⊕₁ b₁, a₂ ⊕₂ b₂) |
| ⊗ | (a₁ ⊗₁ b₁, a₂ ⊗₂ b₂) |
| 0̄ | (0̄₁, 0̄₂) |
| 1̄ | (1̄₁, 1̄₂) |

```rust
use lling_llang::semiring::{Semiring, TropicalWeight, LogWeight, ProductWeight};

// Optimize for both cost and probability
type BiWeight = ProductWeight<TropicalWeight, LogWeight>;

let a = BiWeight::new(
    TropicalWeight::new(1.0),
    LogWeight::from_probability(0.5)
);
let b = BiWeight::new(
    TropicalWeight::new(2.0),
    LogWeight::from_probability(0.3)
);

// Component-wise operations
let sum = a.plus(&b);
let prod = a.times(&b);
```

**When to use**: Multi-objective optimization, e.g., balancing cost and confidence.

### ProbabilityWeight

The **probability semiring** (ℝ₊ ∪ {0}, +, ×, 0, 1) operates directly on probability values, unlike LogWeight which uses negative log space.

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| ⊕ | a + b | Sum probabilities |
| ⊗ | a × b | Multiply probabilities |
| 0̄ | 0 | Impossible event |
| 1̄ | 1 | Certain event |

```rust
use lling_llang::semiring::{Semiring, ProbabilityWeight};

let a = ProbabilityWeight::new(0.3);
let b = ProbabilityWeight::new(0.5);

// Sum probabilities: 0.3 + 0.5 = 0.8
assert!((a.plus(&b).value() - 0.8).abs() < 1e-10);

// Multiply probabilities: 0.3 × 0.5 = 0.15
assert!((a.times(&b).value() - 0.15).abs() < 1e-10);

// Convert to/from log space
let log_weight = a.to_log_weight();
let recovered = ProbabilityWeight::from_log_weight(log_weight);
```

**Comparison with LogWeight**:
- `ProbabilityWeight`: Stores `p` directly. Use when probabilities are moderate.
- `LogWeight`: Stores `-log(p)`. Use when probabilities are very small (avoids underflow).

Both represent the same mathematical probability but with different representations:
```rust
// These are equivalent
let prob = ProbabilityWeight::new(0.1);
let log = LogWeight::new(-0.1_f64.ln());  // ≈ 2.303

// Easy conversion
let from_prob: LogWeight = prob.into();
let from_log: ProbabilityWeight = log.into();
```

**When to use**: Moderate probabilities where underflow is not a concern, or when you need to frequently convert to/from direct probability values.

### String Semirings (LeftStringWeight, RightStringWeight)

The **string semiring** operates on strings with longest common prefix/suffix for addition and concatenation for multiplication.

| Variant | ⊕ Operation | Distributivity |
|---------|-------------|----------------|
| LeftStringWeight | Longest common prefix (lcp) | Left-distributive |
| RightStringWeight | Longest common suffix (lcs) | Right-distributive |

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| ⊕ | lcp(a, b) or lcs(a, b) | Common part of strings |
| ⊗ | a · b (concatenation) | Join strings |
| 0̄ | ∞ (infinite string) | Identity for lcp/lcs |
| 1̄ | ε (empty string) | Identity for concatenation |

```rust
use lling_llang::semiring::LeftStringWeight;

let abc = LeftStringWeight::from_str("abc");
let abx = LeftStringWeight::from_str("abx");
let def = LeftStringWeight::from_str("def");

// Longest common prefix: "ab"
let lcp = abc.plus(&abx);
assert_eq!(lcp.as_str(), Some("ab"));

// No common prefix
let lcp2 = abc.plus(&def);
assert_eq!(lcp2.as_str(), Some(""));

// Concatenation: "abcdef"
let concat = abc.times(&def);
assert_eq!(concat.as_str(), Some("abcdef"));
```

**Important**: String semirings are only **weakly distributive** (left or right, not both):
```rust
// LeftStringWeight: Left-distributive
// a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)  ✓

// RightStringWeight: Right-distributive
// (a ⊕ b) ⊗ c = (a ⊗ c) ⊕ (b ⊗ c)  ✓
```

**When to use**: Computing common label prefixes/suffixes among paths, label disambiguation in determinization, output label accumulation in composition.

### ExpectationWeight

The **expectation semiring** (ℝ × ℝ, ⊕, ⊗, (0,0), (1,0)) combines probabilities with expected value computation.

| Operation | Definition | Intuition |
|-----------|------------|-----------|
| ⊕ | (x₁ + x₂, y₁ + y₂) | Sum probabilities and expectations |
| ⊗ | (x₁·x₂, x₁·y₂ + x₂·y₁) | Product rule for expectations |
| 0̄ | (0, 0) | Zero probability, zero expectation |
| 1̄ | (1, 0) | Certain event, zero cost |

The weight stores two components:
- **value**: The probability component
- **expectation**: The expected value accumulator (probability × cost)

```rust
use lling_llang::semiring::{Semiring, ExpectationWeight};

// Two paths with different probabilities and costs
let path1 = ExpectationWeight::from_probability_and_cost(0.3, 2.0);
let path2 = ExpectationWeight::from_probability_and_cost(0.5, 1.0);

// Sum paths: total prob = 0.8, weighted cost = 0.3*2 + 0.5*1 = 1.1
let total = path1.plus(&path2);
assert!((total.value() - 0.8).abs() < 1e-10);

// Expected cost = 1.1 / 0.8 = 1.375
let expected = total.expected_value().unwrap();
assert!((expected - 1.375).abs() < 1e-10);
```

**Sequential composition** works correctly for additive costs:
```rust
// Edge 1: prob=0.5, cost=2
let e1 = ExpectationWeight::from_probability_and_cost(0.5, 2.0);
// Edge 2: prob=0.4, cost=3
let e2 = ExpectationWeight::from_probability_and_cost(0.4, 3.0);

let path = e1.times(&e2);

// Total prob = 0.5 * 0.4 = 0.2
// Expected cost = 2 + 3 = 5
assert!((path.expected_value().unwrap() - 5.0).abs() < 1e-10);
```

**When to use**: Computing expected path lengths/costs, relative entropy (KL divergence) between automata, gradient computation in differentiable WFSTs, risk-based optimization.

## Details

### Division and Weight Pushing

Weight pushing redistributes weights along paths to improve numerical stability. It requires a `DivisibleSemiring`:

```rust
// TropicalWeight: division is subtraction
let a = TropicalWeight::new(5.0);
let b = TropicalWeight::new(3.0);
let product = a.times(&b);  // 5 + 3 = 8
let quotient = product.divide(&b);  // 8 - 3 = 5
assert_eq!(quotient, Some(a));
```

### Kleene Closure

The star operation computes the infinite sum a* = 1̄ ⊕ a ⊕ a² ⊕ a³ ⊕ ..., needed for epsilon removal in WFSTs:

```rust
// TropicalWeight: star converges for non-negative weights
let w = TropicalWeight::new(5.0);
let star = w.star();  // min(0, 5, 10, ...) = 0
assert_eq!(star, Some(TropicalWeight::one()));

// Negative weights diverge
let neg = TropicalWeight::new(-1.0);
assert_eq!(neg.star(), None);  // min(0, -1, -2, ...) = -∞
```

### Natural Ordering

The `natural_less` method defines what "better" means for each semiring:

| Semiring | "Better" means | natural_less(a, b) returns true when |
|----------|----------------|--------------------------------------|
| Tropical | Lower cost | a < b |
| Log | Higher probability (lower neg-log) | a < b |
| Boolean | true is better than false | a && !b |

This is used by path extraction algorithms to compare paths.

### Numerical Stability

LogWeight includes a fast path optimization for log-sum-exp:

```rust
// When |a - b| > 20, exp(-|a-b|) ≈ 0
// So log(1 + exp(-|a-b|)) ≈ log(1) = 0
// Result is just min(a, b)
fn log_sum_exp(a: f64, b: f64) -> f64 {
    let min = a.min(b);
    let diff = (a - b).abs();

    if diff > 20.0 {
        return min;  // Fast path: skip expensive exp/ln
    }

    min - (1.0 + (-diff).exp()).ln()
}
```

## Choosing a Semiring

| Use Case | Semiring | Why |
|----------|----------|-----|
| Spelling correction | `TropicalWeight` | Edit distances are costs |
| Language modeling | `LogWeight` | N-gram probabilities (numerically stable) |
| Direct probability ops | `ProbabilityWeight` | When you need actual probability values |
| Reachability check | `BoolWeight` | Just need yes/no |
| Multi-objective | `ProductWeight` | Combine multiple criteria |
| Label accumulation | `LeftStringWeight` | Common prefix extraction |
| Label disambiguation | `RightStringWeight` | Common suffix extraction |
| Expected costs | `ExpectationWeight` | Compute average path costs |
| Risk analysis | `ExpectationWeight` | Probability-weighted costs |
| Custom scoring | Implement `Semiring` | Your own logic |

### Decision Tree

```
Need weights?
├── No → BoolWeight
└── Yes
    └── What kind?
        ├── Costs (lower = better)
        │   └── TropicalWeight
        ├── Probabilities
        │   ├── Very small (< 1e-10)? → LogWeight
        │   └── Moderate? → ProbabilityWeight
        ├── Multiple objectives? → ProductWeight
        ├── Expected values? → ExpectationWeight
        └── String labels? → LeftStringWeight / RightStringWeight
```

## Implementing Custom Semirings

To create a custom semiring:

```rust
use lling_llang::semiring::Semiring;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MyWeight(f64);

impl Semiring for MyWeight {
    fn zero() -> Self { MyWeight(f64::INFINITY) }
    fn one() -> Self { MyWeight(0.0) }

    fn plus(&self, other: &Self) -> Self {
        MyWeight(self.0.min(other.0))
    }

    fn times(&self, other: &Self) -> Self {
        MyWeight(self.0 + other.0)
    }

    fn is_zero(&self) -> bool { self.0.is_infinite() }
    fn is_one(&self) -> bool { self.0 == 0.0 }

    fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        (self.0 - other.0).abs() <= epsilon
    }

    fn natural_less(&self, other: &Self) -> Option<bool> {
        Some(self.0 < other.0)
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }
}
```

Verify your implementation with the provided test utilities:

```rust
#[test]
fn test_my_semiring() {
    use lling_llang::semiring::traits::tests::verify_semiring_axioms;

    let a = MyWeight(1.0);
    let b = MyWeight(2.0);
    let c = MyWeight(3.0);

    verify_semiring_axioms(a, b, c, 1e-10);
}
```

## Next Steps

- [Signed Tropical Semiring](signed-tropical-semiring.md): Extended tropical semiring with negative weights (rewards)
- [Power Semiring](power-semiring.md): η-power semiring for soft path selection and RRWM algorithm
- [Lattices](lattices.md): How semirings are used in lattice weights
- [WFST Operations](wfst-operations.md): Rational and unary operations on WFSTs
- [Path Extraction](../algorithms/path-extraction.md): Algorithms that use semiring operations
- [Shortest Distance](../algorithms/shortest-distance.md): Computing distances with different semirings
- [Weight Pushing](../algorithms/weight-pushing.md): Normalizing weight distributions
- [Differentiable WFSTs](../advanced/differentiable.md): Gradient computation through WFSTs
- [API Reference](../api/semiring-reference.md): Complete API documentation
