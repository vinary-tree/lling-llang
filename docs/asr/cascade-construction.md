# ASR Cascade Construction

The ASR cascade is a pre-compiled recognition network that enables real-time speech recognition by combining all knowledge sources (acoustic model, pronunciation dictionary, language model) into a single weighted finite-state transducer.

## The Recognition Network

The full ASR cascade follows Mohri et al.'s formulation:

```
N = π(min(det(H ∘ det(C ∘ det(L ∘ G)))))
```

Where:
- **G** = Grammar (word-level language model)
- **L** = Lexicon (pronunciation dictionary)
- **C** = Context-dependency transducer (triphone mapping)
- **H** = HMM transducer (hidden Markov model structure)
- **det** = Determinization (powerset construction)
- **min** = Minimization (state reduction)
- **π** = Projection/erasing (remove auxiliary symbols)

### Why Pre-Compilation?

Pre-compiling the cascade offers significant advantages:

1. **Real-time decoding**: Single FST traversal vs. on-the-fly composition
2. **Optimized graph**: Determinization and minimization reduce redundancy
3. **Memory efficiency**: Shared structure across recognition sessions
4. **Predictable latency**: No runtime composition overhead

## Pipeline Components

### Grammar (G): Word-Level Language Model

The grammar assigns probabilities to word sequences, typically from n-gram language models:

```
G: WordId → WordId (identity transducer with LM weights)

Structure:
  ┌────────────────────────────────────────┐
  │  Unigram state with backoff arcs       │
  │       │                                │
  │       ▼                                │
  │  ┌─────────┐    word/word    ┌───────┐ │
  │  │ History │ ─────────────►  │ Next  │ │
  │  │  State  │    (LM weight)  │ State │ │
  │  └─────────┘                 └───────┘ │
  │       │                          │     │
  │       └────── backoff ───────────┘     │
  └────────────────────────────────────────┘
```

The grammar FST encodes:
- N-gram probabilities as arc weights
- Backoff structure for unseen n-grams
- Start/end sentence markers

### Lexicon (L̃): Pronunciation Dictionary

The lexicon maps phone sequences to words:

```
L: PhoneId (input) → WordId (output)

Entry: "cat" → [k, æ, t] with weight w

FST Structure:
              word_id
  (start) ───k/cat───► (s1) ───æ/ε───► (s2) ───t/ε───► (start)
     │                                                    ▲
     │                                                    │
     └────────────────────────────────────────────────────┘
                    (return for next word)
```

Key features:
- **First arc**: Emits word ID on output
- **Subsequent arcs**: Epsilon output (word already emitted)
- **Returns to start**: Allows continuous word sequences
- **Multiple pronunciations**: Homophones have parallel paths

```rust
pub struct LexiconEntry<W: Semiring> {
    /// Word identifier
    pub word: WordId,

    /// Pronunciation as phone sequence
    pub phones: Vec<PhoneId>,

    /// Pronunciation probability (for variants)
    pub weight: W,

    /// Auxiliary symbols for disambiguation
    pub auxiliaries: Vec<AuxiliarySymbol>,
}
```

### Context-Dependency (C̃): Triphone Mapping

The context-dependency transducer maps context-independent phones to context-dependent ones:

```
C: PhoneId (context-dependent) → PhoneId (context-independent)

Example: Triphone "a-n+b" → monophone "n"

Structure encodes context windows:
  Left context × Phone × Right context → Phone

  ┌─────┐   p_cd/p_ci   ┌─────┐
  │ ctx │ ────────────► │ ctx'│
  └─────┘               └─────┘
```

Context-dependent phone IDs typically encode:
- Center phone identity
- Left context (preceding phone or word boundary)
- Right context (following phone or word boundary)

### HMM Transducer (H̃): Hidden Markov Model

The HMM transducer models sub-phonetic states:

```
H: HMM-state-Id (input) → PhoneId (output)

Standard 3-state HMM per phone:
           ┌───┐     ┌───┐     ┌───┐
  phone ─► │ 1 │ ──► │ 2 │ ──► │ 3 │ ──► next
           └─┬─┘     └─┬─┘     └─┬─┘
             │         │         │
             └─────────┴─────────┘
              (self-loops for duration modeling)
```

Each state has:
- Self-loop for extended duration
- Forward transition to next state
- Output label on first state only

### Erasing (π): Auxiliary Symbol Removal

Auxiliary symbols (disambiguation, word boundaries) are removed in the final step:

```rust
pub enum AuxiliarySymbol {
    /// Word boundary marker (#)
    WordBoundary,

    /// Disambiguation symbol (#0, #1, ...)
    Disambiguation(u32),

    /// Epsilon (for erasing)
    Epsilon,
}
```

## L ∘ G Composition

The key insight for efficient cascade construction is proper L ∘ G composition.

### Label Flow

For composition to work, output labels of the first FST must match input labels of the second:

```
Lexicon L:
  Input:  PhoneId (acoustic phones)
  Output: WordId  (word tokens)

Grammar G:
  Input:  WordId  (word tokens)
  Output: WordId  (word tokens, or could be different)

Composition L ∘ G:
  L.output (WordId) matches G.input (WordId) ✓
  Result: Input=PhoneId, Output=WordId
```

Visually:

```
┌─────────────────────────────────────────────────────────────────┐
│                        L ∘ G Composition                         │
│                                                                  │
│   Lexicon (L)              Match              Grammar (G)        │
│   ───────────              ─────              ───────────        │
│                                                                  │
│   phone ──► word_id   ═══════════════►   word_id ──► word_id    │
│   (input)   (output)       on WordId      (input)    (output)   │
│                                                                  │
│   Result (L ∘ G):                                                │
│   phone ─────────────────────────────────────────────► word_id  │
│   (input)                                               (output) │
└─────────────────────────────────────────────────────────────────┘
```

### Implementation

The cascade builder creates a lexicon with WordId outputs for proper composition:

```rust
/// Build lexicon FST with WordId on output for L∘G composition
fn build_lexicon_for_composition(&self) -> VectorWfst<u32, W> {
    let mut fst = VectorWfst::new();
    let start = fst.add_state();
    fst.set_start(start);
    fst.set_final(start, W::one());

    for entry in &self.lexicon {
        let mut current = start;

        // First phone: emit word ID on output
        let next = fst.add_state();
        fst.add_arc(
            current,
            Some(entry.phones[0] as u32),  // Input: phone
            Some(entry.word as u32),        // Output: word ID
            next,
            entry.weight.clone(),
        );
        current = next;

        // Middle phones: epsilon output
        for &phone in &entry.phones[1..entry.phones.len()-1] {
            let next = fst.add_state();
            fst.add_arc(current, Some(phone as u32), None, next, W::one());
            current = next;
        }

        // Last phone: return to start
        if entry.phones.len() > 1 {
            let last_phone = entry.phones[entry.phones.len() - 1];
            fst.add_arc(current, Some(last_phone as u32), None, start, W::one());
        }
    }

    fst
}
```

## API Reference

### CascadeBuilder

```rust
pub struct CascadeBuilder<W: Semiring> {
    config: CascadeConfig,
    grammar: Option<VectorWfst<WordId, W>>,
    lexicon: Vec<LexiconEntry<W>>,
    context: Option<VectorWfst<PhoneId, W>>,
    hmm: Option<VectorWfst<PhoneId, W>>,
}

impl<W: Semiring> CascadeBuilder<W> {
    /// Create a new builder
    pub fn new() -> Self;

    /// Set configuration
    pub fn config(self, config: CascadeConfig) -> Self;

    /// Set grammar (language model) FST
    pub fn grammar(self, g: VectorWfst<WordId, W>) -> Self;

    /// Add a lexicon entry
    pub fn add_lexicon_entry(&mut self, entry: LexiconEntry<W>);

    /// Set context-dependency transducer
    pub fn context_dependency(self, c: VectorWfst<PhoneId, W>) -> Self;

    /// Set HMM transducer
    pub fn hmm(self, h: VectorWfst<PhoneId, W>) -> Self;

    /// Build cascade (basic version)
    pub fn build(self) -> AsrCascade<W>;

    /// Build cascade with optimization (requires DivisibleSemiring)
    pub fn build_optimized(self) -> AsrCascade<W>
    where
        W: DivisibleSemiring + TotallyOrderedSemiring;
}
```

### Configuration

```rust
pub struct CascadeConfig {
    /// Apply determinization after each composition (default: true)
    pub incremental_det: bool,

    /// Minimize the final result (default: true)
    pub minimize: bool,

    /// Use lazy composition (default: false)
    pub lazy: bool,

    /// Maximum pronunciations per word (default: 10)
    pub max_homophony: usize,

    /// Add word boundary markers (default: true)
    pub word_boundaries: bool,
}
```

### Build Strategies

**`build()`**: Basic composition without optimization

```rust
let cascade = CascadeBuilder::new()
    .grammar(lm_fst)
    .add_lexicon_entries(&lexicon)
    .build();

// Suitable for:
// - Development and debugging
// - Small vocabularies
// - When optimization overhead exceeds benefit
```

**`build_optimized()`**: Incremental optimization

```rust
let cascade = CascadeBuilder::new()
    .config(CascadeConfig {
        incremental_det: true,
        minimize: true,
        ..Default::default()
    })
    .grammar(lm_fst)
    .add_lexicon_entries(&lexicon)
    .context_dependency(triphone_fst)
    .hmm(hmm_fst)
    .build_optimized();

// Suitable for:
// - Production deployment
// - Large vocabularies (10K+ words)
// - Real-time requirements
```

### Statistics

```rust
pub struct CascadeStats {
    /// Grammar FST states
    pub g_states: usize,

    /// States after L ∘ G
    pub lg_states: usize,

    /// States after det(L ∘ G)
    pub det_lg_states: usize,

    /// States after C ∘ det(L ∘ G)
    pub clg_states: usize,

    /// States after det(C ∘ L ∘ G)
    pub det_clg_states: usize,

    /// Final cascade states
    pub final_states: usize,

    /// Final cascade arcs
    pub final_arcs: usize,
}
```

## Examples

### Simple Lexicon + Grammar

```rust
use lling_llang::asr::{CascadeBuilder, CascadeConfig, LexiconEntry};
use lling_llang::semiring::LogWeight;

// Define vocabulary
let lexicon = vec![
    LexiconEntry {
        word: 0,  // "the"
        phones: vec![0, 1],  // [DH, AX]
        weight: LogWeight::one(),
        auxiliaries: vec![],
    },
    LexiconEntry {
        word: 1,  // "cat"
        phones: vec![2, 3, 4],  // [K, AE, T]
        weight: LogWeight::one(),
        auxiliaries: vec![],
    },
    LexiconEntry {
        word: 2,  // "sat"
        phones: vec![5, 3, 4],  // [S, AE, T]
        weight: LogWeight::one(),
        auxiliaries: vec![],
    },
];

// Build cascade (no grammar = accept any word sequence)
let mut builder = CascadeBuilder::<LogWeight>::new();
for entry in lexicon {
    builder.add_lexicon_entry(entry);
}
let cascade = builder.build();

println!("Cascade: {} states, {} arcs",
    cascade.stats().final_states,
    cascade.stats().final_arcs);
```

### Full Pipeline with Language Model

```rust
use lling_llang::asr::{CascadeBuilder, CascadeConfig};
use lling_llang::asr::ngram::NgramTransducer;
use lling_llang::semiring::LogWeight;

// Load n-gram language model as FST
let grammar: VectorWfst<WordId, LogWeight> = NgramTransducer::from_arpa("lm.arpa")
    .to_fst();

// Build context-dependency transducer
let context_fst = build_triphone_transducer(&phone_inventory);

// Build HMM transducer
let hmm_fst = build_3state_hmm(&phone_inventory);

// Construct full cascade
let cascade = CascadeBuilder::new()
    .config(CascadeConfig {
        incremental_det: true,
        minimize: true,
        ..Default::default()
    })
    .grammar(grammar)
    .context_dependency(context_fst)
    .hmm(hmm_fst)
    .add_lexicon_entries(&lexicon)
    .build_optimized();

// Inspect pipeline statistics
let stats = cascade.stats();
println!("Pipeline growth:");
println!("  G:           {} states", stats.g_states);
println!("  L∘G:         {} states", stats.lg_states);
println!("  det(L∘G):    {} states", stats.det_lg_states);
println!("  C∘det(L∘G):  {} states", stats.clg_states);
println!("  Final:       {} states, {} arcs",
    stats.final_states, stats.final_arcs);
```

### Using the Cascade for Decoding

```rust
use lling_llang::algorithms::shortest_path;
use lling_llang::composition::compose;

// Get the recognition network
let network = cascade.as_fst();

// Acoustic scores from neural network (as FST)
let acoustic_fst = build_acoustic_fst(&encoder_output);

// Compose acoustic scores with recognition network
let search_space = compose(acoustic_fst, network);

// Find best path
let best_path = shortest_path(&search_space, 1);

// Extract word sequence from path
let words: Vec<WordId> = best_path
    .output_labels()
    .filter_map(|l| l)
    .collect();
```

## Advanced Topics

### Incremental Determinization Strategy

Determinizing after each composition prevents exponential state explosion:

```
Without incremental det:
  L: 10K states
  L ∘ G: 10M states (explosion!)
  (L ∘ G) ∘ C: Memory exhausted

With incremental det:
  L: 10K states
  L ∘ G: 500K states
  det(L ∘ G): 50K states (10x reduction)
  det(L ∘ G) ∘ C: 200K states
  det(C ∘ L ∘ G): 100K states
  Final: Manageable size
```

The determinization removes:
- Redundant paths to the same state
- Non-deterministic choice points
- Epsilon transitions (via epsilon removal)

### Handling Homophones

Multiple pronunciations for the same word create parallel paths:

```rust
// "read" (present) vs "read" (past) - same spelling, different pronunciation
lexicon.push(LexiconEntry {
    word: word_id("read"),
    phones: vec![R, IY, D],  // "reed"
    weight: LogWeight::new(0.6),  // More common
    ..Default::default()
});
lexicon.push(LexiconEntry {
    word: word_id("read"),
    phones: vec![R, EH, D],  // "red"
    weight: LogWeight::new(0.4),  // Less common
    ..Default::default()
});
```

The `max_homophony` config limits pronunciations per word to control graph size.

### Lazy Composition (Future)

For dynamic applications, lazy composition avoids materializing the full graph:

```rust
// Future API
let cascade = CascadeBuilder::new()
    .config(CascadeConfig {
        lazy: true,  // Don't materialize full graph
        ..Default::default()
    })
    .build_lazy();

// Composition happens on-demand during search
let result = beam_search(&cascade, &acoustic_scores, beam_width);
```

### Memory vs. Speed Tradeoffs

| Setting | Memory | Decoding Speed | Build Time |
|---------|--------|----------------|------------|
| No optimization | High | Slow | Fast |
| Det only | Medium | Medium | Medium |
| Det + Min | Low | Fast | Slow |
| Lazy | Very Low | Medium | None |

## Related Documentation

- [ASR Pipeline](asr-pipeline.md) - High-level ASR architecture
- [Determinization](../algorithms/determinization.md) - Powerset construction
- [Minimization](../algorithms/minimization.md) - State reduction
- [Composition](../algorithms/composition.md) - WFST composition
- [N-gram Models](../integration/external/speech-nlp.md) - Language model integration

## References

- Mohri, M., Pereira, F., & Riley, M. (2002). "Weighted Finite-State Transducers in Speech Recognition"
- Mohri, M., & Riley, M. (2001). "A Weight Pushing Algorithm for Large Vocabulary Speech Recognition"
