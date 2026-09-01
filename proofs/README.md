# Formal Verification for lling-llang

This directory contains formal proofs and specifications for the lling-llang WFST framework.

## Structure

<!-- vdl-disable-next-line ASCII001 -->
```text
proofs/
├── README.md           # This file
├── coq/                # Rocq/Coq proofs
│   ├── _CoqProject     # Coq project configuration
│   ├── Makefile        # Build system
│   ├── foundations/    # Semiring algebra proofs
│   │   ├── Semiring.v          # Semiring typeclass and laws
│   │   ├── TropicalWeight.v    # Tropical semiring proofs
│   │   ├── LogWeight.v         # Log semiring proofs
│   │   ├── Quantization.v      # Quantization and epsilon-approximation proofs
│   │   ├── Interval.v          # Exact interval containment proofs
│   │   ├── Roundoff.v          # Abstract floating roundoff contract proofs
│   │   ├── MatrixClosure.v     # Generic finite semiring matrix-closure proofs
│   │   └── SemiringProperties.v # Generic property lemmas
│   ├── wfst/           # WFST definitions and properties
│   │   ├── Definitions.v       # WFST, State, Transition types
│   │   ├── Paths.v             # Path, PathWeight definitions
│   │   ├── MatrixSemantics.v   # WFST adjacency matrix-closure semantics
│   │   └── Language.v          # Weighted language L(A)
│   └── algorithms/     # Checked algorithm specs and partial-correctness lemmas
│       ├── Viterbi.v           # Finite-candidate and Bellman-update lemmas
│       ├── ShortestDistance.v  # Initialization and relaxation lemmas
│       ├── Determinize.v       # Weighted-subset and normalization lemmas
│       └── Minimize.v          # Equivalence and partition lemmas
│   ├── optimizer/      # Categorical optimizer contracts
│   │   ├── TapeSignatures.v    # Typed input/output composition
│   │   ├── RewriteSemantics.v  # Exactness, precision, completeness
│   │   └── PlanDag.v           # Ranked DAG and ordered provenance
│   ├── domain_integration/
│   │   ├── FuzzyReference.v    # Indexed fuzzy-reference denotation
│   │   ├── TypedHclg.v         # Typed H/C/L/G composition
│   │   ├── DataflowMigration.v # llattice v2 adapter and convergence laws
│   │   ├── GraphQuotient.v     # SCC quotient, renaming, and work bounds
│   │   ├── EvidenceAssurance.v # Fresh, bound, independent exact evidence
│   │   ├── ProviderResult.v    # Non-promoting provider-result algebra
│   │   ├── CanonicalArtifact.v # Canonical manifests and evidence identity
│   │   ├── ProviderBoundary.v  # Independence, dependency, and ownership laws
│   │   └── NeutralFoundationContracts.v # RegresSpec-driven neutral foundation laws
│   └── abi/
│       └── OwnershipLifecycle.v # Retain/release and opaque ABI v1
├── tla/                # TLA+ specifications
│   ├── RRWM.tla            # RRWM bounded accounting invariants
│   ├── LazyComposition.tla # Lazy composition memory bounds
│   ├── CascadeOrder.tla    # ASR cascade ordering invariants
│   ├── OptimizerLifecycle.tla # Concurrent plan lifecycle
│   ├── LazyWfstLifecycle.tla  # Cache policy transitions
│   ├── AbiOwnershipLifecycle.tla # Opaque-handle ownership
│   ├── LibcpgEvidenceLifecycle.tla # Candidate/guarantee publication
│   ├── ProviderBoundaryLifecycle.tla # Generic result/evidence/handle lifecycle
│   ├── NeutralFoundationLifecycle.tla # Neutral release safety and liveness
│   └── MC/                 # TLC model checking configurations
├── smt/                # Z3 dual consistency/countermodel queries
├── kani/               # Bit-precise bounded ABI ownership model
└── doc/                # Documentation
    ├── libcpg-assurance-invariants.tsv # 122 E7 formal/property mappings
    ├── provider-boundary-invariants.tsv # 132 E9 formal/property mappings
    ├── neutral-foundation-invariants.tsv # 77 E9 formal/property/mutant mappings
    ├── neutral-foundation-api-baselines.tsv # Protected owner hashes and gates
    ├── proof-status.md     # Current verification status
    └── failed-strategies.md # Documentation of failed approaches
```

## Running the formal gate

The proofs use Rocq 9.1 or later. Run every theorem, model, negative control,
SMT query, and bounded ABI harness through the self-scoping gate:

```bash
make verify-proofs
```

Local execution requires user systemd. The gate enforces a 4 GiB RSS ceiling,
disables swap, uses one Rocq job and one TLC worker, and imposes a 120-second
timeout per TLC model. Kani runs in its own 2 GiB/no-swap scope with one job.
Tool temporary files, model metadata, and evidence logs stay under ignored
`target/formal-verification/` on persistent repository storage.

The hosted workflow splits that gate by toolchain:

```bash
bash proofs/verify.sh --rocq-only
bash proofs/verify.sh --tla-only
```

`--rocq-only` needs only the pinned Rocq environment. `--tla-only` runs the
portable invariant registries, every finite TLC model and negative model, and
the model-derived exhaustive and mutation controls. The unqualified command
also runs the SMT and bounded-ABI controls plus the exact protected-baseline
registries and required-red checks that depend on independently owned sibling
repositories. Those ownership-gated checks are deliberately not weakened or
silently approximated in a standalone hosted checkout.

## Verification Goals

### Phase 1: Semiring Foundations

- [x] Semiring typeclass definition
- [x] Tropical semiring law proofs
- [x] Log-weight mass semiring law proofs
- [x] Quantization grid and epsilon-approximation proofs
- [x] Exact interval arithmetic containment proofs
- [x] Abstract floating roundoff contract proofs over interval enclosures
- [x] Generic finite matrix closure over semirings, including stabilization-to-star-solution lemmas
- [x] Idempotent semiring properties
- [x] k-closed semiring interface with a real stabilization obligation

### Phase 2: WFST Basics

- [x] WFST definition
- [x] Path and path weight definitions
- [x] Well-formed WFST transition-membership lemmas
- [x] Well-formed finite weighted-language relation over exact duplicate-free accepting-path enumerations
- [x] Bounded/stable language-weight relation for checked cyclic-closure approximations
- [x] Matrix-backed epsilon-closure language weights for cyclic epsilon paths
- [x] Product-state matrix language weights for arbitrary input/output strings
- [x] Non-vacuous language-equivalence relation requiring finite, stable-closure, or matrix-closure witnesses
- [x] WFST adjacency matrix construction, matrix-closure semantics, and finite occurrence-indexed product-path enumeration for filtered transitions
- [x] Viterbi finite-candidate optimal-value theorem and Bellman-update lemmas

### Phase 3: Core Algorithms

- [x] Shortest-distance initialization, relaxation, and well-formed empty-WFST solution lemmas
- [x] Determinization weighted-subset aggregation, normalization, and well-formed already-deterministic correctness lemmas
- [x] Functional/sequential precondition lemmas
- [x] Minimization residual-equivalence, partition, and non-vacuous identity-correctness lemmas

### Phase 4: TLA+ Specifications

- [x] RRWM bounded accounting invariants over finite TLC configs, plus an accounting mutant expected-failure check
- [x] Lazy composition cache/worklist/LRU-order invariants over finite TLC configs, plus a no-cache mutant expected-failure check
- [x] ASR cascade ordering invariants over finite TLC configs, including overlapping alphabets and an order mutant expected-failure check

### Phase 5: Optimizer and ABI Contracts

- [x] Separate input/output tape compatibility and typed morphism category laws
- [x] Exact rewrite witnesses with independent precision and completeness axes
- [x] Rank-certified finite plan DAG and ordered provenance commit
- [x] Optimizer cancellation, budget, failure, completion, and publication lifecycle
- [x] LazyWfst cache-policy transitions for CacheAll, LRU, zero-LRU, and NoCache
- [x] Retain/clone/transfer/release ownership and opaque ABI v1 observation
- [x] Z3 dual consistency/countermodel transcript and Kani/CBMC bounded ABI harnesses

### Phase 6: Domain and libcpg Integration Contracts

- [x] Indexed fuzzy-reference and typed H/C/L/G denotations
- [x] llattice v2 join/order/subsumption migration and explicit bottom bridge
- [x] deterministic fixed-point completion and resource-cap monotonicity
- [x] exact SCC quotient fibers, condensation acyclicity, and renaming equivariance
- [x] strict linear CSR import charge and finite heap-owned control
- [x] five-coordinate evidence freshness, digest binding, trust, and independence
- [x] positive libcpg evidence lifecycle plus required dependent-evidence mutant
- [x] 13-query E7 Z3 transcript with a nonvacuous valid witness
- [x] exhaustive 122-obligation required-red Rust property registry

## Verification Boundary

- Rocq WFST language proofs include exact finite path enumerations plus
  bounded/stable language weights for cyclic closure surfaces whose path-length
  approximations have converged. Generic finite matrix closure and WFST
  adjacency matrix semantics are checked, including stabilization-to-star
  solution lemmas. The language layer includes matrix-backed witnesses for both
  epsilon closure and fixed input/output strings via a WFST-state x
  input-position x output-position product matrix. Product index encode/decode
  and finite-carrier bound lemmas are checked, along with product-matrix
  step/walk predicates. Generic matrix partial-star closure is proved equal to
  an explicit bounded finite walk-sum expansion. Product-matrix closed weights
  can be rewritten to a finite occurrence-indexed enumeration over outgoing
  transition-list entries and product targets, preserving duplicate-transition
  accounting instead of collapsing equal transition records. Public
  `path_matches` facts now imply the corresponding product-matrix consuming
  walk, final-weight endpoints are connected to the encoded product final
  state, and ordinary accepting paths can be lifted to occurrence-indexed
  paths. The checked transition-sequence aggregate is an explicit finite list
  of occurrence paths with final-weighted target annotations, and this
  enumeration is now characterized exactly: its membership is proved sound and
  complete against the bounded product-occurrence walk relation (under
  `wfst_well_formed`), and the generated occurrence-path and final-annotated
  closed-path lists are proved duplicate-free (`NoDup`). The weighted aggregate
  equality therefore no longer stands alone — the structural correspondence
  between the generated lists and the bounded product-occurrence walks is now a
  checked theorem, closing the last formal bridge. Furthermore, the product
  semantics is no longer self-referential: the weighted equivalence chain
  bottoms out in `product_transition_matches`, and that primitive is now
  validated against genuinely independent oracles. On the structure axis, every
  position-accepting closed occurrence path landing on a final state is proved
  to be an actual `accepting_path` whose epsilon-collapsed labels equal the
  strings (`path_matches`, which mentions neither `consume_label` nor
  `product_transition_matches`), and conversely every real accepting transducing
  path is enumerated as such a closed occurrence path — a full bidirectional
  correspondence. On the weight axis, the product-matrix language
  weight is proved equal to a multiplicity-preserving sum of the independent
  `accepting_path_weight` over those genuinely transducing closed paths. The
  older duplicate-free
  `PathSet` relation remains the finite/acyclic plain-path surface and is
  intentionally not used to quotient duplicate occurrences in non-idempotent
  semirings, since occurrence indexing deliberately preserves the multiplicity
  of duplicate transitions for the weighted sum. Algorithm
  language-equivalence specs require an explicit finite, stable-closure, or
  matrix-closure witness for each input/output pair, so unsupported cyclic
  cases do not prove equivalent by an empty relation.
- Rocq algorithm files contain checked specification predicates and
  partial-correctness theorems over the current finite, stable-closed, or
  matrix-backed WFST language surface.
- TLA+ files are finite model checks. They are useful for catching state-machine
  mistakes; asymptotic mathematical claims must be stated and checked as
  separate theorems.
- The E7 formal baseline proves contracts, not the concrete libcpg/libvgraph/
  llattice v2 adapter refinement. The 122 registered Rust properties must
  record a genuine red baseline before production migration and pass afterward.
- The E9 formal baseline is provider-neutral. It does not place lling-llang
  inside libcpg or create a reverse dependency. Its 132 properties must record
  a genuine red baseline before provider-adapter production work and pass
  afterward.
- The RegresSpec-driven E9 neutral-foundation baseline adds 77 exhaustive
  obligations and 20 causal TLA+ mutants. Five executable owner surfaces are
  causally red; the independent content-identity crate is absent by design;
  requirements and documentation remain explicitly blocked by protected
  pre-API build baselines.

## Floating-Point Strategy

The current Rocq proofs use exact mathematical domains:

1. **Tropical reference model**: finite real costs plus `+∞`, excluding NaN and `-∞`.
2. **Log-weight reference model**: exact probability-mass algebra corresponding to
   negative-log semantics, not Rust `f64` rounding behavior.
3. **Rust numeric boundary**: `TropicalWeight::new` and `LogWeight::new`
   reject `NaN` and `-∞`, preserving the finite-real-or-`+∞` boundary used by
   the Rocq models. `QuantizationParams::new` rejects non-finite bounds and
   non-finite ranges, preserving the finite real grid modeled by
   `Quantization.v`. `LogWeight::from_probability` rejects probabilities
   outside `[0, 1]`; `new_unchecked` remains only for low-level raw IEEE-754
   interop.

Quantization grid correctness, bucket half-step error, epsilon-approximate
equality, exact interval containment arithmetic, and abstract floating roundoff
contracts are modeled over real values in `Quantization.v`, `Interval.v`, and
`Roundoff.v`.

## Documentation Policy

- Rocq proof files must build without unchecked proof escapes
- Failed proof strategies are documented in `doc/failed-strategies.md`
- Proof status is tracked in `doc/proof-status.md`

## References

- OpenFst documentation
- Mehryar Mohri's WFST tutorial
- Kaldi documentation for ASR-specific algorithms
