# API Migration Transducers

API migration transducers provide WFST-based automation for transforming code between API versions. This document explains how to define migration rules and apply them to modernize codebases.

## Concepts

### What is API Migration?

**API migration** is the process of updating code to work with a newer version of a library, framework, or language. Common scenarios include:

- **Function renames**: `oldFunction()` → `newFunction()`
- **Parameter changes**: New required parameters, renamed parameters
- **Signature changes**: Different argument types or orders
- **Deprecations**: Removal of old APIs with replacement suggestions
- **Syntax evolution**: Python 2 to Python 3, ES5 to ES6

### Why WFSTs for Migration?

Traditional approaches (regex, AST rewriting) have limitations:

| Approach | Strengths | Weaknesses |
|----------|-----------|------------|
| Regex | Fast, simple | No context, fragile |
| AST rewriting | Precise, structural | Language-specific, complex |
| **WFST** | Context-aware, composable, weighted | Token-based |

WFSTs excel at:
- **Pattern matching** with context
- **Weighted alternatives** when multiple migrations are possible
- **Composition** of migration passes
- **Incremental application** through streaming

### Architecture

```
┌─────────────────────────────────────────────────────┐
│              API Migration System                    │
├─────────────────────────────────────────────────────┤
│  Migration Rules                                     │
│  ├── RenameFunction("old" → "new")                  │
│  ├── RenameType("OldType" → "NewType")              │
│  ├── ReplaceCall(pattern → replacement)             │
│  └── Custom transformations                          │
├─────────────────────────────────────────────────────┤
│  Version Range                                       │
│  └── from: v1.0.0  to: v2.0.0                       │
├─────────────────────────────────────────────────────┤
│  Migration Transducer                                │
│  ├── Token-based pattern matching                   │
│  ├── Rule application with costs                    │
│  └── WFST compilation for composition               │
└─────────────────────────────────────────────────────┘
```

## Core API

### Version and VersionRange

Semantic versioning support for constraining rule applicability:

```rust
use lling_llang::programming::{Version, VersionRange};

// Create versions
let v1_0 = Version::new(1, 0);           // 1.0.0
let v2_0 = Version::new(2, 0);           // 2.0.0
let v1_2_3 = Version::with_patch(1, 2, 3); // 1.2.3

// Parse from strings
let parsed = Version::parse("1.5.2").unwrap();

// Version ordering
assert!(v1_0 < v2_0);
assert!(v1_0 < v1_2_3);

// Version ranges
let range = VersionRange::new(v1_0, v2_0);
assert!(range.contains(v1_2_3));   // 1.0.0 ≤ 1.2.3 ≤ 2.0.0
assert!(!range.contains(Version::new(3, 0)));
```

### MigrationType

Different kinds of API changes:

```rust
use lling_llang::programming::MigrationType;

// Function rename
MigrationType::RenameFunction {
    old_name: "deprecated_fn".to_string(),
    new_name: "modern_fn".to_string(),
}

// Parameter rename (optionally scoped to a function)
MigrationType::RenameParameter {
    function: Some("my_function".to_string()),
    old_name: "old_param".to_string(),
    new_name: "new_param".to_string(),
}

// Type rename
MigrationType::RenameType {
    old_name: "OldClass".to_string(),
    new_name: "NewClass".to_string(),
}

// Multi-token pattern replacement
MigrationType::ReplaceCall {
    old_pattern: vec!["obj".to_string(), ".", "method".to_string()],
    new_pattern: vec!["obj".to_string(), ".", "newMethod".to_string()],
}

// Deprecation with message
MigrationType::RemoveFunction {
    function: "legacy_fn".to_string(),
    message: "Use modern_fn instead".to_string(),
}

// Custom transformation
MigrationType::Custom {
    description: "Custom transformation".to_string(),
    old_tokens: vec!["old".to_string(), "pattern".to_string()],
    new_tokens: vec!["new".to_string(), "pattern".to_string()],
}
```

### ApiMigrationRule

Rules define individual transformations:

```rust
use lling_llang::programming::{ApiMigrationRule, Version};

// Function rename rule
let rename_rule = ApiMigrationRule::rename_function(
    "getUser",
    "fetchUser",
    Version::new(1, 0),
    Version::new(2, 0),
);

// Type rename rule
let type_rule = ApiMigrationRule::rename_type(
    "Response",
    "ApiResponse",
    Version::new(1, 0),
    Version::new(2, 0),
);

// Parameter rename (function-scoped)
let param_rule = ApiMigrationRule::rename_parameter(
    Some("configure"),  // Only in configure() calls
    "timeout",
    "timeoutMs",
    Version::new(1, 0),
    Version::new(2, 0),
);

// Multi-token pattern replacement
let pattern_rule = ApiMigrationRule::replace(
    ["this", ".", "setState"],  // Old pattern
    ["setState"],               // New pattern
    Version::new(16, 8),
    Version::new(18, 0),
);

// Deprecation warning (requires manual review)
let deprecation = ApiMigrationRule::deprecate(
    "unsafeMethod",
    "This method is deprecated with no replacement",
    Version::new(1, 0),
    Version::new(2, 0),
);

// Customize rule cost and review requirement
let custom_rule = ApiMigrationRule::rename_function("a", "b", Version::new(1, 0), Version::new(2, 0))
    .with_cost(0.5)      // Higher cost = less preferred
    .manual_review();    // Mark as needing human review
```

### ApiMigrationTransducer

The transducer applies rules to token streams:

```rust
use lling_llang::programming::{ApiMigrationTransducer, ApiMigrationRule, Version};
use lling_llang::semiring::TropicalWeight;

// Create transducer with version context
let mut transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationTransducer::new(
        Version::new(1, 0),  // Source version
        Version::new(2, 0),  // Target version
    );

// Add migration rules
transducer.add_rule(ApiMigrationRule::rename_function(
    "oldFn",
    "newFn",
    Version::new(1, 0),
    Version::new(2, 0),
));

// Apply migration to tokens
let tokens = vec![
    "call".to_string(),
    "oldFn".to_string(),
    "(".to_string(),
    ")".to_string(),
];

let result = transducer.migrate(&tokens);

// Check results
assert_eq!(result.migrated, vec!["call", "newFn", "(", ")"]);
assert_eq!(result.stats.rules_applied, 1);
assert_eq!(result.stats.automatic_migrations, 1);
```

### ApiMigrationBuilder

Fluent API for building transducers:

```rust
use lling_llang::programming::{ApiMigrationBuilder, ApiMigrationRule, Version};
use lling_llang::semiring::TropicalWeight;

let transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
        .add_rule(ApiMigrationRule::rename_function(
            "getUserById",
            "fetchUser",
            Version::new(1, 0),
            Version::new(2, 0),
        ))
        .add_rule(ApiMigrationRule::rename_type(
            "UserData",
            "User",
            Version::new(1, 0),
            Version::new(2, 0),
        ))
        .add_rule(ApiMigrationRule::replace(
            ["callback", "(", "err", ",", "data", ")"],
            ["async", "/", "await"],
            Version::new(1, 0),
            Version::new(2, 0),
        ))
        .build();
```

### MigrationResult and MigrationStats

Detailed feedback from migration operations:

```rust
use lling_llang::programming::{MigrationResult, MigrationStats};

// MigrationResult contains:
let result: MigrationResult = transducer.migrate(&tokens);

// Original input tokens
let original: &Vec<String> = &result.original;

// Transformed output tokens
let migrated: &Vec<String> = &result.migrated;

// IDs of rules that were applied
let applied: &Vec<String> = &result.applied_rules;

// Statistics
let stats: &MigrationStats = &result.stats;
println!("Rules applied: {}", stats.rules_applied);
println!("Automatic: {}", stats.automatic_migrations);
println!("Need review: {}", stats.manual_review_items);
println!("Total cost: {}", stats.total_cost);
```

## Examples

### Basic Function Rename

The simplest migration: renaming a function.

```rust
use lling_llang::programming::{ApiMigrationBuilder, ApiMigrationRule, Version};
use lling_llang::semiring::TropicalWeight;

let transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
        .add_rule(ApiMigrationRule::rename_function(
            "getUsers",
            "fetchUsers",
            Version::new(1, 0),
            Version::new(2, 0),
        ))
        .build();

// Before: getUsers()
let tokens = vec!["getUsers".to_string(), "(".to_string(), ")".to_string()];

// After: fetchUsers()
let result = transducer.migrate(&tokens);
assert_eq!(result.migrated, vec!["fetchUsers", "(", ")"]);
```

### Multi-Token Pattern Replacement

Replace complex patterns involving multiple tokens.

```rust
use lling_llang::programming::{ApiMigrationBuilder, ApiMigrationRule, Version};
use lling_llang::semiring::TropicalWeight;

let transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
        .add_rule(ApiMigrationRule::replace(
            // Old: this.setState({...})
            ["this", ".", "setState"],
            // New: setState(...)
            ["setState"],
            Version::new(16, 8),
            Version::new(18, 0),
        ))
        .build();

// Before: this.setState({ count: 1 })
let tokens = vec![
    "this".to_string(), ".".to_string(), "setState".to_string(),
    "(".to_string(), "{".to_string(), "count".to_string(),
    ":".to_string(), "1".to_string(), "}".to_string(), ")".to_string(),
];

// After: setState({ count: 1 })
let result = transducer.migrate(&tokens);
assert_eq!(result.migrated[0], "setState");
assert_eq!(result.migrated[1], "(");
```

### Version-Constrained Migration

Rules only apply within their version range.

```rust
use lling_llang::programming::{ApiMigrationBuilder, ApiMigrationRule, Version};
use lling_llang::semiring::TropicalWeight;

// Transducer for migrating from v1.0 to v2.0
let transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
        // Rule for v1.0 → v1.5 only
        .add_rule(ApiMigrationRule::rename_function(
            "alpha",
            "beta",
            Version::new(1, 0),
            Version::with_patch(1, 5, 0),
        ))
        // Rule for v1.5 → v2.0 only
        .add_rule(ApiMigrationRule::rename_function(
            "beta",
            "gamma",
            Version::with_patch(1, 5, 0),
            Version::new(2, 0),
        ))
        .build();

// With source v1.0, only first rule applies
let tokens = vec!["alpha".to_string()];
let result = transducer.migrate(&tokens);
assert_eq!(result.migrated, vec!["beta"]);
```

### Handling Deprecations

Flag deprecated APIs for manual review.

```rust
use lling_llang::programming::{ApiMigrationBuilder, ApiMigrationRule, Version};
use lling_llang::semiring::TropicalWeight;

let transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
        .add_rule(ApiMigrationRule::deprecate(
            "unsafeEval",
            "REMOVED: Use safe alternatives instead",
            Version::new(1, 0),
            Version::new(2, 0),
        ))
        .build();

let tokens = vec!["unsafeEval".to_string(), "(".to_string(), ")".to_string()];
let result = transducer.migrate(&tokens);

// Deprecation requires manual review
assert_eq!(result.stats.manual_review_items, 1);
assert_eq!(result.stats.automatic_migrations, 0);

// Output contains the warning message
assert!(result.migrated.contains(&"REMOVED: Use safe alternatives instead".to_string()));
```

### Building a WFST for Composition

Convert migration rules to a WFST for composition with other transducers.

```rust
use lling_llang::programming::{ApiMigrationBuilder, ApiMigrationRule, Version};
use lling_llang::semiring::TropicalWeight;
use lling_llang::wfst::Wfst;

let transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
        .add_rule(ApiMigrationRule::rename_function(
            "old",
            "new",
            Version::new(1, 0),
            Version::new(2, 0),
        ))
        .build();

// Build WFST with cost mapping
let fst = transducer.build_wfst(TropicalWeight::new);

// WFST can be composed with other transducers
// (lexical normalizers, syntax checkers, etc.)
```

## Pattern Libraries

Pre-built migration rules for common frameworks.

### React Class to Function Components

```rust
use lling_llang::programming::patterns;

// Get migration rules for React modernization
let react_rules = patterns::react_class_to_function();

// Includes transformations like:
// - componentDidMount → useEffect(() => { ... })
// - componentWillUnmount → useEffect(() => { return () => { ... }})
// - this.setState → setState
// - this.state → state
```

### Python 2 to Python 3

```rust
use lling_llang::programming::patterns;

let python_rules = patterns::python2_to_python3();

// Includes transformations like:
// - print "x" → print("x")
// - xrange → range
// - raw_input → input
// - unicode → str
// - .iteritems() → .items()
// - .iterkeys() → .keys()
// - .itervalues() → .values()
```

### jQuery to Vanilla JavaScript

```rust
use lling_llang::programming::patterns;

let jquery_rules = patterns::jquery_to_vanilla_js();

// Includes transformations like:
// - $("#id") → document.getElementById("id")
// - $(".class") → document.querySelectorAll(".class")
// - .addClass() → .classList.add()
// - .removeClass() → .classList.remove()
// - .toggleClass() → .classList.toggle()
// - .attr() → .getAttribute()
```

### Using Pattern Libraries

```rust
use lling_llang::programming::{ApiMigrationBuilder, Version, patterns};
use lling_llang::semiring::TropicalWeight;

// Combine pattern libraries with custom rules
let transducer: ApiMigrationTransducer<TropicalWeight> =
    ApiMigrationBuilder::new(Version::new(2, 7), Version::new(3, 10))
        // Add Python 2→3 rules
        .add_rules(patterns::python2_to_python3())
        // Add your custom rules
        .add_rule(ApiMigrationRule::rename_function(
            "my_legacy_fn",
            "my_modern_fn",
            Version::new(2, 7),
            Version::new(3, 10),
        ))
        .build();
```

## Migration Workflow

A typical migration workflow:

```
1. Define Version Context
   ├── Source version (current codebase)
   └── Target version (desired API version)
                  ↓
2. Collect Migration Rules
   ├── Use pattern libraries for known frameworks
   ├── Add custom rules for project-specific APIs
   └── Mark breaking changes as manual_review
                  ↓
3. Tokenize Source Code
   └── Split code into token stream
                  ↓
4. Apply Migration
   ├── Pattern matching against rules
   ├── Apply transformations
   └── Track statistics
                  ↓
5. Review Results
   ├── Automatic migrations: apply directly
   └── Manual review items: human verification needed
                  ↓
6. Reconstruct Code
   └── Join tokens back into source code
```

## Performance Considerations

### Rule Indexing

Rules are indexed by their first token for O(1) lookup:

```rust
// Internal structure:
// rules_by_token: HashMap<String, Vec<ApiMigrationRule>>
//
// Lookup: O(1) for first token, then linear scan of matching rules
```

### Cost-Based Selection

When multiple rules match, costs influence selection:

| Rule Type | Default Cost | Notes |
|-----------|--------------|-------|
| Rename (function/type/param) | 0.1 | Low cost, highly automatic |
| Replace pattern | 0.2 | Slightly higher due to complexity |
| Deprecation | 1.0 | High cost, needs manual review |

### WFST Compilation

Building a WFST is O(n) in the number of rules. The resulting WFST can be:
- **Composed** with other transducers
- **Determinized** for faster matching
- **Minimized** to reduce state count

## Related Topics

- [WFST Composition](../algorithms/composition.md): Composing transducers
- [WFST Operations](../architecture/wfst-operations.md): Core WFST operations
- [Determinization](../algorithms/determinization.md): Making transducers deterministic
- [Semirings](../architecture/semirings.md): Weight algebras for transducers
