"""An owned native symbolic finite transducer over `Char` or bounded `Int64`."""
mutable struct SymbolicTransducer{D}
    handle::Ptr{Cvoid}
    universe_min::Int64
    universe_max::Int64
    lock::ReentrantLock
end

"""Explicit limits for one exact native concrete-input transduction."""
struct SymbolicTransductionLimits
    struct_size::UInt32
    version::UInt32
    max_states::UInt64
    max_arcs::UInt64
    max_work::UInt64
    max_heap_bytes::UInt64
    max_elapsed_ns::UInt64
    max_paths::UInt64
    max_frontier::UInt64
end

function symbolic_limit(value::Integer, name::AbstractString)
    0 <= value <= typemax(UInt64) || throw(ArgumentError("$name must fit UInt64"))
    UInt64(value)
end

function SymbolicTransductionLimits(;
    max_states::Integer=100_000,
    max_arcs::Integer=1_000_000,
    max_work::Integer=10_000_000,
    max_heap_bytes::Integer=256_000_000,
    max_elapsed_ns::Integer=10_000_000_000,
    max_paths::Integer=100_000,
    max_frontier::Integer=100_000)
    SymbolicTransductionLimits(UInt32(sizeof(SymbolicTransductionLimits)), UInt32(1),
        symbolic_limit(max_states, "max_states"),
        symbolic_limit(max_arcs, "max_arcs"),
        symbolic_limit(max_work, "max_work"),
        symbolic_limit(max_heap_bytes, "max_heap_bytes"),
        symbolic_limit(max_elapsed_ns, "max_elapsed_ns"),
        symbolic_limit(max_paths, "max_paths"),
        symbolic_limit(max_frontier, "max_frontier"))
end

const SYMBOLIC_OUTPUT_EPSILON = UInt32(1)
const SYMBOLIC_OUTPUT_IDENTITY = UInt32(2)
const SYMBOLIC_OUTPUT_CONSTANT = UInt32(3)

function symbolic_transducer(::Type{D}, domain::UInt32, lo::Int64, hi::Int64) where {D}
    api_revision() >= 14 || throw(NativeError(STATUS_UNSUPPORTED,
        :symbolic_transducer, "native symbolic transducers require API revision 14"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_symbolic_transducer_new), UInt32,
        (UInt32, Int64, Int64, Ref{Ptr{Cvoid}}), domain, lo, hi, output),
        :symbolic_transducer_new)
    output[] != C_NULL || error("native symbolic constructor returned a null transducer")
    value = SymbolicTransducer{D}(output[], lo, hi, ReentrantLock())
    finalizer(finalize_close, value)
    value
end

SymbolicTransducer(::Type{Char}) = symbolic_transducer(Char, SYMBOLIC_CHAR, 0, 0)

function SymbolicTransducer(::Type{Int64}; universe_min::Integer, universe_max::Integer)
    lo = symbolic_int64(universe_min, "universe_min")
    hi = symbolic_int64(universe_max, "universe_max")
    lo < hi || throw(ArgumentError("symbolic integer universe must be nonempty"))
    symbolic_transducer(Int64, SYMBOLIC_INTERVAL, lo, hi)
end

function close!(transducer::SymbolicTransducer)
    lock(transducer.lock) do
        transducer.handle == C_NULL && return nothing
        ccall(native(:lling_symbolic_transducer_free), Cvoid,
            (Ptr{Cvoid},), transducer.handle)
        transducer.handle = C_NULL
    end
    nothing
end

Base.close(transducer::SymbolicTransducer) = close!(transducer)
Base.isopen(transducer::SymbolicTransducer) = transducer.handle != C_NULL

function with_symbolic_transducer(call, transducer::SymbolicTransducer)
    lock(transducer.lock) do
        transducer.handle != C_NULL || throw(NativeError(STATUS_CLOSED,
            :symbolic_transducer, "symbolic transducer is closed"))
        call(transducer.handle)
    end
end

function with_symbolic_transducer_handles(call, first::SymbolicTransducer,
    second::SymbolicTransducer)
    if first === second
        return with_symbolic_transducer(handle -> call(handle, handle), first)
    end
    outer, inner = objectid(first) < objectid(second) ?
        (first, second) : (second, first)
    with_symbolic_transducer(outer) do _
        with_symbolic_transducer(inner) do _
            call(first.handle, second.handle)
        end
    end
end

"""Append an accepting or nonaccepting state and return its zero-based ID."""
function add_state!(transducer::SymbolicTransducer; accepting::Bool=false)
    output = Ref{UInt64}(0)
    with_symbolic_transducer(transducer) do handle
        checked(ccall(native(:lling_symbolic_transducer_add_state), UInt32,
            (Ptr{Cvoid}, UInt8, Ref{UInt64}), handle, UInt8(accepting), output),
            :symbolic_transducer_add_state)
    end
    output[]
end

"""Mark an existing transducer state as initial."""
function set_initial!(transducer::SymbolicTransducer, state::Integer)
    state >= 0 || throw(ArgumentError("state ID must be nonnegative"))
    id = UInt64(state)
    with_symbolic_transducer(transducer) do handle
        checked(ccall(native(:lling_symbolic_transducer_set_initial), UInt32,
            (Ptr{Cvoid}, UInt64), handle, id),
            :symbolic_transducer_set_initial)
    end
    transducer
end

function symbolic_add_transition!(transducer::SymbolicTransducer{D},
    from::Integer, to::Integer, guard::SymbolicPredicate{D},
    output_kind::UInt32, output::Vector{Int64}) where {D}
    from >= 0 && to >= 0 || throw(ArgumentError("state IDs must be nonnegative"))
    transducer.universe_min == guard.universe_min &&
        transducer.universe_max == guard.universe_max ||
        throw(ArgumentError("symbolic guard universe differs from transducer"))
    source = UInt64(from)
    target = UInt64(to)
    with_symbolic_handle(guard) do predicate_handle
        with_symbolic_transducer(transducer) do transducer_handle
            GC.@preserve output begin
                checked(ccall(native(:lling_symbolic_transducer_add_transition), UInt32,
                    (Ptr{Cvoid}, UInt64, UInt64, Ptr{Cvoid}, UInt32,
                        Ptr{Int64}, Csize_t),
                    transducer_handle, source, target, predicate_handle, output_kind,
                    output, length(output)), :symbolic_transducer_add_transition)
            end
        end
    end
    transducer
end

"""Add a guarded transition with `:epsilon` or `:identity` output."""
function add_transition!(transducer::SymbolicTransducer{D}, from::Integer,
    to::Integer, guard::SymbolicPredicate{D}, output::Symbol) where {D}
    kind = output === :epsilon ? SYMBOLIC_OUTPUT_EPSILON :
        output === :identity ? SYMBOLIC_OUTPUT_IDENTITY :
        throw(ArgumentError("symbolic output must be :epsilon or :identity"))
    symbolic_add_transition!(transducer, from, to, guard, kind, Int64[])
end

"""Add a guarded transition with a constant Unicode output word."""
function add_transition!(transducer::SymbolicTransducer{Char}, from::Integer,
    to::Integer, guard::SymbolicPredicate{Char}, output::AbstractVector{Char})
    symbolic_add_transition!(transducer, from, to, guard,
        SYMBOLIC_OUTPUT_CONSTANT, Int64[UInt32(c) for c in output])
end

"""Add a guarded transition with a constant signed-integer output word."""
function add_transition!(transducer::SymbolicTransducer{Int64}, from::Integer,
    to::Integer, guard::SymbolicPredicate{Int64}, output::AbstractVector{<:Integer})
    symbolic_add_transition!(transducer, from, to, guard,
        SYMBOLIC_OUTPUT_CONSTANT,
        Int64[symbolic_int64(value, "symbolic output") for value in output])
end

symbolic_word(::Type{Char}, input::AbstractString) =
    Int64[UInt32(c) for c in input]
symbolic_word(::Type{Char}, input::AbstractVector{Char}) =
    Int64[UInt32(c) for c in input]
symbolic_word(::Type{Int64}, input::AbstractVector{<:Integer}) =
    Int64[symbolic_int64(value, "symbolic input") for value in input]

symbolic_output(::Type{Char}, values::Vector{Int64}) =
    join(Char[Char(UInt32(value)) for value in values])
symbolic_output(::Type{Int64}, values::Vector{Int64}) = values

function read_symbolic_transduction(::Type{D}, result::Ptr{Cvoid}) where {D}
    count = Ref{UInt64}(0)
    checked(ccall(native(:lling_symbolic_transduction_count), UInt32,
        (Ptr{Cvoid}, Ref{UInt64}), result, count),
        :symbolic_transduction_count)
    output_type = D === Char ? String : Vector{Int64}
    output = Vector{output_type}(undef, Int(count[]))
    count[] == 0 && return output
    for index in UInt64(0):(count[] - UInt64(1))
        required = Ref{Csize_t}(0)
        checked(ccall(native(:lling_symbolic_transduction_output), UInt32,
            (Ptr{Cvoid}, UInt64, Ptr{Int64}, Csize_t, Ref{Csize_t}),
            result, index, Ptr{Int64}(C_NULL), 0, required),
            :symbolic_transduction_output)
        values = Vector{Int64}(undef, Int(required[]))
        GC.@preserve values begin
            checked(ccall(native(:lling_symbolic_transduction_output), UInt32,
                (Ptr{Cvoid}, UInt64, Ptr{Int64}, Csize_t, Ref{Csize_t}),
                result, index, values, length(values), required),
                :symbolic_transduction_output)
        end
        output[Int(index) + 1] = symbolic_output(D, values)
    end
    output
end

"""Enumerate exact outputs of every accepting native path under explicit limits.

For `Char`, outputs are strings; for `Int64`, they are integer vectors.
Duplicate words are retained when different accepting paths produce them.
An incomplete search raises `NativeError(STATUS_LIMIT_EXCEEDED, ...)`.
"""
function symbolic_transduce(transducer::SymbolicTransducer{D}, input;
    limits::SymbolicTransductionLimits=SymbolicTransductionLimits()) where {D}
    values = symbolic_word(D, input)
    result = Ref{Ptr{Cvoid}}(C_NULL)
    with_symbolic_transducer(transducer) do handle
        GC.@preserve values begin
            checked(ccall(native(:lling_symbolic_transducer_transduce), UInt32,
                (Ptr{Cvoid}, Ptr{Int64}, Csize_t,
                    Ref{SymbolicTransductionLimits}, Ref{Ptr{Cvoid}}),
                handle, values, length(values), Ref(limits), result),
                :symbolic_transducer_transduce)
        end
    end
    result[] != C_NULL || error("native symbolic transduction returned a null result")
    try
        read_symbolic_transduction(D, result[])
    finally
        ccall(native(:lling_symbolic_transduction_free), Cvoid,
            (Ptr{Cvoid},), result[])
    end
end

"""Execute `second(first(input))` exactly in Rust under one shared budget.

The intermediate word is handled by the native bounded composition engine.
Different accepting path pairs can produce duplicate output words.
"""
function symbolic_compose_transduce(first::SymbolicTransducer{D},
    second::SymbolicTransducer{D}, input;
    limits::SymbolicTransductionLimits=SymbolicTransductionLimits()) where {D}
    first.universe_min == second.universe_min &&
        first.universe_max == second.universe_max ||
        throw(ArgumentError("symbolic transducer universes differ"))
    values = symbolic_word(D, input)
    result = Ref{Ptr{Cvoid}}(C_NULL)
    with_symbolic_transducer_handles(first, second) do first_handle, second_handle
        GC.@preserve values begin
            checked(ccall(native(:lling_symbolic_transducer_compose_transduce), UInt32,
                (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{Int64}, Csize_t,
                    Ref{SymbolicTransductionLimits}, Ref{Ptr{Cvoid}}),
                first_handle, second_handle, values, length(values), Ref(limits), result),
                :symbolic_transducer_compose_transduce)
        end
    end
    result[] != C_NULL || error("native symbolic composition returned a null result")
    try
        read_symbolic_transduction(D, result[])
    finally
        ccall(native(:lling_symbolic_transduction_free), Cvoid,
            (Ptr{Cvoid},), result[])
    end
end
