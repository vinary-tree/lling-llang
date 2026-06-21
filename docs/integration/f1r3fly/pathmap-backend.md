# PathMap Backend

PathMap provides distributed, content-addressed storage with structural sharing for lling-llang lattices.

## Overview

**PathMap** is F1R3FLY.io's distributed storage system built on content-addressed data structures. The `PathMapBackend` implements `LatticeBackend` to enable:

- **Distributed vocabulary**: Share vocabulary across cluster nodes
- **Structural sharing**: Copy-on-write for efficient lattice cloning
- **Content addressing**: Automatic deduplication of identical substructures
- **Persistence**: Lattices survive process restarts

## Concepts

### Content-Addressed Storage

PathMap stores data by its cryptographic hash:

```
"hello" → hash("hello") → 0xabc123...
"world" → hash("world") → 0xdef456...
```

Same content = same hash = single storage location. This provides:
- **Deduplication**: Identical strings stored once
- **Immutability**: Content never changes (new version = new hash)
- **Verification**: Hash verifies content integrity

### Structural Sharing

When cloning a lattice:

```
Original Lattice                Clone
┌──────────────────┐           ┌──────────────────┐
│ nodes: [...]     │──────────►│ nodes: [...]     │ (shared)
│ edges: [...]     │──────────►│ edges: [...]     │ (shared)
│ vocabulary: ref  │──────────►│ vocabulary: ref  │ (shared)
└──────────────────┘           └──────────────────┘

                    │
                    ▼ (modification to clone)

Original Lattice                Clone (after modification)
┌──────────────────┐           ┌──────────────────┐
│ nodes: [...]     │           │ nodes: [...]     │ (copy-on-write)
│ edges: [...]     │──────────►│ edges: [...]     │ (still shared)
│ vocabulary: ref  │──────────►│ vocabulary: ref  │ (still shared)
└──────────────────┘           └──────────────────┘
```

Clones share structure until modified, then copy only the modified portion.

### Distributed Access

PathMap distributes data across cluster nodes. The component view below shows the
end-to-end path: a `LatticeBuilder` calls through the `LatticeBackend` interface
into `PathMapBackend`, which checks a local LRU vocabulary cache and, on a miss,
reaches the cluster's content-addressing layer that shards interned words across
nodes and deduplicates identical substructures via copy-on-write.

![Component view of PathMapBackend: lling-llang's LatticeBuilder talks through the LatticeBackend interface to PathMapBackend, which fronts a local LRU cache; cache misses reach the PathMap cluster's content-addressing and structural-sharing components that shard data across three storage nodes.](../../diagrams/integration/pathmap-backend.svg)

*Blue = lling-llang components and the PathMap cluster boundary; amber = the
local vocabulary cache; green = storage-node databases; grey = the
`LatticeBackend` interface. Dotted edges denote content-addressed deduplication.
Forward-looking integration **target**.*

<details><summary>Text view</summary>

```text
┌─────────────────────────────────────────────────────────────┐
│                    PathMap Cluster                          │
│   ┌─────────┐     ┌─────────┐     ┌─────────┐              │
│   │ Node 1  │     │ Node 2  │     │ Node 3  │              │
│   │ "hello" │◄───►│ "world" │◄───►│ "foo"   │              │
│   │ "bar"   │     │ "baz"   │     │ "qux"   │              │
│   └─────────┘     └─────────┘     └─────────┘              │
└─────────────────────────────────────────────────────────────┘
         ▲               ▲               ▲
         │               │               │
    ┌────┴────┐     ┌────┴────┐     ┌────┴────┐
    │ Client  │     │ Client  │     │ Client  │
    │ lling-  │     │ lling-  │     │ lling-  │
    │ llang   │     │ llang   │     │ llang   │
    └─────────┘     └─────────┘     └─────────┘
```

</details>

## Planned API

### Connection

```rust
use lling_llang::backend::PathMapBackend;

// Connect to PathMap cluster
let backend = PathMapBackend::connect("pathmap://cluster:8080")?;

// Or with configuration
let backend = PathMapBackend::builder()
    .endpoint("pathmap://cluster:8080")
    .timeout(Duration::from_secs(30))
    .retry_policy(RetryPolicy::exponential(3))
    .build()?;
```

### Backend Operations

```rust
impl LatticeBackend for PathMapBackend {
    fn intern(&mut self, word: &str) -> VocabId {
        // Hash word, store in PathMap, return ID
    }

    fn lookup(&self, id: VocabId) -> Option<&str> {
        // Fetch from PathMap by ID (cached locally)
    }

    fn vocab_size(&self) -> usize {
        // Return local + remote vocabulary size
    }

    fn supports_sharing(&self) -> bool {
        true  // PathMap supports structural sharing
    }
}
```

### Structural Sharing

```rust
use lling_llang::backend::PathMapSharingBackend;

let backend1 = PathMapBackend::connect("pathmap://cluster")?;
let backend2 = backend1.clone();

// Clones share the same underlying storage
assert!(backend1.shares_structure_with(&backend2));

// Modifications use copy-on-write
backend2.intern("new_word");  // Creates new version
```

### Persistence

```rust
// Save lattice to PathMap
let lattice_id = backend.persist(&lattice)?;

// Later: load lattice from PathMap
let restored = backend.load::<TropicalWeight>(lattice_id)?;
```

## Integration with Lattices

### Building Distributed Lattices

```rust
let backend = PathMapBackend::connect("pathmap://cluster")?;
let mut builder = LatticeBuilder::new(backend);

builder.add_correction(0, 1, "the", weight, meta);
builder.add_correction(1, 2, "dog", weight, meta);

let lattice = builder.build(2);

// Lattice vocabulary stored in PathMap
// Can be accessed from any cluster node
```

### Sharing Across Nodes

```rust
// Node A: create lattice
let backend_a = PathMapBackend::connect("pathmap://cluster")?;
let mut builder = LatticeBuilder::new(backend_a);
// ... build lattice ...
let lattice_id = backend_a.persist(&lattice)?;

// Node B: use same lattice
let backend_b = PathMapBackend::connect("pathmap://cluster")?;
let lattice = backend_b.load(lattice_id)?;

// Both nodes share the same vocabulary storage
```

## Performance Considerations

### Caching

The `PathMapBackend` maintains a local cache:

```rust
let backend = PathMapBackend::builder()
    .endpoint("pathmap://cluster")
    .cache_size(10_000)  // Cache up to 10K vocabulary entries
    .cache_policy(LruCache)
    .build()?;
```

### Batching

Batch operations for efficiency:

```rust
// Batch intern multiple words
let ids = backend.intern_batch(&["the", "quick", "brown", "fox"]);

// Batch lookup
let words = backend.lookup_batch(&[0, 1, 2, 3]);
```

### Prefetching

Prefetch vocabulary for known paths:

```rust
// Before path extraction, prefetch likely vocabulary
backend.prefetch(&[0, 1, 2, 3, 4])?;

// Path extraction hits local cache
let paths = nbest(&mut lattice, 10);
```

## Comparison with HashMapBackend

| Feature | HashMapBackend | PathMapBackend |
|---------|----------------|----------------|
| Storage | In-memory | Distributed |
| Sharing | No | Yes (structural) |
| Persistence | No | Yes |
| Scalability | Single process | Cluster-wide |
| Latency | Nanoseconds | Microseconds* |
| Throughput | Very high | High* |

*With local caching

### When to Use PathMapBackend

- Distributed processing across multiple nodes
- Need for persistence across sessions
- Large vocabularies that don't fit in memory
- Structural sharing for memory efficiency

### When to Use HashMapBackend

- Single-process applications
- Low-latency requirements
- Simple deployment without cluster
- Development and testing

## Current Status

**Status**: Planned

The `PathMapBackend` is planned but not yet implemented. Current blockers:

1. PathMap Rust bindings not yet available
2. Content-addressing scheme for lattices not finalized
3. Caching strategy needs benchmarking

## Next Steps

- [Vision](vision.md): Overall F1R3FLY.io integration
- [MeTTaIL Layer](mettail-layer.md): Type-based filtering
- [Backends](../../architecture/backends.md): Backend trait documentation
