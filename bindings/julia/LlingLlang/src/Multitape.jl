"""Mutable, checked Julia construction of an immutable native multi-tape graph."""
mutable struct MultiTapeBuilder{L,W<:AbstractScalarWeight}
    num_tapes::Int
    states::Vector{Union{Nothing,W}}
    start::Union{Nothing,UInt64}
    arcs::Vector{Tuple{UInt64,UInt64,Vector{UInt64},Vector{UInt8},Float64}}
    symbols::Vector{Union{Nothing,SymbolTable}}
    budget::BudgetV2
    closed::Bool
end

function MultiTapeBuilder{L,W}(num_tapes::Integer;
    symbols=nothing,
    budget::BudgetV2=BudgetV2(max_states=10_000, max_arcs=100_000,
        max_bytes=64_000_000, max_work=1_000_000)) where {L,W<:AbstractScalarWeight}
    unit_domain(L)
    weight_domain(W)
    0 < num_tapes <= typemax(Int) || throw(ArgumentError(
        "multi-tape arity must be a positive native size"))
    (budget.header.flags & (BUDGET_STATES | BUDGET_ARCS |
        BUDGET_BYTES | BUDGET_WORK)) ==
        (BUDGET_STATES | BUDGET_ARCS | BUDGET_BYTES | BUDGET_WORK) ||
        throw(ArgumentError("multi-tape construction requires four finite budget axes"))
    tables = Vector{Union{Nothing,SymbolTable}}(undef, Int(num_tapes))
    if isnothing(symbols)
        fill!(tables, nothing)
    else
        length(symbols) == num_tapes || throw(ArgumentError(
            "symbol table count must equal the tape count"))
        for index in eachindex(tables)
            tables[index] = checked_symbols(L, symbols[index])
        end
    end
    MultiTapeBuilder{L,W}(Int(num_tapes), Union{Nothing,W}[], nothing,
        Tuple{UInt64,UInt64,Vector{UInt64},Vector{UInt8},Float64}[],
        tables, budget, false)
end

function mt_open(builder::MultiTapeBuilder)
    builder.closed && throw(NativeError(STATUS_CLOSED, :multitape_builder,
        "multi-tape builder is closed"))
    builder
end

function mt_state(builder::MultiTapeBuilder, state::Integer)
    0 <= state < length(builder.states) || throw(ArgumentError(
        "state is outside the multi-tape builder's state range"))
    UInt64(state)
end

"""Append a state and return its zero-based native identifier."""
function add_state!(builder::MultiTapeBuilder)
    mt_open(builder)
    UInt64(length(builder.states)) < min(builder.budget.max_states,
        UInt64(typemax(UInt32))) ||
        throw(ArgumentError("multi-tape state bound exhausted"))
    state = UInt64(length(builder.states))
    push!(builder.states, nothing)
    state
end

"""Set the start state of a multi-tape builder."""
function set_start!(builder::MultiTapeBuilder, state::Integer)
    mt_open(builder)
    builder.start = mt_state(builder, state)
    builder
end

"""Set a state's final weight in the selected semiring."""
function set_final!(builder::MultiTapeBuilder{L,W}, state::Integer,
    weight=one(W)) where {L,W}
    mt_open(builder)
    index = Int(mt_state(builder, state)) + 1
    builder.states[index] = typed_weight(W, weight)
    builder
end

"""Remove a state's final status."""
function clear_final!(builder::MultiTapeBuilder, state::Integer)
    mt_open(builder)
    builder.states[Int(mt_state(builder, state)) + 1] = nothing
    builder
end

"""Append one arc with exactly one label or epsilon per tape."""
function add_arc!(builder::MultiTapeBuilder{L,W}, from::Integer,
    labels::AbstractVector, target::Integer, weight=one(W)) where {L,W}
    mt_open(builder)
    source_id = mt_state(builder, from)
    target_id = mt_state(builder, target)
    length(labels) == builder.num_tapes || throw(ArgumentError(
        "arc label count must equal the tape count"))
    UInt64(length(builder.arcs)) < builder.budget.max_arcs ||
        throw(ArgumentError("multi-tape arc bound exhausted"))
    values = Vector{UInt64}(undef, builder.num_tapes)
    present = Vector{UInt8}(undef, builder.num_tapes)
    for index in eachindex(values)
        values[index], present[index] = wire_label(L, labels[index],
            builder.symbols[index])
    end
    typed = typed_weight(W, weight)
    push!(builder.arcs, (source_id, target_id, values, present,
        raw_weight(typed)))
    builder
end

"""Release an unbuilt multi-tape builder."""
function close!(builder::MultiTapeBuilder)
    builder.closed && return nothing
    empty!(builder.states)
    empty!(builder.arcs)
    builder.closed = true
    nothing
end
Base.close(builder::MultiTapeBuilder) = close!(builder)
Base.isopen(builder::MultiTapeBuilder) = !builder.closed

"""Immutable native graph snapshot with a runtime tape count."""
mutable struct MultiTapeWfst{L,W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    closed::Bool
    num_tapes::Int
    symbols::Vector{Union{Nothing,SymbolTable}}
end

function close!(graph::MultiTapeWfst)
    graph.closed && return nothing
    ccall(native(:lling_multitape_free), Cvoid, (Ptr{Cvoid},), graph.handle)
    graph.handle = C_NULL
    graph.closed = true
    nothing
end
Base.close(graph::MultiTapeWfst) = close!(graph)
Base.isopen(graph::MultiTapeWfst) = !graph.closed

function mt_handle(graph::MultiTapeWfst)
    isopen(graph) || throw(NativeError(STATUS_CLOSED, :multitape,
        "multi-tape graph is closed"))
    graph.handle
end

"""Capture and validate the builder as an immutable native graph."""
function build!(builder::MultiTapeBuilder{L,W}) where {L,W}
    mt_open(builder)
    nstates = length(builder.states)
    narcs = length(builder.arcs)
    nstates == 0 || !isnothing(builder.start) || throw(ArgumentError(
        "a nonempty multi-tape graph needs a start state"))
    narcs <= typemax(Int) ÷ builder.num_tapes || throw(ArgumentError(
        "flat multi-tape label count overflows native size"))
    final_flags = UInt8[isnothing(weight) ? 0 : 1 for weight in builder.states]
    final_weights = Float64[isnothing(weight) ? 0.0 : raw_weight(weight)
        for weight in builder.states]
    arc_from = UInt64[arc[1] for arc in builder.arcs]
    arc_to = UInt64[arc[2] for arc in builder.arcs]
    arc_weights = Float64[arc[5] for arc in builder.arcs]
    labels = Vector{UInt64}(undef, narcs * builder.num_tapes)
    present = Vector{UInt8}(undef, narcs * builder.num_tapes)
    for (index, arc) in enumerate(builder.arcs)
        destination = ((index - 1) * builder.num_tapes + 1):(index * builder.num_tapes)
        labels[destination] = arc[3]
        present[destination] = arc[4]
    end
    output = Ref{Ptr{Cvoid}}(C_NULL)
    start = isnothing(builder.start) ? UInt64(typemax(UInt32)) : builder.start
    frozen = Union{Nothing,SymbolTable}[frozen_copy(table)
        for table in builder.symbols]
    GC.@preserve final_flags final_weights arc_from arc_to arc_weights labels present begin
        checked(ccall(native(:lling_multitape_new), UInt32,
            (UInt32, UInt32, UInt64, UInt64, UInt64,
                Ptr{UInt8}, Ptr{Float64}, Ptr{UInt64}, Ptr{UInt64},
                Ptr{Float64}, Ptr{UInt64}, Ptr{UInt8}, UInt64,
                Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
            UInt32(unit_domain(L)), UInt32(weight_domain(W)),
            UInt64(builder.num_tapes), UInt64(nstates), start,
            final_flags, final_weights, arc_from, arc_to, arc_weights,
            labels, present, UInt64(narcs), Ref(builder.budget), output),
            :multitape_new)
    end
    graph = MultiTapeWfst{L,W}(output[], false, builder.num_tapes, frozen)
    finalizer(finalize_close, graph)
    close!(builder)
    graph
end

"""One state snapshot, including final weight and outgoing arc count."""
struct MultiTapeState{W<:AbstractScalarWeight}
    id::UInt64
    final_weight::Union{Nothing,W}
    arc_count::Int
end

"""One native arc decoded from a tape-sized caller buffer."""
struct MultiTapeArc{L,W<:AbstractScalarWeight}
    from::UInt64
    to::UInt64
    labels::Vector{Union{Nothing,L}}
    weight::W
end

"""Return tape, state, and arc counts plus the optional start state."""
function multitape_info(graph::MultiTapeWfst)
    tapes = Ref{Csize_t}(0)
    states = Ref{Csize_t}(0)
    arcs = Ref{Csize_t}(0)
    start = Ref{UInt64}(0)
    checked(ccall(native(:lling_multitape_info), UInt32,
        (Ptr{Cvoid}, Ref{Csize_t}, Ref{Csize_t}, Ref{Csize_t}, Ref{UInt64}),
        mt_handle(graph), tapes, states, arcs, start), :multitape_info)
    (num_tapes=Int(tapes[]), num_states=Int(states[]),
        num_arcs=Int(arcs[]),
        start=start[] == UInt64(typemax(UInt32)) ? nothing : start[])
end

"""Read one state from the immutable native graph."""
function multitape_state(graph::MultiTapeWfst{L,W}, state::Integer) where {L,W}
    state >= 0 || throw(ArgumentError("state cannot be negative"))
    final = Ref{UInt8}(0)
    weight = Ref{Float64}(0)
    count = Ref{Csize_t}(0)
    checked(ccall(native(:lling_multitape_state_info), UInt32,
        (Ptr{Cvoid}, UInt64, Ref{UInt8}, Ref{Float64}, Ref{Csize_t}),
        mt_handle(graph), UInt64(state), final, weight, count),
        :multitape_state_info)
    MultiTapeState{W}(UInt64(state),
        final[] == 1 ? decode_weight(W, weight[]) : nothing, Int(count[]))
end

function multitape_arc(graph::MultiTapeWfst{L,W}, state::Integer,
    index::Integer) where {L,W}
    state >= 0 && index >= 0 || throw(ArgumentError(
        "state and arc index cannot be negative"))
    to = Ref{UInt64}(0)
    weight = Ref{Float64}(0)
    labels = Vector{UInt64}(undef, graph.num_tapes)
    present = Vector{UInt8}(undef, graph.num_tapes)
    GC.@preserve labels present begin
        checked(ccall(native(:lling_multitape_arc), UInt32,
            (Ptr{Cvoid}, UInt64, Csize_t, Ref{UInt64}, Ref{Float64},
                Ptr{UInt64}, Ptr{UInt8}, Csize_t),
            mt_handle(graph), UInt64(state), Csize_t(index), to, weight,
            labels, present, graph.num_tapes), :multitape_arc)
    end
    decoded = Union{Nothing,L}[present[tape] == 1 ?
        decode_label(L, labels[tape]) : nothing for tape in eachindex(labels)]
    MultiTapeArc{L,W}(UInt64(state), to[], decoded,
        decode_weight(W, weight[]))
end

"""Lazy native arc traversal for one state; each step reads one arc."""
struct MultiTapeArcIterator{G}
    graph::G
    state::UInt64
    count::Int
end

function Base.iterate(iterator::MultiTapeArcIterator, index::Int=0)
    index >= iterator.count && return nothing
    (multitape_arc(iterator.graph, iterator.state, index), index + 1)
end
Base.length(iterator::MultiTapeArcIterator) = iterator.count
Base.IteratorSize(::Type{<:MultiTapeArcIterator}) = Base.HasLength()

function multitape_arcs(graph::MultiTapeWfst, state::Integer)
    info = multitape_state(graph, state)
    MultiTapeArcIterator(graph, info.id, info.arc_count)
end

function adopt_multitape(handle::Ptr{Cvoid}, ::Type{L}, ::Type{W},
    num_tapes::Int, symbols) where {L,W<:AbstractScalarWeight}
    try
        frozen = Union{Nothing,SymbolTable}[frozen_copy(table) for table in symbols]
        graph = MultiTapeWfst{L,W}(handle, false, num_tapes, frozen)
        finalizer(finalize_close, graph)
        graph
    catch
        ccall(native(:lling_multitape_free), Cvoid, (Ptr{Cvoid},), handle)
        rethrow()
    end
end

const DEFAULT_MULTITAPE_BUDGET = BudgetV2(max_states=10_000,
    max_arcs=100_000, max_bytes=64_000_000, max_work=1_000_000)

"""Return the first accepted native path weight, or `nothing`."""
function multitape_transduce(graph::MultiTapeWfst{L,W}, input::AbstractVector;
    budget::BudgetV2=DEFAULT_MULTITAPE_BUDGET) where {L,W}
    mt_handle(graph)
    length(input) <= typemax(Int) ÷ graph.num_tapes || throw(ArgumentError(
        "flat multi-tape input size overflows native size"))
    labels = Vector{UInt64}(undef, length(input) * graph.num_tapes)
    present = Vector{UInt8}(undef, length(labels))
    for (step, tuple) in enumerate(input)
        length(tuple) == graph.num_tapes || throw(ArgumentError(
            "input tuple arity must equal the tape count"))
        for tape in 1:graph.num_tapes
            index = (step - 1) * graph.num_tapes + tape
            labels[index], present[index] = wire_label(L, tuple[tape],
                graph.symbols[tape])
        end
    end
    accepted = Ref{UInt8}(0)
    weight = Ref{Float64}(0)
    GC.@preserve labels present begin
        checked(ccall(native(:lling_multitape_transduce), UInt32,
            (Ptr{Cvoid}, Ptr{UInt64}, Ptr{UInt8}, UInt64,
                Ref{BudgetV2}, Ref{UInt8}, Ref{Float64}),
            graph.handle, labels, present, UInt64(length(input)),
            Ref(budget), accepted, weight), :multitape_transduce)
    end
    accepted[] == 0 ? nothing : decode_weight(W, weight[])
end

"""Whether the native multi-tape graph accepts a sequence of label tuples."""
multitape_accepts(graph::MultiTapeWfst, input::AbstractVector; kwargs...) =
    !isnothing(multitape_transduce(graph, input; kwargs...))

"""Project one Julia one-based tape to an ordinary scalar WFST."""
function multitape_project(graph::MultiTapeWfst{L,W}, tape::Integer;
    budget::BudgetV2=DEFAULT_MULTITAPE_BUDGET) where {L,W}
    mt_handle(graph)
    1 <= tape <= graph.num_tapes || throw(ArgumentError(
        "tape index must be in 1:$(graph.num_tapes)"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_multitape_project), UInt32,
        (Ptr{Cvoid}, UInt64, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
        graph.handle, UInt64(tape - 1), Ref(budget), output),
        :multitape_project)
    table = graph.symbols[Int(tape)]
    adopt_native_wfst(output[], L, W;
        input_symbols=table, output_symbols=table)
end

"""Project an ordered nonempty selection of Julia one-based tapes."""
function multitape_project_tapes(graph::MultiTapeWfst{L,W}, tapes::AbstractVector{<:Integer};
    budget::BudgetV2=DEFAULT_MULTITAPE_BUDGET) where {L,W}
    mt_handle(graph)
    isempty(tapes) && throw(ArgumentError("tape selection cannot be empty"))
    raw = Vector{UInt64}(undef, length(tapes))
    for (index, tape) in enumerate(tapes)
        1 <= tape <= graph.num_tapes || throw(ArgumentError(
            "tape index must be in 1:$(graph.num_tapes)"))
        raw[index] = UInt64(tape - 1)
    end
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve raw begin
        checked(ccall(native(:lling_multitape_project_tapes), UInt32,
            (Ptr{Cvoid}, Ptr{UInt64}, UInt64, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
            graph.handle, raw, UInt64(length(raw)), Ref(budget), output),
            :multitape_project_tapes)
    end
    adopt_multitape(output[], L, W, length(raw),
        [graph.symbols[Int(tape)] for tape in tapes])
end

"""Return the exact maximum pairwise delay, or `nothing` if unbounded."""
function multitape_max_delay(graph::MultiTapeWfst;
    max_work::Integer=1_000_000)
    mt_handle(graph)
    0 <= max_work <= typemax(UInt64) || throw(ArgumentError(
        "max_work must fit UInt64"))
    bounded = Ref{UInt8}(0)
    delay = Ref{UInt64}(0)
    checked(ccall(native(:lling_multitape_max_delay), UInt32,
        (Ptr{Cvoid}, UInt64, Ref{UInt8}, Ref{UInt64}),
        graph.handle, UInt64(max_work), bounded, delay),
        :multitape_max_delay)
    bounded[] == 0 ? nothing : delay[]
end

"""Whether every reachable path stays within a finite tape delay."""
function multitape_has_bounded_delay(graph::MultiTapeWfst, max_delay::Integer;
    max_work::Integer=1_000_000)
    max_delay >= 0 || throw(ArgumentError("max_delay cannot be negative"))
    maximum = multitape_max_delay(graph; max_work)
    !isnothing(maximum) && maximum <= max_delay
end

"""Materialize the native bounded-delay subgraph under four finite limits."""
function multitape_synchronize(graph::MultiTapeWfst{L,W}, max_delay::Integer;
    allow_epsilon::Bool=true,
    budget::BudgetV2=DEFAULT_MULTITAPE_BUDGET) where {L,W}
    mt_handle(graph)
    0 <= max_delay <= typemax(UInt64) || throw(ArgumentError(
        "max_delay must fit UInt64"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_multitape_synchronize), UInt32,
        (Ptr{Cvoid}, UInt64, UInt8, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
        graph.handle, UInt64(max_delay), UInt8(allow_epsilon),
        Ref(budget), output), :multitape_synchronize)
    adopt_multitape(output[], L, W, graph.num_tapes, graph.symbols)
end
