# Bounded exact SFT pre-image

The pre-image of an output-language acceptor under a symbolic finite
transducer (SFT) is the set of original input words for which **some** SFT
output is accepted. `symbolic::bounded_preimage::BoundedSftPreimage` builds
an exact reachable symbolic finite automaton (SFA) for that language,
provided an exact finite output-guard pullback oracle is available. The
result is a product of one SFT state and one output-SFA state. Product
acceptance requires both component states to accept.

For one SFT transition with output function `f` and one output-SFA start
state, an `ExactOutputPreimageOracle` supplies finitely many cases. Each
case identifies an output-SFA endpoint and an input predicate that is true
**exactly** for inputs on which `f` produces an output sequence reaching
that endpoint. Cases may overlap when the output SFA is nondeterministic.
The adapter conjoins each case predicate with the SFT transition guard and
discards unsatisfiable conjunctions. This contract is the semantic authority
for a `Complete` result; a merely possible or conservative case must never
be returned as exact. An oracle also reports inspected acceptor arcs and
derivation work for the shared resource account.

`SameAlgebraPreimageOracle` provides a built-in exact route when SFT input
and output use the same Boolean algebra:

- Epsilon output leaves the output-SFA state unchanged.
- A finite constant output is simulated through *all* matching output-SFA
  transitions; distinct reachable endpoints become cases.
- Identity output uses each outgoing output-SFA guard directly as the
  input-side pullback.
- Opaque map and flat-map closures return a typed
  `UnrepresentableOutput` error. A caller that knows an exact finite
  pullback for particular closures can implement the oracle trait and bind
  its semantics explicitly.

The built-in route does not pretend that every computed function is
unrepresentable: it refuses to infer a predicate from an opaque closure.
The custom-oracle test demonstrates a known-constant map and flat-map whose
exact cases are derived by the finite-constant algorithm. The caller is
responsible for the proof obligation that its oracle's cases match its
closures and Boolean algebra for **all** domain values, not merely a test
sample. Source and oracle content digests must include semantic versions of
opaque closures; Rust cannot hash their behavior automatically.

```text
charge and index both ordered source transition vectors
sort and create every initial SFT/SFA product pair
while an unexpanded product pair exists:
    preflight one state visit
    for each satisfiable outgoing SFT transition:
        ask the exact oracle for output-SFA endpoint/guard cases
        conjoin each case with the SFT input guard
        validate targets, costs, and cancellation
    charge the whole batch, then append first-discovered product states
    and source-order symbolic transitions
return Complete only when the product frontier is empty
```

Indexing and every accepted expansion are charged for states, inspected
arcs, abstract work and caller-metered logical heap. Time and cancellation
are polled at progress boundaries. A rejected batch leaves the product
frontier and partial graph unchanged; `Incomplete` cannot enter the generic
complete-only cache. Resume requires the exact checkpoint and same live
in-memory map, frontier, result and oracle. A checkpoint alone is not a
serialized pre-image. The adapter does not trim unreachable result states
with an unbounded post-pass; every emitted state is reachable from an
initial product pair by construction.

Generic Boolean-algebra predicates and custom oracles are assumed to obey
their documented semantics. The adapter cannot preempt one arbitrary
oracle invocation or hard-cap transient allocations inside it. Its heap
limit covers retained result/index structures plus caller-reported
predicate payloads, not process RSS. An invalid reachable source target,
missing exact pullback or representation overflow fails closed rather than
producing an approximate result under `Complete`.

`tests/bounded_sft_preimage.rs` checks shallow language parity against
independent SFT-output/SFA-acceptance simulation, nondeterministic constant
output paths, typed computed-output rejection, a known-exact custom oracle,
all shared limits, cancellation, checkpoint/resume equivalence, binding
drift, malformed targets and partial/false-complete cache exclusion.
