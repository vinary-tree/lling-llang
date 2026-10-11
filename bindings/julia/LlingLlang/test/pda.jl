const PDA_LABEL_CASES = [(UInt8, UInt8(0xfe)), (Char, 'λ'),
    (UInt64, typemax(UInt64))]
const PDA_WEIGHT_CASES = [TropicalWeight, LogWeight, ProbabilityWeight,
    ArcticWeight, SignedTropicalWeight, CountWeight, BooleanWeight]

@testset "native weighted PDA domain and lifecycle parity" begin
    for (Label, terminal) in PDA_LABEL_CASES
        for Weight in PDA_WEIGHT_CASES
            builder = PdaBuilder{Label,Weight}()
            start = add_state!(builder)
            target = add_state!(builder)
            set_start!(builder, start)
            set_final!(builder, target, one(Weight))
            add_transition!(builder, start, terminal, 0, target,
                keep_stack(), one(Weight))
            pda = build!(builder)
            @test !isopen(builder)
            @test pda_domains(pda) == (
                LlingLlang.unit_domain(Label), LlingLlang.weight_domain(Weight))
            session = pda_session(pda; max_stack_depth=32)
            close(pda)
            @test !pda_acceptance(session).accepted
            choices = pda_legal_next(session; max_work=100, page_size=1)
            @test length(choices) == 1
            @test choices[1].label == terminal
            @test choices[1].weight == one(Weight)
            @test advance!(session, terminal; max_work=100)
            @test pda_acceptance(session).accepted
            @test pda_acceptance(session).weight == one(Weight)
            @test pda_info(session) == (state=target, stack_depth=1)
            @test pda_stack(session; page_size=1) == UInt32[0]
            close(session)
            @test !isopen(session)
        end
    end
end

@testset "native PDA stack actions, paging, and deep iterative decode" begin
    builder = PdaBuilder{Char,TropicalWeight}()
    start = add_state!(builder)
    nested = add_state!(builder)
    done = add_state!(builder)
    set_start!(builder, start)
    set_final!(builder, done)
    marker = add_stack_symbol!(builder)
    add_transition!(builder, start, '(', 0, nested,
        push_stack([0, marker]))
    add_transition!(builder, nested, '(', marker, nested,
        push_stack([marker, marker]))
    add_transition!(builder, nested, ')', marker, nested, pop_stack())
    add_transition!(builder, nested, nothing, 0, done, keep_stack())
    pda = build!(builder)
    session = pda_session(pda; max_stack_depth=2_000)
    @test only(pda_legal_next(session)).label == '('
    @test advance!(session, '(')
    frontier = pda_frontier(session; page_size=1)
    @test length(frontier) == 2
    @test Set(choice.label for choice in frontier) == Set(['(', ')'])
    partial = pda_frontier(session; page_size=1)
    @test iterate(partial) !== nothing
    @test length(partial) == 1
    @test length(collect(partial)) == 1
    old_frontier = pda_frontier(session; page_size=1)
    @test advance!(session, '(')
    @test_throws ArgumentError collect(old_frontier)
    for _ in 1:1_022
        @test advance!(session, '('; max_work=100)
    end
    @test pda_info(session).stack_depth == 1_025
    @test pda_stack(session; page_size=17)[end] == marker
    shallow = pda_session(pda; max_stack_depth=1)
    @test_throws NativeError advance!(shallow, '('; max_work=100)
    @test pda_info(shallow).stack_depth == 1
    close(shallow)
    for _ in 1:1_024
        @test advance!(session, ')'; max_work=100)
    end
    @test pda_info(session).stack_depth == 1
    @test pda_acceptance(session; max_work=100).accepted
    @test pda_stack(session) == UInt32[0]
    close(session)
    close(pda)
end

@testset "native PDA count-weight epsilon diamond" begin
    builder = PdaBuilder{Char,CountWeight}()
    start = add_state!(builder)
    left = add_state!(builder)
    right = add_state!(builder)
    joined = add_state!(builder)
    set_start!(builder, start)
    set_final!(builder, joined, CountWeight(1))
    for middle in (left, right)
        add_transition!(builder, start, nothing, 0, middle,
            keep_stack(), CountWeight(1))
        add_transition!(builder, middle, nothing, 0, joined,
            keep_stack(), CountWeight(1))
    end
    add_transition!(builder, joined, 'λ', 0, joined,
        keep_stack(), CountWeight(3))
    pda = build!(builder)
    session = pda_session(pda)
    @test pda_acceptance(session).weight == CountWeight(2)
    @test only(pda_legal_next(session)).weight == CountWeight(6)
    close(session)
    close(pda)
end

@testset "native PDA rejects invalid domains and divergent epsilon work" begin
    @test_throws ArgumentError push_stack([-1])
    @test_throws ArgumentError PdaStackAction(99)
    builder = PdaBuilder{Char,TropicalWeight}()
    state = add_state!(builder)
    set_start!(builder, state)
    set_final!(builder, state)
    @test_throws NativeError set_initial_stack!(builder, 99)
    @test_throws ArgumentError add_transition!(builder, state, 0xd800, 0,
        state, keep_stack())
    add_transition!(builder, state, nothing, 0, state, pop_stack())
    @test_throws NativeError build!(builder)
    @test isopen(builder)
    close(builder)

    counting = PdaBuilder{UInt8,CountWeight}()
    q = add_state!(counting)
    set_start!(counting, q)
    set_final!(counting, q, CountWeight(1))
    add_transition!(counting, q, nothing, 0, q, keep_stack(), CountWeight(1))
    pda = build!(counting)
    session = pda_session(pda)
    @test_throws NativeError pda_acceptance(session; max_work=20)
    @test_throws NativeError pda_frontier(session; max_work=20)
    close(session)
    close(pda)
end
