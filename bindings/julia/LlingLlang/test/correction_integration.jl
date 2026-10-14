using Test
using LlingLlang
import Libdictenstein as LD
import Duallity
import VinaryTreeInterop as VTI

function accepted_language(graph)
    accepted = Dict{String,Float64}()
    stack = [(VTI.start(graph), "", 0.0)]
    visited = 0
    while !isempty(stack)
        state, output, cost = pop!(stack)
        visited += 1
        visited <= 100_000 || error("bounded WFST traversal did not converge")
        info = VTI.state_info(graph, state)
        info === nothing && continue
        if info.final
            candidate = cost + info.final_weight
            accepted[output] = min(get(accepted, output, Inf), candidate)
        end
        for arc in VTI.arcs(graph, state)
            suffix = isnothing(arc.output) ? "" : string(Char(UInt32(arc.output)))
            push!(stack, (arc.target, output * suffix, cost + arc.weight))
        end
    end
    accepted
end

@testset "retained libdictenstein correction layer" begin
    @test sizeof(LlingLlang.RawCorrectionConfig) == 72
    @test_throws ArgumentError CorrectionConfig(max_distance=-1)
    @test_throws ArgumentError CorrectionConfig(cost_per_edit=NaN)
    @test_throws ArgumentError CorrectionConfig(case_insensitive=true)

    producer = LD.DynamicDawg()
    LD.insert_batch!(producer,
        ["cat" => nothing, "cot" => nothing, "dog" => nothing])
    snapshot = LD.snapshot(producer)
    layer = correction_dictionary(snapshot;
        config=CorrectionConfig(max_distance=1, min_word_length=1))
    overflow_layer = correction_dictionary(snapshot;
        config=CorrectionConfig(max_distance=3, min_word_length=1,
            cost_per_edit=1e308))
    @test_throws NativeError corrections(overflow_layer, "cat")
    close(overflow_layer)
    close(snapshot)
    close(producer)

    found = corrections(layer, "cut")
    @test Set(candidate.word for candidate in found) == Set(["cat", "cot"])
    @test all(candidate.cost == 1.0 for candidate in found)
    @test isempty(corrections(layer, ""))
    close(layer)
    @test !isopen(layer)
    @test_throws NativeError corrections(layer, "cut")

    producer = LD.DynamicDawg()
    LD.insert_batch!(producer, ["cat" => nothing])
    snapshot = LD.snapshot(producer)
    layer = correction_dictionary(snapshot; config=CorrectionConfig(
        max_distance=0, min_word_length=1, case_insensitive=true,
        normalization=:unicode_lowercase_keys))
    @test only(corrections(layer, "CAT")).word == "cat"
    close(layer)
    close(snapshot)
    close(producer)
end

@testset "duallity query product agrees with native correction" begin
    producer = LD.DynamicDawg()
    LD.insert_batch!(producer,
        ["cat" => nothing, "cot" => nothing, "dog" => nothing])
    snapshot = LD.snapshot(producer)
    layer = correction_dictionary(snapshot;
        config=CorrectionConfig(max_distance=1))
    query = Duallity.wfst(snapshot, "cat"; maximum_distance=1)
    close(snapshot)
    close(producer)

    expected = Dict(candidate.word => candidate.cost
        for candidate in corrections(layer, "cat"))
    @test expected == Dict("cat" => 0.0, "cot" => 1.0)
    @test accepted_language(query) == expected

    mapper_builder = WfstBuilder(size_hint=1)
    state = add_state!(mapper_builder)
    set_start!(mapper_builder, state)
    set_final!(mapper_builder, state)
    for character in ['a', 'c', 'o', 't']
        add_arc!(mapper_builder, state, character, uppercase(character), state)
    end
    mapper = build!(mapper_builder)
    product = compose(query, mapper)
    @test accepted_language(product) == Dict("CAT" => 0.0, "COT" => 1.0)
    close(product)
    close(mapper)
    close(query)
    close(layer)
end
