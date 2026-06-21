# Symbolic-Automata Core Hoist (`lling_llang::symbolic`)

> **Status:** Implemented & verified (Task #21 / ADR-018).
> **Scope:** Relocation of the reusable symbolic-automata + algebra-tower core from
> the `prattail` crate (`mettail-rust/prattail`) into `lling-llang` as the shared
> foundational home, with `prattail` re-exporting it for source compatibility.

## 1. Motivation

`prattail` (the PraTTaIL parser generator) had grown a substantial, *parser-agnostic*
symbolic-verification core: effective Boolean algebras, Symbolic Finite Automata /
Transducers (SFA/SFT), the `Sat3` three-valued algebra tower
(`BooleanAlgebra` → `RejectSafeAlgebra` → `HeytingAlgebra`), a generic
solver bridge (`ConstraintTheory` / `TheoryAlgebra`), behavioral (μ-calculus) algebra,
and a Presburger-arithmetic decision procedure — all backed by **zero-admission Rocq
proofs**.

This core is needed by consumers that have nothing to do with `prattail`'s parser:

- **pgmcp** — links it in-process for native formal-verification MCP tools
  (`protocol_soundness`, `language_inclusion`, `behavioral_check`, …).
- **the constrained decoder** (`SymbolicConstrainedDecoder`) — masks generation to
  *behaviorally*-valid output.
- **the WFST sidecar** — generation-time constraint/repair services.

The placement rule for automata in this workspace is **"automata belong in
`lling-llang`"** (the foundational WFST/automata home). Keeping the symbolic core in
`prattail` would force every consumer to depend on the whole parser generator and would
invert the dependency hierarchy. The clean long-term solution is to **hoist the core into
`lling-llang`** and have `prattail` (and everyone else) depend on it there.

```text
            ┌──────────────────────── BEFORE ────────────────────────┐
            │  prattail  ── owns ──▶  symbolic core  ◀── (no clean      │
            │     ▲                   + Rocq proofs       path for      │
            │     └── parser, WPDS, grammar glue          pgmcp/decoder)│
            └─────────────────────────────────────────────────────────┘

            ┌──────────────────────── AFTER ─────────────────────────┐
            │  lling-llang::symbolic  ◀── owns ── symbolic core        │
            │       ▲      ▲      ▲                + Rocq proofs        │
            │       │      │      └──────────────── pgmcp (in-process)  │
            │   prattail  decoder / sidecar                            │
            │   (re-exports + grammar glue)                            │
            └─────────────────────────────────────────────────────────┘
```

No dependency cycle is introduced: `lling-llang` depends on none of
`prattail`/`rigail`/`mettail-*`; `prattail` gains a single path dependency on
`lling-llang`.

## 2. What was hoisted

`lling-llang/src/symbolic/` (19 files: `mod.rs` + 18 modules):

| Module | Contents |
|--------|----------|
| `sfa` | `BooleanAlgebra` trait, `SymbolicAutomaton` (SFA), `IntervalAlgebra`, `CharClassAlgebra`, `PredicateExpr`, `SymbolicAnalysis`, `ProductAlgebra`, minterm determinization / intersection / complement / emptiness / equivalence. **The root** — re-exported at `crate::symbolic::*`. |
| `kat_algebra` | KAT `BooleanTest` (the Boolean subalgebra of Kleene Algebra with Tests) + the `KatBooleanAlgebra` adapter + `eval_test_public`. |
| `algebra_tower` | `Sat3` (`Sat`/`Unsat`/`DontKnow`), `RejectSafeAlgebra`, `HeytingAlgebra`. |
| `any_algebra` | `AnyAlgebra`/`AnyPred` dynamic-dispatch union over all algebras. |
| `regex_sfa` | `RegexAlgebra` / `RegexPred` (regex predicates over SFA). |
| `string_algebra` | `StringAlgebra` / `StrPred`. |
| `collection_algebra` | `BagAlgebra`, `MapAlgebra`, `Singleton` predicates. |
| `ordered_field` | `OrderedFieldAlgebra` over `OrderedF64` (rationals / reals). |
| `product_nary` | `NaryProductAlgebra`, `SumAlgebra` (n-ary product / sum). |
| `sym_tree` | symbolic **tree** automata (`SymTerm`, `TreeAlgebra`, `TreePred`). |
| `sym_tree_transducer` | symbolic tree transducers. |
| `sft` | Symbolic Finite **Transducers** (SFT/STFT). |
| `behavioral_algebra` | μ-calculus `BehavioralFormula` over an LTS, on the `Sat3` tower. |
| `behavioral_pred` | `BehavioralPred` (`moniker::BoundTerm` leaf). |
| `logict` | `ConstraintTheory` trait, `TheoryAlgebra<T>` (the generic solver→`BooleanAlgebra` bridge), `LogicStream`. |
| `lattice_theory` | subtype-lattice `ConstraintTheory` (`LatticeTheory`, `LatticeStore`). |
| `bisimulation` | bisimulation equivalence. |
| `presburger` | Presburger predicates, `PresburgerNfa`, `PresburgerAlgebra` (`impl BooleanAlgebra`). |

Plus the **Rocq proofs** (`lling-llang/proofs/coq/{logict,presburger,sft,symbolic_algebra}/`,
16 `.v` files, all admission-free) that verify the algebra laws of this core.

### Deliberately **not** hoisted

- `tree_automaton.rs` — a *weighted* tree automaton over `rigail::Semiring`. It is
  depended on by nothing in the symbolic core and would overlap `lling-llang`'s existing
  `tree_transducers/`. It stays in `prattail`.
- `ltl.rs`, `buchi.rs` — deeply parser-coupled (`SyntaxItemSpec`/`pipeline`/`wpds`). Stay
  in `prattail`.
- The WPDS engine, the parser, and all grammar-specific glue. Stay in `prattail`.

## 3. The core / glue split

Five files mixed the *reusable core* with *prattail-grammar glue* (functions consuming
`crate::SyntaxItemSpec` / `crate::pipeline::CategoryInfo` — the grammar IR). The core
moved; the glue stayed in `prattail` (it cannot move without dragging the grammar IR,
which would invert the dependency).

| File | Core → `lling-llang` | Glue stays in `prattail` |
|------|----------------------|--------------------------|
| `symbolic.rs` | SFA core → `sfa.rs`; KAT adapter → `kat_algebra.rs` | `collect_leading_terminals`, `analyze_from_bundle`, `SymbolicCompiler` |
| `kat.rs` | `BooleanTest` → `kat_algebra.rs` | `KatExpr`, the KAT automaton + Hoare logic (uses `wpds`) |
| `lattice_theory.rs` | `LatticeTheory`/`LatticeStore`/`SubtypeConstraint`/`TypeAssignment` | `LatticeAnalysis`, `analyze_from_bundle`, `extract_*` |
| `presburger.rs` | predicates / NFA / `PresburgerAlgebra` / `div_floor` | `extract_numeric_guard`, `analyze_from_bundle` |
| `sft.rs` | SFT/STFT machinery | `analyze_from_bundle` |

The remaining 13 modules had no grammar coupling and moved wholesale.

## 4. Module layout & path resolution

`symbolic/mod.rs` declares the 18 submodules and **re-exports the SFA root**:

```rust
pub mod sfa;
pub mod kat_algebra;
/* … 16 more … */
pub use sfa::*;                                   // crate::symbolic::BooleanAlgebra, …
pub use kat_algebra::{eval_test_public, KatBooleanAlgebra};
```

Because the SFA root is re-exported at `crate::symbolic`, sibling modules keep writing
`crate::symbolic::BooleanAlgebra` unchanged. Cross-sibling references were rewritten
`crate::<sib>::` → `crate::symbolic::<sib>::` (the only mechanical edit), and the KAT
`BooleanTest` import was redirected to `crate::symbolic::kat_algebra::BooleanTest`.

`#![allow(missing_docs)]` is set on the `symbolic` module: the code is hoisted verbatim
and inherits prattail's documentation posture rather than `lling-llang`'s
`#![warn(missing_docs)]`.

## 5. prattail re-export (source compatibility)

Every hoisted `prattail` module becomes a thin shim so all existing `crate::<mod>::*`
consumer paths (36 modules referencing `crate::symbolic`, plus `kat`/`logict`/`sft`/…)
keep resolving:

```rust
// prattail/src/algebra_tower.rs
pub use lling_llang::symbolic::algebra_tower::*;
```

The five split files become **residuals**: `pub use lling_llang::symbolic::<mod>::*;`
plus the retained grammar glue and the original unit tests (which exercise the
re-exported core through prattail). Test-only imports that the lib no longer needs are
`#[cfg(test)]`-gated.

## 6. Public-API widenings

A handful of core internals that the retained prattail tests exercise were promoted to
`pub` — each is legitimately useful library surface, not test-only scaffolding:

- `sfa::compute_minterms` — minterm computation (a core SFA primitive).
- `PresburgerNfa::{universal, empty_language}` — standard automaton constructors.
- `presburger::div_floor` — floor division over `i64`.
- `LatticeStore` fields (`edges`, `closure`, `closure_dirty`, `lub_cache`, `glb_cache`,
  `cycles`) — read-only inspection of an analysis store; call
  `LatticeTheory::compute_closure` before reading `closure`.

## 7. Cargo wiring

`lling-llang` gained four non-optional dependencies (the symbolic module is
unconditional — no feature gates): `num-bigint`, `num-rational` (+`num-bigint-std`),
`num-traits`, `moniker` (+`moniker-derive`). `prattail` gained `lling-llang = { path =
"../../lling-llang" }`.

## 8. Rocq-proof migration

The four self-contained theory directories moved from `mettail-rust/formal/rocq/` to
`lling-llang/proofs/coq/` and were wired into the single-tree `_CoqProject` with their own
logical roots (`LogicTProofs`, `PresburgerProofs`, `Sft`, `SymbolicAlgebra`). The
`proof-check` admission grep was tightened to match Coq **vernacular** (anchored
`Admitted.`/`Abort.`/`Axiom`/`Conjecture`/`Parameter`/`admit.`) instead of bare words, so
the theories' self-documenting prose ("zero `Admitted`") no longer false-positives while
real escapes are still caught.

> The `mettail-rust/formal/rocq` originals were **copied, not deleted** (pending explicit
> approval to remove); the canonical home is now `lling-llang`.

## 9. Verification

| Check | Result |
|-------|--------|
| `cargo build -p lling-llang` | ✅ clean |
| `cargo test -p lling-llang --lib` | ✅ **2047 passed, 0 failed** (172 in `symbolic::`) |
| `cargo build -p prattail` | ✅ clean (0 own warnings) |
| `cargo check -p prattail --all-targets` | ✅ clean (lib + tests + benches + examples) |
| `cargo test -p prattail --lib '{symbolic,kat,lattice_theory,presburger,sft}::'` | ✅ **279 passed** through the re-export |
| Rocq compile (16 theories, Rocq 9.1.1) | ✅ 16/16 `.vo`, **38× "Closed under the global context"** (admission-free) |
| `make proof-check` | ✅ "No unchecked proof escapes found" |
| pgmcp `verify.sh` (links `lling-llang`) | ✅ builds/clippy-clean against the hoisted crate |
