# Minimization

Minimization produces a WFST with the minimum number of states that accepts the same weighted language. This is the final optimization step in the standard WFST pipeline, reducing both states and transitions. (WFST = **W**eighted **F**inite-**S**tate **T**ransducer.)

## Terms & symbols

Defined centrally in [`../NOTATION.md`](../NOTATION.md); repeated locally for the terms this doc uses.

| Symbol | Meaning |
|---|---|
| $`\oplus`$ / $`\otimes`$ | semiring *plus* (combine alternatives) / *times* (combine arcs). |
| $`\bar{0}`$ / $`\bar{1}`$ | $`\oplus`$-identity / $`\otimes`$-identity. |
| $`\rho(q)`$ | final-weight function $`\rho : F \to K`$. |
| $`\equiv`$ | state-equivalence (Myhill-Nerode): same weighted future. |
| $`\Sigma^*`$ | all finite strings over the input alphabet $`\Sigma`$. |
| $`\lvert Q\rvert`$, $`\lvert E\rvert`$ | number of states / transitions. |

## Concepts

### What is Minimization?

Minimization identifies and merges **equivalent states**—states that behave identically for all possible continuations. In the example below, states 1 and 2 share a future (both read $`b`$ into a final class), as do 3 and 4 (both final with the same weight), so each pair collapses to a single class.

![Minimization before/after: a 5-state acceptor whose equivalent state pairs {1,2} and {3,4} merge into one class each, yielding a 3-state minimal acceptor](../diagrams/algorithms/minimize-before-after.svg)

*Amber states = equivalence class A $`\{1,2\}`$; green double-ring states = final class B $`\{3,4\}`$. Left panel is the input; right panel is the minimal automaton (one state per class); the green bold arc is the shared $`b`$-transition.*

<details><summary>Text view</summary>

```text
Before minimization:              After minimization:

    0 ──a──► 1 ──b──► 3 (final)       0 ──a──► 1 ──b──► 2 (final)
      │                                 │
      └──c──► 2 ──b──► 4 (final)        └──c──┘

States 3 and 4 are equivalent (same outgoing transitions, same final weight)
States 1 and 2 are also equivalent → merge them
```

</details>

### Why Minimize?

1. **Smaller automata**: Fewer states and transitions
2. **Faster recognition**: Less memory traversal
3. **Canonical form**: Equivalent WFSTs produce identical minimized forms
4. **Memory efficiency**: Reduced storage requirements

### The Minimization Pipeline

Minimization first validates the immutable input, then performs three transformation steps:

1. **Endpoint validation**: Reject an unrepresentable state count, an invalid start, or any invalid transition in the original input. This happens before trimming, pushing, partitioning, estimating, or constructing output.
2. **Weight pushing**: Normalize weight distribution to canonical form when enabled.
3. **Partition refinement**: Find equivalence classes of states.
4. **Build minimal WFST**: Create one state per equivalence class.

### Fail-closed input boundary

The input WFST is an immutable borrowed graph. An **owner state** is the state whose outgoing transition slice contains an arc; the arc also encodes a **source state** and **target state**. A **transition index** is its zero-based position in that outgoing slice. A **snapshot identity** is a caller-supplied key for an immutable graph version. Without one, diagnostics use the borrowed object's address, which is meaningful only during that object's lifetime and is not a content hash.

![Validation flow: original WFST is checked before any transform; the first invalid endpoint returns a typed error with provenance](../diagrams/algorithms/minimize-validation.svg)

For $`n`$ states, an arc in the slice of state $`q`$ is valid exactly when its encoded source equals $`q`$ and both endpoints are less than $`n`$. The state count must fit the `StateId` address space without using the reserved `NO_STATE` sentinel. An empty WFST has `NO_STATE` as start; a nonempty WFST starts at a state less than $`n`$.

```math
\mathrm{validArc}(q,a,n)
  \iff a.\mathrm{from}=q \land a.\mathrm{from}<n \land a.\mathrm{to}<n.
```

The validator is a single, allocation-free, iterative scan in state order, then outgoing-slice order. It reports the first malformed transition, making the diagnostic deterministic for a fixed input. Its cost is $`O(\lvert Q\rvert+\lvert E\rvert)`$ time and $`O(1)`$ auxiliary space. No arc is deleted, clamped, synthesized, or redirected as a repair. A malformed input cannot reach `connect`, weight pushing, partition refinement, reduction estimation, or output construction.

```text
⟨ validate original input ⟩ ≡
    reject if state count exceeds the representable range
    reject if start is invalid for that count
    for each state q in ascending order:
        for each outgoing arc a in slice order:
            reject with (input identity, q, a.from, a.to, arc index)
                if not validArc(q, a, state count)
    return success

⟨ minimize ⟩ ≡
    ⟨ validate original input ⟩
    check epsilon and determinism
    optionally connect and push weights
    refine partitions; construct output
```

The formal refinement model is [`MinimizeEndpointValidation.v`](../../proofs/coq/algorithms/MinimizeEndpointValidation.v). Its kernel-checked invariants map to executable checks as follows:

| Kernel-checked invariant | Generated Rust property |
|---|---|
| `valid_arcb_exact` | `valid_endpoints_are_accepted_without_mutating_the_input`, `source_owner_mismatch_is_rejected`, and the source/target endpoint mutants cover the exact owner/source/target predicate. |
| `first_invalid_none_iff` | `valid_endpoints_are_accepted_without_mutating_the_input` accepts fully valid scans; `generated_first_error_wins_in_state_and_slice_order` rejects a scan with an invalid arc. |
| `first_invalid_first_error` | `generated_first_error_wins_in_state_and_slice_order` varies the bad state and slice index, adds a later bad arc where possible, and checks the earliest error. |
| `first_invalid_error_identity` | `generated_first_error_wins_in_state_and_slice_order` checks the original snapshot identity and state count. |
| `first_invalid_complete` | `generated_first_error_wins_in_state_and_slice_order` requires rejection for every generated malformed source or target. |
| `validate_input_none_iff` | The valid-input, malformed-arc, invalid-start, unrepresentable-count, and empty-input properties exercise every acceptance branch. |
| `invalid_state_count_rejected_first` | `generated_unrepresentable_count_precedes_start_and_arc` checks count precedence over simultaneous start and arc faults on 64-bit targets. |
| `invalid_start_rejected_before_arcs` | `generated_invalid_start_precedes_malformed_arc` checks start precedence over a simultaneous arc fault. |
| `checked_then_preserves_valid_input` | `valid_endpoints_are_accepted_without_mutating_the_input` and `empty_input_with_sentinel_start_is_accepted` check valid input; `valid_worklist_partitions_match_moore` checks the independent valid-input partition oracle. |
| `checked_then_rejects_before_transform` | `malformed_target_reports_the_original_arc_before_any_transform` varies connect/push flags and checks rejection of the original malformed arc. |

The proof establishes the validation boundary and provenance properties; it does **not** prove the complete weighted minimization algorithm. Property tests in [`minimize_endpoint_properties.rs`](../../tests/minimize_endpoint_properties.rs) exercise those invariants, causal one-endpoint mutants, an explicit snapshot ID, and a 50,000-state scan on a 64-KiB thread stack. The independent Moore reference also rejects malformed endpoints and is compared with worklist refinement on valid generated WFSTs.

Run `sh scripts/verify-minimize-endpoints.sh` from the repository root to rebuild the Rocq proof, check its kernel dependencies, and run the Rust target and documentation tests under bounded user scopes. Its logs and temporary files stay under `target/` on persistent storage.

## Core API

### Types

`MinimizeConfig` controls `push_weights`, `push_direction`, `connect_first`, and the positive finite `weight_epsilon`. `MinimizeInputIdentity::Snapshot(u64)` is supplied by callers needing durable provenance; otherwise the API reports `BorrowedWfst(address)`.

`MinimizeError` distinguishes `InvalidStateCount`, `InvalidStartState`, `InvalidTransition`, `NotDeterministic`, `InvalidWeightEpsilon`, and `PushError`. `InvalidTransition` carries input identity, state count, owner state, encoded source and target, and transition index. `NoStartState` remains in the enum for compatibility but input validation now reports `InvalidStartState` with provenance.

### Functions

`minimize(&fst, config)` returns `Result<F, MinimizeError>`. `minimize_with_input_identity(&fst, identity, config)` uses a caller-supplied snapshot key. `estimate_reduction(&fst)` and `estimate_reduction_with_epsilon(&fst, epsilon)` now return `Result<usize, MinimizeError>`; `estimate_reduction_with_epsilon_and_input_identity` combines custom epsilon with a snapshot key. Estimates no longer collapse malformed-input or epsilon errors to zero.

## Examples

### Basic Usage

```rust
use lling_llang::prelude::*;
use lling_llang::algorithms::{
    determinize, minimize,
    DeterminizeConfig, MinimizeConfig,
};

// Build a WFST with redundant states
let mut fst = VectorWfst::<char, TropicalWeight>::new();
fst.add_states(5);
fst.set_start(0);
fst.add_arc(0, Some('a'), Some('a'), 1, TropicalWeight::new(1.0));
fst.add_arc(0, Some('c'), Some('c'), 2, TropicalWeight::new(1.0));
fst.add_arc(1, Some('b'), Some('b'), 3, TropicalWeight::new(1.0));
fst.add_arc(2, Some('b'), Some('b'), 4, TropicalWeight::new(1.0));
fst.set_final(3, TropicalWeight::one());
fst.set_final(4, TropicalWeight::one());

// States 3 and 4 are equivalent (same final weight, same transitions)
// States 1 and 2 are equivalent (same outgoing, target same equivalence class)

let initial_states = fst.num_states();

// Minimize
let min_fst = minimize(&fst, MinimizeConfig::standard())?;

// Fewer states after minimization
assert!(min_fst.num_states() < initial_states);
```

### Full Optimization Pipeline

```rust
use lling_llang::algorithms::{
    remove_epsilon, determinize, minimize,
    EpsilonRemovalConfig, DeterminizeConfig, MinimizeConfig,
};

// Standard WFST optimization pipeline:
// 1. Remove epsilon transitions
remove_epsilon(&mut fst, EpsilonRemovalConfig::default())?;

// 2. Determinize (required for minimization)
let det = determinize(&fst, DeterminizeConfig::standard())?;

// 3. Minimize
let min = minimize(&det, MinimizeConfig::standard())?;

println!("Original: {} states", fst.num_states());
println!("Minimized: {} states", min.num_states());
```

### Estimating Reduction

```rust
use lling_llang::algorithms::estimate_reduction;

// Before expensive minimization, check if it's worthwhile
let reduction = estimate_reduction(&fst)?;

if reduction > 0 {
    println!("Can remove {} states via minimization", reduction);
    let min_fst = minimize(&fst, MinimizeConfig::standard())?;
} else {
    println!("Already minimal");
}
```

### Without Weight Pushing

```rust
// If input is already pushed, skip pushing step
let config = MinimizeConfig {
    push_weights: false,  // Skip pushing
    connect_first: true,
    ..Default::default()
};

let min_fst = minimize(&pushed_fst, config)?;
```

## Algorithm Details

### Partition Refinement

The algorithm uses **Hopcroft-style partition refinement** ([Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)). The invariant is that two states sharing a block could still be equivalent; each refinement round splits a block whenever two of its states are *distinguished* by their signature — their final weight or the block-labelled profile of their outgoing arcs. The partition only ever gets finer, so the fixpoint is the coarsest stable partition: exactly the equivalence classes.

<details><summary>Text view</summary>

```text
procedure PARTITION_REFINEMENT(fst):
    partition ← separate states by final_weight        // initial split
    repeat:
        for each state q:
            signature[q] ← (final_weight(q), {(label, weight, partition[target])})
        new_partition ← group states by signature
    until partition == new_partition
    return partition
```

</details>

```text
⟨ initial partition by final weight ⟩ ≡
    partition ← { states with the same ρ(q) value go in one block }
    // non-final states (ρ = 0̄) form one block; each distinct final weight its own
```

```text
⟨ compute a state signature ⟩ ≡
    signature[q] ← ( ρ(q),  multiset{ (in, out, w, partition[target]) : arc q→target } )
    // two states distinguishable ⟺ different signatures under the CURRENT partition
```

```text
⟨ refine until stable ⟩ ≡
    repeat:
        for each state q:  ⟨ compute a state signature ⟩
        new_partition ← group states by equal signature
        swap(partition, new_partition)
    until partition unchanged
```

```text
⟨ partition refinement ⟩ ≡
    ⟨ initial partition by final weight ⟩
    ⟨ refine until stable ⟩
    return partition
```

The refinement terminates because each round either splits at least one block (strictly
increasing the block count, bounded by $`\lvert Q\rvert`$) or changes nothing and stops. A
worklist implementation of this scheme attains Hopcroft's $`\mathcal{O}(\lvert E\rvert \log \lvert Q\rvert)`$ bound.

### State Signatures

A state's **signature** captures everything needed to distinguish it:

```rust
struct StateSignature<L, W> {
    final_weight: Option<W>,
    transitions: Vec<(input_label, output_label, weight, target_partition)>,
}
```

Two states are equivalent if and only if they have identical signatures.

### Building the Minimal WFST

Once partitions are computed:

```text
1. Create one state per partition
2. Choose representative from each partition
3. Copy transitions from representative to new state
4. Map target states to their partition numbers
```

```text
Partitions:                    Minimal WFST:

  [0]: {0}                        0' (start)
  [1]: {1, 2}                     1' (merged from 1,2)
  [2]: {3, 4}                     2' (merged from 3,4, final)
```

### Why Weight Pushing First?

Weight pushing ensures a **canonical weight distribution**:

```text
Before pushing:                    After pushing:
  0 --a/2--> 1 --b/3--> (F)         0 --a/5--> 1 --b/0--> (F)
  0 --a/2--> 2 --b/3--> (F)         0 --a/5--> 2 --b/0--> (F)

  States 1,2 have same             States 1,2 now have
  transitions but different         identical signatures
  weight distributions              → Can be merged!
```

Without pushing, equivalent states might have different weight distributions, preventing their identification.

## Complexity

### Time Complexity

| Case | Complexity |
|------|------------|
| Acyclic | $`\mathcal{O}(\lvert Q\rvert + \lvert E\rvert)`$ |
| General | $`\mathcal{O}(\lvert E\rvert \log \lvert Q\rvert)`$ |

The $`\mathcal{O}(\lvert E\rvert \log \lvert Q\rvert)`$ bound comes from Hopcroft's algorithm for partition refinement.

### Space Complexity

| Structure | Size |
|-----------|------|
| Partition array | $`\mathcal{O}(\lvert Q\rvert)`$ |
| Signatures | $`\mathcal{O}(\lvert Q\rvert \times \text{avg\_out\_degree})`$ |
| Output WFST | $`\mathcal{O}(\lvert Q'\rvert + \lvert E'\rvert)`$ where $`\lvert Q'\rvert \le \lvert Q\rvert`$ |

## Requirements

### Deterministic Input

Minimization **requires deterministic input**:

```rust
let result = minimize(&non_det_fst, config);
// Returns Err(MinimizeError::NotDeterministic)

// Solution: determinize first
let det = determinize(&fst, DeterminizeConfig::standard())?;
let min = minimize(&det, MinimizeConfig::standard())?;
```

### Divisible Semiring

Weight pushing (part of minimization) requires a divisible semiring:

| Semiring | Divisible | Minimizable |
|----------|-----------|-------------|
| Tropical | Yes | Yes |
| Log | Yes | Yes |
| Probability | Yes | Yes |
| Boolean | No | No |
| String | No | No |

## Common Patterns

### ASR Transducer Optimization

In speech recognition, the full cascade is minimized:

```rust
// Build recognition transducer
let cascade = compose(&h, &compose(&c, &compose(&l, &g)));

// Standard optimization pipeline
remove_epsilon(&mut cascade, EpsilonRemovalConfig::default())?;
let det = determinize(&cascade, DeterminizeConfig::standard())?;
let min = minimize(&det, MinimizeConfig::standard())?;

// Typically: 30-50% state reduction
```

### Equivalence Testing

Minimized WFSTs provide a canonical form for equivalence testing:

```rust
let min1 = minimize(&fst1, MinimizeConfig::standard())?;
let min2 = minimize(&fst2, MinimizeConfig::standard())?;

// If minimal forms are isomorphic, WFSTs are equivalent
let equivalent = are_isomorphic(&min1, &min2);
```

### Incremental Minimization

For large WFSTs, check if minimization is worthwhile:

```rust
let reduction = estimate_reduction(&fst)?;
let ratio = reduction as f64 / fst.num_states() as f64;

if ratio > 0.1 {  // >10% reduction
    let min = minimize(&fst, MinimizeConfig::standard())?;
    // Use minimized version
} else {
    // Reduction too small, skip minimization
}
```

## Visualization

The [before/after diagram](#what-is-minimization) above renders this reduction; the ASCII views are kept here for reference.

### Before Minimization

```text
                 a/1.0         b/1.0
          [0] ─────────► 1 ─────────► (3)
            │
            │ c/1.0         b/1.0
            └─────────► 2 ─────────► (4)

States: 5
Transitions: 4

Equivalent pairs: {1,2}, {3,4}
```

### After Minimization

```text
                 a/1.0
          [0] ─────────► 1 ─────────► (2)
            │             ▲
            │ c/1.0       │ b/1.0
            └─────────────┘

States: 3 (40% reduction)
Transitions: 3
```

### Partition Evolution

```text
Iteration 0: Initial partition by final weight
  P0 = {0, 1, 2}     (non-final)
  P1 = {3, 4}        (final, weight=0̄... here ρ=0)

Iteration 1: Refine by transitions
  P0 = {0}           (start, has a/c arcs)
  P1 = {1, 2}        (has b arc to P2)
  P2 = {3, 4}        (final, no arcs)

Iteration 2: Stable (no change)
  Final partitions: {0}, {1,2}, {3,4}
```

## Error Handling

```rust
use lling_llang::algorithms::MinimizeError;

match minimize(&fst, config.clone()) {
    Ok(min) => {
        println!("Minimized: {} -> {} states",
                 fst.num_states(), min.num_states());
    }
    Err(MinimizeError::InvalidTransition { owner_state, transition_index, .. }) => {
        // Repair the original input arc at this exact position; no arc was dropped.
        println!("Malformed arc {} in state {}", transition_index, owner_state);
    }
    Err(MinimizeError::InvalidStartState { start_state, .. }) => {
        println!("Invalid start state: {}", start_state);
    }
    Err(MinimizeError::InvalidStateCount { state_count, .. }) => {
        println!("Unrepresentable state count: {}", state_count);
    }
    Err(MinimizeError::NotDeterministic) => {
        // Must determinize first
        let det = determinize(&fst, DeterminizeConfig::standard())?;
        let min = minimize(&det, config)?;
    }
    Err(MinimizeError::PushError(msg)) => {
        // Weight pushing failed (e.g., no path to final)
        println!("Push failed: {}", msg);
    }
    Err(MinimizeError::InvalidWeightEpsilon { epsilon }) => {
        println!("Invalid epsilon: {}", epsilon);
    }
    Err(MinimizeError::NoStartState) => {
        // Compatibility variant; current input validation uses InvalidStartState.
    }
}
```

## Performance Tips

1. **Determinize first**: Minimization requires deterministic input
2. **Connect before minimizing**: Removes unreachable states early
3. **Estimate first**: Use `estimate_reduction()` for large WFSTs
4. **Skip if already minimal**: Simple chains are often already minimal
5. **Choose push direction**: Forward vs backward may affect intermediate size

## Theoretical Notes

### Myhill-Nerode Theorem

The minimal WFST corresponds to the Myhill-Nerode equivalence relation:

```math
q_1 \equiv q_2 \iff \forall w \in \Sigma^* : \mathrm{weight}(q_1, w) = \mathrm{weight}(q_2, w)
```

Two states are equivalent if they produce identical weights for all continuations.

### Uniqueness

The minimal WFST is **unique up to isomorphism**—all minimal WFSTs for the same language have identical structure (modulo state renaming and weight distribution).

### States vs Transitions

**Theorem** ([Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)): Minimizing states also minimizes transitions.

This means the minimal WFST is optimal in both metrics simultaneously.

## References

- [Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) — *Weighted Automata Algorithms*: weighted minimization, the push-then-partition-refine pipeline, the Hopcroft $`\mathcal{O}(\lvert E\rvert \log \lvert Q\rvert)`$ bound, and the states-also-minimizes-transitions theorem.
- [Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002) — *Weighted Finite-State Transducers in Speech Recognition*: minimization as the final stage of the recognition-cascade optimization, with reported state reductions.
- [Allauzen 2007](../BIBLIOGRAPHY.md#ref-allauzen2007) — *OpenFst*: the `Minimize` operation and equivalence-by-isomorphism testing this implementation mirrors.

## Related Topics

- [Determinization](determinization.md): Required before minimization
- [Weight Pushing](weight-pushing.md): Part of minimization pipeline
- [Epsilon Removal](epsilon-removal.md): Often precedes determinization
- [WFST Operations](../architecture/wfst-operations.md): Building WFSTs to minimize
