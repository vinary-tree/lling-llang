# Proof Status

This document tracks the current formal verification surface for lling-llang.

## Overview

| Category | Total | Checked | Support Lemmas | Unchecked Proof Escapes |
|----------|-------|---------|----------------|-------------------------|
| Semiring Foundations | 8 | 8 | 0 | 0 |
| WFST Definitions | 4 | 4 | 0 | 0 |
| Algorithm Models | 4 | 4 partial-correctness/spec files | 0 | 0 |
| TLA+ Specifications | 3 specs / 9 configs + 3 expected-failure mutants | 9 finite TLC configs | 3 expected failures | 0 |

## Detailed Status

### Phase 1: Semiring Foundations (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Semiring.v` | Checked | Semiring laws as typeclass obligations, with derived lemmas and no unchecked escapes |
| `TropicalWeight.v` | Checked | Constructive proofs for tropical semiring, order, idempotence, commutative multiplication, and star |
| `LogWeight.v` | Checked | Exact real-valued probability-mass semiring corresponding to log-weight algebra |
| `Quantization.v` | Checked | Exact real-valued quantization grid with explicit max-raw-index convention, dequantization range, monotonicity, epsilon-approximation, bucket-coverage half-step error, and per-bucket error lemmas |
| `Interval.v` | Checked | Exact real-valued interval containment, width, midpoint, add/neg/subtract, and widening soundness lemmas |
| `Roundoff.v` | Checked | Abstract floating roundoff error contracts and interval-sound rounded addition/subtraction lemmas |
| `MatrixClosure.v` | Checked | Generic finite semiring matrices, finite bounded sums, matrix addition/multiplication, partial matrix star, explicit bounded finite walk-sum expansion, partial-star-to-walk-sum equivalence, and stabilization-to-star-solution lemmas |
| `SemiringProperties.v` | Checked | Power, partial-star, homomorphism, and natural-order lemmas |

Rust `TropicalWeight::new` and `LogWeight::new` now enforce the same
finite-real-or-`+∞` raw-value boundary used by these models; `NaN` and `-∞` are
rejected before values enter ordinary semiring operations. `QuantizationParams`
also rejects non-finite bounds and ranges so runtime quantization grids match
the finite real grid modeled in `Quantization.v`.

### Phase 2: WFST Definitions (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Definitions.v` | Checked | WFST, state, transition, well-formedness, determinism, acceptor predicates; `NO_STATE` matches Rust's `u32::MAX` sentinel in the nat model; empty WFST well-formedness is checked |
| `Paths.v` | Checked | Accepting paths must be connected, start/end correctly, and use transitions present in the WFST's outgoing lists; includes reusable connected-from-start and end-state lemmas; WFST membership implies transition well-formedness under `wfst_well_formed` |
| `MatrixSemantics.v` | Checked | WFST adjacency matrix construction for filtered transitions, partial matrix-closure weights, empty-WFST matrix closure, product-state matrix construction for fixed input/output strings, product index encode/decode and finite-carrier bound lemmas, product-matrix step/walk predicates, accepting-path-to-product-walk theorem over explicit consumption, product-matrix closure-to-walk-sum equivalence including final closed-path weights, occurrence-indexed outgoing-transition expansion preserving list-entry multiplicity, finite occurrence-path enumeration with final target annotations, accepting-path occurrence lifting, occurrence-enumerator exactness (membership soundness and completeness against the bounded product-occurrence walk relation under well-formedness), duplicate-freedom (`NoDup`) of the occurrence-path and final-annotated closed-path enumerators, closed-path enumerator exactness, and stabilization-to-star-solution handoff |
| `Language.v` | Checked | Well-formed finite weighted-language relation sums final-weighted accepting paths over exact duplicate-free path enumerations; includes exact-enumeration soundness/completeness, aggregate-weight lemmas, bounded language approximations, stable closed-language witnesses, matrix-backed epsilon-closure and product-matrix language weights, public `path_matches` to product-consuming-walk bridge, final-weight endpoint bridge, matching accepting-path occurrence lifting, product-matrix language to product-walk-sum, occurrence-indexed transition-expansion, and finite occurrence-enumerator witness theorems, a label-consumption converse recovering `path_matches` from full-string consumption, a Prop-level characterization of position-accepting final closed occurrence paths as genuine label-transducing accepting paths, weight-axis de-self-referencing grounding the product-matrix language weight in a multiplicity-preserving sum of independent `accepting_path_weight`s over transducing closed paths, and non-vacuous language equivalence requiring a finite, stable-closure, or matrix-closure witness for each input/output pair; path simulation is separate |

### Phase 3: Algorithm Models (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Viterbi.v` | Checked partial correctness | Final-weight-aware finite candidate-list facts, `viterbi_candidate_optimal` spec predicate, optimal-value theorem, and Bellman-update facts |
| `ShortestDistance.v` | Checked partial correctness | Initialization, relaxation, well-formed empty-WFST solution theorem, and `shortest_distance_solution` fixed-point spec predicate |
| `Determinize.v` | Checked partial correctness | Weighted-subset operations aggregate duplicate target states before normalization, explicit normalization pass with soundness theorem, nonempty-step fact, quotient soundness under nonzero-divisor precondition, non-vacuous `determinize_correct` spec predicate, already-deterministic identity correctness, and functional/sequential precondition facts |
| `Minimize.v` | Checked partial correctness | Residual right-language state equivalence, partition helpers, non-vacuous `minimize_correct` and `push_weights_spec` predicates requiring defined source/target language surfaces, identity-minimize correctness, and language-preservation sanity lemmas |

### Phase 4: TLA+ Specifications

| File | Status | Invariants | Notes |
|------|--------|------------|-------|
| `RRWM.tla` | Finite TLC model | `TypeOK`, `RegretWithinAccountingHorizon`, `WeightsPositive`, `LossesBounded`, `TotalLossBounded`, `WeightsExact`, `RoundAccounting` | Bounded integer accounting model with nondeterministic expert choice; includes zero/single/multiple expert configs and an expected-failure stale-weight mutant; not the asymptotic regret theorem |
| `LazyComposition.tla` | Finite TLC model | `MemoryBounded`, `CacheValid`, `WorklistValid`, `NoDuplicateProcessing`, `ProcessedValid`, `NoCacheEmpty`, `AccessOrderValid`, `CacheCoveredByAccessOrder` | Synthetic bounded multi-label/epsilon composition model with `CacheAll`, LRU eviction, and `NoCache` configs plus an expected-failure no-cache mutant |
| `CascadeOrder.tla` | Finite TLC model | `AlphabetsCompatible`, `OrderingConstraints`, `NoRepetition`, `ValidCascade`, `PrefixValid` | Nondeterministic explicit-order component append model starting at AM; includes ordinary, fair, overlapping-alphabet configs and an expected-failure order mutant |

## Last Updated

2026-05-26

## Notes

- Rocq files are required to build without unchecked proof escapes.
- TLA+ specs include TLC config files under `proofs/tla/MC`.
- Algorithm files contain checked specification predicates and partial-correctness theorems over the current finite, stable-closed, matrix-backed epsilon-closure, or product-matrix WFST language surface.
- `make verify-proofs` runs the Rocq checks, all TLC configs, and expected-failure TLC mutants with metadata under `/tmp`.
