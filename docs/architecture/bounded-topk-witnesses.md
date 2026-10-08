# Bounded top-k WFST witnesses

`BoundedTopK` enumerates complete accepting paths of a finite WFST with
nonnegative tropical weights. It uses a best-first priority queue over an
append-only, parent-indexed path arena. Prefix cost is a lower bound on every
completion; a terminal candidate carries the final weight. Consequently a
terminal is emitted only after all lower-cost prefixes and terminals have been
settled. Equal-cost candidates use path depth and then insertion order, making
the output deterministic and ensuring that finite-depth paths are not starved
by zero-cost cycles. Arc order in the source determines insertion order.

In tropical notation, a path's total weight is the sum of its arc weights and
its final weight. For a prefix with accumulated cost `$c$`, every completion
has cost at least `$c$` because all remaining weights are nonnegative. The
queue therefore orders candidates by `(cost, depth, insertion order)`, with
terminal candidates carrying their exact total cost.

```text
pending := {start prefix at cost 0}
while pending is not empty:
    if a shared or path-specific limit prevents the next atomic step:
        return Incomplete(emitted prefix, reason, checkpoint)
    take the minimum (cost, depth, insertion-order) candidate
    if it is terminal:
        follow its parent indices iteratively and emit a complete witness
    otherwise:
        inspect its ordered outgoing arcs and final weight
        enqueue a terminal candidate when its final weight is finite
        append one arena node and enqueue one prefix per finite-weight arc
return Complete(all emitted witnesses, checkpoint)
```

The queue separates prefixes from terminal candidates because a final state
can have outgoing arcs: emitting its current path must not discard longer
paths through that state. A path arena node stores only a parent index and one
source arc location, so creating a child does not copy its entire prefix.

Each witness contains the ordered source state, arc index, input/output
labels, target, original arc weight, final state/weight and total weight.
There is no recursive path materialization in the implementation.

The shared operation session charges state visits, inspected arcs, work, and
caller-metered logical heap. `TopKLimits` additionally caps path depth,
emitted-path count and frontier length. Reaching any cap while a candidate
remains returns `Incomplete` with an exact emitted prefix and a reason, never
`Complete`. In particular, reaching `max_paths = k` is not a proof that only
`k` accepting paths exist. Exact `Complete` requires an empty frontier. A
cancelled or capped operation cannot populate `CompleteResultCache`.

Resume is in-memory only: the caller retains the machine, supplies exactly its
last checkpoint, a new cancellation token and limits, and observes the same
source binding. This retains every pending candidate and prior output. It is
not a serialized frontier or a cross-process resume protocol. The binding is
a caller-computed content digest, not a digest calculated by this adapter.

For example, with one zero-weight self-loop and one zero-weight arc to a final
state, the first three witnesses have path lengths one, two and three. A
`max_paths` value of three returns those witnesses under `Incomplete(PathLimit)`:
the loop still makes the language infinite. Raising the path limit and
resuming the same machine continues from the retained frontier.

Negative, NaN and negative-infinite reachable weights, malformed reachable
targets and finite-cost overflow fail closed. Positive infinity arcs/finals
cannot contribute an accepting path. This adapter does not support negative
weights, establish bounds for generic label-internal allocations or process
RSS, or promise termination for an infinite path language under unbounded
limits. A finite path, depth, work, frontier, time or cancellation bound makes
such searches interruptible without asserting exhaustion.
