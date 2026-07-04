# Industry-Standard & State-of-the-Art Solution Review

**Scope.** This document records the epic *"Industry-standard and state-of-the-art
solution review"* of the `lling-llang` WFST library: how its algorithms compare to
standard weighted-automata practice (§1), which state-of-the-art techniques are
already present or were considered (§2), and how every identified follow-up was
resolved to a final outcome — *implemented* or *benchmarked and deliberately
retained* (§3). It is the
evidence-backed conclusion of the optimization pass; per the project's
data-driven mandate, no optimization is claimed without a benchmark or a concrete
complexity argument.

## §1 — Comparison against standard WFST practice

| Standard practice (Mohri; OpenFst) | Status in `lling-llang` | Evidence |
|---|---|---|
| **Semiring-generic algorithms** — one implementation parameterized over the weight semiring | ✅ Followed | Every algorithm is generic over `W: Semiring`; `Semiring: Clone + Copy` (`src/semiring/traits.rs:50`) so weight moves are register copies, and specializations (Tropical, Log, Probability, Expectation, String, Product, Lexicographic, Signed-Tropical, Power) share the same generic code paths. |
| **ε-removal before determinization** | ✅ Followed | `determinize` runs an ε-removal prepass (`remove_epsilon`, then recurses with `remove_epsilon_first=false`) and rejects residual input-ε with `NotDeterminizable` (`src/algorithms/determinize.rs`). |
| **Weight pushing before minimization** | ✅ Followed | `minimize` pushes weights (`push_weights`, `src/algorithms/minimize.rs:243`) to canonicalize before partition refinement. |
| **Deterministic canonical subset keys** (determinization) | ✅ Followed | Weighted subsets keyed by `BTreeMap` → deterministic ordering; bounded by `max_states` (default 1,000,000). |
| **Lazy / on-the-fly composition** | ✅ Followed | `LazyComposition` computes product states on demand with pluggable cache policies (`CacheAll` / `NoCache` / `Lru`); `materialize` realizes the reachable part (`src/composition/`). A TLA⁺ model (`LazyComposition.tla`) proves the cache stays memory-bounded. |
| **CSR representation for accelerator/GPU paths** | ✅ Followed | `src/gpu/csr.rs` provides a checked CSR builder with explicit `u32` overflow detection for the GPU boundary. |
| **Partition-refinement minimization** | ✅ Followed | `minimize` uses a **worklist-driven (Hopcroft-family) partition refinement** (commit `3799e03`): a block is re-examined only when a successor's block changes. It replaced the earlier Moore full-pass refinement (retained as a `#[cfg(test)]` differential oracle) after a benchmark exposed Moore's `O(|Q|²)` chain behaviour — the worklist is **82–87 % faster** at scale with byte-identical output. |

**Conclusion.** The library conforms to standard weighted-automata practice on
every structural axis, minimization included (a worklist-driven, Hopcroft-family
partition refinement).

## §2 — State-of-the-art techniques considered

| SOTA technique | Status | Notes |
|---|---|---|
| Indexed composition filters | ✅ Present | Composition indexes FST2 input transitions per product state; a *cross-product-state* index cache was evaluated and retained — composition is benchmarked fast + linear (§3, R2). |
| Cache-policy pluggability | ✅ Present | `CacheAll` / `NoCache` / `Lru` policies selectable; verified memory-bounded in TLA⁺. |
| Symbolic automata / BDD-style label sets | ✅ Present | `src/symbolic/` implements Symbolic Finite Automata/Transducers over effective Boolean algebras (`sfa.rs`, `string_algebra.rs`, Presburger/KAT predicates), with Rocq proofs (`EffectiveBooleanAlgebra.v`, `GuardTierCertificate.v`). |
| e-graph rewrite for algebraic normalization | ✗ Not present | A distinct rewrite-engine subsystem (external e-graph dependency + cost model), not an optimization of existing code — outside this pass's scope. Algebraic normalization here is carried by the semiring laws and the `symbolic` module. |
| Property-based testing | ✅ Present | `proptest` strategies throughout (`src/test_utils/arbitrary.rs`); e.g. `minimize_idempotent`, `minimize_preserves_determinism`, `push_no_start_fails`. |
| Formal model checking | ✅ Present | TLA⁺ specs for RRWM, LazyComposition, CascadeOrder — including **negative/mutant** checks that must fail — plus Rocq proofs for WFST semantics and symbolic algebra. |
| Mutation testing | ◐ Present (formal) | The TLA⁺ suite runs hand-written *mutants* that must fail (e.g. the `RRWM` weight mutant, the `LazyComposition` NoCache mutant), covering the formally-modeled invariants; whole-crate `cargo-mutants` is external tooling rather than a code change. |
| Differential testing vs a reference (OpenFst) | ✗ Not present | Requires the external OpenFst library as an oracle — an external-dependency harness outside a self-contained codebase pass. (Internally, `minimize`'s worklist refinement *is* differential-tested against the Moore oracle — §1.) |

## §3 — Every follow-up resolved: implemented or retained (todo 1940)

### Immediately-actionable — landed this pass (each benchmarked or complexity-argued, validated through the full gate, committed)

| ID | Change | Evidence | Commit |
|---|---|---|---|
| A1 | Correct minimize docs (Moore, not Hopcroft) | Mandated doc-accuracy fix | `e0ad2b4` |
| A2 | minimize: precompute canonical arc order once + reuse pass buffers | criterion −26…−43% (p<0.05) | `e0ad2b4` |
| A3 | `reverse_shortest_distance`: preallocate reverse-adjacency by in-degree | complexity: removes doubling reallocs on the backward push path | `da14bdd` |
| A4 | `materialize`: move labels instead of a second clone | halves per-decode label-clone traffic (owned `SmallVec`) | `da14bdd` |
| A5 | Add a composition benchmark (infra) | unblocks R2/R3 evidence; baseline below | `073096d` |
| A6 | minimize: worklist (Hopcroft-family) partition refinement (R7) | criterion −82…−87 % at scale (546 ms → 72 ms @ ≈4 000 states); differential-tested vs Moore | `3799e03` |
| A7 | composition `reconcile_lru_order` O(cache²) → O(cache) (R3) | complexity; opt-in `Lru` path only | `4a35352` |
| A8 | tree `cartesian_product` output preallocation (R5) | complexity; reserves the exact product size | `4a35352` |

**Composition baseline (criterion, taskset-pinned).** The new `composition` group
confirms the compose→materialize pipeline is fast and scales linearly, so the
composition-caching follow-ups (R2, R3) were resolved by retaining / simplifying
rather than adding speculative machinery:

| Case | time |
|---|---|
| `composition/chain/10` | 1.84 µs |
| `composition/chain/50` | 7.54 µs |
| `composition/chain/100` | 14.65 µs (≈0.15 µs/product state, linear) |

A per-decode CTC composition (obs ∘ ctc ∘ lm) at realistic sizes therefore costs
tens of microseconds. R3's `O(cache²)` LRU reconcile was still fixed to `O(cache)`
(commit `4a35352`); the R2 cross-`s2` index cache would only surface at far larger
products and is retained as-is — both resolved, neither deferred.

### Resolved — every follow-up decided (nothing deferred)

Each opportunity was taken to a final outcome: **implemented** (A6–A8 above) or
**benchmarked and deliberately retained**, because the current code is measurably
adequate and changing it would be speculative — which the data-driven mandate
forbids. Nothing is gated on future work.

- **R1 — determinize subset key.** RETAINED. Determinize benchmarks at 3–8 µs
  (10–25 states, branching 2–3); the throwaway `Vec` key is negligible, and the
  `BTreeMap`-key alternative has ambiguous benefit (ordered-traversal hashing can
  regress on miss-heavy inputs), so it is left unchanged.
- **R2 — composition per-`s2` index cache.** RETAINED. Composition is benchmarked
  fast and linear (chain/100 = 14.65 µs); the per-`s2` `input_transition_index`
  rebuild is inside that time. A `RefCell` side-table would pay off only at product
  sizes far beyond this library's decode use.
- **R4 — Earley completer reverse index.** RETAINED. Earley parses in 3–6.5 µs
  (3/5-word) and 4.6 µs (lattice-with-alternatives); the completer's
  items-per-position scan is not a bottleneck for the small grammars parsed here.
- **R6 — flat CSR for path adjacency.** RETAINED. Path extraction runs in 1.7–6.9 µs
  (n-best); `edge_adjacency` already preallocates each bucket by in-degree, so a
  flat CSR is a locality-only change that would touch many callers for no measured
  gain at these speeds.

(R3, R5 and R7 were the follow-ups worth implementing — see A6–A8.)

### Already-adequate (evaluated, no change warranted — evidence on file)

Shortest-distance/queue disciplines (dense `Vec` + adaptive `Dense/Sparse`
`StatePositionIndex`); determinization bounds & canonical keys; path extraction
(topological DP + admissible-heuristic A* n-best, bounded cycles); Earley chart
sparsity & one-shot nullable cache; pervasive `SmallVec` for small fixed fan-out;
sparse-ID→compact-position mapping; and exhaustive `usize`→compact-ID overflow
checks on the GPU/CSR paths.
