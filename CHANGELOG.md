# Changelog

All notable changes to **lling-llang** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries are reverse-chronological and grouped under **Added**, **Changed**,
**Fixed**, and **Performance**. Mathematics is written in Unicode and wrapped in
backticks per [`docs/STYLE.md`](docs/STYLE.md); symbols are defined in
[`docs/NOTATION.md`](docs/NOTATION.md).

> **Tags & version history.** The crate manifest declared `version = "0.1.0"`
> from the initial commit through the `lattice`-bridge commit (git tag
> [`v0.1.0`](https://github.com/vinary-tree/lling-llang/releases/tag/v0.1.0),
> `6d08f25`, 2026-06-10), and bumped to `0.2.0` in the release commit (git tag
> [`v0.2.0`](https://github.com/vinary-tree/lling-llang/releases/tag/v0.2.0),
> `743127e`, 2026-06-15). The release headings below group features by the
> release that shipped them: `0.1.0` is the initial WFST framework (the
> 2025-12 development burst), and `0.2.0` collects everything between the two
> tags — formal verification, the transducer-family expansion, the
> semiring↔lattice bridge, the Apache-2.0 relicense, and the documentation
> overhaul.

---

## [Unreleased]

### Added
- **Symbolic-automata + algebra-tower core (`lling_llang::symbolic`).** Hoisted from
  the `prattail` crate as the shared foundational home (Task #21 / ADR-018): effective
  Boolean algebras (`BooleanAlgebra`), Symbolic Finite Automata/Transducers (SFA/SFT),
  the `Sat3` three-valued tower (`RejectSafeAlgebra`/`HeytingAlgebra`), the generic
  solver bridge (`ConstraintTheory`/`TheoryAlgebra`), behavioral (μ-calculus) algebra,
  KAT `BooleanTest`, subtype-lattice and Presburger decision procedures, and the
  zero-admission Rocq proofs of their algebra laws (`proofs/coq/{logict,presburger,sft,
  symbolic_algebra}`, 16 theories, admission-free under Rocq 9.1.1). `prattail` now
  re-exports this core for source compatibility and retains only its grammar-specific
  glue. New deps: `num-bigint`, `num-rational`, `num-traits`, `moniker` (all
  non-optional — the module is unconditional). See
  [`docs/architecture/symbolic-core-hoist.md`](docs/architecture/symbolic-core-hoist.md).
- **Documentation overhaul.** A full pedagogical documentation tier under
  [`docs/`](docs/), governed by a machine-checkable style guide:
  - [`docs/STYLE.md`](docs/STYLE.md) — Unicode-math-in-backticks convention,
    define-before-use rule, the *thesis → terms → model → intuition →
    architecture → algorithm → examples → diagrams → references* topic-doc flow,
    and the literate-pseudocode (Knuth) house template.
  - [`docs/NOTATION.md`](docs/NOTATION.md) — the canonical glossary of every
    algebraic/automata symbol (`⊕`, `⊗`, `0̄`, `1̄`, `∘`, `π`, `η`, `∣Q∣`, …)
    and acronym (WFST, WFSA, CTC, RNN-T, PDA, LF-MMI, …), defined once.
  - [`docs/BIBLIOGRAPHY.md`](docs/BIBLIOGRAPHY.md) — the citation-checked
    reference list; every DOI / arXiv / ACL / PMLR identifier verified to
    resolve to the stated work.
  - Per-tier module docs under `docs/architecture/`, `docs/algorithms/`,
    `docs/advanced/`, `docs/transducers/`, `docs/correction/`, `docs/asr/`,
    `docs/acoustic/`, `docs/training/`, and `docs/integration/`.
- **Diagram pipeline.** Diagrams authored as text sources and rendered to
  committed SVGs via `make diagrams` (and `make diagrams-force` /
  `make diagrams-check`), using the locally-installed subset of the pgmcp
  diagramming catalog (Graphviz, PlantUML, D2, TikZ). The tool-per-concept
  matrix, color palette (one intuitive color per tier), and contributor
  workflow live in [`docs/diagrams/README.md`](docs/diagrams/README.md).
- **Repository documentation.** `CHANGELOG.md` (this file), `CONTRIBUTING.md`,
  and `ARCHITECTURE.md`.

### Changed
- README and module doc-comments cross-link into the new `docs/` tier rather
  than restating concepts inline.

## [0.2.0] — 2026-06-15

The verification, transducer-family, and integration release. The core
(`semiring`, `wfst`, `lattice`, `algorithms`, `path`, `cfg`, `composition`,
`layers`) gains a machine-checked semantics, a transducer zoo, and a
semiring↔lattice bridge into the dictionary-family crates.

### Added
- **Semiring↔lattice bridge** (`lattice` feature). The
  `SemiringLatticeWrapper` / `llattice::Lattice` (join/meet) bridge, relocated
  from `libdictenstein` to `lling-llang` where it is orphan-rule-legal (this
  crate owns the semiring types), letting `lling-llang` semirings be used
  directly as dictionary values. Adds `src/lattice_bridge.rs` and the
  union-zipper integration tests. A `lattice-persistent` feature adds a
  serde-bounded `DictionaryValue` for disk-backed (persistent-artrie)
  dictionaries. (`6d08f25`)
- **Formal verification of WFST semantics** — Coq/Rocq proofs and TLA⁺ models,
  with no `admit`/`Axiom`/`sorry` (`d54000b`, `6a2316d`):
  - **Coq foundations:** semiring laws; tropical & log weights; quantization,
    interval, and abstract-roundoff contracts; generic finite-semiring
    matrix-closure with stabilization-to-star-solution lemmas.
  - **Coq WFST semantics:** WFST/state/transition definitions; path & path-weight
    definitions; adjacency-matrix-closure semantics; the weighted language
    `L(A)` via duplicate-free, occurrence-indexed accepting-path enumerations.
  - **Reverse inclusion** — every real accepting transducing path is enumerated
    by the product-occurrence closed-path machinery, completing a full
    bidirectional correspondence between position-accepting-final closed
    occurrence paths and accepting transducing paths (`6a2316d`).
  - **Coq algorithm specs:** partial-correctness predicates and Bellman-update
    lemmas for Viterbi, shortest-distance, determinization, and minimization.
  - **TLA⁺ models:** `RRWM` (bounded online-learning accounting),
    `LazyComposition` (cache / worklist / LRU-order memory bounds), and
    `CascadeOrder` (`H → C → L → G` ordering) — 9 TLC configs plus 3
    expected-failure mutants that prove the checks have teeth.
- **Transducer families & new layers** (`f60fc69`): multitape (`k`-tape)
  transducers, weighted pushdown automata (PDA), tree transducers, error models
  (edit-distance, Damerau-Levenshtein, confusion-matrix, homophone), and
  additional correction/proof layers.
- **Documentation index & guides** under [`docs/`](docs/) wired to the README
  (architecture, algorithms, advanced, ASR/acoustic, training, integration,
  API reference).

### Changed
- **License: `MIT OR Apache-2.0` → `Apache-2.0`** in the crate manifest
  ([`Cargo.toml`](Cargo.toml), [`LICENSE`](LICENSE)). (`6d08f25`)
- **Dependency pins.** `pathmap` pinned to the crates.io `0.2` release; inter-crate
  dependencies given explicit versions; added `rust-version` and
  `[package.metadata.docs.rs] all-features = true`. The crate `repository` field
  now points at `vinary-tree`. (`6d08f25`)
- **`libdictenstein` 0.2 dictionary-family submodule reorg.** Repointed
  `libdictenstein::dynamic_dawg_char::*` → `dynamic_dawg::char::*` across the
  liblevenshtein bridge, the `integration` module, and the union-zipper tests;
  bumped the `libdictenstein` requirement to `0.2` (a breaking module-path
  change surfaced through `lling-llang`'s public `integration` re-exports).
  (`8ece99d`, `743127e`)
- **README overhaul** (`559b16e`): documents every module (transducer families,
  ASR/CTC/RNN-T, training, differentiable, GPU, text/LLM/programming) and the
  Coq+TLA⁺ suite; adds a Notation glossary, a compiled quick-start, and literate
  shortest-distance/Viterbi pseudocode; replaces the malformed ASCII lattice
  with a color-coded WFSA SVG plus a plain-text fallback; wraps all inline math
  in backticks and uses the Unicode bar `∣` (U+2223) for cardinality.
- **Technical-debt cleanup** across the crate (`11cabba`).

### Fixed
- **Citations corrected** across source doc-comments and `docs/` (`559b16e`):
  the GPU decoder is attributed to **Braun et al. (2020)** (not Laptev et al.);
  the GTN differentiable-WFST venue is **ICML 2020** (`arXiv:2010.01003`), not
  "ICLR 2021"; path-experts / power-semiring is **COLT 2015** (PMLR v40), not
  "JMLR 16"; and the Factorized-Neural-Transducer and NeMo-ITN paper titles are
  fixed. Unbenchmarked performance numbers were removed.

### Performance
- The TLA⁺ `LazyComposition` model bounds the lazy-composition cache memory
  (cache / worklist / LRU-order invariants), underwriting the demand-driven
  composition strategy.

## [0.1.0] — 2025-12-29

Initial public framework: a pure-Rust, **semiring-generic** WFST toolkit. The
foundation (`semiring`, `wfst`, `lattice`, `algorithms`, `path`, `cfg`,
`composition`, `layers`) is exercised by property tests and benchmarks.

### Added
- **Semiring foundation** — ~15 weight types (Tropical, Log, Probability,
  Boolean, Expectation, Product, Lexicographic, Power/`η`-power, String, Count,
  Gödel, SignedTropical, …) behind a single `Semiring` trait, so one algorithm
  computes shortest path, total probability mass, reachability, or an expected
  gradient by swapping the weight type (`2a9495a`, Phase 4 semirings
  `0f368e0`).
- **WFST core & rational operations** — the `Wfst`/`MutableWfst` traits and
  `VectorWfst`; union (`A ∪ B`), concatenation (`A · B`), Kleene closure (`A*`),
  invert, project, reverse, and lazy composition (`A ∘ B`) (Phase 2 core ops,
  `c36afe5`).
- **Lattices** — the weighted-DAG `Lattice`, `LatticeBuilder`, `Node`, `Edge`,
  and the `LatticeBackend` storage abstraction (`HashMapBackend`).
- **Shortest-distance & path extraction** — the generalized single-source
  shortest-distance algorithm with **queue disciplines** (`3cf4a19`), and
  `viterbi`, `nbest` (top-`k`), and `beam_search` path extractors.
- **Core WFST algorithms** — weight pushing, `ε`-removal, `connect` (trimming),
  and synchronization, generic over the semiring (`c36afe5`).
- **Determinization & minimization** — weighted-subset determinization and
  partition-refinement minimization (Phase 3, `e72e343`).
- **CTC topologies** — `CorrectCtc`, `CompactCtc`, `MinimalCtc`, `SelflessCtc`
  graph topologies for ASR (Phase 5, `8b4c46a`).
- **CFG parsing on lattices** — a `Grammar`, an **Earley parser**, and
  `ParseForest`, adapted to run over a lattice rather than a single string.
- **Acoustic modeling, path sampling, RRWM, and phonetic rescoring** —
  triphone context-dependency, n-gram LMs, the cascade builder, randomized
  weighted-majority (RRWM) over path experts, and phonetic lattice rescoring
  (`3b41d11`).
- **Differentiable operations** — forward-score and Viterbi autograd through
  WFST operations, WFST convolutional layers, and arc-posterior gradients
  (GTN-style).
- **GPU-ready data structures** — CSR adjacency, lock-free uint64 token packing,
  k-vector atomic reduction, and mark-and-compact soft pruning (CPU-side
  layouts; CUDA/`wgpu` kernels are a documented extension point).
- **Benchmark harness** — a Criterion harness in
  [`benches/core_benchmarks.rs`](benches/core_benchmarks.rs) and the scientific
  optimization ledger in
  [`docs/optimization/journal.md`](docs/optimization/journal.md).

### Fixed
- **Minimization floating-point tolerance** and context-dependency label
  encoding (`b44d10b`).

### Performance
*Accepted optimizations from the scientific ledger
([`docs/optimization/journal.md`](docs/optimization/journal.md)); each was merged
only after a benchmarked improvement at `p < 0.05`.*
- **Topological sort `O(∣V∣²)` → `O(∣V∣ + ∣E∣)`** by building an
  `edge_id → target` lookup table once instead of scanning all nodes per edge —
  **−94%** on a 200-node diamond lattice (**17.6×** faster) (`c3449c2`).
- **`log_sum_exp` fast path** — when `∣a − b∣ > 20`, `e⁻ᵈⁱᶠᶠ` underflows below
  `f64` precision, so the result is simply `min(a, b)`, skipping `exp`/`ln`:
  **≈ −10%** on log-semiring ops, with `−5…12%` cascading across algorithms
  (`ef735dc`).
- **Beam-search allocation removal** — eliminated the intermediate `Vec` in the
  edge-expansion loop (direct iteration): **≈ −23%** on beam search (`347e98d`).
- **Path-extend clone reduction** — added `extend_move(self, …)` and a
  move-last pattern so each path extension saves one `SmallVec<[EdgeId; 16]>`
  clone: **≈ −25%** on beam search, **up to −21%** on N-best (`8bf8d78`).

*Rejected optimizations (documented in the ledger so they are not re-attempted):*
semiring `#[inline(always)]` (compiler already inlined; forcing it bloated
code), beam-search `select_nth_unstable` (`O(n)` only wins for large `n`),
Earley chart-merge `HashSet` and Earley state-clone reduction (both regress for
`SmallVec<[T; 4]>`).

[Unreleased]: https://github.com/vinary-tree/lling-llang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/vinary-tree/lling-llang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/vinary-tree/lling-llang/releases/tag/v0.1.0
