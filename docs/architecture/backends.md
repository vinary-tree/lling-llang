# Backends

The backend abstraction separates lattice logic from vocabulary storage, enabling different storage strategies for different deployment scenarios.

## Terms & symbols

Symbols link to [`NOTATION.md`](../NOTATION.md); conventions in [`STYLE.md`](../STYLE.md).

| Symbol / term | Meaning |
|---|---|
| **Interning** | Mapping each distinct string to one compact integer handle (and back). |
| `VocabId` | The handle type — a `u32` index identifying an interned word. |
| **Structural sharing** | Two backends sharing one underlying store via copy-on-write (PathMap). |
| $`\lvert V\rvert`$ | Vocabulary size (number of unique interned words), `LatticeBackend::vocab_size()`. |

## Concepts

### What is a Backend?

A backend handles **vocabulary interning** - converting strings to compact integer IDs and back. This is critical for:

1. **Memory efficiency**: Store word IDs (4 bytes) instead of strings
2. **Deduplication**: Same word → same ID everywhere
3. **Fast comparison**: Compare IDs instead of strings
4. **Storage flexibility**: In-memory, distributed, or persistent

### The Interning Pattern

```
"the" ──intern()──► 0
"quick" ──intern()──► 1
"the" ──intern()──► 0   (same word → same ID)

0 ──lookup()──► "the"
1 ──lookup()──► "quick"
```

This pattern is used throughout lling-llang:
- Lattice edges store `VocabId` labels
- Path extraction returns `VocabId` sequences
- String conversion happens only at boundaries

## The LatticeBackend Trait

One interning trait, two storage strategies: the default in-memory `HashMapBackend` and the structurally-sharing `PathMapBackend` (extended by the `PathMapSharingBackend` trait under the `f1r3fly` feature).

![Backend storage hierarchy: the LatticeBackend interning trait (blue) with intern/lookup/vocab_size/contains/get_id/iter/supports_sharing is implemented by HashMapBackend (FxHashMap + Vec, supports_sharing=false) and PathMapBackend (Arc<PathMap> + IndexMap, supports_sharing=true, cfg f1r3fly); PathMapSharingBackend (green) extends LatticeBackend with share_prefix/shares_structure_with and is implemented by PathMapBackend.](../diagrams/architecture/backends.svg)

*Blue = the `LatticeBackend` interning interface; green = the `PathMapSharingBackend` extension trait (copy-on-write sharing); grey = the concrete `HashMapBackend` and `PathMapBackend` structs.*

<details><summary>Text view</summary>

```text
                       LatticeBackend  (str ↔ VocabId)
        intern · lookup · vocab_size · contains · get_id · iter · supports_sharing
                 ▲                                  ▲
                 │ impl                             │ impl
        HashMapBackend                        PathMapBackend ──impl──► PathMapSharingBackend
   FxHashMap<Arc<str>,VocabId>           Arc<PathMap<VocabMetadata>>     (cfg f1r3fly)
   + Vec<Arc<str>>                       + IndexMap<Arc<str>,VocabId>    share_prefix
   supports_sharing() = false           supports_sharing() = true       shares_structure_with
```

</details>

```rust
pub type VocabId = u32;

pub trait LatticeBackend: Clone + Send + Sync {
    /// Intern a word, returning its vocabulary ID.
    fn intern(&mut self, word: &str) -> VocabId;

    /// Look up a word by vocabulary ID.
    fn lookup(&self, id: VocabId) -> Option<&str>;

    /// Get the number of unique words.
    fn vocab_size(&self) -> usize;

    /// Check if a word has been interned.
    fn contains(&self, word: &str) -> bool;

    /// Get ID without interning.
    fn get_id(&self, word: &str) -> Option<VocabId>;

    /// Iterate over all vocabulary entries.
    fn iter(&self) -> impl Iterator<Item = (VocabId, &str)>;

    /// Check if this backend supports structural sharing.
    fn supports_sharing(&self) -> bool { false }
}
```

### Key Operations

| Operation | Time | Description |
|-----------|------|-------------|
| `intern()` | O(1)* | Add word or return existing ID |
| `lookup()` | O(1) | Get word by ID |
| `contains()` | O(1)* | Check if word exists |
| `get_id()` | O(1)* | Get ID without interning |
| `iter()` | O(n) | Iterate all entries |

*Amortized for hash-based implementations

## HashMapBackend

The default backend for single-process, in-memory use:

```rust
use lling_llang::backend::{LatticeBackend, HashMapBackend};

let mut backend = HashMapBackend::new();

// Intern words
let id1 = backend.intern("hello");  // 0
let id2 = backend.intern("world");  // 1
let id3 = backend.intern("hello");  // 0 (same as id1)

// Lookup
assert_eq!(backend.lookup(id1), Some("hello"));
assert_eq!(backend.lookup(id2), Some("world"));

// Check existence
assert!(backend.contains("hello"));
assert!(!backend.contains("goodbye"));

// Vocabulary size
assert_eq!(backend.vocab_size(), 2);
```

### Implementation Details

```rust
pub struct HashMapBackend {
    word_to_id: FxHashMap<Arc<str>, VocabId>,
    id_to_word: Vec<Arc<str>>,
}
```

**Storage**:
- `FxHashMap`: Fast Rust hash map for string → ID
- `Vec`: Sequential storage for ID → string
- `Arc<str>`: Shared ownership, avoids duplication

**Characteristics**:
- O(1) intern and lookup
- Sequential IDs (0, 1, 2, ...)
- No structural sharing (`supports_sharing() = false`)
- Thread-safe via `Arc<str>`

### Pre-allocation

For known vocabulary sizes:

```rust
// Pre-allocate for ~1000 words
let backend = HashMapBackend::with_capacity(1000);

// Or reserve incrementally
let mut backend = HashMapBackend::new();
backend.reserve(500);
```

## PathMapBackend

For distributed storage with structural sharing (requires `f1r3fly` feature):

```rust
// Cargo.toml: lling-llang = { features = ["f1r3fly"] }

use lling_llang::backend::PathMapBackend;

let backend = PathMapBackend::new();
assert!(backend.supports_sharing());
```

### Structural Sharing

PathMap enables copy-on-write sharing between lattices:

```rust
use lling_llang::backend::PathMapSharingBackend;

let backend1 = PathMapBackend::new();
let backend2 = backend1.clone();

// Clones share the same underlying storage
assert!(backend1.shares_structure_with(&backend2));
```

Benefits:
- Multiple lattices share common vocabulary
- Modifications use copy-on-write
- Efficient for distributed processing

### When to Use PathMapBackend

| Use Case | Backend |
|----------|---------|
| Single process, small vocab | `HashMapBackend` |
| Single process, large vocab | `HashMapBackend` |
| Distributed processing | `PathMapBackend` |
| Persistent storage | `PathMapBackend` |
| F1R3FLY.io integration | `PathMapBackend` |

## Implementing Custom Backends

Create a backend for your storage system:

```rust
use lling_llang::backend::{LatticeBackend, VocabId};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct MyBackend {
    words: Vec<String>,
    ids: HashMap<String, VocabId>,
}

impl LatticeBackend for MyBackend {
    fn intern(&mut self, word: &str) -> VocabId {
        if let Some(id) = self.ids.get(word) {
            return *id;
        }

        let id = self.words.len() as VocabId;
        self.words.push(word.to_owned());
        self.ids.insert(word.to_owned(), id);
        id
    }

    fn lookup(&self, id: VocabId) -> Option<&str> {
        self.words.get(id as usize).map(String::as_str)
    }

    fn vocab_size(&self) -> usize {
        self.words.len()
    }

    fn contains(&self, word: &str) -> bool {
        self.ids.contains_key(word)
    }

    fn get_id(&self, word: &str) -> Option<VocabId> {
        self.ids.get(word).copied()
    }

    fn iter(&self) -> impl Iterator<Item = (VocabId, &str)> {
        self.words
            .iter()
            .enumerate()
            .map(|(id, word)| (id as VocabId, word.as_str()))
    }

    fn supports_sharing(&self) -> bool {
        false // or true for shared backends
    }
}
```

### Requirements

1. **Clone**: Backends must be cloneable
2. **Send + Sync**: Thread-safe access
3. **Deterministic IDs**: Same word → same ID always
4. **Sequential or sparse**: IDs can be sequential or sparse

## Details

### Thread Safety

All backends must implement `Send + Sync`:

```rust
pub trait LatticeBackend: Clone + Send + Sync { ... }
```

For mutable access from multiple threads, wrap in a synchronization primitive:

```rust
use std::sync::Mutex;

let backend = Mutex::new(HashMapBackend::new());

// Thread-safe interning
{
    let mut b = backend.lock().unwrap();
    let id = b.intern("word");
}
```

Or use the lattice builder pattern (single-threaded construction, shared read-only access after).

### ID Stability

IDs are stable within a backend instance:

```rust
let mut backend = HashMapBackend::new();
let id1 = backend.intern("hello");
let id2 = backend.intern("hello");
assert_eq!(id1, id2);  // Always equal
```

But IDs may differ across instances:

```rust
let mut backend1 = HashMapBackend::new();
backend1.intern("world");
let id1 = backend1.intern("hello");  // 1

let mut backend2 = HashMapBackend::new();
let id2 = backend2.intern("hello");  // 0

assert_ne!(id1, id2);  // Different instances, different IDs
```

### Unicode Handling

Backends handle arbitrary UTF-8 strings:

```rust
let mut backend = HashMapBackend::new();

let id1 = backend.intern("hello");     // ASCII
let id2 = backend.intern("héllo");     // Accent
let id3 = backend.intern("你好");       // Chinese
let id4 = backend.intern("🦀");         // Emoji

assert_eq!(backend.lookup(id3), Some("你好"));
```

### Empty Strings

Empty strings are valid vocabulary entries:

```rust
let mut backend = HashMapBackend::new();
let id = backend.intern("");
assert_eq!(backend.lookup(id), Some(""));
```

This is useful for epsilon transitions in WFSTs.

## Common Patterns

### Vocabulary Pre-loading

Load vocabulary before building lattices:

```rust
let mut backend = HashMapBackend::with_capacity(vocabulary.len());

for word in vocabulary {
    backend.intern(word);
}

// Now build lattices with pre-interned vocabulary
let builder = LatticeBuilder::new(backend);
```

### Path to Words Conversion

Convert a path of edge IDs to words:

```rust
fn path_to_words<W: Semiring, B: LatticeBackend>(
    path: &LatticePath<W>,
    lattice: &Lattice<W, B>,
) -> Vec<String> {
    path.edges
        .iter()
        .filter_map(|&edge_id| {
            lattice.edge(edge_id).and_then(|edge| {
                lattice.backend().lookup(edge.label).map(String::from)
            })
        })
        .collect()
}
```

### Vocabulary Statistics

Analyze vocabulary distribution:

```rust
fn vocab_stats(backend: &HashMapBackend) {
    let mut lengths: Vec<usize> = backend.iter()
        .map(|(_, word)| word.len())
        .collect();
    lengths.sort();

    println!("Vocabulary size: {}", backend.vocab_size());
    println!("Min word length: {}", lengths.first().unwrap_or(&0));
    println!("Max word length: {}", lengths.last().unwrap_or(&0));
    println!("Median length: {}", lengths[lengths.len() / 2]);
}
```

## Related Topics

- [PathMap Integration](../integration/f1r3fly/pathmap-backend.md): Distributed storage details
- [Lattices](lattices.md): How backends are used in lattice construction
- [API Reference](../api/backend-reference.md): Complete API documentation

## References

Full entries — including DOIs — are in [`BIBLIOGRAPHY.md`](../BIBLIOGRAPHY.md).

- [**Mohri 2009**](../BIBLIOGRAPHY.md#ref-mohri2009) — Mohri, *Weighted Automata Algorithms*: symbol tables and label interning as a precondition for efficient automaton operations. [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6)
- [**Allauzen 2007**](../BIBLIOGRAPHY.md#ref-allauzen2007) — Allauzen et al., *OpenFst*: the `SymbolTable` abstraction that `LatticeBackend` generalizes (in-memory vs. content-addressed/shared stores). [doi:10.1007/978-3-540-76336-9_3](https://doi.org/10.1007/978-3-540-76336-9_3)
