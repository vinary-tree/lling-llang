# LaTeX Repair Strategies

The repair module provides strategies for generating fix suggestions for LaTeX syntax errors detected during validation.

## Repair Suggestion Structure

```rust
pub struct RepairSuggestion {
    pub kind: RepairKind,
    pub position: usize,
    pub tokens: Vec<String>,
    pub confidence: f32,
    pub description: String,
}
```

### Repair Kinds

```rust
pub enum RepairKind {
    /// Insert tokens at position
    Insert,
    /// Delete tokens starting at position
    Delete { count: usize },
    /// Replace tokens starting at position
    Replace { count: usize },
}
```

## Creating Repair Suggestions

```rust
use lling_llang::layers::latex::RepairSuggestion;

// Insert suggestion
let insert = RepairSuggestion::insert(
    10,                          // position
    vec!["}".to_string()],       // tokens to insert
    0.8,                         // confidence
    "Insert missing closing brace"
);

// Delete suggestion
let delete = RepairSuggestion::delete(
    5,                           // position
    1,                           // count of tokens to delete
    0.6,                         // confidence
    "Delete unmatched brace"
);

// Replace suggestion
let replace = RepairSuggestion::replace(
    3,                           // position
    1,                           // count to replace
    vec!["equation".to_string()], // replacement tokens
    0.85,                        // confidence
    "Fix environment name"
);
```

## Repair Strategy Trait

```rust
pub trait RepairStrategy: Send + Sync {
    /// Generate repair suggestions for a validation issue
    fn suggest(&self, issue: &ValidationIssue, context: &[&str]) -> Vec<RepairSuggestion>;

    /// Name of this strategy for diagnostics
    fn name(&self) -> &str;
}
```

## Built-in Strategies

### Brace Repair Strategy

Fixes unmatched braces, brackets, and parentheses:

```rust
use lling_llang::layers::latex::BraceRepairStrategy;

let strategy = BraceRepairStrategy::new();
```

**Handled Issues:**

| Issue | Repair |
|-------|--------|
| `UnmatchedOpenBrace` | Insert `}` at end or logical break |
| `UnmatchedCloseBrace` | Delete brace or insert `{` at start |
| `UnmatchedOpenBracket` | Insert `]` at end |
| `UnmatchedCloseBracket` | Delete bracket |
| `UnmatchedOpenParen` | Insert `)` at end |
| `UnmatchedCloseParen` | Delete parenthesis |

```rust
let issue = ValidationIssue::error(
    IssueKind::UnmatchedOpenBrace,
    Some(0),
    "Unclosed '{' at position 0",
);
let context = vec!["{", "content"];

let suggestions = strategy.suggest(&issue, &context);
// Returns:
// - Insert "}" at end (confidence: 0.7)
// - Insert "}" at logical break (confidence: 0.8)
```

### Environment Repair Strategy

Fixes environment begin/end mismatches:

```rust
use lling_llang::layers::latex::EnvironmentRepairStrategy;

let strategy = EnvironmentRepairStrategy::new();
```

**Handled Issues:**

| Issue | Repair |
|-------|--------|
| `MissingEnvironmentEnd` | Insert `\end{name}` at end |
| `ExtraEnvironmentEnd` | Delete `\end{name}` or insert `\begin{name}` |
| `EnvironmentMismatch` | Replace end name with begin name |

```rust
let issue = ValidationIssue::error(
    IssueKind::MissingEnvironmentEnd,
    Some(0),
    "Unclosed environment 'equation'",
);
let context = vec!["\\begin", "{", "equation", "}", "x"];

let suggestions = strategy.suggest(&issue, &context);
// Returns:
// - Insert "\end { equation }" at end (confidence: 0.9)
```

### Math Repair Strategy

Fixes math delimiter issues:

```rust
use lling_llang::layers::latex::MathRepairStrategy;

let strategy = MathRepairStrategy::new();
```

**Handled Issues:**

| Issue | Repair |
|-------|--------|
| `UnmatchedMathDelimiter` | Insert closing delimiter or delete unmatched |
| `NestedMathMode` | Close outer mode or delete nested delimiter |

```rust
let issue = ValidationIssue::error(
    IssueKind::UnmatchedMathDelimiter,
    Some(0),
    "Unclosed inline math",
);
let context = vec!["$", "x", "+", "y"];

let suggestions = strategy.suggest(&issue, &context);
// Returns:
// - Insert "$" at end (confidence: 0.8)
```

## Composite Strategy

Combines multiple strategies:

```rust
use lling_llang::layers::latex::CompositeRepairStrategy;

// Use all default strategies
let strategy = CompositeRepairStrategy::all();

// Or build custom
let strategy = CompositeRepairStrategy::with_strategies(vec![
    Box::new(BraceRepairStrategy::new()),
    Box::new(MathRepairStrategy::new()),
]);
```

The composite strategy:
1. Runs all sub-strategies
2. Collects all suggestions
3. Sorts by confidence (highest first)

## Using Repairs with the Layer

```rust
use lling_llang::layers::latex::{LatexSyntaxLayer, LatexGrammar, LatexSyntaxConfig};

let grammar = LatexGrammar::standard()?;

// Configure repair generation
let config = LatexSyntaxConfig {
    generate_repairs: true,
    max_repairs_per_issue: 3,
    auto_repair: false,
    auto_repair_threshold: 0.9,
    ..Default::default()
};

let layer = LatexSyntaxLayer::with_config(grammar, config);

// Apply layer to lattice
let result = layer.apply(&lattice);

// Get repair suggestions
let repairs = layer.last_repairs();
for repair in repairs {
    println!("At {}: {} (confidence: {:.2})",
        repair.position,
        repair.description,
        repair.confidence
    );

    match repair.kind {
        RepairKind::Insert => {
            println!("  Insert: {:?}", repair.tokens);
        }
        RepairKind::Delete { count } => {
            println!("  Delete {} token(s)", count);
        }
        RepairKind::Replace { count } => {
            println!("  Replace {} token(s) with {:?}", count, repair.tokens);
        }
    }
}
```

## Custom Repair Strategy

Implement your own repair strategy:

```rust
use lling_llang::layers::latex::{RepairStrategy, RepairSuggestion, ValidationIssue};

pub struct CustomRepairStrategy;

impl RepairStrategy for CustomRepairStrategy {
    fn name(&self) -> &str {
        "custom-repair"
    }

    fn suggest(&self, issue: &ValidationIssue, context: &[&str]) -> Vec<RepairSuggestion> {
        let mut suggestions = Vec::new();

        // Custom repair logic based on issue kind and context
        if issue.kind == IssueKind::UnknownEnvironment {
            // Suggest similar known environments
            suggestions.push(RepairSuggestion::replace(
                issue.position.unwrap_or(0) + 2,
                1,
                vec!["equation".to_string()],
                0.7,
                "Did you mean 'equation'?",
            ));
        }

        suggestions
    }
}

// Use with layer
let layer = LatexSyntaxLayer::new(grammar)
    .with_repair_strategy(CustomRepairStrategy);
```

## Auto-Repair

Enable automatic application of high-confidence repairs:

```rust
let config = LatexSyntaxConfig {
    auto_repair: true,
    auto_repair_threshold: 0.9,  // Only auto-apply repairs with ≥90% confidence
    ..Default::default()
};

let layer = LatexSyntaxLayer::with_config(grammar, config);
```

When `auto_repair` is enabled:
1. Repairs with confidence ≥ threshold are applied automatically
2. The resulting lattice includes the repairs
3. Lower-confidence repairs are still available via `last_repairs()`

## Disabling Repairs

```rust
let layer = LatexSyntaxLayer::new(grammar)
    .without_repairs();

// Or via config
let config = LatexSyntaxConfig {
    generate_repairs: false,
    ..Default::default()
};
```

## Confidence Scoring

Confidence values are based on:

| Factor | Impact |
|--------|--------|
| Issue type | Base confidence varies by issue |
| Context analysis | Higher if logical break point found |
| Repair simplicity | Simple repairs (single token) score higher |

Typical confidence ranges:

| Repair Type | Confidence Range |
|-------------|------------------|
| Insert at end | 0.5 - 0.7 |
| Insert at break | 0.7 - 0.9 |
| Delete unmatched | 0.5 - 0.6 |
| Environment end | 0.8 - 0.9 |
| Math delimiter | 0.7 - 0.9 |

## Related

- [Overview](./overview.md): Layer architecture
- [Grammar](./grammar.md): CFG rules
- [Validator](./validator.md): Validation that triggers repairs
