# ABI-facing property oracles for proofs/tla/JuliaPathSearchLifecycle.tla.
# The generated include supplies the bounded seeds and exact invariant names.

function path_contract_graph(seed::Integer)
    offset = seed % 3
    builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=4)
    root = add_state!(builder)
    set_start!(builder, root)
    for (index, cost) in enumerate((1, 1, 2))
        terminal = add_state!(builder)
        set_final!(builder, terminal, TropicalWeight(0))
        label = UInt8('a') + UInt8(index - 1)
        add_arc!(builder, root, label, label, terminal,
            TropicalWeight(cost + offset))
    end
    source = build!(builder)
    try
        complete_graph(source)
    finally
        close(source)
    end
end

function with_path_contract_graph(operation, seed::Integer)
    graph = path_contract_graph(seed)
    try
        operation(graph)
    finally
        close(graph)
    end
end

function path_contract_labels(paths)
    [only(path.steps).input for path in paths]
end

function poll_sample_until_ready!(cursor)
    for _ in 1:128
        result = poll_sample_path!(cursor)
        result isa SamplePathPending || return result
    end
    error("bounded sample did not leave Pending within 128 polls")
end

function check_julia_path_property(property::Symbol, seed::Integer)
    if property === :type_ok
        if seed % 4 == 0
            @test_throws ArgumentError RankedPathLimits(max_work=0)
        elseif seed % 4 == 1
            @test_throws ArgumentError RankedPathLimits(max_frontier=0)
        elseif seed % 4 == 2
            @test_throws ArgumentError SamplePathLimits(max_samples=0)
        else
            @test_throws ArgumentError SamplePathLimits(seed=-1)
        end
        return
    end
    if property === :numeric_failure_no_output
        builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=2)
        root = add_state!(builder)
        terminal = add_state!(builder)
        set_start!(builder, root)
        set_final!(builder, terminal, TropicalWeight(floatmax(Float64)))
        add_arc!(builder, root, UInt8('a') + UInt8(seed % 16),
            UInt8('a'), terminal, TropicalWeight(floatmax(Float64)))
        source = build!(builder)
        try
            published = WfstPath[]
            @test_throws NativeError begin
                for path in paths(source)
                    push!(published, path)
                end
            end
            @test isempty(published)
        finally
            close(source)
        end
        return
    end

    with_path_contract_graph(seed) do graph
        if property === :graph_pinned
            cursor = ranked_paths(graph;
                limits=RankedPathLimits(work_per_call=1))
            close(graph)
            @test length(collect(cursor)) == 3
            @test !isopen(cursor)
        elseif property === :work_bounded
            cursor = ranked_paths(graph;
                limits=RankedPathLimits(work_per_call=1))
            @test poll_ranked_path!(cursor) isa RankedPathPending
            @test length(collect(cursor)) == 3
            limited = ranked_paths(graph;
                limits=RankedPathLimits(max_work=1, work_per_call=1))
            @test_throws NativeError collect(limited)
            @test !isopen(limited)
        elseif property === :output_bounded
            requested = seed % 4
            selected = k_best_paths(graph, requested;
                limits=RankedPathLimits(max_paths=max(requested, 1)))
            @test length(selected) == requested
            @test_throws ArgumentError k_best_paths(graph, 4;
                limits=RankedPathLimits(max_paths=3))
        elseif property === :ranked_order
            found = collect(ranked_paths(graph))
            @test path_contract_labels(found) == UInt8['a', 'b', 'c']
            @test [path.weight.value for path in found] ==
                [1 + seed % 3, 1 + seed % 3, 2 + seed % 3]
        elseif property === :prune_sound
            beam = seed % 2
            found = collect(cost_pruned_paths(graph; beam))
            @test path_contract_labels(found) ==
                (beam == 0 ? UInt8['a', 'b'] : UInt8['a', 'b', 'c'])
        elseif property === :seed_stable
            limits = SamplePathLimits(max_samples=8, seed=seed,
                strategy=:uniform, work_per_call=1)
            first = sample_n_paths(graph, 8; limits)
            second = sample_n_paths(graph, 8; limits)
            wider_slice = sample_n_paths(graph, 8;
                limits=SamplePathLimits(max_samples=8, seed=seed,
                    strategy=:uniform, work_per_call=7))
            @test path_contract_labels(first) == path_contract_labels(second)
            @test path_contract_labels(first) == path_contract_labels(wider_slice)
            @test length(first) == 8
        elseif property === :terminal_distinct
            @test length(collect(ranked_paths(graph;
                limits=RankedPathLimits(max_paths=3)))) == 3
            @test_throws PathTruncatedError collect(ranked_paths(graph;
                limits=RankedPathLimits(max_paths=1)))
            cursor = sample_paths(graph;
                limits=SamplePathLimits(max_samples=1, seed=seed))
            @test poll_sample_until_ready!(cursor) isa WfstPath
            @test_throws PathTruncatedError poll_sample_until_ready!(cursor)
            @test !isopen(cursor)
        elseif property === :cancel_sticky
            cancellation = CancellationV2()
            try
                cursor = ranked_paths(graph; cancellation)
                request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
                @test_throws PathCancelledError poll_ranked_path!(cursor)
                @test !isopen(cursor)
            finally
                close(cancellation)
            end
        elseif property === :reducer_settles
            @test_throws ErrorException reduce_ranked_paths(
                (_, _) -> error("stop after a path"), 0, graph)
            @test best_path(graph) isa WfstPath
        else
            error("unmapped formal property: $property")
        end
    end
end

include("generated_path_properties.jl")
