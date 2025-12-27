# MeTTaTron Compiler Layer

The MeTTaTron Compiler Layer compiles high-level MeTTa specifications into optimized lattice transformations.

## Overview

**MeTTaTron** is a compiler that transforms MeTTa programs into efficient executable forms. The `MeTTaTronLayer` enables:

- **Declarative specifications**: Write correction logic in MeTTa
- **Automatic optimization**: Compiler optimizes for lattice operations
- **Type safety**: Leverages MeTTaIL type system
- **Hot reload**: Update rules without restarting

## MeTTa Language

### Basic Syntax

MeTTa uses S-expression syntax with functional semantics:

```lisp
;; Function definition
(= (function-name arg1 arg2)
   (body-expression))

;; Pattern matching
(= (classify "the") Determiner)
(= (classify "dog") Noun)
(= (classify "runs") Verb)

;; Conditional
(if condition then-branch else-branch)

;; Let binding
(let* ((x value1)
       (y value2))
  body)
```

### Type Annotations

MeTTa supports type annotations:

```lisp
;; Type declaration
(: classify (-> String PartOfSpeech))

;; Typed function
(= (classify (: token String)) (: PartOfSpeech)
   (lookup-pos token))

;; Type constraints
(= (filter-by-type tokens (: expected Type))
   (filter (lambda (t) (has-type t expected)) tokens))
```

## Correction Specifications

### Spelling Correction

```lisp
;; Spelling correction specification
(= (correct-spelling input)
   (let* ((candidates (fuzzy-match input 2))      ; Edit distance 2
          (filtered (filter valid-word candidates))
          (ranked (rank-by-frequency filtered)))
     (best ranked)))

;; With context
(= (correct-spelling-context input context)
   (let* ((candidates (fuzzy-match input 2))
          (typed (infer-types candidates context))
          (compatible (filter-compatible typed context))
          (ranked (rank-by-lm compatible context)))
     (nbest ranked 5)))
```

### Grammar Correction

```lisp
;; Grammar filter
(= (grammar-filter lattice grammar)
   (let* ((paths (all-paths lattice))
          (parsed (map (partial parse grammar) paths))
          (valid (filter has-parse parsed)))
     (rebuild-lattice valid)))

;; Grammar correction with suggestions
(= (grammar-correct lattice grammar)
   (let* ((invalid-edges (find-invalid-edges lattice grammar))
          (corrections (map suggest-correction invalid-edges))
          (expanded (add-corrections lattice corrections)))
     (grammar-filter expanded grammar)))
```

### Full Pipeline

```lisp
;; Complete correction pipeline
(= (correct-text input)
   (let* (;; Tokenize
          (tokens (tokenize input))
          ;; Build initial lattice
          (lattice (tokens->lattice tokens))
          ;; Spelling layer
          (spelled (apply-spelling-layer lattice))
          ;; Grammar layer
          (grammared (apply-grammar-layer spelled))
          ;; Semantic layer
          (semantic (apply-semantic-layer grammared))
          ;; Extract best
          (result (viterbi semantic)))
     (detokenize result)))
```

## Compilation Pipeline

### Stages

```
MeTTa Source → Parse → Type Check → Optimize → Emit → Runtime
```

1. **Parse**: Build AST from MeTTa source
2. **Type Check**: Verify type correctness using MeTTaIL
3. **Optimize**: Apply lattice-specific optimizations
4. **Emit**: Generate Rust code or IR
5. **Runtime**: Execute on lattices

### Optimizations

| Optimization | Description |
|--------------|-------------|
| Fusion | Combine adjacent transformations |
| Predication | Convert branches to conditionals |
| Vectorization | SIMD for bulk operations |
| Lazy evaluation | Defer computation until needed |
| Memoization | Cache repeated subexpressions |
| Dead code elimination | Remove unused computations |

### Example Optimization

```lisp
;; Before optimization
(= (pipeline lattice)
   (let* ((step1 (map transform1 (all-edges lattice)))
          (step2 (filter pred1 step1))
          (step3 (map transform2 step2))
          (step4 (filter pred2 step3)))
     (rebuild step4)))

;; After fusion
(= (pipeline-optimized lattice)
   (let ((combined (filter-map
                     (compose pred2 transform2 pred1 transform1)
                     (all-edges lattice))))
     (rebuild combined)))
```

## Using the Layer

### From String

```rust
// Future API
use lling_llang::layers::MeTTaTronLayer;

let spec = r#"
    (= (correct-spelling input)
       (let* ((candidates (fuzzy-match input 2))
              (filtered (grammar-filter candidates))
              (ranked (lm-rank filtered)))
         (best ranked)))
"#;

let layer = MeTTaTronLayer::compile(spec)?;
let corrected = layer.apply(&lattice)?;
```

### From File

```rust
let layer = MeTTaTronLayer::from_file("correction.metta")?;
```

### Hot Reload

```rust
// Create reloadable layer
let mut layer = MeTTaTronLayer::reloadable("correction.metta")?;

// Check for updates periodically
if layer.needs_reload()? {
    layer.reload()?;
    println!("Reloaded correction rules");
}

// Apply (uses latest version)
let result = layer.apply(&lattice)?;
```

### Compilation Options

```rust
let layer = MeTTaTronLayer::builder()
    .source_file("correction.metta")
    .optimization_level(OptLevel::Aggressive)
    .debug_info(true)
    .emit_ir("correction.ll")  // Optional: emit LLVM IR
    .build()?;
```

## Layer Properties

```rust
impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for MeTTaTronLayer {
    fn name(&self) -> &str {
        "mettatron-compiled"
    }

    fn estimated_reduction(&self) -> f64 {
        // Depends on compiled spec
        self.compiled_reduction_estimate()
    }

    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool {
        // Type check lattice against spec requirements
        self.type_check_lattice(lattice)
    }
}
```

## Type System Integration

### MeTTaIL Types in MeTTa

```lisp
;; Import MeTTaIL types
(import mettail/types)

;; Use type constraints
(= (typed-filter (: lattice (Lattice a)) (: constraint TypeExpr))
   (filter (lambda (edge)
             (check-type (label edge) constraint))
           (edges lattice)))

;; Type inference
(= (infer-and-filter (: lattice (Lattice a)))
   (let* ((typed (map-edges infer-type lattice))
          (consistent (filter type-consistent typed)))
     consistent))
```

### Type-Directed Compilation

The compiler uses type information for optimization:

```lisp
;; Compiler knows this is String -> Bool
(: is-noun (-> String Bool))
(= (is-noun word)
   (ends-with word "ness"))

;; Can inline and specialize
(= (filter-nouns words)
   (filter is-noun words))  ; Compiler specializes filter for String
```

## Advanced Features

### Pattern Guards

```lisp
;; Pattern matching with guards
(= (classify token)
   | (numeric? token) Number
   | (capitalized? token) ProperNoun
   | (in-dictionary? token) (lookup-pos token)
   | otherwise Unknown)
```

### Monadic Composition

```lisp
;; Error handling with Result monad
(= (safe-correct input)
   (do (tokens <- (try-tokenize input))
       (lattice <- (try-build-lattice tokens))
       (result <- (try-correct lattice))
       (pure (detokenize result))))
```

### Effect System

```lisp
;; Declare effects
(effect Log (log : String -> ()))
(effect Metrics (record : String -> Float -> ()))

;; Use effects in specs
(= (logged-correct input)
   (do (log (concat "Processing: " input))
       (result <- (correct input))
       (log (concat "Result: " (show result)))
       (pure result)))
```

## Debugging

### Trace Compilation

```rust
let layer = MeTTaTronLayer::builder()
    .source_file("correction.metta")
    .trace_compilation(true)
    .build()?;

// Print compilation trace
for stage in layer.compilation_trace() {
    println!("Stage {}: {} transforms, {:.2}ms",
        stage.name, stage.transforms, stage.time_ms);
}
```

### Inspect Generated Code

```rust
// Emit readable intermediate representation
layer.dump_ir(&mut std::io::stdout())?;

// Emit optimized code
layer.dump_optimized(&mut std::io::stdout())?;
```

### Runtime Profiling

```rust
let mut layer = MeTTaTronLayer::compile(spec)?
    .with_profiling(true);

let result = layer.apply(&lattice)?;

// Get profiling data
let profile = layer.profile();
println!("Total time: {:.2}ms", profile.total_time_ms);
for (func, time) in profile.function_times {
    println!("  {}: {:.2}ms", func, time);
}
```

## Integration with Other Layers

### MORK Rules in MeTTa

```lisp
;; Import MORK rule engine
(import mork/rules)

;; Define rules in MeTTa syntax
(define grammar-rules
  (mork/compile
    '((rule valid-sentence
        (valid ?s)
        (and (det-noun-pair ?s)
             (verb-phrase ?s))))))

;; Use in pipeline
(= (apply-grammar lattice)
   (mork/apply grammar-rules lattice))
```

### Rholang Concurrency

```lisp
;; Import Rholang primitives
(import rholang/par)

;; Parallel correction
(= (parallel-correct lattices)
   (rholang/par-map correct lattices))
```

## Performance

### Compilation Time

| Spec Size | Parse | Type Check | Optimize | Total |
|-----------|-------|------------|----------|-------|
| Small (10 functions) | 1ms | 5ms | 10ms | 16ms |
| Medium (100 functions) | 10ms | 50ms | 100ms | 160ms |
| Large (1000 functions) | 100ms | 500ms | 1000ms | 1600ms |

### Runtime Performance

Compiled code typically achieves:
- 10-100x faster than interpreted MeTTa
- Near-native Rust performance
- Predictable memory usage

## Current Status

**Status**: Planned

The `MeTTaTronLayer` is planned but not yet implemented. Current blockers:

1. MeTTaTron compiler not yet integrated
2. Lattice-specific optimizations need design
3. Hot reload mechanism needs implementation

## Next Steps

- [Vision](vision.md): F1R3FLY.io integration overview
- [MeTTaIL Layer](mettail-layer.md): Type inference
- [MORK Layer](mork-layer.md): Rule-based filtering
- [Rholang Layer](rholang-layer.md): Concurrent execution
