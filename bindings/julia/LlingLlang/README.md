# LlingLlang.jl

Composable weighted finite-state transducers and host-defined automata for
Julia. A **weighted finite-state transducer** (WFST) is a directed graph whose
arcs consume an input label, produce an output label, and carry a weight.
LlingLlang builds eager scalar WFSTs over byte, Unicode-scalar, or `UInt64`
labels and every built-in family semiring. It imports compatible Vinary Tree
resources without losing domain metadata and composes immutable snapshots
lazily.

The package also lets Julia code implement a WFST by extending three methods.
The native engine captures that provider once, expands states on demand, and
caches each expanded state. This is the customer integration boundary used by
custom normalization, language-model, grammar, and correction automata.
Julia code may also implement the weight algebra itself. The native engine
consumes that algebra through a retained, capability-negotiated semiring
resource without requiring the weight type to be `isbits` or `Copy`.

The [published development guide and API reference](https://vinary-tree.github.io/lling-llang/dev/)
documents the current source; it is not a claim that RC.6 is registered.

## Install

The current release-candidate source layout uses local packages:

```julia
using Pkg
Pkg.develop(path="../vinary-tree-interop/bindings/julia/VinaryTreeInterop")
Pkg.develop(path="../llattice/bindings/julia/LLattice") # custom lattice values
Pkg.develop(path="bindings/julia/LlingLlang")
```

Build the native library and point the loader at it:

```sh
cargo build --release --no-default-features --features julia-bindings
export LLING_LLANG_LIBRARY="$PWD/target/release/liblling_llang.so"
```

On macOS use `liblling_llang.dylib`; on Windows use `lling_llang.dll`.

## Quickstart

```julia
using LlingLlang
import VinaryTreeInterop as VTI

builder = WfstBuilder(size_hint=2)
source = add_state!(builder)
target = add_state!(builder)
set_start!(builder, source)
set_final!(builder, target)
add_arc!(builder, source, 'a', 'b', target, 0.25)
graph = build!(builder)

@assert VTI.start(graph) == 0
@assert only(VTI.arcs(graph, 0)).output == UInt64('b')
close(graph)
```

## Bounded context-free parsing

`compile_cfg` accepts explicitly typed rules. An empty right-hand side is an
epsilon production. `parse_cfg` accepts terminal names or `UInt32` vocabulary
IDs and returns an owned `CfgAnalysis`, including for rejected input. The
analysis exposes acceptance, a deterministic Earley chart, complete parse
roots, and packed forest nodes and children. `close` the grammar and analysis
when finished; the analysis retains its own snapshot and survives closing the
grammar.

```julia
S = CfgNonterminal("S")
word = CfgTerminal("word")
grammar = compile_cfg(S, [CfgRule(S, [word])])
analysis = parse_cfg(grammar, ["word"];
    limits=CfgLimits(max_tokens=8, max_chart_items=128,
        max_forest_nodes=128, max_work=1024))
@assert cfg_info(analysis).accepted
@assert length(cfg_roots(analysis)) == 1
@assert length(collect(cfg_chart(analysis; batch_size=16))) ==
    cfg_info(analysis).chart_items
close(analysis)
close(grammar)
```

The same grammar can parse a scalar WFST lattice. Put grammar terminal IDs on
the selected tape; `cfg_terminal_id(grammar, word)` returns the ID assigned
when the grammar was compiled. `parse_cfg(grammar, graph; tape=:input,
limits=CfgWfstLimits(...))` captures the reachable graph once and parses all
reachable final states together. It accepts any built-in scalar label and
weight domain when the selected labels fit `UInt32`. `cfg_edge_labels` returns
the captured labels in forest edge-ID order. Input and output tapes can be
selected independently. The selected tape must have no epsilon arcs, and the
reachable graph must be acyclic; these cases raise `NativeError` with an
explicit status. All graph and parser limits are mandatory. The parser uses
arc labels for recognition and retains the arc weights only in its native
lattice; it does not currently rank parses by arc or production weight.

```julia
S = CfgNonterminal("S")
word = CfgTerminal("word")
grammar = compile_cfg(S, [CfgRule(S, [word])])
token_id = UInt64(cfg_terminal_id(grammar, word))
builder = WfstBuilder{UInt64,TropicalWeight}(size_hint=2)
source = add_state!(builder); target = add_state!(builder)
set_start!(builder, source); set_final!(builder, target, TropicalWeight(0))
add_arc!(builder, source, token_id, token_id, target, TropicalWeight(0))
graph = build!(builder)
analysis = parse_cfg(grammar, graph;
    limits=CfgWfstLimits(max_states=2, max_arcs=1, max_bytes=4096,
        max_import_work=16, max_chart_items=128,
        max_forest_nodes=128, max_parse_work=1024))
@assert cfg_info(analysis).accepted
close(analysis); close(graph); close(grammar)
```

### Choose label and weight domains

The default `WfstBuilder()` remains `WfstBuilder{Char,TropicalWeight}()`.
Select other domains through Julia type parameters; the resulting `Wfst`,
`WfstArc`, and `WfstState` retain those concrete types.

| Julia weight type | Accepted values | Path multiplication |
|---|---|---|
| `TropicalWeight` | finite or `Inf` | addition |
| `LogWeight` | finite or `Inf` | addition |
| `ProbabilityWeight` | finite and nonnegative | multiplication |
| `ArcticWeight` | finite or `-Inf` | addition |
| `SignedTropicalWeight` | finite or `Inf` | addition |
| `CountWeight` | exact integer from 0 through $`2^{53}`$ | multiplication |
| `BooleanWeight` | `false`/`true` | logical AND |

```julia
counts = WfstBuilder{UInt64,CountWeight}(size_hint=2)
source = add_state!(counts)
target = add_state!(counts)
set_start!(counts, source)
set_final!(counts, target, CountWeight(3))
add_arc!(counts, source, UInt64(10), UInt64(20), target, CountWeight(2))
graph = build!(counts)

arc = only(arcs(graph, source))
@assert arc isa WfstArc{UInt64,CountWeight}
@assert arc.weight == CountWeight(2)
close(graph)
```

Byte and `UInt64` graphs may attach separate input/output `SymbolTable`s so
applications can use vocabulary strings without putting strings on the ABI
wire. Tables assign dense zero-based labels, are copied into the builder, and
freeze when the immutable graph is built.

```julia
inputs = SymbolTable{UInt8}(["known"])
builder = WfstBuilder{UInt8,ProbabilityWeight}(
    input_symbols=inputs, output_symbols=SymbolTable{UInt8}())
source = add_state!(builder); target = add_state!(builder)
set_start!(builder, source); set_final!(builder, target)
add_arc!(builder, source, "cat", "feline", target, 0.75)
graph = build!(builder)
@assert label(input_symbols(graph), "cat") == UInt8(1)
@assert symbol(output_symbols(graph), UInt8(0)) == "feline"
close(graph)
```

Composition joins the output tape of the first graph to the input tape of the
second. If their matching arc weights are $`w_1`$ and $`w_2`$, tropical
multiplication produces the composed weight $`w_1 \otimes w_2 = w_1 + w_2`$.

```julia
product = compose(first, second)
try
    outgoing = VTI.arcs(product, VTI.start(product))
finally
close(product)
end
```

`compose` also accepts a raw `VinaryTreeInterop.Wfst` on either side of a
typed `LlingLlang.Wfst`. The raw operand is checked against the typed label
and weight domains; no ownership transfer or test-only conversion is needed.

Input and output projection copy the chosen tape's label to both tapes,
producing an acceptor without eagerly expanding its output states. `reverse`
is Julia's ordinary `Base.reverse` method specialized for `Wfst`; it reverses
the graph constructively. All three methods preserve the concrete label and
weight types. An optional `BudgetV2` bounds the imported input plus the
complete potential output. For a two-state, one-arc input with one final
state, projection needs four state units, two arc units, and six work units;
reversal needs five, three, and eight respectively:

```julia
using LlingLlang

builder = WfstBuilder(size_hint=2)
start = add_state!(builder)
final = add_state!(builder)
set_start!(builder, start)
set_final!(builder, final, 0.0)
add_arc!(builder, start, 'a', 'b', final, 0.5)
graph = build!(builder)

projected = project_input(graph;
    budget=BudgetV2(max_states=4, max_arcs=2, max_work=6))
try
    arc = only(arcs(projected, 0))
    @assert arc.input == arc.output
finally
    close(projected)
end

reversed = reverse(graph;
    budget=BudgetV2(max_states=5, max_arcs=3, max_work=8))
close(reversed)

either = union(graph, graph;
    budget=BudgetV2(max_states=9, max_arcs=6, max_work=15))
@assert length(arcs(either, 0)) == 2
close(either)

twice = concat(graph, graph)
close(twice)
zero_or_more = closure(graph)
@assert VTI.state_info(zero_or_more, VTI.start(zero_or_more)).final
close(zero_or_more)
one_or_more = closure_plus(graph)
@assert !VTI.state_info(one_or_more, VTI.start(one_or_more)).final
close(one_or_more)
close(graph)
```

`max_bytes` accounts native and scalar graph payload, not process RSS or
allocations made by a custom provider. A rejected budget publishes no result
and throws `NativeError` with `STATUS_LIMIT_EXCEEDED`. A default `BudgetV2()`
has no active limits.

The revision-10 `determinize`, `minimize`, `remove_epsilon`, and `connect`
methods eagerly materialize the corresponding native transforms. They require
an explicit `budget` with nonzero state, arc, byte, and work limits; unlike the
older lazy unary methods, `BudgetV2()` is rejected. Work is a conservative
upper-bound reservation over native graph visits, not elapsed time, and bytes
count retained graph payload rather than peak memory. A finite limit may
therefore reject a graph that could be produced within that limit. For example:

```julia
limits = BudgetV2(max_states=16, max_arcs=256,
    max_bytes=100_000, max_work=100_000)
trimmed = connect(open_graph; budget=limits) # an open Wfst
close(trimmed)
```

`union` chooses either operand's paths; `concat` sequences them; `closure`
and `closure_plus` repeat paths zero-or-more and one-or-more times. They
import checked input snapshots before returning independently owned, lazy
results. Binary operations require matching label/weight domains and matching
Julia symbol tables on both tapes. Kleene-plus accepts an empty path if its
input already does, because a required repetition may itself be empty.

### Traverse accepting paths with explicit bounds

`paths(graph)` returns a Julia iterator backed by a native cursor over one
captured immutable snapshot. The cursor expands only states reached by its
iterative depth-first walk; it does not require a provider to know its state
count. A final state is emitted before its outgoing arcs, and arcs are visited
in their insertion order. This is **traversal order**, not shortest-path or
score order. Each `WfstPath` owns its copied `WfstPathStep`s and includes the
terminal state's final weight in `weight`. An empty accepting path has no
steps. Epsilon is represented by `nothing` on the corresponding tape.

```julia
builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=2)
source = add_state!(builder)
target = add_state!(builder)
set_start!(builder, source)
set_final!(builder, target, TropicalWeight(3))
add_arc!(builder, source, UInt8('a'), UInt8('b'), target,
    TropicalWeight(2))
graph = build!(builder)

limits = PathLimits(max_states=2, max_arcs=1, max_depth=1,
    max_paths=2, max_work=100, work_per_call=4)
cursor = paths(graph; limits)
close(graph) # cursor independently owns the captured snapshot
try
    for path in cursor
        @assert path.weight == TropicalWeight(5)
        @assert only(path.steps).output == UInt8('b')
    end
finally
    close(cursor) # also required when stopping iteration early
end
```

Every bound is finite. `max_states` counts distinct expanded states;
`max_arcs` counts the arcs cached from them; `max_depth` bounds the number of
steps in one path; `max_paths` bounds emitted paths; `max_work` bounds traversal
decisions over the cursor lifetime; and `work_per_call` divides traversal into
pollable slices. Call `poll_path!(cursor)` to consume exactly one native slice:
it returns a `WfstPath`, a `PathPending` marker, or `nothing` on exact
exhaustion. Ordinary `for` iteration repeats pending polls internally until
it has a path or terminal result. A state expansion itself is bounded by the
remaining arc budget. Hitting the depth or path-count bound raises `PathTruncatedError`
instead of making an incomplete result look exhaustive. Exhausting a state,
arc, or work budget raises `NativeError` with `STATUS_LIMIT_EXCEEDED`.
For this path cursor, the same status reports a computed weight outside its
scalar carrier (for example, finite tropical path costs whose sum overflows
`Float64`); this is never interpreted as a valid infinity-weight path. A
positive probability product that underflows to zero is likewise reported as
a numeric limit. This strict path-result rule does not change the existing
scalar-composition or native semiring arithmetic conventions.
`CancellationV2` can stop traversal cooperatively; a cancelled iteration
raises `PathCancelledError`. The cursor owns its snapshot until `close` or
normal exhaustion; yielded paths are independent owned Julia values.

`reduce_paths(operation, initial, live_graph; limits, cancellation)` folds this
iterator and closes its native cursor even if the reducer throws. For example,
`reduce_paths((count, _) -> count + 1, 0, live_graph; limits)` counts accepting
paths up to the declared bounds. The `live_graph` argument must be open when
the reducer is constructed; a count is exact only when iteration ends
normally. The algorithmic distinction between path enumeration, shortest
distance, and randomized sampling is detailed in the
[path-extraction](../../../docs/algorithms/path-extraction.md) and
[path-sampling](../../../docs/algorithms/path-sampling.md) guides.

### Capture a complete reachable graph under explicit limits

`capture_graph(graph)` starts a second, breadth-first native cursor over one
immutable snapshot. Unlike accepting-path traversal, it visits every reachable
state once and pages each state's arcs in batches of at most 256. It does not
ask a lazy provider for a known state count. A complete graph is suitable for
global analyses; an interrupted or budget-limited capture is never presented
as exact.

```julia
cursor = capture_graph(graph; limits=GraphLimits(
    max_states=10_000, max_arcs=100_000,
    max_work=100_000, work_per_call=16))
try
    while true
        result = poll_graph!(cursor) # one bounded provider-work slice
        result isa GraphPending && continue
        snapshot = result::GraphSnapshot
        try
            info = graph_info(snapshot)
            first = graph_state(snapshot, 0) # local IDs start at zero
            @assert first.raw_id == info.start_raw
        finally
            close(snapshot)
        end
        break
    end
finally
    close(cursor)
end
```

`complete_graph(graph; limits=...)` performs the same polling loop and returns
the complete `GraphSnapshot`. The caller must close it. Its local state IDs are
assigned in breadth-first discovery order, with arcs in provider order, and
`GraphArc` retains both the compact target ID and original provider target ID.
`GraphLimits.max_work` and `work_per_call` count provider callbacks, not
traversal decisions; each callback can have provider-defined latency.
`GraphCancelledError` reports cancellation, while `NativeError` with
`STATUS_LIMIT_EXCEEDED` reports a finite resource bound. Neither outcome
returns a partial graph as a complete result. The source may close after
`capture_graph` opens because the cursor owns its snapshot; a complete graph
is independent of that snapshot and remains live until closed.

### Compute exact forward and backward distances

Once a graph is completely captured, `analyze_distances` runs native semiring
analysis in resumable slices. Forward distance at local state $`q`$ combines
weights of all start-to-$`q`$ paths with semiring addition; backward distance
combines all $`q`$-to-final paths, including the state's final weight. The
reported total is the backward distance at the start. For an acyclic graph,
the native kernel visits states in a deterministic topological order and
supports all seven built-in scalar weight domains.

```julia
builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=2)
start = add_state!(builder)
finish = add_state!(builder)
set_start!(builder, start)
set_final!(builder, finish, TropicalWeight(3))
add_arc!(builder, start, UInt8('a'), UInt8('a'), finish, TropicalWeight(2))
source = build!(builder)
graph = complete_graph(source)
close(source) # the complete graph no longer needs the provider snapshot

cursor = analyze_distances(graph; limits=DistanceLimits(
    max_work=100_000, work_per_call=64))
close(graph) # the analysis cursor retains its own graph lease
try
    while true
        result = poll_distance!(cursor)
        result isa DistancePending && continue
        distances = result::DistanceResult
        try
            @assert distance_info(distances).total == TropicalWeight(5)
            page = distance_page(distances, 0; capacity=2)
            @assert page.forward == [TropicalWeight(0), TropicalWeight(2)]
            @assert page.backward == [TropicalWeight(5), TropicalWeight(3)]
        finally
            close(distances)
        end
        break
    end
finally
    close(cursor)
end
```

For probability, log, or count weights with nonzero accepting-path mass,
`posterior_arcs(distances, local_id; offset=0, capacity=256)` returns a page
of arc-use probabilities in provider order, and
`posterior_final(distances, local_id)` returns the probability of terminating
at that state. Parallel arcs remain distinct. Posterior probabilities use the
exact forward/backward result bound to the same native graph; no caller-supplied
graph can accidentally be mixed with another result. Other weight domains and
zero accepting-path mass fail explicitly. A probability too small to represent
as a nonzero `Float64` also fails explicitly rather than silently becoming
zero.

`complete_distances(graph; limits=...)` runs the same bounded polling loop and
returns an owned result. One `poll_distance!` performs no more than
`work_per_call` graph-vertex or graph-edge transitions; `max_work` bounds the
whole analysis. A pending poll is never an exact answer. The caller can
cancel between transitions. Results remain valid after both the cursor and
graph close, until the result itself is closed.

For cyclic graphs, the exact native solver handles the idempotent tropical,
signed-tropical, arctic, and Boolean semirings. A strictly improving cycle
raises `NativeError` with `STATUS_NON_CONVERGENT`; cyclic probability, log,
and count sums currently raise `STATUS_UNSUPPORTED` because summing all walks
requires a separate convergence proof. Count overflow and other scalar
representation failures raise `STATUS_LIMIT_EXCEEDED`. None of these outcomes
is silently replaced by a depth-truncated approximation. This native
analysis does not change the library's existing scalar-composition arithmetic.

### Enumerate best paths without eager path materialization

`ranked_paths` enumerates accepting paths from a complete `GraphSnapshot` in
best-completion order. It uses resumable native Viterbi suffix analysis and
then a bounded best-first frontier; neither phase builds an eager path list.
Equal costs are resolved by shorter path length and captured provider arc
order. The result is a Julia iterator of owned `WfstPath` values. Close it
when stopping early; `reduce_ranked_paths` closes it automatically.
`best_path(graph)` returns one path or `nothing`; `k_best_paths(graph, k)` and
its `n_best_paths` alias collect only the requested finite prefix and close
their native cursors. Their `limits.max_paths` must be at least `k`.

```julia
builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=2)
start = add_state!(builder)
finish = add_state!(builder)
set_start!(builder, start)
set_final!(builder, finish, TropicalWeight(3))
add_arc!(builder, start, UInt8('a'), UInt8('a'), finish, TropicalWeight(2))
source = build!(builder)
graph = complete_graph(source)
close(source)
cursor = ranked_paths(graph; limits=RankedPathLimits(
    max_work=100_000, work_per_call=32, max_depth=64,
    max_paths=100, max_frontier=10_000))
close(graph) # cursor retains its own immutable graph lease
try
    best = first(cursor)
    @assert best.weight == TropicalWeight(5)
finally
    close(cursor)
end
```

For probability weights, the rank cost is $`-\log p`$, so the most probable
path is first. Tropical, signed-tropical, and log weights rank by additive
cost; arctic weights rank by negated score. Count weights rank by increasing
multiplicity and Boolean weights use unit cost for accepting paths. These
projections rank individual paths, while `analyze_distances` computes semiring
aggregates over *all* paths; they differ on non-idempotent domains.

`poll_ranked_path!` performs at most `work_per_call` native work transitions
and returns a path, `RankedPathPending`, or `nothing` on exact exhaustion.
Ordinary iteration repeats pending polls. Reaching `max_depth` or `max_paths`
throws `PathTruncatedError`; exhausting `max_work` or `max_frontier` throws
`NativeError` with `STATUS_LIMIT_EXCEEDED`. Cancellation throws
`PathCancelledError`. A reachable improving cycle returns
`STATUS_NON_CONVERGENT` before any ranked path is emitted. The cursor never
reports a truncated result as exhaustive. Numeric overflow/underflow is
rejected in this new path analysis only; existing scalar composition semantics
remain unchanged.

### Prune by an exact complete-path cost window

`cost_pruned_paths(graph; beam=1.0)` lazily keeps every accepting path whose
native Viterbi cost is at most one cost unit worse than the best path. The
underlying best-first ordering makes the first out-of-window path a sound
stopping point. This is exact *complete-path* cost-window pruning, not the
approximate partial-hypothesis beam search used by the native lattice API.
Equal-cost ties remain in native path order. The beam must be finite and
nonnegative; for probability weights it is measured in negative-log units.

```julia
# Use a live complete graph; close the iterator when stopping early.
cursor = cost_pruned_paths(graph; beam=1.0,
    limits=RankedPathLimits(max_work=100_000, work_per_call=32,
        max_depth=64, max_paths=100, max_frontier=10_000))
try
    for path in cursor
        println(path.weight)
    end
finally
    close(cursor)
end
```

`poll_cost_pruned_path!` returns a path, `RankedPathPending`, or `nothing`;
one call performs at most one native ranked-path poll. Use
`reduce_cost_pruned_paths(operation, initial, graph; beam=...)` for a fold
that always closes the cursor. Native work, frontier, count, depth,
cancellation, and numeric failures propagate unchanged; an explicit limit
is never mistaken for exact exhaustion.

### Draw seeded accepting paths

`sample_paths(graph; limits)` is a bounded lazy iterator of owned paths. Its
native sampler first computes exact backward masses for the complete graph,
then chooses between stopping and each outgoing arc according to conditional
accepting-path mass. Unlike a local random walk, it never chooses a branch
that cannot reach a final state. `:uniform` gives each finite accepting path
equal probability and ignores scalar weights. `:proportional` weights paths by
their probability, log-probability, or count-semiring mass; other weight domains
fail with `STATUS_UNSUPPORTED`. Uniform and proportional draws on cyclic
graphs currently fail explicitly, because this solver's exact path-count or
mass analysis requires an acyclic graph. An unsupported graph is never
silently approximated by a depth cutoff.

```julia
builder = WfstBuilder{UInt8,ProbabilityWeight}(size_hint=3)
start = add_state!(builder)
left = add_state!(builder)
right = add_state!(builder)
set_start!(builder, start)
set_final!(builder, left, ProbabilityWeight(1))
set_final!(builder, right, ProbabilityWeight(1))
add_arc!(builder, start, UInt8('a'), UInt8('a'), left,
    ProbabilityWeight(0.25))
add_arc!(builder, start, UInt8('b'), UInt8('b'), right,
    ProbabilityWeight(0.75))
source = build!(builder)
graph = complete_graph(source)
close(source)
limits = SamplePathLimits(strategy=:proportional, seed=42,
    max_samples=100, max_depth=64, max_work=100_000, work_per_call=32)
try
    draws = sample_n_paths(graph, 10; limits)
    @assert length(draws) == 10
finally
    close(graph)
end
```

For a state $`q`$ with exact backward mass $`B(q)`$, an outgoing arc
$`q \xrightarrow{w} r`$ is selected with conditional probability
$`w B(r) / B(q)`$ in the probability/count domains; a final choice is
$`\rho(q) / B(q)`$. The log domain computes the equivalent values in log
space. `:uniform` substitutes path counts for weights. Each option is
examined in provider order, one per bounded native work step. The public
seed-to-draw mapping uses SplitMix64 and is independent of `work_per_call`.

`poll_sample_path!` returns a path, `SamplePathPending`, or `nothing` when
there is provably no accepting path. `sample_path` returns one draw;
`sample_n_paths` and `reduce_sampled_paths` consume only the requested finite
prefix and close the cursor. Directly collecting the iterator through its
`max_samples` bound raises `PathTruncatedError` because the distribution
continues to admit paths. `max_depth` truncation, work exhaustion, numeric
failure, and cancellation are likewise explicit. The sampler retains its
complete graph lease, so the source graph may close after construction.
The [finite Julia path cursor contract](../../../docs/algorithms/julia-path-formal-contract.md)
maps resource, order, limit, seed, and terminal laws to executable model and
facade properties.

### Implement a lazy Julia provider

```julia
struct RewriteAB <: AbstractWfstProvider end
LlingLlang.wfst_start(::RewriteAB) = 0
LlingLlang.wfst_state_count(::RewriteAB) = 2
function LlingLlang.wfst_state(::RewriteAB, state::UInt64)
    state == 0 && return ProviderState(
        arcs=[ProviderArc('a', 'b', 1, 0.0)])
    state == 1 && return ProviderState(final=true, final_weight=0.0)
    ProviderState(valid=false)
end

graph = provider(RewriteAB(); acyclic=true)
close(graph)
```

`wfst_state` must return a complete immutable `ProviderState`. State IDs are
`UInt64`; `nothing` on an arc tape means epsilon. `wfst_state_count` may return
`nothing` when a lazy graph does not know its final size.

For a nondefault domain, return matching concrete provider values and publish
the provider with its types:

```julia
struct Reachability <: AbstractWfstProvider end
LlingLlang.wfst_start(::Reachability) = 0
LlingLlang.wfst_state_count(::Reachability) = 1
LlingLlang.wfst_state(::Reachability, state::UInt64) = state == 0 ?
    ProviderState{UInt8,BooleanWeight}(final=true) :
    ProviderState{UInt8,BooleanWeight}(valid=false)

graph = provider(UInt8, BooleanWeight, Reachability(); acyclic=true)
close(graph)
```

### Implement a Julia semiring

Subtype `AbstractSemiringProvider` and implement the two identities, `plus`,
`times`, and natural order. Equality defaults to Julia's `==`; stable bytes
are required only when callers use them. Optional division, Kleene star,
numeric projections, declared laws, and a closure bound are ordinary method
overloads enabled explicitly at publication.

```julia
struct Tropical <: AbstractSemiringProvider end
LlingLlang.semiring_zero(::Tropical) = Inf
LlingLlang.semiring_one(::Tropical) = 0.0
LlingLlang.semiring_plus(::Tropical, a, b) = min(a, b)
LlingLlang.semiring_times(::Tropical, a, b) = a + b
LlingLlang.semiring_natural_order(::Tropical, a, b) =
    a < b ? VTI.SEMIRING_ORDER_BETTER :
    a > b ? VTI.SEMIRING_ORDER_WORSE : VTI.SEMIRING_ORDER_EQUAL
LlingLlang.semiring_stable_bytes(::Tropical, value) =
    Vector{UInt8}(codeunits(repr(Float64(value))))

host = semiring_provider(Tropical();
    domain_id=VTI.interface_id("demo.tropical.v1"), stable_bytes=true)
algebra = semiring_context(host)
close(host) # `algebra` owns an independent retain

zero = semiring_zero(algebra)
one = semiring_one(algebra)
best = one + zero
@assert semiring_equal(algebra, best, one)
close(zero); close(one); close(best); close(algebra)
```

`domain_id` is exactly 16 bytes and identifies compatible carrier semantics;
it does not make values from two provider instances interchangeable. Host
values live in a recycling generation-checked arena. Every `SemiringWeight`
owns one token reference, `copy` invokes the provider's clone operation, and
`close` releases exactly once. A stale or cross-context token is rejected.
Batch release validates every token and its multiplicity before consuming any
reference, so a failed batch is safe to retry. A slot whose generation is
exhausted is retired rather than reused. Resource context words are
never-reused opaque cookies, not Julia object addresses; stale callback
contexts return `STATUS_CLOSED` instead of aliasing a later provider.
Use `validate_semiring_laws` with representative identities, boundaries, and
workload values before enabling algorithms that trust declared properties.
`semiring_plus_many` and `semiring_times_many` preserve left-fold order while
using bounded provider batches when available. `semiring_diagnostic(algebra)`
describes the domain, while `semiring_diagnostic(algebra, weight)` describes an
owned weight without exposing its provider token.

These are the currently versioned customer-provider seams in this package:
the dynamic semiring capability and immutable scalar WFST capability. Julia
can also create lattice providers through LLattice.jl and consume them here.
CFG/grammar, symbolic constraint, decoder, and pipeline extension traits do
not yet have corresponding negotiated native provider interfaces; a Julia
subtype or wrapper around a scalar WFST would not implement those distinct
contracts. They remain open binding/ABI work, not architectural exclusions.

### Send an LLattice value through lling-llang

LLattice.jl owns the provider implementation; `DynamicLatticeValue` is the
checked lling-llang consumer. Import retains independently, so the original
LLattice handle may close immediately:

```julia
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

The domain identifier names both the encoding and the lattice laws and must
contain exactly 16 bytes. Join, meet, and batched folds return new owned
handles. Law validation accepts at most sixteen representative values and can
falsify—not prove—the universal lattice axioms.

## Ownership & memory model

`WfstBuilder` owns one native builder and `build!` consumes it on success.
Returned `LlingLlang.Wfst` values wrap a `VinaryTreeInterop.Wfst`, preserve
their concrete label/weight types and optional symbol tables, and own one
retained immutable resource.
`compose` captures independent snapshots of both inputs, so callers may close
either input immediately after construction without invalidating the product.
Unary transforms borrow the input for the call and return independently owned
results.
`paths` captures another independent snapshot and releases it when its cursor
closes. Each yielded `WfstPath` copies only its own bounded sequence of steps
and remains valid after cursor closure.
Use `close` deterministically; finalizers are leak-safety fallbacks.

Provider objects are rooted while any native retain exists. A provider
snapshot is identity-with-retain because the facade advertises immutable
resources. Mutating provider-visible state after `provider` therefore violates
the snapshot contract. Snapshot retain and final release share one registry
lock, preventing publication of a provider whose final owner just closed.

A `SemiringContext` independently retains its provider resource, so the
original resource may close immediately after import. Weights keep their exact
context rooted. Close weights before their context for deterministic error
reporting; finalizers remain leak-safety fallbacks only.

Each `DynamicLatticeValue` similarly owns one retained immutable resource.
Every join, meet, or fold result owns another retain. `close` releases exactly
that owner; copying the native pointer would not create another owner.

## Errors

Native operations throw `NativeError`, which contains the stable `Status`, the
operation, and a copied thread-local diagnostic. Provider exceptions never
unwind through C: callbacks convert them to `STATUS_PROVIDER_ERROR`, including
when a custom exception's `showerror` method itself fails. A rejected callback
does not publish a successful result; callers must ignore all output bytes on
non-OK status because a paged arc callback can have written a partial page.
Invalid labels, negative sizes, unknown domains, and weights outside their
selected carrier are rejected before graph mutation.

## Concurrency

The default `parallel=false` is a nonblocking, fail-fast serial contract:
overlapping or recursive callbacks return `STATUS_PROVIDER_ERROR`; they do not
queue behind a lock held across customer code. Set `parallel=true` only when
every provider method and its stored state are safe for concurrent and
reentrant calls. The facade never invokes customer code while holding its
cache or semiring-value-arena lock. Under `parallel=true`, two racing first
WFST expansions may compute the same state; one complete immutable copy wins
the cache insertion. Under `parallel=false`, the second callback is rejected
instead. Providers should not reenter their own native handle from a callback
unless they declared and implemented true reentrancy.

Semiring providers default to `thread_bound=true`: this is an explicit
creator-Julia-thread affinity policy enforced at callback entry, not a
requirement to attach foreign threads to Julia. `parallel=true` requires
`thread_bound=false`; declare it only when every method and stored value is
concurrently callable and reentrant. For Julia 1.10 and later, the supported
runtime automatically adopts foreign threads entering through `@cfunction`
(introduced in [Julia 1.9](https://docs.julialang.org/en/v1.9/NEWS/)); the
[general FFI manual](https://docs.julialang.org/en/v1/manual/calling-c-and-fortran-code/)
also requires callbacks not to throw across the C boundary. Automatic adoption
does not make customer objects race-free, waive the creator-thread policy, or
turn a callback blocked on its own provider into a safe reentrant call.

Dynamic lattice handles in Julia are same-thread consumers. The Rust adapter
uses a nonblocking atomic admission gate for serial providers and holds no
consumer lock while Julia join or meet code executes. The C/Julia facade does
not expose Rust's explicit parallel-wrapper promotion.

## Zero-copy paths

`resource(graph)` returns an independent two-word `VtResource` retain without
materializing the graph. `compose` hands those resource words to Rust in
constant time and expands only reachable product states. Arc pages are written
into caller-owned contiguous buffers by the provider ABI. The Julia facade
copies each provider state's arc vector once into its immutable cache.
Projection first imports one checked input snapshot, then defers output-state
expansion; reversal materializes the output before returning.

## Security and provider trust

Foreign vtables are capability-negotiated by interface ID and minimum version.
The native consumer validates status codes, booleans, labels and weights
against their advertised domains, page counts, reserved bytes, and resource
ownership. A provider must
still obey its declared immutability, domain, threading, and state-stability
contracts. Treat untrusted providers like synchronous plugin code: constrain
their work and do not expose secrets through callbacks.

## Troubleshooting

- A loader error means `LLING_LLANG_LIBRARY` does not name the matching native
  library or the platform loader cannot find one of its dependencies.
- `STATUS_INCOMPATIBLE_RESOURCE` means the input lacks a supported
  `vt.scalar-wfst.1`, has a malformed interface, or a composition peer uses
  different label or weight domains.
- `STATUS_PROVIDER_ERROR` means a provider threw or returned malformed state
  data. Reproduce the state callback directly to obtain the Julia exception.
- `STATUS_PROVIDER_ERROR` with a concurrent/recursive diagnostic means a
  nonparallel provider was entered while another callback was active. Make
  callers serial or implement a genuinely reentrant provider before opting in
  to `parallel=true`; setting the flag alone does not make blocking recursion
  safe.

## Version compatibility

| Component | Required value |
|---|---:|
| LlingLlang.jl | `4.0.0-rc.6` |
| lling-llang C ABI | `1` |
| lling-llang API revision | at least `7` |
| VinaryTreeInterop.jl | major version `4` |
| Julia | `1.10` or newer |

The module validates ABI and API compatibility during initialization.

## Executable conformance evidence

[`test/runtests.jl`](test/runtests.jl) exercises ABI negotiation and all 21
label/weight combinations across eager build, import, and Julia-defined lazy
providers. It checks all seven native composition multiplications, typed
states/arcs, dense frozen symbol tables, snapshot lifetime, arc tapes, and
final weights against the real native library. It
also publishes a Julia-defined semiring and exercises base algebra, optional
capabilities, law validation, stable bytes, cloning, and deterministic release
through Rust. The LLattice integration adds eight assertions over Julia-hosted
join, meet, bounded folds, equality, domain negotiation, capability flags,
law validation, and deterministic close.

```sh
TMPDIR="$PWD/target/julia-tmp" \
LLING_LLANG_LIBRARY="$PWD/target/debug/liblling_llang.so" \
julia --project=bindings/julia/LlingLlang -e \
  'using Pkg; Pkg.test()'
```

## Maintainer workflow

1. Change the project-owned C ABI and `bindings/api.json` together.
2. Regenerate `GeneratedAbi.jl` from the authoritative model.
3. Run the Rust FFI suite, Julia tests, documentation build, binding drift
   gate, and mandatory pgmcp bug gate.
4. Commit generated and handwritten changes together with verification counts.
5. Push only the approved feature branch; this campaign does not tag or publish.
