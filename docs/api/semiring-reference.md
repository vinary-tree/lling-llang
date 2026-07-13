# Semiring API Reference

Complete API reference for semiring types and operations.

## Semiring Trait

```rust
pub trait Semiring: Clone + Default + PartialEq + Debug + Send + Sync + 'static {
    /// Additive identity (⊕ identity)
    fn zero() -> Self;

    /// Multiplicative identity (⊗ identity)
    fn one() -> Self;

    /// Addition operation (⊕)
    fn plus(&self, other: &Self) -> Self;

    /// Multiplication operation (⊗)
    fn times(&self, other: &Self) -> Self;

    /// Check if this is the zero element
    fn is_zero(&self) -> bool {
        self == &Self::zero()
    }

    /// Check if this is the one element
    fn is_one(&self) -> bool {
        self == &Self::one()
    }

    /// Natural ordering for path comparison (if applicable)
    fn natural_less(&self, other: &Self) -> bool;

    /// Division (if the semiring supports it)
    fn divide(&self, other: &Self) -> Option<Self> {
        None
    }

    /// Power operation
    fn power(&self, n: u32) -> Self {
        let mut result = Self::one();
        for _ in 0..n {
            result = result.times(self);
        }
        result
    }
}
```

## TropicalWeight

Tropical semiring for shortest-path problems.

**Definition**: $`(\mathbb{R} \cup \{\infty\}, \min, +, \infty, 0)`$

```rust
pub struct TropicalWeight(f64);

impl TropicalWeight {
    /// Create from value
    pub fn new(value: f64) -> Self;

    /// Get the inner value
    pub fn value(&self) -> f64;

    /// Create from log probability
    pub fn from_log_prob(log_prob: f64) -> Self;

    /// Convert to log probability
    pub fn to_log_prob(&self) -> f64;
}
```

### Operations

| Operation | Implementation |
|-----------|----------------|
| `zero()` | `f64::INFINITY` |
| `one()` | `0.0` |
| `plus(a, b)` | `min(a, b)` |
| `times(a, b)` | `a + b` |
| `natural_less(a, b)` | `a < b` |
| `divide(a, b)` | `Some(a - b)` |

### Usage

```rust
use lling_llang::semiring::TropicalWeight;

let w1 = TropicalWeight::new(2.0);
let w2 = TropicalWeight::new(3.0);

// Addition: min
assert_eq!(w1.plus(&w2), TropicalWeight::new(2.0));

// Multiplication: +
assert_eq!(w1.times(&w2), TropicalWeight::new(5.0));

// Identities
assert_eq!(TropicalWeight::zero().value(), f64::INFINITY);
assert_eq!(TropicalWeight::one().value(), 0.0);
```

## LogWeight

Log semiring for probability computations.

**Definition**: $`(\mathbb{R} \cup \{-\infty, +\infty\}, \text{log-add}, +, +\infty, 0)`$

```rust
pub struct LogWeight(f64);

impl LogWeight {
    /// Create from log probability
    pub fn new(log_prob: f64) -> Self;

    /// Get the inner value
    pub fn value(&self) -> f64;

    /// Create from probability (applies log)
    pub fn from_prob(prob: f64) -> Self;

    /// Convert to probability (applies exp)
    pub fn to_prob(&self) -> f64;
}
```

### Operations

| Operation | Implementation |
|-----------|----------------|
| `zero()` | `f64::INFINITY` |
| `one()` | `0.0` |
| `plus(a, b)` | `log(exp(-a) + exp(-b))` (log-add) |
| `times(a, b)` | `a + b` |
| `natural_less(a, b)` | `a < b` |
| `divide(a, b)` | `Some(a - b)` |

### Log-Add Implementation

```rust
fn log_add(a: f64, b: f64) -> f64 {
    if a == f64::INFINITY { return b; }
    if b == f64::INFINITY { return a; }
    if a < b {
        a - (1.0 + (a - b).exp()).ln()
    } else {
        b - (1.0 + (b - a).exp()).ln()
    }
}
```

### Usage

```rust
use lling_llang::semiring::LogWeight;

// From probabilities
let w1 = LogWeight::from_prob(0.7);  // log(0.7) ≈ -0.357
let w2 = LogWeight::from_prob(0.3);  // log(0.3) ≈ -1.204

// Addition: log-add (like probability addition)
let sum = w1.plus(&w2);
assert!((sum.to_prob() - 1.0).abs() < 0.001);  // 0.7 + 0.3 = 1.0

// Multiplication: + (like probability multiplication in log space)
let product = w1.times(&w2);
assert!((product.to_prob() - 0.21).abs() < 0.001);  // 0.7 * 0.3 = 0.21
```

## BooleanWeight

Boolean semiring for reachability.

**Definition**: $`(\{\text{false}, \text{true}\}, \lor, \land, \text{false}, \text{true})`$

```rust
pub struct BooleanWeight(bool);

impl BooleanWeight {
    /// Create from boolean
    pub fn new(value: bool) -> Self;

    /// Get the inner value
    pub fn value(&self) -> bool;
}
```

### Operations

| Operation | Implementation |
|-----------|----------------|
| `zero()` | `false` |
| `one()` | `true` |
| `plus(a, b)` | $`a \lor b`$ |
| `times(a, b)` | $`a \land b`$ |
| `natural_less(a, b)` | `!a && b` |

### Usage

```rust
use lling_llang::semiring::BooleanWeight;

let t = BooleanWeight::new(true);
let f = BooleanWeight::new(false);

// Addition: OR
assert_eq!(t.plus(&f), BooleanWeight::new(true));
assert_eq!(f.plus(&f), BooleanWeight::new(false));

// Multiplication: AND
assert_eq!(t.times(&f), BooleanWeight::new(false));
assert_eq!(t.times(&t), BooleanWeight::new(true));
```

## ProductWeight

Product of two semirings.

```rust
pub struct ProductWeight<W1: Semiring, W2: Semiring>(W1, W2);

impl<W1: Semiring, W2: Semiring> ProductWeight<W1, W2> {
    /// Create from components
    pub fn new(w1: W1, w2: W2) -> Self;

    /// Get first component
    pub fn first(&self) -> &W1;

    /// Get second component
    pub fn second(&self) -> &W2;

    /// Destructure into components
    pub fn into_inner(self) -> (W1, W2);
}
```

### Operations

Component-wise operations:

| Operation | Implementation |
|-----------|----------------|
| `zero()` | `(W1::zero(), W2::zero())` |
| `one()` | `(W1::one(), W2::one())` |
| `plus(a, b)` | $`(a_0 \oplus b_0, a_1 \oplus b_1)`$ |
| `times(a, b)` | $`(a_0 \otimes b_0, a_1 \otimes b_1)`$ |
| `natural_less` | Lexicographic comparison |

### Usage

```rust
use lling_llang::semiring::{TropicalWeight, LogWeight, ProductWeight};

type CombinedWeight = ProductWeight<TropicalWeight, LogWeight>;

let w1 = CombinedWeight::new(TropicalWeight::new(1.0), LogWeight::new(2.0));
let w2 = CombinedWeight::new(TropicalWeight::new(3.0), LogWeight::new(4.0));

let sum = w1.plus(&w2);
// First component: min(1.0, 3.0) = 1.0
// Second component: log-add(2.0, 4.0)
```

## StringWeight

String semiring for path labels.

```rust
pub struct StringWeight(Option<String>);

impl StringWeight {
    /// Create from string
    pub fn new(s: impl Into<String>) -> Self;

    /// Create empty string (one)
    pub fn empty() -> Self;

    /// Get the inner string
    pub fn value(&self) -> Option<&str>;

    /// Check if this is the "no string" element
    pub fn is_none(&self) -> bool;
}
```

### Operations

| Operation | Implementation |
|-----------|----------------|
| `zero()` | `None` (no string) |
| `one()` | `Some("")` (empty string) |
| `plus(a, b)` | Longest common prefix |
| `times(a, b)` | Concatenation |

### Usage

```rust
use lling_llang::semiring::StringWeight;

let w1 = StringWeight::new("hello");
let w2 = StringWeight::new("world");

// Multiplication: concatenation
let product = w1.times(&w2);
assert_eq!(product.value(), Some("helloworld"));

// Addition: LCP
let w3 = StringWeight::new("hello");
let w4 = StringWeight::new("help");
let lcp = w3.plus(&w4);
assert_eq!(lcp.value(), Some("hel"));
```

## Weight Conversion

### Between Tropical and Log

```rust
impl From<TropicalWeight> for LogWeight {
    fn from(w: TropicalWeight) -> Self {
        LogWeight::new(w.value())
    }
}

impl From<LogWeight> for TropicalWeight {
    fn from(w: LogWeight) -> Self {
        TropicalWeight::new(w.value())
    }
}
```

### To/From Probability

```rust
impl TropicalWeight {
    /// From negative log probability
    pub fn from_neg_log_prob(p: f64) -> Self {
        Self::new(p)
    }

    /// To negative log probability
    pub fn to_neg_log_prob(&self) -> f64 {
        self.value()
    }
}

impl LogWeight {
    /// From probability (takes -log)
    pub fn from_prob(p: f64) -> Self {
        Self::new(-p.ln())
    }

    /// To probability (takes exp(-x))
    pub fn to_prob(&self) -> f64 {
        (-self.value()).exp()
    }
}
```

## Utility Functions

```rust
/// Sum over a collection of weights
pub fn sum<W: Semiring>(weights: impl IntoIterator<Item = W>) -> W {
    weights.into_iter().fold(W::zero(), |acc, w| acc.plus(&w))
}

/// Product over a collection of weights
pub fn product<W: Semiring>(weights: impl IntoIterator<Item = W>) -> W {
    weights.into_iter().fold(W::one(), |acc, w| acc.times(&w))
}

/// Find the minimum weight (for tropical-like semirings)
pub fn min_weight<W: Semiring>(weights: impl IntoIterator<Item = W>) -> Option<W> {
    weights.into_iter().reduce(|a, b| {
        if a.natural_less(&b) { a } else { b }
    })
}
```

## See Also

- [Semirings (Architecture)](../architecture/semirings.md): Conceptual overview
- [Path Extraction](../algorithms/path-extraction.md): Using semirings in algorithms
- [Lattice Reference](lattice-reference.md): Weighted lattices
