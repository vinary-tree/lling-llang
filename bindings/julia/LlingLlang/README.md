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
Use `validate_semiring_laws` with representative identities, boundaries, and
workload values before enabling algorithms that trust declared properties.
`semiring_plus_many` and `semiring_times_many` preserve left-fold order while
using bounded provider batches when available. `semiring_diagnostic(algebra)`
describes the domain, while `semiring_diagnostic(algebra, weight)` describes an
owned weight without exposing its provider token.

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
Use `close` deterministically; finalizers are leak-safety fallbacks.

Provider objects are rooted while any native retain exists. A provider
snapshot is identity-with-retain because the facade advertises immutable
resources. Mutating provider-visible state after `provider` therefore violates
the snapshot contract.

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
unwind through C: callbacks convert them to `STATUS_PROVIDER_ERROR`. Invalid
labels, negative sizes, unknown domains, and weights outside their selected
carrier are rejected before graph mutation.

## Concurrency

Providers are serialized by consumers unless `parallel=true` is explicitly
declared. Set it only when `wfst_start`, `wfst_state_count`, and `wfst_state`
are safe for concurrent and reentrant calls. The facade never invokes customer
code while holding its cache lock; two racing first expansions may compute the
same state, after which one immutable value is retained.

Semiring providers default to `thread_bound=true`, matching Julia's runtime
attachment requirements. Their arena lock covers only token lookup,
allocation, and publication; Julia algebra methods run outside it. Set
`parallel=true, thread_bound=false` only when every method and stored value is
concurrently callable and reentrant.

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
- A stalled composition commonly indicates that a provider declared parallel
  reentrancy but blocks recursively; remove `parallel=true` until corrected.

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
