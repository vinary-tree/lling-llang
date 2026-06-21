# Syntax Recovery Layer

The syntax recovery layer handles structural syntax errors by inserting or deleting tokens to produce a parseable token sequence.

## Recovery Strategies

The layer supports three recovery strategies:

```rust
pub enum RecoveryStrategy {
    /// Insert missing tokens (e.g., missing closing bracket)
    Insertion,
    /// Delete unexpected tokens (e.g., extra semicolon)
    Deletion,
    /// Replace tokens (e.g., wrong bracket type)
    Replacement,
    /// All strategies combined (default)
    All,
}
```

### How Each Strategy Works

The layer scans the token stream as a small state machine: at each position it may
take an **Insertion**, **Deletion**, or **Replacement** transition (each gated by
the configured `RecoveryStrategy` set and adding its per-operation cost), while
tracking an open-bracket stack. At end of input, any unclosed brackets are
balanced before reaching the accepting `Recovered` state. Every repair is added as
a *parallel* lattice edge, so all candidates coexist and the lowest-cost path —
`` `cost = original_cost + Σ recovery_costs` `` — is preferred downstream.

![State diagram: from Scanning, conditional transitions to Insertion (+insertion_cost), Deletion (+deletion_cost via an ε edge), and Replacement (+replacement_cost), each returning to Scanning; at end of input, green edges go to Balancing (if the bracket stack is non-empty) then to the green double-ringed Recovered state, or directly to Recovered when the stack is empty.](../../diagrams/layers/code-correction/syntax-recovery.svg)

*Amber = recovery states; bold green = the accepting transitions to the green
double-ring `Recovered` state; guards in `` `[ … ]` `` are the strategy/cost/limit
conditions (`max_insertions`, `max_deletions`); `` `ε` `` is the empty (skip) label.*

<details><summary>Text view (per-strategy lattice edits)</summary>

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                       Recovery Strategies                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  INSERTION - Add missing tokens                                          │
│  ──────────────────────────────────────────────────────────────────────  │
│  Input:  "def foo(x"                                                     │
│  Output: "def foo(x )"  (insert ")")                                    │
│                                                                          │
│  Implementation: Self-loop edges for insertable tokens at each node      │
│      ┌────┐                                                              │
│      ▼    │ ")"                                                          │
│  ○───────○───────○                                                       │
│     "x"                                                                  │
│                                                                          │
│  DELETION - Remove unexpected tokens                                     │
│  ──────────────────────────────────────────────────────────────────────  │
│  Input:  "def;; foo"                                                     │
│  Output: "def foo"  (delete extra ";")                                  │
│                                                                          │
│  Implementation: Skip edges that bypass deletable tokens                 │
│                                                                          │
│     ○──────";"──────○──────"foo"──────○                                 │
│      \              /                                                    │
│       `──── ε ────´                                                     │
│            (skip)                                                        │
│                                                                          │
│  REPLACEMENT - Swap wrong tokens                                         │
│  ──────────────────────────────────────────────────────────────────────  │
│  Input:  "arr[0)"  (wrong bracket type)                                  │
│  Output: "arr[0]"  (replace ")" with "]")                               │
│                                                                          │
│  Implementation: Alternative edges with replacement tokens               │
│                                                                          │
│     ○──────")"──────○    (cost: 0)                                      │
│      \              │                                                    │
│       \─────"]"────-┘    (cost: replacement_cost)                       │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

</details>

## Configuration

### SyntaxRecoveryConfig

```rust
pub struct SyntaxRecoveryConfig {
    /// Recovery strategies to use
    pub strategies: Vec<RecoveryStrategy>,

    /// Cost for inserting a token
    pub insertion_cost: f64,

    /// Cost for deleting a token
    pub deletion_cost: f64,

    /// Cost for replacing a token
    pub replacement_cost: f64,

    /// Maximum consecutive insertions
    pub max_insertions: usize,

    /// Maximum consecutive deletions
    pub max_deletions: usize,

    /// Tokens that can be inserted
    pub insertable_tokens: HashSet<Arc<str>>,

    /// Tokens that can be deleted
    pub deletable_tokens: HashSet<Arc<str>>,

    /// Bracket pairs (open -> close)
    pub bracket_pairs: HashMap<Arc<str>, Arc<str>>,

    /// Whether to balance brackets
    pub balance_brackets: bool,

    /// Whether to add missing semicolons
    pub add_semicolons: bool,

    /// Language hint
    pub language_hint: Option<String>,
}
```

### Default Configuration

```rust
impl Default for SyntaxRecoveryConfig {
    fn default() -> Self {
        // Default insertable tokens
        let insertable = hashset!["(", ")", "[", "]", "{", "}", ";", ",", ":", "."];

        // Default deletable tokens
        let deletable = hashset![";", ",", ".", "(", ")", "[", "]", "{", "}"];

        // Standard bracket pairs
        let brackets = hashmap![
            "(" => ")",
            "[" => "]",
            "{" => "}",
            "<" => ">",
        ];

        Self {
            strategies: vec![RecoveryStrategy::All],
            insertion_cost: 2.0,
            deletion_cost: 1.5,
            replacement_cost: 1.0,
            max_insertions: 3,
            max_deletions: 2,
            insertable_tokens: insertable,
            deletable_tokens: deletable,
            bracket_pairs: brackets,
            balance_brackets: true,
            add_semicolons: false,
            language_hint: None,
        }
    }
}
```

## Creating a Syntax Recovery Layer

### Basic Usage

```rust
use lling_llang::layers::code_correction::{SyntaxRecoveryLayer, SyntaxRecoveryConfig};

// With default configuration
let layer = SyntaxRecoveryLayer::new(SyntaxRecoveryConfig::default());

// Apply to a lattice
let recovered = layer.apply(&lattice)?;
```

### Custom Configuration

```rust
let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::Insertion])
    .with_insertion_cost(1.5)
    .with_max_insertions(5)
    .with_bracket_balancing(true)
    .with_insertable_tokens(vec!["async", "await"])
    .with_bracket_pair("<<", ">>");  // Custom brackets

let layer = SyntaxRecoveryLayer::new(config);
```

### Language-Specific Configuration

```rust
// Python (no semicolons, uses indentation)
let python_config = SyntaxRecoveryConfig::default()
    .with_semicolon_insertion(false)
    .with_language("python");

// Rust (uses semicolons and braces)
let rust_config = SyntaxRecoveryConfig::default()
    .with_semicolon_insertion(true)
    .with_insertable_tokens(vec!["->", "=>", "::"])
    .with_language("rust");

// Rholang (parallel composition)
let rholang_config = SyntaxRecoveryConfig::default()
    .with_insertable_tokens(vec!["|", "<-", "<<", ">>"])
    .with_bracket_pair("{*", "*}")
    .with_language("rholang");
```

## Builder Pattern

```rust
let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::All])
    .with_insertion_cost(3.0)      // Higher cost for insertions
    .with_deletion_cost(1.5)       // Lower cost for deletions
    .with_replacement_cost(1.0)    // Lowest cost for replacements
    .with_max_insertions(5)        // Allow up to 5 consecutive inserts
    .with_max_deletions(2)         // Allow up to 2 consecutive deletes
    .with_bracket_balancing(true)  // Balance unclosed brackets
    .with_semicolon_insertion(true) // Add missing semicolons
    .with_language("java");
```

## Bracket Balancing

The layer automatically balances unclosed brackets:

```rust
// Input: "def foo(x:"
// The "(" is never closed

// With balance_brackets = true:
// The layer tracks: bracket_stack = ["("]
// At end of input, it adds closing ")" with insertion_cost

// Result lattice includes path: "def foo(x:)"
```

### Custom Bracket Pairs

```rust
let config = SyntaxRecoveryConfig::default()
    .with_bracket_pair("(*", "*)")     // ML-style comments
    .with_bracket_pair("{|", "|}")     // Set builder notation
    .with_bracket_pair("<%", "%>");    // Template tags
```

## Cost Model

Costs determine which corrections are preferred. The total cost of a recovered
path is `` `total_cost = original_cost + Σ recovery_costs` ``:

```text
total_cost = original_cost + Σ recovery_costs

Recovery costs:
- Insertion:   insertion_cost   per token (default 2.0)
- Deletion:    deletion_cost    per token (default 1.5)
- Replacement: replacement_cost per token (default 1.0)
```

Lower total cost = more preferred path. Because weights live in the tropical
semiring (`` `⊕ = min` ``, `` `⊗ = +` ``), the best path is the minimum-cost path.

### Cost Tuning

```rust
// Prefer deletions over insertions (for noisy input)
let config = SyntaxRecoveryConfig::default()
    .with_insertion_cost(3.0)
    .with_deletion_cost(1.0);

// Prefer replacements (for bracket mismatches)
let config = SyntaxRecoveryConfig::default()
    .with_replacement_cost(0.5)
    .with_insertion_cost(2.0)
    .with_deletion_cost(2.0);
```

## Examples

### Missing Closing Bracket

```rust
// Input lattice represents: "foo(x, y"

let config = SyntaxRecoveryConfig::default();
let layer = SyntaxRecoveryLayer::new(config);

let recovered = layer.apply(&input)?;
// Recovered lattice includes path: "foo(x, y)"
// with cost = original_cost + insertion_cost(2.0)
```

### Extra Punctuation

```rust
// Input: "return;; x"

let config = SyntaxRecoveryConfig::default();
let layer = SyntaxRecoveryLayer::new(config);

let recovered = layer.apply(&input)?;
// Recovered lattice includes path: "return; x"
// with cost = original_cost + deletion_cost(1.5)
```

### Wrong Bracket Type

```rust
// Input: "arr[0)"

let config = SyntaxRecoveryConfig::default();
let layer = SyntaxRecoveryLayer::new(config);

let recovered = layer.apply(&input)?;
// Recovered lattice includes:
//   "arr[0)" with cost = original_cost
//   "arr[0]" with cost = original_cost + replacement_cost(1.0)
```

### Multiple Errors

```rust
// Input: "fn foo( {"  (missing ) and body)

let config = SyntaxRecoveryConfig::default()
    .with_max_insertions(5);  // Allow multiple insertions

let layer = SyntaxRecoveryLayer::new(config);
let recovered = layer.apply(&input)?;

// Recovered lattice includes path: "fn foo() {"
// with cost = original_cost + insertion_cost * 1
```

## Strategy Selection

### Insertion-Only

For completing partial code:

```rust
let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::Insertion]);
```

Use when:
- Code is being typed (IDE completion)
- Input is mostly correct but incomplete
- You want to preserve all original tokens

### Deletion-Only

For cleaning noisy input:

```rust
let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::Deletion]);
```

Use when:
- Input may have accidental duplicates
- Copy-paste errors
- Removing debug statements

### All Strategies

For general error recovery:

```rust
let config = SyntaxRecoveryConfig::new(vec![RecoveryStrategy::All]);
// Or just use default
let config = SyntaxRecoveryConfig::default();
```

Use when:
- Unknown error types
- General code repair
- Maximum flexibility needed

## Layer Statistics

The layer reports statistics about its operation:

```rust
let (recovered, stats) = layer.apply_with_stats(&lattice)?;

println!("Input edges: {}", stats.input_edges);
println!("Output edges: {}", stats.output_edges);
println!("Time: {}μs", stats.time_us);

// Expansion factor
let expansion = stats.output_edges as f64 / stats.input_edges as f64;
println!("Expansion: {:.2}x", expansion);
```

## Estimated Expansion

The layer provides an estimate of how many paths it adds:

```rust
let layer = SyntaxRecoveryLayer::new(config);
let expansion = layer.estimated_expansion();
// Typical value: 1.05 - 1.15 (5-15% more edges)
```

This helps with planning pipeline resources.

## See Also

- [Overview](overview.md) - Code correction introduction
- [Pattern-Aware Correction](pattern-aware.md) - Idiom-based boosting
- [Language Configuration](configuration.md) - Per-language settings

## References

- [Mohri 2002](../../BIBLIOGRAPHY.md#ref-mohri2002) — weighted finite-state
  transducers; recovery edits are weighted arcs, and the corrected reading is the
  best path through the resulting lattice.
- [Mohri 2009](../../BIBLIOGRAPHY.md#ref-mohri2009) — shortest-/best-path
  algorithms over the tropical semiring used to select the minimum-cost recovery.
