# Phonetic Rescoring Layer

Reranking lattice paths using phonetic similarity for error recovery and OOV handling.

## What is Phonetic Rescoring?

Phonetic rescoring adjusts lattice edge weights based on how words *sound*, not just their surface form. This helps recover from ASR errors where acoustic confusions produce phonetically similar but orthographically different words.

![Phonetic-rescoring worked example as a left-to-right WFSA: position 0 chooses between the acoustically-confused "knight" and "night" (which normalize to the same Zompist form), position 1 between "mare" and "mayor"; after rescoring against the reference "night mayor", the bold green path "night mayor" is the best path.](../../diagrams/integration/phonetic-rescore.svg)

*Green bold = the best (Viterbi) path after rescoring; grey = alternatives. The
homophone "knight" is boosted from `1.00` → `0.60` because it normalizes to the
same phonetic form as the reference word "night", yet the known word "night"
(`0.50`) still wins. The green double-ring node `2` is final.*

<details><summary>Text view</summary>

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                       Phonetic Rescoring in Lattices                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Original Lattice:                                                         │
│                                                                             │
│       ┌─ "knight" (w=1.0) ─┐                                               │
│   (0) ┤                     ├─(1)─ "..." ─(2)                              │
│       └─ "night"  (w=1.2) ─┘                                               │
│                                                                             │
│   After Phonetic Rescoring (reference has "night"):                        │
│                                                                             │
│       ┌─ "knight" (w=0.6) ─┐    ← Same sound! Boosted                      │
│   (0) ┤                     ├─(1)─ "..." ─(2)                              │
│       └─ "night"  (w=0.5) ─┘    ← Known word, best score                   │
│                                                                             │
│   Key insight: "knight" and "night" normalize to the same phonetic form    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

</details>

## Terminology

| Term | Definition |
|------|------------|
| **Phonetic normalization** | Converting spelling to pronunciation representation |
| **Zompist rules** | 62 verified English spelling-to-sound rules |
| **Reference** | Expected/correct words for comparison |
| **Interpolation weight** | Balance between original and phonetic scores (λ) |
| **Lattice rescoring** | Adjusting edge weights without changing structure |

## PhoneticReference Trait

The reference provides expected words for phonetic comparison.

```rust
pub trait PhoneticReference: Send + Sync {
    /// Get reference word(s) for a position.
    fn reference_at(&self, position: usize) -> Option<&[String]>;

    /// Check if a word is known/correct.
    fn is_known(&self, word: &str) -> bool;
}
```

### Reference Types

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Reference Type Comparison                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   VocabularyReference:                                                      │
│   ┌──────────────────────────────────────────────────────────────────────┐ │
│   │ Words: {"hello", "world", "the", "quick", "brown", "fox"}            │ │
│   │                                                                      │ │
│   │ is_known("hello") → true                                             │ │
│   │ is_known("helo")  → false                                            │ │
│   │ reference_at(0)   → None  (no position info)                         │ │
│   └──────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
│   SequenceReference:                                                        │
│   ┌──────────────────────────────────────────────────────────────────────┐ │
│   │ Sequence: ["the", "quick", "brown", "fox"]                           │ │
│   │                                                                      │ │
│   │ is_known("quick") → true                                             │ │
│   │ reference_at(0)   → Some(["the"])                                    │ │
│   │ reference_at(1)   → Some(["quick"])                                  │ │
│   │ reference_at(4)   → None  (out of bounds)                            │ │
│   └──────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Creating References

### VocabularyReference

Use when you have a dictionary of valid words:

```rust
use lling_llang::layers::VocabularyReference;

// From an iterator of strings
let vocab = VocabularyReference::new(
    ["hello", "world", "the", "quick", "brown", "fox"]
        .iter()
        .map(|s| s.to_string())
);

// Check membership
assert!(vocab.is_known("hello"));
assert!(!vocab.is_known("helo"));
```

### SequenceReference

Use when you have the expected word sequence:

```rust
use lling_llang::layers::SequenceReference;

// Expected transcription
let reference = SequenceReference::from_sequence(
    ["the", "quick", "brown", "fox"]
        .iter()
        .map(|s| s.to_string())
);

// Position-aware lookup
assert_eq!(reference.reference_at(0), Some(&["the".to_string()][..]));
assert_eq!(reference.reference_at(3), Some(&["fox".to_string()][..]));

// With multiple alternatives per position
let multi_ref = SequenceReference::new(vec![
    vec!["the".to_string(), "a".to_string()],  // Position 0: "the" or "a"
    vec!["quick".to_string()],                  // Position 1: "quick"
    vec!["brown".to_string(), "red".to_string()],  // etc.
]);
```

## PhoneticRescoreLayer

The main layer for phonetic lattice rescoring.

### Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `weight` | 0.5 | Interpolation: 0=original only, 1=phonetic only |
| `fuel` | 1000 | Max rewrite iterations per word |
| `cache_size` | 10,000 | Max cached normalizations |

### Basic Usage

```rust
use lling_llang::layers::{PhoneticRescoreLayer, VocabularyReference, CorrectionLayer};
use std::sync::Arc;

// Create reference
let vocab = VocabularyReference::new(
    ["hello", "world", "night", "knight"]
        .iter()
        .map(|s| s.to_string())
);

// Create layer with default Zompist English rules
let layer = PhoneticRescoreLayer::new(Arc::new(vocab))
    .with_weight(0.3)  // 30% phonetic, 70% original
    .with_cache_size(50_000);

// Apply to lattice
let rescored_lattice = layer.apply(&input_lattice)?;
```

### Weight Interpolation Formula

The rescoring interpolates between original and phonetic scores, where `λ ∈ [0, 1]`
is the phonetic mixing weight:
`new_weight = (1 − λ)·original_weight + λ·phonetic_cost`.

```text
new_weight = (1 − λ) × original_weight + λ × phonetic_cost

where:
  λ               = phonetic weight (0.0 to 1.0)
  original_weight = edge weight from input lattice
  phonetic_cost   = −log(phonetic_score) for the word
```

### Phonetic Score Calculation

Phonetic similarity is `sim = 1 − levenshtein(normalize(w₁), normalize(w₂)) / max_len`,
so words that normalize to the same Zompist form score `sim = 1`.

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                       Phonetic Score Calculation                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   For a word at position p:                                                 │
│                                                                             │
│   1. Known word in vocabulary?                                              │
│      └── YES → score = -0.1 (high probability)                             │
│                                                                             │
│   2. Position has reference words?                                          │
│      └── YES → score = ln(best_similarity × 0.9 + 0.1)                     │
│          where best_similarity = max phonetic_sim(word, ref) for each ref  │
│                                                                             │
│   3. Unknown word, no reference                                             │
│      └── score = -2.0 (moderate penalty)                                   │
│                                                                             │
│   Phonetic similarity:                                                      │
│     sim = 1 - (levenshtein(normalize(w1), normalize(w2)) / max_len)        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Direct Phonetic Methods

Access phonetic operations directly:

```rust
let layer = PhoneticRescoreLayer::new(reference);

// Normalize using Zompist rules
let normalized = layer.normalize("knight");  // "nait" or similar
let normalized2 = layer.normalize("night");  // "nait" - same!

// Compute phonetic distance [0, 1]
let dist = layer.phonetic_distance("knight", "night");  // ~0.0 (homophones)

// Compute phonetic similarity [0, 1]
let sim = layer.phonetic_similarity("phone", "fone");  // ~1.0 (same sound)
```

## Custom Phonetic Rules

### Using Non-English Rules

```rust
use lling_llang::layers::PhoneticRescoreLayer;
use liblevenshtein::phonetic::llev::{parse_str, RuleSetChar};

// Load custom rules from .llev file format
let german_llev = r#"
# German phonetic rules
@rules
"sch" -> "S"
"ch" -> "x"
"ie" -> "i:"
"ei" -> "aI"
"#;

let rule_set = RuleSetChar::from_llev(&parse_str(german_llev)?)?;

// Create layer with custom rules
let layer = PhoneticRescoreLayer::with_rules(
    Arc::new(reference),
    rule_set.rules
);
```

### Rule Format

liblevenshtein's `.llev` format supports:

```
@rules
"pattern" -> "replacement"     # Simple replacement
"[aeiou]" -> "V"               # Character class
"^[A-Z]" -> ""                 # Regex-style patterns
```

## Integration with CorrectionLayer Pipeline

```rust
use lling_llang::layers::{
    LayerPipelineBuilder, PhoneticRescoreLayer, ConfusionLayer,
};
use std::sync::Arc;

// Build correction pipeline. `add_layer` takes the layer by value and
// boxes it internally, so concrete layers are passed directly (not wrapped
// in `Box::new`).
let vocab_ref = Arc::new(VocabularyReference::new(vocabulary));

let pipeline = LayerPipelineBuilder::new()
    // Step 1: Expand with confusion alternatives
    .add_layer(ConfusionLayer::new(confusion_matrix))
    // Step 2: Rescore based on phonetics
    .add_layer(
        PhoneticRescoreLayer::new(vocab_ref)
            .with_weight(0.4),
    )
    .build();

// Apply the layer pipeline to the input lattice.
let corrected = pipeline.apply(&input_lattice)?;

// Beam pruning is a path-side operation rather than a layer: after the
// pipeline produces the reweighted lattice, prune with `beam_search`
// (see `lling_llang::path::{beam_search, BeamSearchConfig}`).
```

## Thread Safety

`PhoneticRescoreLayer` is thread-safe:

```rust
use std::sync::Arc;
use std::thread;

let layer = Arc::new(
    PhoneticRescoreLayer::new(reference)
        .with_weight(0.3)
);

let handles: Vec<_> = lattices
    .into_iter()
    .map(|lattice| {
        let layer = layer.clone();
        thread::spawn(move || {
            layer.apply(&lattice)
        })
    })
    .collect();

let results: Vec<_> = handles
    .into_iter()
    .map(|h| h.join().unwrap())
    .collect();
```

Implementation details:
- Reference stored in `Arc<dyn PhoneticReference>`
- Normalization cache uses `DashMap` (lock-free concurrent map)
- Rules cloned per normalization (consider caching for hot paths)

## Complete Example: ASR Error Recovery

```rust
use lling_llang::layers::{
    PhoneticRescoreLayer, SequenceReference, CorrectionLayer
};
use lling_llang::lattice::LatticeBuilder;
use lling_llang::semiring::TropicalWeight;
use lling_llang::backend::HashMapBackend;
use std::sync::Arc;

fn recover_asr_errors(
    asr_lattice: &Lattice<TropicalWeight, HashMapBackend>,
    expected_transcript: &[&str],
) -> Result<Lattice<TropicalWeight, HashMapBackend>, LayerError> {
    // Create sequence reference from expected transcript
    let reference = SequenceReference::from_sequence(
        expected_transcript.iter().map(|s| s.to_string())
    );

    // Create phonetic rescore layer
    // Weight 0.4 = 40% phonetic influence
    let layer = PhoneticRescoreLayer::new(Arc::new(reference))
        .with_weight(0.4)
        .with_cache_size(10_000);

    // Apply rescoring
    layer.apply(asr_lattice)
}

fn main() {
    // ASR produced lattice with confusions
    let backend = HashMapBackend::new();
    let mut builder = LatticeBuilder::new(backend);

    // Position 0: "knight" vs "night" (acoustic confusion)
    builder.add_correction(0, 1, "knight", TropicalWeight::new(1.0), Default::default());
    builder.add_correction(0, 1, "night", TropicalWeight::new(1.2), Default::default());

    // Position 1: "mare" vs "mayor" (vowel confusion)
    builder.add_correction(1, 2, "mare", TropicalWeight::new(1.0), Default::default());
    builder.add_correction(1, 2, "mayor", TropicalWeight::new(1.5), Default::default());

    let lattice = builder.build(2);

    // Expected: "night mayor"
    let expected = ["night", "mayor"];
    let rescored = recover_asr_errors(&lattice, &expected).unwrap();

    // Now "night" and "mayor" should have better scores
    for edge in rescored.edges() {
        let word = rescored.edge_word(edge).unwrap_or("?");
        println!("{}: weight = {:.3}", word, edge.weight.value());
    }
}
```

## Weight Tuning Guidelines

| Use Case | Recommended λ | Rationale |
|----------|---------------|-----------|
| Spell correction | 0.3 - 0.4 | Mild phonetic influence |
| ASR error recovery | 0.4 - 0.6 | Balance acoustic and phonetic |
| Homophone detection | 0.7 - 0.9 | Strong phonetic emphasis |
| Unknown speaker | 0.5 | Balanced default |

## Performance Considerations

### Caching

```rust
// For large vocabularies, increase cache
let layer = PhoneticRescoreLayer::new(reference)
    .with_cache_size(100_000);  // Cache 100k normalizations

// Check cache utilization
// (Normalizations are cached automatically)
```

### Fuel Limit

```rust
// For complex rules, increase fuel
let layer = PhoneticRescoreLayer::new(reference)
    .with_fuel(5000);  // Allow more rewrite iterations

// Prevents infinite loops in pathological cases
```

### Benchmarks

| Operation | Time | Notes |
|-----------|------|-------|
| `normalize()` | ~10 μs | Per word, uncached |
| `normalize()` | ~100 ns | Per word, cached |
| `phonetic_distance()` | ~20 μs | Two normalizations + Levenshtein |
| `apply()` | ~1 ms | 100-edge lattice |

## Related Documentation

- [libgrammstein Phonetic Embeddings](../../../libgrammstein/docs/components/embedding/phonetic.md)
  — sibling-repo link; assumes `libgrammstein` is checked out beside
  `lling-llang` (see the [integration README](../README.md#external-repository-link-convention)).
- [CorrectionLayer API](../../architecture/layers.md)
- [Lattice Operations](../../architecture/lattices.md)

## References

- <a id="cite-mohri2002"></a>[Mohri 2002](../../BIBLIOGRAPHY.md#ref-mohri2002) —
  Mohri, M., Pereira, F., & Riley, M. (2002). *Weighted Finite-State Transducers
  in Speech Recognition.* Computer Speech & Language 16(1):69–88. Lattice
  rescoring as edge-weight adjustment over a fixed WFSA topology — the operation
  this layer performs in the phonetic domain.
