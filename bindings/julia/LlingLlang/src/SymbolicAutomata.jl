"""An owned native symbolic finite automaton over `Char` or bounded `Int64`."""
mutable struct SymbolicAutomaton{D}
    handle::Ptr{Cvoid}
    universe_min::Int64
    universe_max::Int64
    lock::ReentrantLock
end

function symbolic_automaton(::Type{D}, domain::UInt32, lo::Int64, hi::Int64) where {D}
    require_symbolic_abi()
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_symbolic_automaton_new), UInt32,
        (UInt32, Int64, Int64, Ref{Ptr{Cvoid}}), domain, lo, hi, output),
        :symbolic_automaton_new)
    output[] != C_NULL || error("native symbolic constructor returned a null automaton")
    value = SymbolicAutomaton{D}(output[], lo, hi, ReentrantLock())
    finalizer(finalize_close, value)
    value
end

SymbolicAutomaton(::Type{Char}) = symbolic_automaton(Char, SYMBOLIC_CHAR, 0, 0)

function SymbolicAutomaton(::Type{Int64}; universe_min::Integer, universe_max::Integer)
    lo = symbolic_int64(universe_min, "universe_min")
    hi = symbolic_int64(universe_max, "universe_max")
    lo < hi || throw(ArgumentError("symbolic integer universe must be nonempty"))
    symbolic_automaton(Int64, SYMBOLIC_INTERVAL, lo, hi)
end

function close!(automaton::SymbolicAutomaton)
    lock(automaton.lock) do
        automaton.handle == C_NULL && return nothing
        ccall(native(:lling_symbolic_automaton_free), Cvoid,
            (Ptr{Cvoid},), automaton.handle)
        automaton.handle = C_NULL
    end
    nothing
end

Base.close(automaton::SymbolicAutomaton) = close!(automaton)
Base.isopen(automaton::SymbolicAutomaton) = automaton.handle != C_NULL

function with_symbolic_automaton(call, automaton::SymbolicAutomaton)
    lock(automaton.lock) do
        automaton.handle != C_NULL || throw(NativeError(STATUS_CLOSED,
            :symbolic_automaton, "symbolic automaton is closed"))
        call(automaton.handle)
    end
end

"""Append an accepting or nonaccepting state and return its zero-based ID."""
function add_state!(automaton::SymbolicAutomaton; accepting::Bool=false)
    output = Ref{UInt64}(0)
    with_symbolic_automaton(automaton) do handle
        checked(ccall(native(:lling_symbolic_automaton_add_state), UInt32,
            (Ptr{Cvoid}, UInt8, Ref{UInt64}), handle, UInt8(accepting), output),
            :symbolic_automaton_add_state)
    end
    output[]
end

"""Mark a previously added state as initial; multiple initial states are allowed."""
function set_initial!(automaton::SymbolicAutomaton, state::Integer)
    state >= 0 || throw(ArgumentError("state ID must be nonnegative"))
    id = UInt64(state)
    with_symbolic_automaton(automaton) do handle
        checked(ccall(native(:lling_symbolic_automaton_set_initial), UInt32,
            (Ptr{Cvoid}, UInt64), handle, id),
            :symbolic_automaton_set_initial)
    end
    automaton
end

"""Copy a native guard predicate into one transition; the guard remains owned by the caller."""
function add_transition!(automaton::SymbolicAutomaton{D}, from::Integer,
    to::Integer, guard::SymbolicPredicate{D}) where {D}
    from >= 0 && to >= 0 || throw(ArgumentError("state IDs must be nonnegative"))
    automaton.universe_min == guard.universe_min &&
        automaton.universe_max == guard.universe_max ||
        throw(ArgumentError("symbolic guard universe differs from automaton"))
    source = UInt64(from)
    target = UInt64(to)
    with_symbolic_handle(guard) do predicate_handle
        with_symbolic_automaton(automaton) do automaton_handle
            checked(ccall(native(:lling_symbolic_automaton_add_transition), UInt32,
                (Ptr{Cvoid}, UInt64, UInt64, Ptr{Cvoid}),
                automaton_handle, source, target, predicate_handle),
                :symbolic_automaton_add_transition)
        end
    end
    automaton
end

function symbolic_accepts_values(automaton::SymbolicAutomaton, values::Vector{Int64})
    output = Ref{UInt8}(0)
    with_symbolic_automaton(automaton) do handle
        GC.@preserve values begin
            checked(ccall(native(:lling_symbolic_automaton_accepts), UInt32,
                (Ptr{Cvoid}, Ptr{Int64}, Csize_t, Ref{UInt8}),
                handle, values, length(values), output),
                :symbolic_automaton_accepts)
        end
    end
    output[] == 1
end

"""Run the Rust symbolic automaton on a concrete Unicode string or character word."""
symbolic_accepts(automaton::SymbolicAutomaton{Char}, word::AbstractString) =
    symbolic_accepts_values(automaton, Int64[UInt32(c) for c in word])
symbolic_accepts(automaton::SymbolicAutomaton{Char}, word::AbstractVector{Char}) =
    symbolic_accepts_values(automaton, Int64[UInt32(c) for c in word])

"""Run the Rust symbolic automaton on a concrete signed-integer word."""
symbolic_accepts(automaton::SymbolicAutomaton{Int64}, word::AbstractVector{<:Integer}) =
    symbolic_accepts_values(automaton,
        Int64[symbolic_int64(value, "symbolic word value") for value in word])

"""Decide whether the native symbolic automaton accepts no word."""
function Base.isempty(automaton::SymbolicAutomaton)
    output = Ref{UInt8}(0)
    with_symbolic_automaton(automaton) do handle
        checked(ccall(native(:lling_symbolic_automaton_is_empty), UInt32,
            (Ptr{Cvoid}, Ref{UInt8}), handle, output),
            :symbolic_automaton_is_empty)
    end
    output[] == 1
end
