using BenchmarkTools
using LlingLlang
import VinaryTreeInterop as VTI

function sample_graph()
    builder = WfstBuilder(size_hint=2)
    first = add_state!(builder)
    second = add_state!(builder)
    set_start!(builder, first)
    set_final!(builder, second)
    add_arc!(builder, first, 'a', 'b', second)
    build!(builder)
end

left = sample_graph()
right = sample_graph()
suite = BenchmarkGroup()
suite["compose-capture"] = @benchmarkable begin
    product = compose($left, $right)
    close(product)
end
suite["state-expansion"] = @benchmarkable VTI.arcs($left, 0)
display(run(suite))
close(left)
close(right)
