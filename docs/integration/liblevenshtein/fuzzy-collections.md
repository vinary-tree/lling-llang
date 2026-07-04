# liblevenshtein Fuzzy Collections

liblevenshtein provides fuzzy maps and caches that support edit-distance-aware lookups with eviction strategies.

## Concepts

### Fuzzy Lookup

Unlike exact-match collections, fuzzy collections find entries within an edit distance:

```rust
// Exact lookup
map.get("test");  // Only matches "test"

// Fuzzy lookup
fuzzy_map.query("tset", 1);  // Matches "test", "text", etc.
```

### Eviction Strategies

For caches with limited capacity, eviction strategies decide which entries to remove:

| Strategy | Evicts | Use Case |
|----------|--------|----------|
| LRU | Least recently used | General caching |
| LFU | Least frequently used | Hot-spot access |
| TTL | Expired entries | Time-sensitive data |
| FIFO | Oldest entries | Stream processing |
| Cost-aware | Low value entries | Memory pressure |

## FuzzyMultiMap

Aggregates values from multiple fuzzy matches.

### CollectionAggregate Trait

Defines how to combine values:

```rust
pub trait CollectionAggregate: Clone + Default {
    fn aggregate(&mut self, other: &Self);
}
```

Built-in implementations:
- `HashSet<T>`: Union of sets
- `BTreeSet<T>`: Union of sets
- `Vec<T>`: Concatenation

### Basic Usage

```rust
use std::collections::HashSet;
use liblevenshtein::cache::multimap::FuzzyMultiMap;
use liblevenshtein::dictionary::DynamicDawgChar;

// Create dictionary with set values
let mut dict: DynamicDawgChar<HashSet<i32>> = DynamicDawgChar::new();
dict.insert_with_value("hello", HashSet::from([1, 2]));
dict.insert_with_value("hallo", HashSet::from([3]));
dict.insert_with_value("hullo", HashSet::from([4, 5]));

// Create fuzzy multimap
let fuzzy = FuzzyMultiMap::new(dict, Algorithm::Standard);

// Query returns aggregated values
let result = fuzzy.query("helo", 1);
// Returns Some({1, 2, 3, 4, 5}) - union of all matches
```

### With Vec Values

```rust
use liblevenshtein::cache::multimap::FuzzyMultiMap;

let mut dict: DynamicDawgChar<Vec<String>> = DynamicDawgChar::new();
dict.insert_with_value("cat", vec!["feline".into()]);
dict.insert_with_value("car", vec!["vehicle".into()]);
dict.insert_with_value("bat", vec!["mammal".into(), "sports".into()]);

let fuzzy = FuzzyMultiMap::new(dict, Algorithm::Standard);

let result = fuzzy.query("cat", 1);
// Returns Some(["feline", "vehicle"]) - concatenation of matches for "cat" and "car"
```

## Eviction Wrappers

Decorators that add eviction behavior to dictionaries.

### Architecture

```
┌─────────────────────────────────────┐
│     Eviction Wrapper Stack          │
│  ┌─────────────────────────────┐    │
│  │         LRU Wrapper          │   │
│  │  ┌─────────────────────────┐ │   │
│  │  │      TTL Wrapper        │ │   │
│  │  │  ┌────────────────────┐ │ │   │
│  │  │  │ Base Dictionary    │ │ │   │
│  │  │  └────────────────────┘ │ │   │
│  │  └─────────────────────────┘ │   │
│  └─────────────────────────────┘    │
└─────────────────────────────────────┘
```

Wrappers can be composed:

```rust
use liblevenshtein::cache::eviction::{Lru, Ttl};

let dict = DynamicDawgChar::new();
let ttl = Ttl::new(dict, Duration::from_secs(3600));  // 1 hour TTL
let lru = Lru::new(ttl);  // LRU on top of TTL
```

### Noop Wrapper

Zero-cost passthrough:

```rust
use liblevenshtein::cache::eviction::Noop;

let wrapped = Noop::new(dict);
// Behaves exactly like dict, no overhead
```

### LazyInit Wrapper

Deferred dictionary initialization:

```rust
use liblevenshtein::cache::eviction::LazyInit;

// Initialize on first access
let lazy = LazyInit::new(|| {
    let dict = DynamicDawgChar::new();
    // Expensive initialization...
    dict
});

// Dictionary not created until first use
lazy.contains("test");  // Now initialized
```

### TTL (Time-to-Live)

Expires entries after duration:

```rust
use liblevenshtein::cache::eviction::Ttl;
use std::time::Duration;

let dict = DynamicDawgChar::new();
let ttl = Ttl::new(dict, Duration::from_secs(60));

ttl.insert("hello");

// After 60 seconds...
assert!(!ttl.contains("hello"));  // Expired
```

### Age (FIFO)

First-in-first-out eviction:

```rust
use liblevenshtein::cache::eviction::Age;

let dict = DynamicDawgChar::new();
let fifo = Age::new(dict);

fifo.insert("first");
fifo.insert("second");
fifo.insert("third");

// Find oldest entry
let oldest = fifo.find_oldest(&["first", "second", "third"]);
assert_eq!(oldest, Some("first".to_string()));
```

### LRU (Least Recently Used)

Tracks access recency:

```rust
use liblevenshtein::cache::eviction::Lru;

let dict = DynamicDawgChar::new();
let lru = Lru::new(dict);

lru.insert("hello");
lru.insert("world");

// Access updates recency
lru.get_value("hello");  // "hello" is now most recent

// Find least recently used
let victim = lru.find_lru(&["hello", "world"]);
assert_eq!(victim, Some("world".to_string()));
```

### LruOptimized

Optimized LRU implementation:

```rust
use liblevenshtein::cache::eviction::LruOptimized;

let lru = LruOptimized::new(dict);
// Same API as Lru, better performance for large dictionaries
```

### LFU (Least Frequently Used)

Tracks access frequency:

```rust
use liblevenshtein::cache::eviction::Lfu;

let dict = DynamicDawgChar::new();
let lfu = Lfu::new(dict);

lfu.insert("hello");
lfu.insert("world");

// Access increases frequency
lfu.get_value("hello");
lfu.get_value("hello");
lfu.get_value("hello");
lfu.get_value("world");

// Find least frequently used
let victim = lfu.find_lfu(&["hello", "world"]);
assert_eq!(victim, Some("world".to_string()));  // 1 access vs 3
```

### CostAware

Balances age, size, and hit count:

```rust
use liblevenshtein::cache::eviction::CostAware;

let dict = DynamicDawgChar::new();
let cost = CostAware::new(dict);

// Cost formula: (age * size) / (hits + 1)
// Higher cost = more likely to evict

let victim = cost.find_highest_cost(&["small_hot", "large_cold"]);
```

### MemoryPressure

Memory-aware eviction:

```rust
use liblevenshtein::cache::eviction::MemoryPressure;

let dict = DynamicDawgChar::new();
let pressure = MemoryPressure::new(dict);

// Cost formula: size / (hit_rate + 0.1)
// Large entries with low hit rate evicted first

let victim = pressure.find_highest_pressure(&["small", "large"]);
```

## Composing Wrappers

### Example: LRU + TTL + Memory

```rust
use liblevenshtein::cache::eviction::{Lru, Ttl, MemoryPressure};
use std::time::Duration;

let base = DynamicDawgChar::new();

// Stack wrappers
let with_memory = MemoryPressure::new(base);
let with_ttl = Ttl::new(with_memory, Duration::from_secs(3600));
let cache = Lru::new(with_ttl);

// Entries:
// - Expire after 1 hour (TTL)
// - LRU tracking for eviction selection
// - Memory pressure as secondary metric
```

### Eviction Selection

When evicting, check each wrapper:

```rust
fn select_victim<D>(cache: &Lru<Ttl<D>>, candidates: &[&str]) -> Option<String> {
    // First: evict expired entries
    if let Some(expired) = cache.inner().find_expired(candidates) {
        return Some(expired);
    }

    // Then: evict LRU entry
    cache.find_lru(candidates)
}
```

## Wrapper Metadata

All wrappers maintain metadata:

```rust
pub struct Metadata {
    pub inserted_at: Instant,
    pub last_accessed: Instant,
    pub access_count: usize,
    pub size_bytes: usize,
}
```

Access metadata:

```rust
let lru = Lru::new(dict);
lru.insert("hello");

if let Some(meta) = lru.metadata("hello") {
    println!("Inserted: {:?}", meta.inserted_at);
    println!("Last accessed: {:?}", meta.last_accessed);
    println!("Access count: {}", meta.access_count);
}
```

## Thread Safety

All eviction wrappers use `Arc<RwLock<>>` for metadata:

```rust
// Safe for concurrent access
let cache = Arc::new(Lru::new(dict));

let cache1 = cache.clone();
let cache2 = cache.clone();

// Concurrent reads
thread::spawn(move || { cache1.contains("hello"); });
thread::spawn(move || { cache2.contains("world"); });
```

## Performance Considerations

### Wrapper Overhead

| Wrapper | Overhead | Notes |
|---------|----------|-------|
| Noop | 0% | Zero-cost passthrough |
| LazyInit | 0% after init | One-time init cost |
| TTL | ~5% | Timestamp check |
| Age | ~5% | Timestamp tracking |
| LRU | ~10% | Access reordering |
| LFU | ~10% | Counter updates |
| CostAware | ~15% | Multiple metrics |
| MemoryPressure | ~20% | Size tracking |

### Composition Depth

Each wrapper layer adds overhead:

```rust
// Fast: 1 wrapper
let cache = Lru::new(dict);

// Medium: 2 wrappers
let cache = Lru::new(Ttl::new(dict, ttl));

// Slow: 3+ wrappers
let cache = Lru::new(Ttl::new(MemoryPressure::new(dict), ttl));
```

Keep wrapper stacks shallow for performance-critical paths.

## Usage Patterns

### LRU Cache with Capacity

```rust
fn maintain_capacity<D: MappedDictionary>(
    cache: &mut Lru<D>,
    max_entries: usize,
) {
    while cache.len() > max_entries {
        let terms: Vec<_> = cache.iter_terms().collect();
        if let Some(victim) = cache.find_lru(&terms) {
            cache.remove(&victim);
        }
    }
}
```

### TTL-Based Cleanup

```rust
fn cleanup_expired<D: MappedDictionary>(cache: &Ttl<D>) {
    let terms: Vec<_> = cache.iter_terms().collect();
    for term in terms {
        if cache.is_expired(&term) {
            cache.remove(&term);
        }
    }
}
```

### Adaptive Eviction

```rust
fn adaptive_evict<D: MappedDictionary>(
    cache: &CostAware<D>,
    memory_limit: usize,
) {
    while cache.memory_usage() > memory_limit {
        let terms: Vec<_> = cache.iter_terms().collect();
        if let Some(victim) = cache.find_highest_cost(&terms) {
            cache.remove(&victim);
        } else {
            break;
        }
    }
}
```

## Related Topics

- [Overview](overview.md): Architecture overview
- [Dictionaries](dictionaries.md): Dictionary implementations
- [Transducers](transducers.md): Fuzzy matching API
- [Integration](lling-llang-integration.md): Using with lling-llang
