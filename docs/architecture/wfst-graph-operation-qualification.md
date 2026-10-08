# WFST graph-operation qualification boundary

This note qualifies the bounded graph-operation adapters introduced in the
G6.S5 campaign. It is local executable evidence, not a formal proof of every
generic source, label, or semiring implementation. The primary test is
`tests/wfst_graph_operation_qualification.rs`; operation-specific tests are
`bounded_composition.rs`, `bounded_determinization.rs`,
`bounded_intersection.rs`, and `bounded_projection.rs`.

## Supported operations

| Adapter | State exploration | Label/weight scope | Deliberate exclusion |
| --- | --- | --- | --- |
| `BoundedComposition` | FIFO product states with sequencing epsilon filter | matching intermediate labels; semiring product | no assertion that arbitrary cyclic path enumeration terminates |
| `BoundedDeterminization` | FIFO normalized weighted subsets; sorted input labels | divisible, totally ordered, hashable/equatable weights; orderable labels | reachable input epsilon, conflicting outputs, undefined division, unbounded post-trim |
| `BoundedIntersection` | checked acceptors, then bounded composition | equal input/output labels including epsilon; same label and weight types on both sides | malformed starts/targets and non-acceptor arcs |
| `BoundedInputProjection` / `BoundedOutputProjection` | FIFO reachable source states through the existing lazy `ProjectSource` | copied label on both sides; original arc/final weights | unreachable states are intentionally not emitted |

All four use the shared `OperationPlan`/`OperationSession` identity, limits,
cancel signal, checkpoint, and `Complete`/`Incomplete` outcome separation.
None emits `Approximate`, because no operation-specific sound error bound has
been implemented. In particular, hitting a resource cap cannot mean
“probably complete.” The generic complete-result cache conservatively rejects
dynamic plans, including completed graph traversals: their terminal frontier
is not in the generic plan. A checkpoint contains identity, cursor, charges,
and elapsed time, not a serialized graph/frontier. Resume therefore requires
the exact live in-memory continuation and the last returned checkpoint.

The identity contract is caller-assisted. A nonzero caller-computed content
binding must encode all relevant operand contents, order, and algorithm
configuration; `run` checks a fresh observed binding. A dishonest or stale
caller-supplied digest is not independently detectable by these adapters.
Canonical checkpoint and quality receipt bytes round-trip deterministically
for a captured event; receipts bind quality and resource accounting, not the
full result payload or a trusted source attestation.

## Independent semantic and stack evidence

The test's deliberately recursive depth-limited oracle enumerates tiny
acyclic source paths and their tropical weights. Composition joins the first
path's output sequence to the second path's input sequence. Intersection
uses the same join on acceptors. Determinization compares the minimum-cost
weighted language before and after subset construction. Projection compares
the source path language after selecting input or output labels. These oracles
are independent of the adapters' state maps and worklists and include sibling
branches in differing orders. They do not claim an exhaustive all-graph
correctness proof.

The resource gate runs all four adapters on chains of 64, 512, and 4096 arcs,
and on a 2048-way fanout, inside a thread with a 128 KiB native stack. The
state and arc counts are exact. Charged work and logical heap on the 512-arc
chain remain between seven and nine times the 64-arc values, matching the
eightfold input increase; sampled callback stack-address span grows by at
most 8 KiB. This is evidence against depth-proportional native recursion in
these adapters, not a portable byte-accurate stack proof.

The abstract work meter counts admitted state and arc operations (and new
residual entries for determinization). The logical-heap meter counts specified
structural entries plus caller-metered label/weight payloads. Neither is a
peak resident-memory guarantee. A single source expansion, its transient
batch, result snapshot clone, underlying source, and acceptor-admission scan
can allocate or run outside those accepted-output charges. Time and
cancellation are polled between atomic states, not at every source arc.
Production users needing hard memory/time isolation must impose independent
input-size/process limits and use an appropriately metered source provider.

The negative suite sets a zero-state cap for every adapter and checks the
`Incomplete` receipt tag, zero cursor, and canonical checkpoint round-trip.
It also constructs an `Approximate` outcome with a bound identity and verifies
that the exact-only cache refuses it. Operation-specific tests cover partial
cache exclusion, cancellation, source drift, stale checkpoints, malformed
inputs, and limit/resume equivalence.

## Remaining work

The legacy determinizer's epsilon-removal prepass is not yet wrapped by this
bounded contract; the new adapter explicitly rejects reachable input-epsilon
arcs. The generic semiring constraints do not prove that a custom semiring's
`divide` and total order implement the algebra assumed by weighted subset
construction. Acceptor validation is a complete pre-traversal scan but is not
charged to the graph traversal session. These are visible API boundaries, not
silent completeness claims. They should be addressed before claiming a
fully bounded arbitrary-input WFST pipeline.
