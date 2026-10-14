using Test
using LlingLlang

function training_chain(label::UInt64)
    builder = WfstBuilder{UInt64,LogWeight}(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second, LogWeight(0.0))
    add_arc!(builder, first, label, label, second, LogWeight(0.0))
    build!(builder)
end

@testset "paged native log gradients" begin
    builder = WfstBuilder{UInt64,LogWeight}(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second, LogWeight(0.0))
    add_arc!(builder, first, UInt64(0), UInt64(0), second, LogWeight(0.0))
    add_arc!(builder, first, UInt64(1), UInt64(1), second, LogWeight(1.0))
    graph = build!(builder)
    expected_first = 1 / (1 + exp(-1))
    cursor = log_gradient_cursor(graph)
    @test cursor.arc_count == 2
    @test isapprox(cursor.score, -log1p(exp(-1)); atol=1e-12)
    pages = collect(gradient_pages(cursor; page_size=1))
    @test length(pages) == 2
    @test length.(pages) == [1, 1]
    @test isapprox(pages[1][1].gradient, expected_first; atol=1e-12)
    @test isapprox(pages[2][1].gradient, 1 - expected_first; atol=1e-12)
    @test pages[1][1].from == first
    @test pages[2][1].arc_index == 1
    close(cursor)
    @test !isopen(cursor)
    @test_throws ArgumentError gradient_page(cursor, 0)
    result = log_gradients(graph; page_size=1)
    @test length(result.arcs) == 2
    @test isapprox(result.score, cursor.score; atol=1e-12)
    close(graph)

    cyclic_builder = WfstBuilder{UInt64,LogWeight}(size_hint=1)
    state = add_state!(cyclic_builder)
    set_start!(cyclic_builder, state)
    set_final!(cyclic_builder, state, LogWeight(0.0))
    add_arc!(cyclic_builder, state, UInt64(0), UInt64(0), state, LogWeight(1.0))
    cyclic = build!(cyclic_builder)
    @test_throws NativeError log_gradient_cursor(cyclic)
    close(cyclic)
end

@testset "native LF-MMI matrix and graph bridge" begin
    @test sizeof(LlingLlang.LfMmiConfigRaw) == 48
    graph = training_chain(UInt64(0))
    scores = reshape([0.0], 1, 1)
    result = lfmmi_loss(scores, graph, graph;
        config=LfMmiConfig(leaky_hmm_coefficient=0,
            l2_regularize=0, xent_regularize=0))
    @test isapprox(result.loss, 0.0; atol=1e-12)
    @test result.gradients == zeros(1, 1)
    @test size(result.gradients) == size(scores)
    @test_throws NativeError lfmmi_loss(scores, graph, graph;
        budget=BudgetV2(max_states=1))
    cancellation = CancellationV2()
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test_throws TrainingCancelledError lfmmi_loss(scores, graph, graph;
        cancellation)
    close(cancellation)
    @test_throws ArgumentError lfmmi_loss(zeros(0, 1), graph, graph)
    @test_throws ArgumentError lfmmi_loss(fill(-Inf, 1, 1), graph, graph)
    close(graph)

    invalid = training_chain(typemax(UInt64))
    @test_throws NativeError lfmmi_loss(scores, invalid, invalid)
    close(invalid)
end

@testset "LF-MMI acoustic gradient agrees with finite differences" begin
    numerator = training_chain(UInt64(0))
    builder = WfstBuilder{UInt64,LogWeight}(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second, LogWeight(0.0))
    add_arc!(builder, first, UInt64(0), UInt64(0), second, LogWeight(0.0))
    add_arc!(builder, first, UInt64(1), UInt64(1), second, LogWeight(0.0))
    denominator = build!(builder)
    config = LfMmiConfig(leaky_hmm_coefficient=0.1, l2_regularize=0.05,
        xent_regularize=0)
    scores = reshape([0.3, -0.2], 1, 2)
    result = lfmmi_loss(scores, numerator, denominator; config)
    step = 1e-6
    for pdf in 1:2
        upper = copy(scores)
        lower = copy(scores)
        upper[1, pdf] += step
        lower[1, pdf] -= step
        estimate = (lfmmi_loss(upper, numerator, denominator; config).loss -
            lfmmi_loss(lower, numerator, denominator; config).loss) / (2step)
        @test isapprox(result.gradients[1, pdf], estimate; atol=1e-7)
    end
    close(numerator)
    close(denominator)
end

@testset "native CTC topology feeds acoustic training" begin
    topology = ctc_topology(3; topology=:compact)
    numerator = training_chain(UInt64(0))
    scores = reshape([0.1, 0.2, 0.3], 1, 3)
    result = lfmmi_loss(scores, numerator, topology;
        config=LfMmiConfig(leaky_hmm_coefficient=0, l2_regularize=0,
            xent_regularize=0))
    @test isfinite(result.loss)
    @test size(result.gradients) == size(scores)
    @test_throws NativeError log_gradient_cursor(topology)
    close(topology)
    close(numerator)
    @test_throws NativeError ctc_topology(3; budget=BudgetV2(max_arcs=2))
    @test_throws ArgumentError ctc_topology(3; topology=:unknown)
end
