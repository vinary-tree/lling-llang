# Shortest-Distance Algorithms

Shortest-distance algorithms compute the total weight of all paths between states in a WFST. These are foundational algorithms that underpin weight pushing, epsilon removal, and many optimization techniques.

## Concepts

### What is Shortest-Distance?

In a weighted automaton, the "shortest distance" from state `s` to state `t` is the combination of all path weights using the semiring's ⊕ operation:

```
d(s,t) = ⊕ { w(π) : π is a path from s to t }
```

The meaning of "shortest" depends on the semiring:

| Semiring | ⊕ Operation | "Shortest" Means |
|----------|-------------|------------------|
| Tropical | min | Minimum cost path |
| Log | log-sum-exp | Total probability (in log-space) |
| Probability | + | Sum of probabilities |
| Boolean | ∨ | Any path exists |

### Why Shortest-Distance?

Shortest-distance computation is essential for:

- **Weight Pushing**: Normalizing weight distribution along paths
- **Epsilon Removal**: Computing epsilon-closure weights
- **Pruning**: Estimating best achievable scores for beam search
- **Scoring**: Computing total probability of all accepting paths

## Core API

### Types

```rust
// Configuration for shortest-distance computation
pub struct ShortestDistanceConfig {
    pub queue_type: QueueType,       // Queue discipline
    pub max_iterations: Option<usize>, // Iteration limit
    pub is_acyclic: Option<bool>,    // Graph structure hint
    pub epsilon: f64,                // Convergence threshold
}

// Queue discipline selection
pub enum QueueType {
    Auto,          // Automatic selection
    Fifo,          // General-purpose
    Topological,   // For acyclic graphs
    ShortestFirst, // Dijkstra-style
}
```

### Functions

```rust
// Single-source: from start to all states
pub fn single_source_shortest_distance<L, W, F>(
    fst: &F,
    config: ShortestDistanceConfig,
) -> Option<Vec<W>>;

// All-pairs: between all state pairs
pub fn all_pairs_shortest_distance<L, W, F>(
    fst: &F,
) -> Option<Vec<Vec<W>>>;

// Reverse: from each state to final states
pub fn reverse_shortest_distance<L, W, F>(
    fst: &F,
    config: ShortestDistanceConfig,
) -> Option<Vec<W>>;

// Convenience: total weight to any final state
pub fn shortest_distance_to_final<L, W, F>(
    fst: &F,
    config: ShortestDistanceConfig,
) -> Option<W>;
```

## Queue Disciplines

The choice of queue discipline significantly impacts algorithm performance:

### FIFO Queue

```
┌───┬───┬───┬───┐
│ 0 │ 1 │ 2 │ 3 │  →  Process in arrival order
└───┴───┴───┴───┘
      ↑
   enqueue
```

**When to use**: General-purpose for any k-closed semiring.

**Complexity**: O(C · |E|) where C bounds the path length.

```rust
let config = ShortestDistanceConfig::general();
let distances = single_source_shortest_distance(&fst, config);
```

### Topological Queue

```
Graph: 0 → 1 → 2 → 3  (acyclic)
       ↓
Order: [0, 1, 2, 3]  (process in dependency order)
```

**When to use**: Acyclic graphs (lattices, DAGs).

**Complexity**: O(|Q| + |E|) — each state processed exactly once.

```rust
let config = ShortestDistanceConfig::acyclic();
let distances = single_source_shortest_distance(&fst, config);
```

### Shortest-First Queue (Dijkstra)

```
Priority Queue:
  ┌─────────────────────────────┐
  │ (state=2, dist=1.0) ← min  │
  │ (state=0, dist=3.0)        │
  │ (state=1, dist=5.0)        │
  └─────────────────────────────┘
```

**When to use**: Tropical semiring with non-negative weights.

**Complexity**: O(|E| + |Q| log |Q|) — Dijkstra's algorithm.

```rust
let config = ShortestDistanceConfig::tropical();
let distances = single_source_shortest_distance(&fst, config);
```

### Automatic Selection

The `Auto` queue type examines the graph structure and selects appropriately:

1. Try topological sort
2. If acyclic → use `TopologicalQueue`
3. If cyclic → fall back to `FifoQueue`

```rust
let config = ShortestDistanceConfig::default(); // Uses Auto
let distances = single_source_shortest_distance(&fst, config);
```

## Examples

### Basic Usage

```rust
use lling_llang::prelude::*;
use lling_llang::algorithms::{
    single_source_shortest_distance,
    ShortestDistanceConfig,
};

// Build a simple WFST
let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
    .add_states(4)
    .start(0)
    .final_state(3, TropicalWeight::one())
    .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
    .arc(0, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
    .arc(1, Some('c'), Some('c'), 3, TropicalWeight::new(1.0))
    .arc(2, Some('d'), Some('d'), 3, TropicalWeight::new(1.0))
    .build();

// Compute distances from start
let distances = single_source_shortest_distance(
    &fst,
    ShortestDistanceConfig::default()
).expect("computation should converge");

// Distance to state 3 = min(1+1, 2+1) = 2
assert!((distances[3].value() - 2.0).abs() < 1e-10);
```

### Log Semiring for Probabilities

```rust
use lling_llang::semiring::LogWeight;

let fst: VectorWfst<char, LogWeight> = VectorWfstBuilder::new()
    .add_states(4)
    .start(0)
    .final_state(3, LogWeight::one())
    // Two paths with weights 2.0 and 3.0
    .arc(0, Some('a'), Some('a'), 1, LogWeight::new(1.0))
    .arc(0, Some('b'), Some('b'), 2, LogWeight::new(2.0))
    .arc(1, Some('c'), Some('c'), 3, LogWeight::new(1.0))
    .arc(2, Some('d'), Some('d'), 3, LogWeight::new(1.0))
    .build();

let distances = single_source_shortest_distance(
    &fst,
    ShortestDistanceConfig::default()
).unwrap();

// In log semiring, distances combine via log-sum-exp
// d[3] = -log(exp(-2) + exp(-3)) ≈ 1.69
// This represents the total probability mass
```

### Reverse Shortest-Distance

Useful for weight pushing toward final states:

```rust
use lling_llang::algorithms::reverse_shortest_distance;

// Compute distance from each state TO final states
let reverse_dists = reverse_shortest_distance(
    &fst,
    ShortestDistanceConfig::default()
).unwrap();

// reverse_dists[0] = distance from start to any final
// reverse_dists[3] = 0 (final state has zero distance to itself)
```

### All-Pairs Shortest-Distance

For complete distance matrix (requires `StarSemiring`):

```rust
use lling_llang::algorithms::all_pairs_shortest_distance;

let distances = all_pairs_shortest_distance(&fst).unwrap();

// distances[i][j] = total weight of paths from state i to state j
println!("Distance 0→3: {:?}", distances[0][3]);
```

## Algorithm Details

### Gen-Single-Source (Mohri's Algorithm)

The algorithm generalizes classical relaxation-based shortest paths:

```
procedure SINGLE_SOURCE(fst):
    d[s] ← 1̄ for all states s (⊗-identity, meaning "zero cost")
    d[start] ← 0̄ (⊕-identity, meaning "no accumulated weight")
    r[s] ← 0̄ for all states s (remainder to propagate)
    r[start] ← 1̄
    Q.insert(start)

    while Q not empty:
        s ← Q.pop()
        remainder ← r[s]
        r[s] ← 0̄

        for each arc (s, label, weight, t):
            contribution ← remainder ⊗ weight
            if d[t] ⊕ contribution ≠ d[t]:
                d[t] ← d[t] ⊕ contribution
                r[t] ← r[t] ⊕ contribution
                Q.update(t)

    return d
```

Key insight: Track "remainder" separately from distance. The remainder represents weight that still needs to be propagated to successor states.

### Floyd-Warshall Generalization

For all-pairs distances, the algorithm uses a star operation for cycles:

```
d[i][j] ← d[i][j] ⊕ (d[i][k] ⊗ d[k][k]* ⊗ d[k][j])
```

Where `d[k][k]*` handles cycles through state k. The star operation computes:

```
a* = 1̄ ⊕ a ⊕ a² ⊕ a³ ⊕ ...
```

For tropical semiring: `a* = 0` if `a ≥ 0`, undefined otherwise.
For log semiring: `a* = -log(1 - exp(-a))` if `a > 0`.

## Performance

### Complexity by Queue Type

| Queue | Time Complexity | Space | Best For |
|-------|-----------------|-------|----------|
| Topological | O(\|Q\| + \|E\|) | O(\|Q\|) | Acyclic graphs |
| ShortestFirst | O(\|E\| + \|Q\| log \|Q\|) | O(\|Q\|) | Tropical semiring |
| FIFO | O(C · \|E\|) | O(\|Q\|) | General k-closed |
| All-Pairs | O(\|Q\|³) | O(\|Q\|²) | Complete matrix |

Where:
- |Q| = number of states
- |E| = number of transitions
- C = bound on number of times a state is processed (depends on semiring)

### Queue Selection Decision Tree

```
                    ┌─────────────────┐
                    │ Graph acyclic?  │
                    └────────┬────────┘
                      yes/   \no
                        /     \
            ┌──────────┘       └──────────┐
            │                             │
    ┌───────▼───────┐           ┌─────────▼─────────┐
    │ Topological   │           │ Tropical semiring? │
    │ O(|Q| + |E|)  │           └─────────┬─────────┘
    └───────────────┘                yes/   \no
                                      /     \
                          ┌──────────┘       └──────────┐
                          │                             │
                  ┌───────▼───────┐           ┌─────────▼─────────┐
                  │ ShortestFirst │           │       FIFO        │
                  │ O(|E|+|Q|log|Q|)│         │     O(C · |E|)    │
                  └───────────────┘           └───────────────────┘
```

## Convergence

### When Does It Converge?

- **Acyclic graphs**: Always converges in O(|Q|) iterations
- **k-closed semirings**: Converges after at most k iterations per state
- **Tropical with negative cycles**: May not converge (returns `None`)

### Detecting Non-Convergence

```rust
let config = ShortestDistanceConfig {
    max_iterations: Some(1000),
    ..Default::default()
};

match single_source_shortest_distance(&fst, config) {
    Some(distances) => { /* converged */ }
    None => { /* did not converge within limit */ }
}
```

## Next Steps

- [Weight Pushing](weight-pushing.md): Uses shortest-distance for normalization
- [Epsilon Removal](epsilon-removal.md): Uses shortest-distance for ε-closures
- [Determinization](determinization.md): Uses shortest-distance for subset weights
- [Semirings](../architecture/semirings.md): Understanding ⊕ and ⊗ operations
