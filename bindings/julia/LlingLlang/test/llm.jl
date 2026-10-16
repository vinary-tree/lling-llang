@testset "bounded native token decoder" begin
    @test sizeof(LlingLlang.RawLlmConfig) == 56
    @test sizeof(LlingLlang.RawLlmStats) == 48
    @test_throws ArgumentError LlmConfig(temperature=0)
    @test_throws ArgumentError LlmConfig(max_candidates_per_step=0)

    builder = WfstBuilder{UInt64,TropicalWeight}(size_hint=3)
    root, middle, terminal = (add_state!(builder) for _ in 1:3)
    set_start!(builder, root)
    set_final!(builder, terminal, TropicalWeight(0))
    add_arc!(builder, root, UInt64(0), UInt64(0), middle, TropicalWeight(0))
    add_arc!(builder, middle, UInt64(1), UInt64(1), terminal, TropicalWeight(0))
    graph = build!(builder)
    session = llm_session(graph; vocab_size=2)
    close(graph)

    initial = only(llm_hypotheses(session))
    @test isempty(initial.tokens)
    @test initial.score == 0.0
    @test !initial.accepting
    @test_throws ErrorException llm_step!(session, _ -> error("scorer failed"))
    @test llm_stats(session).steps == 0
    @test llm_step!(session, _ -> [0.0, -Inf]) == LLM_ADVANCED
    @test only(llm_hypotheses(session)).tokens == UInt32[0]
    @test llm_step!(session, _ -> [-Inf, -0.25]) == LLM_COMPLETE
    @test only(llm_hypotheses(session)).tokens == UInt32[0, 1]
    @test only(llm_hypotheses(session)).accepting
    @test llm_stats(session).steps == 2
    @test llm_stats(session).score_calls == 2
    close(session)
    @test !isopen(session)
    @test_throws NativeError llm_hypotheses(session)

    builder = WfstBuilder{UInt64,TropicalWeight}(size_hint=2)
    root, terminal = add_state!(builder), add_state!(builder)
    set_start!(builder, root)
    set_final!(builder, terminal, TropicalWeight(0))
    add_arc!(builder, root, UInt64(0), UInt64(0), terminal,
        TropicalWeight(0))
    graph = build!(builder)
    session = llm_session(graph; vocab_size=1)
    cancel!(session)
    @test llm_step!(session, _ -> [0.0]) == LLM_CANCELLED
    @test llm_stats(session).steps == 0
    close(session)
    close(graph)

    builder = WfstBuilder{UInt64,TropicalWeight}(size_hint=3)
    root, prefix, longer = (add_state!(builder) for _ in 1:3)
    set_start!(builder, root)
    set_final!(builder, prefix, TropicalWeight(0))
    set_final!(builder, longer, TropicalWeight(0))
    add_arc!(builder, root, UInt64(0), UInt64(0), prefix, TropicalWeight(0))
    add_arc!(builder, prefix, UInt64(1), UInt64(1), longer, TropicalWeight(0))
    graph = build!(builder)
    session = llm_session(graph; vocab_size=2, config=LlmConfig(beam_width=2))
    @test llm_step!(session, _ -> [0.0, -Inf]) == LLM_ADVANCED
    @test only(llm_hypotheses(session)).accepting
    @test llm_step!(session, _ -> [-Inf, -0.25]) == LLM_ADVANCED
    @test Set(hypothesis.tokens for hypothesis in llm_hypotheses(session)) ==
        Set([UInt32[0], UInt32[0, 1]])
    @test llm_step!(session, _ -> [-Inf, -Inf]) == LLM_COMPLETE
    close(session)
    close(graph)
end
