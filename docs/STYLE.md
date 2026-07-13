# Documentation Style Guide

This guide makes the `lling-llang` documentation uniform, pedagogical, and
renderable. It operationalizes the **pgmcp documentation guidelines** (the
canonical checklist returned by pgmcp's `documentation_guidelines` tool —
categories: Placement, Coverage, Pedagogy, Diagrams, Math-notation,
Citations/DOIs, Algorithms/code) for this repository. The top-level
[`README.md`](../README.md) is the canonical worked example of every rule below.

---

## 1. Mathematical notation — MathJax LaTeX, GitHub-delimited

All mathematics is written as **MathJax LaTeX**, never as Unicode glyphs and never
as bare `$…$`. The Unicode → LaTeX map lives in [`NOTATION.md`](NOTATION.md) (its
**LaTeX** column is authoritative); every symbol is defined there before use.

- **Inline math = a backtick span wrapped in dollar signs:** ``$`\oplus`$``,
  ``$`O(\lvert Q\rvert + \lvert E\rvert)`$``, ``$`T = (Q, \Sigma, q_0, F, E, \rho)`$``.
  That is: dollar, backtick, LaTeX, backtick, dollar. The backticks are load-bearing —
  GitHub's CommonMark pass strips LaTeX backslash escapes (`\_ \{ \} \; \, \#`)
  *before* MathJax runs, so a bare `$\Sigma_1$` silently corrupts to `$\Sigma1$`.
  Wrapping the expression in a code span preserves it. ✅ ``$`\bar{0} \otimes a = \bar{0}`$`` ❌ `$\bar{0} \otimes a$` ❌ `` `$\bar{0}$` `` (renders as literal code, not math).
- **Display math = a fenced block whose info-string is `math`:**

  ````markdown
  ```math
  d(s,t) = \bigoplus \{\, w(\pi) : \pi \text{ a path } s \to t \,\}
  ```
  ````

  Never use `$$…$$`. A multi-line derivation uses `\begin{aligned} … \end{aligned}`
  inside the `math` fence.
- **Never leave math bare or in an inert code span.** A quantity in prose, a table
  cell, or a heading is either a ``$`…`$`` span or (for a listing) inside a
  pseudocode/ART fence — never a plain `` `⊕` `` code span and never unwrapped prose.
- **Cardinality** is ``$`\lvert Q\rvert`$`` / ``$`\lvert E\rvert`$`` (`\lvert…\rvert`).
  The **conditional bar** is ``$`P(a \mid b)`$`` (`\mid`). A literal pipe `|` inside a
  math span is safe in a Markdown table only because the backtick code span protects
  it; prefer `\lvert…\rvert` in tables to avoid ambiguity.
- **Identities** are ``$`\bar{0}`$`` (`\bar{0}`, the $`\oplus`$-identity, "no path")
  and ``$`\bar{1}`$`` (`\bar{1}`, the $`\otimes`$-identity, "empty path"), defined in
  prose on first use. **Combining accents** ($`\bar{0}`$, $`\tilde{H}`$, $`\hat{x}`$)
  are always a base wrapped by an accent macro, never a base + combining codepoint.
- **A literal dollar sign** is written as inline code — `` `$` `` — so it never opens a
  math span. Docs that discuss `$`/`$$` as LaTeX tokens
  ([`layers/latex/*`](layers/latex/)) or the money symbol
  ([`correction/text-normalization.md`](correction/text-normalization.md)) keep those
  as `` `$` `` code spans. **Never let an ASCII letter abut the opening ``$` ``** — write
  `the $`x`$`, not `the$`x`$`.
- **Unicode is still correct for non-mathematical text:** box-drawing, arrows, and
  geometric glyphs inside diagrams and their `<details>` text-views; enumerations and
  separators in prose; IPA, CJK, and other script samples in language examples. Only
  *mathematics* migrates to LaTeX.

## 2. Define before use

Every symbol, acronym, and key term is defined **before** its first use — in a local
"Terms & symbols" table at the top of the doc, cross-linked to the central
[`NOTATION.md`](NOTATION.md). Acronyms (WFST, WFSA, CTC, RNN-T, PDA, …) are expanded on
first occurrence per document.

## 3. Pedagogical structure

Each topic/module doc follows this flow (the order conveys *what → how → why*):

1. **Thesis** — one sentence: what this is, in plain language.
2. **Terms & symbols** — local definition table (links to `NOTATION.md`).
3. **Formal model** — the defining tuple/equations as ``$`…`$`` spans and `math`
   blocks, each component defined in a table.
4. **Intuition** — a small worked example *before* the heavy theory.
5. **Architecture & API** — key traits/structs and their responsibilities.
6. **Algorithms** — literate pseudocode (§5) where non-trivial.
7. **Examples** — valid Rust (§6).
8. **Diagrams** — at least one structural + one flow diagram (§4).
9. **Relation to the library** — composition points, feature flags.
10. **References** — a `## References` section linking [`BIBLIOGRAPHY.md`](BIBLIOGRAPHY.md).

Reference / feature / status docs (`api/`, `layers/`, `integration/`) follow their own
genre conventions (`## See Also` / `## Related`) and need not force the full ten-part
template — but they still obey §1 (math), §6 (valid code), and §7 (citations).

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
- **Always keep the `<details>` plain-text fallback** so screen readers, plain-text
  viewers, and code review never lose the diagram's information. The art inside it is
  Unicode (§1) and stays verbatim.
- **Math in diagram labels.** PlantUML (`.puml`) and TikZ (`.tex`) sources typeset math
  with LaTeX — `<latex>\oplus</latex>` in PlantUML (JLaTeXMath) and math macros in TikZ
  — rather than Unicode literals, per the pgmcp *diagrams-plantuml-latex* guideline.
  Graphviz (`.dot`) and D2 (`.d2`) have no LaTeX facility; the tool-matrix uses them for
  automaton and dataflow graphs, where Unicode labels (`a:ε/0.5`, `q₀`) are retained and
  render identically in the SVG.

## 5. Algorithms — literate pseudocode

Present non-trivial algorithms in Knuth literate-programming style: a prose paragraph
stating intent and the loop invariant, a named chunk in a ```` ```text ```` fence, then
prose explaining each step and the complexity ``$`O(\lvert V\rvert + \lvert E\rvert)`$``,
then a worked trace. Name chunks `⟨ relax outgoing arcs ⟩` and cross-reference them. The
README's *"The one algorithm behind it"* section is the house template.

- **Pseudocode listings stay in ```` ```text ```` (or ```` ```rust ````) fences** — they
  are *code*, not prose, and a fenced block cannot host a rendered ``$`…`$`` span. Inside
  such a listing the operators stay as their readable Unicode forms (`⊕`, `⊗`, `0̄`, `←`)
  and the chunk names stay `⟨ … ⟩`, verbatim.
- **Every mathematical statement a listing makes must also render somewhere.** State the
  loop invariant, the recurrence, and the complexity in the surrounding prose as ``$`…`$``
  spans or a `math` block, so no reader is forced to parse un-rendered glyphs to get the
  math. (This is the one place Unicode math is retained — bounded to code listings and
  paired with rendered prose.)

## 6. Code snippets

All code snippets must be **valid** — syntactically and semantically. Prefer snippets
lifted from the module's own `#[cfg(test)]` tests or doctests, which are compiler-checked.
Use the real API (`TropicalWeight::new(0.5)`, not `TropicalWeight(0.5)`;
`EdgeMetadata::original()` / `EdgeMetadata::correction(n)`; `viterbi` returning
`ViterbiResult`; `VectorWfstBuilder`). Doc examples meant to compile go through
`cargo test --doc`.

## 7. Citations

Every non-trivial claim, algorithm, or model traces to a citation in
[`BIBLIOGRAPHY.md`](BIBLIOGRAPHY.md), linked by anchor
(`[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)`). Each topic doc ends with a
`## References` section. Prefer DOIs; never fabricate one.

## 8. Placement & naming

- Topic docs live under the section that matches their tier: `architecture/`
  (foundations), `algorithms/`, `advanced/`, `transducers/` (transducer families),
  `correction/` (NLP/correction), `asr/`, `acoustic/`, `training/`, `programming/`,
  `integration/`, `api/`, `optimization/`. Frozen historical records live under
  `archive/`.
- File names are intuitive kebab-case (`weight-pushing.md`, `tree-transducers.md`).
- Every doc is reachable from [`README.md`](README.md) (the documentation index).
