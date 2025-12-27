# liblevenshtein Transducers

Transducers provide the main query API for fuzzy string matching in liblevenshtein.

## Concepts

### What is a Transducer?

A **transducer** combines a dictionary with a Levenshtein automaton to find all dictionary terms within a given edit distance of a query string.

```
Query: "tset" (misspelled)
Dictionary: ["test", "testing", "best", "text", ...]
Max Distance: 1

Transducer matches:
  - "test" (distance 1: transpose s↔e)
  - "text" (distance 1: substitute s→x)
```

### How It Works

1. **Build automaton**: Create Levenshtein automaton for query string
2. **Parallel traversal**: Traverse automaton and dictionary together
3. **Prune**: Skip branches exceeding distance threshold
4. **Collect**: Return terms at final states within threshold

## Transducer API

### Construction

```rust
use liblevenshtein::prelude::*;

// Create dictionary
let dict = DoubleArrayTrie::from_terms(vec!["test", "testing", "tested"]);

// Create transducer with algorithm
let transducer = Transducer::new(dict, Algorithm::Standard);

// Convenience constructors
let transducer = Transducer::standard(dict);           // Standard algorithm
let transducer = Transducer::with_transposition(dict); // + Transpose
let transducer = Transducer::with_merge_split(dict);   // + Merge/Split
```

### Basic Queries

**Query returning strings**:

```rust
// Find all terms within distance 2
for term in transducer.query("tset", 2) {
    println!("Match: {}", term);
}

// Collect results
let matches: Vec<String> = transducer.query("tset", 2).collect();
```

**Query with distances**:

```rust
// Get Candidate structs with distance info
for candidate in transducer.query_with_distance("tset", 2) {
    println!("{}: distance {}", candidate.term, candidate.distance);
}

// Alias
let candidates: Vec<Candidate> = transducer.query_candidates("tset", 2).collect();
```

**Ordered queries** (by distance, then lexicographically):

```rust
// Get results ordered by distance
for candidate in transducer.query_ordered("tset", 2) {
    println!("{}: distance {}", candidate.term, candidate.distance);
}

// Get top 5 closest matches
let top_5: Vec<_> = transducer.query_ranked("tset", 2).take(5).collect();
```

### Query Builder

Fluent API for complex queries:

```rust
let results = transducer.query_builder("tset")
    .max_distance(2)
    .with_distances()
    .ordered()
    .take(10)
    .collect::<Vec<_>>();
```

### Value-Aware Queries

For `MappedDictionary` types:

```rust
let dict: DynamicDawg<u32> = DynamicDawg::new();
dict.insert_with_value("hello", 1);
dict.insert_with_value("hallo", 2);
dict.insert_with_value("hullo", 3);

let transducer = Transducer::new(dict, Algorithm::Standard);

// Filter by value during traversal (10-100x faster than post-filter)
let matches: Vec<_> = transducer
    .query_filtered("helo", 1, |scope_id| *scope_id == 1)
    .collect();

// Filter by set membership
use std::collections::HashSet;
let allowed: HashSet<u32> = HashSet::from([1, 2]);

let matches: Vec<_> = transducer
    .query_with_value_set("helo", 1, &allowed)
    .collect();
```

## Algorithm Types

### Standard

Basic Levenshtein operations:

| Operation | Cost | Example |
|-----------|------|---------|
| Match | 0 | test → test |
| Insert | 1 | tst → test |
| Delete | 1 | testt → test |
| Substitute | 1 | tast → test |

```rust
let transducer = Transducer::new(dict, Algorithm::Standard);
```

### Transposition

Standard + adjacent character swaps:

| Operation | Cost | Example |
|-----------|------|---------|
| Transpose | 1 | tset → test |

```rust
let transducer = Transducer::new(dict, Algorithm::Transposition);
// or
let transducer = Transducer::with_transposition(dict);
```

### Merge and Split

Standard + OCR-style errors:

| Operation | Cost | Example |
|-----------|------|---------|
| Merge | 1 | te st → test |
| Split | 1 | test → te st |

```rust
let transducer = Transducer::new(dict, Algorithm::MergeAndSplit);
// or
let transducer = Transducer::with_merge_split(dict);
```

### Algorithm Methods

```rust
impl Algorithm {
    pub fn name(&self) -> &'static str;
    pub fn supports_transposition(&self) -> bool;
    pub fn supports_merge_split(&self) -> bool;
    pub fn to_operation_set(&self) -> OperationSet;
}

// Parse from string
let algo: Algorithm = "transposition".parse()?;
```

## Substitution Policies

### Unrestricted (Default)

Standard Levenshtein - all substitutions cost 1:

```rust
// Unrestricted is the default
let transducer = Transducer::new(dict, Algorithm::Standard);

// Explicit type annotation
let transducer: Transducer<_, Unrestricted> = Transducer::new(dict, algo);
```

Zero-sized type, zero overhead.

### Restricted

Custom zero-cost substitutions:

```rust
use liblevenshtein::transducer::{SubstitutionSet, Restricted};

// Define zero-cost substitutions
let mut subs = SubstitutionSet::new();
subs.add('a', 'e');  // a↔e is free
subs.add('i', 'y');  // i↔y is free

let transducer = Transducer::with_substitutions(
    dict,
    Algorithm::Standard,
    subs
);

// Now "test" matches "tast" at distance 0 (a→e is free)
```

1-5% overhead vs unrestricted.

### Character-Level (RestrictedChar)

For Unicode dictionaries:

```rust
use liblevenshtein::transducer::RestrictedChar;

let mut subs = SubstitutionSetChar::new();
subs.add('é', 'e');  // é↔e is free
subs.add('ñ', 'n');  // ñ↔n is free

let transducer: Transducer<_, RestrictedChar> =
    Transducer::with_policy(dict, algo, RestrictedChar::new(subs));
```

## Generalized Operations

### OperationSet

Runtime-configurable edit operations:

```rust
use liblevenshtein::transducer::OperationSetBuilder;

let ops = OperationSetBuilder::new()
    .with_match()
    .with_substitution()
    .with_insertion()
    .with_deletion()
    .with_transposition()
    .build();

// Use with generalized automaton
let transducer = Transducer::generalized(dict, ops);
```

### OperationType

Individual operation definitions:

```rust
pub enum OperationType {
    Match,
    Substitute,
    Insert,
    Delete,
    Transpose,
    Merge,
    Split,
    Custom { name: String, cost: usize },
}
```

### Predefined Sets

```rust
// Standard: Match, Substitute, Insert, Delete
let ops = OperationSet::standard();

// With transposition
let ops = OperationSet::with_transposition();

// With merge/split
let ops = OperationSet::with_merge_split();
```

## Automata Types

### Lazy Automata (Default)

Built on-demand for each query:

```rust
// Default behavior
let transducer = Transducer::new(dict, Algorithm::Standard);

for term in transducer.query("test", 2) {
    // Automaton built for "test" during this query
}
```

**Characteristics**:
- Lower memory usage
- Best for diverse queries
- No precomputation cost

### Universal Automata

Precomputed once, reused for all queries:

```rust
use liblevenshtein::transducer::UniversalAutomaton;

// Precompute universal automaton for max distance 2
let universal = UniversalAutomaton::new(2, Algorithm::Standard);

// Use for queries
let transducer = Transducer::with_universal(dict, universal);

for term in transducer.query("test", 2) {
    // Uses precomputed automaton
}
```

**Characteristics**:
- Higher initial cost
- Faster subsequent queries
- Best for repeated patterns

### Generalized Automata

Runtime-configurable operations:

```rust
use liblevenshtein::transducer::GeneralizedAutomaton;

let ops = OperationSet::standard();
let automaton = GeneralizedAutomaton::new(ops);

let transducer = Transducer::with_generalized(dict, automaton);
```

**Characteristics**:
- ~10-20% overhead vs fixed algorithms
- Maximum flexibility
- Custom cost functions

## Query Results

### Candidate

```rust
pub struct Candidate {
    pub term: String,
    pub distance: usize,
}

impl Candidate {
    pub fn new(term: String, distance: usize) -> Self;
    pub fn term(&self) -> &str;
    pub fn distance(&self) -> usize;
}
```

### Iterators

| Iterator | Returns | Ordering |
|----------|---------|----------|
| `QueryIterator<_, String, _>` | `String` | Arbitrary |
| `QueryIterator<_, Candidate, _>` | `Candidate` | Arbitrary |
| `OrderedQueryIterator` | `Candidate` | Distance, then lex |
| `ValueFilteredQueryIterator` | `String` | Arbitrary |

All iterators are lazy - computation happens on-demand.

## Performance Tips

### Choose Right Algorithm

```rust
// Fastest for most cases
Algorithm::Standard

// Use only if transposition errors expected
Algorithm::Transposition

// Use only for OCR/scanning errors
Algorithm::MergeAndSplit
```

### Use Value Filtering

Filter during traversal, not after:

```rust
// Fast: filter during traversal
let matches: Vec<_> = transducer
    .query_filtered("test", 2, |v| *v > 100)
    .collect();

// Slow: filter after collection
let matches: Vec<_> = transducer
    .query("test", 2)
    .filter(|term| dict.get_value(term).map(|v| v > 100).unwrap_or(false))
    .collect();
```

10-100x speedup for selective filters.

### Limit Results

Use `.take()` for top-k queries:

```rust
// Stop after 10 results
let top_10: Vec<_> = transducer.query("test", 2).take(10).collect();

// Ordered top-k
let top_10: Vec<_> = transducer.query_ranked("test", 2).take(10).collect();
```

### Choose Right Dictionary

- Static data: `DoubleArrayTrie` (3x faster)
- Dynamic data: `DynamicDawg`
- Substring search: `SuffixAutomaton`

## Next Steps

- [Overview](overview.md): Architecture overview
- [Dictionaries](dictionaries.md): Dictionary implementations
- [Fuzzy Collections](fuzzy-collections.md): Maps and caches
- [Integration](lling-llang-integration.md): Using with lling-llang
