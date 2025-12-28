# Weight Pushing

Weight pushing redistributes weights along paths to normalize their distribution, moving weights toward initial or final states. This is essential for minimization, beam search optimization, and equivalence testing.

## Concepts

### What is Weight Pushing?

Weight pushing transforms a WFST by redistributing weights along paths while preserving the total weight of each path. The weights are "pushed" toward either:

- **Forward (toward initial)**: Early transitions carry more weight
- **Backward (toward final)**: Late transitions carry more weight

```
Before:           After Backward Push:

 0 --1.0--> 1     0 --0.0--> 1
     |           |     |           |
    2.0         0.5   0.0         0.5
     |           |     |           |
     v           v     v           v
 2 --3.0--> 3    2 --0.0--> 3
   (f=0.0)         (f=1.0)

Path weight: 1+2+3+0 = 6    Path weight: 0+0+0+1 = 1
                            (but total = 6, absorbed in initial potential)
```

### Why Weight Pushing?

1. **Prerequisite for Minimization**: Weighted minimization requires pushed WFSTs
2. **Beam Search Pruning**: Log-semiring pushing dramatically improves pruning efficacy (18× speedup)
3. **Equivalence Testing**: Pushed WFSTs can be compared structurally
4. **Stochastic Normalization**: Creates automata where weights sum to 1 at each state

## Core API

### Types

```rust
// Push direction
pub enum PushDirection {
    Forward,   // Push toward initial state
    Backward,  // Push toward final states (default, recommended)
}

// General push configuration
pub struct PushConfig {
    pub direction: PushDirection,
    pub remove_non_coaccessible: bool,
    pub distance_config: ShortestDistanceConfig,
}

// Log-semiring push for beam search
pub struct LogPushConfig {
    pub verify_stochastic: bool,    // Check weights sum to 1
    pub stochastic_epsilon: f64,    // Tolerance for check
    pub normalize_finals: bool,      // Set final weights to one
}

// Result of beam search preparation
pub struct BeamSearchPrepResult {
    pub pushed: bool,
    pub total_weight: LogWeight,
    pub is_stochastic: Option<bool>,
    pub num_states: usize,
    pub num_transitions: usize,
}
```

### Functions

```rust
// General weight pushing (requires DivisibleSemiring)
pub fn push_weights<L, W, F>(
    fst: &mut F,
    config: PushConfig,
) -> Result<(), PushError>;

// Log-semiring pushing for beam search
pub fn prepare_for_beam_search<L, F>(
    fst: &mut F,
    config: LogPushConfig,
) -> Result<BeamSearchPrepResult, LogPushError>;

// Compute log-semiring potentials (backward)
pub fn compute_log_potentials<L, F>(
    fst: &F,
) -> Result<Vec<LogWeight>, LogPushError>;

// Check if WFST is stochastic
pub fn is_stochastic<L, W, F>(
    fst: &F,
    epsilon: f64,
) -> bool;
```

## The Critical Insight: Log vs Tropical

This is the most important concept in this document:

### Tropical Semiring Pushing (DON'T USE for beam search)

- Uses **minimum-weight** potential (best single path)
- Can actually **harm** beam search by distorting relative scores
- Quote from Mohri et al.: "May slow down beam-pruned Viterbi decoding many fold"

### Log Semiring Pushing (USE THIS for beam search)

- Uses **total probability** potential (sum of all path probabilities)
- Creates a **stochastic** automaton where weights sum to 1
- Quote: "Has a very large beneficial impact on pruning efficacy"
- Conjecture: "Optimal likelihood ratio test for pruning decisions"

```
                    ┌─────────────────────────────────────┐
                    │        CRITICAL FOR BEAM SEARCH     │
                    │                                     │
                    │   Use LogWeight + prepare_for_      │
                    │   beam_search() for up to 18×       │
                    │   speedup in beam-pruned decoding   │
                    │                                     │
                    └─────────────────────────────────────┘
```

## Examples

### Basic Backward Push

```rust
use lling_llang::prelude::*;
use lling_llang::algorithms::{push_weights, PushConfig};

let mut fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
    .add_states(3)
    .start(0)
    .arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0))
    .arc(1, Some('b'), Some('b'), 2, TropicalWeight::new(2.0))
    .final_state(2, TropicalWeight::new(0.5))
    .build();

// Push weights toward final states
push_weights(&mut fst, PushConfig::backward())?;

// After pushing:
// - Final weight becomes normalized
// - Transition weights are redistributed
// - Total path weight is preserved
```

### Beam Search Optimization

```rust
use lling_llang::prelude::*;
use lling_llang::optimization::{prepare_for_beam_search, LogPushConfig};

// Build a recognition WFST with log weights
let mut fst: VectorWfst<char, LogWeight> = VectorWfstBuilder::new()
    .add_states(4)
    .start(0)
    .arc(0, Some('a'), Some('a'), 1, LogWeight::new(1.0))
    .arc(0, Some('b'), Some('b'), 2, LogWeight::new(2.0))
    .arc(1, Some('c'), Some('c'), 3, LogWeight::new(1.0))
    .arc(2, Some('d'), Some('d'), 3, LogWeight::new(0.5))
    .final_state(3, LogWeight::one())
    .build();

// Prepare for beam search with log-semiring pushing
let result = prepare_for_beam_search(&mut fst, LogPushConfig::verified())?;

println!("Total weight: {:?}", result.total_weight);
println!("Is stochastic: {:?}", result.is_stochastic);

// Now beam search will have optimal pruning behavior
```

### Verifying Stochasticity

```rust
use lling_llang::algorithms::is_stochastic;

// After log-semiring pushing, the WFST should be stochastic
// (weights sum to 1 at each state in probability space)
if is_stochastic(&fst, 1e-6) {
    println!("WFST is stochastic - optimal for beam search");
} else {
    println!("WFST is not stochastic - check for issues");
}
```

## Algorithm Details

### Potential Functions

Weight pushing uses **potential functions** V(q) computed via shortest-distance:

**Forward Push (toward initial)**:
- V(q) = shortest distance from initial state to q
- Transition: w'(e) = V(source)⁻¹ ⊗ w(e) ⊗ V(target)
- Final: ρ'(q) = V(q)⁻¹ ⊗ ρ(q)

**Backward Push (toward final)**:
- V(q) = shortest distance from q to any final state
- Transition: w'(e) = w(e) ⊗ V(target) ⊗ V(source)⁻¹
- Final: ρ'(q) = 1̄ (normalized)

### Log-Semiring Potentials

For log semiring, the potential V(q) represents the **total probability** of all paths from q to final:

```
V(q) = -log(Σ_{paths π from q to final} exp(-weight(π)))
```

This is computed in reverse topological order:

```
V(final) = final_weight
V(q) = logadd_{outgoing arcs} (arc_weight + V(target))
```

Where `logadd(a, b) = -log(exp(-a) + exp(-b))`.

### Reweighting Invariant

Path weights are preserved under the reweighting:

```
                original                    after push
              ┌───────────┐               ┌───────────┐
              │           │               │           │
              │  w₁ ⊗ w₂  │       =       │  w'₁ ⊗ w'₂│
              │           │               │           │
              └───────────┘               └───────────┘

Because: The potentials cancel along the path
         V(s₁)⁻¹ ⊗ V(s₂) ⊗ V(s₂)⁻¹ ⊗ V(s₃) = V(s₁)⁻¹ ⊗ V(s₃)
```

## Performance

### Complexity

| Operation | Time | Space |
|-----------|------|-------|
| Compute potentials (acyclic) | O(\|Q\| + \|E\|) | O(\|Q\|) |
| Compute potentials (general) | O(\|E\| + \|Q\| log \|Q\|) | O(\|Q\|) |
| Apply push | O(\|Q\| + \|E\|) | O(\|E\|) |

### Beam Search Speedup

From the literature (Mohri et al., 2002):

| Configuration | Relative Speed |
|---------------|---------------|
| Unpushed | 1× |
| Tropical pushed | 0.2× (slower!) |
| **Log pushed** | **18×** |

The dramatic difference comes from optimal pruning decisions when weights represent true probabilities.

## Semiring Requirements

Weight pushing requires a **divisible semiring** (implements `DivisibleSemiring`):

| Semiring | Divisible | Push Supported |
|----------|-----------|----------------|
| Tropical | Yes | Yes |
| Log | Yes | Yes (recommended for beam search) |
| Probability | Yes | Yes |
| Boolean | No | No |
| String | No | No |

## Common Patterns

### Pre-Minimization Push

```rust
use lling_llang::algorithms::{push_weights, minimize, PushConfig, MinimizeConfig};

// Weight pushing is a prerequisite for minimization
push_weights(&mut fst, PushConfig::backward())?;
let minimized = minimize(&fst, MinimizeConfig::default())?;
```

### Cascade Optimization

For speech recognition cascades (H ∘ C ∘ L ∘ G):

```rust
// Build the cascade
let cascade = compose(&compose(&h, &c), &compose(&l, &g));

// Apply log-semiring pushing for optimal beam search
prepare_for_beam_search(&mut cascade, LogPushConfig::verified())?;

// Now decode with beam search
let result = beam_search(&cascade, &input, config);
```

## Error Handling

```rust
use lling_llang::algorithms::PushError;
use lling_llang::optimization::LogPushError;

match push_weights(&mut fst, config) {
    Ok(()) => { /* success */ }
    Err(PushError::NoStartState) => { /* no start state set */ }
    Err(PushError::NoPotentials) => { /* no path to finals */ }
    Err(PushError::DivisionByZero) => { /* weight division failed */ }
}

match prepare_for_beam_search(&mut fst, config) {
    Ok(result) => {
        if result.is_stochastic == Some(false) {
            // Unexpected: weights don't sum to 1
        }
    }
    Err(LogPushError::NoPathToFinal) => { /* disconnected */ }
    // ...
}
```

## Next Steps

- [Shortest-Distance](shortest-distance.md): Foundation for potential computation
- [Minimization](minimization.md): Uses weight pushing as prerequisite
- [Beam Optimization](../advanced/beam-optimization.md): Comprehensive beam search tuning
- [Semirings](../architecture/semirings.md): Understanding divisible semirings
