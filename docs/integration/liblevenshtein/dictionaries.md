# liblevenshtein Dictionaries

liblevenshtein provides multiple dictionary implementations optimized for different use cases.

## Dictionary Trait Hierarchy

```
Dictionary ─────────────────────────────────────────────────────┐
    │                                                           │
    ├── MappedDictionary ───────────────────────────────────────┤
    │       │                                                   │
    │       └── MutableMappedDictionary                         │
    │                                                           │
DictionaryNode ──────────────────────────────────────────────────┤
    │                                                           │
    └── MappedDictionaryNode                                    │
                                                                │
CharUnit ─────────────────────────────────────────────────────────┘
    ├── u8  (bytes, ASCII)
    └── char (Unicode codepoints)
```

### Dictionary Trait

```rust
pub trait Dictionary {
    /// Node type for traversal
    type Node: DictionaryNode;

    /// Get the root node
    fn root(&self) -> Self::Node;

    /// Check if term exists
    fn contains(&self, term: &str) -> bool;

    /// Get dictionary size
    fn len(&self) -> Option<usize>;

    /// Check if empty
    fn is_empty(&self) -> bool;

    /// Thread safety strategy
    fn sync_strategy() -> SyncStrategy;

    /// Whether dictionary supports substring matching
    fn is_suffix_based() -> bool;
}
```

### DictionaryNode Trait

```rust
pub trait DictionaryNode {
    /// Character unit type (u8 or char)
    type Unit: CharUnit;

    /// Check if this node marks end of a term
    fn is_final(&self) -> bool;

    /// Follow edge with given label
    fn transition(&self, label: Self::Unit) -> Option<Self>;

    /// Iterate over outgoing edges
    fn edges(&self) -> Box<dyn Iterator<Item = (Self::Unit, Self)>>;

    /// Check if edge exists
    fn has_edge(&self, label: Self::Unit) -> bool;

    /// Count outgoing edges
    fn edge_count(&self) -> Option<usize>;
}
```

### MappedDictionary Trait

Associates values with terms:

```rust
pub trait MappedDictionary: Dictionary {
    /// Value type associated with terms
    type Value: DictionaryValue;

    /// Get value for term
    fn get_value(&self, term: &str) -> Option<Self::Value>;

    /// Check term with value predicate
    fn contains_with_value<F>(&self, term: &str, predicate: F) -> bool
    where
        F: Fn(&Self::Value) -> bool;
}
```

### MutableMappedDictionary Trait

Enables modifications:

```rust
pub trait MutableMappedDictionary: MappedDictionary {
    /// Insert or update term with value
    fn insert_with_value(&mut self, term: &str, value: Self::Value) -> bool;

    /// Union with another dictionary
    fn union_with<F>(&mut self, other: &Self, merge_fn: F) -> usize
    where
        F: Fn(&Self::Value, &Self::Value) -> Self::Value;

    /// Union keeping right value on conflict
    fn union_replace(&mut self, other: &Self) -> usize;

    /// Update existing or insert new
    fn update_or_insert<F>(
        &mut self,
        term: &str,
        default: Self::Value,
        update_fn: F
    ) -> bool
    where
        F: Fn(&mut Self::Value);
}
```

## Dictionary Implementations

### DoubleArrayTrie

Fast, compact dictionary for static or append-only use cases.

**Characteristics**:
- Query: 3x faster than DynamicDawg
- Contains: 30x faster than DynamicDawg
- Memory: ~8 bytes per state (most compact)
- Updates: Append-only insertions

**Usage**:

```rust
use liblevenshtein::dictionary::DoubleArrayTrie;

// From terms
let dict = DoubleArrayTrie::from_terms(vec!["apple", "banana", "cherry"]);

// Check membership
assert!(dict.contains("apple"));
assert!(!dict.contains("grape"));

// Insert new term (append-only)
dict.insert("grape");
```

**When to use**:
- Static dictionaries that don't change
- Maximum query performance required
- Memory is a constraint

### DoubleArrayTrieChar

UTF-8 character-level variant of DoubleArrayTrie.

**Characteristics**:
- ~5% overhead vs byte-level
- 4x higher memory for edge labels (char vs byte)
- Full Unicode support

**Usage**:

```rust
use liblevenshtein::dictionary::DoubleArrayTrieChar;

let dict = DoubleArrayTrieChar::from_terms(vec!["café", "naïve", "中文"]);

assert!(dict.contains("café"));
assert!(dict.contains("中文"));
```

**When to use**:
- Multi-language applications
- Unicode text processing
- Diacritics and special characters

### DynamicDawg

Fully mutable DAWG (Directed Acyclic Word Graph).

**Characteristics**:
- Full insert AND remove operations
- Thread-safe concurrent access
- Auto-minimization with suffix caching (20-40% size reduction)
- Optional bloom filter for fast negative lookups
- SIMD optimizations available

**Usage**:

```rust
use liblevenshtein::dictionary::DynamicDawg;

let mut dict = DynamicDawg::new();

// Insert terms
dict.insert("hello");
dict.insert("world");
dict.insert("hello");  // Returns false (already exists)

// Remove terms
dict.remove("hello");  // Returns true

// Check membership
assert!(!dict.contains("hello"));
assert!(dict.contains("world"));

// Compact after deletions
dict.compact();
```

**With values**:

```rust
use liblevenshtein::dictionary::DynamicDawg;

let mut dict: DynamicDawg<u32> = DynamicDawg::new();

dict.insert_with_value("apple", 1);
dict.insert_with_value("banana", 2);

assert_eq!(dict.get_value("apple"), Some(1));
```

**When to use**:
- Dynamic dictionaries with frequent updates
- Need to remove terms
- Concurrent access required

### DynamicDawgChar

UTF-8 character-level variant of DynamicDawg.

**Characteristics**:
- Full Unicode support
- ~5% overhead vs byte-level
- All DynamicDawg features

**Usage**:

```rust
use liblevenshtein::dictionary::DynamicDawgChar;

let mut dict: DynamicDawgChar<u32> = DynamicDawgChar::new();

dict.insert_with_value("日本語", 1);
dict.insert_with_value("한국어", 2);

assert!(dict.contains("日本語"));
```

### DynamicDawgU64

DAWG with u64 edge labels for token sequences.

**Characteristics**:
- 8-byte integers instead of single bytes
- Useful for vocabulary IDs, hash values
- Supports time series data (f64)

**Usage**:

```rust
use liblevenshtein::dictionary::DynamicDawgU64;

let mut dict: DynamicDawgU64<()> = DynamicDawgU64::new();

// Insert token sequence
dict.insert(&[1u64, 2, 3, 4]);

// Time series data
dict.insert_f64(&[1.0, 2.5, 3.7]);

assert!(dict.contains_f64(&[1.0, 2.5, 3.7]));
```

**When to use**:
- Token ID sequences
- Time series matching
- Hash sequence matching

### SuffixAutomaton

Automaton for substring matching (not just prefixes).

**Characteristics**:
- Matches substrings anywhere in text
- Minimal DFA recognizing all suffixes
- Typically ≤ 2n-1 states for n characters
- O(1) amortized online construction

**Usage**:

```rust
use liblevenshtein::dictionary::SuffixAutomaton;

// From single text
let dict = SuffixAutomaton::<()>::from_text("the quick brown fox");

// From multiple texts
let dict = SuffixAutomaton::<()>::from_texts(vec![
    "hello world",
    "goodbye world",
]);

// Insert and remove
dict.insert("new text");
dict.remove("hello world");

// Find match positions
let positions = dict.match_positions("quick");
// Returns: [(doc_id, position), ...]
```

**When to use**:
- Substring search (not just prefix)
- Full-text fuzzy search
- OCR error correction (mid-word errors)

### SuffixAutomatonChar

UTF-8 character-level variant of SuffixAutomaton.

```rust
use liblevenshtein::dictionary::SuffixAutomatonChar;

let dict = SuffixAutomatonChar::<()>::from_text("日本語のテキスト");

assert!(dict.contains("語の"));  // Substring match
```

## Value System

### DictionaryValue Trait

Values must implement this marker trait:

```rust
pub trait DictionaryValue: Clone + Send + Sync + Unpin + 'static {}
```

Built-in implementations:
- Primitives: `u8`, `u16`, `u32`, `u64`, `usize`, `i8`, `i16`, `i32`, `i64`, `isize`, `bool`, `char`
- Strings: `String`, `&'static str`
- Collections: `Vec<T>`, `HashSet<T>`, `SmallVec<A>`

### FilterableValue Trait

For efficient filtering during traversal:

```rust
pub trait FilterableValue {
    fn matches_any<F>(&self, predicate: &F) -> bool
    where
        F: Fn(&Self) -> bool;

    fn matches_all<F>(&self, predicate: &F) -> bool
    where
        F: Fn(&Self) -> bool;
}
```

## Dictionary Comparison

| Dictionary | Query | Contains | Insert | Remove | Memory | Unicode |
|------------|-------|----------|--------|--------|--------|---------|
| DoubleArrayTrie | ●●●●● | ●●●●● | ●●○○○ | ○○○○○ | ●●●●● | ○ |
| DoubleArrayTrieChar | ●●●●○ | ●●●●○ | ●●○○○ | ○○○○○ | ●●●●○ | ● |
| DynamicDawg | ●●●●○ | ●●●○○ | ●●●●● | ●●●●● | ●●●○○ | ○ |
| DynamicDawgChar | ●●●○○ | ●●●○○ | ●●●●● | ●●●●● | ●●●○○ | ● |
| SuffixAutomaton | ●●●●○ | ●●●●○ | ●●●○○ | ●●●○○ | ●●○○○ | ○ |

Legend: ● = Good, ○ = Limited

## Selection Guide

```
Need substring matching?
├─ Yes → SuffixAutomaton / SuffixAutomatonChar
└─ No
   ├─ Need Unicode?
   │  ├─ Yes → DoubleArrayTrieChar or DynamicDawgChar
   │  └─ No → DoubleArrayTrie or DynamicDawg
   │
   └─ Need updates?
      ├─ Yes → DynamicDawg / DynamicDawgChar
      └─ No → DoubleArrayTrie / DoubleArrayTrieChar
```

## Next Steps

- [Overview](overview.md): Architecture overview
- [Transducers](transducers.md): Fuzzy matching API
- [Fuzzy Collections](fuzzy-collections.md): Maps and caches
- [Integration](lling-llang-integration.md): Using with lling-llang
