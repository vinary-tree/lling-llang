# Proof Status

This document tracks the current formal verification surface for lling-llang.

## Overview

| Category | Total | Checked | Support Lemmas | Unchecked Proof Escapes |
|----------|-------|---------|----------------|-------------------------|
| Semiring Foundations | 4 | 4 | 0 | 0 |
| WFST Definitions | 3 | 3 | 0 | 0 |
| Algorithm Models | 4 | 0 | 4 | 0 |
| TLA+ Specifications | 3 | 3 | 0 | 0 |

## Detailed Status

### Phase 1: Semiring Foundations (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Semiring.v` | Checked | Semiring laws as typeclass obligations, with derived lemmas and no unchecked escapes |
| `TropicalWeight.v` | Checked | Constructive proofs for tropical semiring, order, idempotence, commutative multiplication, and star |
| `LogWeight.v` | Checked | Exact real-valued probability-mass semiring corresponding to log-weight algebra |
| `SemiringProperties.v` | Checked | Power, partial-star, homomorphism, and natural-order lemmas |

### Phase 2: WFST Definitions (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Definitions.v` | Checked | WFST, state, transition, well-formedness, determinism, acceptor predicates |
| `Paths.v` | Checked | Path validity now requires empty accepting paths to start at a final state |
| `Language.v` | Checked | Language equivalence is symmetric by definition and includes final weights |

### Phase 3: Algorithm Models (Rocq/Coq)

| File | Status | Notes |
|------|--------|-------|
| `Viterbi.v` | Support lemmas | Finite candidate-list and Bellman-update facts; no claim for a missing executable implementation |
| `ShortestDistance.v` | Support lemmas | Initialization, relaxation, and empty-WFST facts |
| `Determinize.v` | Support lemmas | Weighted-subset operations and functional/sequential precondition facts |
| `Minimize.v` | Support lemmas | State-equivalence, partition, language-preservation, and identity-relation facts |

### Phase 4: TLA+ Specifications

| File | Status | Invariants | Notes |
|------|--------|------------|-------|
| `RRWM.tla` | Finite TLC model | `TypeOK`, `RegretBound`, `WeightsPositive`, `LossesBounded` | Integer bounded accounting model |
| `LazyComposition.tla` | Finite TLC model | `MemoryBounded`, `CacheValid`, `WorklistValid` | Fixed zero-cache handling and single primed assignment per action |
| `CascadeOrder.tla` | Finite TLC model | `AlphabetsCompatible`, `OrderingConstraints`, `ValidCascade` | Concrete component IDs for AM -> CD -> Lexicon -> LM |

## Last Updated

2026-05-21

## Notes

- Rocq files are required to build without unchecked proof escapes.
- TLA+ specs include TLC config files under `proofs/tla/MC`.
- Algorithm files are intentionally scoped to checked support lemmas until an executable Rocq algorithm relation is added.
