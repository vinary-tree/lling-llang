# Bounded WFST operation contract

`lling_llang::wfst::operation` is the shared resource and outcome boundary for
WFST/SFT algorithms. The fixed-plan adapter drives an ordered sequence of
existing lazy state expansions. A dynamic-plan cursor now supports bounded
breadth-first composition materialization; determinization, projection, top-k
and symbolic operations are separate follow-on leaves. Legacy expansion calls
and their accepted results are unchanged.

An `OperationPlan` binds a versioned algorithm ID, exact ordered state IDs,
the live `SourceSnapshot`, and a caller-computed content binding of the actual
source. The latter is required even when a source uses the default immutable
snapshot. `run_lazy` checks a freshly observed content binding and both live
snapshot views before work. A checkpoint can resume only the same plan and
source; accepted state/arc/work/logical-heap charges and monotonic elapsed time
carry forward. Its versioned fixed-width big-endian bytes round-trip exactly.
The checksum detects accidental corruption; it is not a signature or a
trusted-source attestation.

`OperationPlan::new_dynamic` has a separate domain-separated identity from a
fixed empty plan. Each accepted discovered state advances the cursor and its
resource charge atomically. The checkpoint records that cursor and costs, but
does **not** serialize the algorithm's frontier, result graph, or source. A
dynamic-plan operation must retain those alongside the checkpoint and verify
their correspondence on resume. The generic complete-result cache rejects
dynamic plans because their terminal count cannot be established from the
plan alone. A fixed-plan lazy runner rejects dynamic plans.

Limits cover ordered state visits, arcs, abstract work (`1 + outgoing arcs`
per lazy visit), caller-metered logical heap bytes, and monotonic elapsed
nanoseconds. Cancellation is shared with the lazy source. The heap meter must
account for generic label and weight payloads; the library cannot infer their
actual allocations or promise an RSS cap. Operation-specific algorithms must
charge their own work beyond this lazy adapter. A limit or cancellation leaves
the next state unaccepted. If a fresh completed state exceeded its budget,
the adapter removes it from the wrapper cache before returning. Previously
cached *individually complete* states may be reused; a partial operation is
never inserted under a complete-result cache key.

`OperationOutcome<T>` has disjoint `Complete`, `Approximate` (with an explicit
operation-specific bound identity), and `Incomplete` (partial prefix, exact
reason and checkpoint) variants. `CompleteResultCache` accepts only `Complete`.
An approximation is not exact, and an exhausted cap is not evidence that all
paths were enumerated. Canonical quality/resource receipts bind plan identity,
cursor, usage and outcome class, but not the result payload; consumers needing
content attestation must bind a separate digest. Elapsed time is inherently
runtime-dependent; canonical encoding is deterministic for a captured
checkpoint, not bit-identical across separately timed executions.

The integration tests cover exact limit/cancellation resume equivalence,
stale order/content/snapshot rejection, corrupted and trailing checkpoint
bytes, state/arc/heap/time limits, no source invocation on a preflight limit,
rejected-cache eviction, and exclusion of incomplete/approximate results from
the complete-result cache. These tests are boundary evidence, not a claim that
all WFST algorithms have already adopted the contract.

## Bounded composition adapter

`composition::BoundedComposition` owns the lazy product, reachable-state map,
FIFO frontier, and partial `VectorWfst`. It preserves `materialize`'s source
transition order and breadth-first state numbering. Its caller-supplied
nonzero content binding must include **both** operand contents, operand order,
and epsilon-filter semantics; each `run` checks a freshly observed binding.
This is an application identity obligation, not an automatically computed
hash. The in-memory `resume` accepts only the exact last checkpoint returned
by that continuation, preserving its frontier and spent resources. Persisting
only the checkpoint bytes cannot reconstruct a composition.

The adapter preflights one state visit before expansion, then charges the full
state/arc/work/logical-heap cost before committing finality, targets, or arcs.
The structural charge covers graph transitions and newly discovered target
records; a caller meter covers label/weight payloads and any extra adapter
storage. It is not an RSS or peak-allocation bound: expansion and returned
snapshot cloning may allocate transiently, and source adapters can retain
their own caches. Caps and cancellation produce `Incomplete`, never an
approximation. The tests compare shallow branched output with a hand oracle
and the legacy materializer, and check limit/cancel atomicity, source drift,
stale checkpoint rejection, exact resume and partial-cache exclusion.

## Bounded determinization adapter

`algorithms::BoundedDeterminization` runs a weighted powerset construction
without invoking the legacy recursive epsilon-removal prepass or unbounded
post-trimming. It accepts an immutable WFST with reachable input-epsilon-free
paths, a caller-computed content binding, shared limits and cancellation.
The caller must include all source states, arcs, labels, weights and their
ordering in that binding and recompute it before every run. A reachable input
epsilon, conflicting output labels for one input, or undefined weight
division is a typed error, never a purported complete transducer.

The machine's worklist contains pairs of output-state ID and normalized
weighted subset. For each subset it visits source states in state-ID order,
groups arcs by input label in sorted label order, combines weights for equal
targets, factors out the minimum weight and uses the residual-weight vector
as its deduplication key. It computes the entire outgoing batch before
charging or mutating the result. A rejected batch leaves its output state
untouched and returns `Incomplete`; a completed batch appends states and arcs
in deterministic order. The exact subset map, worklist and partial result
remain inside the in-memory continuation. Its checkpoint alone cannot
reconstruct them, and a dynamic result cannot enter the generic complete-only
cache. The abstract work charge includes source arcs, output arcs, and newly
stored residuals; logical heap charging includes structural entries plus
caller-metered label and weight payloads. It is not a hard RSS cap.

This adapter deliberately returns an untrimmed deterministic WFST. Trimming
can be a separate bounded operation; applying the existing unbounded `connect`
inside this adapter would invalidate its limits. The shallow hand oracle
checks weighted language cost and sorted branches, while cap/cancel/resume
and unsupported-input tests check that incomplete or erroneous attempts are
never silently classified as complete.

## Bounded acceptor intersection adapter

`composition::BoundedIntersection` first checks **every** arc of both finite
operands, including unreachable arcs, for equal input and output labels.
Epsilon arcs have neither label. A non-acceptor operand is rejected with its
operand, state and arc index before an intersection result exists. This full
input-admission scan is separate from the bounded product traversal; callers
must independently bound input acquisition. The algorithm then delegates to
the explicit FIFO product-state worklist of `BoundedComposition`, using the
sequencing epsilon filter. Thus matching labels advance both acceptors, and
epsilon arcs advance one side in canonical order. Matched arc weights use the
semiring product, so no particular numeric weight type is assumed.

The content binding must digest both validated operand graphs in order and
the default epsilon-filter semantics. The same state/arc/work/logical-heap/
time limits, cancellation, typed quality outcome and exact in-memory resume
rules as bounded composition apply. A partial intersection is never a complete
acceptor. Tests use hand-computed branch labels and weights, an independent
epsilon-path cost oracle, and invalid-operand, cap, cancellation, source-drift
and resume cases.

## Bounded reachable projection adapters

`wfst::BoundedInputProjection` and `BoundedOutputProjection` materialize the
existing `ProjectSource` lazily, but only from the source start state. Each
keeps a FIFO queue of source states and a first-discovery map to output state
IDs. The input variant copies source input labels to both sides of each
output arc; the output variant does the same for source output labels. Neither
changes arc or final weights. An unreachable source state is not expanded or
emitted. The direction has its own versioned operation ID, so its checkpoint
cannot resume the opposite projection even over identical source contents.

Before expanding a state, the adapter preflights its visit. It then obtains a
complete state from the existing lazy lifecycle, validates reachable targets,
and charges state/arc/work/logical-heap cost before mutating the output graph.
A rejected newly expanded state is removed from the lazy cache. The caller
must meter generic label/weight payload storage and recompute the source
content binding at each run boundary. The checkpoint is paired with the live
frontier and partial graph; bytes alone are not a persistent projection.
Limits or cancellation return `Incomplete` and cannot enter a complete-only
cache. Tests cover both projection directions, source-order and weight
preservation, exclusion of unreachable states, caps, cancellation, source
drift, cross-direction checkpoint rejection, exact resume, and malformed
reachable targets.

The composition, determinization, intersection, and projection adapters form
the G6.S5 graph-operation set. Their independent cross-operation evidence,
small-stack gate, and exact exclusions are collected in
[WFST graph-operation qualification](wfst-graph-operation-qualification.md).
This link is the S5 integration boundary; it does not elevate local tests to
trusted verification or remove the qualification note's stated limits.

## Bounded accepting-path extraction adapters

`algorithms::BoundedShortestWitness` and `BoundedTopK` extend the same
source-bound plan/session, cancellation, typed outcome and exact in-memory
checkpoint contract to accepting-path extraction. They require nonnegative
tropical costs; unsupported reachable weights or malformed targets are
errors. The first returns one exact minimum-cost witness, while the second
returns an ordered sequence of distinct accepting paths. Both preserve each
source arc's location, labels and weight plus the final state/weight.
Neither calls a recursive production traversal. Top-k has additional path
depth, emitted-path and frontier limits, all of which yield `Incomplete`
when work remains. An exhausted output quota alone is never treated as
proof that the language has only that many paths.

The algorithms and their boundaries are described in
[shortest witness](bounded-shortest-witness.md) and
[top-k witnesses](bounded-topk-witnesses.md). Their independent recursive
shallow oracles, 128 KiB deep/wide gates, resource slopes, interruption
equivalence and false-complete controls are in
[S6 qualification](shortest-topk-qualification.md). These local checks do
not turn caller-metered logical heap into an RSS guarantee or supply a
trusted external verification receipt.

## Bounded symbolic-transducer adapters

The G6.S7 adapters apply the same bounded-operation contract to symbolic
finite transducers (SFTs). Concrete-input [transduction](bounded-sft-transduction.md)
and [composition](bounded-sft-composition.md) return exact ordered witnesses
with flat transition provenance. [Pre-image](bounded-sft-preimage.md),
[post-image](bounded-sft-postimage.md), and
[domain restriction](bounded-sft-domain-restriction.md) construct reachable
symbolic product machines when their exact predicate representations exist.
Post-image has explicit epsilon edges and output-word chains; the old
single-guard `post_image` method was removed because it could not represent
those cases exactly. Opaque computed functions require an exact finite
symbolic oracle for image/pre-image rather than an unmarked approximation.

The [S7 qualification](bounded-sft-operations-qualification.md) compares all
five adapters to an independent shallow relation enumerator and exercises
deep and wide 128 KiB-stack controls, linear charged resource slopes,
interruption, resumption and partial-cache exclusion. This integration
boundary claims the S7 implementation scope, not arbitrary closure
representability, an RSS bound, or trusted external verification.
