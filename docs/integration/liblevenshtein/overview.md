# liblevenshtein Overview

liblevenshtein-rust provides high-performance fuzzy string matching for lling-llang's spelling correction layer.

## What is liblevenshtein?

**liblevenshtein** is a Rust library implementing:

- **Levenshtein automata**: Efficient fuzzy matching within edit distance bounds
- **Trie dictionaries**: Multiple implementations for different use cases
- **Fuzzy collections**: Edit-distance-aware maps and caches
- **Phonetic matching**: Soundex, Metaphone, and other phonetic algorithms

## Architecture

The stack is read **bottom-up**: a dictionary tier stores the vocabulary, a
Levenshtein automaton is driven over it by a transducer, and fuzzy-collection
caches sit on top. lling-llang consumes the transducer's `Candidate { term,
distance }` stream and turns each match into a weighted correction edge.

![Container view of the liblevenshtein fuzzy-matching stack feeding lling-llang's correction layers: a query term enters the transducer, which builds a Levenshtein automaton over a trie dictionary; the dictionary returns candidates with edit distances that lling-llang adds as weighted lattice edges.](../../diagrams/integration/liblevenshtein-overview.svg)

*Blue = dictionary foundation; teal = transducer/automata; amber = fuzzy
collections; orange = liblevenshtein boundary; grey = lling-llang
applications. The bold orange arrow is the `Candidate` hand-off across the
library boundary.*

<details><summary>Text view</summary>

```text
┌─────────────────────────────────────────────────────────────────┐
│                    liblevenshtein                               │
├─────────────────────────────────────────────────────────────────┤
│  Transducer Layer                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────┐│
│  │  Transducer  │  │  Algorithm   │  │  SubstitutionPolicy    ││
│  │  (query API) │  │  (Standard,  │  │  (Unrestricted,        ││
│  │              │  │  Transpos.,  │  │   Restricted)          ││
│  │              │  │  MergeSplit) │  │                        ││
│  └──────────────┘  └──────────────┘  └────────────────────────┘│
├─────────────────────────────────────────────────────────────────┤
│  Automata Layer                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────┐│
│  │    Lazy      │  │  Universal   │  │   Generalized          ││
│  │  Automata    │  │  Automata    │  │   Automata             ││
│  │ (on-demand)  │  │ (precomputed)│  │ (runtime config)       ││
│  └──────────────┘  └──────────────┘  └────────────────────────┘│
├─────────────────────────────────────────────────────────────────┤
│  Dictionary Layer                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────┐│
│  │DoubleArray   │  │ DynamicDawg  │  │  SuffixAutomaton       ││
│  │   Trie       │  │  (ASCII/     │  │  (Substring            ││
│  │(ASCII/Char)  │  │   Char)      │  │   matching)            ││
│  └──────────────┘  └──────────────┘  └────────────────────────┘│
├─────────────────────────────────────────────────────────────────┤
│  Fuzzy Collections                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────┐│
│  │  FuzzyMap    │  │ FuzzyMultiMap│  │  Eviction Wrappers     ││
│  │              │  │              │  │  (LRU, TTL, LFU, etc.) ││
│  └──────────────┘  └──────────────┘  └────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

</details>

## Core Concepts

### Edit Distance

The **Levenshtein distance** counts the minimum edits to transform one string into another:

| Operation | Example | Description |
|-----------|---------|-------------|
| Insert | cat → cats | Add a character |
| Delete | cats → cat | Remove a character |
| Substitute | cat → cut | Replace a character |

Extended operations:
- **Transpose**: cat → cta (swap adjacent characters)
- **Merge**: ca t → cat (merge two characters)
- **Split**: cat → c at (split one character into two)

### Trie-Based Matching

liblevenshtein uses trie (prefix tree) dictionaries for efficient matching:

```
          root
         /    \
        c      d
       /        \
      a          o
     / \          \
    t   r          g

Terms: "cat", "car", "dog"
```

Matching a query term against the trie:
1. Traverse trie and query string in parallel
2. Track edit distance at each position
3. Prune branches exceeding distance threshold
4. Collect all terms within threshold

### Lazy vs Universal Automata

**Lazy automata** (default):
- Built on-demand for each query
- Lower memory usage
- Best for diverse queries

**Universal automata**:
- Precomputed once, reused for all queries
- Higher initial cost, faster subsequent queries
- Best for repeated patterns

## Key Types

### Dictionary Trait

```rust
pub trait Dictionary {
    type Node: DictionaryNode;

    fn root(&self) -> Self::Node;
    fn contains(&self, term: &str) -> bool;
    fn len(&self) -> Option<usize>;
    fn is_empty(&self) -> bool;
}
```

### DictionaryNode Trait

```rust
pub trait DictionaryNode {
    type Unit: CharUnit;

    fn is_final(&self) -> bool;
    fn transition(&self, label: Self::Unit) -> Option<Self>;
    fn edges(&self) -> Box<dyn Iterator<Item = (Self::Unit, Self)>>;
}
```

### Transducer

```rust
pub struct Transducer<D: Dictionary, P: SubstitutionPolicy = Unrestricted> {
    // ...
}

impl<D: Dictionary> Transducer<D> {
    pub fn new(dict: D, algorithm: Algorithm) -> Self;
    pub fn query(&self, term: &str, max_distance: usize)
        -> QueryIterator<D::Node, String, P>;
}
```

### Algorithm

```rust
pub enum Algorithm {
    Standard,       // Insert, Delete, Substitute
    Transposition,  // + Transpose adjacent chars
    MergeAndSplit,  // + Merge/Split operations
}
```

## Performance Characteristics

### Dictionary Comparison

| Dictionary | Query Speed | Updates | Memory | Use Case |
|------------|-------------|---------|--------|----------|
| DoubleArrayTrie | Fastest (3x) | Append-only | Lowest | Static dictionaries |
| DynamicDawg | Fast | Full CRUD | Medium | Dynamic dictionaries |
| SuffixAutomaton | Fast | Limited | Higher | Substring search |

### Automata Comparison

| Automata | Build Cost | Query Cost | Memory | Use Case |
|----------|------------|------------|--------|----------|
| Lazy | Per-query | `O(n·m)` | Low | Diverse queries |
| Universal | Precompute | `O(n·m)` | Medium | Repeated patterns |
| Generalized | Runtime | `O(n·m)` | Medium | Custom operations |

Where `n` = query length, `m` = matched dictionary subtree size. Trie traversal
is linear in the query length per surviving branch, with branches pruned as soon
as the running edit distance exceeds the threshold `k`.

## Thread Safety

All dictionary implementations support concurrent access:

```rust
pub enum SyncStrategy {
    ExternalSync,  // Requires external synchronization
    InternalSync,  // Internally synchronized (Arc<RwLock<>>)
    Persistent,    // Immutable with structural sharing
}
```

Most implementations use `InternalSync` with `Arc<RwLock<>>` for safe concurrent reads and writes.

## Feature Flags

```toml
[dependencies]
liblevenshtein = { version = "1.0", features = [
    "serialization",     # Bincode, JSON, PlainText
    "compression",       # Gzip compression
    "protobuf",          # Protocol buffers
    "pathmap-backend",   # PathMap distributed storage
]}
```

## Design Principles

1. **Zero-cost abstractions**: Default policies are zero-sized types
2. **Composability**: Dictionary backends are interchangeable
3. **Lazy evaluation**: Automata built on-demand
4. **Thread safety**: Concurrent access by default
5. **Generic operations**: OperationSet for runtime configuration

## Related Topics

- [Dictionaries](dictionaries.md): Dictionary implementations
- [Transducers](transducers.md): Query API and algorithms
- [Fuzzy Collections](fuzzy-collections.md): Maps and caches
- [Integration](lling-llang-integration.md): Using with lling-llang

## References

- <a id="cite-mohri2009"></a>[Mohri 2009](../../BIBLIOGRAPHY.md#ref-mohri2009) —
  Mohri, M. (2009). *Weighted Automata Algorithms.* In *Handbook of Weighted
  Automata*, pp. 213–254. Springer. The weighted-automata and edit-distance
  transducer framework underlying fuzzy matching as automaton-over-dictionary
  traversal.
- <a id="cite-allauzen2007"></a>[Allauzen 2007](../../BIBLIOGRAPHY.md#ref-allauzen2007) —
  Allauzen, C., Riley, M., Schalkwyk, J., Skut, W., & Mohri, M. (2007).
  *OpenFst: A General and Efficient Weighted Finite-State Transducer Library.*
  CIAA 2007. The transducer-library design lineage these dictionary/automaton
  abstractions follow.
