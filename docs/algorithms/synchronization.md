# Synchronization

Synchronization normalizes the input/output label shifting in transducers, producing a synchronized form where labels are emitted in a controlled manner. This is useful for canonical representation and for certain composition operations.

## Concepts

### What is Synchronization?

In a transducer, input and output labels can be "out of sync"—you might consume several input symbols before producing any output, or vice versa. The **delay** tracks this difference:

```
Delay = |output consumed| - |input consumed|

Example path:
  a:ε → ε:x → b:y
    ↓      ↓     ↓
  d=-1   d=0   d=0

After 'a': consumed 1 input, 0 output → delay = -1
After 'x': consumed 1 input, 1 output → delay = 0 (synchronized)
After 'b:y': consumed 2 inputs, 2 outputs → delay = 0
```

Synchronization transforms a transducer so that delays are handled in a canonical way, with draining states to emit residual symbols at final states.

### String Delay

The **string delay** represents the accumulated difference between input and output:

```rust
struct StringDelay<L> {
    input: SmallVec<[L; 4]>,   // Residual input (consumed but not matched)
    output: SmallVec<[L; 4]>,  // Residual output (produced but not matched)
}
```

At any point, only one of `input` or `output` is non-empty:
- If we've consumed more input than output: `input` has the excess
- If we've produced more output than input: `output` has the excess

### Bounded Delay

A transducer has **bounded delay** if:
1. All cycles have zero delay (equal input and output lengths)
2. The maximum delay on any path is finite

```
Bounded delay:              Unbounded delay:

  0 ─a:x─► 1 ─b:y─► 2        0 ─a:x─► 1 ─ε:y─┐
                                       ↖────┘
  Cycle: none                 Cycle adds output without input
  Delay: always 0             → delay grows unboundedly
```

Synchronization **requires bounded delay** to terminate.

## Core API

### Types

```rust
/// Accumulated input/output difference
pub struct StringDelay<L> {
    pub input: SmallVec<[L; 4]>,
    pub output: SmallVec<[L; 4]>,
}

/// State in synchronized transducer: (original_state, delay, draining_flag)
pub struct SyncState<L> {
    pub original: StateId,
    pub delay: StringDelay<L>,
    pub draining: bool,
}

/// Mutable synchronization source (handles state creation)
pub struct MutableSyncSource<L, W, T> { ... }
```

### Functions

```rust
/// Synchronize a transducer lazily
pub fn synchronize<L, W, T>(fst: &T) -> SyncWfst<L, W, T>;

/// Synchronize with explicit delay bound
pub fn synchronize_bounded<L, W, T>(fst: &T, max_delay: usize) -> SyncWfst<L, W, T>;

/// Check if transducer has bounded delay
pub fn has_bounded_delay<L, W, T>(fst: &T) -> bool;

/// Compute maximum delay (None if unbounded)
pub fn compute_max_delay<L, W, T>(fst: &T) -> Option<usize>;
```

## Examples

### Checking Bounded Delay

```rust
use lling_llang::prelude::*;
use lling_llang::wfst::synchronize::{has_bounded_delay, compute_max_delay};

// Simple 1:1 transducer (balanced input/output)
let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
    .add_states(2)
    .start(0)
    .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
    .final_state(1, TropicalWeight::one())
    .build();

assert!(has_bounded_delay(&fst));
assert_eq!(compute_max_delay(&fst), Some(0));  // Always synchronized
```

### Detecting Unbounded Delay

```rust
// Transducer with unbounded delay (cycle adds output)
let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
    .add_states(2)
    .start(0)
    .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
    .arc(1, None, Some('y'), 1, TropicalWeight::one())  // ε:y self-loop!
    .final_state(1, TropicalWeight::one())
    .build();

// This cycle increases output without consuming input
assert!(!has_bounded_delay(&fst));
assert!(compute_max_delay(&fst).is_none());
```

### Basic Synchronization

```rust
use lling_llang::wfst::synchronize::synchronize;

let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
    .add_states(3)
    .start(0)
    .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
    .arc(1, Some('b'), None, 2, TropicalWeight::one())  // Input only
    .final_state(2, TropicalWeight::one())
    .build();

assert!(has_bounded_delay(&fst));

// Synchronize the transducer
let synced = synchronize(&fst);

// The synchronized version normalizes the delay handling
assert_eq!(synced.start(), 0);
```

### Using MutableSyncSource

```rust
use lling_llang::wfst::synchronize::MutableSyncSource;

let fst: VectorWfst<char, TropicalWeight> = /* ... */;

// Create mutable synchronization source
let mut sync = MutableSyncSource::new(fst, 10);  // max_delay = 10

// Expand states on demand
sync.expand_state(0);  // Expand start state

// Check if transitions are computed
if sync.is_expanded(0) {
    let transitions = sync.transitions(0);
    for trans in transitions {
        println!("{:?}", trans);
    }
}
```

## Algorithm Details

### Synchronized States

States in the synchronized transducer are triplets:

```
(q, x, y) where:
  q = original state ID
  x = input delay string (residual input)
  y = output delay string (residual output)

Only one of x or y is non-empty at any time.
```

### State Transitions

For an original transition `p --a:b/w--> q`:

```
From synchronized state (p, delay):

1. Extend delays:
   new_input = delay.input ++ [a] if a ≠ ε
   new_output = delay.output ++ [b] if b ≠ ε

2. Synchronize (cancel common prefix):
   while new_input[0] == new_output[0]:
       remove first from both

3. Create output transition:
   - Output label: first symbol from synchronized delay
   - Target: (q, remaining_delay)
```

### Draining States

When a final state has non-empty delay, we need to "drain" the residual:

```
Original: state q is final with delay = [a, b]

Synchronized:
  (q, [a,b]) --a:ε--> (DRAIN, [b])
             --ε:ε--> final         if delay empty

  (DRAIN, [b]) --b:ε--> (DRAIN, [])
  (DRAIN, []) = final state
```

This ensures all accumulated symbols are emitted before accepting.

### Delay Synchronization (Common Prefix Cancellation)

```rust
fn sync(input: Vec<L>, output: Vec<L>) -> StringDelay<L> {
    // Cancel common prefix
    while !input.is_empty() && !output.is_empty() && input[0] == output[0] {
        input.remove(0);
        output.remove(0);
    }
    StringDelay { input, output }
}
```

Example:
```
Input:  [a, b, c]
Output: [a, b, x]

After sync:
  Input:  [c]
  Output: [x]
```

## Complexity

### Time Complexity

```
O((|Q| + |E|) × (|Σ|^d + |Δ|^d))
```

Where:
- |Q| = original states
- |E| = original transitions
- d = maximum delay
- |Σ| = input alphabet size
- |Δ| = output alphabet size

### Space Complexity

```
O(|Q| × |Σ|^d × |Δ|^d)
```

This can be exponential in the delay, which is why bounded delay is required.

### Why Bounded Delay Matters

If delay is unbounded, the synchronized transducer has infinitely many states:

```
Unbounded cycle: ε:y self-loop

States created:
  (q, [], [y])
  (q, [], [y, y])
  (q, [], [y, y, y])
  ...infinitely many
```

The `max_delay` parameter prevents runaway expansion.

## Special Cases

### Zero-Delay Transducers

If every transition has balanced input/output:

```rust
// Every arc has matching input/output
let fst: VectorWfst<char, TropicalWeight> = VectorWfstBuilder::new()
    .add_states(3)
    .start(0)
    .arc(0, Some('a'), Some('x'), 1, TropicalWeight::one())
    .arc(1, Some('b'), Some('y'), 2, TropicalWeight::one())
    .final_state(2, TropicalWeight::one())
    .build();

// Zero delay: synchronized form ≈ original
assert_eq!(compute_max_delay(&fst), Some(0));
```

### Epsilon Transitions

Epsilon transitions affect delay:

```
a:ε  → delay decreases (input consumed, no output)
ε:b  → delay increases (output produced, no input)
ε:ε  → delay unchanged (neither consumed nor produced)
```

### Empty Transducer

```rust
let fst: VectorWfst<char, TropicalWeight> = VectorWfst::new();

assert!(has_bounded_delay(&fst));
assert_eq!(compute_max_delay(&fst), Some(0));
```

## Common Patterns

### Pre-Synchronization Check

```rust
if has_bounded_delay(&fst) {
    let synced = synchronize(&fst);
    // Use synchronized transducer
} else {
    // Handle unbounded case
    eprintln!("Cannot synchronize: unbounded delay");
}
```

### With Delay Limit

```rust
use lling_llang::wfst::synchronize::synchronize_bounded;

// Limit delay to 10 symbols (prunes paths exceeding this)
let synced = synchronize_bounded(&fst, 10);
```

### Computing Delay Statistics

```rust
if let Some(max_delay) = compute_max_delay(&fst) {
    println!("Maximum delay: {}", max_delay);
    if max_delay == 0 {
        println!("Transducer is already synchronized");
    } else {
        println!("Synchronization will expand states by factor ~{}",
                 (input_alphabet_size + output_alphabet_size).pow(max_delay as u32));
    }
} else {
    println!("Unbounded delay - cannot synchronize");
}
```

## Visualization

### Before Synchronization

```
        a:ε                b:x                c:y
  [0] ───────► 1 ───────► 2 ───────► (3)

Path "abc" → "xy":
  After a: delay = -1 (input ahead)
  After b: delay = 0 (synchronized)
  After c: delay = 0 (synchronized)
```

### After Synchronization

```
Synchronized states track delay:

  [(0,[])] ──a:ε──► (1,[a])        // delay = [a,] (input residual)
           ──b:a──► (2,[])         // emit 'a', synchronized
           ──c:x──► (3,[])         // emit 'x', delay back to 0
           ...
           ──ε:y──► (FINAL)        // drain 'y'
```

### Draining at Final State

```
Original final state with residual delay:

  (q, [remaining...]) ──first:ε──► (DRAIN, [rest...])
                                        ↓
  (DRAIN, [rest...]) ──next:ε──► (DRAIN, [more...])
                                        ↓
  (DRAIN, []) = FINAL (accept)
```

## Performance Tips

1. **Check bounded delay first**: Use `has_bounded_delay()` before synchronizing
2. **Set reasonable max_delay**: Use `synchronize_bounded()` for potentially large delays
3. **Consider delay statistics**: `compute_max_delay()` helps estimate expansion
4. **Lazy evaluation**: The `SyncWfst` is lazy—only visited states are computed

## Theoretical Notes

### Relationship to Other Operations

- **Composition**: Synchronized transducers compose more predictably
- **Determinization**: Synchronization can be applied before or after
- **Equivalence**: Synchronized form is one canonical representation

### Double-Tape Semantics

Synchronization relates to viewing a transducer as operating on two tapes:
- Input tape (consumed left-to-right)
- Output tape (produced left-to-right)

The delay is the difference in "head positions" on these tapes.

### Decidability

- **Bounded delay**: Decidable in polynomial time (DFS cycle detection)
- **Synchronization**: Computable for bounded-delay transducers
- **Unbounded delay**: Synchronization does not terminate (infinite state space)

## Next Steps

- [Determinization](determinization.md): Often used with synchronization
- [Epsilon Removal](epsilon-removal.md): Can affect delay patterns
- [WFST Operations](../architecture/wfst-operations.md): Building transducers
- [Composition](composition.md): Synchronized transducers in composition
