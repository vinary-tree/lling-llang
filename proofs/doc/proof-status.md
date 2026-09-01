# Proof Status

This document tracks the current formal verification surface for lling-llang.

## Overview

| Category | Total | Checked | Support Lemmas | Unchecked Proof Escapes |
|----------|-------|---------|----------------|-------------------------|
| Semiring Foundations | 9 | 9 | 0 | 0 |
| WFST Definitions | 4 | 4 | 0 | 0 |
| Algorithm Models | 4 | 4 partial-correctness/spec files | 0 | 0 |
| Campaign Rocq Contracts | 14 | 14 | 0 | 0 |
| TLA+ Specifications | 13 specs / 22 configs + 32 expected-failure mutants | 22 finite TLC configs | 32 expected failures | 0 |
| SMT Dual Checks | 7 transcripts / 101 queries | 101 expected results | 13 satisfiable witnesses/countermodels | 0 |
| Strong-Bisimulation Extraction | 5,124 exhaustive cases / 10 mutants / 13 required-red properties | 5,124 cases and 10 expected failures | 93-row invariant ledger | 0 |
| Kani ABI Models | 3 harnesses | 3 | 0 | 0 |

## Detailed Status

### Phase 1: Semiring Foundations (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Semiring.v` | Checked | Semiring laws as typeclass obligations, with derived lemmas and no unchecked escapes |
| `TropicalWeight.v` | Checked | Constructive proofs for tropical semiring, order, idempotence, commutative multiplication, and star |
| `ArcticWeight.v` | Checked | Exact-real max-plus semiring, preferred-score total order, idempotence, commutative multiplication, convergent-star boundary, positive-cycle rejection, fzf transition-delta telescoping, and an IEEE finite-overflow clamp refinement proving closure, commutativity, identity in range, and loss of divisibility |
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
| `Language.v` | Checked | Well-formed finite weighted-language relation sums final-weighted accepting paths over exact duplicate-free path enumerations; includes exact-enumeration soundness/completeness, aggregate-weight lemmas, bounded language approximations, stable closed-language witnesses, matrix-backed epsilon-closure and product-matrix language weights, public `path_matches` to product-consuming-walk bridge, final-weight endpoint bridge, matching accepting-path occurrence lifting, product-matrix language to product-walk-sum, occurrence-indexed transition-expansion, and finite occurrence-enumerator witness theorems, a label-consumption converse recovering `path_matches` from full-string consumption, a bidirectional Prop-level correspondence between position-accepting final closed occurrence paths and genuine label-transducing accepting paths (both inclusions, including the end-state validity helper), weight-axis de-self-referencing grounding the product-matrix language weight in a multiplicity-preserving sum of independent `accepting_path_weight`s over transducing closed paths, and non-vacuous language equivalence requiring a finite, stable-closure, or matrix-closure witness for each input/output pair; path simulation is separate |

### Phase 3: Algorithm Models (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Viterbi.v` | Checked partial correctness | Final-weight-aware finite candidate-list facts, `viterbi_candidate_optimal` spec predicate, optimal-value theorem, and Bellman-update facts |
| `ShortestDistance.v` | Checked partial correctness | Initialization, relaxation, well-formed empty-WFST solution theorem, and `shortest_distance_solution` fixed-point spec predicate |
| `Determinize.v` | Checked partial correctness | Weighted-subset operations aggregate duplicate target states before normalization, explicit normalization pass with soundness theorem, nonempty-step fact, quotient soundness under nonzero-divisor precondition, non-vacuous `determinize_correct` spec predicate, already-deterministic identity correctness, and functional/sequential precondition facts |
| `Minimize.v` | Checked partial correctness | Residual right-language state equivalence, partition helpers, non-vacuous `minimize_correct` and `push_weights_spec` predicates requiring defined source/target language surfaces, identity-minimize correctness, and language-preservation sanity lemmas |
| `StrongBisimulation.v` | Checked pre-implementation contract | Unbounded labelled semantics, exact replay certificates, complete modal witnesses, dense validation, canonical relation, termination, smaller-half charging, quasilinear work/evidence, linear core heap, zero whole-partition rescans, and constant native stack |

### Phase 4: TLA+ Specifications

| File | Status | Invariants | Notes |
|------|--------|------------|-------|
| `RRWM.tla` | Finite TLC model | `TypeOK`, `RegretWithinAccountingHorizon`, `WeightsPositive`, `LossesBounded`, `TotalLossBounded`, `WeightsExact`, `RoundAccounting` | Bounded integer accounting model with nondeterministic expert choice; includes zero/single/multiple expert configs and an expected-failure stale-weight mutant; not the asymptotic regret theorem |
| `LazyComposition.tla` | Finite TLC model | `MemoryBounded`, `CacheValid`, `WorklistValid`, `NoDuplicateProcessing`, `ProcessedValid`, `NoCacheEmpty`, `AccessOrderValid`, `CacheCoveredByAccessOrder` | Synthetic bounded multi-label/epsilon composition model with `CacheAll`, LRU eviction, and `NoCache` configs plus an expected-failure no-cache mutant |
| `CascadeOrder.tla` | Finite TLC model | `AlphabetsCompatible`, `OrderingConstraints`, `NoRepetition`, `ValidCascade`, `PrefixValid` | Nondeterministic explicit-order component append model starting at AM; includes ordinary, fair, overlapping-alphabet configs and an expected-failure order mutant |
| `AbiCompositionProtocol.tla` | Finite TLC model | `NoCallbackUnderRegWrite`, `RegisterMutualExclusion`, `CacheMutualExclusion`, `NeverBothLocks` | Three-thread provider-callback and registry/cache lock protocol plus callback-under-lock mutant |
| `OptimizerLifecycle.tla` | Finite TLC model | ranked DAG, dependency readiness, no claim promotion, canonical provenance, terminal non-publication, witnessed/confirmed publication | 607 distinct states, depth 14, plus out-of-order provenance mutant |
| `FuzzyReferenceLifecycle.tla` | Finite TLC model | immutable captured index, exact confirmation, no precision/completeness promotion | 175 distinct states, depth 9, plus over-accepting confirmer mutant |
| `LibcpgEvidenceLifecycle.tla` | Finite TLC model | immutable five-coordinate index, digest/trust/independence binding, stale/self/dependent rejection, no outcome promotion | 2,721 distinct states, depth 10, plus dependent-evidence mutant with distinct actor names |
| `LazyWfstLifecycle.tla` | Finite TLC model | no-cache/zero-LRU emptiness, positive LRU bound, exact finite LRU order, transient uniqueness | 80 distinct states, depth 7, plus policy-change mutant |
| `AbiOwnershipLifecycle.tla` | Finite TLC model | retain/owner equality, moved/released non-ownership, stable ABI v1 identity and observations | 912 distinct states, depth 13, plus non-decrementing release mutant |
| `ProviderBoundaryLifecycle.tla` | Finite TLC model | immutable capture, status/limitation preservation, exact-evidence freshness, control-domain independence, cache isolation, balanced native ownership | 1,409 distinct states, depth 11, plus status-promotion, limitation-loss, and dependent-guarantee mutants |
| `NeutralFoundationLifecycle.tla` | Finite TLC model | canonical profile and identity separation; non-strengthening graph projection; atomic patch; complete-only cache; locked exact release; repository spill; compatible resume; tombstone, source, assurance, documentation, release, stack, and terminal-progress laws | 128 distinct states over 16 named adversarial scenarios, depth 8, plus 20 one-defect safety/liveness mutants |
| `StrongBisimulationLifecycle.tla` | Finite TLC model | total dense-input guard; color refinement; reflexive/symmetric relation; exact chained trace; bounded descent; stable oracle equality; canonical matrix; sound/complete separation; eventual terminal state | 14,916 valid distinct states, depth 5; 128 distinct states for each malformed source, target, and dense-label configuration, depth 2 |

### Phase 5: Optimizer and ABI Contracts

| Artifact | Status | Notes |
|---|---|---|
| `TapeSignatures.v` | Checked | Separate tape domains, typed identity/associativity, constructive erased-output counterexample |
| `RewriteSemantics.v` | Checked | Exact rewrite equivalence/witnesses, independent precision/completeness axes, no self-promotion |
| `PlanDag.v` | Checked | Strict-rank path theorem, acyclicity, stack-safe scheduling precondition, ordered commit |
| `OwnershipLifecycle.v` | Checked | Partial release, retain/clone/drop/transfer laws, opaque ABI v1 observation |
| `vco-e4-contracts.smt2` | Checked | Required result sequence `unsat`, `unsat`, `sat`, `unsat`, `unsat`, `unsat` |
| `vco-e4-strong-bisimulation.smt2` | Checked | Fifteen exact validation, relation, certificate, witness, canonicality, progress, work, heap, stack, and nonvacuity verdicts |
| `strong-bisimulation-invariants.tsv` | Checked | 83 exhaustive formal-to-property rows mapped to 13 causally required-red Rust properties |
| `check-strong-bisimulation-exhaustive.py` | Checked | 5,124 complete small-system cases against an independent relational fixed point with stack-safe characteristic formulas |
| `strong_bisimulation_mutants.py` | Checked | All ten injected endpoint, color, transfer, termination, duplicate, modal, certificate, and canonical-ID defects fail causally |
| `abi_ownership_model.rs` | Checked | Kani 0.67.0 / CBMC 6.8.0: 3 of 3 bounded harnesses successful |

### Phase 6: Domain, libcpg, and Provider Boundary Contracts

| Artifact | Status | Notes |
|---|---|---|
| `FuzzyReference.v` | Checked | Indexed exact fuzzy reference, independent confirmation, explicit precision/completeness, stale/configuration/self-confirmation counterexamples |
| `TypedHclg.v` | Checked | Typed H/C/L/G endpoints, category laws, ordered semiring weights, homomorphism obligations, parenthesization equivalence |
| `DataflowMigration.v` | Checked | Join-derived partial order, exact mutation flag, IFDS order reversal, explicit default-bottom bridge, deterministic completion and cap monotonicity |
| `GraphQuotient.v` | Checked | Total/disjoint SCC fibers, exact quotient witnesses, acyclic condensation, renaming equivariance, strict linear import charge |
| `EvidenceAssurance.v` | Checked | Five-coordinate freshness, digest/trust/independence binding, exact accepted/reference equality, no promotion, finite validation control |
| `vco-e6-domain-contracts.smt2` | Checked | Seven-query expected transcript with two constructive satisfiable countermodels |
| `vco-e7-libcpg-assurance.smt2` | Checked | Twelve forbidden claims are unsatisfiable; the all-premises exact witness is satisfiable |
| `domain-integration-invariants.tsv` | Checked | 57 E6 formal-to-property obligations |
| `libcpg-assurance-invariants.tsv` | Checked | 122 exhaustive E7 declaration/check-to-property obligations, all required red before production |
| `ProviderResult.v` | Checked | Three completion classes, functorial payload mapping, non-promoting associative status composition, limitation accumulation, cache eligibility, finite control |
| `CanonicalArtifact.v` | Checked | Permutation/duplicate-invariant finite manifests, digest/size tamper rejection, complete provider identity, six-coordinate evidence freshness |
| `ProviderBoundary.v` | Checked | Control-domain independence, stale/dependent/self rejection, one-way public dependency, private-access rejection, stable native ownership |
| `vco-e9-provider-boundary.smt2` | Checked | Thirteen forbidden claims are unsatisfiable; exact and approximate valid witnesses are satisfiable |
| `provider-boundary-invariants.tsv` | Checked | 132 exhaustive E9 declaration/check-to-property obligations, all required red before production |
| `NeutralFoundationContracts.v` | Checked | 36 unbounded laws for canonical wire, identity, graph, runtime, requirements, assurance, documentation, and stack safety |
| `vco-e9-neutral-foundations.smt2` | Checked | Nineteen forbidden boundary states are unsatisfiable; exact-release and complete-approximate-cache witnesses are satisfiable |
| `neutral-foundation-invariants.tsv` | Checked | 77 exhaustive formal-to-property obligations with complete TLA+ mutant routing; protected-baseline blockers remain explicit |
| `neutral-foundation-api-baselines.tsv` | Checked | Live branch, commit, file-set, and aggregate SHA-256 validation for seven protected or absent owner surfaces |
| Rocq assumption audit | Checked | Every selected E4/E6/E7/E9 theorem is closed under the global context; no global axioms are reported |

## Last Updated

2026-08-30

## Notes

- Rocq files are required to build without unchecked proof escapes.
- The proof-escape checker covers `domain_integration` and distinguishes
  legitimate `Prop := True` counterexample predicates from theorem or lemma
  declarations whose entire proposition is `True`.
- TLA+ specs include TLC config files under `proofs/tla/MC`.
- Algorithm files contain checked specification predicates and partial-correctness theorems over the current finite, stable-closed, matrix-backed epsilon-closure, or product-matrix WFST language surface.
- `make verify-proofs` runs Rocq, every TLC config and expected-failure mutant,
  all seven Z3 transcripts, the exhaustive executable contracts, required-red
  gates, and Kani/CBMC under mandatory local systemd
  resource caps. Java runs headlessly. Persistent evidence lives under ignored
  `target/formal-verification/`.
