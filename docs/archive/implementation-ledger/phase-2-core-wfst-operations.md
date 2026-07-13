# Phase 2: Core WFST Operations

**Branch**: `feature/core-ops`
**Depends on**: Phase 1
**Started**: 2025-12-27
**Status**: COMPLETED

## Overview

Phase 2 implements core WFST optimization operations: weight pushing, epsilon removal,
and connect (trim). These are essential preprocessing steps for determinization and
minimization.

### Components

1. **Weight Pushing**: Redistribute weights toward initial/final states
   - Forward pushing (toward initial states)
   - Backward pushing (toward final states)
   - Stochastic normalization

2. **Epsilon Removal**: Remove ε-transitions while preserving language
   - ε-closure computation via shortest-distance
   - Transition deduplication (⊕-sum redundant)
   - Acyclic optimization

3. **Connect (Trim)**: Remove non-useful states
   - Accessible states (reachable from start)
   - Coaccessible states (can reach final)

---

## 2.1 Weight Pushing

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Weight pushing redistributes weights along paths to normalize the WFST. This is
essential for minimization and improves beam search pruning efficacy.

**Forward Push**: Weights moved toward initial state
- Uses shortest-distance from start to compute potentials
- Preserves path weights

**Backward Push**: Weights moved toward final states
- Uses reverse shortest-distance to compute potentials
- Path weights normalized (V(initial) absorbed)

**Complexity**: O(|Q| + |E|) for acyclic, O(|E| log |Q|) for general tropical

### Design

```rust
pub enum PushDirection {
    Forward,
    Backward,
}

pub struct PushConfig {
    pub direction: PushDirection,
    pub push_finals: bool,
    pub remove_total_weight: bool,
}

pub fn push_weights<L, W, F>(fst: &mut F, config: PushConfig) -> Result<(), PushError>
where
    L: Clone,
    W: DivisibleSemiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
```

### Implementation

**Files created**:
- `src/algorithms/push.rs` (~520 lines)

**Key functions**:
- `push_weights()` - Main API with configuration
- `is_stochastic()` - Check if weights sum to 1 at each state

**Exports**:
- `push_weights`, `is_stochastic`
- `PushConfig`, `PushDirection`, `PushError`

### Benchmark Results

**Configuration**: CPU governor=performance, taskset -c 0-3, 100 samples, 3s warmup

#### Linear WFST (chain: 0 → 1 → 2 → ... → n)

| Size | Forward Push | Backward Push |
|------|--------------|---------------|
| 10 | 1.09 µs | 1.53 µs |
| 50 | 5.89 µs | 9.51 µs |
| 100 | 11.54 µs | 19.28 µs |
| 200 | 23.03 µs | 37.25 µs |

#### Diamond WFST (branching factor 3)

| Size | Forward Push | Backward Push |
|------|--------------|---------------|
| 10 | 1.43 µs | 1.87 µs |
| 50 | 7.38 µs | 11.48 µs |
| 100 | 14.90 µs | 22.18 µs |
| 200 | 28.51 µs | 43.45 µs |

**Observations**:
1. Forward push is ~1.5-1.6x faster than backward push
2. Both scale linearly with graph size, confirming O(|Q| + |E|)
3. Diamond graphs have ~1.2x overhead vs linear due to edge density
4. Backward push is slower because it requires computing reverse graph traversal

### Result: ACCEPTED

Weight pushing shows expected linear complexity scaling. Forward push is more
efficient; backward push provides normalization suitable for beam search optimization.

---

## 2.2 Epsilon Removal

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Epsilon removal eliminates ε-transitions by computing ε-closures and adding
direct transitions that bypass the ε-paths.

**Complexity**:
- Acyclic: O(|Q|² + |Q||E|(T⊕ + T⊗))
- General: O(|Q|³(T⊕ + T⊗ + T*) + |Q||E|(T⊕ + T⊗))

### Design

```rust
pub struct EpsilonRemovalConfig {
    pub input_epsilon: bool,   // Remove input-ε transitions
    pub output_epsilon: bool,  // Remove output-ε transitions
    pub connect_after: bool,   // Run connect after removal
    pub max_iterations: Option<usize>,
}

pub fn remove_epsilon<L, W, F>(fst: &mut F, config: EpsilonRemovalConfig)
    -> Result<(), EpsilonRemovalError>
where
    L: Clone + PartialEq,
    W: Semiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
```

### Implementation

**Files created**:
- `src/algorithms/epsilon_removal.rs` (~330 lines)

**Key functions**:
- `remove_epsilon()` - Standard epsilon removal
- `remove_epsilon_star()` - Iterative removal until fixed point
- `has_epsilon_transitions()` - Check for ε-transitions

**Exports**:
- `remove_epsilon`, `remove_epsilon_star`, `has_epsilon_transitions`
- `EpsilonRemovalConfig`, `EpsilonRemovalError`

### Benchmark Results

**Configuration**: CPU governor=performance, taskset -c 0-3, 100 samples, 3s warmup

#### Epsilon Chain (alternating ε and non-ε transitions)

| Depth | Standard | Acyclic Config |
|-------|----------|----------------|
| 5 | 3.18 µs | 3.20 µs |
| 10 | 6.62 µs | 6.32 µs |
| 25 | 15.63 µs | 16.10 µs |
| 50 | 31.50 µs | 30.24 µs |

**Observations**:
1. Both configurations show similar performance (within noise)
2. Scales linearly with chain depth
3. Acyclic config has slight advantage at larger sizes (~4% faster at depth 50)
4. Low variance indicates stable performance

### Result: ACCEPTED

Epsilon removal shows expected linear scaling for the test cases. The acyclic
optimization provides modest improvement. Both variants are stable and efficient.

---

## 2.3 Connect (Trim)

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Connect removes states that are not on any accepting path. A state is kept iff:
1. **Accessible**: Reachable from the start state
2. **Coaccessible**: Can reach at least one final state

**Complexity**: O(|Q| + |E|) - Linear in the size of the automaton.

### Design

```rust
pub struct ConnectConfig {
    pub keep_non_coaccessible: bool,
    pub keep_non_accessible: bool,
}

pub fn connect<L, W, F>(fst: &mut F, config: ConnectConfig) -> usize
where
    L: Clone,
    W: Semiring,
    F: MutableWfst<L, W> + Wfst<L, W>,
```

### Implementation

**Files created**:
- `src/algorithms/connect.rs` (~260 lines)

**Key functions**:
- `connect()` - Main connect/trim operation
- `compute_accessible()` - Find accessible states (BFS from start)
- `compute_coaccessible()` - Find coaccessible states (reverse BFS from finals)
- `is_connected()` - Check if all states are useful
- `count_useful_states()` - Count states that are both accessible and coaccessible

**Exports**:
- `connect`, `compute_accessible`, `compute_coaccessible`
- `is_connected`, `count_useful_states`, `ConnectConfig`

### Benchmark Results

**Configuration**: CPU governor=performance, taskset -c 0-3, 100 samples, 3s warmup

#### Already Connected (no states to remove)

| Size | Linear | Diamond |
|------|--------|---------|
| 10 | 2.31 µs | 3.08 µs |
| 50 | 11.74 µs | 14.34 µs |
| 100 | 24.59 µs | 30.00 µs |
| 200 | 48.08 µs | 60.07 µs |

#### Trim Disconnected (with unreachable/dead-end states)

| Size | Time |
|------|------|
| 10 | 7.40 µs |
| 50 | 35.62 µs |
| 100 | 70.18 µs |

**Observations**:
1. Linear scaling confirms O(|Q| + |E|) complexity
2. Already-connected case is fastest (early exit optimization)
3. Diamond graphs have ~1.2-1.3x overhead due to higher edge density
4. Trim operation with actual removals takes ~3x longer (expected: extra work)

### Result: ACCEPTED

Connect operation shows expected linear complexity. The implementation correctly
handles both connected graphs (fast path) and graphs requiring trimming.

---

## Phase 2 Summary

**Total Lines Added**: ~1,110 lines across 3 algorithm files
**Tests Added**: 52 unit tests (all passing)
**Benchmarks Added**: 35 benchmark cases

### All Algorithms Verified

| Algorithm | Complexity (Expected) | Complexity (Observed) | Status |
|-----------|----------------------|----------------------|--------|
| Weight Push Forward | O(\|Q\| + \|E\|) | Linear ✓ | ACCEPTED |
| Weight Push Backward | O(\|Q\| + \|E\|) | Linear ✓ | ACCEPTED |
| Epsilon Removal | O(\|Q\|² + \|Q\|\|E\|) | Near-linear ✓ | ACCEPTED |
| Connect | O(\|Q\| + \|E\|) | Linear ✓ | ACCEPTED |

### MutableWfst Trait Enhancement

Added `clear_transitions()` method to `MutableWfst` trait for in-place modification:

```rust
/// Clear all transitions from a state.
fn clear_transitions(&mut self, state: StateId);
```

Implemented in `VectorWfst` for efficient transition manipulation.

---

