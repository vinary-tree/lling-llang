# Homoglyph Disambiguation

The homoglyph disambiguator handles visually similar characters with different mathematical meanings based on context.

## Common Homoglyph Confusions

| Characters | Possible Meanings |
|------------|-------------------|
| `x`, `×`, `✕` | Variable x, Multiplication |
| `-`, `−`, `–`, `—` | Subtraction, Unary minus |
| `0`, `O`, `o` | Digit zero, Variable O |
| `1`, `l`, `I`, `\|` | Digit one, Variable l/I |
| `'`, `′` | Apostrophe, Prime symbol |
| `.`, `·`, `⋅` | Decimal point, Multiplication, Sentence end |

## Glyph Meanings

```rust
pub enum GlyphMeaning {
    Multiplication,
    Variable(String),
    Subtraction,
    UnaryMinus,
    Addition,
    Equals,
    Digit(u8),
    Letter(char),
    Prime,
    Apostrophe,
    Quote,
    Division,
    Ratio,
    DecimalPoint,
    SentenceEnd,
    Separator,
    DecimalSeparator,
    Grouping,
    FunctionApplication,
    Unknown,
}
```

## Homoglyph Sets

```rust
pub struct HomoglyphSet {
    /// The glyphs in this confusion set.
    pub glyphs: Vec<char>,
    /// Possible meanings for these glyphs.
    pub meanings: Vec<GlyphMeaning>,
    /// Canonical form (preferred glyph).
    pub canonical: char,
}
```

### Creating Homoglyph Sets

```rust
use lling_llang::layers::mathml::homoglyph::{HomoglyphSet, GlyphMeaning};

let set = HomoglyphSet::new(
    vec!['a', 'ɑ', 'α'],
    vec![GlyphMeaning::Variable("a".to_string())],
    'a',  // canonical form
);

assert!(set.contains('a'));
assert!(set.contains('α'));
assert!(!set.contains('b'));
assert_eq!(set.canonical, 'a');
```

## Disambiguator

```rust
pub struct HomoglyphDisambiguator {
    confusion_sets: HashMap<char, HomoglyphSet>,
    config: DisambiguatorConfig,
}
```

### Configuration

```rust
pub struct DisambiguatorConfig {
    /// Weight for context-based disambiguation.
    pub context_weight: f32,
    /// Weight for frequency-based disambiguation.
    pub frequency_weight: f32,
    /// Whether to normalize to canonical forms.
    pub normalize: bool,
}

// Default
DisambiguatorConfig {
    context_weight: 0.7,
    frequency_weight: 0.3,
    normalize: true,
}
```

## Creating a Disambiguator

```rust
use lling_llang::layers::mathml::homoglyph::{
    HomoglyphDisambiguator,
    DisambiguatorConfig
};

// Default configuration
let disambiguator = HomoglyphDisambiguator::new();

// Custom configuration
let config = DisambiguatorConfig {
    normalize: false,
    ..Default::default()
};
let disambiguator = HomoglyphDisambiguator::with_config(config);
```

## Math Context

Context for disambiguation decisions:

```rust
pub struct MathContext {
    /// Whether we're in math mode.
    pub in_math_mode: bool,
    /// Whether we're in text mode.
    pub in_text_mode: bool,
    /// Previous token (if any).
    pub prev_token: Option<String>,
    /// Next token (if any).
    pub next_token: Option<String>,
    /// Current nesting depth for parentheses.
    pub paren_depth: i32,
    /// Whether the previous token was an operator.
    pub prev_was_operator: bool,
    /// Whether the previous token was a number.
    pub prev_was_number: bool,
    /// Topic/domain hint.
    pub domain: Option<MathDomain>,
}
```

### Math Domains

```rust
pub enum MathDomain {
    General,
    Algebra,
    Analysis,
    LinearAlgebra,
    NumberTheory,
    Statistics,
    Physics,
    ComputerScience,
}
```

## Checking Ambiguity

```rust
let disambiguator = HomoglyphDisambiguator::new();

// Check if a character is ambiguous
assert!(disambiguator.is_ambiguous('x'));
assert!(disambiguator.is_ambiguous('×'));
assert!(disambiguator.is_ambiguous('-'));
assert!(disambiguator.is_ambiguous('0'));

// Non-ambiguous character
assert!(!disambiguator.is_ambiguous('q'));
```

## Getting Confusion Sets

```rust
let disambiguator = HomoglyphDisambiguator::new();

if let Some(set) = disambiguator.get_confusion_set('x') {
    println!("Confusable with 'x': {:?}", set.glyphs);
    println!("Canonical form: {}", set.canonical);
}
```

## Disambiguating Glyphs

```rust
use lling_llang::layers::mathml::homoglyph::{
    HomoglyphDisambiguator,
    MathContext,
    GlyphMeaning
};

let disambiguator = HomoglyphDisambiguator::new();

// After a number, 'x' is likely multiplication
let context = MathContext {
    in_math_mode: true,
    prev_was_number: true,
    ..Default::default()
};

let meaning = disambiguator.disambiguate('x', &context);
// Returns Multiplication or Variable depending on context scoring
```

### Context-Based Disambiguation Examples

```rust
let disambiguator = HomoglyphDisambiguator::new();

// 'x' after operator -> Variable
let context = MathContext {
    in_math_mode: true,
    prev_was_operator: true,
    ..Default::default()
};
let meaning = disambiguator.disambiguate('x', &context);
assert!(matches!(meaning, GlyphMeaning::Variable(_)));

// '-' after number -> Subtraction
let context = MathContext {
    prev_was_number: true,
    ..Default::default()
};
let meaning = disambiguator.disambiguate('-', &context);
assert!(matches!(meaning, GlyphMeaning::Subtraction));

// '-' after operator -> Unary minus
let context = MathContext {
    prev_was_operator: true,
    ..Default::default()
};
let meaning = disambiguator.disambiguate('-', &context);
assert!(matches!(meaning, GlyphMeaning::UnaryMinus));
```

## Normalization

Replace homoglyphs with canonical forms:

```rust
let disambiguator = HomoglyphDisambiguator::new();

// Normalize multiplication sign
let normalized = disambiguator.normalize("2×3");
assert_eq!(normalized, "2x3");

// Normalize minus sign
let normalized = disambiguator.normalize("a−b");
assert_eq!(normalized, "a-b");
```

### Disable Normalization

```rust
let config = DisambiguatorConfig {
    normalize: false,
    ..Default::default()
};
let disambiguator = HomoglyphDisambiguator::with_config(config);

// With normalize=false, returns original
let result = disambiguator.normalize("2×3");
assert_eq!(result, "2×3");
```

## Getting Confusables

```rust
let disambiguator = HomoglyphDisambiguator::new();

let confusables = disambiguator.get_confusables('x');
assert!(confusables.contains(&'×'));
assert!(confusables.contains(&'X'));

// Non-ambiguous returns empty
let confusables = disambiguator.get_confusables('q');
assert!(confusables.is_empty());
```

## Built-in Confusion Sets

The disambiguator registers standard confusion sets:

### Multiplication vs Variable x

```rust
glyphs: ['x', 'X', '×', '✕', '✖', '⨯']
meanings: [Variable("x"), Multiplication]
canonical: 'x'
```

### Minus Signs and Dashes

```rust
glyphs: ['-', '−', '–', '—', '‐', '‑']
meanings: [Subtraction, UnaryMinus]
canonical: '-'
```

### Zero vs Letter O

```rust
glyphs: ['0', 'O', 'o', 'Ο', 'ο', '০']
meanings: [Digit(0), Variable("O"), Variable("o")]
canonical: '0'
```

### One vs Letter l/I

```rust
glyphs: ['1', 'l', 'I', '|', 'ǀ', 'ⅼ']
meanings: [Digit(1), Variable("l"), Variable("I")]
canonical: '1'
```

### Prime Symbols

```rust
glyphs: ['\'', '′', 'ʹ', 'ˈ', ''']
meanings: [Prime, Apostrophe]
canonical: '\''
```

### Division Symbols

```rust
glyphs: ['/', '÷', '∕', '⁄']
meanings: [Division]
canonical: '/'
```

### Period/Dot Ambiguities

```rust
glyphs: ['.', '·', '⋅', '∙']
meanings: [DecimalPoint, Multiplication, SentenceEnd]
canonical: '.'
```

### Greek/Latin Confusions

The disambiguator handles Greek-Latin lookalikes:

| Latin | Greek | Cyrillic |
|-------|-------|----------|
| A | Α (Alpha) | - |
| B | Β (Beta) | В (Ve) |
| E | Ε (Epsilon) | Е (Ie) |
| H | Η (Eta) | Н (En) |
| K | Κ (Kappa) | К (Ka) |
| M | Μ (Mu) | М (Em) |
| N | Ν (Nu) | - |
| P | Ρ (Rho) | Р (Er) |
| T | Τ (Tau) | Т (Te) |
| Y | Υ (Upsilon) | У (U) |

## Registering Custom Confusion Sets

```rust
let mut disambiguator = HomoglyphDisambiguator::new();

disambiguator.register(HomoglyphSet::new(
    vec!['∞', '8', '∝'],
    vec![
        GlyphMeaning::Variable("infinity".to_string()),
        GlyphMeaning::Digit(8),
    ],
    '∞',
));

assert!(disambiguator.is_ambiguous('∞'));
```

## Scoring Algorithm

The disambiguator scores each possible meaning based on context:

| Factor | Impact |
|--------|--------|
| Previous was number | `+0.3` for Multiplication |
| Previous was operator | `+0.3` for Variable |
| In math mode | `+0.2` for Multiplication |
| Explicit glyph (`×`, `⋅`) | `+0.4` for Multiplication |
| At expression start | `+0.4` for UnaryMinus |
| Between digits | `+0.5` for DecimalPoint |
| After variable in math | `+0.4` for Prime |

The score for each candidate meaning is the weighted sum
`` `score = context_weight · context_factors + frequency_weight · frequency` ``;
the highest-scoring meaning wins, and a margin below `disambiguation_threshold`
is recorded as an `` `AmbiguousGlyph` `` issue.

## Related

- [Overview](./overview.md): Layer architecture
- [Types](./types.md): Type system
- [Checker](./checker.md): Type checking

## References

- *Unicode Technical Standard #39: Unicode Security Mechanisms* — the canonical
  treatment of confusables and the "skeleton" normalization this disambiguator's
  canonical-form mapping mirrors: <https://www.unicode.org/reports/tr39/>.
- [Mohri 2002](../../BIBLIOGRAPHY.md#ref-mohri2002) — weighted finite-state
  transducers; disambiguation decisions reweight (and, under `normalize`, rewrite)
  the glyph labels of the correction lattice.
