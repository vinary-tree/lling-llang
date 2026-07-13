# Diagramming Conventions

All `lling-llang` diagrams are authored as **text sources** and rendered to
**committed SVGs** that sit beside their source. This keeps diagrams diff-able,
regenerable in CI, and always in sync with the docs. The tooling is the locally
installed subset of the **pgmcp diagramming catalog** (the `diagramming` domain:
PlantUML, Graphviz, D2, Mermaid, TikZ/PGF, plus SVG converters).

> **Render everything:** `make diagrams` (only changed sources) ·
> `make diagrams-force` (all) · `make diagrams-check` (validate, write nothing).

---

## 1. Tool-per-concept matrix

Pick the **best** tool for each illustration (pgmcp guideline *diagrams-best-types*).
The extension of the source file selects the render engine.

| Concept | Tool | Source ext | Why this tool |
|---|---|---|---|
| WFSA/WFST state graphs, weighted lattices (DAGs) | **Graphviz** | `.dot` | Canonical node-edge automaton; arc labels `in:out/weight`; trivial per-edge recoloring for the best path. |
| CTC topologies, composition products, $`\varepsilon`$-filters | **Graphviz** | `.dot` | Auto-laid-out automata that change with the API; `circo` for all-to-all, `dot` for the rest. |
| HMM / lexicon / n-gram / PDA-stack / tree-rewrite / RNN-T grid | **Graphviz** | `.dot` | All are relationship graphs whose layout should follow the relation. |
| GPU CSR layout / token-recombination dataflow | **Graphviz** | `.dot` | `record` nodes model array layouts; arrows model dataflow. |
| Trait hierarchies (Semiring, Wfst, backend) | **PlantUML** | `.puml` | Default for *semantic* class diagrams kept under version control; matches the repo's established style. |
| Pipelines / dataflow / state machines (correction, ASR, RRWM, TN-ITN, LaTeX/MathML/syntax flows) | **PlantUML** | `.puml` | Activity/state/sequence/component diagrams read cleanly and diff well. |
| Forward/backward autograd message passing | **PlantUML** | `.puml` | A two-pass sequence diagram. |
| Polished high-level system overviews ($`\le 3`$: whole library, liblevenshtein, F1R3FLY) | **D2** | `.d2` | Container model + `elk` layout yields the cleanest big-picture diagrams. |
| Publication-grade math figures (semiring property lattice, $`\eta`$-power, signed-tropical) | **TikZ/PGF** | `.tex` | Native LaTeX math typography ($`\oplus`$/$`\otimes`$/$`\bar{0}`$/$`\bar{1}`$) for paper-quality figures. |

**Mermaid** (`.mmd`) is supported by the pipeline but deliberately unused for new
diagrams: committed SVGs make its inline-preview advantage moot, and avoiding a
fifth toolchain keeps the visual style consistent. **Kroki** (the pgmcp HTTP
gateway) is an optional fallback; the local engines above are the required path.

**Math in diagram labels.** PlantUML (`.puml`) and TikZ (`.tex`) typeset
mathematics with LaTeX — `<latex>\oplus</latex>` via JLaTeXMath in PlantUML, math
macros in TikZ — rather than Unicode literals, per the pgmcp
*diagrams-plantuml-latex* guideline and [`STYLE.md`](../STYLE.md) §4. Graphviz
(`.dot`) and D2 (`.d2`) have no LaTeX facility, so their automaton and dataflow
labels keep the readable Unicode forms (`a:ε/0.5`, `q₀`) — which render
identically in the SVG.

## 2. Color palette — one intuitive color per concept

Reuse this palette across **every** diagram (pgmcp guideline
*diagrams-fully-colored*). It extends the legend established in
[`architecture-map.puml`](architecture-map.puml) and [`lattice.puml`](lattice.puml).
Fills are Material-100/200; borders Material-700/800.

| Tier / concept | Fill | Border |
|---|---|---|
| Foundation — semiring · wfst · lattice · backend | `#BBDEFB` | `#1565C0` |
| Algorithms — path · composition · optimization | `#C8E6C9` | `#2E7D32` |
| Transducer families — cfg · multitape · pushdown · tree · subsequential | `#B2DFDB` | `#00695C` |
| Correction & NLP — layers · error_models · TN/ITN · llm · programming | `#FFF59D` | `#F9A825` |
| Deep learning & GPU — differentiable · gpu · simd | `#E1BEE7` | `#6A1B9A` |
| Speech / ASR — asr · acoustic · ctc · transducer · training | `#FFE0B2` | `#E65100` |
| Verification — Coq/Rocq · TLA⁺ | `#FFCDD2` | `#B71C1C` |
| Neutral / IO / container | `#ECEFF1` | `#455A64` |

**Automata-element accents** (used in every state graph):

| Element | Color / style |
|---|---|
| Best / Viterbi path | `#2E7D32`, bold (`penwidth=2`) |
| Alternative path | `#B0BEC5` (light) / `#607D8B` (default arrow) |
| Epsilon (`ε`) arc | `#9E9E9E`, dashed |
| Final state | double ring (`peripheries=2`), fill `#C8E6C9`, border `#2E7D32` |
| Start state | fill `#BBDEFB`, border `#1565C0`, arrow from an invisible point |
| Proof / verifies edge | `#B71C1C`, dashed |
| Background | `#FFFFFF`, no shadow |

### Shared Graphviz preamble (copy into every `.dot` automaton)

```dot
rankdir=LR; bgcolor="#FFFFFF"; fontname="Helvetica";
node [shape=circle, style=filled, fillcolor="#BBDEFB", color="#1565C0",
      fontcolor="#102027", fontname="Helvetica", penwidth=1.4];
edge [color="#607D8B", fontcolor="#37474F", fontname="Helvetica", fontsize=11, penwidth=1.3];
// final:  [shape=doublecircle, fillcolor="#C8E6C9", color="#2E7D32"]
// best:   [color="#2E7D32", penwidth=2.0]      alt: [color="#B0BEC5"]
// epsilon:[color="#9E9E9E", style=dashed, label="ε"]
// start:  __start__ [shape=point, width=0.01]; __start__ -> q0;
```

PlantUML sources reuse the `skinparam` blocks from the existing `.puml` files;
TikZ sources `\definecolor{found}{HTML}{BBDEFB}` … one per palette row.

## 3. Directory & naming

Sources and their SVGs are co-located under `docs/diagrams/<section>/`, mirroring
the `docs/` tree (`architecture/ algorithms/ asr/ advanced/ acoustic/ layers/
integration/ training/ programming/ transducers/ correction/`). The five
README-global diagrams stay at the top level. Name files
`<concept-kebab>.<ext>` + sibling `<concept-kebab>.svg`. The `.gitignore`
whitelists `docs/diagrams/**/*.svg`.

## 4. Embedding in docs

```markdown
![<descriptive alt text>](../diagrams/<section>/<name>.svg)

*<one-line color legend>.*

<details><summary>Text view</summary>

```text
<original ASCII/Unicode art, kept verbatim as the accessible fallback>
```

</details>
```

Relative prefix = one `../` per directory level below `docs/`. Always keep the
`<details>` fallback.

## 5. Contributor workflow

1. Add or edit a `.dot` / `.puml` / `.d2` / `.tex` source under
   `docs/diagrams/<section>/`.
2. Run `make diagrams` (renders only changed sources) — or `make diagrams-force`
   after a palette change.
3. Commit **both** the source and its sibling `.svg`.
4. CI runs `make diagrams-check` (validate) then `make diagrams` and fails if any
   tracked `.svg` differs from a fresh render.

---

## 6. Diagram catalog

Every rendered diagram, its engine, and the doc(s) that embed it. (Extended as
diagrams are added.)

| Diagram | Engine | Embedded in |
|---|---|---|
| `architecture-map.svg` | PlantUML | `README.md`, `ARCHITECTURE.md` |
| `lattice.svg` | PlantUML | `README.md` |
| `asr-cascade.svg` | PlantUML | `README.md`, `advanced/asr-pipeline.md` |
| `correction-pipeline.svg` | PlantUML | `README.md`, `architecture/layers.md` |
| `formal-verification.svg` | PlantUML | `README.md`, `proofs/README.md` |
| `architecture/library-overview.svg` | D2 | `architecture/overview.md` |
| `architecture/semiring-traits.svg` | PlantUML | `architecture/semirings.md`, `architecture/wfst-traits.md` |
| `architecture/semiring-hasse.svg` | TikZ | `architecture/semirings.md` |
| `architecture/wfst-traits.svg` | PlantUML | `architecture/wfst-traits.md` |
| `architecture/lattice-worked.svg` | Graphviz | `architecture/overview.md`, `architecture/lattices.md` |
| `algorithms/*` · `asr/*` · `advanced/*` · `transducers/*` · `correction/*` | Graphviz / PlantUML / D2 / TikZ | the corresponding topic docs |
