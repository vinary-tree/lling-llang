# Contributing to lling-llang

Thanks for your interest in **lling-llang** — a pure-Rust, **semiring-generic**
Weighted Finite-State Transducer (WFST) toolkit built on a machine-checked core.
This guide expands the short *Contributing* note in the
[`README.md`](README.md): it covers the dev setup, the design invariants we hold
contributions to, the proofs gate, the documentation guidelines, and the
scientific optimization methodology.

---

## 1. Development setup

```bash
git clone https://github.com/vinary-tree/lling-llang
cd lling-llang

cargo build            # default build: standalone WFST framework, no external deps
cargo test             # unit + property (proptest) tests
cargo test --doc       # doctests — README/doc examples must compile and run
cargo clippy --all-targets
cargo fmt --all
```

The **default build has no external dependencies**. Optional features pull in
integrations and extra layers; see [`Cargo.toml`](Cargo.toml) for the complete,
authoritative list. The full matrix:

| Feature | Enables |
|---|---|
| `levenshtein` | Fuzzy lexical correction via `liblevenshtein` (+ `libdictenstein`). |
| `lattice` | Semiring↔lattice bridge — `lling-llang` semirings as `libdictenstein` dictionary values (via `llattice`). |
| `lattice-persistent` | `lattice` + serde-bounded `DictionaryValue` for disk-backed (persistent-artrie) dictionaries. |
| `pcfg` | Probabilistic CFG support. |
| `error-grammar` | Predefined error grammars (English, etc.). |
| `pos-tagging` / `lm-rerank` | POS-tagging and language-model reranking layers. |
| `phonetic-rescore` | Phonetic lattice rescoring (requires `levenshtein` + `dashmap`; embedded rules + serialization). |
| `code-correction` / `latex-syntax` / `mathml-semantic` | Domain-specific correction layers. |
| `f1r3fly` | Full F1R3FLY.io stack (PathMap; implies `levenshtein` + `sexpr`). |
| `sexpr` | S-expression path format for MORK compatibility. |
| `pathmap-backend` | PathMap-optimized backend (implies `levenshtein`). |
| `serde` / `bincode-ser` | Serialization. |
| `test-utils` | Expose proptest strategies & fixtures to downstream crates. |

Useful invocations while developing:

```bash
cargo build --all-features        # everything (also what docs.rs builds)
cargo test  --features levenshtein,serde
cargo doc   --all-features --no-deps --open
cargo bench                        # Criterion harness (benches/core_benchmarks.rs)
```

## 2. Design invariants

These are the rules that keep the library coherent. A PR that breaks one needs a
very good reason and a discussion first.

### Keep new algorithms generic over the `Semiring` trait

The whole library is organized around one idea: an algorithm written in terms of
$`\oplus`$ (combine alternatives) and $`\otimes`$ (combine in sequence) computes a *different
quantity* depending on which semiring it is instantiated with — shortest path
(Tropical), total probability mass (Log), reachability (Boolean), or an expected
gradient (Expectation). **New graph/automaton algorithms must be parameterized by
`W: Semiring`**, not hard-coded to a single weight type. Concretely:

```rust
// ✅ generic — works for every objective
pub fn my_algorithm<W: Semiring>(lattice: &Lattice<W, B>) -> W { /* ⊕ / ⊗ only */ }

// ❌ avoid — hard-codes the Tropical objective
pub fn my_algorithm(lattice: &Lattice<TropicalWeight, B>) -> f64 { /* min / + */ }
```

Use the identities $`\bar{0}`$ (the $`\oplus`$-identity, "no path") and $`\bar{1}`$ (the $`\otimes`$-identity,
"empty path") rather than literals, and prefer pattern matching over predicate
chains. See [`docs/architecture/semirings.md`](docs/architecture/semirings.md)
and the README's *"The one algorithm behind it"* section for the house style.

Other standing conventions:

- **Preallocate** when the element count is known (`Vec::with_capacity`, sized
  `SmallVec` inline capacity). Preallocation is a best practice here, not a
  premature optimization.
- Prefer `.expect("informative message")` over `.unwrap()` so panics are
  diagnosable.
- Prefer non-blocking algorithms, atomics, and persistent data structures where
  concurrency is in play.

## 3. The proofs gate

The semantics that matter are **machine-checked**. Every Coq/Rocq file builds
with no `admit`, `Axiom`, or `sorry`, and the TLA⁺ specs are model-checked with
TLC (including deliberately-broken mutants that must *fail*). See
[`proofs/README.md`](proofs/README.md) and
[`proofs/doc/proof-status.md`](proofs/doc/proof-status.md).

**If your change touches verified semantics** — the `Semiring` laws, WFST/path
semantics, the determinization/minimization/shortest-distance/Viterbi specs, or
the `RRWM` / `LazyComposition` / `CascadeOrder` protocols — the proof suite must
stay green:

```bash
make verify-proofs        # all Rocq proofs + escape-scan + 9 TLC configs + 3 mutant checks
```

The Rocq build is memory-intensive; a resource-limited invocation is recommended
(adjust `MemoryMax` to your machine):

```bash
systemd-run --user --scope -p MemoryMax=126G -p CPUQuota=1800% \
  -p IOWeight=30 -p TasksMax=200 make -C proofs/coq -j1
```

When you change a proof, record outcomes in the proofs ledgers (§5): document
**failed** strategies too, so nobody re-attempts a dead end.

## 4. Documentation guidelines

All documentation must follow the repository style guide and pedagogical rules.
The top-level [`README.md`](README.md) is the canonical worked example.

- **[`docs/STYLE.md`](docs/STYLE.md)** — the operative rules. Highlights:
  - **Mathematics is MathJax LaTeX, GitHub-delimited.** Write inline math as a
    backtick span wrapped in dollar signs — ``$`\oplus`$``,
    ``$`O(\lvert V\rvert + \lvert E\rvert)`$``, ``$`T = (Q, \Sigma, q_0, F, E, \rho)`$`` —
    and display math in a fenced block whose info-string is `math`. Never a bare
    `$…$` (CommonMark strips the backslashes before MathJax runs) and never
    `$$…$$`. Take the LaTeX for each symbol from the map in
    [`docs/NOTATION.md`](docs/NOTATION.md).
  - **Cardinality** is ``$`\lvert Q\rvert`$`` / ``$`\lvert V\rvert`$`` (`\lvert…\rvert`)
    and the conditional bar is ``$`P(a \mid b)`$`` (`\mid`) — prefer these to a bare
    `|`, which is reserved for Markdown tables and Rust bit-or.
  - **Define before use**: every symbol/acronym gets a local "Terms & symbols"
    table linking the central [`docs/NOTATION.md`](docs/NOTATION.md).
  - Topic docs follow *thesis → terms → formal model → intuition →
    architecture/API → algorithms → examples → diagrams → relation → references*.
  - Algorithms are presented in **literate-programming** form (Knuth): prose
    intent + loop invariant, a named `⟨ chunk ⟩` in a `text` fence — pseudocode
    keeps its Unicode operators inside the fence — then the ``$`O(\cdot)`$``
    complexity and a worked trace as rendered math in the surrounding prose.
  - Code snippets must be **valid** — prefer lifting from `#[cfg(test)]` tests or
    doctests so the compiler checks them; use the real API.
- **[`docs/NOTATION.md`](docs/NOTATION.md)** — the canonical symbol/acronym
  glossary. Add new notation here first, then cite it.
- **[`docs/BIBLIOGRAPHY.md`](docs/BIBLIOGRAPHY.md)** — every non-trivial claim
  traces to a citation, linked by anchor (e.g.
  `[Mohri 2009](docs/BIBLIOGRAPHY.md#ref-mohri2009)`). **Prefer DOIs; never
  fabricate one** — confirm the work exists and the identifier resolves.
- **Diagrams.** Author diagrams as text sources under
  `docs/diagrams/<section>/` and render committed SVGs with:

  ```bash
  make diagrams           # render only changed sources
  make diagrams-force     # re-render everything (e.g. after a palette change)
  make diagrams-check     # validate sources, write nothing (CI gate)
  ```

  Pick the best tool per concept (Graphviz for automata/lattices, PlantUML for
  trait hierarchies & pipelines, D2 for high-level overviews, TikZ for
  publication-grade math figures), reuse the one-color-per-tier palette, and
  **always keep the `<details>` plain-text fallback**. The full tool-per-concept
  matrix, palette, and workflow are in
  [`docs/diagrams/README.md`](docs/diagrams/README.md). Commit **both** the
  source and its sibling `.svg`.

Every new doc must be reachable from [`docs/README.md`](docs/README.md).

## 5. Optimization methodology (scientific ledger)

Optimization is **data-driven and follows the scientific method**. Do not
optimize on intuition: benchmark and profile first, form a hypothesis, test it,
and accept it only if it is a statistically significant improvement. The process
and full results are recorded in
[`docs/archive/journal.md`](docs/archive/journal.md).

1. **Baseline** — benchmark the critical paths (Criterion harness), profile with
   `perf` to find the true hotspot.
2. **Hypothesis** — propose a targeted change with an expected effect and a
   rationale (algorithmic or constant-factor).
3. **Test** — implement, re-benchmark, compare. Acceptance threshold is
   **$`p < 0.05`$** (95% CI, $`\ge 100`$ samples, $`\ge 3`$ s warmup).
4. **Accept / Reject** — merge only statistically significant improvements;
   **record rejections too** with the reason.

Benchmarking hygiene (per the ledger's methodology):

- **CPU affinity** pinned (`taskset -c 0-3`), governor in **performance** mode,
  turbo enabled, ideally under `systemd-run` resource limits.
- Tee benchmark output to a file and analyze it once, rather than re-running to
  read different parts of the output. If a `perf` report can be generated and
  analyzed in parallel with the run, do so.

Every accepted *and* rejected hypothesis gets an entry in the journal, with the
benchmark table, `p`-values, and an analysis of *why*.

### Negative-results ledgers

We deliberately keep records of what did **not** work, so effort is not wasted
re-attempting dead ends:

- **Proofs:** [`proofs/doc/failed-strategies.md`](proofs/doc/failed-strategies.md)
  — failed Coq/Rocq tactic sequences, with the exact error and what worked
  instead.
- **Performance:** the *Rejected Optimizations* sections of
  [`docs/archive/journal.md`](docs/archive/journal.md).

## 6. Commit & PR conventions

- **Branch** off `master`; do not commit directly to it.
- **Conventional-commit** subjects: `feat:`, `fix:`, `perf:`, `docs:`,
  `refactor:`, `chore:`, with an optional scope (`feat(ctc): …`,
  `perf(beam): …`, `fix(lattice): …`). Keep the subject imperative and $`\le`$ ~72
  chars; put the rationale and verification notes in the body.
- **Reference the verification you ran** in the body — e.g. "cargo
  test/clippy clean; `make verify-proofs` green (9 TLC configs + 3 mutants)".
- **Update [`CHANGELOG.md`](CHANGELOG.md)** under `[Unreleased]` for any
  user-visible change (group under Added / Changed / Fixed / Performance).
- A PR should keep `cargo test`, `cargo clippy`, doctests, and — for
  verified-semantics changes — `make verify-proofs` all green. PRs that add or
  change diagrams must pass `make diagrams-check`.

Thank you for contributing!
