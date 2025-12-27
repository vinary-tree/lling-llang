# liblevenshtein Overview

liblevenshtein-rust provides high-performance fuzzy string matching for lling-llang's spelling correction layer.

## What is liblevenshtein?

**liblevenshtein** is a Rust library implementing:

- **Levenshtein automata**: Efficient fuzzy matching within edit distance bounds
- **Trie dictionaries**: Multiple implementations for different use cases
- **Fuzzy collections**: Edit-distance-aware maps and caches
- **Phonetic matching**: Soundex, Metaphone, and other phonetic algorithms

## Architecture

```
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
| Lazy | Per-query | O(nm) | Low | Diverse queries |
| Universal | Precompute | O(nm) | Medium | Repeated patterns |
| Generalized | Runtime | O(nm) | Medium | Custom operations |

Where n = query length, m = dictionary size.

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

## Next Steps

- [Dictionaries](dictionaries.md): Dictionary implementations
- [Transducers](transducers.md): Query API and algorithms
- [Fuzzy Collections](fuzzy-collections.md): Maps and caches
- [Integration](lling-llang-integration.md): Using with lling-llang
