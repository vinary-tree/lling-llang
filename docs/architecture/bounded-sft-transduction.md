# Bounded concrete-input SFT transduction

`symbolic::bounded_transduce::BoundedSftTransduction` enumerates exact accepting
paths of one symbolic finite transducer (SFT) for one concrete input word.
An SFT transition consumes exactly one input element when its Boolean-algebra
guard evaluates true and appends the sequence produced by its output
function. The five supported output forms are epsilon (empty output), a
constant sequence, identity, map (one element), and flat-map (a sequence).
The machine evaluates every taken transition's actual output function; it
does not replace a computed output with an unconstrained symbolic guess.

The machine sorts initial-state IDs and preserves the source transition
vector's order within each state. A charged, incrementally built index
retains transition indices, including a linear scan of transitions outside
reachable paths. The path frontier is first-in, first-out; all paths for a
fixed-length input have the same depth, so this gives deterministic initial-
state/transition-order output. An append-only arena node stores one parent
index and one transition's output fragment. No output prefix is cloned when
a path branches. At an accepting state after the whole input is consumed,
the machine follows parents iteratively, concatenates fragments and emits
one witness with ordered source transition indices, consumed inputs, and
per-step spans of the output sequence. Each witness carries the caller's
source and input bindings so those indices remain attributable to the
specific ordered source and word.

```text
charge and build state buckets; index each source transition in order
sort and enqueue initial states
while a pending path exists:
    if input remains:
        evaluate its state's guards on the next concrete input element
        compute matching output fragments and validate targets
        preflight the whole child batch, charge it, then enqueue children
    else:
        charge one terminal visit
        if accepting, reconstruct and emit the complete path and output
return Complete only after the path frontier is empty
```

The shared `OperationLimits` account for indexed and traversed arcs, accepted
path-state visits, abstract work, caller-metered logical heap and monotonic
time. `SftTransductionLimits` cap emitted paths and pending frames. A
resource limit, cancellation or unexhausted path quota returns a typed
`Incomplete` with an exact accepted prefix. Batch preflight leaves a refused
frame pending, so raising limits and resuming the same live machine yields
the uncapped result. A checkpoint identifies the source/word plan and
resource cursor; it does not serialize the path arena, index or closures.
The generic complete-only cache refuses this dynamic plan.

The caller must supply nonzero content digests for both the SFT and input
word and re-observe them at every run boundary. A source digest must include
ordered transitions, guards, output-function semantics or a stable version
identifier for function closures, state acceptance and initial states.
Because arbitrary Rust closures cannot be inspected, exact resume requires
their behavior to be deterministic, pure, total and unchanged across runs.
The adapter checks bindings and polls around callback invocation, but it
cannot preempt a nonterminating callback or hard-cap transient callback
allocations. Its logical heap charges retained fragments and copied witness
elements plus caller-reported payload bytes, not process RSS.

`tests/bounded_sft_transduction.rs` checks an independent recursive shallow
oracle and hand-computed outputs across all five output forms, multiple
initial states, false guards, full step provenance, indexed/source/path
limits, exact resume, cancellation, stale bindings, label-meter deltas,
empty input and malformed reachable endpoints. The later S7 qualification
boundary supplies the cross-operation deep/wide stack and slope checks.
