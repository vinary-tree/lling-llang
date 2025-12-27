# WFST Traits

Weighted Finite State Transducers (WFSTs) are the theoretical foundation for lling-llang. This document explains the trait hierarchy that provides a unified interface for both eager and lazy WFST implementations.

## Concepts

### What is a WFST?

A **Weighted Finite State Transducer** is a finite automaton that:
1. Reads input symbols
2. Writes output symbols
3. Accumulates weights along transitions
4. Accepts when reaching a final state

```
     a:b/0.5
0 ──────────► 1 ──────────► 2 (final, weight 0.2)
              c:d/0.3
```

This transducer:
- From state 0, reading 'a', writes 'b' with weight 0.5, going to state 1
- From state 1, reading 'c', writes 'd' with weight 0.3, going to state 2
- State 2 is final with weight 0.2
- Path "ac" → "bd" has total weight 0.5 ⊗ 0.3 ⊗ 0.2 = 1.0 (tropical)

### Relation to Lattices

Lattices are a special case of WFSTs where:
- Input labels equal output labels (identity transduction)
- Structure is acyclic (DAG)
- Nodes represent positions

This means lattice algorithms can leverage WFST theory while maintaining the simpler position-based abstraction.

### Trait Hierarchy

```
         Wfst<L, W>
              │
    ┌─────────┴─────────┐
    ▼                   ▼
MutableWfst<L, W>   LazyWfst<L, W>
```

- **Wfst**: Read-only access to transducer structure
- **MutableWfst**: Add states and transitions
- **LazyWfst**: On-demand state expansion with caching

## The Wfst Trait

The base trait for read-only WFST access:

```rust
pub trait Wfst<L, W: Semiring>: Clone + Send + Sync {
    /// Get the start state ID.
    fn start(&self) -> StateId;

    /// Check if a state is final (accepting).
    fn is_final(&self, state: StateId) -> bool;

    /// Get the final weight for a state.
    fn final_weight(&self, state: StateId) -> W;

    /// Get the outgoing transitions from a state.
    fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>];

    /// Get the number of states in the transducer.
    fn num_states(&self) -> usize;

    // ... additional convenience methods
}
```

### Key Types

**StateId**: A `u32` identifying a state:

```rust
type StateId = u32;
```

**WeightedTransition**: A transition with input, output, target, and weight:

```rust
pub struct WeightedTransition<L, W: Semiring> {
    pub from: StateId,
    pub input: Option<L>,   // None = epsilon
    pub output: Option<L>,  // None = epsilon
    pub to: StateId,
    pub weight: W,
}
```

**WfstState**: State information including transitions:

```rust
pub struct WfstState<L, W: Semiring> {
    pub id: StateId,
    pub is_final: bool,
    pub final_weight: W,
    pub transitions: Vec<WeightedTransition<L, W>>,
}
```

### Usage Example

```rust
use lling_llang::wfst::{Wfst, VectorWfst, StateId};
use lling_llang::semiring::TropicalWeight;

fn count_reachable<L: Clone, W: Semiring>(fst: &impl Wfst<L, W>) -> usize {
    let mut visited = vec![false; fst.num_states()];
    let mut stack = vec![fst.start()];
    let mut count = 0;

    while let Some(state) = stack.pop() {
        if visited[state as usize] {
            continue;
        }
        visited[state as usize] = true;
        count += 1;

        for trans in fst.transitions(state) {
            stack.push(trans.to);
        }
    }

    count
}
```

## The MutableWfst Trait

Extends `Wfst` with mutation operations:

```rust
pub trait MutableWfst<L, W: Semiring>: Wfst<L, W> {
    /// Add a new state and return its ID.
    fn add_state(&mut self) -> StateId;

    /// Set the start state.
    fn set_start(&mut self, state: StateId);

    /// Set a state as final with the given weight.
    fn set_final(&mut self, state: StateId, weight: W);

    /// Add a transition to the transducer.
    fn add_transition(&mut self, transition: WeightedTransition<L, W>);

    /// Add a transition with explicit parameters.
    fn add_arc(&mut self, from: StateId, input: Option<L>, output: Option<L>,
               to: StateId, weight: W);

    /// Add an epsilon transition.
    fn add_epsilon(&mut self, from: StateId, to: StateId, weight: W);

    // ... additional methods
}
```

### Building WFSTs

```rust
use lling_llang::wfst::{MutableWfst, VectorWfst, VectorWfstBuilder};
use lling_llang::semiring::TropicalWeight;

// Using VectorWfstBuilder
let mut builder = VectorWfstBuilder::<char, TropicalWeight>::new();

let s0 = builder.add_state();
let s1 = builder.add_state();
let s2 = builder.add_state();

builder.set_start(s0);
builder.add_arc(s0, Some('a'), Some('x'), s1, TropicalWeight::new(1.0));
builder.add_arc(s1, Some('b'), Some('y'), s2, TropicalWeight::new(2.0));
builder.set_final(s2, TropicalWeight::new(0.5));

let fst = builder.build();
```

## The LazyWfst Trait

For WFSTs where computing all states upfront is impractical:

```rust
pub trait LazyWfst<L, W: Semiring>: Wfst<L, W> {
    /// Check if a state has been expanded.
    fn is_expanded(&self, state: StateId) -> bool;

    /// Force expansion of a state.
    fn expand(&mut self, state: StateId);

    /// Get transitions, computing them lazily if needed.
    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<L, W>];

    /// Get the current cache policy.
    fn cache_policy(&self) -> CachePolicy;

    /// Set the cache policy.
    fn set_cache_policy(&mut self, policy: CachePolicy);

    /// Get the number of states computed so far.
    fn computed_states(&self) -> usize;

    /// Clear the state cache.
    fn clear_cache(&mut self);
}
```

### Why Lazy Evaluation?

Consider composing two WFSTs with `n` states each. The product automaton can have up to `n²` states. For large `n`, this is prohibitive.

Lazy evaluation solves this:
1. Only compute states that are actually visited
2. Many states are never reachable from the start
3. Path extraction explores only a subset of states

### Cache Policies

Control memory usage vs. computation tradeoffs:

```rust
pub enum CachePolicy {
    /// Cache all visited states (default).
    CacheAll,

    /// LRU cache with maximum size.
    Lru { max_states: usize },

    /// No caching (recompute each time).
    NoCache,
}
```

| Policy | Memory | Speed | Use Case |
|--------|--------|-------|----------|
| `CacheAll` | Unbounded | Fastest | Small-medium WFSTs |
| `Lru { max_states }` | Bounded | Medium | Large WFSTs, repeated paths |
| `NoCache` | Minimal | Slowest | One-time traversal, huge WFSTs |

### Lazy Composition Example

```rust
use lling_llang::composition::{LazyComposition, compose};
use lling_llang::wfst::{LazyWfst, CachePolicy};

// Compose two WFSTs lazily
let mut composed = LazyComposition::new(fst1, fst2);

// Set memory-bounded caching
composed.set_cache_policy(CachePolicy::Lru { max_states: 10000 });

// Traverse - states computed on demand
let paths = composed.accepting_paths().take(10).collect::<Vec<_>>();

// Check efficiency
println!("Computed {} of {} potential states",
    composed.computed_states(),
    fst1.num_states() * fst2.num_states());
```

## Details

### VectorWfst Implementation

The primary eager implementation stores states in a vector:

```rust
pub struct VectorWfst<L, W: Semiring> {
    states: Vec<VectorState<L, W>>,
    start: StateId,
}

struct VectorState<L, W: Semiring> {
    transitions: Vec<WeightedTransition<L, W>>,
    final_weight: W,
}
```

Benefits:
- O(1) state access by ID
- Contiguous memory for cache efficiency
- Simple implementation

### Epsilon Transitions

Epsilon (ε) transitions consume/produce no symbols:

```rust
// Add epsilon transition with weight
fst.add_epsilon(from_state, to_state, weight);

// Represented as
WeightedTransition {
    from: from_state,
    input: None,   // epsilon on input
    output: None,  // epsilon on output
    to: to_state,
    weight,
}
```

Epsilon transitions are common in:
- Optional elements
- Composition (for synchronization)
- Converting NFAs to WFSTs

### Thread Safety

All WFST traits require `Send + Sync`:

```rust
pub trait Wfst<L, W: Semiring>: Clone + Send + Sync { ... }
```

This enables:
- Sharing WFSTs across threads
- Parallel path extraction
- Concurrent composition operations

For lazy WFSTs, interior mutability (via `RwLock`) may be used to maintain thread safety while allowing lazy expansion.

## Common Patterns

### Accepting Path Enumeration

Find all accepting paths through a WFST:

```rust
fn accepting_paths<L: Clone, W: Semiring>(fst: &impl Wfst<L, W>) -> Vec<Vec<L>> {
    let mut paths = Vec::new();
    let mut stack = vec![(fst.start(), Vec::new())];

    while let Some((state, path)) = stack.pop() {
        if fst.is_final(state) {
            paths.push(path.clone());
        }

        for trans in fst.transitions(state) {
            let mut new_path = path.clone();
            if let Some(label) = &trans.output {
                new_path.push(label.clone());
            }
            stack.push((trans.to, new_path));
        }
    }

    paths
}
```

### State Reachability

Check if a target state is reachable:

```rust
fn is_reachable<L, W: Semiring>(
    fst: &impl Wfst<L, W>,
    target: StateId,
) -> bool {
    let mut visited = vec![false; fst.num_states()];
    let mut stack = vec![fst.start()];

    while let Some(state) = stack.pop() {
        if state == target {
            return true;
        }
        if visited[state as usize] {
            continue;
        }
        visited[state as usize] = true;

        for trans in fst.transitions(state) {
            stack.push(trans.to);
        }
    }

    false
}
```

## Next Steps

- [Composition](../algorithms/composition.md): Lazy composition operators
- [Path Extraction](../algorithms/path-extraction.md): Finding paths through WFSTs
- [API Reference](../api/wfst-reference.md): Complete API documentation
