# Industry-Standard & State-of-the-Art Solution Review

**Scope.** This document records the epic *"Industry-standard and state-of-the-art
solution review"* of the `lling-llang` WFST library: how its algorithms compare to
standard weighted-automata practice (§1), which state-of-the-art techniques are
already present or were considered (§2), and a disciplined split of the remaining
ideas into *immediately-actionable* (already landed) versus *research-grade* work
that requires benchmarking infrastructure or larger design approval (§3). It is the
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
| **Partition-refinement minimization** | ◐ Present, Moore not Hopcroft | `minimize` uses **Moore** iterative signature refinement (`O(|Q|·|E|)` worst case), not Hopcroft's `O(|E| log|Q|)`. Correct and, for the offline/one-shot sizes this library minimizes, adequate. Docs corrected to state this accurately; true Hopcroft is future work (§3, R7). |

**Conclusion.** The library conforms to standard weighted-automata practice on every
structural axis. The one deviation (Moore vs Hopcroft minimization) is a deliberate,
now-documented complexity/robustness trade-off, not an oversight.

## §2 — State-of-the-art techniques considered

| SOTA technique | Status | Notes |
|---|---|---|
| Indexed composition filters | ◐ Partial | Composition indexes FST2 input transitions per product state; a *cross-product-state* index cache is research-grade (§3, R2). |
| Cache-policy pluggability | ✅ Present | `CacheAll` / `NoCache` / `Lru` policies selectable; verified memory-bounded in TLA⁺. |
| Symbolic automata / BDD-style label sets | ✅ Present | `src/symbolic/` implements Symbolic Finite Automata/Transducers over effective Boolean algebras (`sfa.rs`, `string_algebra.rs`, Presburger/KAT predicates), with Rocq proofs (`EffectiveBooleanAlgebra.v`, `GuardTierCertificate.v`). |
| e-graph rewrite for algebraic normalization | ✗ Not present | Research-grade; would need an e-graph dependency and a cost model. Out of scope for a repo-local pass. |
| Property-based testing | ✅ Present | `proptest` strategies throughout (`src/test_utils/arbitrary.rs`); e.g. `minimize_idempotent`, `minimize_preserves_determinism`, `push_no_start_fails`. |
| Formal model checking | ✅ Present | TLA⁺ specs for RRWM, LazyComposition, CascadeOrder — including **negative/mutant** checks that must fail — plus Rocq proofs for WFST semantics and symbolic algebra. |
| Mutation testing | ◐ Partial | The TLA⁺ suite includes hand-written mutants; systematic Rust mutation testing (`cargo-mutants`) is research-grade infra. |
| Differential testing vs a reference (OpenFst) | ✗ Not present | Research-grade; needs an OpenFst harness/oracle. High value but a separate project. |

## §3 — Actionable vs research-grade (todo 1940)

### Immediately-actionable — landed this pass (each benchmarked or complexity-argued, validated through the full gate, committed)

| ID | Change | Evidence | Commit |
|---|---|---|---|
| A1 | Correct minimize docs (Moore, not Hopcroft) | Mandated doc-accuracy fix | `e0ad2b4` |
| A2 | minimize: precompute canonical arc order once + reuse pass buffers | criterion −26…−43% (p<0.05) | `e0ad2b4` |
| A3 | `reverse_shortest_distance`: preallocate reverse-adjacency by in-degree | complexity: removes doubling reallocs on the backward push path | `da14bdd` |
| A4 | `materialize`: move labels instead of a second clone | halves per-decode label-clone traffic (owned `SmallVec`) | `da14bdd` |
| A5 | Add a composition benchmark (infra) | unblocks R2/R3 evidence; baseline below | `073096d` |

**Composition baseline (criterion, taskset-pinned).** The new `composition` group
confirms the compose→materialize pipeline is fast and scales linearly, so R2/R3
are genuinely *not* warranted at current scales:

| Case | time |
|---|---|
| `composition/chain/10` | 1.84 µs |
| `composition/chain/50` | 7.54 µs |
| `composition/chain/100` | 14.65 µs (≈0.15 µs/product state, linear) |

A per-decode CTC composition (obs ∘ ctc ∘ lm) at realistic sizes therefore costs
tens of microseconds; the R2 index-cache and R3 O(1)-LRU wins would only surface
at much larger products, which is exactly why they remain gated on larger stress
benchmarks rather than implemented now.

### Research-grade — deferred with explicit gates (NOT speculative changes)

Per the data-driven mandate, these are real opportunities that must **not** be
implemented without the stated evidence/design, because doing so blindly risks
regressions or unjustified complexity:

- **R1 — determinize subset key without the throwaway `Vec`.** Direction is
  genuinely ambiguous (`BTreeMap` hashing vs contiguous `Vec` hashing); decide with
  a merge-heavy determinize benchmark.
- **R2 — composition per-`s2` transition-index cache.** Rebuilt for every product
  state sharing an `s2`; needs a `RefCell` side-table design and the composition
  benchmark (A5) to confirm the win.
- **R3 — O(1) LRU for composition.** Current LRU `touch`/`reconcile` are O(cache);
  **latent** (default policy is `CacheAll`; the CTC path never selects LRU), so it
  only affects opt-in users. Gate on A5 + an LRU-specific benchmark.
- **R4 — Earley completer reverse index (Leo's optimization).** The completer scans
  all items at a position; a `waiting[(pos,nt)]` index removes the quadratic. Gate on
  an ambiguous/long-sentence benchmark; non-trivial `EarleyChart` rewrite.
- **R5 — tree-transducer subtree sharing via `Rc`.** `cartesian_product` deep-clones
  subtrees; `Rc<Tree>` sharing avoids it. Gate on a branching-rule tree-transducer
  benchmark (none exists yet).
- **R6 — flat CSR for path adjacency.** `edge_adjacency`/reverse-adj are jagged
  `Vec<Vec<…>>`; a flat CSR improves locality. Locality-only; gate on a large-lattice
  benchmark to show the win exceeds noise.
- **R7 — true Hopcroft minimization.** `O(|E| log|Q|)` vs Moore's `O(|Q|·|E|)`, but
  weighted `(label, output, quantized-weight)` splitter bookkeeping is error-prone
  against the property-test-proven Moore code, and minimize is offline/one-shot.
  Gate on a ≥10⁴–10⁵-state minimize benchmark demonstrating a measured bottleneck;
  ship behind a flag cross-checked against Moore.

### Already-adequate (evaluated, no change warranted — evidence on file)

Shortest-distance/queue disciplines (dense `Vec` + adaptive `Dense/Sparse`
`StatePositionIndex`); determinization bounds & canonical keys; path extraction
(topological DP + admissible-heuristic A* n-best, bounded cycles); Earley chart
sparsity & one-shot nullable cache; pervasive `SmallVec` for small fixed fan-out;
sparse-ID→compact-position mapping; and exhaustive `usize`→compact-ID overflow
checks on the GPU/CSR paths.
