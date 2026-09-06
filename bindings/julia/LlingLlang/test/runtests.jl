using Test
using LlingLlang
import VinaryTreeInterop
import LLattice

const VTI = VinaryTreeInterop

const LABEL_CASES = [
    (UInt8, UInt8(0xfe)),
    (Char, 'λ'),
    (UInt64, typemax(UInt64)),
]

const WEIGHT_CASES = [
    (TropicalWeight, TropicalWeight(-2.0), TropicalWeight(3.0)),
    (LogWeight, LogWeight(2.0), LogWeight(3.0)),
    (ProbabilityWeight, ProbabilityWeight(0.5), ProbabilityWeight(0.25)),
    (ArcticWeight, ArcticWeight(-2.0), ArcticWeight(3.0)),
    (SignedTropicalWeight, SignedTropicalWeight(-2.0), SignedTropicalWeight(3.0)),
    (CountWeight, CountWeight(3), CountWeight(4)),
    (BooleanWeight, BooleanWeight(true), BooleanWeight(false)),
]

function scalar_chain(::Type{L}, ::Type{W}, label, arc_weight, final_weight;
    input_symbols=nothing, output_symbols=nothing) where {L,W<:AbstractScalarWeight}
    builder = WfstBuilder{L,W}(size_hint=2; input_symbols, output_symbols)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second, final_weight)
    add_arc!(builder, first, label, label, second, arc_weight)
    build!(builder)
end

@testset "typed ABI-v2 metadata and cancellation" begin
    @test sizeof(AbiV2Header) == 24
    @test sizeof(Id128) == 16
    @test sizeof(Digest256) == 32
    @test sizeof(WfstDescriptorV2) == 120
    @test sizeof(BudgetV2) == 72
    @test sizeof(OutcomeV2) == 96

    header = AbiV2Header(sizeof(WfstDescriptorV2),
        DESCRIPTOR_SIGNATURE_KNOWN |
        DESCRIPTOR_SNAPSHOT_PRESENT |
        DESCRIPTOR_CONTEXT_PRESENT)
    @test validate_abi_v2_header(header, sizeof(WfstDescriptorV2),
        DESCRIPTOR_SIGNATURE_KNOWN |
        DESCRIPTOR_SNAPSHOT_PRESENT |
        DESCRIPTOR_CONTEXT_PRESENT) === header

    descriptor = WfstDescriptorV2(header, Id128(fill(0x11, 16)),
        Id128(fill(0x22, 16)), Id128(fill(0x33, 16)),
        Id128(fill(0x44, 16)), Digest256(fill(0x55, 32)))
    @test typed_evidence_allowed(descriptor)
    @test identity_matches(descriptor, descriptor)

    budget = BudgetV2(max_states=100, max_work=1_000)
    @test validate_budget_v2(budget) === budget
    @test budget.header.flags == BUDGET_STATES | BUDGET_WORK

    outcome = OutcomeV2(AbiV2Header(sizeof(OutcomeV2)),
        UInt32(1), UInt32(1), UInt32(1), UInt32(1), UInt32(2), UInt32(0),
        UInt64(2), UInt64(1), UInt64(64), UInt64(3), UInt64(0), UInt64(0))
    @test authoritative_exact(outcome;
        resource_present=true, evidence_present=true)

    cancellation = CancellationV2()
    @test isnothing(cancellation_reason(cancellation))
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test cancellation_reason(cancellation) ==
        LlingLlang.CANCELLATION_REQUESTED_V2
    request!(cancellation, LlingLlang.CANCELLATION_DEADLINE_V2)
    @test cancellation_reason(cancellation) ==
        LlingLlang.CANCELLATION_REQUESTED_V2
    close(cancellation)
    close(cancellation)
    @test cancellation.closed
end

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

@testset "all built-in scalar WFST domains" begin
    @test_throws ArgumentError TropicalWeight(-Inf)
    @test_throws ArgumentError LogWeight(NaN)
    @test_throws ArgumentError ProbabilityWeight(-0.5)
    @test_throws ArgumentError ArcticWeight(Inf)
    @test_throws ArgumentError SignedTropicalWeight(-Inf)
    @test_throws ArgumentError CountWeight((UInt64(1) << 53) + 1)
    @test_throws ArgumentError BooleanWeight(2)
    @test CountWeight(2) + CountWeight(3) == CountWeight(5)
    @test CountWeight(2) * CountWeight(3) == CountWeight(6)
    @test_throws ArgumentError CountWeight(1 << 53) + CountWeight(1)
    @test_throws OverflowError CountWeight(1 << 53) * CountWeight(1 << 53)

    for (Label, label_value) in LABEL_CASES
        for (Weight, arc_weight, final_weight) in WEIGHT_CASES
            graph = scalar_chain(Label, Weight, label_value,
                arc_weight, final_weight)
            @test VTI.unit_domain(graph) == LlingLlang.unit_domain(Label)
            @test VTI.weight_domain(graph) == LlingLlang.weight_domain(Weight)
            typed_arc = only(arcs(graph, 0))
            @test typed_arc isa WfstArc{Label,Weight}
            @test typed_arc.input === label_value
            @test typed_arc.output === label_value
            @test typed_arc.weight == arc_weight
            typed_state = state(graph, typed_arc.target)
            @test typed_state isa WfstState{Label,Weight}
            @test typed_state.final
            @test typed_state.final_weight == final_weight

            imported = import_wfst(graph)
            @test VTI.unit_domain(imported) == VTI.unit_domain(graph)
            @test VTI.weight_domain(imported) == VTI.weight_domain(graph)
            @test only(arcs(imported, 0)).weight == arc_weight
            close(imported)
            close(graph)
        end
    end
end

mutable struct RetainedStateProvider <: AbstractWfstProvider
    first::ProviderState{UInt8,BooleanWeight}
end
LlingLlang.wfst_start(::RetainedStateProvider) = 0
LlingLlang.wfst_state_count(::RetainedStateProvider) = 1
LlingLlang.wfst_state(provider::RetainedStateProvider, state::UInt64) =
    state == 0 ? provider.first : ProviderState{UInt8,BooleanWeight}(valid=false)

@testset "provider cache owns published arc vectors" begin
    implementation = RetainedStateProvider(ProviderState{UInt8,BooleanWeight}(
        final=true,
        arcs=[ProviderArc{UInt8,BooleanWeight}(UInt8(1), UInt8(2), 0)]))
    graph = provider(UInt8, BooleanWeight, implementation)
    @test length(arcs(graph, 0)) == 1
    push!(implementation.first.arcs,
        ProviderArc{UInt8,BooleanWeight}(UInt8(3), UInt8(4), 0))
    @test length(arcs(graph, 0)) == 1
    close(graph)
end

@testset "domain-specific composition multiplication" begin
    cases = [
        (TropicalWeight, TropicalWeight(2), TropicalWeight(3), TropicalWeight(5)),
        (LogWeight, LogWeight(2), LogWeight(3), LogWeight(5)),
        (ProbabilityWeight, ProbabilityWeight(0.5), ProbabilityWeight(0.25),
            ProbabilityWeight(0.125)),
        (ArcticWeight, ArcticWeight(-2), ArcticWeight(3), ArcticWeight(1)),
        (SignedTropicalWeight, SignedTropicalWeight(-2), SignedTropicalWeight(3),
            SignedTropicalWeight(1)),
        (CountWeight, CountWeight(3), CountWeight(4), CountWeight(12)),
        (BooleanWeight, BooleanWeight(true), BooleanWeight(false),
            BooleanWeight(false)),
    ]
    for (Weight, left_weight, right_weight, expected) in cases
        left = scalar_chain(UInt64, Weight, UInt64(7), left_weight, one(Weight))
        right = scalar_chain(UInt64, Weight, UInt64(7), right_weight, one(Weight))
        product = compose(left, right)
        product_arc = only(arcs(product, VTI.start(product)))
        @test product_arc.weight == expected
        @test state(product, product_arc.target).final_weight == one(Weight)
        close(product)
        close(left)
        close(right)
    end

    left = scalar_chain(UInt8, TropicalWeight, UInt8(1),
        one(TropicalWeight), one(TropicalWeight))
    right = scalar_chain(UInt64, TropicalWeight, UInt64(1),
        one(TropicalWeight), one(TropicalWeight))
    @test_throws ArgumentError compose(left, right)
    close(left)
    close(right)
end

@testset "symbol tables are dense, owned, and frozen with a graph" begin
    inputs = SymbolTable{UInt8}(["known"])
    outputs = SymbolTable{UInt8}()
    @test intern!(inputs, "known") == UInt8(0)
    graph = scalar_chain(UInt8, TropicalWeight, "alpha",
        one(TropicalWeight), one(TropicalWeight);
        input_symbols=inputs, output_symbols=outputs)
    @test label(input_symbols(graph), "alpha") == UInt8(1)
    @test symbol(input_symbols(graph), UInt8(0)) == "known"
    @test label(output_symbols(graph), "alpha") == UInt8(0)
    @test collect(input_symbols(graph)) == [UInt8(0) => "known", UInt8(1) => "alpha"]
    @test isfrozen(input_symbols(graph))
    @test_throws ArgumentError intern!(input_symbols(graph), "late")
    @test !isfrozen(inputs)
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
    sum_many = semiring_plus_many(context, [zero, one])
    product_many = semiring_times_many(context, [one, one])
    quotient = semiring_divide(context, product, one)
    closure = semiring_star(context, one)

    @test semiring_equal(context, sum, one)
    @test semiring_equal(context, sum_many, one)
    @test semiring_equal(context, product_many, one)
    @test semiring_diagnostic(context) == "TropicalProvider()"
    @test semiring_diagnostic(context, one) == "0.0"
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
    for weight in (zero, sum, product, sum_many, product_many, quotient, closure, copied)
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

struct GenericScalarProvider{L,W<:AbstractScalarWeight} <: AbstractWfstProvider
    label::L
    arc_weight::W
    final_weight::W
end
LlingLlang.wfst_start(::GenericScalarProvider) = 0
LlingLlang.wfst_state_count(::GenericScalarProvider) = 2
function LlingLlang.wfst_state(provider::GenericScalarProvider{L,W}, state::UInt64) where {L,W}
    state == 0 && return ProviderState{L,W}(
        arcs=[ProviderArc{L,W}(provider.label, provider.label, 1, provider.arc_weight)])
    state == 1 && return ProviderState{L,W}(
        final=true, final_weight=provider.final_weight)
    ProviderState{L,W}(valid=false)
end

@testset "typed host providers span all scalar domains" begin
    for (Label, label_value) in LABEL_CASES
        for (Weight, arc_weight, final_weight) in WEIGHT_CASES
            host = provider(Label, Weight,
                GenericScalarProvider(label_value, arc_weight, final_weight);
                acyclic=true)
            @test VTI.unit_domain(host) == LlingLlang.unit_domain(Label)
            @test VTI.weight_domain(host) == LlingLlang.weight_domain(Weight)
            host_arc = only(arcs(host, 0))
            @test host_arc isa WfstArc{Label,Weight}
            @test host_arc.input === label_value
            @test host_arc.weight == arc_weight
            @test state(host, 1).final_weight == final_weight
            snapshot = VTI.snapshot(host)
            close(host)
            @test only(arcs(snapshot, 0)).output === label_value
            close(snapshot)
        end
    end
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
