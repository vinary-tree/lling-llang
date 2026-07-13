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
   - Maintains α (forward) and β (backward) values per state
   - Tracks computation state for gradient reuse

2. **Forward Score**: Log-sum-exp over all paths (log semiring)
   - Computes total path weight: Σ_{p∈paths} exp(-weight(p))
   - O(|Q| + |E|) for acyclic WFSTs

3. **Viterbi Score**: Max over all paths (tropical semiring interpretation)
   - Finds minimum weight path
   - O(|Q| + |E|) for acyclic WFSTs

4. **Backward Pass**: Reverse-mode automatic differentiation
   - Computes gradients ∂Z/∂w for all arc weights
   - Uses α·β decomposition: grad(arc) = exp(α[from] + w + β[to] - Z)

---

## 6.1 Forward Score

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Forward score computes the total weight of all paths through a WFST using the log
semiring. This is equivalent to computing -log(Σ_paths exp(-path_weight)).

**Algorithm**:
1. Initialize α[start] = 1̄ (log semiring one = 0.0)
2. Process states in topological order
3. For each arc (s, t, w): α[t] = α[t] ⊕ (α[s] ⊗ w)
4. Total score = ⊕_{f ∈ F} (α[f] ⊗ final_weight[f])

**Complexity**: O(|Q| + |E|) for acyclic WFSTs

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
1. Initialize δ[start] = 0 (tropical one)
2. Process states in topological order
3. For each arc (s, t, w): δ[t] = min(δ[t], δ[s] + w)
4. Best score = min_{f ∈ F}(δ[f] + final_weight[f])

**Complexity**: O(|Q| + |E|) for acyclic WFSTs

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

∂Z/∂w = exp(α[s] + w + β[t] - Z)

Where:
- α[s] = forward score from start to state s
- β[t] = backward score from state t to final states
- Z = total score (normalization constant)

**Algorithm**:
1. Initialize β[f] = final_weight for all final states
2. Process states in reverse topological order
3. For each arc (s, t, w): β[s] = β[s] ⊕ (w ⊗ β[t])
4. Compute arc gradients using α·β formula

**Complexity**: O(|Q| + |E|) for acyclic WFSTs

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

1. **Forward score scales linearly** with graph size O(|Q| + |E|)
2. **Viterbi score slightly faster** (no log-sum-exp, just min)
3. **Backward ~2.5x forward cost** (includes forward pass + gradient computation)
4. **Parallel paths scale linearly** with path count
5. **Diamond complexity** = O(layers × width²) due to full connectivity

### Complexity Verification

| Algorithm | Theory | Observed | Match |
|-----------|--------|----------|-------|
| Forward score | O(\|Q\| + \|E\|) | Linear ✓ | ✓ |
| Viterbi score | O(\|Q\| + \|E\|) | Linear ✓ | ✓ |
| Backward | O(\|Q\| + \|E\|) | ~2.5× forward ✓ | ✓ |

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
| Forward Score | O(\|Q\| + \|E\|) | Linear ✓ | ACCEPTED |
| Viterbi Score | O(\|Q\| + \|E\|) | Linear ✓ | ACCEPTED |
| Backward Pass | O(\|Q\| + \|E\|) | ~2.5× forward ✓ | ACCEPTED |

### LogWeight Semiring Semantics

Key insight during implementation: LogWeight stores NEGATIVE log probabilities:
- `LogWeight::new(x)` represents probability e^(-x)
- Positive values represent valid probabilities < 1
- `LogWeight::one()` = 0.0 (probability 1)
- `LogWeight::zero()` = +∞ (probability 0)

Operations:
- `times`: Addition in log space (product of probabilities)
- `plus`: Log-sum-exp (sum of probabilities)

---

