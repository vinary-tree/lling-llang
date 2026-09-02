# Phase 4: Additional Semirings

**Branch**: `feature/semirings`
**Depends on**: None (parallel track)
**Started**: 2025-12-27
**Status**: COMPLETED

## Overview

Phase 4 extends the semiring foundation (`src/semiring/`) beyond the five basic
weights — Boolean, Count, Tropical, Log, Probability (`src/semiring/basic/`) —
with the compound, loss-augmented, and carrier-valued semirings that the higher
tiers (differentiable decoding, online learning, error models, multi-objective
search) build on. Every weight implements the single `Semiring` trait
($`(K, \oplus, \otimes, \bar{0}, \bar{1})`$) plus whatever capability or marker traits its algebra
supports, so one algorithm runs over all of them. See
[`../../architecture/semirings.md`](../../architecture/semirings.md),
[`../../architecture/power-semiring.md`](../../architecture/power-semiring.md),
and [`../../architecture/signed-tropical-semiring.md`](../../architecture/signed-tropical-semiring.md)
for the full theory.

### Components

1. **Algebraic semirings** (`src/semiring/algebraic/`)
   - **Expectation** `ExpectationWeight`—pairs $`(p,v) \in \mathbb{R} \times \mathbb{R}`$ with the
     product rule $`\otimes`$; computes expected values (for example, expected gradients)
     alongside probabilities.
   - **Gödel** `GodelWeight`—$`([0,1], \max, \min, 0, 1)`$, fuzzy logic.
   - **Lexicographic** `LexicographicWeight` / `Lexicographic3` / `Lexicographic4`
     — priority tuples ordered left-to-right for multi-objective search.
   - **Power** `PowerWeight`—the $`\eta`$-power semiring $`S_{\eta}`$ for soft
     path selection and online learning (`PowerWeight::plus` = $`(x^{1/\eta} + y^{1/\eta})^\eta`$).
   - **Product** `ProductWeight`—the component-wise pair $`(W_1,W_2)`$ for
     running two objectives at once.
   - **Quantized** — 8-/4-bit weight quantization for compact storage.
2. **String/set-carrier semirings** (`src/semiring/string_kind/`)
   - **Edit** `EditWeight` (+ `EditOp`, `EditSequence`, `EditOpCounts`,
     `EditWeightBuilder`) — explicit edit-operation tracking for error models.
   - **Set** `SetWeight` / `StrSetWeight` / `StringSetWeight` / `FeatureSetWeight`
     using $`\cup`$/$`\cap`$ set algebra.
   - **String** `LeftStringWeight` / `RightStringWeight` — longest-common-prefix /
     -suffix with concatenation (documented as *not* a true semiring; see the
     architecture doc for the caveat).
3. **Signed semiring** (`src/semiring/signed/`)
   - **Signed-tropical** `SignedTropicalWeight` — tropical extended with negative
     weights (rewards as negative costs); pairs with `FallibleStarSemiring`
     because $`a^*`$ can diverge on negative weights (`StarDivergenceError`).

---

## 4.1 Compound & loss-augmented semirings

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Each additional weight can be expressed purely through the `Semiring` trait (and
its capability sub-traits) without special-casing any algorithm, so the existing
shortest-distance / composition / determinization code runs unchanged over them.

### Result

Confirmed. All Phase-4 weights implement `Semiring`; capability traits
(`DivisibleSemiring`, `StarSemiring`/`FallibleStarSemiring`,
`WeaklyLeftDivisibleSemiring`, `KClosedSemiring`) and property markers
(`IdempotentSemiring`, `CommutativeTimesSemiring`, `ZeroSumFreeSemiring`,
`TotallyOrderedSemiring`, `QuantizableSemiring`, `StochasticSemiring`) are
implemented exactly where the algebra supports them, and the generic algorithms
operate over every weight with no per-type branches. The semiring-law
obligations are machine-checked in the Coq foundations (`proofs/coq/foundations/`);
the property matrix is summarized in
[`../../architecture/semirings.md`](../../architecture/semirings.md).

### Verification

- Property tests (`proptest`) exercise associativity, commutativity of $`\oplus`$,
  distributivity, and the identity/annihilation laws per weight.
- Coq proofs discharge the semiring laws as typeclass obligations with no
  `admit`/`Axiom`.

---

## Summary

| Weight | $`\oplus`$ | $`\otimes`$ | $`\bar{0}`$ | $`\bar{1}`$ | Notable traits |
|--------|-----|-----|-----|-----|----------------|
| `ExpectationWeight` | $`(p_1+p_2,v_1+v_2)`$ | product rule | $`(0,0)`$ | $`(1,0)`$ | divisible |
| `GodelWeight` | `max` | `min` | `0` | `1` | idempotent, commutative |
| `LexicographicWeight` | lexicographic | componentwise | — | — | totally ordered |
| `PowerWeight` ($`\eta`$) | $`(x^{1/\eta}+y^{1/\eta})^\eta`$ | `+` | $`\infty`$ | `0` | online learning |
| `ProductWeight` | componentwise | componentwise | $`(\bar{0},\bar{0})`$ | $`(\bar{1},\bar{1})`$ | inherits components |
| `EditWeight` | min-cost | compose ops | $`\infty`$ | `[]` | edit tracking |
| `SetWeight` | $`\cup`$ | $`\cap`$ | $`\emptyset`$ | `U` | idempotent |
| `SignedTropicalWeight` | `min` | `+` | $`+\infty`$ | `0` | fallible star |

All eight categories ship and are documented; this phase is **COMPLETED**.
