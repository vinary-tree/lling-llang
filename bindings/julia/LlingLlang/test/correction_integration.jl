using Test
using LlingLlang
import Libdictenstein as LD
import Duallity
import VinaryTreeInterop as VTI

@testset "retained native confusion layer" begin
    @test sizeof(LlingLlang.RawConfusionConfig) == 24
    @test sizeof(LlingLlang.RawConfusionRule) == 24
    @test_throws ArgumentError ConfusionConfig(matrix=:unknown)
    @test_throws ArgumentError ConfusionConfig(threshold=NaN)
    @test_throws ArgumentError ConfusionConfig(:custom, 10, NaN)
    @test_throws ArgumentError ConfusionRule(:substitution, 'a', nothing, 0.5)
    @test_throws ArgumentError ConfusionRule(:insertion, 'a', nothing, 0.0)
    @test_throws ArgumentError confusion_layer(config=ConfusionConfig(matrix=:custom))
    @test_throws ArgumentError confusion_layer(rules=[
        ConfusionRule(:substitution, 'α', 'a', 0.4)])

    custom = confusion_layer(config=ConfusionConfig(matrix=:custom, threshold=0.1),
        rules=[ConfusionRule(:substitution, 'α', 'a', 0.4)])
    found = corrections(custom, "cαt")
    @test length(found) == 1
    @test only(found).word == "cat"
    @test only(found).cost ≈ -log(0.4)
    @test_throws NativeError corrections(custom, repeat("é", 257))
    @test_throws NativeError corrections(custom, repeat("a", 1025))
    close(custom)
    @test !isopen(custom)
    @test_throws NativeError corrections(custom, "cαt")

    builtin = confusion_layer(config=ConfusionConfig(matrix=:qwerty))
    @test !isempty(corrections(builtin, "cat"))
    close(builtin)

    tie = confusion_layer(config=ConfusionConfig(matrix=:custom, threshold=0.1),
        rules=[ConfusionRule(:substitution, 'x', 'b', 0.2),
            ConfusionRule(:substitution, 'x', 'a', 0.2)])
    @test [candidate.word for candidate in corrections(tie, "xx")] ==
        ["ax", "bx", "xa", "xb"]
    close(tie)

    edits = confusion_layer(config=ConfusionConfig(matrix=:custom, threshold=0.1),
        rules=[ConfusionRule(:insertion, 'x', nothing, 0.3),
            ConfusionRule(:deletion, nothing, 'b', 0.4)])
    costs = Dict(candidate.word => candidate.cost
        for candidate in corrections(edits, "ax"))
    @test costs["a"] ≈ -log(0.3)
    @test Set(keys(costs)) == Set(["a", "abx", "axb", "bax"])
    @test all(costs[word] ≈ -log(0.4) for word in ("abx", "axb", "bax"))
    close(edits)
end

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

    @test_throws ArgumentError correction_llm_session(layer, "cut", ["cat", "cat"])
    @test_throws ArgumentError correction_llm_session(layer, "cut", ["dog"])
    @test_throws ArgumentError correction_llm_session(layer, "cut", ["cat"];
        edit_weight=-1)
    @test_throws ArgumentError correction_llm_session(layer, "cut", ["cat"];
        model_weight=big"1e1000")
    session = correction_llm_session(layer, "cat", ["cat", "cot", "dog"];
        edit_weight=3, config=LlmConfig(beam_width=3, temperature=2))
    cancelled = correction_llm_session(layer, "cat", ["cat", "cot"])
    close(layer)
    @test !isopen(layer)
    @test_throws NativeError corrections(layer, "cut")
    @test_throws ErrorException llm_step!(session, _ -> error("model unavailable"))
    @test llm_stats(session).steps == 0
    @test llm_step!(session, _ -> [-2.0, -0.1, -Inf]) == LLM_COMPLETE
    selected = llm_hypotheses(session)
    @test session.words[Int(first(selected[1].tokens)) + 1] == "cat"
    @test selected[1].score == -1.0
    @test llm_stats(session).score_calls == 1
    close(session)
    @test !isopen(session)

    cancel!(cancelled)
    @test llm_step!(cancelled, _ -> [0.0, 0.0]) == LLM_CANCELLED
    @test llm_stats(cancelled).steps == 0
    close(cancelled)

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

    session = correction_llm_session(layer, "cat", ["cat", "cot", "dog"];
        config=LlmConfig(beam_width=3))
    @test llm_step!(session, _ -> [-2.0, -0.1, -Inf]) == LLM_COMPLETE
    @test Dict(session.words[Int(first(h.tokens)) + 1] => h.score
        for h in llm_hypotheses(session)) ==
        Dict("cat" => -2.0, "cot" => -1.1)
    close(session)

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
