"""An owned native predicate over Unicode scalars or a bounded signed-integer universe.

The type parameter is `Char` or `Int64`. Each handle has its own immutable
native algebra interpretation. Close the handle when its lifetime ends.
"""
mutable struct SymbolicPredicate{D}
    handle::Ptr{Cvoid}
    universe_min::Int64
    universe_max::Int64
    lock::ReentrantLock
end

const SYMBOLIC_CHAR = UInt32(1)
const SYMBOLIC_INTERVAL = UInt32(2)
const SYMBOLIC_AND = UInt32(1)
const SYMBOLIC_OR = UInt32(2)
const SYMBOLIC_IMPLIES = UInt32(1)
const SYMBOLIC_EQUIVALENT = UInt32(2)
const SYMBOLIC_OVERLAPS = UInt32(3)

function require_symbolic_abi()
    api_revision() >= 13 || throw(NativeError(STATUS_UNSUPPORTED,
        :symbolic_predicate, "native symbolic predicates require API revision 13"))
    nothing
end

function symbolic_int64(value::Integer, name::AbstractString)
    typemin(Int64) <= value <= typemax(Int64) ||
        throw(ArgumentError("$name is outside the native Int64 domain"))
    Int64(value)
end

function adopt_symbolic(::Type{D}, handle::Ptr{Cvoid}, lo::Int64, hi::Int64) where {D}
    handle != C_NULL || error("native symbolic operation returned a null predicate")
    value = SymbolicPredicate{D}(handle, lo, hi, ReentrantLock())
    finalizer(finalize_close, value)
    value
end

function close!(predicate::SymbolicPredicate)
    lock(predicate.lock) do
        predicate.handle == C_NULL && return nothing
        ccall(native(:lling_symbolic_predicate_free), Cvoid,
            (Ptr{Cvoid},), predicate.handle)
        predicate.handle = C_NULL
    end
    nothing
end

Base.close(predicate::SymbolicPredicate) = close!(predicate)
Base.isopen(predicate::SymbolicPredicate) = predicate.handle != C_NULL

function with_symbolic_handle(call, predicate::SymbolicPredicate)
    lock(predicate.lock) do
        predicate.handle != C_NULL || throw(NativeError(STATUS_CLOSED,
            :symbolic_predicate, "symbolic predicate is closed"))
        call(predicate.handle)
    end
end

function with_symbolic_handles(call, first::SymbolicPredicate, second::SymbolicPredicate)
    if first === second
        return with_symbolic_handle(handle -> call(handle, handle), first)
    end
    outer, inner = objectid(first) < objectid(second) ?
        (first, second) : (second, first)
    with_symbolic_handle(outer) do _
        with_symbolic_handle(inner) do _
            call(first.handle, second.handle)
        end
    end
end

function symbolic_constant(::Type{Char}, truth::Bool)
    require_symbolic_abi()
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_symbolic_predicate_constant), UInt32,
        (UInt32, Int64, Int64, UInt8, Ref{Ptr{Cvoid}}),
        SYMBOLIC_CHAR, 0, 0, UInt8(truth), output),
        :symbolic_predicate_constant)
    adopt_symbolic(Char, output[], 0, 0)
end

function symbolic_constant(::Type{Int64}, truth::Bool;
    universe_min::Integer, universe_max::Integer)
    require_symbolic_abi()
    lo = symbolic_int64(universe_min, "universe_min")
    hi = symbolic_int64(universe_max, "universe_max")
    lo < hi || throw(ArgumentError("symbolic integer universe must be nonempty"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_symbolic_predicate_constant), UInt32,
        (UInt32, Int64, Int64, UInt8, Ref{Ptr{Cvoid}}),
        SYMBOLIC_INTERVAL, lo, hi, UInt8(truth), output),
        :symbolic_predicate_constant)
    adopt_symbolic(Int64, output[], lo, hi)
end

"""Native universal predicate for `Char` or a bounded `Int64` universe."""
symbolic_true(::Type{D}; kwargs...) where {D} = symbolic_constant(D, true; kwargs...)
"""Native empty predicate for `Char` or a bounded `Int64` universe."""
symbolic_false(::Type{D}; kwargs...) where {D} = symbolic_constant(D, false; kwargs...)

"""Inclusive Unicode scalar range, evaluated by Rust's `CharClassAlgebra`."""
function symbolic_char_range(first::Char, last::Char)
    require_symbolic_abi()
    first <= last || throw(ArgumentError("Unicode range endpoints are reversed"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_symbolic_char_range), UInt32,
        (UInt32, UInt32, Ref{Ptr{Cvoid}}),
        UInt32(first), UInt32(last), output), :symbolic_char_range)
    adopt_symbolic(Char, output[], 0, 0)
end

"""Half-open signed integer range inside one explicitly bounded universe."""
function symbolic_interval_range(universe_min::Integer, universe_max::Integer,
    lo::Integer, hi_exclusive::Integer)
    require_symbolic_abi()
    low_universe = symbolic_int64(universe_min, "universe_min")
    high_universe = symbolic_int64(universe_max, "universe_max")
    low = symbolic_int64(lo, "lo")
    high = symbolic_int64(hi_exclusive, "hi_exclusive")
    low_universe <= low < high <= high_universe ||
        throw(ArgumentError("interval must lie in a nonempty universe"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_symbolic_interval_range), UInt32,
        (Int64, Int64, Int64, Int64, Ref{Ptr{Cvoid}}),
        low_universe, high_universe, low, high, output),
        :symbolic_interval_range)
    adopt_symbolic(Int64, output[], low_universe, high_universe)
end

function matching_symbolic_domains(first::SymbolicPredicate{D},
    second::SymbolicPredicate{D}) where {D}
    first.universe_min == second.universe_min &&
        first.universe_max == second.universe_max ||
        throw(ArgumentError("symbolic predicate universes differ"))
    nothing
end

function symbolic_binary(operation::UInt32, first::SymbolicPredicate{D},
    second::SymbolicPredicate{D}) where {D}
    matching_symbolic_domains(first, second)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    with_symbolic_handles(first, second) do left, right
        checked(ccall(native(:lling_symbolic_predicate_binary), UInt32,
            (UInt32, Ptr{Cvoid}, Ptr{Cvoid}, Ref{Ptr{Cvoid}}),
            operation, left, right, output), :symbolic_predicate_binary)
    end
    adopt_symbolic(D, output[], first.universe_min, first.universe_max)
end

Base.:&(first::SymbolicPredicate{D}, second::SymbolicPredicate{D}) where {D} =
    symbolic_binary(SYMBOLIC_AND, first, second)
Base.:|(first::SymbolicPredicate{D}, second::SymbolicPredicate{D}) where {D} =
    symbolic_binary(SYMBOLIC_OR, first, second)

function Base.:!(predicate::SymbolicPredicate{D}) where {D}
    output = Ref{Ptr{Cvoid}}(C_NULL)
    with_symbolic_handle(predicate) do handle
        checked(ccall(native(:lling_symbolic_predicate_not), UInt32,
            (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), handle, output),
            :symbolic_predicate_not)
    end
    adopt_symbolic(D, output[], predicate.universe_min, predicate.universe_max)
end

function symbolic_evaluate(predicate::SymbolicPredicate{Char}, value::Char)
    result = Ref{UInt8}(0)
    with_symbolic_handle(predicate) do handle
        checked(ccall(native(:lling_symbolic_predicate_evaluate), UInt32,
            (Ptr{Cvoid}, Int64, Ref{UInt8}), handle, Int64(UInt32(value)), result),
            :symbolic_predicate_evaluate)
    end
    result[] == 1
end

function symbolic_evaluate(predicate::SymbolicPredicate{Int64}, value::Integer)
    result = Ref{UInt8}(0)
    checked_value = symbolic_int64(value, "symbolic value")
    with_symbolic_handle(predicate) do handle
        checked(ccall(native(:lling_symbolic_predicate_evaluate), UInt32,
            (Ptr{Cvoid}, Int64, Ref{UInt8}), handle, checked_value, result),
            :symbolic_predicate_evaluate)
    end
    result[] == 1
end

Base.in(value::Char, predicate::SymbolicPredicate{Char}) =
    symbolic_evaluate(predicate, value)
Base.in(value::Integer, predicate::SymbolicPredicate{Int64}) =
    symbolic_evaluate(predicate, value)

"""Return a native satisfiability witness or `nothing` if unsatisfiable."""
function symbolic_witness(predicate::SymbolicPredicate{D}) where {D}
    satisfiable = Ref{UInt8}(0)
    witness = Ref{Int64}(0)
    with_symbolic_handle(predicate) do handle
        checked(ccall(native(:lling_symbolic_predicate_witness), UInt32,
            (Ptr{Cvoid}, Ref{UInt8}, Ref{Int64}),
            handle, satisfiable, witness), :symbolic_predicate_witness)
    end
    satisfiable[] == 0 && return nothing
    D === Char ? Char(UInt32(witness[])) : witness[]
end

"""Decide satisfiability in the native effective Boolean algebra."""
symbolic_satisfiable(predicate::SymbolicPredicate) =
    symbolic_witness(predicate) !== nothing

function symbolic_relation(relation::UInt32, first::SymbolicPredicate{D},
    second::SymbolicPredicate{D}) where {D}
    matching_symbolic_domains(first, second)
    result = Ref{UInt8}(0)
    with_symbolic_handles(first, second) do left, right
        checked(ccall(native(:lling_symbolic_predicate_relation), UInt32,
            (UInt32, Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt8}),
            relation, left, right, result), :symbolic_predicate_relation)
    end
    result[] == 1
end

"""Decide whether every value satisfying `first` satisfies `second`."""
symbolic_implies(first::SymbolicPredicate{D}, second::SymbolicPredicate{D}) where {D} =
    symbolic_relation(SYMBOLIC_IMPLIES, first, second)
"""Decide semantic equality over the same native universe."""
symbolic_equivalent(first::SymbolicPredicate{D}, second::SymbolicPredicate{D}) where {D} =
    symbolic_relation(SYMBOLIC_EQUIVALENT, first, second)
"""Decide whether two native predicates admit a common value."""
symbolic_overlaps(first::SymbolicPredicate{D}, second::SymbolicPredicate{D}) where {D} =
    symbolic_relation(SYMBOLIC_OVERLAPS, first, second)
