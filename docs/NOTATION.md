# Notation & Glossary

The canonical reference for every symbol, operator, and acronym used across the
`lling-llang` documentation. Each is defined here once; topic docs link back to
this file and repeat only the terms they use locally (see
[`STYLE.md`](STYLE.md)). All mathematics is written in Unicode and wrapped in
backticks per the style guide.

---

## Algebraic symbols (semirings & weights)

| Symbol | Name | Meaning |
|---|---|---|
| `K` | carrier set | The set of weights of a semiring (e.g. `ℝ ∪ {∞}` for Tropical). |
| `⊕` | semiring *plus* | Combines **alternative** paths/derivations. Associative, commutative, identity `0̄`. (Tropical: `min`; Log: `−ln(e⁻ˣ+e⁻ʸ)`; Probability: `+`.) |
| `⊗` | semiring *times* | Combines **sequential** steps along one path. Associative, identity `1̄`, distributes over `⊕`. (Tropical: `+`; Probability: `×`.) |
| `0̄` | additive identity | The `⊕`-identity — “no path”/unreachable. (Tropical: `∞`; Probability: `0`.) |
| `1̄` | multiplicative identity | The `⊗`-identity — “empty path”/zero cost. (Tropical: `0`; Probability: `1`.) |
| `⊕ₗₒg` | log-add | `x ⊕ₗₒg y = −ln(e⁻ˣ + e⁻ʸ)`, the Log-semiring `⊕`. |
| `a*` | star/closure | `a* = 1̄ ⊕ a ⊕ (a⊗a) ⊕ …`, the Kleene closure of a weight (when it converges). |
| `η` | power exponent | The exponent of the `η`-power semiring `S_η`, controlling soft path selection / online-learning temperature. |
| `∞` | infinity | The Tropical/Log `0̄` (unreachable / infinite cost). |

## Automata & transducer symbols

| Symbol | Name | Meaning |
|---|---|---|
| `Q` | state set | The (finite) set of automaton states. |
| `q₀` | start state | The initial state. |
| `F` | final states | The set of accepting states (often with final weights via `ρ`). |
| `Σ` | input alphabet | Input labels; `Σ₁,…,Σₖ` for the `k` tapes of a multitape transducer. |
| `Γ` | stack alphabet | The stack symbols of a pushdown automaton (PDA). |
| `Δ` / `E` | transition relation | Weighted transitions. `E` for WFST arcs, `Δ` for PDA/tree-transducer rules. |
| `ρ` | final-weight function | `ρ : F → K`, the weight contributed by ending in a final state. |
| `δ` | output symbol / transition | An output label, or a transition function depending on context (defined locally). |
| `ε` | epsilon | The empty label — a transition that consumes/emits nothing. |
| `∘` | composition | `A ∘ B` chains transducers: the output tape of `A` feeds the input tape of `B`. |
| `π` | projection | Keep one tape of a transducer (`π₁`/`π₂`), or the recognition-network result in `N = π(min(det(H ∘ C ∘ L ∘ G)))`. |
| `∣·∣` | cardinality | `∣Q∣`, `∣E∣`, `∣V∣` — sizes used in complexity bounds. Uses U+2223, not ASCII `|`. |
| `λ` | interpolation weight | The back-off mixing coefficient in `P(w∣h) = λ·P̂(w∣h) + (1−λ)·P(w∣h′)`. |

## Set & logic symbols

| Symbol | Meaning |
|---|---|
| `⊆`, `⊂` | subset, proper subset |
| `∪`, `∩` | union, intersection |
| `∈`, `∉` | membership, non-membership |
| `∀`, `∃` | universal, existential quantifiers |
| `⟨ … ⟩` | tuple, or a named literate-pseudocode chunk |

## Acronyms

| Acronym | Expansion | Introduced in |
|---|---|---|
| **WFST** | Weighted Finite-State Transducer (input + output label + weight per arc) | [architecture/wfst-traits.md](architecture/wfst-traits.md) |
| **WFSA** | Weighted Finite-State Acceptor (a WFST with `input = output`) | [architecture/lattices.md](architecture/lattices.md) |
| **DAG** | Directed Acyclic Graph | [architecture/lattices.md](architecture/lattices.md) |
| **Lattice** | A weighted DAG whose start→end paths enumerate hypotheses | [architecture/lattices.md](architecture/lattices.md) |
| **CTC** | Connectionist Temporal Classification (alignment-free sequence labeling) | [advanced/ctc-topologies.md](advanced/ctc-topologies.md) |
| **RNN-T** | Recurrent Neural-network Transducer (streaming encoder–predictor–joiner) | [transducers/neural-transducer.md](transducers/neural-transducer.md) |
| **LF-MMI** | Lattice-Free Maximum Mutual Information (sequence-discriminative training) | [training/weak-supervision.md](training/weak-supervision.md) |
| **PDA** | Pushdown Automaton (finite automaton with a stack; recognizes CFLs) | [transducers/pushdown.md](transducers/pushdown.md) |
| **CFG / CFL** | Context-Free Grammar / Language | [algorithms/parsing.md](algorithms/parsing.md) |
| **TN / ITN** | Text Normalization / Inverse TN (“$5” ⇄ “five dollars”) | [correction/text-normalization.md](correction/text-normalization.md) |
| **CSR** | Compressed Sparse Row (sparse-matrix layout for GPU WFSTs) | [advanced/gpu-acceleration.md](advanced/gpu-acceleration.md) |
| **RRWM** | Rational Randomized Weighted-Majority (online ensemble learning) | [algorithms/rrwm.md](algorithms/rrwm.md) |
| **GCD** | Grammar-Constrained Decoding | [advanced/constrained-decoding.md](advanced/constrained-decoding.md) |
| **GTN** | Graph Transformer Networks (differentiable WFSTs) | [advanced/differentiable.md](advanced/differentiable.md) |
| **BPE** | Byte-Pair Encoding (subword tokenization) | [asr/subword-lexicon.md](asr/subword-lexicon.md) |
| **HMM** | Hidden Markov Model | [acoustic/overview.md](acoustic/overview.md) |
| **MMI** | Maximum Mutual Information | [training/weak-supervision.md](training/weak-supervision.md) |
| **ID** | Instantaneous Description (a PDA configuration `(q, w, γ)`) | [transducers/pushdown.md](transducers/pushdown.md) |
| **ASR** | Automatic Speech Recognition | [asr/cascade-construction.md](asr/cascade-construction.md) |
| **H, C, L, G** | ASR cascade stages: **H**MM · **C**ontext-dependency · **L**exicon · **G**rammar/LM | [asr/cascade-construction.md](asr/cascade-construction.md) |

## The ASR cascade in one line

`N = π(min(det(H ∘ C ∘ L ∘ G)))` — compose the **H**MM, **C**ontext-dependency,
**L**exicon, and **G**rammar transducers; determinize and minimize for efficiency;
project to the recognition network `N`. See
[asr/cascade-construction.md](asr/cascade-construction.md).
