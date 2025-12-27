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
| Language modeling | `LogWeight` | N-gram probabilities |
| Reachability check | `BoolWeight` | Just need yes/no |
| Multi-objective | `ProductWeight` | Combine criteria |
| Custom scoring | Implement `Semiring` | Your own logic |

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

- [Lattices](lattices.md): How semirings are used in lattice weights
- [Path Extraction](../algorithms/path-extraction.md): Algorithms that use semiring operations
- [API Reference](../api/semiring-reference.md): Complete API documentation
