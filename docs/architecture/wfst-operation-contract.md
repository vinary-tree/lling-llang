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
