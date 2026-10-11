"""Native PDA acceptance by final state, empty stack, or either condition."""
@enum PdaAcceptMode::UInt32 begin
    PDA_FINAL_STATE = 0
    PDA_EMPTY_STACK = 1
    PDA_EITHER = 2
end

"""One validated stack action; a replacement word is pushed left to right."""
struct PdaStackAction
    kind::UInt32
    symbols::Vector{UInt32}
    function PdaStackAction(kind::Integer, symbols::AbstractVector{<:Integer}=UInt32[])
        kind in 0:3 || throw(ArgumentError("unknown PDA stack action"))
        kind in (0, 3) && !isempty(symbols) &&
            throw(ArgumentError("pop and noop actions cannot carry symbols"))
        values = UInt32[]
        sizehint!(values, length(symbols))
        for symbol in symbols
            0 <= symbol <= typemax(UInt32) ||
                throw(ArgumentError("stack symbol is outside UInt32"))
            push!(values, UInt32(symbol))
        end
        new(UInt32(kind), values)
    end
end

"""Pop the matched stack symbol."""
pop_stack() = PdaStackAction(0)
"""Pop the matched symbol, then push `symbols` in the given order."""
push_stack(symbols::AbstractVector{<:Integer}) = PdaStackAction(1, symbols)
"""Replace the matched symbol with `symbols` in the given order."""
replace_stack(symbols::AbstractVector{<:Integer}) = PdaStackAction(2, symbols)
"""Retain the matched stack symbol."""
keep_stack() = PdaStackAction(3)

"""Mutable native weighted PDA builder over an explicit label and semiring domain."""
mutable struct PdaBuilder{L,W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    closed::Bool
end

function PdaBuilder{L,W}(; accept_mode::PdaAcceptMode=PDA_FINAL_STATE) where
    {L,W<:AbstractScalarWeight}
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_pda_builder_open), UInt32,
        (UInt32, UInt32, UInt32, Ref{Ptr{Cvoid}}),
        UInt32(unit_domain(L)), UInt32(weight_domain(W)), UInt32(accept_mode), output),
        :pda_builder_open)
    builder = PdaBuilder{L,W}(output[], false)
    finalizer(finalize_close, builder)
    builder
end
PdaBuilder(; kwargs...) = PdaBuilder{Char,TropicalWeight}(; kwargs...)

function pda_handle(builder::PdaBuilder)
    builder.closed && throw(NativeError(STATUS_CLOSED, :pda_builder, "builder is closed"))
    builder.handle
end

"""Append a PDA control state and return its zero-based identifier."""
function add_state!(builder::PdaBuilder)
    output = Ref{UInt32}(0)
    checked(ccall(native(:lling_pda_builder_add_state), UInt32,
        (Ptr{Cvoid}, Ref{UInt32}), pda_handle(builder), output), :pda_add_state)
    output[]
end

"""Set the PDA start state to a previously allocated control state."""
function set_start!(builder::PdaBuilder, state::Integer)
    checked(ccall(native(:lling_pda_builder_set_start), UInt32,
        (Ptr{Cvoid}, UInt32), pda_handle(builder), state), :pda_set_start)
    builder
end

"""Allocate a stack symbol; zero is the initial bottom marker."""
function add_stack_symbol!(builder::PdaBuilder)
    output = Ref{UInt32}(0)
    checked(ccall(native(:lling_pda_builder_add_stack_symbol), UInt32,
        (Ptr{Cvoid}, Ref{UInt32}), pda_handle(builder), output), :pda_add_stack_symbol)
    output[]
end

"""Replace the initial stack symbol with a previously allocated symbol."""
function set_initial_stack!(builder::PdaBuilder, symbol::Integer)
    checked(ccall(native(:lling_pda_builder_set_initial_stack), UInt32,
        (Ptr{Cvoid}, UInt32), pda_handle(builder), symbol), :pda_set_initial_stack)
    builder
end

"""Mark a PDA control state final with a weight in its native semiring."""
function set_final!(builder::PdaBuilder{L,W}, state::Integer,
    weight=one(W)) where {L,W}
    value = typed_weight(W, weight)
    checked(ccall(native(:lling_pda_builder_set_final), UInt32,
        (Ptr{Cvoid}, UInt32, Float64), pda_handle(builder), state,
        raw_weight(value)), :pda_set_final)
    builder
end

"""Add a native transition. `nothing` is epsilon input; `stack_top` must exist.

The native builder copies `action.symbols`. A push or replacement pops the
matched top first, then pushes the supplied sequence with its last symbol on
top. The default weight is the semiring multiplicative identity.
"""
function add_transition!(builder::PdaBuilder{L,W}, from::Integer, input,
    stack_top::Integer, to::Integer, action::PdaStackAction,
    weight=one(W)) where {L,W}
    label, has_input = wire_label(L, input, nothing)
    value = typed_weight(W, weight)
    symbols = action.symbols
    status = GC.@preserve symbols ccall(native(:lling_pda_builder_add_transition), UInt32,
        (Ptr{Cvoid}, UInt32, UInt64, UInt8, UInt32, UInt32, UInt32,
            Ptr{UInt32}, Csize_t, Float64),
        pda_handle(builder), from, label, has_input, stack_top, to, action.kind,
        isempty(symbols) ? C_NULL : pointer(symbols), length(symbols),
        raw_weight(value))
    checked(status, :pda_add_transition)
    builder
end

"""Immutable compiled native weighted PDA. Sessions retain it independently."""
mutable struct Pda{L,W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    closed::Bool
end

function pda_handle(pda::Pda)
    pda.closed && throw(NativeError(STATUS_CLOSED, :pda, "PDA is closed"))
    pda.handle
end

"""Validate and freeze a builder. Failed validation leaves it open for repair."""
function build!(builder::PdaBuilder{L,W}) where {L,W}
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_pda_builder_build), UInt32,
        (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), pda_handle(builder), output), :pda_build)
    pda = Pda{L,W}(output[], false)
    finalizer(finalize_close, pda)
    close!(builder)
    pda
end

"""Return the native label and weight domains of a compiled PDA."""
function pda_domains(pda::Pda)
    unit = Ref{UInt32}(0)
    weight = Ref{UInt32}(0)
    checked(ccall(native(:lling_pda_domains), UInt32,
        (Ptr{Cvoid}, Ref{UInt32}, Ref{UInt32}), pda_handle(pda), unit, weight),
        :pda_domains)
    (VTI.UnitDomain(unit[]), VTI.WeightDomain(weight[]))
end

function close!(builder::PdaBuilder)
    builder.closed && return nothing
    ccall(native(:lling_pda_builder_free), Cvoid, (Ptr{Cvoid},), builder.handle)
    builder.handle = C_NULL
    builder.closed = true
    nothing
end

function close!(pda::Pda)
    pda.closed && return nothing
    ccall(native(:lling_pda_free), Cvoid, (Ptr{Cvoid},), pda.handle)
    pda.handle = C_NULL
    pda.closed = true
    nothing
end

Base.close(value::Union{PdaBuilder,Pda}) = close!(value)
Base.isopen(value::Union{PdaBuilder,Pda}) = !value.closed

"""One owned incremental native PDA configuration."""
mutable struct PdaSession{L,W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    closed::Bool
    generation::UInt64
end

function pda_handle(session::PdaSession)
    session.closed && throw(NativeError(STATUS_CLOSED, :pda_session, "session is closed"))
    session.handle
end

"""Open bounded incremental decoding. Native ownership survives `close!(pda)`."""
function pda_session(pda::Pda{L,W}; max_stack_depth::Integer=4096) where {L,W}
    max_stack_depth > 0 || throw(ArgumentError("max_stack_depth must be positive"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_pda_session_open), UInt32,
        (Ptr{Cvoid}, Csize_t, Ref{Ptr{Cvoid}}), pda_handle(pda),
        max_stack_depth, output), :pda_session_open)
    session = PdaSession{L,W}(output[], false, 0)
    finalizer(finalize_close, session)
    session
end

function close!(session::PdaSession)
    session.closed && return nothing
    ccall(native(:lling_pda_session_free), Cvoid, (Ptr{Cvoid},), session.handle)
    session.handle = C_NULL
    session.closed = true
    session.generation += 1
    nothing
end

Base.close(session::PdaSession) = close!(session)
Base.isopen(session::PdaSession) = !session.closed

struct RawPdaChoice
    label::UInt64
    weight::Float64
end

"""Owned legal terminal with its native semiring aggregate."""
struct PdaChoice{L,W<:AbstractScalarWeight}
    label::L
    weight::W
end

pda_label(::Type{Char}, value::UInt64) = Char(UInt32(value))
pda_label(::Type{UInt8}, value::UInt64) = UInt8(value)
pda_label(::Type{UInt64}, value::UInt64) = value

"""One generation-checked, paged weighted legal frontier."""
mutable struct PdaFrontier{L,W<:AbstractScalarWeight}
    session::PdaSession{L,W}
    generation::UInt64
    total::Int
    page_size::Int
    pending::Vector{PdaChoice{L,W}}
    offset::Int
    yielded::Int
end

Base.IteratorSize(::Type{<:PdaFrontier}) = Base.HasLength()
Base.length(frontier::PdaFrontier) = frontier.total - frontier.yielded
Base.eltype(::Type{PdaFrontier{L,W}}) where {L,W} = PdaChoice{L,W}

function frontier_page!(frontier::PdaFrontier{L,W}) where {L,W}
    session = frontier.session
    frontier.generation == session.generation ||
        throw(ArgumentError("PDA frontier was invalidated by a later query or advance"))
    size = frontier.page_size
    raw = Vector{RawPdaChoice}(undef, size)
    written = Ref{Csize_t}(0)
    status = GC.@preserve raw ccall(native(:lling_pda_session_frontier_next), UInt32,
        (Ptr{Cvoid}, Ptr{RawPdaChoice}, Csize_t, Ref{Csize_t}),
        pda_handle(session), pointer(raw), size, written)
    checked(status, :pda_frontier_next)
    frontier.pending = [PdaChoice{L,W}(pda_label(L, item.label),
        decode_weight(W, item.weight)) for item in @view(raw[1:Int(written[])])]
    frontier.offset = 1
    frontier.pending
end

function Base.iterate(frontier::PdaFrontier, state=nothing)
    frontier.generation == frontier.session.generation ||
        throw(ArgumentError("PDA frontier was invalidated by a later query or advance"))
    if frontier.offset > length(frontier.pending)
        isempty(frontier_page!(frontier)) && return nothing
    end
    value = frontier.pending[frontier.offset]
    frontier.offset += 1
    frontier.yielded += 1
    (value, nothing)
end

"""Capture a complete native legal frontier under a strict work budget.

The returned iterator copies bounded pages. A later frontier query or
`advance!` invalidates earlier iterators. Work exhaustion raises `NativeError`
with `STATUS_LIMIT_EXCEEDED` and publishes no partial frontier.
"""
function pda_frontier(session::PdaSession{L,W}; max_work::Integer=100_000,
    page_size::Integer=64) where {L,W}
    max_work > 0 || throw(ArgumentError("max_work must be positive"))
    page_size > 0 || throw(ArgumentError("page_size must be positive"))
    handle = pda_handle(session)
    session.generation += 1
    count = Ref{Csize_t}(0)
    checked(ccall(native(:lling_pda_session_frontier_open), UInt32,
        (Ptr{Cvoid}, Csize_t, Ref{Csize_t}), handle,
        max_work, count), :pda_frontier_open)
    PdaFrontier{L,W}(session, session.generation, Int(count[]), Int(page_size),
        PdaChoice{L,W}[], 1, 0)
end

"""Collect a bounded legal frontier as a Julia vector."""
pda_legal_next(session::PdaSession; kwargs...) = collect(pda_frontier(session; kwargs...))

"""Advance one terminal. Returns false when no native transition accepts it."""
function advance!(session::PdaSession{L}, label; max_work::Integer=100_000) where {L}
    max_work > 0 || throw(ArgumentError("max_work must be positive"))
    value, present = wire_label(L, label, nothing)
    present == 1 || throw(ArgumentError("PDA advance requires a terminal"))
    advanced = Ref{UInt8}(0)
    checked(ccall(native(:lling_pda_session_advance), UInt32,
        (Ptr{Cvoid}, UInt64, Csize_t, Ref{UInt8}), pda_handle(session),
        value, max_work, advanced), :pda_advance)
    session.generation += 1
    advanced[] == 1
end

"""Return `(accepted, weight)` under the native semiring and work budget."""
function pda_acceptance(session::PdaSession{L,W}; max_work::Integer=100_000) where {L,W}
    max_work > 0 || throw(ArgumentError("max_work must be positive"))
    accepted = Ref{UInt8}(0)
    weight = Ref{Float64}(0)
    checked(ccall(native(:lling_pda_session_acceptance), UInt32,
        (Ptr{Cvoid}, Csize_t, Ref{UInt8}, Ref{Float64}),
        pda_handle(session), max_work, accepted, weight), :pda_acceptance)
    (accepted=accepted[] == 1, weight=decode_weight(W, weight[]))
end

"""Return the current zero-based control state and stack depth."""
function pda_info(session::PdaSession)
    state = Ref{UInt32}(0)
    depth = Ref{Csize_t}(0)
    checked(ccall(native(:lling_pda_session_info), UInt32,
        (Ptr{Cvoid}, Ref{UInt32}, Ref{Csize_t}), pda_handle(session),
        state, depth), :pda_info)
    (state=state[], stack_depth=Int(depth[]))
end

"""Copy the current stack bottom first in bounded native pages."""
function pda_stack(session::PdaSession; page_size::Integer=64)
    page_size > 0 || throw(ArgumentError("page_size must be positive"))
    depth = pda_info(session).stack_depth
    output = UInt32[]
    sizehint!(output, depth)
    while length(output) < depth
        page = Vector{UInt32}(undef, min(Int(page_size), depth - length(output)))
        written = Ref{Csize_t}(0)
        status = GC.@preserve page ccall(native(:lling_pda_session_stack_page), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{UInt32}, Csize_t, Ref{Csize_t}),
            pda_handle(session), length(output), pointer(page), length(page), written)
        checked(status, :pda_stack_page)
        written[] > 0 || throw(ArgumentError("native PDA stack page did not advance"))
        append!(output, @view(page[1:Int(written[])]))
    end
    output
end
