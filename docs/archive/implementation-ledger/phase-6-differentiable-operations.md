# Phase 6: Differentiable Operations

**Branch**: `feature/differentiable`
**Depends on**: Phases 1-3
**Started**: 2025-12-27
**Status**: COMPLETED

## Overview

Phase 6 implements differentiable WFST operations for end-to-end training, based on the
ICML 2020 paper "Differentiable Weighted Finite-State Transducers" (arXiv:2010.01003) by Hannun et al.

This enables gradient-based training with WFST-based loss functions, integrating WFSTs
into deep learning pipelines.

### Components

1. **GradientWfst**: WFST wrapper with forward/backward score caching
   - Maintains $`\alpha`$ (forward) and $`\beta`$ (backward) values per state
   - Tracks computation state for gradient reuse

2. **Forward Score**: Log-sum-exp over all paths (log semiring)
   - Computes total path weight: $`\sum_{p \in \mathrm{paths}} \exp(-\mathrm{weight}(p))`$
   - $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ for acyclic WFSTs

3. **Viterbi Score**: Max over all paths (tropical semiring interpretation)
   - Finds minimum weight path
   - $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ for acyclic WFSTs

4. **Backward Pass**: Reverse-mode automatic differentiation
   - Computes gradients $`\partial Z / \partial w`$ for all arc weights
   - Uses $`\alpha\beta`$ decomposition: $`\mathrm{grad}(e) = \exp(\alpha[\mathrm{from}(e)] + w_e + \beta[\mathrm{to}(e)] - Z)`$

---

## 6.1 Forward Score

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Forward score computes the total weight of all paths through a WFST using the log
semiring. This is equivalent to computing $`-\log(\sum_{p \in \mathrm{paths}} \exp(-\mathrm{weight}(p)))`$.

**Algorithm**:
1. Initialize $`\alpha[\mathrm{start}] = \bar{1}`$ (log-semiring one is $`0`$)
2. Process states in topological order
3. For each arc $`(s,t,w)`$: $`\alpha[t] = \alpha[t] \oplus (\alpha[s] \otimes w)`$
4. Total score: $`\bigoplus_{f \in F}(\alpha[f] \otimes \mathrm{final\_weight}[f])`$

**Complexity**: $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ for acyclic WFSTs

### Implementation

**Files created**:
- `src/differentiable/mod.rs` (~130 lines): Module with exports and tests
- `src/differentiable/gradient.rs` (~405 lines): GradientWfst and backward pass
- `src/differentiable/forward_score.rs` (~290 lines): Forward score algorithm
- `src/differentiable/viterbi.rs` (~380 lines): Viterbi score with gradients

**Key features**:
- `GradientWfst<L>` wrapper for gradient tracking
- `forward_score()` for log-sum-exp over paths
- `log_sum_exp_paths()` alias emphasizing the mathematical operation
- RefCell-based interior mutability for forward/backward score caching
- Topological order computation with cycle fallback

---

## 6.2 Viterbi Score

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Viterbi score finds the minimum weight path through a WFST (tropical semiring).
For log-probability weights, this corresponds to the maximum probability path.

**Algorithm**:
1. Initialize $`\delta[\mathrm{start}] = 0`$ (tropical one)
2. Process states in topological order
3. For each arc $`(s,t,w)`$: $`\delta[t] = \min(\delta[t], \delta[s] + w)`$
4. Best score: $`\min_{f \in F}(\delta[f] + \mathrm{final\_weight}[f])`$

**Complexity**: $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ for acyclic WFSTs

### Implementation

**Key functions**:
- `viterbi_score()` - Compute best path score
- `viterbi_path_with_grad()` - Returns score, path, and gradients

**ViterbiGradResult**:
```rust
pub struct ViterbiGradResult {
    pub score: LogWeight,
    pub path: Vec<ArcIndex>,
    pub gradients: GradientAccumulator,
}
```

---

## 6.3 Backward Pass

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Backward pass computes gradients through a WFST using reverse-mode automatic
differentiation. For arc weight w on transition (s, t):

$`\dfrac{\partial Z}{\partial w} = \exp(\alpha[s] + w + \beta[t] - Z)`$

Where:
- $`\alpha[s]`$ = forward score from the start to state $`s`$
- $`\beta[t]`$ = backward score from state $`t`$ to final states
- Z = total score (normalization constant)

**Algorithm**:
1. Initialize $`\beta[f] = \mathrm{final\_weight}[f]`$ for all final states
2. Process states in reverse topological order
3. For each arc $`(s,t,w)`$: $`\beta[s] = \beta[s] \oplus (w \otimes \beta[t])`$
4. Compute arc gradients using the $`\alpha\beta`$ formula

**Complexity**: $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ for acyclic WFSTs

### Implementation

**Key functions**:
- `backward()` - Compute gradients for all arcs
- Returns `GradientAccumulator` with per-arc gradients

**GradientAccumulator**:
```rust
pub struct GradientAccumulator {
    pub arc_gradients: Vec<ArcGradient>,
    pub num_arcs: usize,
}
```

---

## Benchmark Results

**Configuration**: taskset -c 0-3, 100 samples, 3s warmup

### Linear WFST (chain: 0 → 1 → 2 → ... → n)

| Size | Forward Score | Viterbi Score | Backward |
|------|---------------|---------------|----------|
| 10 | 295 ns | 226 ns | 737 ns |
| 50 | 946 ns | 1.01 µs | 2.48 µs |
| 100 | 1.76 µs | 1.84 µs | 4.69 µs |
| 200 | 3.81 µs | 3.08 µs | 9.71 µs |

### Parallel Paths (multiple arcs between states)

| Paths | Forward Score |
|-------|---------------|
| 10 | 612 ns |
| 50 | 2.43 µs |
| 100 | 4.60 µs |
| 200 | 8.74 µs |

### Diamond WFST (layers × width)

| Dimensions | Forward Score | Backward |
|------------|---------------|----------|
| 3×5 | 1.76 µs | 4.83 µs |
| 5×5 | 2.89 µs | 8.48 µs |
| 5×10 | 11.1 µs | 35.4 µs |
| 8×8 | 11.9 µs | 35.1 µs |

### Analysis

1. **Forward score scales linearly** with graph size $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$
2. **Viterbi score slightly faster** (no log-sum-exp, just min)
3. **Backward ~2.5x forward cost** (includes forward pass + gradient computation)
4. **Parallel paths scale linearly** with path count
5. **Diamond complexity** = $`\mathcal{O}(layers \times width^{2})`$ due to full connectivity

### Complexity Verification

| Algorithm | Theory | Observed | Match |
|-----------|--------|----------|-------|
| Forward score | $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ | Linear ✓ | ✓ |
| Viterbi score | $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ | Linear ✓ | ✓ |
| Backward | $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ | ~2.5× forward ✓ | ✓ |

### Result

- [x] **ACCEPTED**: All operations implemented with correct complexity
- [x] 24 unit tests passing
- [x] 20 benchmark cases added
- [x] Documentation with examples
- [x] Semiring semantics correctly handled (negative log probabilities)

---

## Phase 6 Summary

**Total Lines Added**: ~1,205 lines across 4 source files
**Tests Added**: 24 unit tests (all passing)
**Benchmarks Added**: 20 benchmark cases

### All Algorithms Verified

| Algorithm | Complexity (Expected) | Complexity (Observed) | Status |
|-----------|----------------------|----------------------|--------|
| Forward Score | $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ | Linear ✓ | ACCEPTED |
| Viterbi Score | $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ | Linear ✓ | ACCEPTED |
| Backward Pass | $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ | ~2.5× forward ✓ | ACCEPTED |

### LogWeight Semiring Semantics

Key insight during implementation: LogWeight stores NEGATIVE log probabilities:
- `LogWeight::new(x)` represents probability e^(-x)
- Positive values represent valid probabilities < 1
- `LogWeight::one()` = 0.0 (probability 1)
- `LogWeight::zero()` = $`+\infty`$ (probability $`0`$)

Operations:
- `times`: Addition in log space (product of probabilities)
- `plus`: Log-sum-exp (sum of probabilities)

---
