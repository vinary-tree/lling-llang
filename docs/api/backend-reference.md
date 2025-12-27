# Backend API Reference

Complete API reference for lattice backend types.

## LatticeBackend Trait

Storage abstraction for lattice vocabulary.

```rust
pub trait LatticeBackend: Clone + Send + Sync {
    /// Intern a string, returning its vocabulary ID
    fn intern(&mut self, word: &str) -> VocabId;

    /// Look up a string by vocabulary ID
    fn lookup(&self, id: VocabId) -> Option<&str>;

    /// Get vocabulary size
    fn vocab_size(&self) -> usize;

    /// Check if a word is already interned
    fn contains(&self, word: &str) -> bool;

    /// Get vocabulary ID for a word (without interning)
    fn get_id(&self, word: &str) -> Option<VocabId>;

    /// Check if backend supports structural sharing
    fn supports_sharing(&self) -> bool {
        false
    }

    /// Get synchronization strategy
    fn sync_strategy(&self) -> SyncStrategy {
        SyncStrategy::ExternalSync
    }
}
```

## VocabId

Vocabulary identifier.

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VocabId(pub u32);

impl VocabId {
    /// Create from index
    pub fn new(index: usize) -> Self;

    /// Get index as usize
    pub fn index(&self) -> usize;

    /// Get raw u32 value
    pub fn raw(&self) -> u32;
}

impl From<usize> for VocabId {
    fn from(index: usize) -> Self {
        VocabId(index as u32)
    }
}
```

## SyncStrategy

Synchronization requirements.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncStrategy {
    /// Requires external synchronization (e.g., mutex)
    ExternalSync,

    /// Internally synchronized (safe for concurrent access)
    InternalSync,

    /// Immutable with structural sharing (copy-on-write)
    Persistent,
}
```

## HashMapBackend

In-memory hash-based backend.

```rust
pub struct HashMapBackend {
    words: Vec<String>,
    index: HashMap<String, VocabId>,
}

impl HashMapBackend {
    /// Create new empty backend
    pub fn new() -> Self;

    /// Create with initial capacity
    pub fn with_capacity(capacity: usize) -> Self;

    /// Create from existing vocabulary
    pub fn from_vocabulary(words: Vec<String>) -> Self;

    /// Iterate over all words
    pub fn words(&self) -> impl Iterator<Item = &str>;

    /// Get all words as a vector
    pub fn to_vec(&self) -> Vec<String>;

    /// Clear all entries
    pub fn clear(&mut self);

    /// Reserve additional capacity
    pub fn reserve(&mut self, additional: usize);

    /// Shrink to fit
    pub fn shrink_to_fit(&mut self);
}
```

### Usage

```rust
use lling_llang::backend::HashMapBackend;

let mut backend = HashMapBackend::new();

// Intern words
let id1 = backend.intern("hello");
let id2 = backend.intern("world");
let id3 = backend.intern("hello");  // Returns same ID as id1

assert_eq!(id1, id3);
assert_ne!(id1, id2);

// Look up by ID
assert_eq!(backend.lookup(id1), Some("hello"));
assert_eq!(backend.lookup(id2), Some("world"));

// Check existence
assert!(backend.contains("hello"));
assert!(!backend.contains("foo"));
```

### Properties

| Property | Value |
|----------|-------|
| Intern | O(1) average |
| Lookup | O(1) |
| Memory | O(n) where n = unique words |
| Thread Safety | ExternalSync |
| Persistence | No |
| Sharing | No |

## VecBackend

Simple vector-based backend.

```rust
pub struct VecBackend {
    words: Vec<String>,
}

impl VecBackend {
    /// Create new empty backend
    pub fn new() -> Self;

    /// Create with initial capacity
    pub fn with_capacity(capacity: usize) -> Self;
}
```

### Properties

| Property | Value |
|----------|-------|
| Intern | O(n) (linear search) |
| Lookup | O(1) |
| Memory | O(n) |
| Thread Safety | ExternalSync |
| Use Case | Small vocabularies |

## PathMapBackend (Planned)

Distributed PathMap backend.

```rust
pub struct PathMapBackend {
    // F1R3FLY.io PathMap connection
}

impl PathMapBackend {
    /// Connect to PathMap cluster
    pub fn connect(endpoint: &str) -> Result<Self, Error>;

    /// Builder for configuration
    pub fn builder() -> PathMapBackendBuilder;

    /// Persist lattice to PathMap
    pub fn persist<W: Semiring>(&mut self, lattice: &Lattice<W, Self>) -> Result<LatticeId, Error>;

    /// Load lattice from PathMap
    pub fn load<W: Semiring>(&self, id: LatticeId) -> Result<Lattice<W, Self>, Error>;

    /// Prefetch vocabulary IDs
    pub fn prefetch(&mut self, ids: &[VocabId]) -> Result<(), Error>;

    /// Batch intern
    pub fn intern_batch(&mut self, words: &[&str]) -> Vec<VocabId>;

    /// Batch lookup
    pub fn lookup_batch(&self, ids: &[VocabId]) -> Vec<Option<&str>>;
}
```

### PathMapBackendBuilder

```rust
pub struct PathMapBackendBuilder {
    // Configuration
}

impl PathMapBackendBuilder {
    /// Set endpoint
    pub fn endpoint(self, endpoint: &str) -> Self;

    /// Set connection timeout
    pub fn timeout(self, duration: Duration) -> Self;

    /// Set retry policy
    pub fn retry_policy(self, policy: RetryPolicy) -> Self;

    /// Set cache size
    pub fn cache_size(self, size: usize) -> Self;

    /// Set cache policy
    pub fn cache_policy(self, policy: CachePolicy) -> Self;

    /// Build the backend
    pub fn build(self) -> Result<PathMapBackend, Error>;
}
```

### Properties

| Property | Value |
|----------|-------|
| Intern | O(1) + network |
| Lookup | O(1) cached / network |
| Memory | Bounded by cache |
| Thread Safety | InternalSync |
| Persistence | Yes |
| Sharing | Yes (structural) |

## SharedBackend

Wrapper for shared ownership.

```rust
pub struct SharedBackend<B: LatticeBackend> {
    inner: Arc<RwLock<B>>,
}

impl<B: LatticeBackend> SharedBackend<B> {
    /// Wrap a backend for shared access
    pub fn new(backend: B) -> Self;

    /// Get inner backend (exclusive access)
    pub fn into_inner(self) -> B;
}
```

### Usage

```rust
use lling_llang::backend::{HashMapBackend, SharedBackend};
use std::sync::Arc;

let backend = SharedBackend::new(HashMapBackend::new());

// Clone for use in multiple threads
let backend1 = backend.clone();
let backend2 = backend.clone();

// Safe concurrent access
std::thread::spawn(move || {
    backend1.intern("hello");
});
std::thread::spawn(move || {
    backend2.intern("world");
});
```

## CachedBackend

Caching wrapper for remote backends.

```rust
pub struct CachedBackend<B: LatticeBackend> {
    inner: B,
    cache: LruCache<VocabId, String>,
    reverse_cache: LruCache<String, VocabId>,
}

impl<B: LatticeBackend> CachedBackend<B> {
    /// Create with cache size
    pub fn new(backend: B, cache_size: usize) -> Self;

    /// Clear cache
    pub fn clear_cache(&mut self);

    /// Get cache hit rate
    pub fn hit_rate(&self) -> f64;

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats;
}

pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub size: usize,
    pub capacity: usize,
}
```

## Backend Comparison

| Backend | Intern | Lookup | Memory | Thread Safe | Persistent | Distributed |
|---------|--------|--------|--------|-------------|------------|-------------|
| HashMapBackend | O(1) | O(1) | High | No | No | No |
| VecBackend | O(n) | O(1) | Low | No | No | No |
| PathMapBackend | Net | Net/O(1) | Bounded | Yes | Yes | Yes |
| SharedBackend | O(1)* | O(1)* | High | Yes | No | No |
| CachedBackend | Net | O(1)/Net | Medium | No | Depends | Depends |

## Utility Functions

```rust
/// Copy vocabulary from one backend to another
pub fn copy_vocabulary<B1, B2>(from: &B1, to: &mut B2) -> HashMap<VocabId, VocabId>
where
    B1: LatticeBackend,
    B2: LatticeBackend;

/// Merge vocabularies
pub fn merge_vocabularies<B>(backends: &[&B]) -> HashMapBackend
where
    B: LatticeBackend;

/// Export vocabulary to file
pub fn export_vocabulary<B: LatticeBackend>(
    backend: &B,
    path: impl AsRef<Path>,
) -> io::Result<()>;

/// Import vocabulary from file
pub fn import_vocabulary(path: impl AsRef<Path>) -> io::Result<HashMapBackend>;
```

## See Also

- [Backends (Architecture)](../architecture/backends.md): Conceptual overview
- [Lattice Reference](lattice-reference.md): Using backends with lattices
- [PathMap Backend](../integration/f1r3fly/pathmap-backend.md): F1R3FLY.io integration
