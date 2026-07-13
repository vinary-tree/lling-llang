# Pattern-Aware Correction

The pattern-aware layer uses mined code idioms to boost corrections that match common patterns, making idiomatic code more likely to be selected.

## Why Pattern-Aware Correction?

Code has conventions:
- `def foo():` in Python (not `def foo ( ) :`)
- `fn main() {` in Rust
- `for (x <- xs)` in Rholang

The pattern-aware layer recognizes these patterns and boosts paths that match them, improving correction quality by preferring idiomatic code.

## How It Works

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      Pattern-Aware Boosting                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Pattern Library:                                                        │
│    Pattern 1: ["def", "_", "(", ")"]     boost=1.0                      │
│    Pattern 2: ["if", "_", ":"]           boost=0.8                      │
│    Pattern 3: ["for", "_", "in", "_"]    boost=1.0                      │
│                                                                          │
│  Input Lattice:                                                          │
│    ○─"def"─○─"foo"─○─"("─○─"x"─○─")"─○─":"─○                           │
│                                                                          │
│  Pattern Matching:                                                       │
│    Position 0: "def" starts Pattern 1                                    │
│    Match: ["def", "foo", "(", ")"] ✓                                    │
│                                                                          │
│  Boosted Lattice:                                                        │
│    ○─"def"─○─"foo"─○─"("─○─"x"─○─")"─○─":"─○                           │
│       ↑       ↑      ↑           ↑                                      │
│      -1.0   -1.0   -1.0        -1.0   (negative cost = bonus)           │
│                                                                          │
│  Result: Path through pattern gets lower total cost                      │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Pattern Boost

A pattern boost associates a token sequence with a bonus:

```rust
pub struct PatternBoost {
    /// The token sequence pattern
    pub pattern: Vec<Arc<str>>,
    /// Boost value (negative cost = bonus)
    pub boost: f64,
    /// Pattern ID for tracking
    pub pattern_id: u64,
    /// Support count from mining
    pub support: usize,
    /// Pattern name for debugging
    pub name: Option<String>,
}
```

### Creating Pattern Boosts

```rust
use lling_llang::layers::code_correction::PatternBoost;

// Simple pattern
let pattern = PatternBoost::new(vec!["def", "foo", "(", ")"], 1.0);

// With metadata
let pattern = PatternBoost::new(vec!["for", "_", "in", "_", ":"], 0.8)
    .with_id(42)
    .with_support(150)  // Seen 150 times in corpus
    .with_name("for_loop");
```

### Wildcard Patterns

Use `"_"` as a wildcard to match any token:

```rust
// Matches: "def foo()", "def bar()", "def anything()"
let pattern = PatternBoost::new(vec!["def", "_", "(", ")"], 1.0);

// Matches: "for x in items:", "for item in collection:"
let pattern = PatternBoost::new(vec!["for", "_", "in", "_", ":"], 1.0);
```

## Configuration

### PatternAwareConfig

```rust
pub struct PatternAwareConfig {
    /// Patterns with their boost values
    pub patterns: Vec<PatternBoost>,

    /// Minimum pattern length to consider
    pub min_pattern_length: usize,

    /// Maximum pattern length to consider
    pub max_pattern_length: usize,

    /// Default boost for patterns without explicit boost
    pub default_boost: f64,

    /// Whether to use longest matching pattern only
    pub longest_match_only: bool,

    /// Maximum boost to apply (caps total boost)
    pub max_boost: f64,

    /// Whether patterns must match at token boundaries
    pub token_boundary_only: bool,
}
```

### Default Configuration

```rust
impl Default for PatternAwareConfig {
    fn default() -> Self {
        Self {
            patterns: Vec::new(),
            min_pattern_length: 2,
            max_pattern_length: 10,
            default_boost: 0.5,
            longest_match_only: true,
            max_boost: 5.0,
            token_boundary_only: true,
        }
    }
}
```

## Built-in Language Patterns

The library includes patterns for common languages:

### Python Patterns

```rust
let config = PatternAwareConfig::python_patterns();
// Includes:
// - ["def", "_", "(", ")"]     function definition
// - ["if", "_", ":"]           if statement
// - ["for", "_", "in", "_", ":"]  for loop
// - ["class", "_", ":"]        class definition
// - ["import", "_"]            import statement
// - ["from", "_", "import", "_"]  from import
```

### Rust Patterns

```rust
let config = PatternAwareConfig::rust_patterns();
// Includes:
// - ["fn", "_", "(", ")"]      function definition
// - ["let", "_", "="]          variable binding
// - ["let", "mut", "_", "="]   mutable binding
// - ["impl", "_", "for", "_"]  trait implementation
// - ["struct", "_", "{"]       struct definition
// - ["match", "_", "{"]        match expression
// - ["if", "let", "Some", "(", "_", ")", "="]  if-let
```

### Rholang Patterns

```rust
let config = PatternAwareConfig::rholang_patterns();
// Includes:
// - ["new", "_", "in"]         new binding
// - ["contract", "_", "(", ")"]  contract definition
// - ["for", "(", "_", "<-", "_", ")"]  receive
// - ["match", "_", "{"]        pattern match
// - ["|"]                      parallel composition
```

### MeTTa Patterns

```rust
let config = PatternAwareConfig::metta_patterns();
// Includes:
// - ["(", "=", "_", "_", ")"]  equality definition
// - ["(", ":", "_", "_", ")"]  type annotation
// - ["(", "match", "_", "_", "_", ")"]  match expression
// - ["(", "let", "_", "_", "_", ")"]  let binding
// - ["!", "(", "_", ")"]       evaluation
```

## Creating a Pattern-Aware Layer

### With Built-in Patterns

```rust
use lling_llang::layers::code_correction::PatternAwareLayer;

// Use language-specific patterns
let layer = PatternAwareLayer::python();
let layer = PatternAwareLayer::rust();
let layer = PatternAwareLayer::rholang();
let layer = PatternAwareLayer::metta();
```

### With Custom Patterns

```rust
let config = PatternAwareConfig::new()
    .with_pattern(vec!["my", "custom", "pattern"], 1.5)
    .with_pattern(vec!["another", "_", "pattern"], 0.8)
    .with_max_boost(3.0)
    .with_longest_match_only(true);

let layer = PatternAwareLayer::new(config);
```

### From Mined Patterns

```rust
use libgrammstein::code::subtree::{TreeminerD, SubtreePattern};

// Mine patterns from a corpus
let miner = TreeminerD::new(0.1);
let result = miner.mine(&corpus_trees);

// Convert to pattern boosts
let pattern_boosts: Vec<PatternBoost> = result.patterns
    .iter()
    .map(|p| {
        let tokens: Vec<&str> = p.nodes.iter()
            .map(|n| n.label.as_ref())
            .collect();

        PatternBoost::new(tokens, p.support_ratio)
            .with_id(p.pattern_id)
            .with_support(p.support)
    })
    .collect();

let config = PatternAwareConfig::new()
    .with_patterns(pattern_boosts);

let layer = PatternAwareLayer::new(config);
```

## Pattern Matching

### Finding Best Pattern

```rust
let config = PatternAwareConfig::new()
    .with_pattern(vec!["def", "foo"], 0.5)
    .with_pattern(vec!["def", "foo", "(", ")"], 1.0);

let tokens = vec!["def", "foo", "(", ")"];

// With longest_match_only = true:
let best = config.find_best_pattern(&tokens);
// Returns the 4-token pattern (longest match)
```

### Pattern Index

Patterns are indexed by their first token for efficient lookup:

```rust
// Get all patterns starting with "def"
for pattern in config.patterns_starting_with("def") {
    println!("Pattern: {:?}, boost: {}", pattern.pattern, pattern.boost);
}
```

## Boost Application

The boost is applied as a negative cost in the tropical semiring:

```rust
// Original weight: 5.0
// Pattern boost: 1.0
// Boosted weight: 5.0 + (-1.0) = 4.0

// In tropical semiring (where lower is better):
// The boosted path is now preferred
```

### Boost Capping

To prevent extreme boosts:

```rust
let config = PatternAwareConfig::new()
    .with_max_boost(3.0);  // Total boost capped at 3.0

// Even if multiple patterns overlap, total boost won't exceed 3.0
```

## Examples

### Python Function Completion

```rust
// Input: "def foo x )"
// Pattern: ["def", "_", "(", ")"] with boost=1.0

let layer = PatternAwareLayer::python();
let boosted = layer.apply(&lattice)?;

// Path "def foo ( x )" gets boost because:
// - "def" matches position 0
// - "foo" matches wildcard at position 1
// - "(" matches position 2
// - ")" matches position 3
// Total boost: 1.0 applied to these edges
```

### Rholang Contract

```rust
// Input: "contract foo ( )"
// Pattern: ["contract", "_", "(", ")"] with boost=1.0

let layer = PatternAwareLayer::rholang();
let boosted = layer.apply(&lattice)?;

// Path through "contract foo ( )" is boosted
```

### Combined Patterns

```rust
// Input could match multiple patterns
let config = PatternAwareConfig::new()
    .with_pattern(vec!["if", "_"], 0.3)           // Short pattern
    .with_pattern(vec!["if", "_", ":"], 0.8)      // Longer pattern
    .with_longest_match_only(true);

// With longest_match_only=true, only the 3-token pattern applies
// With longest_match_only=false, both boosts would stack
```

## Integration with Mining

Use patterns from subtree mining:

```rust
// 1. Mine patterns from corpus
let miner = TreeminerD::new(0.05);  // 5% support
let mining_result = miner.mine(&ast_trees);

// 2. Convert to pattern boosts
let boosts = mining_result.patterns.iter()
    .filter(|p| p.size() >= 3)  // Only patterns with 3+ nodes
    .map(|p| pattern_to_boost(p))
    .collect();

// 3. Create layer
let config = PatternAwareConfig::new()
    .with_patterns(boosts);
let layer = PatternAwareLayer::new(config);

// 4. Apply to corrections
let improved = layer.apply(&correction_lattice)?;
```

## Performance

| Metric | Complexity |
|--------|------------|
| Pattern matching | $`O(n \times p \times m)`$ |
| Weight adjustment | $`O(e)`$ |
| Total | $`O(n \times p \times m + e)`$ |

Where:
- $`n`$ = number of tokens
- $`p`$ = number of patterns
- $`m`$ = maximum pattern length
- $`e`$ = number of edges

The pattern index reduces average complexity by grouping patterns by first token.

## Layer Statistics

```rust
let layer = PatternAwareLayer::rust();

println!("Number of patterns: {}", layer.num_patterns());
println!("Estimated reduction: {}", layer.estimated_reduction());
// Typically 1.0 (boosting doesn't remove paths)
```

## Best Practices

### 1. Use Appropriate Boost Values

```rust
// Higher support = higher boost (more common = more idiomatic)
let boost = pattern.support_ratio * 2.0;

// Cap at reasonable maximum
let boost = boost.min(2.0);
```

### 2. Balance Pattern Length

```rust
// Short patterns match too often (noisy)
// Long patterns match too rarely (sparse)
let config = PatternAwareConfig::new()
    .with_min_length(3)   // At least 3 tokens
    .with_max_length(8);  // At most 8 tokens
```

### 3. Prefer Longest Match

```rust
// Usually want longest pattern to win
let config = PatternAwareConfig::new()
    .with_longest_match_only(true);

// Overlapping patterns get handled cleanly
```

### 4. Update Patterns Periodically

```rust
// Re-mine patterns as codebase evolves
fn update_patterns(layer: &mut PatternAwareLayer, corpus: &[FlatTree]) {
    let result = miner.mine(corpus);
    let new_config = build_config_from_patterns(&result.patterns);
    *layer = PatternAwareLayer::new(new_config);
}
```

## See Also

- [Overview](overview.md) - Code correction introduction
- [Syntax Recovery](syntax-recovery.md) - Error recovery layer
- [Language Configuration](configuration.md) - Per-language settings
- [Subtree Mining](../../../libgrammstein/docs/components/subtree/overview.md) - Pattern discovery

## References

- [Mohri 2002](../../BIBLIOGRAPHY.md#ref-mohri2002) — weighted finite-state
  transducers; pattern boosts are negative-cost arcs in the tropical-semiring
  lattice, so boosted idioms become lower-cost (preferred) paths.
- [Goodman 1999](../../BIBLIOGRAPHY.md#ref-goodman1999) — semiring parsing; the
  algebraic basis for accumulating pattern weights along derivations.
