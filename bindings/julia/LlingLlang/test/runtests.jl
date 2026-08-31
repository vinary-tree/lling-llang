using Test
using LlingLlang
import VinaryTreeInterop
import LLattice

const VTI = VinaryTreeInterop

@testset "ABI and eager builder" begin
    @test abi_version() == ABI_VERSION
    @test api_revision() >= API_REVISION

    builder = WfstBuilder(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    @test (first, second) == (0, 1)
    set_start!(builder, first)
    set_final!(builder, second, 0.25)
    add_arc!(builder, first, 'a', 'b', second, 0.5)
    graph = build!(builder)
    @test !isopen(builder)
    @test VTI.start(graph) == 0
    @test VTI.state_count(graph) == 2
    @test VTI.state_info(graph, 1).final_weight == 0.25
    @test only(VTI.arcs(graph, 0)).output == UInt64('b')

    imported = import_wfst(graph)
    @test VTI.start(imported) == 0
    close(imported)
    close(graph)
end

@testset "host-defined lattice consumed through lling-llang" begin
    encode(value) = Vector{UInt8}(codeunits(string(value.value)))
    providers = [
        LLattice.provider(LLattice.MaxMin(value);
            domain_id="test.maxmin.v1..", encode=encode)
        for value in (2, 7, 4)
    ]
    values = [dynamic_lattice_value(provider.resource) for provider in providers]
    close.(providers)

    joined = lattice_join(values[1], values[2])
    met = lattice_meet(values[1], values[2])
    joined_many = lattice_join_many(values[1], values[2:3])
    met_many = lattice_meet_many(values[2], values[[1, 3]])
    @test String(lattice_stable_bytes(joined)) == "7"
    @test String(lattice_stable_bytes(met)) == "2"
    @test String(lattice_stable_bytes(joined_many)) == "7"
    @test String(lattice_stable_bytes(met_many)) == "2"
    @test lattice_equal(joined, joined_many)
    @test lattice_domain_id(joined) == VTI.interface_id("test.maxmin.v1..")
    @test lattice_flags(joined) & VTI.LATTICE_FLAG_BATCH != 0
    validate_lattice_laws(values)

    close.([joined, met, joined_many, met_many])
    close.(values)
    @test all(!isopen(value) for value in values)
end

struct TropicalProvider <: AbstractSemiringProvider end
LlingLlang.semiring_zero(::TropicalProvider) = Inf
LlingLlang.semiring_one(::TropicalProvider) = 0.0
LlingLlang.semiring_plus(::TropicalProvider, left, right) = min(left, right)
LlingLlang.semiring_times(::TropicalProvider, left, right) = left + right
LlingLlang.semiring_approx_equal(::TropicalProvider, left, right, epsilon) =
    isapprox(left, right; atol=epsilon, rtol=0)
LlingLlang.semiring_natural_order(::TropicalProvider, left, right) =
    left < right ? VTI.SEMIRING_ORDER_BETTER :
    left > right ? VTI.SEMIRING_ORDER_WORSE : VTI.SEMIRING_ORDER_EQUAL
LlingLlang.semiring_stable_bytes(::TropicalProvider, value) =
    Vector{UInt8}(codeunits(repr(Float64(value))))
LlingLlang.semiring_divide(::TropicalProvider, dividend, divisor) =
    isfinite(divisor) ? dividend - divisor : nothing
LlingLlang.semiring_left_divide(provider::TropicalProvider, value, divisor) =
    LlingLlang.semiring_divide(provider, value, divisor)
LlingLlang.semiring_star(::TropicalProvider, value) = value >= 0 ? 0.0 : nothing
LlingLlang.semiring_numerical_value(::TropicalProvider, value) = value
LlingLlang.semiring_quantize(::TropicalProvider, value, epsilon) =
    round(Int64, value / epsilon)
LlingLlang.semiring_probability(::TropicalProvider, value) = exp(-value)
LlingLlang.semiring_properties(::TropicalProvider) =
    VTI.SEMIRING_PROPERTY_HASHABLE |
    VTI.SEMIRING_PROPERTY_IDEMPOTENT_PLUS |
    VTI.SEMIRING_PROPERTY_K_CLOSED |
    VTI.SEMIRING_PROPERTY_ZERO_SUM_FREE |
    VTI.SEMIRING_PROPERTY_COMMUTATIVE_TIMES |
    VTI.SEMIRING_PROPERTY_TOTALLY_ORDERED
LlingLlang.semiring_closure_bound(::TropicalProvider) = 1

@testset "host-defined dynamic semiring" begin
    resource = semiring_provider(TropicalProvider();
        domain_id=VTI.interface_id("test.tropical.v1"), division=true, star=true,
        numeric=true, stable_bytes=true)
    context = semiring_context(resource)
    close(resource)

    zero = semiring_zero(context)
    one = semiring_one(context)
    sum = one + zero
    product = one * one
    quotient = semiring_divide(context, product, one)
    closure = semiring_star(context, one)

    @test semiring_equal(context, sum, one)
    @test semiring_approx_equal(context, product, one, 1e-12)
    @test semiring_natural_order(context, one, zero) == VTI.SEMIRING_ORDER_BETTER
    @test semiring_numerical_value(context, product) == 0.0
    @test semiring_quantize(context, product, 0.25) == 0
    @test semiring_probability(context, product) == 1.0
    @test semiring_closure_bound(context) == 1
    @test String(semiring_stable_bytes(context, one)) == "0.0"
    @test semiring_properties(context) == LlingLlang.semiring_properties(TropicalProvider())
    @test !isnothing(quotient)
    @test !isnothing(closure)
    @test isnothing(semiring_divide(context, one, zero))
    validate_semiring_laws(context, [zero, one, sum, product]; epsilon=1e-12)

    copied = copy(one)
    close(one)
    @test semiring_equal(context, copied, product)
    for weight in (zero, sum, product, quotient, closure, copied)
        close(weight)
    end
    close(context)
    @test !isopen(context)
end

struct ExampleProvider <: AbstractWfstProvider end
LlingLlang.wfst_start(::ExampleProvider) = 0
LlingLlang.wfst_state_count(::ExampleProvider) = 2
function LlingLlang.wfst_state(::ExampleProvider, state::UInt64)
    state == 0 && return ProviderState(arcs=[ProviderArc('b', 'c', 1, 0.75)])
    state == 1 && return ProviderState(final=true, final_weight=0.125)
    ProviderState(valid=false)
end

@testset "host provider and lazy composition" begin
    host = provider(ExampleProvider(); acyclic=true)
    @test VTI.start(host) == 0
    @test VTI.state_count(host) == 2
    @test only(VTI.arcs(host, 0)).output == UInt64('c')

    builder = WfstBuilder(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second)
    add_arc!(builder, first, 'a', 'b', second, 0.5)
    left = build!(builder)
    product = compose(left, host)
    arc = only(VTI.arcs(product, VTI.start(product)))
    @test arc.input == UInt64('a')
    @test arc.output == UInt64('c')
    @test arc.weight == 1.25
    @test VTI.state_info(product, arc.target).final_weight == 0.125

    snapshot = VTI.snapshot(product)
    close(product)
    @test VTI.start(snapshot) == 0
    close(snapshot)
    close(left)
    close(host)
end
