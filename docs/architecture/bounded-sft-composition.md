# Exact bounded SFT relational composition

`symbolic::bounded_compose::BoundedSftComposition` executes two symbolic
finite transducers (SFTs) in sequence on a bound concrete input word. It
computes the exact relation `second(first(word))`: every accepted first-stage
path supplies its complete intermediate word to every compatible
second-stage path. Multiple initial states, overlapping guards and all five
output-function forms—epsilon, constant sequence, identity, map and
flat-map—are evaluated, not approximated. The result is a sequence of
complete `ComposedSftWitness` records with both source transition paths,
intermediate and final output sequences, per-step output spans, source state
endpoints and caller-computed content bindings.

This adapter represents relational composition by retaining the original
sources and evaluating their guards on concrete elements. It does **not**
claim to materialize a standard finite `SymbolicFiniteTransducer<A,C>` with
exact static guards. Arbitrary map/flat-map closures generally have no
representable inverse image in a generic `BooleanAlgebra`; merely checking
the second guard's satisfiability would introduce false paths. A symbolic
materializer needs a separate exact guard-pullback or representability
contract. The executable relation is exact for every concrete word satisfying
the callback contract below, with a new bound machine for each queried word.

## Flat product machine

Both source transition vectors are indexed incrementally into ordered
per-state buckets, with each indexed entry charged and checkpointed. Sorted
initial-state pairs seed a first-in, first-out frontier. A `Ready` frame
evaluates first-stage guards on the next original input element. It stores
the taken first transition's output fragment in an append-only arena node.
An empty fragment advances to another `Ready` frame without moving the
second state. A nonempty fragment creates a `Feeding` frame, which evaluates
every matching second-stage transition for one intermediate element and
branches for every match. After its last element, it returns to `Ready`.
Only a `Ready` frame that consumed the whole original word and reached
accepting states in both sources becomes a witness.

```text
Ready(q1, q2, input cursor):
    for each matching first transition q1 -> q1' in source order:
        middle := first output on the concrete input element
        enqueue Ready(q1', q2) if middle is empty
        otherwise enqueue Feeding(q1', q2, middle, cursor 0)

Feeding(q1', q2, middle, cursor):
    for each matching second transition q2 -> q2' in source order:
        append its output fragment and advance middle cursor
        enqueue Feeding if middle remains, otherwise Ready(q1', q2')
```

The arena stores parent indices, first/second transition events and output
fragments. An intermediate fragment is shared among nondeterministic
second-stage branches. Witness reconstruction follows parent indices
iteratively, then copies ordered intermediate and final output sequences;
there is no native recursive production traversal. All four kinds of batch
(indexing, root seeding, first expansion, second expansion) preflight logical
cost before mutating the live frontier. A refused batch is retained intact.

## Quality and limits

The shared operation session charges source-index and traversal arcs,
accepted product/microstep state visits, abstract work, logical heap and
elapsed time. `SftCompositionLimits` additionally caps emitted witnesses and
pending frames. A cap or cooperative cancellation returns an exact prefix
under `Incomplete`, never an approximate or complete result. `Complete`
requires the frontier to be empty. A checkpoint is bound to both ordered
sources and the input word via three caller-computed digests; resume also
requires the same in-memory machine, not checkpoint bytes alone. Dynamic
results cannot enter the generic static-plan complete-result cache.

The digests must account for guards, state sets, transition order and output
function semantics or stable versions. Arbitrary user closures cannot be
hashed or interrupted by this adapter: they must be pure, deterministic,
total, unchanged across resume, and return finite output. Caller meters
report payload bytes beyond inline domain-element sizes. The resulting
logical heap charge is not a process-RSS or transient-callback-allocation
guarantee. Invalid taken targets and representation overflow fail closed.

`tests/bounded_sft_composition.rs` compares the exact composed outputs with
independent sequential evaluation, checks complete first/second provenance,
repeatable ordering, identity/map guard-mismatch false positives, empty-word
multi-initial products, each shared/path-specific cap, cancellation, source
drift, exact resume, malformed targets and cache rejection. S7's later
cross-operation qualification supplies deep/wide small-stack evidence.
