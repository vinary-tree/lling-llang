# Notation & Glossary

The canonical reference for every symbol, operator, and acronym used across the
`lling-llang` documentation. Each is defined here once; topic docs link back to
this file and repeat only the terms they use locally (see [`STYLE.md`](STYLE.md)).

All mathematics is written as **MathJax LaTeX** delimited for GitHub-flavored
Markdown — inline math as a backtick span wrapped in dollar signs (``$`\oplus`$``)
and display math in a fenced block whose info-string is `math`. The **LaTeX**
column below is therefore also the canonical *Unicode → LaTeX* map: to typeset a
symbol, copy the command from that column into a ``$`…`$`` span. (See
[`STYLE.md`](STYLE.md) §1 for the delimiter rules and the reasons behind them.)

---

## Algebraic symbols (semirings & weights)

| Symbol | LaTeX | Name | Meaning |
|---|---|---|---|
| $`K`$ | `K` | carrier set | The set of weights of a semiring (e.g. $`\mathbb{R} \cup \{\infty\}`$ for Tropical). |
| $`\oplus`$ | `\oplus` | semiring *plus* | Combines **alternative** paths/derivations. Associative, commutative, identity $`\bar{0}`$. (Tropical: $`\min`$; Log: $`-\ln(e^{-x}+e^{-y})`$; Probability: $`+`$.) |
| $`\otimes`$ | `\otimes` | semiring *times* | Combines **sequential** steps along one path. Associative, identity $`\bar{1}`$, distributes over $`\oplus`$. (Tropical: $`+`$; Probability: $`\times`$.) |
| $`\bar{0}`$ | `\bar{0}` | additive identity | The $`\oplus`$-identity — "no path"/unreachable. (Tropical: $`\infty`$; Probability: $`0`$.) |
| $`\bar{1}`$ | `\bar{1}` | multiplicative identity | The $`\otimes`$-identity — "empty path"/zero cost. (Tropical: $`0`$; Probability: $`1`$.) |
| $`\oplus_{\log}`$ | `\oplus_{\log}` | log-add | $`x \oplus_{\log} y = -\ln(e^{-x} + e^{-y})`$, the Log-semiring $`\oplus`$. |
| $`a^*`$ | `a^*` | star/closure | $`a^* = \bar{1} \oplus a \oplus (a \otimes a) \oplus \cdots`$, the Kleene closure of a weight (when it converges). |
| $`\eta`$ | `\eta` | power exponent | The exponent of the $`\eta`$-power semiring $`S_\eta`$, controlling soft path selection / online-learning temperature. |
| $`\infty`$ | `\infty` | infinity | The Tropical/Log $`\bar{0}`$ (unreachable / infinite cost). |

## Automata & transducer symbols

| Symbol | LaTeX | Name | Meaning |
|---|---|---|---|
| $`Q`$ | `Q` | state set | The (finite) set of automaton states. |
| $`q_0`$ | `q_0` | start state | The initial state. |
| $`F`$ | `F` | final states | The set of accepting states (often with final weights via $`\rho`$). |
| $`\Sigma`$ | `\Sigma` | input alphabet | Input labels; $`\Sigma_1, \dots, \Sigma_k`$ for the $`k`$ tapes of a multitape transducer. |
| $`\Gamma`$ | `\Gamma` | stack alphabet | The stack symbols of a pushdown automaton (PDA). |
| $`\Delta`$ / $`E`$ | `\Delta` / `E` | transition relation | Weighted transitions. $`E`$ for WFST arcs, $`\Delta`$ for PDA/tree-transducer rules. |
| $`\rho`$ | `\rho` | final-weight function | $`\rho : F \to K`$, the weight contributed by ending in a final state. |
| $`\delta`$ | `\delta` | output symbol / transition | An output label, or a transition function depending on context (defined locally). |
| $`\varepsilon`$ | `\varepsilon` | epsilon | The empty label — a transition that consumes/emits nothing. |
| $`\circ`$ | `\circ` | composition | $`A \circ B`$ chains transducers: the output tape of $`A`$ feeds the input tape of $`B`$. |
| $`\pi`$ | `\pi` | projection | Keep one tape of a transducer ($`\pi_1`$/$`\pi_2`$), or the recognition-network result in the cascade below. |
| $`\lvert\cdot\rvert`$ | `\lvert\cdot\rvert` | cardinality | $`\lvert Q\rvert`$, $`\lvert E\rvert`$, $`\lvert V\rvert`$ — sizes used in complexity bounds. |
| $`\lambda`$ | `\lambda` | interpolation weight | The back-off mixing coefficient in $`P(w \mid h) = \lambda \cdot \hat{P}(w \mid h) + (1 - \lambda) \cdot P(w \mid h')`$. |

## Set & logic symbols

| Symbol | LaTeX | Meaning |
|---|---|---|
| $`\subseteq`$, $`\subset`$ | `\subseteq`, `\subset` | subset, proper subset |
| $`\cup`$, $`\cap`$ | `\cup`, `\cap` | union, intersection |
| $`\in`$, $`\notin`$ | `\in`, `\notin` | membership, non-membership |
| $`\forall`$, $`\exists`$ | `\forall`, `\exists` | universal, existential quantifiers |
| $`\langle \dots \rangle`$ | `\langle \dots \rangle` | tuple. **Note:** in a literate-pseudocode fence the same angle-bracket form names a chunk (`⟨ relax outgoing arcs ⟩`) and stays verbatim Unicode inside the ` ```text ` block — it is not math there. |
| $`\mid`$ | `\mid` | "given" / conditional bar, as in $`P(a \mid b)`$ (distinct from cardinality $`\lvert\cdot\rvert`$). |

> **Combining accents.** The semiring identities are $`\bar{0}`$ (`\bar{0}`) and
> $`\bar{1}`$ (`\bar{1}`); the ASR cascade uses $`\tilde{H}, \tilde{C}, \tilde{L}`$
> (`\tilde{H}` …) for encoded stages and $`\hat{x}, \hat{y}`$ (`\hat{x}` …) for
> estimates. Always typeset these as a base wrapped by an accent macro, never as a
> base character followed by a combining codepoint.

## Acronyms

| Acronym | Expansion | Introduced in |
|---|---|---|
| **WFST** | Weighted Finite-State Transducer (input + output label + weight per arc) | [architecture/wfst-traits.md](architecture/wfst-traits.md) |
| **WFSA** | Weighted Finite-State Acceptor (a WFST whose input and output labels coincide) | [architecture/lattices.md](architecture/lattices.md) |
| **DAG** | Directed Acyclic Graph | [architecture/lattices.md](architecture/lattices.md) |
| **Lattice** | A weighted DAG whose start→end paths enumerate hypotheses | [architecture/lattices.md](architecture/lattices.md) |
| **CTC** | Connectionist Temporal Classification (alignment-free sequence labeling) | [advanced/ctc-topologies.md](advanced/ctc-topologies.md) |
| **RNN-T** | Recurrent Neural-network Transducer (streaming encoder–predictor–joiner) | [transducers/neural-transducer.md](transducers/neural-transducer.md) |
| **LF-MMI** | Lattice-Free Maximum Mutual Information (sequence-discriminative training) | [training/weak-supervision.md](training/weak-supervision.md) |
| **PDA** | Pushdown Automaton (finite automaton with a stack; recognizes CFLs) | [transducers/pushdown.md](transducers/pushdown.md) |
| **CFG / CFL** | Context-Free Grammar / Language | [algorithms/parsing.md](algorithms/parsing.md) |
| **TN / ITN** | Text Normalization / Inverse TN (e.g. "five dollars" $`\rightleftarrows`$ `` `$5` ``) | [correction/text-normalization.md](correction/text-normalization.md) |
| **CSR** | Compressed Sparse Row (sparse-matrix layout for GPU WFSTs) | [advanced/gpu-acceleration.md](advanced/gpu-acceleration.md) |
| **RRWM** | Rational Randomized Weighted-Majority (online ensemble learning) | [algorithms/rrwm.md](algorithms/rrwm.md) |
| **GCD** | Grammar-Constrained Decoding | [advanced/constrained-decoding.md](advanced/constrained-decoding.md) |
| **GTN** | Graph Transformer Networks (differentiable WFSTs) | [advanced/differentiable.md](advanced/differentiable.md) |
| **BPE** | Byte-Pair Encoding (subword tokenization) | [asr/subword-lexicon.md](asr/subword-lexicon.md) |
| **HMM** | Hidden Markov Model | [acoustic/overview.md](acoustic/overview.md) |
| **MMI** | Maximum Mutual Information | [training/weak-supervision.md](training/weak-supervision.md) |
| **ID** | Instantaneous Description (a PDA configuration $`(q, w, \gamma)`$) | [transducers/pushdown.md](transducers/pushdown.md) |
| **ASR** | Automatic Speech Recognition | [asr/cascade-construction.md](asr/cascade-construction.md) |
| **H, C, L, G** | ASR cascade stages: **H**MM · **C**ontext-dependency · **L**exicon · **G**rammar/LM | [asr/cascade-construction.md](asr/cascade-construction.md) |

## The ASR cascade in one line

Compose the **H**MM, **C**ontext-dependency, **L**exicon, and **G**rammar
transducers; determinize and minimize for efficiency; project to the recognition
network $`N`$:

```math
N = \pi\bigl(\min\bigl(\det(H \circ C \circ L \circ G)\bigr)\bigr)
```

See [asr/cascade-construction.md](asr/cascade-construction.md).
