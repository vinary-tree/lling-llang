@testset "native symbolic finite automata" begin
    automaton = SymbolicAutomaton(Char)
    guard = symbolic_char_range('a', 'z')
    try
        initial = add_state!(automaton)
        final = add_state!(automaton; accepting=true)
        @test initial == 0
        @test final == 1
        @test_throws NativeError set_initial!(automaton, 2)
        set_initial!(automaton, initial)
        @test isempty(automaton)
        add_transition!(automaton, initial, final, guard)
        close(guard)
        @test !isempty(automaton)
        @test symbolic_accepts(automaton, "a")
        @test symbolic_accepts(automaton, ['z'])
        @test !symbolic_accepts(automaton, "A")
        @test !symbolic_accepts(automaton, "")
        @test !symbolic_accepts(automaton, "aa")
        invalid_edge_guard = symbolic_char_range('a', 'b')
        try
            @test_throws NativeError add_transition!(automaton, 0, 2,
                invalid_edge_guard)
        finally
            close(invalid_edge_guard)
        end
    finally
        close(automaton)
        close(guard)
    end
    @test !isopen(automaton)
    @test_throws NativeError isempty(automaton)

    numeric = SymbolicAutomaton(Int64; universe_min=-10, universe_max=10)
    range = symbolic_interval_range(-10, 10, 1, 5)
    mismatch = symbolic_interval_range(0, 10, 1, 5)
    try
        start = add_state!(numeric)
        finish = add_state!(numeric; accepting=true)
        set_initial!(numeric, start)
        @test_throws ArgumentError add_transition!(numeric, start, finish, mismatch)
        add_transition!(numeric, start, finish, range)
        @test symbolic_accepts(numeric, [1])
        @test symbolic_accepts(numeric, [4])
        @test !symbolic_accepts(numeric, [5])
        @test !symbolic_accepts(numeric, Int64[])
        @test !symbolic_accepts(numeric, [1, 2])
    finally
        foreach(close, (numeric, range, mismatch))
    end
end
