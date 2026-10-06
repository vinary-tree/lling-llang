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

@testset "snapshot-pinned bounded path traversal" begin
    @test sizeof(LlingLlang.RawPathConfig) == 56
    @test sizeof(LlingLlang.RawPathStep) == 48
    @test_throws ArgumentError PathLimits(max_states=0)
    @test_throws ArgumentError PathLimits(max_work=-1)
    @test_throws ArgumentError PathLimits(work_per_call=true)

    builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=2)
    root = add_state!(builder)
    terminal = add_state!(builder)
    set_start!(builder, root)
    set_final!(builder, root, TropicalWeight(0))
    set_final!(builder, terminal, TropicalWeight(3))
    add_arc!(builder, root, UInt8('a'), UInt8('A'), terminal,
        TropicalWeight(2))
    graph = build!(builder)

    iterator = paths(graph; limits=PathLimits(max_states=2, max_arcs=1,
        max_depth=1, max_paths=3, work_per_call=1))
    close(graph)
    found = collect(iterator)
    @test length(found) == 2
    @test isempty(found[1].steps)
    @test found[1].weight == TropicalWeight(0)
    @test length(found[2].steps) == 1
    @test found[2].steps[1].input == UInt8('a')
    @test found[2].steps[1].output == UInt8('A')
    @test found[2].weight == TropicalWeight(5)
    @test !isopen(iterator)

    graph = scalar_chain(UInt8, TropicalWeight, UInt8('b'),
        TropicalWeight(2), TropicalWeight(3))
    sliced = paths(graph; limits=PathLimits(max_states=2, max_arcs=1,
        max_depth=1, max_paths=2, work_per_call=1))
    @test poll_path!(sliced) isa PathPending
    @test only(collect(sliced)).weight == TropicalWeight(5)
    @test poll_path!(sliced) === nothing
    @test only(collect(paths(graph; limits=PathLimits(max_states=2,
        max_arcs=1, max_depth=1, max_paths=2,
        work_per_call=1)))).weight == TropicalWeight(5)
    @test reduce_paths((count, _) -> count + 1, 0, graph;
        limits=PathLimits(max_states=2, max_arcs=1, max_depth=1,
            max_paths=2, work_per_call=1)) == 1
    @test_throws PathTruncatedError collect(paths(graph;
        limits=PathLimits(max_states=2, max_arcs=1, max_depth=0,
            max_paths=2)))
    @test_throws PathTruncatedError collect(paths(graph;
        limits=PathLimits(max_states=2, max_arcs=1, max_depth=1,
            max_paths=1)))
    @test_throws NativeError collect(paths(graph;
        limits=PathLimits(max_states=1, max_arcs=1, max_depth=1,
            max_paths=2)))

    overflowing = scalar_chain(UInt8, TropicalWeight, UInt8('z'),
        TropicalWeight(floatmax(Float64)), TropicalWeight(floatmax(Float64)))
    @test_throws NativeError collect(paths(overflowing;
        limits=PathLimits(max_states=2, max_arcs=1, max_depth=1,
            max_paths=2)))
    close(overflowing)

    cancellation = CancellationV2()
    iterator = paths(graph; cancellation)
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test_throws PathCancelledError collect(iterator)
    @test !isopen(iterator)
    close(cancellation)
    close(graph)
end

@testset "bounded resumable reachable-graph capture" begin
    @test sizeof(LlingLlang.RawGraphConfig) == 40
    @test sizeof(LlingLlang.RawGraphArc) == 48
    @test_throws ArgumentError GraphLimits(max_states=0)
    @test_throws ArgumentError GraphLimits(work_per_call=false)

    graph = scalar_chain(UInt8, TropicalWeight, UInt8('a'),
        TropicalWeight(2), TropicalWeight(3))
    cursor = capture_graph(graph; limits=GraphLimits(max_states=2,
        max_arcs=1, max_work=4, work_per_call=1))
    close(graph)
    @test poll_graph!(cursor) isa GraphPending
    snapshot = nothing
    while isnothing(snapshot)
        result = poll_graph!(cursor)
        if !(result isa GraphPending)
            snapshot = result
        end
    end
    @test snapshot isa GraphSnapshot
    @test !isopen(cursor)
    @test graph_info(snapshot) == (start_raw=UInt64(0),
        states=Csize_t(2), arcs=Csize_t(1))
    first = graph_state(snapshot, 0)
    second = graph_state(snapshot, 1)
    @test first.raw_id == 0
    @test !first.final
    @test length(first.arcs) == 1
    @test first.arcs[1].input == UInt8('a')
    @test first.arcs[1].target_local == 1
    @test first.arcs[1].target_raw == 1
    @test isempty(second.arcs)
    @test second.final_weight == TropicalWeight(3)
    close(snapshot)
    @test !isopen(snapshot)

    graph = scalar_chain(UInt8, TropicalWeight, UInt8('b'),
        TropicalWeight(1), TropicalWeight(2))
    full = complete_graph(graph; limits=GraphLimits(max_states=2,
        max_arcs=1, max_work=4, work_per_call=1))
    @test graph_info(full).states == 2
    close(full)
    @test_throws NativeError complete_graph(graph;
        limits=GraphLimits(max_states=2, max_arcs=0,
            max_work=4, work_per_call=1))
    cancellation = CancellationV2()
    cursor = capture_graph(graph; cancellation)
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test_throws GraphCancelledError poll_graph!(cursor)
    @test !isopen(cursor)
    close(cancellation)
    close(graph)
end

@testset "bounded exact native graph distances" begin
    @test sizeof(LlingLlang.RawDistanceConfig) == 24
    @test_throws ArgumentError DistanceLimits(max_work=0)
    @test_throws ArgumentError DistanceLimits(work_per_call=false)

    for (Weight, first_weight, final_weight, expected) in [
        (TropicalWeight, TropicalWeight(2), TropicalWeight(3), TropicalWeight(5)),
        (LogWeight, LogWeight(2), LogWeight(3), LogWeight(5)),
        (ProbabilityWeight, ProbabilityWeight(0.4), ProbabilityWeight(0.5),
            ProbabilityWeight(0.2)),
        (ArcticWeight, ArcticWeight(2), ArcticWeight(3), ArcticWeight(5)),
        (SignedTropicalWeight, SignedTropicalWeight(-2), SignedTropicalWeight(3),
            SignedTropicalWeight(1)),
        (CountWeight, CountWeight(2), CountWeight(3), CountWeight(6)),
        (BooleanWeight, BooleanWeight(true), BooleanWeight(true), BooleanWeight(true)),
    ]
        source = scalar_chain(UInt8, Weight, UInt8('d'), first_weight, final_weight)
        graph = complete_graph(source; limits=GraphLimits(max_states=2,
            max_arcs=1, max_work=4, work_per_call=1))
        close(source)
        cursor = analyze_distances(graph; limits=DistanceLimits(
            max_work=30, work_per_call=1))
        close(graph)
        @test poll_distance!(cursor) isa DistancePending
        result = nothing
        while isnothing(result)
            polled = poll_distance!(cursor)
            if !(polled isa DistancePending)
                result = polled
            end
        end
        @test result isa DistanceResult
        @test !isopen(cursor)
        @test distance_info(result) == (total=expected, states=Csize_t(2))
        page = distance_page(result, 0; capacity=1)
        @test page.total == 2
        @test length(page.forward) == 1
        @test page.forward[1] == one(Weight)
        @test length(page.backward) == 1
        @test page.backward[1] == expected
        @test distance_page(result, 1).backward == [final_weight]
        if Weight in (LogWeight, ProbabilityWeight, CountWeight)
            @test isapprox(only(posterior_arcs(result, 0).probabilities), 1.0;
                atol=1e-12)
            @test isapprox(posterior_final(result, 1), 1.0; atol=1e-12)
            @test posterior_final(result, 0) == 0.0
        else
            @test_throws NativeError posterior_arcs(result, 0)
        end
        close(result)
        @test !isopen(result)
    end

    source = scalar_chain(UInt8, TropicalWeight, UInt8('w'),
        TropicalWeight(1), TropicalWeight(1))
    graph = complete_graph(source)
    @test_throws NativeError complete_distances(graph;
        limits=DistanceLimits(max_work=1, work_per_call=1))
    cancellation = CancellationV2()
    cursor = analyze_distances(graph; cancellation)
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test_throws DistanceCancelledError poll_distance!(cursor)
    @test !isopen(cursor)
    close(cancellation)
    close(graph)
    close(source)
end

@testset "bounded best-first accepting paths" begin
    @test sizeof(LlingLlang.RawRankedPathConfig) == 48
    @test_throws ArgumentError RankedPathLimits(max_work=0)
    @test_throws ArgumentError RankedPathLimits(max_frontier=0)

    for (Weight, first_weight, final_weight, expected) in [
        (TropicalWeight, TropicalWeight(2), TropicalWeight(3), TropicalWeight(5)),
        (LogWeight, LogWeight(2), LogWeight(3), LogWeight(5)),
        (ProbabilityWeight, ProbabilityWeight(0.4), ProbabilityWeight(0.5),
            ProbabilityWeight(0.2)),
        (ArcticWeight, ArcticWeight(2), ArcticWeight(3), ArcticWeight(5)),
        (SignedTropicalWeight, SignedTropicalWeight(-2), SignedTropicalWeight(3),
            SignedTropicalWeight(1)),
        (CountWeight, CountWeight(2), CountWeight(3), CountWeight(6)),
        (BooleanWeight, BooleanWeight(true), BooleanWeight(true),
            BooleanWeight(true)),
    ]
        source = scalar_chain(UInt8, Weight, UInt8('r'), first_weight, final_weight)
        graph = complete_graph(source)
        close(source)
        iterator = ranked_paths(graph; limits=RankedPathLimits(work_per_call=1))
        close(graph)
        @test poll_ranked_path!(iterator) isa RankedPathPending
        paths_found = collect(iterator)
        @test length(paths_found) == 1
        @test only(paths_found).weight == expected
        @test only(paths_found).steps[1].input == UInt8('r')
        @test !isopen(iterator)
    end

    source = scalar_chain(UInt8, TropicalWeight, UInt8('s'),
        TropicalWeight(1), TropicalWeight(2))
    graph = complete_graph(source)
    @test reduce_ranked_paths((n, _) -> n + 1, 0, graph) == 1
    @test best_path(graph).weight == TropicalWeight(3)
    @test only(k_best_paths(graph, 1)).weight == TropicalWeight(3)
    @test only(n_best_paths(graph, 2)).weight == TropicalWeight(3)
    @test isempty(k_best_paths(graph, 0))
    @test_throws ArgumentError k_best_paths(graph, 2;
        limits=RankedPathLimits(max_paths=1))
    @test_throws PathTruncatedError collect(ranked_paths(graph;
        limits=RankedPathLimits(max_depth=0)))
    @test_throws PathTruncatedError collect(ranked_paths(graph;
        limits=RankedPathLimits(max_paths=0)))
    @test_throws NativeError collect(ranked_paths(graph;
        limits=RankedPathLimits(max_work=1, work_per_call=1)))
    cancellation = CancellationV2()
    iterator = ranked_paths(graph; cancellation)
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test_throws PathCancelledError collect(iterator)
    @test !isopen(iterator)
    close(cancellation)
    close(graph)
    close(source)
end

@testset "exact cost-window path pruning" begin
    builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=5)
    start = add_state!(builder)
    destinations = [add_state!(builder) for _ in 1:4]
    set_start!(builder, start)
    for (index, destination) in enumerate(destinations)
        set_final!(builder, destination, TropicalWeight(0))
        label = UInt8('a') + UInt8(index - 1)
        add_arc!(builder, start, label, label, destination,
            TropicalWeight((1, 2, 2, 4)[index]))
    end
    source = build!(builder)
    graph = complete_graph(source)
    close(source)
    @test_throws ArgumentError cost_pruned_paths(graph; beam=-1)
    @test_throws ArgumentError cost_pruned_paths(graph; beam=Inf)
    @test_throws ArgumentError cost_pruned_paths(graph; beam=NaN)
    @test [only(path.steps).input for path in collect(
        cost_pruned_paths(graph; beam=0))] == [UInt8('a')]
    @test reduce_cost_pruned_paths((n, _) -> n + 1, 0, graph;
        beam=1) == 3
    @test_throws PathTruncatedError collect(cost_pruned_paths(graph;
        beam=10, limits=RankedPathLimits(max_paths=1)))
    cancellation = CancellationV2()
    cancelled = cost_pruned_paths(graph; beam=10, cancellation)
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test_throws PathCancelledError collect(cancelled)
    @test !isopen(cancelled)
    close(cancellation)
    cursor = cost_pruned_paths(graph; beam=1,
        limits=RankedPathLimits(work_per_call=1))
    close(graph)
    @test poll_cost_pruned_path!(cursor) isa RankedPathPending
    @test [only(path.steps).input for path in collect(cursor)] ==
        [UInt8('a'), UInt8('b'), UInt8('c')]
    @test !isopen(cursor)
    @test poll_cost_pruned_path!(cursor) === nothing

    builder = WfstBuilder{UInt8,ProbabilityWeight}(size_hint=4)
    start = add_state!(builder)
    for (index, mass) in enumerate((0.5, 0.25, 0.125))
        destination = add_state!(builder)
        set_final!(builder, destination, ProbabilityWeight(1))
        label = UInt8('p') + UInt8(index - 1)
        add_arc!(builder, start, label, label, destination,
            ProbabilityWeight(mass))
    end
    set_start!(builder, start)
    source = build!(builder)
    graph = complete_graph(source)
    close(source)
    @test [path.weight for path in collect(cost_pruned_paths(graph;
        beam=0.7))] == [ProbabilityWeight(0.5), ProbabilityWeight(0.25)]
    close(graph)
end

@testset "seeded bounded accepting-path sampling" begin
    @test sizeof(LlingLlang.RawSamplePathConfig) == 56
    @test_throws ArgumentError SamplePathLimits(max_samples=0)
    @test_throws ArgumentError SamplePathLimits(strategy=:unknown)
    @test_throws ArgumentError SamplePathLimits(seed=-1)

    for (Weight, arc_weight, final_weight, expected) in [
        (TropicalWeight, TropicalWeight(2), TropicalWeight(3), TropicalWeight(5)),
        (LogWeight, LogWeight(2), LogWeight(3), LogWeight(5)),
        (ProbabilityWeight, ProbabilityWeight(0.4), ProbabilityWeight(0.5),
            ProbabilityWeight(0.2)),
        (ArcticWeight, ArcticWeight(2), ArcticWeight(3), ArcticWeight(5)),
        (SignedTropicalWeight, SignedTropicalWeight(-2), SignedTropicalWeight(3),
            SignedTropicalWeight(1)),
        (CountWeight, CountWeight(2), CountWeight(3), CountWeight(6)),
        (BooleanWeight, BooleanWeight(true), BooleanWeight(true), BooleanWeight(true)),
    ]
        source = scalar_chain(UInt8, Weight, UInt8('m'), arc_weight, final_weight)
        graph = complete_graph(source)
        close(source)
        iterator = sample_paths(graph; limits=SamplePathLimits(
            max_samples=1, work_per_call=1, seed=42))
        close(graph)
        @test poll_sample_path!(iterator) isa SamplePathPending
        sampled = nothing
        while isnothing(sampled)
            result = poll_sample_path!(iterator)
            result isa SamplePathPending || (sampled = result)
        end
        @test sampled.steps[1].input == UInt8('m')
        @test sampled.weight == expected
        @test_throws PathTruncatedError poll_sample_path!(iterator)
        @test !isopen(iterator)
    end

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
    limits = SamplePathLimits(max_samples=40, strategy=:proportional, seed=123,
        work_per_call=1)
    a = sample_n_paths(graph, 40; limits)
    b = sample_n_paths(graph, 40; limits)
    @test [only(path.steps).input for path in a] ==
        [only(path.steps).input for path in b]
    @test count(path -> only(path.steps).input == UInt8('b'), a) > 20
    @test sample_path(graph; limits).steps[1].input == a[1].steps[1].input
    @test reduce_sampled_paths((n, _) -> n + 1, 0, graph, 5; limits) == 5
    @test isempty(sample_n_paths(graph, 0; limits))
    @test reduce_sampled_paths((n, _) -> n + 1, 0, graph, 0; limits) == 0
    @test_throws ArgumentError sample_n_paths(graph, 41; limits)
    @test_throws NativeError sample_n_paths(graph, 1;
        limits=SamplePathLimits(max_work=1, work_per_call=1))
    @test_throws PathTruncatedError sample_n_paths(graph, 1;
        limits=SamplePathLimits(max_depth=0))
    cancellation = CancellationV2()
    cursor = sample_paths(graph; cancellation)
    request!(cancellation, LlingLlang.CANCELLATION_REQUESTED_V2)
    @test_throws PathCancelledError poll_sample_path!(cursor)
    @test !isopen(cursor)
    close(cancellation)
    close(graph)
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

@testset "native unary WFST operations and cumulative budgets" begin
    builder = WfstBuilder(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second, 0.0)
    add_arc!(builder, first, 'a', 'b', second, 0.5)
    graph = build!(builder)
    input = project_input(graph)
    output = project_output(graph)
    reversed = reverse(graph)
    @test only(arcs(input, 0)).input === 'a'
    @test only(arcs(input, 0)).output === 'a'
    @test only(arcs(output, 0)).input === 'b'
    @test only(arcs(output, 0)).output === 'b'
    reversed_first = only(arcs(reversed, 0))
    reversed_second = only(arcs(reversed, reversed_first.target))
    @test reversed_second.input === 'a'
    @test reversed_second.output === 'b'
    @test_throws NativeError project_input(graph; budget=BudgetV2(max_states=3))
    ref_handle = LlingLlang.unary_wfst_call(:project_input, graph, BudgetV2();
        pointer_form=true)
    ref_projection = LlingLlang.adopt_native_wfst(ref_handle, Char, TropicalWeight)
    @test only(arcs(ref_projection, 0)).output === 'a'
    close(ref_projection)
    close(reversed)
    close(output)
    close(input)
    close(graph)
end

@testset "native rational WFST algebra and lazy repetition" begin
    builder = WfstBuilder(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second, 0.0)
    add_arc!(builder, first, 'a', 'b', second, 0.5)
    graph = build!(builder)

    either = union(graph, graph;
        budget=BudgetV2(max_states=9, max_arcs=6, max_work=15))
    @test length(arcs(either, 0)) == 2
    @test all(arc -> isnothing(arc.input) && isnothing(arc.output), arcs(either, 0))
    twice = concat(graph, graph)
    @test length(arcs(twice, 1)) == 1
    @test isnothing(only(arcs(twice, 1)).input)
    zero_or_more = closure(graph)
    @test VTI.state_info(zero_or_more, 0).final
    one_or_more = closure_plus(graph)
    @test !VTI.state_info(one_or_more, 0).final
    @test any(arc -> isnothing(arc.input) && arc.target == 0, arcs(one_or_more, 1))
    @test_throws NativeError union(graph, graph; budget=BudgetV2(max_states=8))

    ref_handle = LlingLlang.rational_binary_wfst_call(:union, graph, graph,
        BudgetV2(); pointer_form=true)
    ref_union = LlingLlang.adopt_native_wfst(ref_handle, Char, TropicalWeight)
    @test length(arcs(ref_union, 0)) == 2
    close(ref_union)
    ref_handle = LlingLlang.unary_wfst_call(:closure_plus, graph, BudgetV2();
        pointer_form=true)
    ref_plus = LlingLlang.adopt_native_wfst(ref_handle, Char, TropicalWeight)
    @test any(arc -> isnothing(arc.input) && arc.target == 0, arcs(ref_plus, 1))
    close(ref_plus)

    close(one_or_more)
    close(zero_or_more)
    close(twice)
    close(either)
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

@testset "mixed VinaryTreeInterop and lling-llang composition" begin
    first = scalar_chain(UInt8, TropicalWeight, UInt8(7),
        one(TropicalWeight), one(TropicalWeight))
    second = scalar_chain(UInt8, TropicalWeight, UInt8(7),
        one(TropicalWeight), one(TropicalWeight))
    left_mixed = compose(first.native, second)
    right_mixed = compose(first, second.native)
    @test length(arcs(left_mixed, VTI.start(left_mixed))) == 1
    @test length(arcs(right_mixed, VTI.start(right_mixed))) == 1
    close(left_mixed)
    close(right_mixed)

    wrong = scalar_chain(UInt64, TropicalWeight, UInt64(7),
        one(TropicalWeight), one(TropicalWeight))
    @test_throws ArgumentError compose(wrong.native, second)
    @test_throws ArgumentError compose(first, wrong.native)
    close(wrong)
    close(first)
    close(second)
end

@testset "materializing core transforms require four-axis budgets" begin
    graph = scalar_chain(UInt8, TropicalWeight, UInt8(7),
        one(TropicalWeight), one(TropicalWeight))
    budget = BudgetV2(max_states=16, max_arcs=256,
        max_bytes=100_000, max_work=100_000)
    for operation in (determinize, minimize, remove_epsilon, connect)
        output = operation(graph; budget)
        @test length(arcs(output, VTI.start(output))) == 1
        close(output)
    end
    @test_throws NativeError connect(graph; budget=BudgetV2())
    raw = LlingLlang.unary_wfst_call(:determinize, graph, budget;
        pointer_form=true)
    pointer_output = LlingLlang.adopt_native_wfst(raw, UInt8, TropicalWeight)
    @test length(arcs(pointer_output, VTI.start(pointer_output))) == 1
    close(pointer_output)
    close(graph)
end

@testset "weighted acceptor intersection checks domains and budgets" begin
    left = scalar_chain(UInt8, TropicalWeight, UInt8('a'),
        TropicalWeight(2), one(TropicalWeight))
    right = scalar_chain(UInt8, TropicalWeight, UInt8('a'),
        TropicalWeight(3), one(TropicalWeight))
    budget = BudgetV2(max_states=1_000, max_arcs=10_000,
        max_bytes=10_000_000, max_work=100_000)
    for pointer_form in (false, true)
        intersection = acceptor_intersect(left, right; budget, pointer_form)
        @test only(arcs(intersection, VTI.start(intersection))).weight ==
            TropicalWeight(5)
        close(intersection)
    end
    transducer_builder = WfstBuilder{UInt8,TropicalWeight}(size_hint=2)
    first, last = add_state!(transducer_builder), add_state!(transducer_builder)
    set_start!(transducer_builder, first)
    set_final!(transducer_builder, last, one(TropicalWeight))
    add_arc!(transducer_builder, first, UInt8('a'), UInt8('b'), last,
        one(TropicalWeight))
    transducer = build!(transducer_builder)
    @test_throws NativeError acceptor_intersect(left, transducer; budget)
    @test_throws NativeError acceptor_intersect(left, right; budget=BudgetV2())
    close(transducer)
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

struct UnprintableProviderFailure <: Exception end
Base.showerror(::IO, ::UnprintableProviderFailure) =
    throw(ErrorException("diagnostic rendering failed"))

struct ThrowingSemiringProvider <: AbstractSemiringProvider end
LlingLlang.semiring_zero(::ThrowingSemiringProvider) =
    throw(UnprintableProviderFailure())

struct ReentrantSemiringProvider <: AbstractSemiringProvider
    context::Base.RefValue{Ptr{Cvoid}}
    nested_status::Base.RefValue{Cint}
end
function LlingLlang.semiring_zero(provider::ReentrantSemiringProvider)
    output = Ref(VTI.VtSemiringValue(0, 0))
    provider.nested_status[] = ccall(LlingLlang.SEMIRING_CALLBACKS[:one], Cint,
        (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}), provider.context[], output)
    if provider.nested_status[] == Cint(VTI.STATUS_OK)
        @assert ccall(LlingLlang.SEMIRING_CALLBACKS[:release_values], Cint,
            (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}, Csize_t),
            provider.context[], output, 1) == Cint(VTI.STATUS_OK)
    end
    0
end
LlingLlang.semiring_one(::ReentrantSemiringProvider) = 1

struct BlockingSemiringProvider <: AbstractSemiringProvider
    entered::Channel{Bool}
    release::Channel{Bool}
end
function LlingLlang.semiring_zero(provider::BlockingSemiringProvider)
    put!(provider.entered, true)
    take!(provider.release)
    0
end
LlingLlang.semiring_one(::BlockingSemiringProvider) = 1

@testset "semiring provider callback containment and atomic release" begin
    release_batch(raw, batch) = GC.@preserve batch ccall(
        LlingLlang.SEMIRING_CALLBACKS[:release_values], Cint,
        (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Csize_t),
        raw.context, pointer(batch), length(batch))

    resource = semiring_provider(TropicalProvider();
        domain_id=VTI.interface_id("test.atomic.rel1"))
    raw = VTI.raw_resource(resource)
    context = LlingLlang.semiring_provider_context(raw.context)
    @test !isnothing(context)

    valid = LlingLlang.allocate_semiring_value(context, 9.0)
    invalid = VTI.VtSemiringValue(valid.word0, valid.word1 + UInt64(1))
    for batch in ([valid, invalid], [valid, valid])
        @test release_batch(raw, batch) == Cint(VTI.STATUS_PROVIDER_ERROR)
        @test LlingLlang.resolve_semiring_value(context, valid) == 9.0
    end
    @test release_batch(raw, [valid]) == Cint(VTI.STATUS_OK)
    @test_throws ArgumentError LlingLlang.resolve_semiring_value(context, valid)

    # A generation that cannot advance is retired rather than wrapped.
    last = LlingLlang.allocate_semiring_value(context, 11.0)
    lock(context.arena_lock) do
        context.slots[Int(last.word0)].generation = typemax(UInt64)
    end
    exhausted = VTI.VtSemiringValue(last.word0, typemax(UInt64))
    @test release_batch(raw, [exhausted]) == Cint(VTI.STATUS_OK)
    replacement = LlingLlang.allocate_semiring_value(context, 12.0)
    @test replacement.word0 != exhausted.word0
    @test release_batch(raw, [replacement]) == Cint(VTI.STATUS_OK)

    recursive = ReentrantSemiringProvider(Ref(C_NULL), Ref(Cint(-1)))
    serial = semiring_provider(recursive;
        domain_id=VTI.interface_id("test.reentrant.s"))
    serial_raw = VTI.raw_resource(serial)
    recursive.context[] = serial_raw.context
    result = Ref(VTI.VtSemiringValue(0, 0))
    @test ccall(LlingLlang.SEMIRING_CALLBACKS[:zero], Cint,
        (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}),
        serial_raw.context, result) == Cint(VTI.STATUS_OK)
    @test recursive.nested_status[] == Cint(VTI.STATUS_PROVIDER_ERROR)
    @test occursin("recursive", LlingLlang.semiring_provider_context(
        serial_raw.context).last_error)
    @test release_batch(serial_raw, [result[]]) == Cint(VTI.STATUS_OK)
    close(serial)

    parallel_implementation = ReentrantSemiringProvider(Ref(C_NULL), Ref(Cint(-1)))
    parallel_host = semiring_provider(parallel_implementation;
        domain_id=VTI.interface_id("test.parallel.s."),
        thread_bound=false, parallel=true)
    parallel_raw = VTI.raw_resource(parallel_host)
    parallel_implementation.context[] = parallel_raw.context
    @test ccall(LlingLlang.SEMIRING_CALLBACKS[:zero], Cint,
        (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}),
        parallel_raw.context, result) == Cint(VTI.STATUS_OK)
    @test parallel_implementation.nested_status[] == Cint(VTI.STATUS_OK)
    @test release_batch(parallel_raw, [result[]]) == Cint(VTI.STATUS_OK)
    close(parallel_host)

    blocking = BlockingSemiringProvider(Channel{Bool}(1), Channel{Bool}(1))
    shared = semiring_provider(blocking;
        domain_id=VTI.interface_id("test.block.sem1."), thread_bound=false)
    shared_raw = VTI.raw_resource(shared)
    worker = Base.Threads.@spawn begin
        output = Ref(VTI.VtSemiringValue(0, 0))
        status = ccall(LlingLlang.SEMIRING_CALLBACKS[:zero], Cint,
            (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}), shared_raw.context, output)
        (status, output[])
    end
    entered = Base.timedwait(() -> isready(blocking.entered), 5)
    @test entered == :ok
    if entered == :ok
        take!(blocking.entered)
        @test ccall(LlingLlang.SEMIRING_CALLBACKS[:one], Cint,
            (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}),
            shared_raw.context, result) == Cint(VTI.STATUS_PROVIDER_ERROR)
    end
    put!(blocking.release, true)
    status, completed = fetch(worker)
    @test status == Cint(VTI.STATUS_OK)
    @test release_batch(shared_raw, [completed]) == Cint(VTI.STATUS_OK)
    close(shared)

    if Base.Threads.nthreads() > 1
        bounded = semiring_provider(TropicalProvider();
            domain_id=VTI.interface_id("test.bound.sem1."))
        bounded_raw = VTI.raw_resource(bounded)
        creator = LlingLlang.semiring_provider_context(
            bounded_raw.context).owner_thread
        statuses = fill(Cint(-1), Base.Threads.nthreads())
        executing_threads = fill(0, Base.Threads.nthreads())
        Base.Threads.@threads :static for index in eachindex(statuses)
            executing_threads[index] = Base.Threads.threadid()
            if executing_threads[index] != creator
                local_output = Ref(VTI.VtSemiringValue(0, 0))
                statuses[index] = ccall(LlingLlang.SEMIRING_CALLBACKS[:zero], Cint,
                    (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}),
                    bounded_raw.context, local_output)
            end
        end
        @test any(!=(creator), executing_threads)
        @test all(index -> executing_threads[index] == creator ?
            statuses[index] == Cint(-1) :
            statuses[index] == Cint(VTI.STATUS_PROVIDER_ERROR),
            eachindex(statuses))
        owner_output = Ref(VTI.VtSemiringValue(0, 0))
        @test ccall(LlingLlang.SEMIRING_CALLBACKS[:zero], Cint,
            (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}),
            bounded_raw.context, owner_output) == Cint(VTI.STATUS_OK)
        @test release_batch(bounded_raw, [owner_output[]]) == Cint(VTI.STATUS_OK)
        close(bounded)
    end

    close(resource)
    output = Ref(VTI.VtSemiringValue(UInt64(77), UInt64(88)))
    @test ccall(LlingLlang.SEMIRING_CALLBACKS[:zero], Cint,
        (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}),
        raw.context, output) == Cint(VTI.STATUS_CLOSED)
    @test output[].word0 == 77 && output[].word1 == 88

    throwing = semiring_provider(ThrowingSemiringProvider();
        domain_id=VTI.interface_id("test.throw.sem1."))
    throwing_raw = VTI.raw_resource(throwing)
    result = Ref(VTI.VtSemiringValue(0, 0))
    @test ccall(LlingLlang.SEMIRING_CALLBACKS[:zero], Cint,
        (Ptr{Cvoid}, Ref{VTI.VtSemiringValue}),
        throwing_raw.context, result) == Cint(VTI.STATUS_PROVIDER_ERROR)
    @test LlingLlang.semiring_provider_context(throwing_raw.context).last_error ==
        "semiring provider raised an unprintable exception"
    close(throwing)
    @test length(unique([raw.context, serial_raw.context, throwing_raw.context])) == 3
    GC.gc()
    successor = semiring_provider(TropicalProvider();
        domain_id=VTI.interface_id("test.cookie.succ"))
    @test VTI.raw_resource(successor).context ∉
        (raw.context, serial_raw.context, throwing_raw.context)
    close(successor)
end

struct ExampleProvider <: AbstractWfstProvider end
LlingLlang.wfst_start(::ExampleProvider) = 0
LlingLlang.wfst_state_count(::ExampleProvider) = 2
function LlingLlang.wfst_state(::ExampleProvider, state::UInt64)
    state == 0 && return ProviderState(arcs=[ProviderArc('b', 'c', 1, 0.75)])
    state == 1 && return ProviderState(final=true, final_weight=0.125)
    ProviderState(valid=false)
end

struct ThrowingWfstProvider <: AbstractWfstProvider end
LlingLlang.wfst_start(::ThrowingWfstProvider) = 0
LlingLlang.wfst_state(::ThrowingWfstProvider, ::UInt64) =
    throw(UnprintableProviderFailure())

struct ReentrantWfstProvider <: AbstractWfstProvider
    context::Base.RefValue{Ptr{Cvoid}}
    nested_status::Base.RefValue{Cint}
end
function LlingLlang.wfst_start(provider::ReentrantWfstProvider)
    if provider.context[] != C_NULL
        count = Ref{Csize_t}(0)
        known = Ref{UInt8}(0)
        provider.nested_status[] = ccall(LlingLlang.CALLBACKS[:count], Cint,
            (Ptr{Cvoid}, Ref{Csize_t}, Ref{UInt8}),
            provider.context[], count, known)
    end
    0
end
LlingLlang.wfst_state_count(::ReentrantWfstProvider) = 1
LlingLlang.wfst_state(::ReentrantWfstProvider, ::UInt64) = ProviderState(final=true)

struct BlockingWfstProvider <: AbstractWfstProvider
    entered::Channel{Bool}
    release::Channel{Bool}
end
LlingLlang.wfst_start(::BlockingWfstProvider) = 0
LlingLlang.wfst_state_count(::BlockingWfstProvider) = 1
function LlingLlang.wfst_state(provider::BlockingWfstProvider, ::UInt64)
    put!(provider.entered, true)
    take!(provider.release)
    ProviderState(final=true)
end

@testset "WFST provider callback exceptions and closed contexts" begin
    host = provider(ThrowingWfstProvider())
    raw = LlingLlang.raw_resource(host)
    valid = Ref{UInt8}(0)
    finality = Ref{UInt8}(0)
    weight = Ref{Float64}(0)
    @test ccall(LlingLlang.CALLBACKS[:state_info], Cint,
        (Ptr{Cvoid}, UInt64, Ref{UInt8}, Ref{UInt8}, Ref{Float64}),
        raw.context, UInt64(0), valid, finality, weight) ==
        Cint(VTI.STATUS_PROVIDER_ERROR)
    @test LlingLlang.provider_context(raw.context).last_error ==
        "WFST provider raised an unprintable exception"
    close(host)
    start = Ref{UInt64}(typemax(UInt64))
    @test ccall(LlingLlang.CALLBACKS[:start], Cint,
        (Ptr{Cvoid}, Ref{UInt64}), raw.context, start) ==
        Cint(VTI.STATUS_CLOSED)
    @test start[] == typemax(UInt64)

    recursive = ReentrantWfstProvider(Ref(C_NULL), Ref(Cint(-1)))
    serial = provider(recursive)
    serial_raw = LlingLlang.raw_resource(serial)
    recursive.context[] = serial_raw.context
    @test ccall(LlingLlang.CALLBACKS[:start], Cint,
        (Ptr{Cvoid}, Ref{UInt64}), serial_raw.context, start) ==
        Cint(VTI.STATUS_OK)
    @test recursive.nested_status[] == Cint(VTI.STATUS_PROVIDER_ERROR)
    @test occursin("recursive", LlingLlang.provider_context(
        serial_raw.context).last_error)
    close(serial)

    parallel_implementation = ReentrantWfstProvider(Ref(C_NULL), Ref(Cint(-1)))
    parallel_host = provider(parallel_implementation; parallel=true)
    parallel_raw = LlingLlang.raw_resource(parallel_host)
    parallel_implementation.context[] = parallel_raw.context
    @test ccall(LlingLlang.CALLBACKS[:start], Cint,
        (Ptr{Cvoid}, Ref{UInt64}), parallel_raw.context, start) ==
        Cint(VTI.STATUS_OK)
    @test parallel_implementation.nested_status[] == Cint(VTI.STATUS_OK)
    close(parallel_host)

    blocking = BlockingWfstProvider(Channel{Bool}(1), Channel{Bool}(1))
    shared = provider(blocking)
    shared_raw = LlingLlang.raw_resource(shared)
    worker = Base.Threads.@spawn begin
        valid = Ref{UInt8}(0)
        finality = Ref{UInt8}(0)
        weight = Ref{Float64}(0)
        ccall(LlingLlang.CALLBACKS[:state_info], Cint,
            (Ptr{Cvoid}, UInt64, Ref{UInt8}, Ref{UInt8}, Ref{Float64}),
            shared_raw.context, UInt64(0), valid, finality, weight)
    end
    entered = Base.timedwait(() -> isready(blocking.entered), 5)
    @test entered == :ok
    if entered == :ok
        take!(blocking.entered)
        count = Ref{Csize_t}(0)
        known = Ref{UInt8}(0)
        @test ccall(LlingLlang.CALLBACKS[:count], Cint,
            (Ptr{Cvoid}, Ref{Csize_t}, Ref{UInt8}),
            shared_raw.context, count, known) == Cint(VTI.STATUS_PROVIDER_ERROR)
    end
    put!(blocking.release, true)
    @test fetch(worker) == Cint(VTI.STATUS_OK)
    close(shared)
    @test serial_raw.context != raw.context
    GC.gc()
    successor = provider(ExampleProvider())
    @test LlingLlang.raw_resource(successor).context ∉
        (raw.context, serial_raw.context)
    close(successor)
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

include("path_contract_properties.jl")
