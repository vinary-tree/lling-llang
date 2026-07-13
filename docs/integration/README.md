# Integration

How `lling-llang` connects to the world: the external libraries it builds on, the
sibling repositories it cross-references, and the forward-looking
[F1R3FLY.io](https://f1r3fly.io) platform it is designed to slot into. This page
indexes every integration subsection and documents the **external-repository link
convention** these docs rely on.

This is a section landing page; for the documentation root see the
[top-level docs index](../README.md). For the algebra, algorithms, and transducer
families that the integrations consume, start at the
[architecture overview](../architecture/overview.md).

---

## Subsections

### `external/` — application-level integration

Patterns for embedding `lling-llang` into your own applications and pipelines.

| Doc | What it covers |
|---|---|
| [Library usage patterns](external/library-usage.md) | Adding the dependency, builder pattern, generics over semiring/backend, error handling, configuration, serialization, parallelism, testing, benchmarking, logging. |
| [Speech recognition & NLP](external/speech-nlp.md) | Importing ASR lattices (HTK SLF, OpenFst, Kaldi, Whisper), language-model rescoring, grammar filtering, N-best/consensus extraction, streaming. |
| [Text correction](external/text-correction.md) | Spelling and grammar correction applications: candidate generation, context preservation, real-word error detection, full correction pipeline. |

### `liblevenshtein/` — fuzzy string matching

[liblevenshtein-rust](https://github.com/) supplies the edit-distance machinery
behind the spelling-correction, phonetic-matching, and code-completion layers.

| Doc | What it covers |
|---|---|
| [Overview](liblevenshtein/overview.md) | Architecture of the dictionary → automaton → transducer → fuzzy-collection stack; core concepts; performance characteristics. |
| [Dictionaries](liblevenshtein/dictionaries.md) | `DoubleArrayTrie`, `DynamicDawg`, `SuffixAutomaton` (+ `Char`/`U64` variants); the trait hierarchy; selection guide. |
| [Transducers](liblevenshtein/transducers.md) | The query API; `Standard`/`Transposition`/`MergeAndSplit` algorithms; substitution policies; generalized operations. |
| [Fuzzy collections](liblevenshtein/fuzzy-collections.md) | `FuzzyMap`/`FuzzyMultiMap`; LRU/TTL/LFU/cost-aware eviction wrappers; composition. |
| [lling-llang integration](liblevenshtein/lling-llang-integration.md) | `SpellingCorrectionLayer`, `PhoneticMatchingLayer`, `CodeCompletionLayer`, and the combined correction pipeline. |

### `libgrammstein/` — phonetic rescoring

[libgrammstein](https://github.com/) provides phonetic embeddings and
spelling-to-sound rules consumed by the rescoring layer.

| Doc | What it covers |
|---|---|
| [Phonetic rescoring](libgrammstein/phonetic-rescore.md) | Reranking lattice paths by how words *sound* (Zompist normalization); `PhoneticRescoreLayer`; $`\lambda`$-interpolation; ASR error recovery. |

### `f1r3fly/` — F1R3FLY.io platform (integration targets)

The full-stack vision for running `lling-llang` as the lattice-processing core of
the F1R3FLY.io distributed-computation platform. **These docs are forward-looking:**
they describe the *integration design* and planned APIs, not shipped code. Each
component doc states its current status (Planned / Stub).

| Doc | What it covers |
|---|---|
| [Vision](f1r3fly/vision.md) | The whole-stack integration architecture, feature flags, status table, and phased roadmap. |
| [PathMap backend](f1r3fly/pathmap-backend.md) | `PathMapBackend`: content-addressed distributed storage with structural sharing for lattice vocabularies. |
| [MeTTaIL layer](f1r3fly/mettail-layer.md) | `MeTTaILTypeLayer`: filtering lattice paths by OSLF semantic type constraints. |
| [MORK layer](f1r3fly/mork-layer.md) | `MorkRuleLayer`: declarative logic rules for grammar/semantic filtering and reweighting. |
| [MeTTaTron layer](f1r3fly/mettatron-layer.md) | `MeTTaTronLayer`: compiling MeTTa specifications into optimized lattice transformations. |
| [Rholang layer](f1r3fly/rholang-layer.md) | `RholangLayer`: concurrent, distributed lattice processing via the rho-calculus. |

---

## Reading order

```text
external/library-usage  ─▶  external/{speech-nlp, text-correction}
                                     │
              liblevenshtein/overview ─▶ dictionaries ─▶ transducers
                                     │                       │
                                     ▼                       ▼
              liblevenshtein/fuzzy-collections   liblevenshtein/lling-llang-integration
                                     │
                       libgrammstein/phonetic-rescore
                                     │
                          f1r3fly/vision  ─▶  pathmap-backend
                                     │
                  mettail-layer ─▶ mork-layer ─▶ mettatron-layer ─▶ rholang-layer
```

Newcomers: read [library usage](external/library-usage.md) first, then the
`liblevenshtein/` overview. The `f1r3fly/` subsection is best read top-down from
[the vision](f1r3fly/vision.md).

---

## External-repository link convention

Some integration docs link **out of this repository** into sibling F1R3FLY.io
repositories that are expected to be checked out **beside** `lling-llang` in the
same parent directory:

```text
f1r3fly.io/
├── lling-llang/        ← this repository
├── libgrammstein/      ← phonetic embeddings, spelling-to-sound rules
└── libdictenstein/     ← dictionary-family submodule (liblevenshtein)
```

Such links use a relative path that climbs out of `lling-llang/` with one `../`
per directory level **plus one more** to leave the repository root, then descends
into the sibling repo — for example, from a two-level doc like
`docs/integration/libgrammstein/phonetic-rescore.md`:

```text
../../../libgrammstein/docs/...
└┬─┘└┬─┘└┬─┘
 │   │   └─ leave lling-llang/ → f1r3fly.io/
 │   └───── leave docs/integration/libgrammstein/ … (one `../` per level)
 └───────── … up to the repo root
```

These links are **intentional**, not broken. They resolve only when the sibling
repository is present; in a standalone checkout of `lling-llang` they will 404,
which is expected. The known cross-repository links are:

| Citing doc (in `lling-llang`) | Link target (sibling repo) |
|---|---|
| [`docs/layers/code-correction/pattern-aware.md`](../layers/code-correction/pattern-aware.md) | `../../../libgrammstein/docs/components/subtree/overview.md` |
| [`docs/acoustic/overview.md`](../acoustic/overview.md) | `../../libgrammstein/docs/components/acoustic/models.md` |
| [`docs/integration/libgrammstein/phonetic-rescore.md`](libgrammstein/phonetic-rescore.md) | `../../../libgrammstein/docs/components/embedding/phonetic.md` |
| [`docs/asr/subword-lexicon.md`](../asr/subword-lexicon.md) | `../../libgrammstein/docs/components/embedding/bpe.md` |

The `../` depth differs by the citing file's directory level: top-level-section
docs (`docs/<section>/x.md`, e.g. `acoustic/`, `asr/`) need `../../` to reach the
repo's parent, while two-level docs (`docs/integration/<sub>/x.md`) need `../../../`.
When adding a new cross-repository link, count the levels from the citing file and
add a row to the table above so the link reads as deliberate.

---

## References

Integration docs that cite published work carry their own `## References`
sections linking the central [bibliography](../BIBLIOGRAPHY.md) by anchor — see
[`liblevenshtein/overview.md`](liblevenshtein/overview.md#references) and
[`libgrammstein/phonetic-rescore.md`](libgrammstein/phonetic-rescore.md#references).
The foundational citations behind these integrations are
[Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002) (WFSTs in speech recognition),
[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) (weighted-automata algorithms), and
[Allauzen 2007](../BIBLIOGRAPHY.md#ref-allauzen2007) (the OpenFst library design).
