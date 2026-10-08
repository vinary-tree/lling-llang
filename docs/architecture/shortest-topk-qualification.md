# Shortest and top-k WFST qualification

The S6 qualification suite is
[`tests/wfst_shortest_topk_qualification.rs`](../../tests/wfst_shortest_topk_qualification.rs).
It checks two separate questions: whether each adapter returns the right
source paths, and whether interruption can be distinguished from proof of
exhaustion. The small-graph oracle is recursive by design and never used by
the production algorithms. The production searches use iterative queues and
flat predecessor/path records.

## Semantic domain and oracle

A weighted finite-state transducer (WFST) here has finitely many source
states and ordered outgoing arcs. An accepting path starts at the source
start state and ends at a state with finite final weight. Tropical path cost
is the sum of its arc weights and final weight; an absent path has infinite
cost. Both adapters require nonnegative finite contributing arc and final
weights. They fail closed on encountered negative/NaN weights, invalid
targets, and finite-cost overflow. Positive-infinite weights do not
contribute a path. Unreachable malformed source components are outside the
reachable-path claim.

The independent oracle recursively enumerates every accepting path in
twelve small acyclic graphs. It compares top-k's complete path multiset and
nondecreasing costs, shortest's minimum cost and exact selected provenance,
and every witness's ordered source arc indices, labels, original weights,
final state and final weight. A separate equal-weight sibling fixture checks
the algorithms' deterministic policies. Shortest chooses the first
discovered equal-cost best path; top-k orders equal-cost candidates by path
depth, then stable insertion order. The policies need not produce the same
first path when equally optimal paths have different depths.

## Resource and interruption controls

One test thread has a 128 KiB native stack and runs both adapters on
64-, 512- and 4096-arc chains and a 2048-branch fanout. State and arc
charges must equal the admitted graph visits, and the 64-to-512 work and
caller-metered logical-heap charges must grow within a linear slope window.
Sampled label-meter callback addresses must remain in a small stack band;
the 128 KiB thread is the stronger operational stack gate. This does not
claim a process-RSS bound, an allocator-capacity bound, or universal
complexity over graph representations other than the tested `VectorWfst`.

The suite also checks exact equality of resumed and uncapped results after
state, work, heap, depth, emitted-path and frontier limits, plus cooperative
cancellation. A separate label-meter comparison proves that copying source
labels adds exactly the caller-supplied payload charge. A limit just below
the final heap usage interrupts materialization and can resume without
reporting false completion.

`OperationOutcome` quality tags and canonical checkpoint bytes are stable
and round-trip in these tests. They record the adapter's claim and resource
cursor; a receipt alone does not authenticate result contents. The suite
deliberately relabels incomplete cursors as `Complete` and confirms that the
shared complete-only cache rejects dynamic-plan entries. An exact result is
usable through `into_complete()`, but dynamic search results are not
automatically cacheable under a static complete-result key.

## Exactness boundary

Shortest returns `Complete(None)` only after its reachable frontier is
exhausted. Top-k returns `Complete(paths)` only after its pending prefix and
terminal frontier is empty. Thus a finite `max_paths` on an infinite path
language yields a ranked prefix under `Incomplete(PathLimit)`, not a false
finite-language proof. Path-depth and frontier limits are likewise typed
incomplete states. A source content digest is supplied and re-observed by
the caller; the adapter does not itself hash source data. Checkpoint resume
requires the same live in-memory machine, not a serialized checkpoint alone.

This suite is local executable evidence for the stated algorithms and test
sizes. It is not a machine-checked proof of all possible WFSTs or a trusted
external verification receipt.
