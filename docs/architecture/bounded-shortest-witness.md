# Bounded shortest WFST witness

`algorithms::BoundedShortestWitness` extracts one exact accepting path from a
finite WFST with nonnegative tropical arc and final costs. An exact `Complete`
value is `Some(witness)` when a path exists and `None` when all reachable
states have been exhausted without an accepting path. A cap or cancellation
returns `Incomplete` with no guessed witness. Reachable negative, invalid, or
overflowing weights and malformed transition targets are explicit errors.
This API does not claim arbitrary-semiring shortest paths.

The cursor owns a min-cost priority queue, best known cost and predecessor
for each discovered source state, and a set of settled states. A settled
state is never expanded again. For equal-cost alternatives, the first path
discovered through source arc order wins; the priority queue's monotone
sequence number makes this deterministic. Nonnegative costs justify settling
a state at its least reachable cost. Zero-cost cycles cannot cause unbounded
state revisits because an equal-cost rediscovery does not replace the best
record. Stale queued entries are discarded with charged work.

Each state is processed as one atomic batch:

```text
peek the least-cost live state
preflight one state visit
inspect ordered outgoing arcs and compute candidate improvements
validate weights, targets, resource cost and cancellation
charge the whole batch, then settle the state and append improvements
```

After frontier exhaustion, the best final state is selected by total arc
plus final cost, then first-discovery sequence. Reconstruction follows the
settled predecessor chain backward, validates each source arc, reverses that
flat list, and copies complete provenance into ordered `ShortestWitnessStep`
records: source state, arc index, input/output labels, target, and original
weight. Final-state ID, final weight, and total weight remain separate. This
copy has its own work and caller-metered label-payload charge. If it exceeds
a limit, the outcome remains `Incomplete`; the live continuation can resume
and finish the copy without rerunning settled states.

The operation plan binds an algorithm version and a caller-supplied nonzero
content digest. Callers must digest the actual source graph and provide a
fresh observation at each run boundary. A checkpoint contains identity,
cursor, elapsed time, and accepted resource charges but not the priority
queue, predecessors, or source; resume requires the exact live continuation.
The shared complete-only cache conservatively refuses dynamic plans. Logical
heap charging covers retained queue/predecessor entries and copied witness
storage, with generic label payload supplied by the caller; it is not a hard
process-RSS or transient-allocation cap. The source graph itself is outside
the operation's output-resource account.

`tests/bounded_shortest_witness.rs` checks a hand-worked equal-cost tie and
full arc provenance, state/arc/work/heap/time caps, cancellation, stale
source/checkpoint rejection, exact resume, a limit at the final reconstruction
boundary, zero-cost cycles, exact empty-language reporting, and malformed or
negative-weight inputs. Deep/wide stack and resource slopes are the separate
S6 qualification gate.
