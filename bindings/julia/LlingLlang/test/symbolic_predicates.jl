@testset "native symbolic Boolean predicates" begin
    latin = symbolic_char_range('a', 'z')
    vowels = symbolic_char_range('a', 'e')
    overlap = latin & vowels
    outside = !overlap
    try
        @test 'a' in overlap
        @test !('f' in overlap)
        @test 'f' in outside
        @test symbolic_satisfiable(overlap)
        @test symbolic_witness(overlap) in overlap
        @test symbolic_implies(vowels, latin)
        @test symbolic_overlaps(latin, vowels)
        @test !symbolic_overlaps(outside, overlap)
        universal = symbolic_true(Char)
        retained = latin & universal
        try
            @test symbolic_equivalent(retained, latin)
        finally
            close(universal)
            close(retained)
        end
    finally
        foreach(close, (latin, vowels, overlap, outside))
    end

    first = symbolic_interval_range(-20, 20, -4, 8)
    second = symbolic_interval_range(-20, 20, 5, 15)
    union = first | second
    empty = symbolic_false(Int64; universe_min=-20, universe_max=20)
    different = symbolic_interval_range(0, 20, 1, 5)
    try
        @test all(value in union for value in -4:14)
        @test !(-5 in union)
        @test !(15 in union)
        @test symbolic_witness(union) in union
        @test symbolic_witness(empty) === nothing
        @test !symbolic_satisfiable(empty)
        @test_throws ArgumentError first & different
        @test_throws ArgumentError symbolic_char_range('z', 'a')
        @test_throws ArgumentError symbolic_interval_range(0, 10, 9, 11)
    finally
        foreach(close, (first, second, union, empty, different))
    end
    @test !isopen(first)
    @test_throws NativeError symbolic_witness(first)

    below_gap = symbolic_char_range('\0', '\U0000d7ff')
    upper_scalars = !below_gap
    try
        @test symbolic_witness(upper_scalars) == '\U0000e000'
        @test '\U0010ffff' in upper_scalars
        @test !('\U0000d7ff' in upper_scalars)
    finally
        close(below_gap)
        close(upper_scalars)
    end
end
