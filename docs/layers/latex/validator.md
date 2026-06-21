# LaTeX Structural Validator

The LaTeX validator performs structural validation beyond CFG parsing, checking brace matching, environment pairing, and math delimiter balance.

## Validator Structure

```rust
pub struct LatexValidator {
    validate_environments: bool,
    validate_arguments: bool,
    allow_nested_math: bool,
    known_environments: Vec<String>,
}
```

## Basic Usage

```rust
use lling_llang::layers::latex::{LatexValidator, ValidationResult};

let validator = LatexValidator::new();

let tokens = vec!["{", "content", "}"];
let result = validator.validate(&tokens);

if result.is_valid {
    println!("Document is structurally valid");
} else {
    for issue in &result.issues {
        println!("[{}] {}", issue.severity, issue.message);
    }
}
```

## Configuration

### Builder Pattern

```rust
let validator = LatexValidator::new()
    .with_environment_validation(true)   // Check environment names
    .with_argument_validation(true)      // Check command arguments
    .with_nested_math(false)             // Disallow nested math modes
    .add_environment("myenv");           // Add custom environment
```

### Default Environments

The validator recognizes standard LaTeX environments:

**Document Structure:**
- `document`, `abstract`, `titlepage`

**Lists:**
- `itemize`, `enumerate`, `description`

**Math:**
- `equation`, `equation*`, `align`, `align*`
- `gather`, `gather*`, `multline`, `multline*`
- `split`, `cases`, `aligned`, `gathered`

**Matrices:**
- `matrix`, `pmatrix`, `bmatrix`, `vmatrix`, `Vmatrix`, `Bmatrix`

**Floats:**
- `figure`, `figure*`, `table`, `table*`

**Tables:**
- `tabular`, `tabular*`, `array`, `tabularx`

**Theorems:**
- `theorem`, `lemma`, `corollary`, `proposition`
- `definition`, `example`, `remark`, `proof`

**Formatting:**
- `center`, `flushleft`, `flushright`
- `quote`, `quotation`, `verse`

**Code:**
- `verbatim`, `lstlisting`

## Validation Checks

### Brace Matching

Validates matching of `{}`, `[]`, and `()`:

```rust
// Valid
let valid = validator.validate(&["{", "content", "}"]);
assert!(valid.is_valid);

// Invalid - unclosed brace
let invalid = validator.validate(&["{", "content"]);
assert!(!invalid.is_valid);
assert!(invalid.issues.iter().any(|i|
    i.kind == IssueKind::UnmatchedOpenBrace
));

// Invalid - mismatched
let mismatched = validator.validate(&["{", "[", "}", "]"]);
assert!(!mismatched.is_valid);
```

### Environment Matching

Validates `\begin{...}` and `\end{...}` pairs:

```rust
// Valid environment
let valid = validator.validate(&[
    "\\begin", "{", "equation", "}",
    "x", "=", "1",
    "\\end", "{", "equation", "}"
]);
assert!(valid.is_valid);

// Mismatched environment names
let mismatched = validator.validate(&[
    "\\begin", "{", "equation", "}",
    "\\end", "{", "align", "}"
]);
assert!(mismatched.issues.iter().any(|i|
    i.kind == IssueKind::EnvironmentMismatch
));

// Missing end
let missing_end = validator.validate(&[
    "\\begin", "{", "equation", "}", "x"
]);
assert!(missing_end.issues.iter().any(|i|
    i.kind == IssueKind::MissingEnvironmentEnd
));
```

### Math Delimiter Matching

Validates `$`, `$$`, `\[`, `\]`, `\(`, `\)`:

```rust
// Valid inline math
let valid = validator.validate(&["$", "x", "$"]);
assert!(valid.is_valid);

// Unclosed inline math
let unclosed = validator.validate(&["$", "x"]);
assert!(unclosed.issues.iter().any(|i|
    i.kind == IssueKind::UnmatchedMathDelimiter
));

// Nested math mode (error by default)
let nested = validator.validate(&["$", "$", "x", "$", "$"]);
// May report NestedMathMode warning/error
```

## Validation Result

```rust
pub struct ValidationResult {
    pub is_valid: bool,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Check if there are errors (not just warnings)
    pub fn has_errors(&self) -> bool;

    /// Get error issues only
    pub fn errors(&self) -> impl Iterator<Item = &ValidationIssue>;

    /// Get warning issues only
    pub fn warnings(&self) -> impl Iterator<Item = &ValidationIssue>;
}
```

## Validation Issues

```rust
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub kind: IssueKind,
    pub position: Option<usize>,
    pub message: String,
}
```

### Issue Severity

| Severity | Description |
|----------|-------------|
| `Error` | Makes the document invalid |
| `Warning` | May indicate a problem |

### Issue Kinds

| Kind | Description |
|------|-------------|
| `UnmatchedOpenBrace` | `{` without matching `}` |
| `UnmatchedCloseBrace` | `}` without matching `{` |
| `UnmatchedOpenBracket` | `[` without matching `]` |
| `UnmatchedCloseBracket` | `]` without matching `[` |
| `UnmatchedOpenParen` | `(` without matching `)` |
| `UnmatchedCloseParen` | `)` without matching `(` |
| `EnvironmentMismatch` | `\begin{X}` closed by `\end{Y}` |
| `MissingEnvironmentEnd` | `\begin{X}` without `\end{X}` |
| `ExtraEnvironmentEnd` | `\end{X}` without `\begin{X}` |
| `UnmatchedMathDelimiter` | Unbalanced `$`, `$$`, `\[`, etc. |
| `NestedMathMode` | Math mode inside math mode |
| `InvalidArgumentCount` | Wrong number of command arguments |
| `UnknownEnvironment` | Environment not in known list |
| `EmptyRequiredArgument` | Required argument is empty |

## Processing Issues

```rust
let result = validator.validate(&tokens);

// Handle all issues
for issue in &result.issues {
    match issue.severity {
        IssueSeverity::Error => {
            eprintln!("ERROR at {}: {}",
                issue.position.unwrap_or(0),
                issue.message
            );
        }
        IssueSeverity::Warning => {
            eprintln!("WARNING at {}: {}",
                issue.position.unwrap_or(0),
                issue.message
            );
        }
    }
}

// Count errors only
let error_count = result.errors().count();
```

## Integration with Layer

The validator is used internally by `LatexSyntaxLayer`:

```rust
use lling_llang::layers::latex::{LatexSyntaxLayer, LatexGrammar};

let grammar = LatexGrammar::standard()?;
let layer = LatexSyntaxLayer::new(grammar);

// Validate tokens directly through layer
let validation = layer.validate_tokens(&["\\begin", "{", "equation", "}"]);
if !validation.is_valid {
    // Handle validation errors
}
```

### Custom Validator

```rust
let validator = LatexValidator::new()
    .with_environment_validation(false)  // Skip environment check
    .with_nested_math(true);             // Allow nested math

let layer = LatexSyntaxLayer::new(grammar)
    .with_validator(validator);
```

## Unknown Environment Handling

```rust
// Unknown environments generate warnings (not errors)
let result = validator.validate(&[
    "\\begin", "{", "myenv", "}",
    "\\end", "{", "myenv", "}"
]);

// Document is valid (begin/end match)
assert!(result.is_valid);

// But has a warning about unknown environment
assert!(result.warnings().any(|w|
    w.kind == IssueKind::UnknownEnvironment
));
```

## Nested Structure Validation

```rust
// Properly nested
let valid = validator.validate(&[
    "{",
        "[", "(", ")", "]",
    "}"
]);
assert!(valid.is_valid);

// Improperly nested - crossing delimiters
let invalid = validator.validate(&[
    "{", "[", "}", "]"
]);
assert!(!invalid.is_valid);
```

## Related

- [Overview](./overview.md): Layer architecture
- [Grammar](./grammar.md): CFG rules
- [Repair](./repair.md): Repair strategies

## References

- [Earley 1970](../../BIBLIOGRAPHY.md#ref-earley1970) — context-free parsing;
  structural validation complements the parser by checking the non-context-free
  constraints (`` `\begin{X}` `` ⇄ `` `\end{X}` `` name agreement, delimiter
  nesting) that a CFG alone does not enforce.
