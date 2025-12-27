# WFST API Reference

Complete API reference for WFST traits and implementations.

## Wfst Trait

Read-only WFST operations.

```rust
pub trait Wfst<L, W: Semiring> {
    /// State identifier type
    type StateId: Copy + Eq + Hash;

    /// Arc type for this WFST
    type Arc: WfstArc<L, W, StateId = Self::StateId>;

    /// Get the start state
    fn start(&self) -> Option<Self::StateId>;

    /// Check if a state is final
    fn is_final(&self, state: Self::StateId) -> bool;

    /// Get the final weight of a state
    fn final_weight(&self, state: Self::StateId) -> Option<W>;

    /// Get number of states
    fn num_states(&self) -> usize;

    /// Iterate over all states
    fn states(&self) -> impl Iterator<Item = Self::StateId>;

    /// Iterate over arcs from a state
    fn arcs(&self, state: Self::StateId) -> impl Iterator<Item = Self::Arc>;

    /// Get number of arcs from a state
    fn num_arcs(&self, state: Self::StateId) -> usize;

    /// Check if state exists
    fn contains_state(&self, state: Self::StateId) -> bool;
}
```

## MutableWfst Trait

Mutable WFST operations.

```rust
pub trait MutableWfst<L, W: Semiring>: Wfst<L, W> {
    /// Add a new state, return its ID
    fn add_state(&mut self) -> Self::StateId;

    /// Set the start state
    fn set_start(&mut self, state: Self::StateId);

    /// Set a state as final with given weight
    fn set_final(&mut self, state: Self::StateId, weight: W);

    /// Remove final status from a state
    fn unset_final(&mut self, state: Self::StateId);

    /// Add an arc from source to target
    fn add_arc(
        &mut self,
        source: Self::StateId,
        input: Option<L>,
        output: Option<L>,
        target: Self::StateId,
        weight: W,
    );

    /// Remove a state and all its arcs
    fn remove_state(&mut self, state: Self::StateId);

    /// Remove all arcs from a state
    fn clear_arcs(&mut self, state: Self::StateId);

    /// Reserve capacity for states
    fn reserve_states(&mut self, additional: usize);

    /// Reserve capacity for arcs at a state
    fn reserve_arcs(&mut self, state: Self::StateId, additional: usize);
}
```

## LazyWfst Trait

On-demand expansion.

```rust
pub trait LazyWfst<L, W: Semiring>: Wfst<L, W> {
    /// Expand a state (compute its arcs on demand)
    fn expand(&mut self, state: Self::StateId);

    /// Check if a state has been expanded
    fn is_expanded(&self, state: Self::StateId) -> bool;

    /// Get cache policy
    fn cache_policy(&self) -> CachePolicy;

    /// Set cache policy
    fn set_cache_policy(&mut self, policy: CachePolicy);

    /// Clear cached expansions
    fn clear_cache(&mut self);

    /// Get number of cached states
    fn cached_states(&self) -> usize;
}
```

## WfstArc Trait

Arc representation.

```rust
pub trait WfstArc<L, W: Semiring> {
    type StateId: Copy + Eq + Hash;

    /// Get input label
    fn input(&self) -> Option<&L>;

    /// Get output label
    fn output(&self) -> Option<&L>;

    /// Get target state
    fn target(&self) -> Self::StateId;

    /// Get weight
    fn weight(&self) -> &W;
}
```

## VectorWfst

Array-based WFST implementation.

```rust
pub struct VectorWfst<L, W: Semiring> {
    // Internal state
}

impl<L, W: Semiring> VectorWfst<L, W> {
    /// Create empty WFST
    pub fn new() -> Self;

    /// Create with capacity
    pub fn with_capacity(states: usize) -> Self;

    /// Get total number of arcs
    pub fn total_arcs(&self) -> usize;

    /// Shrink internal storage
    pub fn shrink_to_fit(&mut self);
}
```

### VectorWfstBuilder

```rust
pub struct VectorWfstBuilder<L, W: Semiring> {
    wfst: VectorWfst<L, W>,
}

impl<L, W: Semiring> VectorWfstBuilder<L, W> {
    /// Create new builder
    pub fn new() -> Self;

    /// Add states
    pub fn add_states(self, count: usize) -> Self;

    /// Set start state
    pub fn start(self, state: usize) -> Self;

    /// Set final state with weight
    pub fn final_state(self, state: usize, weight: W) -> Self;

    /// Add arc
    pub fn arc(
        self,
        source: usize,
        input: Option<L>,
        output: Option<L>,
        target: usize,
        weight: W,
    ) -> Self;

    /// Build the WFST
    pub fn build(self) -> VectorWfst<L, W>;
}
```

### Usage

```rust
use lling_llang::wfst::{VectorWfstBuilder, Wfst};
use lling_llang::semiring::TropicalWeight;

let fst = VectorWfstBuilder::new()
    .add_states(3)
    .start(0)
    .final_state(2, TropicalWeight::one())
    .arc(0, Some('a'), Some('x'), 1, TropicalWeight::new(1.0))
    .arc(1, Some('b'), Some('y'), 2, TropicalWeight::new(2.0))
    .build();

assert_eq!(fst.num_states(), 3);
assert!(fst.is_final(2.into()));
```

## VectorArc

Arc implementation for VectorWfst.

```rust
pub struct VectorArc<L, W: Semiring> {
    pub input: Option<L>,
    pub output: Option<L>,
    pub target: StateId,
    pub weight: W,
}

impl<L, W: Semiring> VectorArc<L, W> {
    /// Create new arc
    pub fn new(
        input: Option<L>,
        output: Option<L>,
        target: StateId,
        weight: W,
    ) -> Self;
}
```

## StateId

State identifier.

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct StateId(pub u32);

impl StateId {
    /// Create from index
    pub fn new(index: usize) -> Self;

    /// Get index
    pub fn index(&self) -> usize;
}

impl From<usize> for StateId {
    fn from(index: usize) -> Self {
        StateId(index as u32)
    }
}
```

## CachePolicy

Cache control for lazy WFSTs.

```rust
pub enum CachePolicy {
    /// Cache all expanded states
    CacheAll,

    /// LRU cache with maximum size
    Lru { max_states: usize },

    /// No caching (recompute each access)
    NoCache,
}

impl Default for CachePolicy {
    fn default() -> Self {
        CachePolicy::CacheAll
    }
}
```

## WFST Operations

### Composition

```rust
use lling_llang::composition::compose;

let composed = compose(fst1, fst2);

for path in composed.accepting_paths() {
    println!("{:?} -> {:?}", path.inputs, path.outputs);
}
```

### Determinization

```rust
use lling_llang::wfst::determinize;

let det = determinize(&fst)?;
assert!(det.is_deterministic());
```

### Minimization

```rust
use lling_llang::wfst::minimize;

let min = minimize(&fst)?;
println!("Reduced from {} to {} states", fst.num_states(), min.num_states());
```

### Reversal

```rust
use lling_llang::wfst::reverse;

let rev = reverse(&fst);
// Swaps start and final states, reverses arc directions
```

### Epsilon Removal

```rust
use lling_llang::wfst::remove_epsilon;

let no_eps = remove_epsilon(&fst)?;
// All epsilon transitions removed
```

## Path Iteration

### AcceptingPathIterator

```rust
pub struct AcceptingPathIterator<'a, L, W: Semiring> {
    // ...
}

impl<'a, L: Clone, W: Semiring> Iterator for AcceptingPathIterator<'a, L, W> {
    type Item = WfstPath<L, W>;

    fn next(&mut self) -> Option<Self::Item>;
}

pub struct WfstPath<L, W: Semiring> {
    pub inputs: Vec<L>,
    pub outputs: Vec<L>,
    pub weight: W,
}
```

### Usage

```rust
// Iterate accepting paths
for path in fst.accepting_paths() {
    println!("Input: {:?}", path.inputs);
    println!("Output: {:?}", path.outputs);
    println!("Weight: {:?}", path.weight);
}

// Collect first 10 paths
let paths: Vec<_> = fst.accepting_paths().take(10).collect();
```

## Utility Functions

```rust
/// Check if WFST is deterministic
pub fn is_deterministic<L, W>(wfst: &impl Wfst<L, W>) -> bool
where
    L: Eq + Hash,
    W: Semiring;

/// Check if WFST is epsilon-free
pub fn is_epsilon_free<L, W>(wfst: &impl Wfst<L, W>) -> bool
where
    W: Semiring;

/// Compute total weight of all accepting paths
pub fn total_weight<L, W>(wfst: &impl Wfst<L, W>) -> W
where
    W: Semiring;

/// Count accepting paths
pub fn count_paths<L, W>(wfst: &impl Wfst<L, W>) -> Option<usize>
where
    W: Semiring;
```

## See Also

- [WFST Traits (Architecture)](../architecture/wfst-traits.md): Conceptual overview
- [Composition](../algorithms/composition.md): Lazy composition
- [Lattice Reference](lattice-reference.md): Lattice specialization
