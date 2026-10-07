# LlingLlang.jl

LlingLlang.jl builds and lazily composes weighted finite-state transducers and
lets Julia applications provide custom immutable transducers and weight
semirings through Vinary Tree's versioned resource ABI. It also consumes
host-defined lattice values published by LLattice.jl. Start with the package
[README](https://github.com/vinary-tree/lling-llang/tree/master/bindings/julia/LlingLlang#readme)
for ownership, concurrency, security, and complete examples.

The package also exposes bounded native context-free grammar parsing through
`compile_cfg`, `parse_cfg`, `cfg_chart`, `cfg_roots`, and packed forest access.
The [CFG example in the package README](https://github.com/vinary-tree/lling-llang/tree/master/bindings/julia/LlingLlang#bounded-context-free-parsing)
shows token and scalar-WFST input, explicit graph and parser bounds, and
ownership.

## Build a typed scalar WFST

`WfstBuilder{Label,Weight}` accepts the three family label carriers (`UInt8`,
`Char`, and `UInt64`) and seven checked scalar-weight types. The default
`WfstBuilder()` is the familiar Unicode/tropical specialization.

```julia
using LlingLlang

builder = WfstBuilder{UInt64,CountWeight}(size_hint=2)
source = add_state!(builder)
target = add_state!(builder)
set_start!(builder, source)
set_final!(builder, target, CountWeight(3))
add_arc!(builder, source, UInt64(10), UInt64(20), target, CountWeight(2))
graph = build!(builder)

arc = only(arcs(graph, source))
@assert arc isa WfstArc{UInt64,CountWeight}
@assert state(graph, target).final_weight == CountWeight(3)
close(graph)
```

`TropicalWeight`, `LogWeight`, `ProbabilityWeight`, `ArcticWeight`,
`SignedTropicalWeight`, `CountWeight`, and `BooleanWeight` reject values
outside their ABI carriers at construction. Native composition requires equal
label and weight types and applies that weight domain's path multiplication.

For named byte or `UInt64` vocabularies, attach input/output `SymbolTable`s to
the builder. Tables assign dense labels and freeze with the graph. For lazy
custom automata, subtype `AbstractWfstProvider`, return matching
`ProviderState{Label,Weight}` values, then call
`provider(Label, Weight, implementation)`. The Rust engine captures and caches
those states without cloning the automaton algorithm into Julia.

Every returned `Wfst` owns one family-resource retain. Call `close`
deterministically; finalizers exist only as leak-safety fallbacks. `resource`
creates an independent retain, while `compose` captures one immutable snapshot
of each operand.

## Consume a host-defined lattice

LLattice.jl owns the provider implementation; `DynamicLatticeValue` is
lling-llang's checked consumer. Import takes an independent retain, and every
join, meet, or fold produces another independently owned value.

```julia
using LlingLlang
import LLattice

encode(value) = Vector{UInt8}(codeunits(string(value.value)))
hosts = [LLattice.provider(LLattice.MaxMin(value);
    domain_id="demo.maxmin.v1..", encode=encode) for value in (2, 7, 4)]
values = [dynamic_lattice_value(host.resource) for host in hosts]
close.(hosts)

maximum = lattice_join_many(values[1], values[2:3])
minimum = lattice_meet(values[1], values[2])
@assert String(lattice_stable_bytes(maximum)) == "7"
@assert String(lattice_stable_bytes(minimum)) == "2"
validate_lattice_laws(values)

close(maximum); close(minimum); close.(values)
```

The 16-byte domain identifier names both the encoding and the algebra. Values
from different domains are rejected before a foreign callback runs. Law
validation checks representative samples and can falsify, but not prove, the
universal lattice laws. Julia handles are same-thread consumers; the Rust
adapter uses fail-fast atomic admission and does not hold a mutex while host
code executes.

## Bounded accepting-path traversal

`paths(graph; limits=PathLimits(...))` creates a lazy Julia iterator backed by
one native cursor and one captured immutable WFST snapshot. It works for
providers whose state count is unknown and visits arcs in insertion order by
an iterative depth-first walk. `WfstPath` and `WfstPathStep` retain concrete
label and weight types; a yielded path remains valid after the cursor closes.

The limits bound distinct expanded states, aggregate arcs, depth, emitted
paths, total work, and work per call. Normal exhaustion is exact. Reaching the
depth or path-count bound raises `PathTruncatedError`; cancellation raises
`PathCancelledError`; other resource-budget exhaustion raises `NativeError`
with `STATUS_LIMIT_EXCEEDED`. In this path cursor, a computed finite weight
that overflows or a positive probability that underflows to zero also fails
explicitly; the existing scalar-composition and native semiring conventions
are unchanged. `poll_path!` advances one native work slice and returns
a path, `PathPending`, or `nothing` for exact exhaustion. Ordinary Julia
iteration repeats pending polls internally; one provider state expansion may
still read up to the cursor's remaining aggregate arc budget. Close a cursor
when stopping early, or use
`reduce_paths` for a fold that closes it on every exit path. The
[package guide](https://github.com/vinary-tree/lling-llang/tree/master/bindings/julia/LlingLlang#traverse-accepting-paths-with-explicit-bounds)
contains a runnable example and the ownership rules. This traversal is not
weight-ranked n-best search or random sampling.

## Bounded reachable-graph capture

`capture_graph` snapshots a scalar WFST once and incrementally discovers its
reachable graph without calling `num_states`. Each `poll_graph!` consumes at
most `GraphLimits.work_per_call` provider callbacks, with at most 256 arcs in
one page. `GraphPending` means more work remains; only a complete poll yields
an independently owned `GraphSnapshot`. Use `graph_info`, `graph_state`, and
`graph_arcs` to inspect its stable breadth-first local IDs and original
provider IDs. Cancellation and exhausted budgets fail explicitly without
returning a partial graph as exact. `complete_graph` is the convenience loop;
close its result when finished. See the [package guide](https://github.com/vinary-tree/lling-llang/tree/master/bindings/julia/LlingLlang#capture-a-complete-reachable-graph-under-explicit-limits)
for a runnable example and ownership details. A complete capture is a
foundation for global path analysis, not itself a shortest-path algorithm.

## Exact forward/backward distance analysis

`analyze_distances` retains a complete `GraphSnapshot` independently and
advances the native semiring-distance machine by at most
`DistanceLimits.work_per_call` vertex/edge transitions per `poll_distance!`.
Only a complete poll yields a `DistanceResult`; inspect its total with
`distance_info` and its state vectors in pages with `distance_page`. Acyclic
graphs support all seven scalar domains. Cyclic tropical, signed-tropical,
arctic, and Boolean distances are exact when they converge; improving cycles
report `STATUS_NON_CONVERGENT`. Cyclic probability/log/count sums report
`STATUS_UNSUPPORTED` until a proven convergent solver is available. Work,
numeric, cancellation, and unsupported failures never return a partial answer
as exact. Close the result when finished. See the [package guide](https://github.com/vinary-tree/lling-llang/tree/master/bindings/julia/LlingLlang#compute-exact-forward-and-backward-distances)
for a runnable example and explicit ownership sequence.
For probability/log/count graphs with nonzero accepting-path mass,
`posterior_arcs` and `posterior_final` expose paged arc and final-state
posterior probabilities from that same exact graph/result pair.

## Public API

```@autodocs
Modules = [LlingLlang]
Private = false
```
