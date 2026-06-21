# Weighted Pushdown Automata

A **weighted pushdown automaton (PDA)** is a finite automaton augmented with an
unbounded **stack**: on each transition it consumes an input symbol (or `ε`),
inspects the symbol on top of the stack, and rewrites the stack top. The stack is
what lets a PDA count and nest — recognizing context-free languages such as
balanced brackets or `` `aⁿbⁿ` `` that no finite-state machine can. This module
([`src/pushdown/`](../../src/pushdown/)) attaches semiring weights to that model,
so each accepting computation carries a `⊗`-weight and a language gets a
`⊕`-aggregated score.

---

## Terms & symbols

Shared notation lives in [`NOTATION.md`](../NOTATION.md); the acronyms **PDA**
(Pushdown Automaton), **CFL** (Context-Free Language), and **ID** (Instantaneous
Description) are expanded there. Locally:

| Symbol / term | Meaning |
|---|---|
| `Q` | Finite set of states. |
| `Σ` | Input alphabet (the Rust label type `L`). |
| `Γ` | Stack alphabet (`StackSymbol`, a wrapped `u32`). |
| `q₀` | Start state. |
| `Z₀` | The initial / bottom-of-stack marker, `StackSymbol::BOTTOM` (= `γ0`). |
| `F ⊆ Q` | Final states. |
| `Δ` | Transition relation (weighted; see below). |
| `ρ` | Final-weight function `ρ : F → K`. |
| `K` | Carrier of the weight semiring `W`. |
| `⊗`, `⊕` | Semiring *times* (sequential) and *plus* (alternative). |
| `0̄`, `1̄` | The `⊕`- and `⊗`-identities. |
| `γ` | A stack symbol in `Γ` (drawn `γ1`, `γ2`, …; `Z₀ = γ0`). |
| `(q, w, γ)` | An **instantaneous description** (ID): current state, remaining input, stack. |
| `⊢` | The "yields in one step" relation between IDs. |

---

## Formal model

A weighted pushdown automaton is the tuple

`` `P = (Q, Σ, Γ, q₀, Z₀, F, Δ, ρ)` ``

with components:

| Component | Type | Role |
|---|---|---|
| `Q` | finite set | States. |
| `Σ` | alphabet | Input symbols. |
| `Γ` | alphabet | Stack symbols, with distinguished bottom `Z₀ ∈ Γ`. |
| `q₀ ∈ Q` | state | Start state. |
| `Z₀ ∈ Γ` | symbol | The symbol initially on the stack. |
| `F ⊆ Q` | state subset | Final states. |
| `Δ` | relation | Transitions `(q, a, X) → (q′, σ, w)`: from `q`, optionally reading `a ∈ Σ ∪ {ε}`, with `X ∈ Γ` on top, go to `q′`, apply stack action `σ`, weight `w ∈ K`. |
| `ρ` | `F → K` | Final weight of an accepting state. |

A **stack action** `σ` (`StackAction`) is one of four operations applied to the
top of the stack `X`:

| Action | Effect on the stack (top at right) | Net height change |
|---|---|---|
| `Pop` | remove `X` | `−1` |
| `Push(γ₁…γₘ)` | remove `X`, then push `γ₁…γₘ` (so `γₘ` ends on top) | `m − 1` |
| `Replace(γ₁…γₘ)` | remove `X`, then push `γ₁…γₘ` (an explicit pop-then-push) | `m − 1` |
| `Noop` | leave `X` in place | `0` |

> **Why `Push` and `Replace` both pop first.** In this library a transition is
> *guarded* by the required top symbol `X`, and applying the action always
> consumes that matched `X` before pushing the new sequence — so
> `` `Push([Z₀, γ])` `` on top `Z₀` yields stack `…Z₀γ`. `Replace` is the same
> operation, named to document intent; only `Noop` leaves the matched symbol in
> place. (See [`StackAction::apply`](../../src/pushdown/stack.rs).)

### Instantaneous descriptions

A configuration, or **instantaneous description (ID)**, is the triple

`` `(q, w, γ)` ``

— state `q ∈ Q`, remaining input `w ∈ Σ*`, and stack contents `γ ∈ Γ*` (the
top at the right end). One transition step is the relation `⊢`:

```text
(q, a·w, β·X)  ⊢  (q′, w, β·σ(X))     when (q, a, X) → (q′, σ, w′) ∈ Δ
(q, w,   β·X)  ⊢  (q′, w, β·σ(X))     when (q, ε, X) → (q′, σ, w′) ∈ Δ   (ε-move, input unchanged)
```

The start ID is `` `(q₀, w, Z₀)` `` for input `w`. The weight of a run is the
`⊗`-product of the weights of the `Δ`-steps it uses (closed by `ρ` when accepting
in a final state).

### Acceptance modes

The library supports the three classical acceptance conditions, selected by
[`PdaAcceptMode`](../../src/pushdown/traits.rs); an ID is accepting only when the
input is fully consumed (`w = ε`):

| `PdaAcceptMode` | Accepts when input is exhausted **and** … | Accepting weight |
|---|---|---|
| `FinalState` (default) | the state is in `F` | `ρ(q)` |
| `EmptyStack` | the stack is empty | `1̄` |
| `Both` | state ∈ `F` **or** stack empty | `ρ(q)`, else `1̄` |

It is a standard result that the three modes recognize exactly the
context-free languages and are inter-convertible
([Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)).

---

## Intuition: recognizing `aⁿbⁿ`

The canonical non-regular language `` `{ aⁿbⁿ ∣ n ≥ 1 }` `` needs memory of *how
many* `a`s were seen — exactly what a stack provides. The
[`PdaBuilder::a_n_b_n`](../../src/pushdown/builder.rs) construction uses three
states and one auxiliary stack symbol `γ` (`StackSymbol::new(1)`):

1. In `q₀` ("read `a`"), each `a` **pushes** a marker `γ` (the first on `Z₀`, the
   rest on `γ`). The stack height records `n`.
2. An `ε`-move on a `γ` top switches to `q₁` ("read `b`").
3. In `q₁`, each `b` **pops** one `γ` — matching one `b` per `a`.
4. An `ε`-move on `Z₀` (all markers gone) accepts in the final state `q₂`.

So `aabb` drives the stack `Z₀ → Z₀γ → Z₀γγ → Z₀γγ → Z₀γ → Z₀`, ending on `Z₀`
with the input exhausted — accept. The string `aab` would leave a `γ` unmatched,
and `abb` would try to pop from `Z₀` — both rejected. This machine is drawn in the
[Diagrams](#diagrams) section, with its full ID trace.

---

## Architecture & API

| Item | Kind | Responsibility |
|---|---|---|
| [`WeightedPda<L, W>`](../../src/pushdown/traits.rs) | trait | Structural queries + acceptance predicates (`start`, `initial_stack`, `accept_mode`, `transitions`, `is_accepting`, `accepting_weight`). |
| [`VectorPda<L, W>`](../../src/pushdown/vector.rs) | struct | Default implementation; also hosts the executable `accepts` / `total_weight` / `approximate_fst`. |
| [`PdaState<L, W>`](../../src/pushdown/vector.rs) | struct | One state: `is_final`, `final_weight`, outgoing `transitions`. |
| [`PdaBuilder<L, W>`](../../src/pushdown/builder.rs) | struct | Construction with stack-symbol allocation and `add_push/pop/replace/read` helpers; canned `balanced_brackets`, `a_n_b_n`, `palindrome_with_center`. |
| [`StackSymbol`](../../src/pushdown/stack.rs) | struct | A `u32` stack symbol; `BOTTOM` is `Z₀`. |
| [`StackAction`](../../src/pushdown/stack.rs) | enum | `Pop` / `Push` / `Replace` / `Noop`, with `apply(&mut Vec<StackSymbol>)`. |
| [`PdaTransition<L, W>`](../../src/pushdown/transition.rs) | struct | An arc `{ from, input, stack_top, stack_action, to, weight }`. |
| [`PdaConfiguration<L>`](../../src/pushdown/traits.rs) | struct | An ID `{ state, remaining_input, stack }` with `apply_transition`. |
| [`PdaAcceptMode`](../../src/pushdown/traits.rs) | enum | `FinalState` / `EmptyStack` / `Both`. |

The `PdaConfiguration` *is* the ID `(q, w, γ)`: `state` is `q`, `remaining_input`
is `w`, and `stack` is `γ` with the top at the end. Its `apply_transition`
realizes a single `⊢` step — it checks the stack top, optionally consumes input,
and applies the stack action:

```rust
// from src/pushdown/traits.rs — one ⊢ step
use lling_llang::pushdown::{PdaConfiguration, PdaTransition, StackSymbol, StackAction};
use lling_llang::semiring::{Semiring, TropicalWeight};

let config: PdaConfiguration<char> =
    PdaConfiguration::new(0, vec!['a', 'b'], vec![StackSymbol::BOTTOM]);
let trans = PdaTransition::<char, TropicalWeight>::new(
    0, Some('a'), StackSymbol::BOTTOM,
    StackAction::Push(vec![StackSymbol::BOTTOM, StackSymbol::new(1)]),
    1, TropicalWeight::one(),
);
let next = config.apply_transition(&trans).expect("a on Z₀ is enabled");
assert_eq!(next.state, 1);
assert_eq!(next.remaining_input, vec!['b']);                 // 'a' consumed
assert_eq!(next.stack, vec![StackSymbol::BOTTOM, StackSymbol::new(1)]);  // pushed γ1
```

---

## Algorithms

### Recognition by configuration stepping

`VectorPda::accepts` decides membership by exploring the space of reachable IDs
breadth-first. The invariant is that the BFS queue holds exactly the IDs reachable
from the start ID `` `(q₀, w, Z₀)` `` that have not yet been expanded, and the
`visited` set keys on `(state, ∣remaining input∣, stack)` so that no ID is
expanded twice — which terminates the search on `ε`-cycles that revisit a
configuration.

```text
⟨ PDA accepts w ⟩ ≡
  queue   ← [ (q₀, w, [Z₀]) ]                         ⟨ start ID ⟩
  visited ← ∅
  while queue not empty:
      C ← queue.pop_front()
      key ← (C.state, ∣C.remaining∣, C.stack)
      if key ∈ visited: continue
      visited ← visited ∪ {key}
      if accepting(C): return true                    ⟨ input exhausted, mode satisfied ⟩
      X ← top(C.stack); if none: continue
      for e in ε-arcs(C.state, X):                    ⟨ ε-moves first ⟩
          if C′ ← e.apply(C): queue.push_back(C′)
      if a ← C.next_input():                          ⟨ then consume one symbol ⟩
          for e in arcs(C.state, a, X) with e non-ε:
              if C′ ← e.apply(C): queue.push_back(C′)
  return false
```

The chunk `` ⟨ input exhausted, mode satisfied ⟩ `` calls `is_config_accepting`,
which applies the [acceptance mode](#acceptance-modes) table. The two arc loops
correspond to the `ε`-transition and input-consuming branches of `⊢`.

**Complexity.** The state space is the set of distinct IDs. The `visited` key
bounds revisits, but the *stack* component can grow without bound (a PDA's stack
is unbounded by definition), so in the worst case the number of reachable IDs —
and hence the running time — is not polynomial in `∣w∣`; for grammars whose stack
stays shallow it is effectively linear. The companion `total_weight` runs the
same search but accumulates the `⊕`-sum of `⊗`-path-weights over **all** accepting
runs rather than stopping at the first.

### Weighted aggregation and FST approximation

- `total_weight(w)` returns `` `⊕ { w(π) ∣ π accepts w }` `` (the language weight
  of `w`), or `None` if `w ∉ L(P)`.
- `approximate_fst(max_depth)` unrolls the stack up to `max_depth` symbols,
  producing an ordinary [`VectorWfst`](../architecture/wfst-traits.md) whose
  states are `(PDA state, stack contents)` pairs. This is a *regular
  approximation*: it captures exactly the runs whose stack never exceeds
  `max_depth`, after which standard finite-state algorithms apply.

---

## Examples

### Balanced parentheses

The `balanced_brackets` constructor recognizes `` `{ (ⁿ )ⁿ ∣ n ≥ 0 }` `` and its
nestings:

```rust
use lling_llang::pushdown::PdaBuilder;
use lling_llang::semiring::{Semiring, TropicalWeight};

let pda = PdaBuilder::balanced_brackets('(', ')', TropicalWeight::one());

assert!(pda.accepts("".chars()));
assert!(pda.accepts("()".chars()));
assert!(pda.accepts("(())".chars()));
assert!(pda.accepts("((()))".chars()));
assert!(!pda.accepts("(".chars()));
assert!(!pda.accepts("(()".chars()));
```

### `aⁿbⁿ` from primitives

The same language, built arc-by-arc to show the `push`/`pop`/`ε` structure
mirrored in the [diagram](#diagrams):

```rust
use lling_llang::pushdown::{VectorPda, StackSymbol, StackAction, WeightedPda};
use lling_llang::semiring::{Semiring, TropicalWeight};

let mut pda: VectorPda<char, TropicalWeight> = VectorPda::new();
let s0 = pda.add_state();                                 // read a's
let s1 = pda.add_state();                                 // read b's
let s2 = pda.add_final_state(TropicalWeight::one());      // accept
pda.set_start(s0);

let z0 = StackSymbol::BOTTOM;
let g = StackSymbol::new(1);

pda.add_transition_parts(s0, Some('a'), z0, StackAction::Push(vec![z0, g]), s0, TropicalWeight::one());
pda.add_transition_parts(s0, Some('a'), g,  StackAction::Push(vec![g, g]),  s0, TropicalWeight::one());
pda.add_epsilon_transition(s0, g, StackAction::Noop, s1, TropicalWeight::one());
pda.add_transition_parts(s1, Some('b'), g,  StackAction::Pop, s1, TropicalWeight::one());
pda.add_epsilon_transition(s1, z0, StackAction::Noop, s2, TropicalWeight::one());

assert!(pda.accepts("aabb".chars()));
assert!(pda.accepts("aaabbb".chars()));
assert!(!pda.accepts("aab".chars()));
assert!(!pda.accepts("abb".chars()));
```

### Accepting by empty stack

```rust
use lling_llang::pushdown::{VectorPda, PdaAcceptMode, StackSymbol, StackAction};
use lling_llang::semiring::{Semiring, TropicalWeight};

let mut pda: VectorPda<char, TropicalWeight> =
    VectorPda::with_accept_mode(PdaAcceptMode::EmptyStack);
let s0 = pda.add_state();
pda.set_start(s0);
// Reading 'a' pops Z₀, emptying the stack.
pda.add_transition_parts(s0, Some('a'), StackSymbol::BOTTOM, StackAction::Pop, s0, TropicalWeight::one());

assert!(pda.accepts("a".chars()));    // stack emptied
assert!(!pda.accepts("".chars()));    // stack still holds Z₀
```

---

## Diagrams

### PDA for `aⁿbⁿ` with stack actions

![A pushdown automaton recognizing a-to-the-n b-to-the-n: state q0 pushes a marker for each a, an epsilon move switches to q1 which pops a marker for each b, and an epsilon move on the bottom marker accepts in q2.](../diagrams/transducers/pda-stack.svg)

*Teal nodes = PDA states; each arc reads `input , stack_top → action`; the green
double-ring `q₂` is final; grey dashed arcs are `ε`-moves (phase switch and
accept). `Z₀` is the bottom marker, `γ` the per-`a` marker.*

<details><summary>Text view</summary>

```text
                 a , Z₀ → push[Z₀,γ]            b , γ → pop
                 a , γ  → push[γ,γ]                 ┌──┐
                    ┌──┐                            │  │
                    │  ▼                            ▼  │
   start ─▶ (q₀ read a) ─ ─ ε , γ → noop ─ ─▶ (q₁ read b) ─ ─ ε , Z₀ → noop ─ ─▶ ((q₂ accept))

   stack on "aabb":  Z₀ → Z₀γ → Z₀γγ → Z₀γγ → Z₀γ → Z₀   (then ε-accept)
```

</details>

### ID trace on `aabb`

![A state-style trace of the instantaneous descriptions of the a-n-b-n PDA on input aabb, from (q0, aabb, Z0) through pushes and pops to the accepting (q2, epsilon, Z0).](../diagrams/transducers/pda-trace.svg)

*Each node is an ID `(state, remaining input, stack)`; teal arcs are
input-consuming steps, amber arcs are `ε`-moves, and the green node is the
accepting ID (input `ε`, stack `Z₀`, state `q₂`).*

<details><summary>Text view</summary>

```text
(q₀, aabb, Z₀)
   │  read a · push γ
(q₀, abb,  Z₀γ)
   │  read a · push γ
(q₀, bb,   Z₀γγ)
   ┊  ε · switch phase
(q₁, bb,   Z₀γγ)
   │  read b · pop γ
(q₁, b,    Z₀γ)
   │  read b · pop γ
(q₁, ε,    Z₀)
   ┊  ε · accept  (top Z₀, input ε)
(q₂, ε,    Z₀)        ← accepting
```

</details>

---

## Relation to the library

- **Beyond finite-state composition.** PDAs recognize the context-free languages,
  a strict superset of the regular languages handled by the
  [WFST](../architecture/wfst-traits.md) core; they complement the
  lattice-oriented [CFG/Earley parser](../algorithms/parsing.md) when an explicit
  stack machine is the more natural formulation.
- **`approximate_fst` bridges back to WFSTs.** Bounding the stack depth yields a
  `VectorWfst`, after which [composition](../algorithms/composition.md),
  [determinization](../algorithms/determinization.md), and
  [path extraction](../algorithms/path-extraction.md) all apply.
- **Weights are any semiring.** `total_weight` aggregates with `⊕`/`⊗` over the
  chosen [semiring](../architecture/semirings.md); the examples use
  `TropicalWeight`, but Log or Probability give expected counts / probabilities.
- **No feature flag.** Always compiled (`pub mod pushdown;` in
  [`src/lib.rs`](../../src/lib.rs)) and re-exported from the crate `prelude`.

See the [transducer-family overview](README.md) to place pushdown automata among
the multi-tape, tree, subsequential, and neural transducer families.

---

## References

- <a id="cite-mohri2009"></a>[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) —
  Mohri, M. (2009). *Weighted Automata Algorithms.* In *Handbook of Weighted
  Automata*, pp. 213–254. Springer. Weighted pushdown systems and the
  equivalence of final-state and empty-stack acceptance.
- <a id="cite-mohri1997"></a>[Mohri 1997](../BIBLIOGRAPHY.md#ref-mohri1997) —
  Mohri, M. (1997). *Finite-State Transducers in Language and Speech Processing.*
  Computational Linguistics 23(2):269–311. Background on weighted automata and the
  semiring framework the weights inhabit.
- <a id="cite-allauzen2007"></a>[Allauzen 2007](../BIBLIOGRAPHY.md#ref-allauzen2007) —
  Allauzen, C., Riley, M., Schalkwyk, J., Skut, W., & Mohri, M. (2007).
  *OpenFst: A General and Efficient Weighted Finite-State Transducer Library.*
  CIAA 2007. The finite-state library this module's `approximate_fst` output
  targets.
