# Lookahead Pruning

**Thesis.** A *lookahead table* `` `L` `` precomputes, for every state `` `q` ``,
an estimate of the cost still to come — `` `L[q] = ⊕` `` over the arcs reachable
from `` `q` `` to a final state — so that beam search can compare hypotheses at
different stages of completion on equal footing and prune the hopeless ones
earlier.

During beam search a hypothesis that has consumed three words has necessarily
accumulated more cost than one that has consumed one, even if the longer
hypothesis is globally better. Adding `` `L[q] `` — the *backward potential* —
turns the raw accumulated cost into an estimate of the **whole-path** cost, which
*is* comparable across hypotheses. Source:
[`src/optimization/lookahead.rs`](../../src/optimization/lookahead.rs).

---

## Terms & symbols

| Term | Meaning |
|---|---|
| **WFST / WFSA** | Weighted Finite-State Transducer / Acceptor. ([NOTATION](../NOTATION.md)) |
| `` `L[q]` `` | Lookahead score for state `` `q` `` — its backward potential `` `V(q)` ``. |
| `` `V(q)` `` | Backward potential: `` `V(q) = ⊕` `` over all paths from `` `q` `` to a final state. |
| `` `⊕` `` | Semiring *plus*. In the **log** semiring `` `⊕ = ⊕ₗₒg` ``; combines alternative futures. |
| `` `⊗` `` | Semiring *times* (`` `+` `` in log space); combines a prefix with its future estimate. |
| `` `0̄` `` | Additive identity (`` `⊕` ``-identity) — “no path”/unreachable; log `` `0̄ = ∞` ``. |
| `` `g(q)` `` | Accumulated weight of the prefix reaching `` `q` `` (the beam's running score). |
| **frontier** | The active hypotheses kept by the beam at one step. |
| `` `β` `` | Beam width — keep hypotheses within `` `best + β` `` of the best score. |
| `` `∣Q∣` ``, `` `∣E∣` `` | State / edge counts (cardinality, U+2223, not ASCII `|`). |

---

## Formal model

The lookahead score is the **backward shortest-distance** in the log semiring.
For an acyclic WFST with final-weight function `` `ρ` ``,

```text
V(q) = ⊕ over paths π : q ⇝ F   of   w(π) ⊗ ρ(end(π))
     = ⊕ₐ∈arcs(q)   w(a) ⊗ V(target(a))          (recurrence)
     = ρ(q)                                       if q ∈ F (base case)
```

so `` `L[q] = V(q) `` is exactly the *total probability mass* of all
continuations from `` `q` `` to acceptance, expressed as a negative log weight.
A hypothesis sitting at `` `q` `` with prefix cost `` `g(q)` `` gets the
**normalized score**

```text
score(q) = g(q) ⊗ L[q]            (log space: g(q) + V(q))
```

which estimates the full-path cost; beam pruning then keeps `` `q` `` iff
`` `score(q) ≤ best + β` ``. This is the admissible "A\*-style" completion
estimate: in the log semiring `` `V(q)` `` sums *all* futures (not just the best
one), the same quantity log-semiring weight pushing computes
([Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002)), so the module reuses those
potentials directly.

| Component | Type | Role |
|---|---|---|
| `` `V` `` | `` `Vec<LogWeight>` `` (`potentials`) | Backward potential per state. |
| `` `L[q]` `` | `LogWeight` (`get(q)`) | The lookahead score; `` `0̄` `` for out-of-range `` `q` ``. |
| total | `LogWeight` (`total_weight`) | `` `V(start)` `` — total mass through the WFST. |

---

## Intuition — a chain and a fork

For the chain `` `q0 --a/1.0--> q1 --b/2.0--> q2(final)` `` the futures are
unique, so the potentials are just suffix sums:

```text
V(q2) = 0   (final, log 1 = 0)
V(q1) = 2.0 (only continuation: b)
V(q0) = 3.0 (a then b: 1.0 + 2.0)
```

For a fork `` `q0` `` with two arcs to the same final (`` `a/1.0` `` and
`` `b/2.0` ``), the futures combine with `` `⊕ₗₒg` ``:

```text
V(q0) = −log(e^{−1.0} + e^{−2.0}) ≈ 0.687
```

Both are tested directly: `test_build_lookahead_chain` checks
`` `L[0]=3.0, L[1]=2.0, L[2]=0` ``, and `test_lookahead_parallel` checks the
fork's `` `≈ 0.687` ``.

---

## Architecture & API

### `LookaheadTable`

`LookaheadTable` is the materialized `` `L` ``. It exposes the lookahead per
state and the global mass:

```rust
use lling_llang::optimization::{build_lookahead_table, LookaheadConfig};

let table = build_lookahead_table(&fst, LookaheadConfig::default())
    .expect("WFST reaches a final state");

let future = table.get(current_state);                 // L[q] as a LogWeight
let estimate = table.normalize_score(current_state, &g);   // g ⊗ L[q]
```

| Method | Returns | Meaning |
|---|---|---|
| `get(q)` | `LogWeight` | `` `L[q] = V(q)` ``; `` `0̄` `` (`LogWeight::zero()`) if `` `q` `` is out of range. |
| `get_value(q)` | `f64` | The raw potential, or `` `∞` `` if unreachable. |
| `is_reachable(q)` | `bool` | Whether `` `q` `` has any path to a final state. |
| `normalize_score(q, g)` | `LogWeight` | `` `g ⊗ L[q]` `` — the completion estimate. |
| `total_weight()` | `&LogWeight` | `` `V(start)` `` — total mass through the WFST. |
| `num_reachable()` / `num_states()` | `usize` | Reachable-to-final count / table size. |

### `build_lookahead_table` and `LookaheadConfig`

`build_lookahead_table(fst, config)` computes all potentials in one backward
pass via `compute_log_potentials` (shared with log-semiring weight pushing). It
is total: an empty WFST yields an empty table; a WFST with no start state errors
with `LogPushError::NoStartState`. `LookaheadConfig` has two knobs —

| Field | Default | Effect |
|---|---|---|
| `cache` | `true` | Keep the table for reuse across the search. |
| `allow_unreachable` | `true` | On a potential-computation failure, return an all-`` `0̄` `` table instead of erroring. |

For one-off queries, `compute_lookahead_single(fst, q)` returns `` `V(q)` ``
without storing the table (though it still computes all potentials, so the table
is preferable for repeated lookups).

---

## Algorithms

### ⟨ build lookahead table ⟩

The intent is to *materialize `` `V(q)` `` for every state so the frontier can
read it in `` `O(1)` ``*. The invariant is that, processing states in reverse
topological order, `` `V(q)` `` is final before any predecessor of `` `q` `` is
visited — the standard backward shortest-distance order.

```text
⟨ build lookahead table ⟩ ≡
  if num_states = 0:  return empty table
  if start = NO_STATE: error NoStartState
  V ← compute_log_potentials(fst)            ⟨ backward log shortest-distance ⟩
        V(q) = ⊕ₐ∈arcs(q) w(a) ⊗ V(target(a)),  base V(f) = ρ(f) for f ∈ F
  total ← V(start)                            ⟨ total mass ⟩
  num_reachable ← count { q : V(q) ≠ 0̄ }
  return LookaheadTable { potentials = V, total, num_reachable }
```

For an acyclic WFST this is `` `O(∣Q∣ + ∣E∣)` `` — one visit per state and per
arc. Cyclic WFSTs require the fixed-point shortest-distance solver inside
`compute_log_potentials`; `allow_unreachable` shields callers from the failure
case by returning an all-`` `0̄` `` table.

### ⟨ push lookahead to the frontier ⟩

At search time each hypothesis combines its prefix cost with the table:

```text
⟨ push lookahead to the frontier ⟩ ≡
  for hyp in frontier:
      score(hyp) ← g(hyp.state) ⊗ L[hyp.state]     // normalize_score
  best ← ⊕ over score(hyp)                          // tropical min for the cutoff
  keep hyp  iff  score(hyp) ≤ best + β              // prune
```

Without lookahead, `` `best` `` is dominated by the *shortest* prefixes and long
hypotheses are unfairly pruned; with `` `L` ``, every score is a whole-path
estimate, so the cutoff is meaningful across stages.

![Lookahead pruning: a small WFSA's backward potentials V(q) populate a lookahead table L, which the beam frontier combines with each hypothesis's accumulated cost via g ⊗ L[q] to decide what survives the beam threshold.](../diagrams/optimization/lookahead.svg)

*Blue = WFSA states annotated with `` `V(q)` ``; green/bold = the best path and the kept frontier; grey = alternative arcs; amber = the materialized `` `L` `` table; dotted = the data flow from potentials to the frontier.*

<details><summary>Text view</summary>

```text
WFSA (arc weights = −log p):
  [start] → q0(V=3.0) ──a/1.0──▶ q1(V=2.0) ──b/2.0──▶ q3(V=0) final
            q0        ──c/1.0──▶ q2(V=2.0) ──d/2.0──▶ q3

LookaheadTable L:        pruning frontier:
  q  | L[q]=V(q)           hyp @ q1 : g=1.0 ⊗ L=2.0 = 3.0  ✓ keep
  q0 | 3.0                 hyp @ q2 : g=1.0 ⊗ L=2.0 = 3.0  ✓ keep
  q1 | 2.0                 best + beam threshold = 3.0 + β
  q2 | 2.0
  q3 | 0.0
  (compute_log_potentials → L → normalize_score: g ⊗ L[q])
```

</details>

---

## Examples

From `#[cfg(test)]` in
[`src/optimization/lookahead.rs`](../../src/optimization/lookahead.rs).

### Build a table and read potentials

```rust
use lling_llang::optimization::{build_lookahead_table, LookaheadConfig};
use lling_llang::semiring::{LogWeight, Semiring};
use lling_llang::wfst::{MutableWfst, VectorWfst};

let mut fst: VectorWfst<char, LogWeight> = VectorWfst::new();
let (s0, s1, s2) = (fst.add_state(), fst.add_state(), fst.add_state());
fst.set_start(s0);
fst.set_final(s2, LogWeight::one());
fst.add_arc(s0, Some('a'), Some('a'), s1, LogWeight::new(1.0));
fst.add_arc(s1, Some('b'), Some('b'), s2, LogWeight::new(2.0));

let table = build_lookahead_table(&fst, LookaheadConfig::default())
    .expect("Should build table");

assert!(table.get(2).approx_eq(&LogWeight::one(), 0.001));     // V(q2) = 0
assert!(table.get(1).approx_eq(&LogWeight::new(2.0), 0.001));  // V(q1) = 2.0
assert!(table.get(0).approx_eq(&LogWeight::new(3.0), 0.001));  // V(q0) = 3.0
```

### Normalize a hypothesis score

Continuing with the `table` and `LogWeight` from the snippet above
(`test_lookahead_normalize_score`):

```rust
// Accumulated cost 1.0 to reach state 1, lookahead 2.0 ⇒ estimate 3.0
let accumulated = LogWeight::new(1.0);
let normalized = table.normalize_score(1, &accumulated);
assert!(normalized.approx_eq(&LogWeight::new(3.0), 0.001));
```

---

## Relation to the library

- **Log-semiring weight pushing.** The potentials come from
  `compute_log_potentials`, the same backward pass that
  [`advanced/beam-optimization.md`](../advanced/beam-optimization.md) uses for
  stochastic pushing; lookahead reads the potentials instead of reweighting arcs.
- **Shortest distance.** `` `V(q)` `` is a backward shortest-distance
  ([`algorithms/shortest-distance.md`](../algorithms/shortest-distance.md)) in the
  log semiring.
- **Beam search & SIMD pruning.** `normalize_score` feeds the cutoff that
  `BatchForwardScores::prune` ([`advanced/simd.md`](../advanced/simd.md))
  applies on the lane-vectorized frontier.
- **Constrained decoding.** The per-state `valid_token_cache` in
  [`advanced/constrained-decoding.md`](../advanced/constrained-decoding.md) is the
  same "precompute per state, read in `` `O(1)` `` at search time" pattern.
- See the optimization research log in [`journal.md`](journal.md).

---

## References

- [Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002) — *Weighted Finite-State
  Transducers in Speech Recognition.* Backward potentials and their use in
  beam-pruned Viterbi decoding.
- [Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) — *Weighted Automata Algorithms.*
  The shortest-distance framework that defines `` `V(q) = ⊕` `` over reachable
  paths.
