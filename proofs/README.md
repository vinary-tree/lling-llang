# Formal Verification for lling-llang

This directory contains formal proofs and specifications for the lling-llang WFST framework.

## Structure

```
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
├── tla/                # TLA+ specifications
│   ├── RRWM.tla            # RRWM bounded accounting invariants
│   ├── LazyComposition.tla # Lazy composition memory bounds
│   ├── CascadeOrder.tla    # ASR cascade ordering invariants
│   └── MC/                 # TLC model checking configurations
└── doc/                # Documentation
    ├── proof-status.md     # Current verification status
    └── failed-strategies.md # Documentation of failed approaches
```

## Building Coq Proofs

The proofs use Coq 8.18 or later. To build:

```bash
# With resource limiting (recommended for memory-intensive proofs)
systemd-run --user --scope -p MemoryMax=126G -p CPUQuota=1800% \
  -p IOWeight=30 -p TasksMax=200 make -C proofs/coq -j1

# Without resource limiting
make -C proofs/coq
```

## Running TLA+ Model Checking

TLA+ specifications use TLC for model checking:

```bash
# Run all Rocq and TLA+ proof/model checks from the repository root
make verify-proofs

# Or run one TLC model directly
tlc -metadir /tmp/lling-llang-tlc-rrwm \
  -config proofs/tla/MC/RRWM.cfg proofs/tla/RRWM.tla
```

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
  `product_transition_matches`). On the weight axis, the product-matrix language
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
