@testset "native bounded symbolic transducers" begin
    machine = SymbolicTransducer(Char)
    letters = symbolic_char_range('a', 'z')
    try
        start = add_state!(machine)
        finish = add_state!(machine; accepting=true)
        set_initial!(machine, start)
        add_transition!(machine, start, finish, letters, :identity)
        add_transition!(machine, start, finish, letters, ['X', 'Y'])
        add_transition!(machine, start, finish, letters, :epsilon)
        close(letters)
        @test sort(symbolic_transduce(machine, "a")) == ["", "XY", "a"]
        @test sort(symbolic_transduce(machine, ['z'])) == ["", "XY", "z"]
        @test isempty(symbolic_transduce(machine, "A"))
        @test isempty(symbolic_transduce(machine, ""))
        @test isempty(symbolic_transduce(machine, "aa"))
        @test_throws NativeError symbolic_transduce(machine, "a";
            limits=SymbolicTransductionLimits(max_paths=0))
        @test_throws NativeError symbolic_transduce(machine, "a";
            limits=SymbolicTransductionLimits(max_heap_bytes=0))
        @test_throws ArgumentError SymbolicTransductionLimits(max_work=-1)
    finally
        close(machine)
        close(letters)
    end
    @test_throws NativeError symbolic_transduce(machine, "a")

    numeric = SymbolicTransducer(Int64; universe_min=-10, universe_max=10)
    guard = symbolic_interval_range(-10, 10, 2, 6)
    try
        first = add_state!(numeric)
        final = add_state!(numeric; accepting=true)
        set_initial!(numeric, first)
        add_transition!(numeric, first, final, guard, [4, 5])
        @test symbolic_transduce(numeric, [3]) == [[4, 5]]
        @test isempty(symbolic_transduce(numeric, [6]))
        @test_throws NativeError add_transition!(numeric, first, final,
            guard, [10])
    finally
        close(numeric)
        close(guard)
    end

    first = SymbolicTransducer(Char)
    second = SymbolicTransducer(Char)
    input_guard = symbolic_char_range('a', 'z')
    middle_guard = symbolic_char_range('b', 'b')
    try
        for transducer in (first, second)
            start = add_state!(transducer)
            finish = add_state!(transducer; accepting=true)
            set_initial!(transducer, start)
        end
        add_transition!(first, 0, 1, input_guard, ['b'])
        add_transition!(second, 0, 1, middle_guard, ['Q'])
        @test symbolic_compose_transduce(first, second, "a") == ["Q"]
        @test isempty(symbolic_compose_transduce(first, second, "A"))
        @test_throws NativeError symbolic_compose_transduce(first, second, "a";
            limits=SymbolicTransductionLimits(max_work=0))
    finally
        foreach(close, (first, second, input_guard, middle_guard))
    end
end
