# Documentation Style Guide

This guide makes the `lling-llang` documentation uniform, pedagogical, and
machine-checkable. It operationalizes the
[pgmcp documentation guidelines](https://github.com/) (Placement, Coverage,
Pedagogy, Diagrams, Math-notation, Citations/DOIs, Algorithms/code) for this
repository. The top-level [`README.md`](../README.md) is the canonical worked
example of every rule below.

---

## 1. Mathematical notation

- **Unicode, never LaTeX `$…$`.** Use Unicode glyphs for all mathematics:
  `⊕ ⊗ 0̄ 1̄ ∘ π η ∞ ε ⊆ ⊂ ∪ ∩ Σ Γ Δ ρ δ λ ≤ ≥ ≠ ≈ ∈ ∉ ∀ ∃ ⟨ ⟩`. The repo does
  **not** use MathJax; do not introduce it. Subscripts/superscripts use Unicode
  where the glyph exists (`Σ₁`, `q₀`, `aⁿbⁿ`, `e⁻ˣ`), falling back to `Σ_k` only
  when no glyph exists. Every symbol is defined in [`NOTATION.md`](NOTATION.md).
- **Backtick-wrap every mathematical token or expression**, in prose, tables, and
  headings: `` `⊕` ``, `` `O(∣Q∣ + ∣E∣)` ``, `` `T = (Q, Σ, q₀, F, E, ρ)` ``,
  `` `a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)` ``. This keeps math visually distinct and
  prevents Markdown from mangling glyphs. ✅ `` `0.5 ⊗ 0.3 = 0.8` `` ❌ `0.5 ⊗ 0.3 = 0.8`
- **Cardinality bar = `∣` (U+2223 DIVIDES), not ASCII `|`.** Write `` `∣Q∣` ``,
  `` `∣E∣` ``, `` `∣V∣` ``. ASCII `|` is reserved for Markdown **table delimiters**
  and Rust **bit-or** inside code fences — never for set cardinality in math.
- **Identities** are always `0̄` (the `⊕`-identity, “no path”) and `1̄` (the
  `⊗`-identity, “empty path”), defined in prose on first use; never spell them
  "zero-bar" / "0bar".
- **Display formulae** that span multiple lines go in a ```` ```text ```` fence,
  **and** their key relation is restated once inline-backticked in the lead
  sentence so the prose stands alone. Do not rely on a bare fence to read as math.

## 2. Define before use

Every symbol, acronym, and key term is defined **before** its first use — in a
local "Terms & symbols" table at the top of the doc, cross-linked to the central
[`NOTATION.md`](NOTATION.md). Acronyms (WFST, WFSA, CTC, RNN-T, PDA, …) are
expanded on first occurrence per document.

## 3. Pedagogical structure

Each topic/module doc follows this flow (the order conveys *what → how → why*):

1. **Thesis** — one sentence: what this is, in plain language.
2. **Terms & symbols** — local definition table (links to `NOTATION.md`).
3. **Formal model** — the defining tuple/equations in Unicode + backticks, each
   component defined in a table.
4. **Intuition** — a small worked example *before* the heavy theory.
5. **Architecture & API** — key traits/structs and their responsibilities.
6. **Algorithms** — literate pseudocode (§5) where non-trivial.
7. **Examples** — valid Rust (§6).
8. **Diagrams** — at least one structural + one flow diagram (§4).
9. **Relation to the library** — composition points, feature flags.
10. **References** — a `## References` section linking [`BIBLIOGRAPHY.md`](BIBLIOGRAPHY.md).

## 4. Diagrams

Diagrams are authored as text sources and rendered to committed SVGs by
`make diagrams`. See [`diagrams/README.md`](diagrams/README.md) for the
tool-per-concept matrix, the color palette, and the render pipeline. Embedding
pattern (mirrors the README):

```markdown
![<descriptive alt text>](../diagrams/<section>/<name>.svg)

*<one-line color legend>.*

<details><summary>Text view</summary>

```text
<the original ASCII/Unicode art, preserved verbatim>
```

</details>
```

- The relative prefix is one `../` per directory level below `docs/`
  (`docs/algorithms/x.md` → `../diagrams/algorithms/…`; `docs/layers/latex/x.md` →
  `../../diagrams/layers/latex/…`). The root `README.md` uses `docs/diagrams/…`.
- **Always keep the `<details>` plain-text fallback** so screen readers,
  plain-text viewers, and code review never lose the diagram's information.

## 5. Algorithms — literate pseudocode

Present non-trivial algorithms in Knuth literate-programming style: a prose
paragraph stating intent and the loop invariant, a named chunk in a
```` ```text ```` fence (using `⊕`/`⊗`, backticked in the surrounding prose),
then prose explaining each step and the complexity `` `O(∣V∣ + ∣E∣)` ``, then a
worked trace. Name chunks `⟨ relax outgoing arcs ⟩` and cross-reference them. The
README's *“The one algorithm behind it”* section is the house template.

## 6. Code snippets

All code snippets must be **valid** — syntactically and semantically. Prefer
snippets lifted from the module's own `#[cfg(test)]` tests or doctests, which are
compiler-checked. Use the real API (`TropicalWeight::new(0.5)`, not
`TropicalWeight(0.5)`; `EdgeMetadata::original()` / `EdgeMetadata::correction(n)`,
not `EdgeMetadata::default()` where a constructor is intended). Doc examples that
are meant to compile go through `cargo test --doc`.

## 7. Citations

Every non-trivial claim, algorithm, or model traces to a citation in
[`BIBLIOGRAPHY.md`](BIBLIOGRAPHY.md), linked by anchor
(`[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)`). Each topic doc ends with a
`## References` section. Prefer DOIs; never fabricate one.

## 8. Placement & naming

- Topic docs live under the section that matches their tier: `architecture/`
  (foundations), `algorithms/`, `advanced/`, `transducers/` (transducer families),
  `correction/` (NLP/correction), `asr/`, `acoustic/`, `training/`, `programming/`,
  `integration/`, `api/`, `optimization/`.
- File names are intuitive kebab-case (`weight-pushing.md`, `tree-transducers.md`).
- Every doc is reachable from [`README.md`](README.md) (the documentation index).
