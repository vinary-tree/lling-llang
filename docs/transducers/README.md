# Transducer Families

This section documents the **transducer families** in `lling-llang` — automata
that go beyond the single-input/single-output [WFST](../architecture/wfst-traits.md)
core by adding tapes, a stack, tree structure, determinism guarantees, or a
neural alignment lattice. Each family targets a different language class and a
different application, but all share the library's semiring-weighted foundation
(`⊕` over alternatives, `⊗` along a path; identities `0̄` and `1̄`).

The family palette throughout these docs is teal
(`#B2DFDB` fill / `#00695C` border) for the finite-state transducer families and
orange (`#FFE0B2` / `#E65100`) for the neural (speech/ASR) family, matching the
[diagram conventions](../diagrams/README.md).

---

## Terms & symbols

Acronyms are expanded on first use and defined centrally in
[`NOTATION.md`](../NOTATION.md): **WFST** (Weighted Finite-State Transducer),
**CFG/CFL** (Context-Free Grammar / Language), **PDA** (Pushdown Automaton),
**WTT** (Weighted Tree Transducer), **RNN-T** (Recurrent Neural-network
Transducer), **ASR** (Automatic Speech Recognition), **DAG** (Directed Acyclic
Graph).

| Term | Meaning |
|---|---|
| **tape** | One input/output stream an automaton reads or writes in lock-step. |
| **determinizable** | Whether the family admits an equivalent deterministic form (key for fast, unambiguous application). |
| **language class** | The formal-language family the model recognizes/relates (regular ⊂ context-free ⊂ …). |
| **memory model** | The auxiliary memory beyond the finite control: none, a stack, the tree itself, or a neural label-history state. |

---

## The five families at a glance

| Family | Tapes | Memory model | Language class | Determinizable? | Primary use | Primary citation |
|---|---|---|---|---|---|---|
| **cfg** — [Earley parsing](../algorithms/parsing.md) | input lattice → parse forest | Earley chart (item sets) | context-free | n/a (chart parser, not a single automaton) | parse a lattice against a grammar; filter to grammatical paths | [Earley 1970](../BIBLIOGRAPHY.md#ref-earley1970) |
| **multitape** — [multitape.md](multitape.md) | `N ≥ 1` (const generic) | none (finite control) | `N`-ary rational relations | partially (per-tape; via [synchronization](../algorithms/synchronization.md)) | word alignment, morphology, multi-stream relations | [Mohri 1997](../BIBLIOGRAPHY.md#ref-mohri1997) |
| **pushdown** — [pushdown.md](pushdown.md) | 1 input | a **stack** (`Γ*`) | context-free | not in general (CFLs aren't closed under it) | nested-structure recognition (`aⁿbⁿ`, brackets, palindromes) | [Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) |
| **tree_transducers** — [tree-transducers.md](tree-transducers.md) | input tree → output tree | the tree (recursion) | (weighted) tree transformations | depends on rule class (linear/non-copying) | syntax-directed translation, AST/parse-tree rewriting | [Fülöp & Vogler 2009](../BIBLIOGRAPHY.md#ref-fulop2009) |
| **subsequential** — [../advanced/subsequential-transducers.md](../advanced/subsequential-transducers.md) | 2 (input/output) | none (deterministic control) | subsequential functions (∪ of pieces) | **yes, by construction** (input-deterministic) | fast, backtrack-free application of (decomposed) functions | [Mohri 2000](../BIBLIOGRAPHY.md#ref-mohri2000) |
| **neural-transducer** — [neural-transducer.md](neural-transducer.md) | acoustic `x` → labels `y` | neural label-history state + `T×U` lattice | learned alignment distribution | n/a (probabilistic, marginalized) | streaming ASR (RNN-T) | [Graves 2012](../BIBLIOGRAPHY.md#ref-graves2012) |

**How to read "determinizable?"** Determinism makes application linear-time and
unambiguous. Finite-state transducers are sometimes determinizable; pushdown
automata are *not* in general (deterministic PDAs recognize only the strict subset
DCFLs); subsequential transducers are deterministic by definition, which is
exactly their value; and the neural transducer is probabilistic — there is nothing
to determinize, only an alignment distribution to marginalize.

---

## Choosing a family

- **Relate two or more aligned streams** (surface ↔ lemma ↔ tag, source ↔ target ↔
  alignment) → **[multitape](multitape.md)**.
- **Recognize nested or balanced structure** that a finite-state machine cannot
  count → **[pushdown](pushdown.md)**; or parse a lattice against a full grammar →
  **[cfg / Earley](../algorithms/parsing.md)**.
- **Rewrite trees into trees** (reorder/copy/delete/relabel children) for
  syntax-directed translation or AST transformation →
  **[tree_transducers](tree-transducers.md)**.
- **Apply a (possibly non-subsequential) function fast and backtrack-free** by
  decomposing it into deterministic pieces →
  **[subsequential](../advanced/subsequential-transducers.md)**.
- **Transduce speech to text in a streaming, end-to-end, differentiable model** →
  **[neural-transducer (RNN-T)](neural-transducer.md)**.

---

## Documents in this section

| Document | Module | What it covers |
|---|---|---|
| [Multi-Tape Transducers](multitape.md) | [`src/multitape/`](../../src/multitape/) | `MultiTapeWfst`, `VectorMultiTapeWfst`, builder, projection, synchronization (`TapeDelay`). |
| [Weighted Pushdown Automata](pushdown.md) | [`src/pushdown/`](../../src/pushdown/) | `WeightedPda`, `VectorPda`, `StackSymbol`/`StackAction`, `PdaConfiguration`, accept modes. |
| [Weighted Tree Transducers](tree-transducers.md) | [`src/tree_transducers/`](../../src/tree_transducers/) | `WeightedTreeTransducer`, `RankedAlphabet`, `Tree`, `TreeRule`/`TreePattern`. |
| [Neural Transducer (RNN-T)](neural-transducer.md) | [`src/transducer/`](../../src/transducer/) | `NeuralTransducer`, encoder/predictor/joiner, `T×U` lattice, loss/decoding. |

The **subsequential** family is documented one tier up, alongside the other
advanced finite-state topics, at
[../advanced/subsequential-transducers.md](../advanced/subsequential-transducers.md);
the **cfg** family's parser is documented under algorithms at
[../algorithms/parsing.md](../algorithms/parsing.md). Both are included in the
[comparison table](#the-five-families-at-a-glance) above so the families can be
weighed side by side.

---

## Relation to the library

All finite-state families project or approximate back to the single-tape
[WFST](../architecture/wfst-traits.md) core, where the shared algorithm suite
applies — [composition](../algorithms/composition.md),
[determinization](../algorithms/determinization.md),
[shortest distance](../algorithms/shortest-distance.md),
[path extraction](../algorithms/path-extraction.md):

- multi-tape WFSTs via [`project`](multitape.md#projection);
- pushdown automata via [`approximate_fst`](pushdown.md#weighted-aggregation-and-fst-approximation)
  (bounded stack depth);
- the neural transducer via [`to_wfst`](neural-transducer.md#lattice-construction-and-to_wfst).

Every family is generic over the weight [semiring](../architecture/semirings.md)
`W`; the worked examples use `TropicalWeight`. None requires a feature flag — all
five modules (`cfg`, `multitape`, `pushdown`, `tree_transducers`, `subsequential`)
plus `transducer` are unconditionally compiled in [`src/lib.rs`](../../src/lib.rs).

---

## References

- <a id="cite-earley1970"></a>[Earley 1970](../BIBLIOGRAPHY.md#ref-earley1970) —
  Earley, J. (1970). *An Efficient Context-Free Parsing Algorithm.* CACM
  13(2):94–102. The chart parser underlying the cfg family.
- <a id="cite-mohri1997"></a>[Mohri 1997](../BIBLIOGRAPHY.md#ref-mohri1997) —
  Mohri, M. (1997). *Finite-State Transducers in Language and Speech Processing.*
  Computational Linguistics 23(2):269–311. Foundation for the multi-tape family.
- <a id="cite-mohri2009"></a>[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) —
  Mohri, M. (2009). *Weighted Automata Algorithms.* In *Handbook of Weighted
  Automata*, pp. 213–254. Springer. Weighted pushdown systems and the shared
  finite-state algorithms.
- <a id="cite-mohri2000"></a>[Mohri 2000](../BIBLIOGRAPHY.md#ref-mohri2000) —
  Mohri, M. (2000). *Minimization Algorithms for Sequential Transducers.* TCS
  234(1–2):177–201. The subsequential-transducer theory.
- <a id="cite-fulop2009"></a>[Fülöp & Vogler 2009](../BIBLIOGRAPHY.md#ref-fulop2009) —
  Fülöp, Z., & Vogler, H. (2009). *Weighted Tree Automata and Tree Transducers.*
  In *Handbook of Weighted Automata*, pp. 313–403. Springer. The tree-transducer
  family.
- <a id="cite-graves2012"></a>[Graves 2012](../BIBLIOGRAPHY.md#ref-graves2012) —
  Graves, A. (2012). *Sequence Transduction with Recurrent Neural Networks.*
  arXiv:1211.3711. The neural (RNN-T) family.
