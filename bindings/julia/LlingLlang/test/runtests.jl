using Test
using LlingLlang
import VinaryTreeInterop

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
