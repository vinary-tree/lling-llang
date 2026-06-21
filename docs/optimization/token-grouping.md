# Token Grouping

**Thesis.** During on-the-fly composition, many beam-search tokens share the same
*base-graph* state but differ only in their *grammar* state; grouping them into
**token equivalence classes** keyed by base state — and deferring their
materialization until a word boundary — cuts redundant composition work by
10–20× while preserving the lattice the search produces.

This is the LET-Decoder lazy-evaluation strategy
([Lv 2021](../BIBLIOGRAPHY.md#ref-lv2021)): a `TokenGroup` stores only the *best* forward
probability (its `` `⊕` ``-combination) for pruning, links groups across frames
with lightweight `GroupLink`s instead of real tokens, and **expands** a group
into concrete tokens only when a word arc forces it. Source:
[`src/optimization/token_group.rs`](../../src/optimization/token_group.rs).

---

## Terms & symbols

| Term | Meaning |
|---|---|
| **token** | A search hypothesis `` `(base_state, grammar_state, forward_prob, …)` ``. |
| **base state** | A state in the base graph (e.g. HCLG); the grouping key. |
| **grammar state** | A state in the residual grammar `` `Gᵣ` `` that distinguishes tokens. |
| **token group** | Equivalence class of tokens sharing one base state (`TokenGroup`). |
| `` `⊕` `` | Semiring *plus*. In the **log** semiring `` `⊕ = ⊕ₗₒg` `` (log-add); combines the group's tokens. |
| **forward prob** | Accumulated `` `−log` `` probability of reaching a token (`forward_prob`). |
| **lazy evaluation** | Defer materializing tokens until a word boundary needs them. |
| **frame** | One time step of the decoder; groups are keyed within a frame. |
| **α-stable** | Property that group forward probabilities stay valid after updates (correct lattices). |
| `` `∣V∣` `` | Vocabulary size (cardinality, U+2223, not ASCII `|`). |

HCLG = the composed **H**MM ∘ **C**ontext ∘ **L**exicon ∘ **G**rammar graph
([`asr/cascade-construction.md`](../asr/cascade-construction.md)).

---

## Formal model

On-the-fly composition explores the product `` `HCLG ∘ Gᵣ` ``, whose states are
pairs `` `(b, s)` `` of a base-graph state `` `b` `` and a grammar state
`` `s` ``. Many such pairs share `` `b` `` and differ only in `` `s` ``. A token
group is the **fiber over a base state**:

```text
group(b) = { token : token.base_state = b }       (within one frame)
group(b).best_forward_prob = ⊕  over  { token.forward_prob : token ∈ group(b) }
```

where `` `⊕ = ⊕ₗₒg` `` is log-add. Pruning compares groups by
`` `best_forward_prob` `` alone — a single number per base state — so a group can
be discarded *before* its constituent tokens are ever built. Materialization is
deferred: a group stays **lazy** until a token arrives via a **word arc** (an
arc with an output label), at which point `expand_group` realizes its tokens.
Between frames, a `GroupLink` records the `(source, target, weight, is_word_arc)`
relation so back-tracing can reconstruct paths without storing intermediate
tokens — this is what makes the saving real.

The α-stable property guarantees that updating `` `best_forward_prob` `` with new
incoming mass (via `add_token` or `add_preceding_link`, both folding with
`` `⊕ₗₒg` ``) keeps the group's score correct, so the lazily generated lattice
equals the one eager expansion would produce.

| Component | Type | Role |
|---|---|---|
| `Token` | `{ base_state, grammar_state, forward_prob, prev_token, prev_arc }` | One hypothesis. |
| `TokenGroup` | `{ base_state, best_forward_prob, expanded, tokens, links… }` | The equivalence class. |
| `GroupLink` | `{ source_group, target_group, weight, is_word_arc }` | Cross-frame back-trace edge. |

---

## Intuition — three tokens, one base state

Suppose three tokens enter a frame at base state 7 with grammar states `a, b, c`,
and one token enters at base state 9. Grouping collapses the first three into a
single class:

```text
(7,a) (7,b) (7,c)  ──group──▶  TokenGroup base=7,  best = ⊕(a, b, c)   (one entry)
(9,a)              ──group──▶  TokenGroup base=9,  best = a
```

Pruning now compares two groups, not four tokens; if group 9 falls outside the
beam it is dropped without materializing `(9,a)`. Only when a word arc reaches
group 7 are its tokens expanded. `test_token_group_pool` confirms two
`get_or_create(0)` calls in one frame return the *same* group id, while a
different base state gets a new one.

---

## Architecture & API

### `Token` and `TokenGroup`

A `TokenGroup` carries the grouping key (`base_state`), the pruning score
(`best_forward_prob`), an `expanded` flag, and — only once expanded — the actual
`tokens` plus `preceding_links`/`succeeding_links`. `add_token` folds a token's
`forward_prob` into `best_forward_prob` with `` `⊕ₗₒg` `` (the `plus` of the log
semiring) and stores the token; `add_preceding_link` does the same for an
incoming link's weight without storing a token.

```rust
use lling_llang::optimization::{TokenGroup, Token};
use lling_llang::semiring::{LogWeight, Semiring};

let mut group = TokenGroup::new(/*base_state=*/0, /*frame=*/0);
group.expanded = true;
group.add_token(Token { base_state: 0, grammar_state: 1,
    forward_prob: LogWeight::new(1.0), prev_token: None, prev_arc: None });
group.add_token(Token { base_state: 0, grammar_state: 2,
    forward_prob: LogWeight::new(1.0), prev_token: None, prev_arc: None });

// best_forward_prob = logadd(1.0, 1.0) = −log(2·e^{−1}) ≈ 0.307
let expected = -(2.0 * (-1.0_f64).exp()).ln();
assert!(group.best_forward_prob.approx_eq(&LogWeight::new(expected), 0.01));
```

### `TokenGroupPool` and per-frame keying

`TokenGroupPool` allocates groups and maps `` `base_state → group_id` `` **for
the current frame**. `get_or_create(base_state)` returns the existing group
within the frame or makes a new one; `advance_frame()` bumps the frame counter
and clears the map, so the same base state in the next frame yields a fresh
group (`test_token_group_pool_advance_frame`).

### `BucketQueue` — histogram pruning

Groups are pruned by quantized forward probability using a `BucketQueue<T>`: a
weight maps to an integer bucket, `pop` returns the best (lowest-weight)
bucket first, and `prune_beyond(max_bucket)` discards everything past a
threshold — `O(1)` amortized histogram pruning rather than a full sort.

| Method | Role |
|---|---|
| `insert(weight, item)` | Place `item` in the bucket for `weight`. |
| `pop()` / `peek()` | Best-first removal / inspection. |
| `prune_beyond(max_bucket)` | Drop all items past `max_bucket`; returns the count pruned. |
| `histogram()` | Per-bucket occupancy (for adaptive thresholds). |

### `TokenGroupManager` — the decoder driver

`TokenGroupManager` ties the pool and queue together with a `TokenGroupConfig`
(`max_tokens_per_group`, `max_groups`, `num_buckets`, `lazy_evaluation`). Its key
methods:

| Method | Role |
|---|---|
| `process_token(token, is_word_arc)` | Add to the base-state group; **expand** if `is_word_arc` (or eager mode), else stay lazy and just fold `forward_prob`. |
| `add_link(src, tgt, weight, is_word_arc)` | Record a `GroupLink` for lazy back-tracing (counts toward `ops_saved`). |
| `expand_group(id)` | Materialize a group's tokens (at a word boundary / during back-trace). |
| `advance_frame()` | Snapshot active groups into a `GroupedFrame`, then roll the pool/queue forward. |
| `prune(threshold)` | Histogram-prune groups beyond a quantized threshold. |
| `stats()` | `TokenGroupStats { tokens_processed, groups_created, expansions, ops_saved, … }`. |

The lazy/word-arc distinction is the crux: a non-word arc (`is_word_arc = false`)
in lazy mode only updates `best_forward_prob`, leaving `expanded == false`; a word
arc forces `expanded == true` and stores the token
(`test_token_group_manager_word_arc`).

---

## Algorithms

### ⟨ lazy token grouping per frame ⟩

The intent is to *keep one prunable score per base state and only pay for tokens
that survive to a word boundary*. The invariant is: **a group's
`best_forward_prob` equals the `` `⊕ₗₒg` `` of all token/link masses routed to its
base state in the current frame**, whether or not those tokens are materialized.

```text
⟨ lazy token grouping per frame ⟩ ≡
  for each incoming token (token, is_word_arc):
      g ← pool.get_or_create(token.base_state)       ⟨ key by base state ⟩
      if is_word_arc or g.expanded or eager:          ⟨ must materialize ⟩
          g.expanded ← true
          g.best_forward_prob ← g.best_forward_prob ⊕ token.forward_prob
          g.tokens.push(token)
      else:                                           ⟨ stay lazy ⟩
          g.best_forward_prob ← g.best_forward_prob ⊕ token.forward_prob
      queue.insert(g.best_forward_prob, g.id)
  prune(threshold):  queue.prune_beyond(bucket(threshold))   ⟨ histogram prune ⟩
  advance_frame():   snapshot active groups → GroupedFrame; roll pool & queue
```

Per token the work is `` `O(1)` `` (a hash lookup, a `` `⊕ₗₒg` ``, a bucket
insert); pruning is `` `O(buckets)` `` to sweep the tail. The saving comes from
**not** running composition for the grammar-state distinctions inside a group
until a word boundary, and from pruning whole groups by one number — the source
notes 10–20× fewer composition operations and the matching `ops_saved` counter.

![Token equivalence classes: four tokens sharing or differing in base state collapse into TokenGroups keyed by base state, each holding the log-added best forward probability; the surviving group is histogram-pruned and a word arc forces its expansion, while a dashed GroupLink carries lazy back-trace information.](../diagrams/optimization/token-grouping.svg)

*Blue = raw `(base, grammar)` tokens; green = the surviving group keyed by base 7 (its score is `` `⊕(a,b,c)` ``); grey = the pruned group (base 9); amber = the word-arc-triggered expansion; grey dashed = the lazy `GroupLink` back-trace edge.*

<details><summary>Text view</summary>

```text
tokens at frame t (base, grammar):
  (7,a) (7,b) (7,c)        (9,a)
     │     │     │            │
     └──── group ────┐        └── group ──┐
                     ▼                    ▼
   TokenGroup base=7  [SURVIVES]   TokenGroup base=9  [PRUNED]
   best_forward_prob = ⊕(a,b,c)    best_forward_prob = a
   lazy: tokens deferred           outside beam
        │ word arc → expand                ▲
        ▼                                  ┊ GroupLink (lazy back-trace, dashed)
   expand_group(base=7) ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┘
   materialize tokens at word boundary
```

</details>

---

## Examples

From `#[cfg(test)]` in
[`src/optimization/token_group.rs`](../../src/optimization/token_group.rs).

### Lazy vs. word-arc expansion

```rust
use lling_llang::optimization::{TokenGroupManager, TokenGroupConfig, Token};
use lling_llang::semiring::LogWeight;

let config = TokenGroupConfig { lazy_evaluation: true, ..Default::default() };
let mut manager = TokenGroupManager::new(config);

// Non-word arc ⇒ stays lazy (not expanded)
let id1 = manager.process_token(Token { base_state: 0, grammar_state: 1,
    forward_prob: LogWeight::new(1.0), prev_token: None, prev_arc: None }, false);
assert!(!manager.group(id1).expect("group").expanded);

// Word arc ⇒ forces expansion
let id2 = manager.process_token(Token { base_state: 1, grammar_state: 2,
    forward_prob: LogWeight::new(2.0), prev_token: None, prev_arc: None }, true);
assert!(manager.group(id2).expect("group").expanded);
```

### Histogram pruning with a `BucketQueue`

```rust
use lling_llang::optimization::BucketQueue;

let mut queue: BucketQueue<u32> = BucketQueue::new(10, 0.0, 10.0);
queue.insert(1.0, 1);
queue.insert(5.0, 2);
queue.insert(9.0, 3);

let pruned = queue.prune_beyond(5);     // drop items past bucket 5
assert_eq!(pruned, 1);                  // the weight-9.0 item
assert_eq!(queue.len(), 2);
```

---

## Relation to the library

- **On-the-fly composition.** Grouping optimizes the lazy product
  `` `HCLG ∘ Gᵣ` `` from [`algorithms/composition.md`](../algorithms/composition.md)
  and the lazy WFST machinery in
  [`architecture/wfst-operations.md`](../architecture/wfst-operations.md).
- **Log semiring.** `forward_prob` is a `LogWeight`; group folding uses
  `` `⊕ₗₒg` `` ([`architecture/semirings.md`](../architecture/semirings.md)).
- **Beam search & lookahead.** Histogram pruning complements the beam cutoff and
  the future-cost estimate of [`lookahead.md`](lookahead.md); both decide what to
  keep before doing full work.
- **SIMD frontier.** The per-group score collapse mirrors
  `BatchForwardScores::merge_duplicates_log`
  ([`advanced/simd.md`](../advanced/simd.md)).
- See the optimization research log in [`journal.md`](journal.md).

---

## References

- [Lv 2021](../BIBLIOGRAPHY.md#ref-lv2021) — *LET-Decoder: A WFST-Based
  Lazy-Evaluation Token-Group Decoder with Exact Lattice Generation* (IEEE SPL
  28:703–707). The token-group lazy-evaluation strategy and the 10–20× reduction
  in composition operations this module implements.
- [Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002) — *Weighted Finite-State
  Transducers in Speech Recognition.* The on-the-fly composition and beam-pruned
  decoding that token grouping accelerates.
