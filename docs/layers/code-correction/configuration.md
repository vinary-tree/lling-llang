# Language Configuration

The code correction layer supports multiple programming languages with language-specific keywords, syntax tokens, and correction behaviors.

## Supported Languages

```rust
pub enum CodeCorrectionLanguage {
    Python,
    Rust,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
    Rholang,    // F1R3FLY.io
    MeTTa,      // F1R3FLY.io
    Generic,
    Custom(String),
}
```

### Language Detection

```rust
// Parse from string (case-insensitive)
let lang = CodeCorrectionLanguage::from_str("python");  // Python
let lang = CodeCorrectionLanguage::from_str("py");      // Python
let lang = CodeCorrectionLanguage::from_str("RUST");    // Rust
let lang = CodeCorrectionLanguage::from_str("rs");      // Rust
let lang = CodeCorrectionLanguage::from_str("rholang"); // Rholang
let lang = CodeCorrectionLanguage::from_str("rho");     // Rholang
let lang = CodeCorrectionLanguage::from_str("metta");   // MeTTa

// Unknown languages become Custom
let lang = CodeCorrectionLanguage::from_str("mylan");
// CodeCorrectionLanguage::Custom("mylan")
```

## Language Properties

### Keywords

Each language has a set of reserved keywords:

| Language | Sample Keywords |
|----------|----------------|
| Python | `def`, `class`, `if`, `for`, `import`, `return`, `async`, `await` |
| Rust | `fn`, `let`, `mut`, `struct`, `impl`, `trait`, `match`, `async` |
| JavaScript | `function`, `var`, `let`, `const`, `class`, `async`, `await` |
| Go | `func`, `var`, `type`, `struct`, `interface`, `go`, `defer` |
| Rholang | `new`, `contract`, `for`, `match`, `select`, `Nil`, `bundle` |
| MeTTa | `!`, `=`, `:`, `match`, `let`, `type`, `import` |

```rust
let lang = CodeCorrectionLanguage::Rust;
println!("Rust keywords: {:?}", lang.keywords());
// ["fn", "let", "mut", "const", "static", "struct", "enum", "impl", ...]
```

### Syntax Tokens

Language-specific punctuation and operators:

| Language | Sample Tokens |
|----------|--------------|
| Python | `(`, `)`, `:`, `->`, `...`, `**`, `//` |
| Rust | `::`, `->`, `=>`, `..`, `..=`, `?`, `#`, `'` |
| JavaScript | `===`, `!==`, `=>`, `?.`, `??`, `...` |
| Rholang | `|`, `<-`, `<<`, `>>`, `/\`, `\/`, `@`, `~` |
| MeTTa | `!`, `=`, `:`, `->`, `$`, `?`, `*` |

```rust
let lang = CodeCorrectionLanguage::Rholang;
println!("Rholang syntax: {:?}", lang.syntax_tokens());
// ["(", ")", "[", "]", "{", "}", "|", ";", "<-", "<<", ">>", ...]
```

### Structural Properties

```rust
// Does this language use braces for blocks?
lang.uses_braces()
// Python: false (uses indentation)
// Rust: true
// MeTTa: false (uses S-expressions)

// Does this language require semicolons?
lang.uses_semicolons()
// Python: false
// Rust: true
// JavaScript: true (optional but common)
// Rholang: true
```

## CodeCorrectionConfig

### Creating Configuration

```rust
use lling_llang::layers::code_correction::CodeCorrectionConfig;

// For a specific language
let config = CodeCorrectionConfig::new("rust");

// Default (generic)
let config = CodeCorrectionConfig::default();
```

### Configuration Fields

```rust
pub struct CodeCorrectionConfig {
    /// Target programming language
    pub language: CodeCorrectionLanguage,

    /// Maximum corrections per token
    pub max_corrections_per_token: usize,  // default: 5

    /// Maximum edit distance for corrections
    pub max_edit_distance: usize,          // default: 2

    /// Cost per edit operation
    pub edit_cost: f64,                    // default: 1.0

    /// Cost for inserting a missing token
    pub insertion_cost: f64,               // default: 2.0

    /// Cost for deleting an unexpected token
    pub deletion_cost: f64,                // default: 1.5

    /// Boost (negative cost) for keyword matches
    pub keyword_boost: f64,                // default: 0.5

    /// Syntax recovery configuration
    pub syntax_config: Option<SyntaxRecoveryConfig>,

    /// Pattern-aware configuration
    pub pattern_config: Option<PatternAwareConfig>,

    /// Token vocabulary (keywords + syntax)
    pub vocabulary: HashSet<Arc<str>>,

    /// Whether to preserve original tokens
    pub keep_original: bool,               // default: true

    /// Minimum token length for edit distance
    pub min_token_length: usize,           // default: 2
}
```

### Builder Pattern

```rust
let config = CodeCorrectionConfig::new("python")
    .with_max_corrections(10)
    .with_max_edit_distance(3)
    .with_edit_cost(0.5)
    .with_insertion_cost(1.5)
    .with_deletion_cost(1.0)
    .with_keyword_boost(1.0)
    .with_syntax_recovery(SyntaxRecoveryConfig::default())
    .with_pattern_aware(PatternAwareConfig::python_patterns())
    .with_vocabulary(vec!["my_func", "my_class"])
    .with_keep_original(true)
    .with_min_token_length(3);
```

### Vocabulary Management

The vocabulary is automatically populated from language keywords and syntax:

```rust
let config = CodeCorrectionConfig::new("rust");

// Vocabulary includes:
// - All Rust keywords ("fn", "let", "mut", ...)
// - All Rust syntax tokens ("(", ")", "::", "->", ...)

// Check if a token is in vocabulary
config.is_in_vocabulary("fn");     // true
config.is_in_vocabulary("myvar");  // false

// Check if a token is a keyword
config.is_keyword("fn");           // true
config.is_keyword("(");            // false
```

### Adding Custom Vocabulary

```rust
let config = CodeCorrectionConfig::new("python")
    .with_vocabulary(vec![
        "numpy", "pandas", "tensorflow",  // Common libraries
        "my_project_function",            // Project-specific
    ]);
```

## Language-Specific Configuration

### Python

```rust
let config = CodeCorrectionConfig::new("python")
    // Python doesn't use semicolons or braces
    .with_syntax_recovery(
        SyntaxRecoveryConfig::default()
            .with_semicolon_insertion(false)
            .with_bracket_balancing(true)  // Still balance (), [], {}
    )
    .with_pattern_aware(PatternAwareConfig::python_patterns());
```

### Rust

```rust
let config = CodeCorrectionConfig::new("rust")
    .with_syntax_recovery(
        SyntaxRecoveryConfig::default()
            .with_semicolon_insertion(true)
            .with_insertable_tokens(vec!["->", "=>", "::"])
    )
    .with_pattern_aware(PatternAwareConfig::rust_patterns())
    .with_vocabulary(vec!["Result", "Option", "Vec", "String"]);
```

### Rholang

```rust
let config = CodeCorrectionConfig::new("rholang")
    .with_syntax_recovery(
        SyntaxRecoveryConfig::default()
            .with_semicolon_insertion(true)
            .with_insertable_tokens(vec!["|", "<-", "<<", ">>"])
            .with_bracket_pair("{*", "*}")  // Set operations
    )
    .with_pattern_aware(PatternAwareConfig::rholang_patterns())
    .with_vocabulary(vec![
        "stdout", "stdoutAck", "stderr", "stderrAck",
        "rho:registry:lookup", "rho:io:stdout",
    ]);
```

### MeTTa

```rust
let config = CodeCorrectionConfig::new("metta")
    .with_syntax_recovery(
        SyntaxRecoveryConfig::default()
            .with_semicolon_insertion(false)  // No semicolons
            .with_bracket_balancing(true)     // Balance ()
    )
    .with_pattern_aware(PatternAwareConfig::metta_patterns())
    .with_vocabulary(vec![
        "atom", "symbol", "expression", "grounded",
        "Type", "Atom", "Symbol", "Expression",
    ]);
```

## Configuration Templates

### IDE Completion

For real-time code completion:

```rust
fn ide_config(language: &str) -> CodeCorrectionConfig {
    CodeCorrectionConfig::new(language)
        .with_max_corrections(3)       // Quick, few options
        .with_max_edit_distance(1)     // Only minor typos
        .with_keyword_boost(2.0)       // Strongly prefer keywords
        .with_keep_original(true)
}
```

### Batch Repair

For offline code repair:

```rust
fn batch_config(language: &str) -> CodeCorrectionConfig {
    CodeCorrectionConfig::new(language)
        .with_max_corrections(20)      // Many options
        .with_max_edit_distance(3)     // More tolerance
        .with_syntax_recovery(SyntaxRecoveryConfig::default())
        .with_pattern_aware(get_patterns(language))
}
```

### Learning/Teaching

For educational tools showing alternatives:

```rust
fn learning_config(language: &str) -> CodeCorrectionConfig {
    CodeCorrectionConfig::new(language)
        .with_max_corrections(10)      // Show multiple options
        .with_max_edit_distance(2)
        .with_keyword_boost(0.3)       // Don't over-prefer keywords
        .with_keep_original(true)      // Show original vs corrections
}
```

## Combining Configurations

### Multi-Language Support

```rust
use std::collections::HashMap;

struct MultiLanguageCorrector {
    configs: HashMap<String, CodeCorrectionConfig>,
}

impl MultiLanguageCorrector {
    fn new() -> Self {
        let mut configs = HashMap::new();

        configs.insert("python".to_string(), CodeCorrectionConfig::new("python"));
        configs.insert("rust".to_string(), CodeCorrectionConfig::new("rust"));
        configs.insert("rholang".to_string(), CodeCorrectionConfig::new("rholang"));
        configs.insert("metta".to_string(), CodeCorrectionConfig::new("metta"));

        Self { configs }
    }

    fn get_config(&self, language: &str) -> &CodeCorrectionConfig {
        self.configs.get(language)
            .unwrap_or_else(|| self.configs.get("generic").unwrap())
    }
}
```

### Project-Specific Configuration

```rust
fn project_config(base_language: &str, project_vocabulary: &[&str]) -> CodeCorrectionConfig {
    CodeCorrectionConfig::new(base_language)
        .with_vocabulary(project_vocabulary.iter().map(|s| *s))
        // Add project patterns from mining
        .with_pattern_aware(mine_project_patterns())
}
```

## Validation

### Checking Configuration

```rust
fn validate_config(config: &CodeCorrectionConfig) -> Result<(), String> {
    if config.max_corrections_per_token == 0 {
        return Err("max_corrections must be > 0".to_string());
    }

    if config.edit_cost <= 0.0 {
        return Err("edit_cost must be positive".to_string());
    }

    if config.vocabulary.is_empty() && matches!(config.language, CodeCorrectionLanguage::Generic) {
        return Err("Generic language needs vocabulary".to_string());
    }

    Ok(())
}
```

## Serialization

For saving/loading configurations:

```rust
// Using serde (if enabled)
#[cfg(feature = "serde")]
{
    let config = CodeCorrectionConfig::new("rust");

    // Serialize
    let json = serde_json::to_string(&config)?;

    // Deserialize
    let loaded: CodeCorrectionConfig = serde_json::from_str(&json)?;
}
```

## See Also

- [Overview](overview.md) - Code correction introduction
- [Syntax Recovery](syntax-recovery.md) - Error recovery layer
- [Pattern-Aware Correction](pattern-aware.md) - Idiom-based boosting
