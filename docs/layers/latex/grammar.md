# LaTeX Grammar

The LaTeX grammar module defines context-free grammar rules for syntactic filtering of LaTeX documents.

## Grammar Structure

```rust
pub struct LatexGrammar {
    grammar: Arc<Grammar>,
}
```

The grammar wraps lling-llang's generic CFG implementation with LaTeX-specific productions.

## Grammar Variants

### Standard Grammar

Full LaTeX document structure with AMS math extensions:

```rust
let grammar = LatexGrammar::standard()?;
```

Productions include:
- Document structure (Document, Content, ContentList)
- Environments (begin/end pairing)
- Commands with arguments
- Math mode (inline and display)
- Groups and text

### Math Grammar

Optimized for mathematical expressions only:

```rust
let grammar = LatexGrammar::math()?;
```

Productions include:
- Math expressions (MathExpr, MathGroup)
- Fractions (\frac, \dfrac, \tfrac)
- Subscripts and superscripts
- Operators (\sum, \prod, \int, \lim)
- Greek letters and symbols

### Minimal Grammar

Fast parsing with basic brace matching:

```rust
let grammar = LatexGrammar::minimal()?;
```

Productions:
- Document → ContentList
- ContentList → Content ContentList | ε
- Content → Group | Token
- Group → { ContentList }

## Builder API

Customize grammar construction:

```rust
use lling_llang::layers::latex::LatexGrammarBuilder;

let grammar = LatexGrammarBuilder::new()
    .with_amsmath(true)      // Include AMS math extensions
    .with_environments(true)  // Include environment rules
    .with_commands(true)      // Include command rules
    .with_math(true)          // Include math mode rules
    .build_standard()?;
```

## Grammar Rules

### Document Structure

```
Document → ContentList
ContentList → Content ContentList | Content | ε
Content → Environment | Command | MathMode | Group | Text
```

### Environments

```
Environment → BeginEnv ContentList EndEnv
BeginEnv → \begin { EnvName } OptArgs
EndEnv → \end { EnvName }
EnvName → equation | align | figure | table | ...
OptArgs → [ ArgContent ] OptArgs | ε
```

### Commands

```
Command → CommandName Arguments
CommandName → \section | \ref | \cite | \textbf | ...
Arguments → OptArgs ReqArgs
ReqArgs → Group ReqArgs | ε
```

### Math Mode

```
MathMode → InlineMath | DisplayMath
InlineMath → $ MathContent $ | \( MathContent \)
DisplayMath → $$ MathContent $$ | \[ MathContent \]
MathContent → MathExpr MathContent | ε
MathExpr → MathGroup | MathCommand | MathToken
MathGroup → { MathContent }
MathCommand → \frac MathGroup MathGroup | \sqrt OptArgs MathGroup | ...
```

### Math Grammar (Specialized)

```
MathDocument → MathExprList
MathExprList → MathExpr MathExprList | ε
MathExpr → MathGroup | Fraction | Script | Operator | Symbol | Identifier | Number
Fraction → \frac MathGroup MathGroup
Script → MathExpr _ MathGroup | MathExpr ^ MathGroup | ...
Operator → \sum Limits | \prod Limits | \int Limits | \lim Limits
Limits → _ MathGroup ^ MathGroup | _ MathGroup | ^ MathGroup | ε
```

## Terminal Symbols

### Delimiters

| Terminal | Matches |
|----------|---------|
| `lbrace` | `{` |
| `rbrace` | `}` |
| `lbracket` | `[` |
| `rbracket` | `]` |
| `lparen` | `(` |
| `rparen` | `)` |
| `dollar` | `$` |
| `ddollar` | `$$` |

### Commands

| Terminal | Matches |
|----------|---------|
| `cmd_begin` | `\begin` |
| `cmd_end` | `\end` |
| `cmd_frac` | `\frac` |
| `cmd_sqrt` | `\sqrt` |
| `greek_cmd` | `\alpha`, `\beta`, ... |

### Environment Names

| Terminal | Matches |
|----------|---------|
| `env_equation` | `equation` |
| `env_align` | `align` |
| `env_figure` | `figure` |
| `env_table` | `table` |
| ... | ... |

## AMS Math Extensions

When `with_amsmath(true)`:

```
EnvName → env_align_star | env_gather | env_gather_star |
          env_multline | env_split | env_cases |
          env_matrix | env_pmatrix | env_bmatrix | env_vmatrix | ...

MathCommand → \text Group | \boldsymbol MathGroup |
              \binom MathGroup MathGroup | ...
```

## Using the Grammar

### Direct Parsing

```rust
use lling_llang::cfg::EarleyParser;

let grammar = LatexGrammar::standard()?;
let parser = EarleyParser::new(grammar.grammar());

// Parse a token sequence
let tokens = vec!["\\begin", "{", "equation", "}", "x", "\\end", "{", "equation", "}"];
let parse_result = parser.parse(&tokens);

match parse_result {
    Ok(forest) => println!("Valid LaTeX"),
    Err(e) => println!("Parse error: {:?}", e),
}
```

### Lattice Parsing

```rust
let grammar = LatexGrammar::standard()?;
let parser = EarleyParser::new(grammar.grammar());

// Parse all paths through a lattice
let forest = parser.parse_lattice(&lattice)?;
let valid_edges = forest.collect_used_edges();
```

### Grammar Sharing

Share grammar across threads:

```rust
use std::sync::Arc;

let grammar = LatexGrammar::standard()?;

// Get Arc reference for sharing
let grammar_arc = grammar.grammar_arc();

// Use in multiple threads
let handle = std::thread::spawn(move || {
    let parser = EarleyParser::new(&grammar_arc);
    // ...
});
```

## Accessing Grammar Properties

```rust
let grammar = LatexGrammar::standard()?;

// Number of production rules
let num_productions = grammar.grammar().num_productions();
println!("Grammar has {} productions", num_productions);

// Get underlying grammar for custom parsing
let inner = grammar.grammar();
```

## Error Handling

```rust
use lling_llang::layers::latex::LatexGrammarError;

match LatexGrammar::standard() {
    Ok(grammar) => {
        // Use grammar
    }
    Err(LatexGrammarError::Construction(msg)) => {
        eprintln!("Failed to build grammar: {}", msg);
    }
    Err(LatexGrammarError::Configuration(msg)) => {
        eprintln!("Invalid configuration: {}", msg);
    }
}
```

## Related

- [Overview](./overview.md): Layer architecture
- [Validator](./validator.md): Structural validation
- [Repair](./repair.md): Repair strategies
