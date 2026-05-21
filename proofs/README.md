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
│   │   └── SemiringProperties.v # Generic property lemmas
│   ├── wfst/           # WFST definitions and properties
│   │   ├── Definitions.v       # WFST, State, Transition types
│   │   ├── Paths.v             # Path, PathWeight definitions
│   │   └── Language.v          # Weighted language L(A)
│   └── algorithms/     # Checked algorithm support lemmas
│       ├── Viterbi.v           # Finite-candidate and Bellman-update lemmas
│       ├── ShortestDistance.v  # Initialization and relaxation lemmas
│       ├── Determinize.v       # Weighted-subset lemmas
│       └── Minimize.v          # Equivalence and partition lemmas
├── tla/                # TLA+ specifications
│   ├── RRWM.tla            # RRWM online learning regret bounds
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
# Run model checking for RRWM from the repository root
java -jar tla2tools.jar -config proofs/tla/MC/RRWM.cfg proofs/tla/RRWM.tla
```

## Verification Goals

### Phase 1: Semiring Foundations

- [x] Semiring typeclass definition
- [x] Tropical semiring law proofs
- [x] Log-weight mass semiring law proofs
- [x] Idempotent semiring properties
- [x] k-closed semiring interface with a real stabilization obligation

### Phase 2: WFST Basics

- [x] WFST definition
- [x] Path and path weight definitions
- [x] Weighted language equivalence definition
- [x] Viterbi finite-candidate and Bellman-update support lemmas

### Phase 3: Core Algorithms

- [x] Shortest-distance initialization and relaxation support lemmas
- [x] Determinization weighted-subset support lemmas
- [x] Functional/sequential precondition lemmas
- [x] Minimization equivalence and partition support lemmas

### Phase 4: TLA+ Specifications

- [x] RRWM bounded accounting invariants
- [x] Lazy composition memory bounds
- [x] ASR cascade ordering

## Floating-Point Strategy

The proofs handle floating-point numbers using several strategies:

1. **Abstract numeric domain**: Use ordered fields for proofs not dependent on float specifics
2. **Interval arithmetic**: For error bound proofs (rational upper/lower bounds)
3. **Quantization**: Model QuantizableSemiring for hash-based algorithm proofs
4. **Epsilon-approximate equality**: Match Rust's `approx_eq` with `epsilon: f64`

## Documentation Policy

- Rocq proof files must build without unchecked proof escapes
- Failed proof strategies are documented in `doc/failed-strategies.md`
- Proof status is tracked in `doc/proof-status.md`

## References

- OpenFst documentation
- Mehryar Mohri's WFST tutorial
- Kaldi documentation for ASR-specific algorithms
