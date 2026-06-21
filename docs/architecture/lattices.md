# Lattices

A lattice is a weighted directed acyclic graph (DAG) that represents the space of possible corrections for an input sequence. This document explains how lattices work and how to construct them.

## Terms & symbols

Symbols link to [`NOTATION.md`](../NOTATION.md); conventions in [`STYLE.md`](../STYLE.md).

| Symbol / term | Meaning |
|---|---|
| **Lattice** | A weighted DAG whose start→end paths enumerate hypotheses (a WFSA — see [wfst-traits](wfst-traits.md)). |
| **DAG** | Directed Acyclic Graph — no cycles, so all paths are finite. |
| **WFSA** | Weighted Finite-State Acceptor — a transducer with `input = output`. |
| `` `V` `` / `` `∣V∣` `` | The node (vertex) set and its cardinality. |
| `` `E` `` / `` `∣E∣` `` | The edge set and its cardinality. |
| `` `⊗` `` | Semiring *times*: accumulates edge weight along a path (Tropical: `` `+` ``). |
| `` `⊕` `` | Semiring *plus*: combines paths reaching the same node (Tropical: `` `min` ``). |

## Concepts

### What is a Lattice?

Imagine you're typing `` `"teh quik"` `` and want to find the best correction. There are multiple possibilities:

- `` `"teh"` `` could be: `` `"the"` ``, `` `"teh"` `` (keep it), `` `"tea"` ``, `` `"ten"` ``, …
- `` `"quik"` `` could be: `` `"quick"` ``, `` `"quik"` `` (keep it), `` `"quit"` ``, …

A lattice represents **all these possibilities compactly** as a graph. The rendered example below adds the word `` `fox` `` to make a complete sentence; its bold green path is the best (Viterbi) correction `` `the quick fox` `` with weight `` `0.5 ⊗ 0.5 ⊗ 0.0 = 1.0` `` (tropical):

![Correction lattice as a left-to-right weighted finite-state acceptor for "teh quik fox": node 0 → 1 has arcs the/0.5 (best, green) and teh/1.0 (alternative, grey); node 1 → 2 has quick/0.5 (best) and quik/1.0 (alternative); node 2 → 3 (final, double ring) has fox/0.0; the bold green path is the Viterbi best path the quick fox.](../diagrams/architecture/lattice-worked.svg)

*Blue circles = positions; green double-ring = the accepting (final) node; bold green arcs = the best (Viterbi) path; light-grey arcs = alternatives. Arc labels read `` `word / weight` ``.*

<details><summary>Text view</summary>

```text
            ┌───the(0.5)───┐
   start ──►│              ├───quick(0.5)───►fox(0.0)──►end
            ├───teh(0.0)───┤               ▲
            └───tea(1.5)───┘───quik(0.0)───┘
```

</details>

Each path from start to end is a complete correction (weights combine by `` `⊗` `` — addition in the tropical semiring):
- `` `"the quick"` `` with weight `` `1.0` `` (`` `0.5 + 0.5` ``)
- `` `"teh quik"` `` with weight `` `0.0` `` (`` `0.0 + 0.0` ``)
- `` `"tea quick"` `` with weight `` `2.0` `` (`` `1.5 + 0.5` ``)
- …

### Key Properties

1. **DAG Structure**: No cycles, so paths are finite
2. **Weighted Edges**: Each alternative has a weight (cost/score)
3. **Shared Structure**: Multiple paths share common edges
4. **Position-Based**: Nodes correspond to positions in the input

### Core Types

| Type | Description |
|------|-------------|
| `NodeId` | Identifier for a node (position in sequence) |
| `EdgeId` | Identifier for an edge (token alternative) |
| `Node` | Position with incoming/outgoing edge lists |
| `Edge<W>` | Transition with label, weight, and metadata |
| `EdgeMetadata` | Provenance info (edit distance, source layer, etc.) |
| `Lattice<W, B>` | The complete lattice structure |
| `LatticeBuilder<W, B>` | Incremental lattice construction |

## Building Lattices

### Basic Construction

Use `LatticeBuilder` to construct lattices incrementally:

```rust
use lling_llang::prelude::*;

let backend = HashMapBackend::new();
let mut builder = LatticeBuilder::new(backend);

// Add alternatives for position 0 → 1
builder.add_correction(0, 1, "the", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
builder.add_correction(0, 1, "teh", TropicalWeight::new(0.0), EdgeMetadata::original());
builder.add_correction(0, 1, "tea", TropicalWeight::new(1.5), EdgeMetadata::correction(2));

// Add alternatives for position 1 → 2
builder.add_correction(1, 2, "quick", TropicalWeight::new(0.5), EdgeMetadata::correction(1));
builder.add_correction(1, 2, "quik", TropicalWeight::new(0.0), EdgeMetadata::original());

// Build the lattice (end position = 2)
let lattice = builder.build(2);
```

### Position Model

Nodes represent **positions** in the input sequence. An edge from position `i` to position `j` spans the token(s) at positions `i, i+1, ..., j-1`.

```
Position:   0    1    2
            │    │    │
Input:      teh  quik
            │    │    │
Edges:      ├────┤    │     (single-token edges: 0→1, 1→2)
            ├─────────┤     (multi-token edge: 0→2 for "the quick")
```

Multi-token edges are useful for:
- Phrase corrections ("gonna" → "going to")
- Contraction expansion ("it's" → "it is")
- Token merging ("every one" → "everyone")

### Edge Metadata

`EdgeMetadata` tracks the provenance of each correction:

```rust
// Original token (no correction)
EdgeMetadata::original()

// Correction with edit distance
EdgeMetadata::correction(2)  // 2 character edits

// Phonetic match
EdgeMetadata::phonetic()

// Grammar rule application
EdgeMetadata::grammar_rule(42)  // Rule ID 42

// Chain metadata
EdgeMetadata::correction(1).with_layer(0)  // From layer 0
```

This metadata is used for:
- Filtering (e.g., reject corrections with edit distance > 3)
- Diagnostics (explain why a correction was suggested)
- Optimization (prioritize certain correction types)

## Lattice Operations

### Accessing Structure

```rust
// Basic properties
let num_nodes = lattice.num_nodes();
let num_edges = lattice.num_edges();
let start = lattice.start();  // First node
let end = lattice.end();      // Last node

// Access nodes and edges
if let Some(node) = lattice.node(NodeId::new(1)) {
    println!("Position: {:?}", node.position);
    println!("Out-degree: {}", node.out_degree());
}

if let Some(edge) = lattice.edge(EdgeId::new(0)) {
    println!("Label: {}", lattice.backend().lookup(edge.label).unwrap());
    println!("Weight: {:?}", edge.weight);
}
```

### Iterating Edges

```rust
// Iterate outgoing edges from a node
for edge in lattice.outgoing_edges(NodeId::new(0)) {
    let word = lattice.backend().lookup(edge.label).unwrap();
    println!("{}: {:?}", word, edge.weight);
}

// Iterate incoming edges to a node
for edge in lattice.incoming_edges(NodeId::new(1)) {
    // ...
}

// Iterate all edges
for edge in lattice.edges() {
    // ...
}
```

### Topological Order

Many algorithms require nodes in topological order (every edge goes from earlier to later):

```rust
if let Some(order) = lattice.topological_order() {
    for node_id in order {
        // Process nodes in dependency order
    }
}
```

The builder automatically ensures nodes are sorted by position, which provides a valid topological order for standard lattices.

### Vocabulary Lookup

Edge labels are vocabulary IDs (integers). Use the backend to convert to strings:

```rust
let edge = lattice.edge(EdgeId::new(0)).unwrap();
let word = lattice.backend().lookup(edge.label);
println!("Word: {:?}", word);  // Some("the")
```

## Details

### Memory Layout

Lattices store nodes and edges in contiguous vectors for cache efficiency:

```
Nodes: [Node { outgoing: [...], incoming: [...] }, ...]
Edges: [Edge { source, target, label, weight, metadata }, ...]
```

Edges are referenced by `EdgeId`, which is an index into the edge vector. This avoids pointer chasing and enables efficient iteration.

### SmallVec Optimization

Node adjacency lists use `SmallVec<[EdgeId; 8]>` to avoid heap allocation for typical cases:
- Most nodes have < 8 outgoing edges
- Stack allocation for small lists, heap for larger

### Vocabulary Interning

The backend **interns** strings to avoid duplicate storage:

```rust
let backend = HashMapBackend::new();
let mut builder = LatticeBuilder::new(backend);

// Both edges share the same label ID
builder.add_correction(0, 1, "hello", weight, metadata);
builder.add_correction(1, 2, "hello", weight, metadata);

let lattice = builder.build(2);
assert_eq!(lattice.backend().vocab_size(), 1);  // Only one "hello"
```

This is critical for large vocabularies and distributed storage (PathMap).

### Pre-allocation

For performance, pre-allocate when sizes are known:

```rust
// Pre-allocate for 100 positions, ~5 edges per position
let builder = LatticeBuilder::with_capacity(backend, 100, 5);
```

Or reserve incrementally:

```rust
builder.reserve_positions(50);
builder.reserve_edges(250);
```

### Empty Lattices

An empty lattice (no edges) is valid if start equals end:

```rust
let builder: LatticeBuilder<TropicalWeight, _> = LatticeBuilder::new(backend);
let lattice = builder.build(0);  // Empty input

assert!(lattice.is_empty());
assert_eq!(lattice.start(), lattice.end());
```

This represents an input with no tokens (the empty string).

## Common Patterns

### Building from Token Alternatives

Given a list of token alternatives:

```rust
struct Alternative {
    word: String,
    weight: f64,
    is_original: bool,
}

fn build_lattice(
    tokens: &[Vec<Alternative>],
    backend: HashMapBackend,
) -> Lattice<TropicalWeight, HashMapBackend> {
    let mut builder = LatticeBuilder::with_capacity(
        backend,
        tokens.len(),
        tokens.iter().map(|t| t.len()).max().unwrap_or(1),
    );

    for (pos, alternatives) in tokens.iter().enumerate() {
        for alt in alternatives {
            let meta = if alt.is_original {
                EdgeMetadata::original()
            } else {
                EdgeMetadata::default()
            };

            builder.add_correction(
                pos,
                pos + 1,
                &alt.word,
                TropicalWeight::new(alt.weight),
                meta,
            );
        }
    }

    builder.build(tokens.len())
}
```

### Diamond Lattices

A "diamond" pattern represents two paths that diverge and reconverge:

```rust
//     ┌─a─┐
// 0 ──┤   ├── 2
//     └─b─┘

builder.add_correction(0, 2, "a", weight_a, metadata);
builder.add_correction(0, 2, "b", weight_b, metadata);
```

This is common for alternative phrasings that span the same range.

### Skip Edges

To allow deleting tokens, add edges that skip positions:

```rust
// Original: "the the quick"
// Allow deleting duplicate "the"

builder.add_correction(0, 1, "the", normal_weight, metadata);
builder.add_correction(1, 2, "the", normal_weight, metadata);  // duplicate
builder.add_correction(1, 2, "", deletion_weight, metadata);   // skip
builder.add_correction(2, 3, "quick", normal_weight, metadata);
```

### Phrase Insertions

To allow inserting tokens, use multi-token edges:

```rust
// "gonna" → "going to" (1 token → 2 tokens)

builder.add_correction(0, 1, "gonna", keep_weight, EdgeMetadata::original());
builder.add_correction(0, 2, "going to", correct_weight, EdgeMetadata::correction(1));
```

## Next Steps

- [Path Extraction](../algorithms/path-extraction.md): Find optimal paths through lattices
- [Backends](backends.md): Different storage strategies
- [API Reference](../api/lattice-reference.md): Complete API documentation

## References

Full entries — including DOIs — are in [`BIBLIOGRAPHY.md`](../BIBLIOGRAPHY.md).

- [**Mohri 2002**](../BIBLIOGRAPHY.md#ref-mohri2002) — Mohri, Pereira & Riley, *Weighted Finite-State Transducers in Speech Recognition*: weighted lattices/acceptors as the representation of hypothesis spaces. [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184)
- [**Mohri 2009**](../BIBLIOGRAPHY.md#ref-mohri2009) — Mohri, *Weighted Automata Algorithms*: topological shortest-distance over a DAG in `` `O(∣V∣ + ∣E∣)` ``, the bound the position-ordered lattice achieves. [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6)
