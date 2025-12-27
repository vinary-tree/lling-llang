# MORK Rule Layer

The MORK Rule Layer applies declarative logic rules to filter and reweight lattice paths based on grammar and semantic constraints.

## Overview

**MORK** (Meta Operational Reasoning Kernel) is a rule engine in the F1R3FLY.io ecosystem. The `MorkRuleLayer` uses MORK to:

- **Declarative rules**: Express constraints as logic programs
- **Pattern matching**: Match patterns in token sequences
- **Rule chaining**: Compose rules for complex constraints
- **Incremental evaluation**: Efficient re-evaluation on changes

## Rule Language

### Basic Syntax

MORK rules follow a logic programming style:

```lisp
;; Rule definition
(rule <name> <head> <body>)

;; Head: what the rule derives
;; Body: conditions that must hold

(rule valid-sentence
  (sentence ?subj ?verb ?obj)
  (and (noun ?subj)
       (verb ?verb)
       (noun ?obj)))
```

### Pattern Variables

Variables start with `?`:

```lisp
?x        ; Matches any single token
?_        ; Anonymous variable (match but don't bind)
?tokens*  ; Matches zero or more tokens (Kleene star)
?tokens+  ; Matches one or more tokens (Kleene plus)
```

### Built-in Predicates

```lisp
;; Token classification
(noun ?x)           ; ?x is a noun
(verb ?x)           ; ?x is a verb
(adjective ?x)      ; ?x is an adjective
(determiner ?x)     ; ?x is a determiner

;; String operations
(starts-with ?x "pre")  ; ?x starts with "pre"
(ends-with ?x "ing")    ; ?x ends with "ing"
(contains ?x "abc")     ; ?x contains "abc"
(matches ?x "[0-9]+")   ; ?x matches regex

;; Comparisons
(eq ?x ?y)          ; ?x equals ?y
(ne ?x ?y)          ; ?x not equal ?y
(lt ?x ?y)          ; ?x < ?y (numeric)
(gt ?x ?y)          ; ?x > ?y (numeric)

;; Position constraints
(at-position ?x 0)  ; ?x is at position 0
(before ?x ?y)      ; ?x appears before ?y
(adjacent ?x ?y)    ; ?x immediately precedes ?y
```

### Logical Combinators

```lisp
;; Conjunction
(and <expr1> <expr2> ...)

;; Disjunction
(or <expr1> <expr2> ...)

;; Negation
(not <expr>)

;; Implication
(implies <condition> <consequence>)
```

## Rule Types

### Grammar Rules

Enforce grammatical structure:

```lisp
;; Subject-verb agreement
(rule subject-verb-agreement
  (valid ?sentence)
  (implies (and (at-position (noun ?n) 0)
                (singular ?n))
           (at-position (verb-singular ?v) 1)))

;; Determiner-noun pairing
(rule det-noun
  (valid ?sentence)
  (implies (at-position (determiner ?d) ?i)
           (at-position (noun ?n) (+ ?i 1))))
```

### Semantic Rules

Enforce meaning constraints:

```lisp
;; Animate subjects for certain verbs
(rule animate-subject
  (valid ?sentence)
  (implies (and (verb ?v) (requires-animate ?v))
           (exists ?subj (and (before ?subj ?v)
                             (animate ?subj)))))

;; Selectional restrictions
(rule edible-object
  (valid ?sentence)
  (implies (verb-eat ?v)
           (exists ?obj (and (after ?v ?obj)
                            (edible ?obj)))))
```

### Filtering Rules

Remove specific patterns:

```lisp
;; No double determiners
(rule no-double-det
  (invalid ?sentence)
  (exists ?d1 ?d2
    (and (determiner ?d1)
         (determiner ?d2)
         (adjacent ?d1 ?d2))))

;; No sentence-final prepositions (prescriptive)
(rule no-final-prep
  (invalid ?sentence)
  (and (at-position (preposition ?p) (- (length ?sentence) 1))))
```

### Reweighting Rules

Adjust path weights:

```lisp
;; Prefer active voice
(rule prefer-active
  (weight-adjustment ?sentence -0.5)  ; Bonus (tropical: lower is better)
  (active-voice ?sentence))

;; Penalize passive voice
(rule penalize-passive
  (weight-adjustment ?sentence 1.0)   ; Penalty
  (passive-voice ?sentence))
```

## Rule Compilation

### Rule Sets

```rust
// Future API
use lling_llang::layers::MorkRuleLayer;

// Parse rule set from string
let rules = r#"
    (rule det-noun
      (valid ?s)
      (implies (at-position (determiner ?d) ?i)
               (at-position (noun ?n) (+ ?i 1))))

    (rule no-double-det
      (invalid ?s)
      (exists ?d1 ?d2
        (and (determiner ?d1)
             (determiner ?d2)
             (adjacent ?d1 ?d2))))
"#;

let layer = MorkRuleLayer::parse(rules)?;
```

### From Files

```rust
// Load from file
let layer = MorkRuleLayer::from_file("grammar_rules.mork")?;

// Load multiple files
let layer = MorkRuleLayer::builder()
    .load_file("base_grammar.mork")?
    .load_file("semantic_rules.mork")?
    .load_file("style_rules.mork")?
    .build()?;
```

### Programmatic Construction

```rust
use lling_llang::layers::mork::{Rule, Predicate, Pattern};

let layer = MorkRuleLayer::builder()
    .rule(Rule::new("det-noun")
        .head(Predicate::valid("?s"))
        .body(Predicate::implies(
            Predicate::at_position(Pattern::determiner("?d"), "?i"),
            Predicate::at_position(Pattern::noun("?n"), Pattern::add("?i", 1))
        )))
    .build()?;
```

## Using the Layer

### Basic Usage

```rust
use lling_llang::layers::MorkRuleLayer;

let rules = r#"
    (rule valid-structure
      (valid ?s)
      (and (at-position (determiner ?_) 0)
           (at-position (noun ?_) 1)
           (at-position (verb ?_) 2)))
"#;

let layer = MorkRuleLayer::parse(rules)?;

// Apply to lattice
let filtered = layer.apply(&lattice)?;
```

### In a Pipeline

```rust
use lling_llang::layers::{LayerPipelineBuilder, MorkRuleLayer, MeTTaILTypeLayer};

let pipeline = LayerPipelineBuilder::new()
    // First: spelling candidates
    .add_layer(SpellingLayer::new())
    // Then: MORK grammar rules
    .add_layer(MorkRuleLayer::parse(grammar_rules)?)
    // Then: semantic type filtering
    .add_layer(MeTTaILTypeLayer::new(checker))
    .build();

let result = pipeline.apply(&lattice)?;
```

### Layer Properties

```rust
impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for MorkRuleLayer {
    fn name(&self) -> &str {
        "mork-rules"
    }

    fn estimated_reduction(&self) -> f64 {
        0.3  // Rules typically reduce to ~30% of paths
    }

    fn can_apply(&self, _lattice: &Lattice<W, B>) -> bool {
        true  // Can apply to any lattice
    }
}
```

## Incremental Evaluation

### How It Works

MORK supports incremental rule evaluation:

1. **Initial evaluation**: Evaluate all rules on full lattice
2. **Change detection**: Track which edges change
3. **Dependency analysis**: Determine affected rules
4. **Partial re-evaluation**: Only re-evaluate affected rules

```rust
// Create incremental evaluator
let mut evaluator = layer.incremental_evaluator();

// Initial evaluation
let result = evaluator.evaluate(&lattice)?;

// Modify lattice
lattice.remove_edge(edge_id);

// Incremental update (faster than full re-evaluation)
let updated = evaluator.update(&lattice, &[Change::RemoveEdge(edge_id)])?;
```

### Performance Benefits

| Operation | Full Evaluation | Incremental |
|-----------|-----------------|-------------|
| Small change | O(paths × rules) | O(affected × rules) |
| Batch changes | O(paths × rules) | O(affected × rules) |
| No change | O(paths × rules) | O(1) |

## Rule Ordering

### Evaluation Order

Rules are evaluated in dependency order:

```lisp
;; Base facts (evaluated first)
(rule is-noun
  (noun ?x)
  (lookup-pos ?x "NN"))

;; Derived facts (evaluated after dependencies)
(rule valid-np
  (noun-phrase ?det ?noun)
  (and (determiner ?det)
       (noun ?noun)
       (adjacent ?det ?noun)))

;; Higher-level rules (evaluated last)
(rule valid-sentence
  (valid ?s)
  (and (noun-phrase ?np-det ?np-noun)
       (verb-phrase ?vp)))
```

### Priority

Rules can have explicit priority:

```lisp
;; Higher priority rules evaluated first
(rule (priority 10) important-check
  (invalid ?s)
  (critical-error ?s))

;; Lower priority for style preferences
(rule (priority 1) style-preference
  (weight-adjustment ?s 0.1)
  (style-violation ?s))
```

## Debugging Rules

### Trace Mode

```rust
let layer = MorkRuleLayer::parse(rules)?
    .with_trace(true);

let result = layer.apply(&lattice)?;

// Print trace
for event in layer.trace() {
    println!("{:?}", event);
}
```

### Rule Statistics

```rust
let stats = layer.rule_statistics();

for (rule_name, stat) in stats {
    println!("Rule '{}': {} matches, {} rejects, {:.2}ms avg",
        rule_name, stat.matches, stat.rejects, stat.avg_time_ms);
}
```

## Integration with OSLF

### Operational Semantic Logic Framework

MORK rules can reference OSLF type judgments:

```lisp
;; Use OSLF type inference
(rule type-correct
  (valid ?s)
  (forall ?t in ?s
    (has-type ?t (infer-type ?t))))

;; Check type relationships
(rule compatible-args
  (valid ?s)
  (implies (and (function ?f) (argument ?a))
           (subtype (type-of ?a) (expected-type ?f))))
```

### MeTTaIL Integration

MORK rules can use MeTTaIL types:

```lisp
;; Reference MeTTaIL type expressions
(rule mettail-typed
  (valid ?s)
  (forall ?t in ?s
    (mettail-check ?t (mettail-infer ?t))))
```

## Current Status

**Status**: Planned

The `MorkRuleLayer` is planned but not yet implemented. Current blockers:

1. MORK Rust bindings not yet available
2. Rule compilation pipeline not finalized
3. Incremental evaluation strategy needs design

## Next Steps

- [Vision](vision.md): F1R3FLY.io integration overview
- [MeTTaIL Layer](mettail-layer.md): Type-based filtering
- [MeTTaTron Layer](mettatron-layer.md): Compiled specifications
- [Layers](../../architecture/layers.md): Layer architecture
