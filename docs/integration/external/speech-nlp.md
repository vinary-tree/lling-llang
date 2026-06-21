# Speech Recognition and NLP Pipelines

lling-llang integrates with speech recognition and NLP systems to process lattice outputs from acoustic models.

## Overview

Speech recognition systems produce lattices of word hypotheses. lling-llang processes these lattices to:

- **Rescore paths**: Apply language models and constraints
- **Filter paths**: Remove grammatically invalid sequences
- **Extract results**: Find the best path or n-best list

```
Audio Input
    │
    ▼
┌─────────────────────────┐
│   Acoustic Model        │
│   (produces lattice)    │
└───────────┬─────────────┘
            ▼
┌─────────────────────────┐
│     lling-llang         │
│   ┌─────────────────┐   │
│   │ Language Model  │   │
│   │ Grammar Filter  │   │
│   │ N-best Extract  │   │
│   └─────────────────┘   │
└───────────┬─────────────┘
            ▼
    Text Output
```

## Importing ASR Lattices

### Common Formats

| Format | Tool | Description |
|--------|------|-------------|
| HTK SLF | HTK | Standard Lattice Format |
| OpenFST | Kaldi/OpenFST | Binary FST format |
| ARPA | Various | Text-based format |
| JSON | Custom | Structured JSON |

> **Illustrative.** The `lling_llang::io::*` lattice-format readers/writers in
> this section (`io::htk`, `io::openfst`, `io::json`, `io::kaldi`) describe the
> *intended* import/export surface and are **not yet shipped** by the crate. The
> code blocks below are forward-looking sketches; build lattices today with
> [`LatticeBuilder`](../../api/lattice-reference.md) from your ASR system's API
> (see *Building from API* below).

### HTK SLF Import

```rust,ignore
use lling_llang::io::htk::HtkSlf;

// Parse HTK SLF file
let slf = HtkSlf::from_file("lattice.slf")?;

// Convert to lling-llang lattice
let lattice = slf.to_lattice::<TropicalWeight, HashMapBackend>()?;

println!("Imported {} nodes, {} edges", lattice.num_nodes(), lattice.num_edges());
```

### OpenFST Import

```rust,ignore
use lling_llang::io::openfst::OpenFstReader;

// Read binary FST
let reader = OpenFstReader::new("lattice.fst")?;

// Convert to lattice
let lattice = reader.to_lattice::<TropicalWeight, HashMapBackend>()?;
```

### Custom JSON Format

```rust,ignore
use lling_llang::io::json::JsonLatticeReader;

// Example JSON structure
// {
//   "nodes": [{"id": 0, "time": 0.0}, {"id": 1, "time": 0.5}, ...],
//   "edges": [{"from": 0, "to": 1, "word": "the", "score": -2.3}, ...]
// }

let reader = JsonLatticeReader::new();
let lattice = reader.from_file("lattice.json")?;
```

### Building from API

Many ASR systems provide streaming APIs:

```rust
use lling_llang::lattice::LatticeBuilder;

// Callback from ASR system
fn on_hypothesis(
    builder: &mut LatticeBuilder<TropicalWeight, HashMapBackend>,
    word: &str,
    start_time: f64,
    end_time: f64,
    acoustic_score: f64,
) {
    let start_node = time_to_node(start_time);
    let end_node = time_to_node(end_time);

    let weight = TropicalWeight::new(-acoustic_score);
    let meta = EdgeMetadata::new()
        .with_source("asr")
        .with_timing(start_time, end_time);

    builder.add_correction(start_node, end_node, word, weight, meta);
}
```

## Language Model Integration

### N-gram Language Models

```rust
use lling_llang::layers::LanguageModelLayer;

pub struct NgramLmLayer {
    lm: NgramModel,
    lm_weight: f64,
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for NgramLmLayer {
    fn name(&self) -> &str {
        "ngram-lm"
    }

    fn apply(&self, lattice: &Lattice<TropicalWeight, B>)
        -> Result<Lattice<TropicalWeight, B>, LayerError>
    {
        let mut new_lattice = lattice.clone();

        // For each edge, add LM score
        for edge in new_lattice.edges_mut() {
            let context = self.get_context(lattice, edge);
            let lm_score = self.lm.score(&context, edge.label());

            let combined = edge.weight.value() + self.lm_weight * lm_score;
            edge.set_weight(TropicalWeight::new(combined));
        }

        Ok(new_lattice)
    }

    fn estimated_reduction(&self) -> f64 {
        1.0  // Rescores, doesn't reduce
    }
}
```

### Neural Language Models

```rust
pub struct NeuralLmLayer {
    model: NeuralLM,
    batch_size: usize,
}

impl<B: LatticeBackend> CorrectionLayer<TropicalWeight, B> for NeuralLmLayer {
    fn apply(&self, lattice: &Lattice<TropicalWeight, B>)
        -> Result<Lattice<TropicalWeight, B>, LayerError>
    {
        // Extract all paths for batch scoring
        let paths: Vec<_> = enumerate_paths(lattice, 1000)?;

        // Batch score with neural LM
        let scores = self.model.score_batch(&paths)?;

        // Update weights
        let mut new_lattice = lattice.clone();
        for (path, score) in paths.iter().zip(scores) {
            update_path_weight(&mut new_lattice, path, score)?;
        }

        Ok(new_lattice)
    }
}
```

## Grammar Filtering

### CFG-Based Filtering

```rust
use lling_llang::layers::CfgFilterLayer;
use lling_llang::cfg::GrammarBuilder;

// Define grammar
let grammar = GrammarBuilder::new()
    .start("S")
    .rule("S", &["NP", "VP"])
    .rule("NP", &["Det", "N"])
    .rule("VP", &["V", "NP"])
    .rule("VP", &["V"])
    .terminals_from_file("vocab.txt")?
    .build()?;

let filter = CfgFilterLayer::new(&grammar);

// Apply to ASR lattice
let filtered = filter.apply(&asr_lattice)?;

println!("Reduced from {} to {} paths",
    asr_lattice.path_count(),
    filtered.path_count());
```

### Slot-Based Filtering

For voice command recognition:

```rust
// Grammar with slots
// COMMAND -> "call" CONTACT
// COMMAND -> "play" SONG
// COMMAND -> "navigate to" LOCATION

let grammar = GrammarBuilder::new()
    .start("COMMAND")
    .rule("COMMAND", &["call", "CONTACT"])
    .rule("COMMAND", &["play", "SONG"])
    .rule("COMMAND", &["navigate", "to", "LOCATION"])
    .slot("CONTACT", contact_names)
    .slot("SONG", song_titles)
    .slot("LOCATION", location_names)
    .build()?;
```

## N-best Extraction

### Basic N-best

```rust
use lling_llang::path::{nbest, NbestPath};

// Get top 10 hypotheses
let hypotheses: Vec<NbestPath<TropicalWeight>> = nbest(&mut lattice, 10);

for (rank, hyp) in hypotheses.iter().enumerate() {
    let words: Vec<_> = hyp.labels.iter().map(|l| l.as_str()).collect();
    println!("{}: {} (score: {:.3})",
        rank + 1,
        words.join(" "),
        hyp.weight.value());
}
```

### Diverse N-best

Avoid similar hypotheses:

```rust
use lling_llang::path::diverse_nbest;

// Minimum edit distance between hypotheses
let min_distance = 2;

let diverse = diverse_nbest(&mut lattice, 10, min_distance);
```

### Consensus Decoding

Find words with high agreement across paths:

```rust
use lling_llang::path::consensus_decode;

// Weight hypotheses by posterior probability
let result = consensus_decode(&mut lattice, 100)?;

for word in result.words {
    println!("{} (confidence: {:.2})", word.text, word.confidence);
}
```

## Kaldi Integration

### Reading Kaldi Lattices

```rust,ignore
use lling_llang::io::kaldi::{KaldiLatticeReader, KaldiSymbolTable};

// Load symbol table
let symbols = KaldiSymbolTable::from_file("words.txt")?;

// Read lattice archive
let reader = KaldiLatticeReader::new(&symbols);

for (utt_id, lattice) in reader.read_archive("lat.ark")? {
    println!("Processing utterance: {}", utt_id);

    let filtered = pipeline.apply(&lattice)?;
    let best = viterbi(&mut filtered);

    println!("Result: {}", best.to_string(&symbols));
}
```

### Writing Kaldi-Compatible Output

```rust,ignore
use lling_llang::io::kaldi::KaldiLatticeWriter;

let writer = KaldiLatticeWriter::new(&symbols);

// Write CTM (time-marked transcription)
writer.write_ctm(&result, "output.ctm")?;

// Write lattice
writer.write_lattice(&lattice, "output.lat")?;
```

## Whisper Integration

### Processing Whisper Output

```rust
// Whisper returns word-level hypotheses with timestamps
pub struct WhisperWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub probability: f64,
}

fn whisper_to_lattice(
    words: Vec<Vec<WhisperWord>>,  // Alternatives per position
) -> Lattice<TropicalWeight, HashMapBackend> {
    let mut builder = LatticeBuilder::new(HashMapBackend::new());

    for (pos, alternatives) in words.iter().enumerate() {
        for word in alternatives {
            let weight = TropicalWeight::new(-word.probability.ln());
            let meta = EdgeMetadata::new()
                .with_timing(word.start, word.end)
                .with_confidence(word.probability);

            builder.add_correction(pos, pos + 1, &word.text, weight, meta);
        }
    }

    builder.build(words.len())
}
```

## Streaming Processing

### Incremental Lattice Building

```rust
pub struct StreamingProcessor {
    builder: LatticeBuilder<TropicalWeight, HashMapBackend>,
    current_position: usize,
    window_size: usize,
}

impl StreamingProcessor {
    pub fn process_frame(&mut self, hypotheses: Vec<Hypothesis>) {
        for hyp in hypotheses {
            self.builder.add_correction(
                self.current_position,
                self.current_position + 1,
                &hyp.word,
                TropicalWeight::new(hyp.score),
                EdgeMetadata::default(),
            );
        }

        self.current_position += 1;

        // Process completed window
        if self.current_position >= self.window_size {
            self.emit_partial_result();
        }
    }

    fn emit_partial_result(&mut self) {
        let lattice = self.builder.build_partial(self.window_size);
        let best = viterbi(&mut lattice);
        // Emit partial transcription...
    }
}
```

## Performance Considerations

### Lattice Pruning

Before processing large lattices:

> **Illustrative.** A `lling_llang::prune` module with standalone `beam_prune` /
> `posterior_prune` lattice→lattice helpers is **not yet shipped**. The shipped
> pruning primitive is the path-side
> [`beam_search`](../../api/path-reference.md) in `lling_llang::path`; the sketch
> below shows the intended lattice-level pruning API.

```rust,ignore
use lling_llang::prune::{beam_prune, posterior_prune};

// Beam pruning: keep paths within beam of best
let pruned = beam_prune(&lattice, 10.0)?;

// Posterior pruning: keep high-probability paths
let pruned = posterior_prune(&lattice, 0.01)?;

println!("Reduced from {} to {} edges",
    lattice.num_edges(),
    pruned.num_edges());
```

### Batch Processing

```rust
use rayon::prelude::*;

// Process multiple utterances in parallel
let results: Vec<_> = utterances
    .par_iter()
    .map(|utt| {
        let lattice = import_lattice(utt)?;
        let processed = pipeline.apply(&lattice)?;
        let best = viterbi(&mut processed);
        Ok((utt.id.clone(), best.to_string()))
    })
    .collect::<Result<Vec<_>, Error>>()?;
```

## Next Steps

- [Text Correction](text-correction.md): Grammar and spelling correction
- [Library Usage](library-usage.md): Generic integration patterns
- [Path Extraction](../../algorithms/path-extraction.md): Viterbi and N-best
- [Parsing](../../algorithms/parsing.md): Grammar-based filtering
