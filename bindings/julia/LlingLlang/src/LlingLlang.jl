module LlingLlang

using Libdl
import VinaryTreeInterop

const VTI = VinaryTreeInterop
include("GeneratedAbi.jl")

@doc "Native lling-llang ABI version required by this facade." ABI_VERSION
@doc "Minimum additive lling-llang API revision required by this facade." API_REVISION
@doc "Stable status returned by the lling-llang C ABI." Status

export ABI_VERSION,
    API_REVISION,
    Status,
    TYPED_ABI_VERSION,
    DESCRIPTOR_SIGNATURE_KNOWN,
    DESCRIPTOR_SNAPSHOT_PRESENT,
    DESCRIPTOR_CONTEXT_PRESENT,
    BUDGET_STATES,
    BUDGET_ARCS,
    BUDGET_BYTES,
    BUDGET_WORK,
    CancellationReasonV2,
    AbiV2Header,
    Id128,
    Digest256,
    WfstDescriptorV2,
    BudgetV2,
    OutcomeV2,
    CancellationV2,
    NativeError,
    AbstractScalarWeight,
    TropicalWeight,
    LogWeight,
    ProbabilityWeight,
    ArcticWeight,
    SignedTropicalWeight,
    CountWeight,
    BooleanWeight,
    SymbolTable,
    WfstBuilder,
    Wfst,
    WfstArc,
    WfstState,
    PathLimits,
    WfstPathStep,
    WfstPath,
    PathIterator,
    PathPending,
    PathTruncatedError,
    PathCancelledError,
    GraphLimits,
    GraphCaptureCursor,
    GraphSnapshot,
    GraphPending,
    GraphCancelledError,
    GraphArc,
    GraphState,
    DistanceLimits,
    DistanceCursor,
    DistanceResult,
    DistancePending,
    DistanceCancelledError,
    RankedPathLimits,
    RankedPathIterator,
    RankedPathPending,
    CostPrunedPathIterator,
    SamplePathLimits,
    SamplePathIterator,
    SamplePathPending,
    ProviderArc,
    ProviderState,
    AbstractWfstProvider,
    AbstractSemiringProvider,
    DynamicLatticeValue,
    SemiringContext,
    SemiringWeight,
    abi_version,
    api_revision,
    validate_abi_v2_header,
    typed_evidence_allowed,
    validate_budget_v2,
    authoritative_exact,
    identity_matches,
    request!,
    cancellation_reason,
    reserve_states!,
    intern!,
    freeze!,
    isfrozen,
    symbol,
    label,
    add_state!,
    set_start!,
    set_final!,
    clear_final!,
    add_arc!,
    build!,
    import_wfst,
    compose,
    acceptor_intersect,
    concat,
    closure,
    closure_plus,
    determinize,
    minimize,
    remove_epsilon,
    connect,
    project_input,
    project_output,
    state,
    arcs,
    paths,
    poll_path!,
    reduce_paths,
    capture_graph,
    poll_graph!,
    complete_graph,
    graph_info,
    graph_state,
    graph_arcs,
    analyze_distances,
    poll_distance!,
    complete_distances,
    distance_info,
    distance_page,
    posterior_arcs,
    posterior_final,
    ranked_paths,
    poll_ranked_path!,
    reduce_ranked_paths,
    best_path,
    k_best_paths,
    n_best_paths,
    cost_pruned_paths,
    poll_cost_pruned_path!,
    reduce_cost_pruned_paths,
    sample_paths,
    poll_sample_path!,
    sample_path,
    sample_n_paths,
    reduce_sampled_paths,
    input_symbols,
    output_symbols,
    resource,
    provider,
    semiring_provider,
    dynamic_lattice_value,
    lattice_domain_id,
    lattice_flags,
    lattice_join,
    lattice_meet,
    lattice_equal,
    lattice_stable_bytes,
    lattice_diagnostic,
    lattice_join_many,
    lattice_meet_many,
    validate_lattice_laws,
    semiring_context,
    semiring_properties,
    semiring_zero,
    semiring_one,
    semiring_plus,
    semiring_times,
    semiring_plus_many,
    semiring_times_many,
    semiring_equal,
    semiring_approx_equal,
    semiring_natural_order,
    semiring_divide,
    semiring_left_divide,
    semiring_star,
    semiring_numerical_value,
    semiring_quantize,
    semiring_probability,
    semiring_closure_bound,
    semiring_stable_bytes,
    validate_semiring_laws,
    semiring_diagnostic,
    wfst_start,
    wfst_state_count,
    wfst_state,
    close!

"""A copied native error with its stable status and thread-local diagnostic."""
struct NativeError <: Exception
    status::Status
    operation::Symbol
    message::String
end

function Base.showerror(io::IO, error::NativeError)
    print(io, error.operation, " failed with ", error.status)
    isempty(error.message) || print(io, ": ", error.message)
end

const LIBRARY_HANDLE = Ref{Ptr{Cvoid}}(C_NULL)

function library_candidates()
    names = Sys.iswindows() ? ["lling_llang.dll"] :
        Sys.isapple() ? ["liblling_llang.dylib"] : ["liblling_llang.so"]
    explicit = get(ENV, "LLING_LLANG_LIBRARY", "")
    isempty(explicit) ? names : vcat([explicit], names)
end

function library_handle()
    LIBRARY_HANDLE[] != C_NULL && return LIBRARY_HANDLE[]
    failures = String[]
    for candidate in library_candidates()
        try
            LIBRARY_HANDLE[] = Libdl.dlopen(candidate)
            return LIBRARY_HANDLE[]
        catch error
            push!(failures, "$candidate: $(sprint(showerror, error))")
        end
    end
    error("could not load lling-llang; set LLING_LLANG_LIBRARY\n" *
        join(failures, "\n"))
end

native(name::Symbol) = Libdl.dlsym(library_handle(), name)
function semiring_native(name::Symbol)
    name === :lling_semiring_zero && return native(:lling_semiring_zero)
    name === :lling_semiring_one && return native(:lling_semiring_one)
    name === :lling_semiring_plus && return native(:lling_semiring_plus)
    name === :lling_semiring_times && return native(:lling_semiring_times)
    name === :lling_semiring_equal && return native(:lling_semiring_equal)
    name === :lling_semiring_approx_equal && return native(:lling_semiring_approx_equal)
    name === :lling_semiring_divide && return native(:lling_semiring_divide)
    name === :lling_semiring_left_divide && return native(:lling_semiring_left_divide)
    name === :lling_semiring_star && return native(:lling_semiring_star)
    name === :lling_semiring_numerical_value &&
        return native(:lling_semiring_numerical_value)
    name === :lling_semiring_to_probability &&
        return native(:lling_semiring_to_probability)
    name === :lling_semiring_stable_bytes &&
        return native(:lling_semiring_stable_bytes)
    name === :lling_semiring_diagnostic &&
        return native(:lling_semiring_diagnostic)
    name === :lling_semiring_plus_many && return native(:lling_semiring_plus_many)
    name === :lling_semiring_times_many && return native(:lling_semiring_times_many)
    throw(ArgumentError("unknown semiring operation $name"))
end
function lattice_native(name::Symbol)
    name === :lling_lattice_join && return native(:lling_lattice_join)
    name === :lling_lattice_meet && return native(:lling_lattice_meet)
    name === :lling_lattice_stable_bytes &&
        return native(:lling_lattice_stable_bytes)
    name === :lling_lattice_diagnostic && return native(:lling_lattice_diagnostic)
    name === :lling_lattice_join_many && return native(:lling_lattice_join_many)
    name === :lling_lattice_meet_many && return native(:lling_lattice_meet_many)
    throw(ArgumentError("unknown lattice operation $name"))
end
"""Return the ABI version exported by the loaded native library."""
abi_version() = UInt32(ccall(native(:lling_abi_version), UInt32, ()))
"""Return the additive API revision exported by the loaded native library."""
api_revision() = UInt32(ccall(native(:lling_llang_api_revision), UInt32, ()))

function last_error_message()
    pointer = ccall(native(:lling_last_error_message), Cstring, ())
    pointer == C_NULL ? "" : unsafe_string(pointer)
end

function checked(code::Integer, operation::Symbol)
    status = Status(UInt32(code))
    status == STATUS_OK && return nothing
    throw(NativeError(status, operation, last_error_message()))
end

function finalize_close(value)
    try
        close!(value)
    catch
    end
    nothing
end

"""Common fixed-layout prefix carried by every typed ABI-v2 structure."""
struct AbiV2Header
    struct_size::UInt32
    abi_version::UInt32
    flags::UInt64
    reserved::UInt64
end
AbiV2Header(struct_size::Integer, flags::Integer=0) = AbiV2Header(
    UInt32(struct_size), TYPED_ABI_VERSION, UInt64(flags), UInt64(0))

"""A fixed-width semantic identifier; all-zero bytes mean absent."""
struct Id128
    bytes::NTuple{16,UInt8}
end
Id128(bytes::AbstractVector{<:Integer}) =
    Id128(ntuple(index -> UInt8(bytes[index]), 16))
Id128() = Id128(ntuple(_ -> UInt8(0), 16))

"""A fixed-width evidence-context digest; all-zero bytes mean absent."""
struct Digest256
    bytes::NTuple{32,UInt8}
end
Digest256(bytes::AbstractVector{<:Integer}) =
    Digest256(ntuple(index -> UInt8(bytes[index]), 32))
Digest256() = Digest256(ntuple(_ -> UInt8(0), 32))

"""Replay-critical tape, algebra, snapshot, and evidence-context identity."""
struct WfstDescriptorV2
    header::AbiV2Header
    input_tape::Id128
    output_tape::Id128
    algebra::Id128
    snapshot::Id128
    context::Digest256
end

"""Canonical state, arc, byte, and abstract-work limits."""
struct BudgetV2
    header::AbiV2Header
    max_states::UInt64
    max_arcs::UInt64
    max_bytes::UInt64
    max_work::UInt64
    reserved::NTuple{2,UInt64}
end
function BudgetV2(; max_states::Integer=0, max_arcs::Integer=0,
    max_bytes::Integer=0, max_work::Integer=0)
    values = (max_states, max_arcs, max_bytes, max_work)
    all(value -> value >= 0, values) ||
        throw(ArgumentError("ABI-v2 budgets cannot be negative"))
    flags = (max_states == 0 ? UInt64(0) : BUDGET_STATES) |
        (max_arcs == 0 ? UInt64(0) : BUDGET_ARCS) |
        (max_bytes == 0 ? UInt64(0) : BUDGET_BYTES) |
        (max_work == 0 ? UInt64(0) : BUDGET_WORK)
    BudgetV2(AbiV2Header(sizeof(BudgetV2), flags), UInt64(max_states),
        UInt64(max_arcs), UInt64(max_bytes), UInt64(max_work),
        (UInt64(0), UInt64(0)))
end

"""Orthogonal semantic, completion, publication, and evidence outcome axes."""
struct OutcomeV2
    header::AbiV2Header
    precision::UInt32
    completeness::UInt32
    applicability::UInt32
    termination::UInt32
    evidence::UInt32
    reserved0::UInt32
    states::UInt64
    arcs::UInt64
    bytes::UInt64
    work::UInt64
    limitations::UInt64
    reserved1::UInt64
end

function validate_abi_v2_header(
    header::AbiV2Header, required_size::Integer, known_flags::Integer)
    checked(ccall(native(:lling_abi_v2_validate_header), UInt32,
        (Ref{AbiV2Header}, UInt32, UInt64), Ref(header), required_size,
        known_flags), :abi_v2_validate_header)
    header
end
function typed_evidence_allowed(descriptor::WfstDescriptorV2)
    output = Ref{UInt8}(0)
    checked(ccall(native(:lling_abi_v2_validate_descriptor), UInt32,
        (Ref{WfstDescriptorV2}, Ref{UInt8}), Ref(descriptor), output),
        :abi_v2_validate_descriptor)
    output[] != 0
end
function validate_budget_v2(budget::BudgetV2)
    checked(ccall(native(:lling_abi_v2_validate_budget), UInt32,
        (Ref{BudgetV2},), Ref(budget)), :abi_v2_validate_budget)
    budget
end
function authoritative_exact(
    outcome::OutcomeV2; resource_present::Bool, evidence_present::Bool)
    output = Ref{UInt8}(0)
    checked(ccall(native(:lling_abi_v2_validate_outcome), UInt32,
        (Ref{OutcomeV2}, UInt8, UInt8, Ref{UInt8}), Ref(outcome),
        UInt8(resource_present), UInt8(evidence_present), output),
        :abi_v2_validate_outcome)
    output[] != 0
end
function identity_matches(expected::WfstDescriptorV2, observed::WfstDescriptorV2)
    output = Ref{UInt8}(0)
    checked(ccall(native(:lling_abi_v2_identity_matches), UInt32,
        (Ref{WfstDescriptorV2}, Ref{WfstDescriptorV2}, Ref{UInt8}),
        Ref(expected), Ref(observed), output), :abi_v2_identity_matches)
    output[] != 0
end

"""Thread-safe, first-reason-wins cooperative-cancellation owner."""
mutable struct CancellationV2
    handle::Ptr{Cvoid}
    closed::Bool
end
function CancellationV2()
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_cancellation_v2_new), UInt32,
        (Ref{Ptr{Cvoid}},), output), :cancellation_v2_new)
    value = CancellationV2(output[], false)
    finalizer(finalize_close, value)
    value
end
function open_handle(value::CancellationV2)
    value.closed && throw(NativeError(
        STATUS_CLOSED, :cancellation, "cancellation handle is closed"))
    value.handle
end
function request!(value::CancellationV2, reason::CancellationReasonV2)
    checked(ccall(native(:lling_cancellation_v2_request), UInt32,
        (Ptr{Cvoid}, UInt32), open_handle(value), UInt32(reason)),
        :cancellation_v2_request)
    value
end
function cancellation_reason(value::CancellationV2)
    output = Ref{UInt32}(0)
    checked(ccall(native(:lling_cancellation_v2_reason), UInt32,
        (Ptr{Cvoid}, Ref{UInt32}), open_handle(value), output),
        :cancellation_v2_reason)
    output[] == 0 ? nothing : CancellationReasonV2(output[])
end
function close!(value::CancellationV2)
    value.closed && return nothing
    slot = Ref(value.handle)
    checked(ccall(native(:lling_cancellation_v2_free), UInt32,
        (Ref{Ptr{Cvoid}},), slot), :cancellation_v2_free)
    value.handle = slot[]
    value.closed = true
    nothing
end
Base.close(value::CancellationV2) = close!(value)

# Typed scalar ABI domains --------------------------------------------------

"""Marker for an isbits Julia value in one built-in scalar semiring."""
abstract type AbstractScalarWeight end

macro checked_float_weight(name, predicate, message)
    quote
        struct $(esc(name)) <: AbstractScalarWeight
            value::Float64
            function $(esc(name))(value::Real)
                raw = Float64(value)
                $(esc(predicate))(raw) || throw(ArgumentError($(esc(message))))
                new(raw)
            end
        end
    end
end

positive_infinity_carrier(value) = isfinite(value) || value == Inf
probability_carrier(value) = isfinite(value) && value >= 0.0
arctic_carrier(value) = isfinite(value) || value == -Inf

@checked_float_weight(TropicalWeight, positive_infinity_carrier,
    "tropical weights must be finite or +Inf")
@checked_float_weight(LogWeight, positive_infinity_carrier,
    "log weights must be finite or +Inf")
@checked_float_weight(ProbabilityWeight, probability_carrier,
    "probability weights must be finite and nonnegative")
@checked_float_weight(ArcticWeight, arctic_carrier,
    "arctic weights must be finite or -Inf")
@checked_float_weight(SignedTropicalWeight, positive_infinity_carrier,
    "signed tropical weights must be finite or +Inf")

@doc "A min-plus scalar weight; finite values and `Inf` are representable." TropicalWeight
@doc "A negative-log scalar weight; finite values and `Inf` are representable." LogWeight
@doc "A nonnegative finite scalar weight with sum/product operations." ProbabilityWeight
@doc "A max-plus scalar weight; finite values and `-Inf` are representable." ArcticWeight
@doc "A signed min-plus scalar weight; finite values and `Inf` are representable." SignedTropicalWeight

const MAX_EXACT_COUNT = UInt64(1) << 53

"""An exact path count representable in the family ABI's `Float64` slot."""
struct CountWeight <: AbstractScalarWeight
    value::UInt64
    function CountWeight(value::Integer)
        0 <= value <= MAX_EXACT_COUNT || throw(ArgumentError(
            "count weights must be integers in 0:2^53"))
        new(UInt64(value))
    end
end

"""A reachability weight encoded as exactly zero or one on the ABI wire."""
struct BooleanWeight <: AbstractScalarWeight
    value::Bool
end
BooleanWeight(value::Integer) = value in (0, 1) ? BooleanWeight(value == 1) :
    throw(ArgumentError("Boolean weights must be false/true or zero/one"))

unit_domain(::Type{UInt8}) = VTI.UNIT_BYTE
unit_domain(::Type{Char}) = VTI.UNIT_UNICODE_SCALAR
unit_domain(::Type{UInt64}) = VTI.UNIT_U64
unit_domain(::Type{L}) where {L} = throw(ArgumentError(
    "unsupported WFST label type $L; use UInt8, Char, or UInt64"))

weight_domain(::Type{TropicalWeight}) = VTI.WEIGHT_TROPICAL_F64
weight_domain(::Type{LogWeight}) = VTI.WEIGHT_LOG_F64
weight_domain(::Type{ProbabilityWeight}) = VTI.WEIGHT_PROBABILITY_F64
weight_domain(::Type{ArcticWeight}) = VTI.WEIGHT_ARCTIC_F64
weight_domain(::Type{SignedTropicalWeight}) = VTI.WEIGHT_SIGNED_TROPICAL_F64
weight_domain(::Type{CountWeight}) = VTI.WEIGHT_COUNT_F64
weight_domain(::Type{BooleanWeight}) = VTI.WEIGHT_BOOLEAN_F64
weight_domain(::Type{W}) where {W} = throw(ArgumentError(
    "unsupported WFST weight type $W; use a built-in scalar weight"))

label_type(domain::VTI.UnitDomain) = domain == VTI.UNIT_BYTE ? UInt8 :
    domain == VTI.UNIT_UNICODE_SCALAR ? Char :
    domain == VTI.UNIT_U64 ? UInt64 :
    throw(ArgumentError("unknown WFST unit domain $domain"))
weight_type(domain::VTI.WeightDomain) = domain == VTI.WEIGHT_TROPICAL_F64 ? TropicalWeight :
    domain == VTI.WEIGHT_LOG_F64 ? LogWeight :
    domain == VTI.WEIGHT_PROBABILITY_F64 ? ProbabilityWeight :
    domain == VTI.WEIGHT_ARCTIC_F64 ? ArcticWeight :
    domain == VTI.WEIGHT_SIGNED_TROPICAL_F64 ? SignedTropicalWeight :
    domain == VTI.WEIGHT_COUNT_F64 ? CountWeight :
    domain == VTI.WEIGHT_BOOLEAN_F64 ? BooleanWeight :
    throw(ArgumentError("unknown WFST weight domain $domain"))

raw_weight(weight::Union{TropicalWeight,LogWeight,ProbabilityWeight,
    ArcticWeight,SignedTropicalWeight}) = weight.value
raw_weight(weight::CountWeight) = Float64(weight.value)
raw_weight(weight::BooleanWeight) = weight.value ? 1.0 : 0.0

decode_weight(::Type{W}, value::Real) where {W<:AbstractScalarWeight} = W(value)
function decode_weight(::Type{CountWeight}, value::Real)
    raw = Float64(value)
    isfinite(raw) && 0.0 <= raw <= Float64(MAX_EXACT_COUNT) && isinteger(raw) ||
        throw(ArgumentError("count weight is not an exact integer in 0:2^53"))
    CountWeight(UInt64(raw))
end
function decode_weight(::Type{BooleanWeight}, value::Real)
    raw = Float64(value)
    raw == 0.0 && return BooleanWeight(false)
    raw == 1.0 && return BooleanWeight(true)
    throw(ArgumentError("Boolean weight is not exactly zero or one"))
end

Base.zero(::Type{TropicalWeight}) = TropicalWeight(Inf)
Base.one(::Type{TropicalWeight}) = TropicalWeight(0.0)
Base.zero(::Type{LogWeight}) = LogWeight(Inf)
Base.one(::Type{LogWeight}) = LogWeight(0.0)
Base.zero(::Type{ProbabilityWeight}) = ProbabilityWeight(0.0)
Base.one(::Type{ProbabilityWeight}) = ProbabilityWeight(1.0)
Base.zero(::Type{ArcticWeight}) = ArcticWeight(-Inf)
Base.one(::Type{ArcticWeight}) = ArcticWeight(0.0)
Base.zero(::Type{SignedTropicalWeight}) = SignedTropicalWeight(Inf)
Base.one(::Type{SignedTropicalWeight}) = SignedTropicalWeight(0.0)
Base.zero(::Type{CountWeight}) = CountWeight(0)
Base.one(::Type{CountWeight}) = CountWeight(1)
Base.zero(::Type{BooleanWeight}) = BooleanWeight(false)
Base.one(::Type{BooleanWeight}) = BooleanWeight(true)

Base.:+(left::TropicalWeight, right::TropicalWeight) =
    TropicalWeight(min(left.value, right.value))
Base.:*(left::TropicalWeight, right::TropicalWeight) =
    TropicalWeight(isinf(left.value) || isinf(right.value) ? Inf : left.value + right.value)
Base.:+(left::LogWeight, right::LogWeight) = begin
    a, b = left.value, right.value
    isinf(a) ? right : isinf(b) ? left :
        LogWeight(min(a, b) - log1p(exp(-abs(a - b))))
end
Base.:*(left::LogWeight, right::LogWeight) =
    LogWeight(isinf(left.value) || isinf(right.value) ? Inf : left.value + right.value)
Base.:+(left::ProbabilityWeight, right::ProbabilityWeight) =
    ProbabilityWeight(left.value + right.value)
Base.:*(left::ProbabilityWeight, right::ProbabilityWeight) =
    ProbabilityWeight(left.value * right.value)
Base.:+(left::ArcticWeight, right::ArcticWeight) =
    ArcticWeight(max(left.value, right.value))
function Base.:*(left::ArcticWeight, right::ArcticWeight)
    (left.value == -Inf || right.value == -Inf) && return zero(ArcticWeight)
    raw = left.value + right.value
    ArcticWeight(raw == Inf ? floatmax(Float64) : raw == -Inf ? -floatmax(Float64) : raw)
end
Base.:+(left::SignedTropicalWeight, right::SignedTropicalWeight) =
    SignedTropicalWeight(min(left.value, right.value))
Base.:*(left::SignedTropicalWeight, right::SignedTropicalWeight) =
    SignedTropicalWeight(isinf(left.value) || isinf(right.value) ? Inf : left.value + right.value)
Base.:+(left::CountWeight, right::CountWeight) =
    CountWeight(Base.Checked.checked_add(left.value, right.value))
Base.:*(left::CountWeight, right::CountWeight) =
    CountWeight(Base.Checked.checked_mul(left.value, right.value))
Base.:+(left::BooleanWeight, right::BooleanWeight) =
    BooleanWeight(left.value || right.value)
Base.:*(left::BooleanWeight, right::BooleanWeight) =
    BooleanWeight(left.value && right.value)
Base.:(==)(left::W, right::W) where {W<:AbstractScalarWeight} =
    left.value == right.value
Base.iszero(weight::AbstractScalarWeight) = weight == zero(typeof(weight))
Base.isone(weight::AbstractScalarWeight) = weight == one(typeof(weight))

"""A dense, optionally frozen vocabulary mapping strings to byte or u64 labels."""
mutable struct SymbolTable{L<:Union{UInt8,UInt64}}
    symbols::Vector{String}
    labels::Dict{String,L}
    frozen::Bool
end
SymbolTable{L}() where {L<:Union{UInt8,UInt64}} =
    SymbolTable{L}(String[], Dict{String,L}(), false)
function SymbolTable{L}(symbols) where {L<:Union{UInt8,UInt64}}
    table = SymbolTable{L}()
    for value in symbols
        intern!(table, value)
    end
    table
end

"""Return `value`'s dense label, inserting it unless `table` is frozen."""
function intern!(table::SymbolTable{L}, value::AbstractString) where {L}
    text = String(value)
    haskey(table.labels, text) && return table.labels[text]
    table.frozen && throw(ArgumentError("cannot intern a new symbol after freeze!"))
    index = length(table.symbols)
    index <= typemax(L) || throw(OverflowError("symbol table exhausted $L labels"))
    id = L(index)
    push!(table.symbols, text)
    table.labels[text] = id
    id
end
"""Return the existing dense label for `value`, or throw `KeyError`."""
label(table::SymbolTable, value::AbstractString) = get(table.labels, value) do
    throw(KeyError(value))
end
"""Return the symbol assigned to a dense label, or throw `KeyError`."""
function symbol(table::SymbolTable{L}, value::L) where {L}
    index = Int(value) + 1
    1 <= index <= length(table.symbols) || throw(KeyError(value))
    table.symbols[index]
end
"""Prevent future symbols from being interned and return `table`."""
freeze!(table::SymbolTable) = (table.frozen = true; table)
"""Return whether `table` refuses new symbols."""
isfrozen(table::SymbolTable) = table.frozen
Base.length(table::SymbolTable) = length(table.symbols)
Base.isempty(table::SymbolTable) = isempty(table.symbols)
Base.getindex(table::SymbolTable, value::AbstractString) = label(table, value)
Base.getindex(table::SymbolTable{L}, value::L) where {L} = symbol(table, value)
Base.iterate(table::SymbolTable{L}, state::Int=1) where {L} =
    state > length(table.symbols) ? nothing :
    ((L(state - 1) => table.symbols[state]), state + 1)
function Base.copy(table::SymbolTable{L}) where {L}
    SymbolTable{L}(copy(table.symbols), copy(table.labels), table.frozen)
end

function frozen_copy(table::Union{Nothing,SymbolTable{L}}) where {L}
    isnothing(table) && return nothing
    freeze!(copy(table))
end

"""Mutable builder parameterized by label and built-in scalar-weight types."""
mutable struct WfstBuilder{L,W<:AbstractScalarWeight,S1,S2}
    handle::Ptr{Cvoid}
    closed::Bool
    input_symbols::S1
    output_symbols::S2
end

function checked_symbols(::Type{L}, table) where {L}
    isnothing(table) && return nothing
    L in (UInt8, UInt64) || throw(ArgumentError(
        "symbol tables apply only to UInt8 and UInt64 label domains"))
    table isa SymbolTable{L} || throw(ArgumentError(
        "symbol table label type does not match $L"))
    copy(table)
end

function WfstBuilder{L,W}(; size_hint::Integer=0,
    input_symbols=nothing, output_symbols=nothing) where {L,W<:AbstractScalarWeight}
    size_hint >= 0 || throw(ArgumentError("size_hint cannot be negative"))
    input_table = checked_symbols(L, input_symbols)
    output_table = checked_symbols(L, output_symbols)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    if L === Char && W === TropicalWeight
        checked(ccall(native(:lling_wfst_builder_new), UInt32,
            (Ref{Ptr{Cvoid}},), output), :wfst_builder_new)
    else
        checked(ccall(native(:lling_wfst_builder_new_for_domains), UInt32,
            (UInt32, UInt32, Ref{Ptr{Cvoid}}), UInt32(unit_domain(L)),
            UInt32(weight_domain(W)), output), :wfst_builder_new_for_domains)
    end
    builder = WfstBuilder{L,W,typeof(input_table),typeof(output_table)}(
        output[], false, input_table, output_table)
    finalizer(finalize_close, builder)
    size_hint == 0 || reserve_states!(builder, size_hint)
    builder
end
WfstBuilder(; kwargs...) = WfstBuilder{Char,TropicalWeight}(; kwargs...)

function open_handle(builder::WfstBuilder)
    builder.closed && throw(NativeError(STATUS_CLOSED, :builder, "builder is closed"))
    builder.handle
end

"""Reserve capacity for `additional` states without changing the graph."""
function reserve_states!(builder::WfstBuilder, additional::Integer)
    additional >= 0 || throw(ArgumentError("additional cannot be negative"))
    checked(ccall(native(:lling_wfst_builder_reserve_states), UInt32,
        (Ptr{Cvoid}, Csize_t), open_handle(builder), additional), :reserve_states)
    builder
end

"""Append one state and return its zero-based identifier."""
function add_state!(builder::WfstBuilder)
    output = Ref{UInt32}(0)
    checked(ccall(native(:lling_wfst_builder_add_state), UInt32,
        (Ptr{Cvoid}, Ref{UInt32}), open_handle(builder), output), :add_state)
    output[]
end

"""Set the builder's start-state identifier."""
function set_start!(builder::WfstBuilder, state::Integer)
    checked(ccall(native(:lling_wfst_builder_set_start), UInt32,
        (Ptr{Cvoid}, UInt32), open_handle(builder), state), :set_start)
    builder
end

typed_weight(::Type{W}, weight::W) where {W<:AbstractScalarWeight} = weight
typed_weight(::Type{W}, weight::Real) where {W<:AbstractScalarWeight} = W(weight)

"""Mark `state` final with a value from the builder's scalar semiring."""
function set_final!(builder::WfstBuilder{L,W}, state::Integer,
    weight=one(W)) where {L,W}
    value = typed_weight(W, weight)
    checked(ccall(native(:lling_wfst_builder_set_final), UInt32,
        (Ptr{Cvoid}, UInt32, Float64), open_handle(builder), state,
        raw_weight(value)), :set_final)
    builder
end

"""Remove finality and its weight from `state`."""
function clear_final!(builder::WfstBuilder, state::Integer)
    checked(ccall(native(:lling_wfst_builder_clear_final), UInt32,
        (Ptr{Cvoid}, UInt32), open_handle(builder), state), :clear_final)
    builder
end

wire_label(::Type{L}, ::Nothing, symbols) where {L} = (UInt64(0), UInt8(0))
wire_label(::Type{Char}, value::Char, symbols) = (UInt64(value), UInt8(1))
function wire_label(::Type{Char}, value::AbstractString, symbols)
    characters = collect(value)
    length(characters) == 1 || throw(ArgumentError(
        "a Unicode WFST label is exactly one scalar"))
    wire_label(Char, only(characters), symbols)
end
function wire_label(::Type{Char}, value::Integer, symbols)
    value >= 0 || throw(ArgumentError("a Unicode WFST label cannot be negative"))
    scalar = UInt32(value)
    isvalid(Char, scalar) || throw(ArgumentError("label is not a Unicode scalar"))
    (UInt64(scalar), UInt8(1))
end
function wire_label(::Type{UInt8}, value::Integer, symbols)
    0 <= value <= typemax(UInt8) || throw(ArgumentError("byte label is outside 0:255"))
    (UInt64(value), UInt8(1))
end
function wire_label(::Type{UInt64}, value::Integer, symbols)
    value >= 0 || throw(ArgumentError("u64 label cannot be negative"))
    value <= typemax(UInt64) || throw(ArgumentError("u64 label is out of range"))
    (UInt64(value), UInt8(1))
end
function wire_label(::Type{L}, value::AbstractString,
    symbols::Union{Nothing,SymbolTable{L}}) where {L<:Union{UInt8,UInt64}}
    isnothing(symbols) && throw(ArgumentError(
        "string labels in the $L domain require a SymbolTable{$L}"))
    (UInt64(intern!(symbols, value)), UInt8(1))
end

"""
Append an arc from `from` to `target`.

Labels use the builder's `UInt8`, `Char`, or `UInt64` domain; strings are
interned through the corresponding tape's symbol table. `nothing` is epsilon.
The default weight is the selected semiring's multiplicative identity.
"""
function add_arc!(builder::WfstBuilder{L,W}, from::Integer, input, output,
    target::Integer, weight=one(W)) where {L,W}
    handle = open_handle(builder)
    input_value, has_input = wire_label(L, input, builder.input_symbols)
    output_value, has_output = wire_label(L, output, builder.output_symbols)
    weight_value = typed_weight(W, weight)
    checked(ccall(native(:lling_wfst_builder_add_arc), UInt32,
        (Ptr{Cvoid}, UInt32, UInt64, UInt8, UInt64, UInt8, UInt32, Float64),
        handle, from, input_value, has_input, output_value,
        has_output, target, raw_weight(weight_value)), :add_arc)
    builder
end

"""Release an unconsumed builder; repeated calls are harmless."""
function close!(builder::WfstBuilder)
    builder.closed && return nothing
    ccall(native(:lling_wfst_builder_free), Cvoid, (Ptr{Cvoid},), builder.handle)
    builder.handle = C_NULL
    builder.closed = true
    nothing
end

Base.close(builder::WfstBuilder) = close!(builder)
Base.isopen(builder::WfstBuilder) = !builder.closed

"""Immutable type-stable view over one retained scalar-WFST resource."""
mutable struct Wfst{L,W<:AbstractScalarWeight,S1,S2}
    native::VTI.Wfst
    input_symbols::S1
    output_symbols::S2
end

function Wfst(native::VTI.Wfst, ::Type{L}, ::Type{W};
    input_symbols=nothing, output_symbols=nothing) where {L,W<:AbstractScalarWeight}
    VTI.unit_domain(native) == unit_domain(L) || throw(ArgumentError(
        "resource label domain does not match $L"))
    VTI.weight_domain(native) == weight_domain(W) || throw(ArgumentError(
        "resource weight domain does not match $W"))
    input_table = frozen_copy(checked_symbols(L, input_symbols))
    output_table = frozen_copy(checked_symbols(L, output_symbols))
    Wfst{L,W,typeof(input_table),typeof(output_table)}(
        native, input_table, output_table)
end

function adopt_native_wfst(handle::Ptr{Cvoid}, ::Type{L}, ::Type{W};
    input_symbols=nothing, output_symbols=nothing) where {L,W<:AbstractScalarWeight}
    raw = Ref(VTI.VtResourceRaw(C_NULL, Ptr{VTI.VtResourceVTable}(C_NULL)))
    try
        checked(ccall(native(:lling_wfst_resource), UInt32,
            (Ptr{Cvoid}, Ref{VTI.VtResourceRaw}), handle, raw), :wfst_resource)
        graph = VTI.wfstransducer(VTI.adopt_resource(raw[]); take=true)
        try
            Wfst(graph, L, W; input_symbols, output_symbols)
        catch
            close(graph)
            rethrow()
        end
    finally
        ccall(native(:lling_wfst_free), Cvoid, (Ptr{Cvoid},), handle)
    end
end

"""Consume `builder` and return its immutable interoperable WFST."""
function build!(builder::WfstBuilder{L,W}) where {L,W}
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_wfst_builder_build), UInt32,
        (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), open_handle(builder), output), :build)
    close!(builder)
    adopt_native_wfst(output[], L, W;
        input_symbols=builder.input_symbols,
        output_symbols=builder.output_symbols)
end

raw_resource(resource::VTI.Resource) = VTI.raw_resource(resource)
raw_resource(wfst::VTI.Wfst) = VTI.raw_resource(wfst.resource)
raw_resource(wfst::Wfst) = raw_resource(wfst.native)

Base.close(wfst::Wfst) = close!(wfst)
Base.isopen(wfst::Wfst) = isopen(wfst.native)
close!(wfst::Wfst) = VTI.close!(wfst.native)
VTI.start(wfst::Wfst) = VTI.start(wfst.native)
VTI.state_count(wfst::Wfst) = VTI.state_count(wfst.native)
VTI.state_info(wfst::Wfst, state::Integer) = VTI.state_info(wfst.native, state)
VTI.arcs(wfst::Wfst, state::Integer; kwargs...) = VTI.arcs(wfst.native, state; kwargs...)
VTI.unit_domain(wfst::Wfst{L}) where {L} = unit_domain(L)
VTI.weight_domain(wfst::Wfst{L,W}) where {L,W} = weight_domain(W)
VTI.flags(wfst::Wfst) = VTI.flags(wfst.native)
function VTI.snapshot(wfst::Wfst{L,W}) where {L,W}
    Wfst(VTI.snapshot(wfst.native), L, W;
        input_symbols=wfst.input_symbols,
        output_symbols=wfst.output_symbols)
end

"""One type-stable immutable arc decoded from the family ABI."""
struct WfstArc{L,W<:AbstractScalarWeight}
    input::Union{Nothing,L}
    output::Union{Nothing,L}
    target::UInt64
    weight::W
end

"""A type-stable state snapshot with its outgoing arcs."""
struct WfstState{L,W<:AbstractScalarWeight}
    id::UInt64
    final::Bool
    final_weight::W
    arcs::Vector{WfstArc{L,W}}
end

"""Finite native resource and scheduling bounds for a snapshot-pinned path walk."""
struct PathLimits
    max_states::UInt64
    max_arcs::UInt64
    max_work::UInt64
    work_per_call::UInt64
    max_depth::UInt64
    max_paths::UInt64
end

function path_bound(value, name::Symbol; positive::Bool=false)
    value isa Integer && !(value isa Bool) && 0 <= value <= typemax(UInt64) ||
        throw(ArgumentError("$name must be an unsigned 64-bit integer"))
    positive && value == 0 && throw(ArgumentError("$name must be positive"))
    UInt64(value)
end

function PathLimits(; max_states=10_000, max_arcs=100_000,
    max_work=1_000_000, work_per_call=1_024, max_depth=256,
    max_paths=1_024)
    PathLimits(
        path_bound(max_states, :max_states; positive=true),
        path_bound(max_arcs, :max_arcs),
        path_bound(max_work, :max_work; positive=true),
        path_bound(work_per_call, :work_per_call; positive=true),
        path_bound(max_depth, :max_depth),
        path_bound(max_paths, :max_paths))
end

"""The native cursor encountered a depth or path-count bound before exhaustion."""
struct PathTruncatedError <: Exception end
Base.showerror(io::IO, ::PathTruncatedError) = print(io,
    "path traversal was truncated by max_depth or max_paths; result is incomplete")

"""The caller's cooperative cancellation stopped a native path cursor."""
struct PathCancelledError <: Exception
    reason::Union{Nothing,CancellationReasonV2}
end
Base.showerror(io::IO, error::PathCancelledError) =
    print(io, "path traversal cancelled", isnothing(error.reason) ? "" :
        " ($(error.reason))")

"""One domain-typed scalar arc and its source state along an accepting path."""
struct WfstPathStep{L,W<:AbstractScalarWeight}
    from::UInt64
    input::Union{Nothing,L}
    output::Union{Nothing,L}
    target::UInt64
    weight::W
end

"""One owned accepting path whose weight includes the final-state weight."""
struct WfstPath{L,W<:AbstractScalarWeight}
    steps::Vector{WfstPathStep{L,W}}
    final_state::UInt64
    weight::W
end

"""A single bounded native poll made progress but has not yielded a path yet."""
struct PathPending end
const PATH_PENDING = PathPending()

struct RawPathConfig
    struct_size::UInt32
    version::UInt32
    max_states::UInt64
    max_arcs::UInt64
    max_work::UInt64
    work_per_call::UInt64
    max_depth::UInt64
    max_paths::UInt64
end
RawPathConfig(limits::PathLimits) = RawPathConfig(UInt32(sizeof(RawPathConfig)),
    UInt32(1), limits.max_states, limits.max_arcs, limits.max_work,
    limits.work_per_call, limits.max_depth, limits.max_paths)

struct RawPathStep
    from_state::UInt64
    arc::VTI.VtWfstArc
end

"""
Lazy Julia iterator over a native, snapshot-pinned, bounded path cursor.

Closing the source WFST after construction does not invalidate this cursor.
Close the iterator when stopping early; normal exhaustion closes it automatically.
The cursor is mutable and must not be advanced concurrently.
"""
mutable struct PathIterator{L,W<:AbstractScalarWeight,S1,S2}
    handle::Ptr{Cvoid}
    input_symbols::S1
    output_symbols::S2
    cancellation::Union{Nothing,CancellationV2}
    closed::Bool
    completion::Union{Nothing,UInt32}
end

Base.IteratorSize(::Type{<:PathIterator}) = Base.SizeUnknown()
Base.eltype(::Type{<:PathIterator{L,W}}) where {L,W} = WfstPath{L,W}
Base.isopen(iterator::PathIterator) = !iterator.closed

function close!(iterator::PathIterator)
    iterator.closed && return nothing
    ccall(native(:lling_path_cursor_free), Cvoid, (Ptr{Cvoid},), iterator.handle)
    iterator.handle = C_NULL
    iterator.closed = true
    nothing
end
Base.close(iterator::PathIterator) = close!(iterator)

"""
Create a bounded, lazy iterator of accepting paths in native arc-insertion order.

The provider is snapshotted once. Repeated states and cycles are allowed up to
`limits.max_depth`; reaching a depth or path count limit throws
`PathTruncatedError` rather than silently reporting exact completion. The
total work, distinct-state, and aggregate-arc limits raise `NativeError` with
`STATUS_LIMIT_EXCEEDED`. Native work is divided into `work_per_call` slices.
"""
function paths(source::Wfst{L,W}; limits::PathLimits=PathLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing) where {L,W}
    isnothing(cancellation) || open_handle(cancellation)
    raw = Ref(raw_resource(source))
    config = Ref(RawPathConfig(limits))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve source begin
        checked(ccall(native(:lling_path_cursor_open), UInt32,
            (Ref{VTI.VtResourceRaw}, Ref{RawPathConfig}, Ref{Ptr{Cvoid}}),
            raw, config, output), :path_cursor_open)
    end
    iterator = PathIterator{L,W,typeof(source.input_symbols),
        typeof(source.output_symbols)}(output[], source.input_symbols,
        source.output_symbols, cancellation, false, nothing)
    finalizer(finalize_close, iterator)
    iterator
end

function read_owned_path(::Type{L}, ::Type{W}, handle::Ptr{Cvoid}) where {L,W}
    final_state = Ref{UInt64}(0)
    weight = Ref{Float64}(0)
    count = Ref{Csize_t}(0)
    checked(ccall(native(:lling_path_info), UInt32,
        (Ptr{Cvoid}, Ref{UInt64}, Ref{Float64}, Ref{Csize_t}),
        handle, final_state, weight, count), :path_info)
    count[] <= typemax(Int) || throw(OverflowError("path length exceeds Julia indexing"))
    total = Int(count[])
    steps = Vector{WfstPathStep{L,W}}(undef, total)
    offset = 0
    while offset < total
        page = Vector{RawPathStep}(undef, min(256, total - offset))
        written = Ref{Csize_t}(0)
        reported = Ref{Csize_t}(0)
        checked(ccall(native(:lling_path_steps), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{RawPathStep}, Csize_t,
                Ref{Csize_t}, Ref{Csize_t}),
            handle, Csize_t(offset), page, Csize_t(length(page)),
            written, reported), :path_steps)
        reported[] == count[] && 0 < written[] <= length(page) ||
            throw(ArgumentError("native path page returned inconsistent counts"))
        for index in 1:Int(written[])
            raw = page[index]
            arc = raw.arc
            steps[offset + index] = WfstPathStep{L,W}(
                raw.from_state,
                arc.has_input == 0 ? nothing : decode_label(L, arc.input_label),
                arc.has_output == 0 ? nothing : decode_label(L, arc.output_label),
                arc.target_state, decode_weight(W, arc.weight))
        end
        offset += Int(written[])
    end
    WfstPath{L,W}(steps, final_state[], decode_weight(W, weight[]))
end

"""
Advance one native scheduling slice of a path cursor.

Return a `WfstPath`, `PathPending`, or `nothing` (exact exhaustion). This is
the bounded-per-call primitive; normal Julia iteration repeats pending polls
until a path or terminal result is available. Each native slice performs at
most `limits.work_per_call` traversal decisions, although one provider state
expansion can read up to the cursor's remaining aggregate arc budget.
"""
function poll_path!(iterator::PathIterator{L,W}) where {L,W}
    iterator.completion === PATH_POLL_EXHAUSTED && return nothing
    iterator.closed && throw(NativeError(STATUS_CLOSED, :path_cursor_next,
        "path iterator is closed"))
    try
        poll = Ref{UInt32}(0)
        output = Ref{Ptr{Cvoid}}(C_NULL)
        cancellation = iterator.cancellation
        cancellation_handle = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
        GC.@preserve cancellation begin
            checked(ccall(native(:lling_path_cursor_next), UInt32,
                (Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt32}, Ref{Ptr{Cvoid}}),
                iterator.handle, cancellation_handle, poll, output),
                :path_cursor_next)
        end
        if poll[] == PATH_POLL_PENDING
            return PATH_PENDING
        elseif poll[] == PATH_POLL_PATH
            output[] == C_NULL && throw(ArgumentError("native path poll omitted the path"))
            try
                return read_owned_path(L, W, output[])
            finally
                ccall(native(:lling_path_free), Cvoid, (Ptr{Cvoid},), output[])
            end
        end
        iterator.completion = poll[]
        close!(iterator)
        poll[] == PATH_POLL_EXHAUSTED && return nothing
        poll[] == PATH_POLL_TRUNCATED && throw(PathTruncatedError())
        poll[] == PATH_POLL_CANCELLED && throw(PathCancelledError(
            isnothing(cancellation) ? nothing : cancellation_reason(cancellation)))
        throw(ArgumentError("native path cursor returned unknown poll value $(poll[])"))
    catch
        close!(iterator)
        rethrow()
    end
end

function Base.iterate(iterator::PathIterator, ::Nothing=nothing)
    while true
        result = poll_path!(iterator)
        result isa PathPending && continue
        result === nothing && return nothing
        return (result, nothing)
    end
end

"""Fold accepting paths lazily, closing the native cursor on every exit path."""
function reduce_paths(operation, initial, source::Wfst; kwargs...)
    iterator = paths(source; kwargs...)
    try
        result = initial
        for path in iterator
            result = operation(result, path)
        end
        result
    finally
        close(iterator)
    end
end

"""Explicit provider-callback and allocation bounds for reachable-graph capture."""
struct GraphLimits
    max_states::UInt64
    max_arcs::UInt64
    max_work::UInt64
    work_per_call::UInt64
end

function GraphLimits(; max_states=10_000, max_arcs=100_000,
    max_work=100_000, work_per_call=16)
    GraphLimits(path_bound(max_states, :max_states; positive=true),
        path_bound(max_arcs, :max_arcs),
        path_bound(max_work, :max_work; positive=true),
        path_bound(work_per_call, :work_per_call; positive=true))
end

struct RawGraphConfig
    struct_size::UInt32
    version::UInt32
    max_states::UInt64
    max_arcs::UInt64
    max_work::UInt64
    work_per_call::UInt64
end
RawGraphConfig(limits::GraphLimits) = RawGraphConfig(
    UInt32(sizeof(RawGraphConfig)), UInt32(1), limits.max_states,
    limits.max_arcs, limits.max_work, limits.work_per_call)

struct RawGraphArc
    target_local::UInt64
    arc::VTI.VtWfstArc
end

"""A bounded graph poll consumed one slice without completing capture."""
struct GraphPending end
const GRAPH_PENDING = GraphPending()

"""The caller cancelled an in-progress graph capture."""
struct GraphCancelledError <: Exception
    reason::Union{Nothing,CancellationReasonV2}
end
Base.showerror(io::IO, error::GraphCancelledError) =
    print(io, "graph capture cancelled", isnothing(error.reason) ? "" :
        " ($(error.reason))")

"""One native, snapshot-owning, resumable reachable-graph capture."""
mutable struct GraphCaptureCursor{L,W<:AbstractScalarWeight,S1,S2}
    handle::Ptr{Cvoid}
    input_symbols::S1
    output_symbols::S2
    cancellation::Union{Nothing,CancellationV2}
    closed::Bool
end
Base.isopen(cursor::GraphCaptureCursor) = !cursor.closed
function close!(cursor::GraphCaptureCursor)
    cursor.closed && return nothing
    ccall(native(:lling_graph_cursor_free), Cvoid, (Ptr{Cvoid},), cursor.handle)
    cursor.handle = C_NULL
    cursor.closed = true
    nothing
end
Base.close(cursor::GraphCaptureCursor) = close!(cursor)

"""A complete, compact graph independent of the captured provider resource."""
mutable struct GraphSnapshot{L,W<:AbstractScalarWeight,S1,S2}
    handle::Ptr{Cvoid}
    input_symbols::S1
    output_symbols::S2
    closed::Bool
end
Base.isopen(graph::GraphSnapshot) = !graph.closed
function close!(graph::GraphSnapshot)
    graph.closed && return nothing
    ccall(native(:lling_graph_free), Cvoid, (Ptr{Cvoid},), graph.handle)
    graph.handle = C_NULL
    graph.closed = true
    nothing
end
Base.close(graph::GraphSnapshot) = close!(graph)

"""
Begin bounded breadth-first capture of all states reachable from the start.

The native cursor owns one immutable snapshot after this call; `source` may
close. No state-count callback is needed. Local IDs are assigned in breadth-
first discovery order, with each state's arcs retained in provider order.
"""
function capture_graph(source::Wfst{L,W}; limits::GraphLimits=GraphLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing) where {L,W}
    isnothing(cancellation) || open_handle(cancellation)
    raw = Ref(raw_resource(source))
    config = Ref(RawGraphConfig(limits))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve source begin
        checked(ccall(native(:lling_graph_cursor_open), UInt32,
            (Ref{VTI.VtResourceRaw}, Ref{RawGraphConfig}, Ref{Ptr{Cvoid}}),
            raw, config, output), :graph_cursor_open)
    end
    cursor = GraphCaptureCursor{L,W,typeof(source.input_symbols),
        typeof(source.output_symbols)}(output[], source.input_symbols,
        source.output_symbols, cancellation, false)
    finalizer(finalize_close, cursor)
    cursor
end

"""
Advance at most `GraphLimits.work_per_call` provider callbacks.

Return `GraphPending` or one complete `GraphSnapshot`. A state is paged at
at most 256 arcs per provider call; cancellation is checked between calls.
The provider's own callback latency is outside this bound. Limit failures
and cancellation never return a partial graph as an exact result.
"""
function poll_graph!(cursor::GraphCaptureCursor{L,W,S1,S2}) where {L,W,S1,S2}
    cursor.closed && throw(NativeError(STATUS_CLOSED, :graph_cursor_next,
        "graph capture cursor is closed"))
    try
        poll = Ref{UInt32}(0)
        cancellation = cursor.cancellation
        cancellation_handle = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
        GC.@preserve cancellation begin
            checked(ccall(native(:lling_graph_cursor_next), UInt32,
                (Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt32}),
                cursor.handle, cancellation_handle, poll), :graph_cursor_next)
        end
        poll[] == GRAPH_POLL_PENDING && return GRAPH_PENDING
        if poll[] == GRAPH_POLL_COMPLETE
            output = Ref{Ptr{Cvoid}}(C_NULL)
            checked(ccall(native(:lling_graph_cursor_take), UInt32,
                (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), cursor.handle, output),
                :graph_cursor_take)
            graph = GraphSnapshot{L,W,S1,S2}(output[], cursor.input_symbols,
                cursor.output_symbols, false)
            finalizer(finalize_close, graph)
            close!(cursor)
            return graph
        end
        close!(cursor)
        poll[] == GRAPH_POLL_CANCELLED && throw(GraphCancelledError(
            isnothing(cancellation) ? nothing : cancellation_reason(cancellation)))
        throw(ArgumentError("native graph cursor returned unknown poll value $(poll[])"))
    catch
        close!(cursor)
        rethrow()
    end
end

"""Finish bounded capture, always closing the cursor on exit."""
function complete_graph(source::Wfst; kwargs...)
    cursor = capture_graph(source; kwargs...)
    try
        while true
            result = poll_graph!(cursor)
            result isa GraphPending || return result
        end
    finally
        close(cursor)
    end
end

function open_graph_handle(graph::GraphSnapshot)
    graph.closed && throw(NativeError(STATUS_CLOSED, :graph,
        "complete graph is closed"))
    graph.handle
end

"""Return exact domains, original start ID, and reachable state/arc counts."""
function graph_info(graph::GraphSnapshot{L,W}) where {L,W}
    unit = Ref{UInt32}(0)
    weight = Ref{UInt32}(0)
    start = Ref{UInt64}(0)
    states = Ref{Csize_t}(0)
    arcs = Ref{Csize_t}(0)
    checked(ccall(native(:lling_graph_info), UInt32,
        (Ptr{Cvoid}, Ref{UInt32}, Ref{UInt32}, Ref{UInt64},
            Ref{Csize_t}, Ref{Csize_t}),
        open_graph_handle(graph), unit, weight, start, states, arcs), :graph_info)
    unit[] == UInt32(unit_domain(L)) && weight[] == UInt32(weight_domain(W)) ||
        throw(ArgumentError("captured graph domains changed"))
    (start_raw=start[], states=states[], arcs=arcs[])
end

"""One provider arc with its deterministic compact local target ID."""
struct GraphArc{L,W<:AbstractScalarWeight}
    target_local::UInt64
    input::Union{Nothing,L}
    output::Union{Nothing,L}
    target_raw::UInt64
    weight::W
end

"""One complete reachable state with its provider and local IDs."""
struct GraphState{L,W<:AbstractScalarWeight}
    local_id::UInt64
    raw_id::UInt64
    final::Bool
    final_weight::W
    arcs::Vector{GraphArc{L,W}}
end

"""Copy one state's arcs in bounded native pages and Julia-owned storage."""
function graph_arcs(graph::GraphSnapshot{L,W}, local_id::Integer) where {L,W}
    id = path_bound(local_id, :local_id)
    id <= typemax(Csize_t) || throw(ArgumentError("local_id exceeds size_t"))
    output = GraphArc{L,W}[]
    offset = 0
    total = typemax(Int)
    while offset < total
        page = Vector{RawGraphArc}(undef, 256)
        written = Ref{Csize_t}(0)
        reported = Ref{Csize_t}(0)
        checked(ccall(native(:lling_graph_arcs), UInt32,
            (Ptr{Cvoid}, Csize_t, Csize_t, Ptr{RawGraphArc},
                Csize_t, Ref{Csize_t}, Ref{Csize_t}),
            open_graph_handle(graph), Csize_t(id), Csize_t(offset),
            page, Csize_t(length(page)), written, reported), :graph_arcs)
        reported[] <= typemax(Int) || throw(OverflowError("arc count exceeds Julia indexing"))
        total = Int(reported[])
        0 <= written[] <= length(page) ||
            throw(ArgumentError("native graph arc page returned invalid count"))
        for raw in @view page[1:Int(written[])]
            arc = raw.arc
            push!(output, GraphArc{L,W}(raw.target_local,
                arc.has_input == 0 ? nothing : decode_label(L, arc.input_label),
                arc.has_output == 0 ? nothing : decode_label(L, arc.output_label),
                arc.target_state, decode_weight(W, arc.weight)))
        end
        written[] == 0 && offset < total &&
            throw(ArgumentError("native graph arc page made no progress"))
        offset += Int(written[])
    end
    output
end

"""Return one complete graph state, indexed by zero-based local ID."""
function graph_state(graph::GraphSnapshot{L,W}, local_id::Integer) where {L,W}
    id = path_bound(local_id, :local_id)
    id <= typemax(Csize_t) || throw(ArgumentError("local_id exceeds size_t"))
    raw = Ref{UInt64}(0)
    final = Ref{UInt8}(0)
    weight = Ref{Float64}(0)
    count = Ref{Csize_t}(0)
    checked(ccall(native(:lling_graph_state), UInt32,
        (Ptr{Cvoid}, Csize_t, Ref{UInt64}, Ref{UInt8},
            Ref{Float64}, Ref{Csize_t}),
        open_graph_handle(graph), Csize_t(id), raw, final, weight, count),
        :graph_state)
    arcs = graph_arcs(graph, id)
    length(arcs) == count[] || throw(ArgumentError("native graph arc count changed"))
    GraphState{L,W}(id, raw[], final[] == 1, decode_weight(W, weight[]), arcs)
end

"""Explicit total and per-call native graph-analysis work limits."""
struct DistanceLimits
    max_work::UInt64
    work_per_call::UInt64
end
DistanceLimits(; max_work=1_000_000, work_per_call=64) =
    DistanceLimits(path_bound(max_work, :max_work; positive=true),
        path_bound(work_per_call, :work_per_call; positive=true))

struct RawDistanceConfig
    struct_size::UInt32
    version::UInt32
    max_work::UInt64
    work_per_call::UInt64
end
RawDistanceConfig(limits::DistanceLimits) = RawDistanceConfig(
    UInt32(sizeof(RawDistanceConfig)), UInt32(1),
    limits.max_work, limits.work_per_call)

"""One bounded poll completed without exhausting the distance machine."""
struct DistancePending end
const DISTANCE_PENDING = DistancePending()

"""Cancellation ended a distance analysis before exact completion."""
struct DistanceCancelledError <: Exception
    reason::Union{Nothing,CancellationReasonV2}
end
Base.showerror(io::IO, error::DistanceCancelledError) =
    print(io, "graph distance analysis cancelled", isnothing(error.reason) ? "" :
        " ($(error.reason))")

"""Mutable native analysis cursor that retains the complete graph independently."""
mutable struct DistanceCursor{W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    cancellation::Union{Nothing,CancellationV2}
    closed::Bool
end
Base.isopen(cursor::DistanceCursor) = !cursor.closed
function close!(cursor::DistanceCursor)
    cursor.closed && return nothing
    ccall(native(:lling_graph_distance_cursor_free), Cvoid, (Ptr{Cvoid},), cursor.handle)
    cursor.handle = C_NULL
    cursor.closed = true
    nothing
end
Base.close(cursor::DistanceCursor) = close!(cursor)

"""Exact semiring forward/backward vectors, independent of its graph and cursor."""
mutable struct DistanceResult{W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    closed::Bool
end
Base.isopen(result::DistanceResult) = !result.closed
function close!(result::DistanceResult)
    result.closed && return nothing
    ccall(native(:lling_graph_distance_free), Cvoid, (Ptr{Cvoid},), result.handle)
    result.handle = C_NULL
    result.closed = true
    nothing
end
Base.close(result::DistanceResult) = close!(result)

"""
Begin exact semiring distance analysis on a complete graph.

The native cursor retains its own graph lease, so `graph` may be closed after
this call. Each poll consumes at most `limits.work_per_call` vertex/edge
transitions. Acyclic graphs support all seven scalar domains. Cyclic graphs
support tropical, signed tropical, arctic, and Boolean domains; an improving
cycle returns `STATUS_NON_CONVERGENT`, while cyclic probability/log/count sums
return `STATUS_UNSUPPORTED` instead of a truncated value.
"""
function analyze_distances(graph::GraphSnapshot{L,W};
    limits::DistanceLimits=DistanceLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing) where {L,W}
    isnothing(cancellation) || open_handle(cancellation)
    config = Ref(RawDistanceConfig(limits))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_graph_distance_open), UInt32,
        (Ptr{Cvoid}, Ref{RawDistanceConfig}, Ref{Ptr{Cvoid}}),
        open_graph_handle(graph), config, output), :graph_distance_open)
    cursor = DistanceCursor{W}(output[], cancellation, false)
    finalizer(finalize_close, cursor)
    cursor
end

"""Advance a distance cursor by one bounded native work slice."""
function poll_distance!(cursor::DistanceCursor{W}) where {W}
    cursor.closed && throw(NativeError(STATUS_CLOSED, :graph_distance_next,
        "graph distance cursor is closed"))
    try
        poll = Ref{UInt32}(0)
        cancellation = cursor.cancellation
        cancellation_handle = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
        GC.@preserve cancellation begin
            checked(ccall(native(:lling_graph_distance_next), UInt32,
                (Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt32}),
                cursor.handle, cancellation_handle, poll), :graph_distance_next)
        end
        poll[] == DISTANCE_POLL_PENDING && return DISTANCE_PENDING
        if poll[] == DISTANCE_POLL_COMPLETE
            output = Ref{Ptr{Cvoid}}(C_NULL)
            checked(ccall(native(:lling_graph_distance_take), UInt32,
                (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), cursor.handle, output),
                :graph_distance_take)
            result = DistanceResult{W}(output[], false)
            finalizer(finalize_close, result)
            close!(cursor)
            return result
        end
        close!(cursor)
        poll[] == DISTANCE_POLL_CANCELLED && throw(DistanceCancelledError(
            isnothing(cancellation) ? nothing : cancellation_reason(cancellation)))
        throw(ArgumentError("native distance cursor returned unknown poll value $(poll[])"))
    catch
        close!(cursor)
        rethrow()
    end
end

"""Finish exact distance analysis, closing the cursor on every exit path."""
function complete_distances(graph::GraphSnapshot; kwargs...)
    cursor = analyze_distances(graph; kwargs...)
    try
        while true
            result = poll_distance!(cursor)
            result isa DistancePending || return result
        end
    finally
        close(cursor)
    end
end

"""Explicit lifetime and per-call bounds for best-first accepting paths."""
struct RankedPathLimits
    max_work::UInt64
    work_per_call::UInt64
    max_depth::UInt64
    max_paths::UInt64
    max_frontier::UInt64
end
RankedPathLimits(; max_work=1_000_000, work_per_call=64,
    max_depth=1024, max_paths=10_000, max_frontier=10_000) = RankedPathLimits(
    path_bound(max_work, :max_work; positive=true),
    path_bound(work_per_call, :work_per_call; positive=true),
    path_bound(max_depth, :max_depth), path_bound(max_paths, :max_paths),
    path_bound(max_frontier, :max_frontier; positive=true))

struct RawRankedPathConfig
    struct_size::UInt32
    version::UInt32
    max_work::UInt64
    work_per_call::UInt64
    max_depth::UInt64
    max_paths::UInt64
    max_frontier::UInt64
end
RawRankedPathConfig(limits::RankedPathLimits) = RawRankedPathConfig(
    UInt32(sizeof(RawRankedPathConfig)), UInt32(1), limits.max_work,
    limits.work_per_call, limits.max_depth, limits.max_paths, limits.max_frontier)

"""One ranked-search poll consumed its bounded slice without yielding."""
struct RankedPathPending end
const RANKED_PATH_PENDING = RankedPathPending()

"""Mutable lazy best-first iterator retaining one immutable graph lease."""
mutable struct RankedPathIterator{L,W<:AbstractScalarWeight,S1,S2}
    handle::Ptr{Cvoid}
    input_symbols::S1
    output_symbols::S2
    cancellation::Union{Nothing,CancellationV2}
    closed::Bool
    completion::Union{Nothing,UInt32}
end
Base.IteratorSize(::Type{<:RankedPathIterator}) = Base.SizeUnknown()
Base.eltype(::Type{<:RankedPathIterator{L,W}}) where {L,W} = WfstPath{L,W}
Base.isopen(iterator::RankedPathIterator) = !iterator.closed
function close!(iterator::RankedPathIterator)
    iterator.closed && return nothing
    ccall(native(:lling_ranked_path_cursor_free), Cvoid,
        (Ptr{Cvoid},), iterator.handle)
    iterator.handle = C_NULL
    iterator.closed = true
    nothing
end
Base.close(iterator::RankedPathIterator) = close!(iterator)

"""
Create a bounded, lazy best-first iterator over accepting paths.

Paths are ordered by native Viterbi cost, then length, then captured provider
arc order. Equal-cost ties are deterministic. A complete graph snapshot is
required; the cursor retains it, so the graph may close after this call.
`max_depth`/`max_paths` truncation and work/frontier exhaustion are explicit,
never silently treated as exact completion. A nonconvergent improving cycle
raises `STATUS_NON_CONVERGENT` before any path is yielded.
"""
function ranked_paths(graph::GraphSnapshot{L,W};
    limits::RankedPathLimits=RankedPathLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing) where {L,W}
    isnothing(cancellation) || open_handle(cancellation)
    config = Ref(RawRankedPathConfig(limits))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_ranked_path_cursor_open), UInt32,
        (Ptr{Cvoid}, Ref{RawRankedPathConfig}, Ref{Ptr{Cvoid}}),
        open_graph_handle(graph), config, output), :ranked_path_cursor_open)
    iterator = RankedPathIterator{L,W,typeof(graph.input_symbols),
        typeof(graph.output_symbols)}(output[], graph.input_symbols,
        graph.output_symbols, cancellation, false, nothing)
    finalizer(finalize_close, iterator)
    iterator
end

"""Advance one bounded native best-first slice; return a path or pending."""
function poll_ranked_path!(iterator::RankedPathIterator{L,W}) where {L,W}
    iterator.completion === RANKED_POLL_EXHAUSTED && return nothing
    iterator.closed && throw(NativeError(STATUS_CLOSED, :ranked_path_cursor_next,
        "ranked path iterator is closed"))
    try
        poll = Ref{UInt32}(0)
        output = Ref{Ptr{Cvoid}}(C_NULL)
        cancellation = iterator.cancellation
        cancellation_handle = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
        GC.@preserve cancellation begin
            checked(ccall(native(:lling_ranked_path_cursor_next), UInt32,
                (Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt32}, Ref{Ptr{Cvoid}}),
                iterator.handle, cancellation_handle, poll, output),
                :ranked_path_cursor_next)
        end
        poll[] == RANKED_POLL_PENDING && return RANKED_PATH_PENDING
        if poll[] == RANKED_POLL_PATH
            output[] == C_NULL && throw(ArgumentError("native ranked poll omitted path"))
            try
                return read_owned_path(L, W, output[])
            finally
                ccall(native(:lling_path_free), Cvoid, (Ptr{Cvoid},), output[])
            end
        end
        iterator.completion = poll[]
        close!(iterator)
        poll[] == RANKED_POLL_EXHAUSTED && return nothing
        poll[] == RANKED_POLL_TRUNCATED && throw(PathTruncatedError())
        poll[] == RANKED_POLL_CANCELLED && throw(PathCancelledError(
            isnothing(cancellation) ? nothing : cancellation_reason(cancellation)))
        throw(ArgumentError("native ranked cursor returned unknown poll value $(poll[])"))
    catch
        close!(iterator)
        rethrow()
    end
end

function Base.iterate(iterator::RankedPathIterator, ::Nothing=nothing)
    while true
        result = poll_ranked_path!(iterator)
        result isa RankedPathPending && continue
        result === nothing && return nothing
        return (result, nothing)
    end
end

"""Fold ranked paths lazily and release the cursor on every exit path."""
function reduce_ranked_paths(operation, initial, graph::GraphSnapshot; kwargs...)
    iterator = ranked_paths(graph; kwargs...)
    try
        result = initial
        for path in iterator
            result = operation(result, path)
        end
        result
    finally
        close(iterator)
    end
end

"""Return the best accepting path, or `nothing` when none exists exactly."""
function best_path(graph::GraphSnapshot; kwargs...)
    cursor = ranked_paths(graph; kwargs...)
    try
        result = iterate(cursor)
        isnothing(result) ? nothing : first(result)
    finally
        close(cursor)
    end
end

"""Return up to `k` ranked paths, explicitly bounded by `limits.max_paths`."""
function k_best_paths(graph::GraphSnapshot{L,W}, k::Integer;
    limits::RankedPathLimits=RankedPathLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing) where {L,W}
    requested = path_bound(k, :k)
    requested <= limits.max_paths || throw(ArgumentError(
        "k exceeds ranked path max_paths; raise that explicit bound"))
    result = WfstPath{L,W}[]
    requested == 0 && return result
    cursor = ranked_paths(graph; limits, cancellation)
    try
        for path in cursor
            push!(result, path)
            length(result) == requested && break
        end
        result
    finally
        close(cursor)
    end
end

"""Alias for `k_best_paths` with the same ordering and explicit bounds."""
n_best_paths(graph::GraphSnapshot, n::Integer; kwargs...) =
    k_best_paths(graph, n; kwargs...)

path_order_cost(weight::Union{TropicalWeight,LogWeight,SignedTropicalWeight}) =
    weight.value
path_order_cost(weight::ArcticWeight) = -weight.value
path_order_cost(weight::ProbabilityWeight) = -log(weight.value)
path_order_cost(weight::CountWeight) = log(Float64(weight.value))
path_order_cost(::BooleanWeight) = 0.0

"""A lazy exact cost-window filter over native best-first accepting paths."""
mutable struct CostPrunedPathIterator{I<:RankedPathIterator}
    source::I
    beam::Float64
    best_cost::Union{Nothing,Float64}
    closed::Bool
    exhausted::Bool
end
Base.IteratorSize(::Type{<:CostPrunedPathIterator}) = Base.SizeUnknown()
Base.eltype(::Type{CostPrunedPathIterator{I}}) where {I} = Base.eltype(I)
Base.isopen(iterator::CostPrunedPathIterator) = !iterator.closed
function close!(iterator::CostPrunedPathIterator)
    iterator.closed && return nothing
    close(iterator.source)
    iterator.closed = true
    nothing
end
Base.close(iterator::CostPrunedPathIterator) = close!(iterator)

"""
Lazily retain paths whose native Viterbi cost is within `beam` of the best.

This is exact complete-path cost-window pruning, not an approximate
partial-hypothesis beam search. Native best-first ordering makes the first
out-of-window path a sound stopping point. Each poll does at most one ranked
native poll, so the declared work and frontier bounds remain effective.
"""
function cost_pruned_paths(graph::GraphSnapshot;
    beam::Real,
    limits::RankedPathLimits=RankedPathLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing)
    width = Float64(beam)
    isfinite(width) && width >= 0.0 || throw(ArgumentError(
        "beam must be finite and nonnegative"))
    source = ranked_paths(graph; limits, cancellation)
    iterator = CostPrunedPathIterator{typeof(source)}(
        source, width, nothing, false, false)
    finalizer(finalize_close, iterator)
    iterator
end

"""Advance one bounded ranked-search slice through the cost-window filter."""
function poll_cost_pruned_path!(iterator::CostPrunedPathIterator)
    iterator.exhausted && return nothing
    iterator.closed && throw(NativeError(STATUS_CLOSED, :ranked_path_cursor_next,
        "cost-pruned path iterator is closed"))
    try
        result = poll_ranked_path!(iterator.source)
        result isa RankedPathPending && return result
        if result === nothing
            iterator.exhausted = true
            close(iterator)
            return nothing
        end
        cost = path_order_cost(result.weight)
        isfinite(cost) || throw(ArgumentError(
            "native ranked path has a non-finite Viterbi cost"))
        if isnothing(iterator.best_cost)
            iterator.best_cost = cost
        elseif cost - iterator.best_cost > iterator.beam
            iterator.exhausted = true
            close(iterator)
            return nothing
        end
        result
    catch
        close(iterator)
        rethrow()
    end
end

function Base.iterate(iterator::CostPrunedPathIterator, ::Nothing=nothing)
    while true
        result = poll_cost_pruned_path!(iterator)
        result isa RankedPathPending && continue
        result === nothing && return nothing
        return (result, nothing)
    end
end

"""Fold the exact cost-window path stream and close it on every exit path."""
function reduce_cost_pruned_paths(operation, initial, graph::GraphSnapshot; kwargs...)
    iterator = cost_pruned_paths(graph; kwargs...)
    try
        result = initial
        for path in iterator
            result = operation(result, path)
        end
        result
    finally
        close(iterator)
    end
end

"""Explicit work, depth, count, strategy, and seed for accepting-path draws."""
struct SamplePathLimits
    max_work::UInt64
    work_per_call::UInt64
    max_depth::UInt64
    max_samples::UInt64
    strategy::UInt32
    seed::UInt64
end
function SamplePathLimits(; max_work=1_000_000, work_per_call=64,
    max_depth=1024, max_samples=10_000, strategy=:uniform, seed=0)
    strategy_code = strategy === :uniform ? SAMPLE_UNIFORM :
        strategy === :proportional ? SAMPLE_PROPORTIONAL :
        throw(ArgumentError("strategy must be :uniform or :proportional"))
    SamplePathLimits(path_bound(max_work, :max_work; positive=true),
        path_bound(work_per_call, :work_per_call; positive=true),
        path_bound(max_depth, :max_depth),
        path_bound(max_samples, :max_samples; positive=true),
        strategy_code, path_bound(seed, :seed))
end

struct RawSamplePathConfig
    struct_size::UInt32
    version::UInt32
    max_work::UInt64
    work_per_call::UInt64
    max_depth::UInt64
    max_samples::UInt64
    strategy::UInt32
    reserved::UInt32
    seed::UInt64
end
RawSamplePathConfig(limits::SamplePathLimits) = RawSamplePathConfig(
    UInt32(sizeof(RawSamplePathConfig)), UInt32(1), limits.max_work,
    limits.work_per_call, limits.max_depth, limits.max_samples,
    limits.strategy, UInt32(0), limits.seed)

"""One sampling poll used its bounded work slice without yielding a path."""
struct SamplePathPending end
const SAMPLE_PATH_PENDING = SamplePathPending()

"""Mutable seed-stable iterator retaining a complete native graph lease."""
mutable struct SamplePathIterator{L,W<:AbstractScalarWeight,S1,S2}
    handle::Ptr{Cvoid}
    input_symbols::S1
    output_symbols::S2
    cancellation::Union{Nothing,CancellationV2}
    closed::Bool
    completion::Union{Nothing,UInt32}
end
Base.IteratorSize(::Type{<:SamplePathIterator}) = Base.SizeUnknown()
Base.eltype(::Type{<:SamplePathIterator{L,W}}) where {L,W} = WfstPath{L,W}
Base.isopen(iterator::SamplePathIterator) = !iterator.closed
function close!(iterator::SamplePathIterator)
    iterator.closed && return nothing
    ccall(native(:lling_sample_path_cursor_free), Cvoid,
        (Ptr{Cvoid},), iterator.handle)
    iterator.handle = C_NULL
    iterator.closed = true
    nothing
end
Base.close(iterator::SamplePathIterator) = close!(iterator)

"""
Draw accepting paths lazily from one complete graph with a stable seed.

`:uniform` draws every finite accepting path equally, ignoring scalar weights;
`:proportional` uses exact backward masses for probability, log, or count
weights. Unsupported cycles or domains fail explicitly. Close early when no
longer needed. A sample-count cap raises `PathTruncatedError`, not exhaustion.
"""
function sample_paths(graph::GraphSnapshot{L,W};
    limits::SamplePathLimits=SamplePathLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing) where {L,W}
    isnothing(cancellation) || open_handle(cancellation)
    config = Ref(RawSamplePathConfig(limits))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_sample_path_cursor_open), UInt32,
        (Ptr{Cvoid}, Ref{RawSamplePathConfig}, Ref{Ptr{Cvoid}}),
        open_graph_handle(graph), config, output), :sample_path_cursor_open)
    iterator = SamplePathIterator{L,W,typeof(graph.input_symbols),
        typeof(graph.output_symbols)}(output[], graph.input_symbols,
        graph.output_symbols, cancellation, false, nothing)
    finalizer(finalize_close, iterator)
    iterator
end

"""Advance one bounded native sampling slice; return a path or pending."""
function poll_sample_path!(iterator::SamplePathIterator{L,W}) where {L,W}
    iterator.completion === SAMPLE_POLL_EXHAUSTED && return nothing
    iterator.closed && throw(NativeError(STATUS_CLOSED, :sample_path_cursor_next,
        "sample path iterator is closed"))
    try
        poll = Ref{UInt32}(0)
        output = Ref{Ptr{Cvoid}}(C_NULL)
        cancellation = iterator.cancellation
        cancellation_handle = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
        GC.@preserve cancellation begin
            checked(ccall(native(:lling_sample_path_cursor_next), UInt32,
                (Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt32}, Ref{Ptr{Cvoid}}),
                iterator.handle, cancellation_handle, poll, output),
                :sample_path_cursor_next)
        end
        poll[] == SAMPLE_POLL_PENDING && return SAMPLE_PATH_PENDING
        if poll[] == SAMPLE_POLL_PATH
            output[] == C_NULL && throw(ArgumentError("native sample poll omitted path"))
            try
                return read_owned_path(L, W, output[])
            finally
                ccall(native(:lling_path_free), Cvoid, (Ptr{Cvoid},), output[])
            end
        end
        iterator.completion = poll[]
        close!(iterator)
        poll[] == SAMPLE_POLL_EXHAUSTED && return nothing
        poll[] == SAMPLE_POLL_TRUNCATED && throw(PathTruncatedError())
        poll[] == SAMPLE_POLL_CANCELLED && throw(PathCancelledError(
            isnothing(cancellation) ? nothing : cancellation_reason(cancellation)))
        throw(ArgumentError("native sample cursor returned unknown poll value $(poll[])"))
    catch
        close!(iterator)
        rethrow()
    end
end

function Base.iterate(iterator::SamplePathIterator, ::Nothing=nothing)
    while true
        result = poll_sample_path!(iterator)
        result isa SamplePathPending && continue
        result === nothing && return nothing
        return (result, nothing)
    end
end

"""Draw one accepting path, or `nothing` when no accepting path exists."""
function sample_path(graph::GraphSnapshot; kwargs...)
    cursor = sample_paths(graph; kwargs...)
    try
        result = iterate(cursor)
        isnothing(result) ? nothing : first(result)
    finally
        close(cursor)
    end
end

"""Collect exactly `n` seeded draws or fewer when no accepting path exists."""
function sample_n_paths(graph::GraphSnapshot{L,W}, n::Integer;
    limits::SamplePathLimits=SamplePathLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing) where {L,W}
    requested = path_bound(n, :n)
    requested <= limits.max_samples || throw(ArgumentError(
        "n exceeds max_samples; raise that explicit bound"))
    result = WfstPath{L,W}[]
    requested == 0 && return result
    cursor = sample_paths(graph; limits, cancellation)
    try
        for path in cursor
            push!(result, path)
            length(result) == requested && break
        end
        result
    finally
        close(cursor)
    end
end

"""Reduce a finite prefix of seeded draws and close on every exit path."""
function reduce_sampled_paths(operation, initial, graph::GraphSnapshot, n::Integer;
    limits::SamplePathLimits=SamplePathLimits(),
    cancellation::Union{Nothing,CancellationV2}=nothing)
    requested = path_bound(n, :n)
    requested <= limits.max_samples || throw(ArgumentError(
        "n exceeds max_samples; raise that explicit bound"))
    requested == 0 && return initial
    cursor = sample_paths(graph; limits, cancellation)
    try
        result = initial
        for path in cursor
            result = operation(result, path)
            requested -= 1
            requested == 0 && break
        end
        result
    finally
        close(cursor)
    end
end

function open_distance_handle(result::DistanceResult)
    result.closed && throw(NativeError(STATUS_CLOSED, :graph_distance,
        "graph distance result is closed"))
    result.handle
end

"""Return exact total semiring weight and the number of local states."""
function distance_info(result::DistanceResult{W}) where {W}
    total = Ref{Float64}(0)
    count = Ref{Csize_t}(0)
    checked(ccall(native(:lling_graph_distance_info), UInt32,
        (Ptr{Cvoid}, Ref{Float64}, Ref{Csize_t}),
        open_distance_handle(result), total, count), :graph_distance_info)
    (total=decode_weight(W, total[]), states=count[])
end

"""Copy one page of exact forward/backward distances as Julia-owned vectors."""
function distance_page(result::DistanceResult{W}, offset::Integer;
    capacity::Integer=256) where {W}
    start = path_bound(offset, :offset)
    count = path_bound(capacity, :capacity; positive=true)
    start <= typemax(Csize_t) || throw(ArgumentError("offset exceeds size_t"))
    count <= typemax(Csize_t) || throw(ArgumentError("capacity exceeds size_t"))
    count <= 256 || throw(ArgumentError("distance page capacity exceeds 256"))
    forward_raw = Vector{Float64}(undef, Int(count))
    backward_raw = Vector{Float64}(undef, Int(count))
    written = Ref{Csize_t}(0)
    total = Ref{Csize_t}(0)
    checked(ccall(native(:lling_graph_distance_page), UInt32,
        (Ptr{Cvoid}, Csize_t, Ptr{Float64}, Ptr{Float64},
            Csize_t, Ref{Csize_t}, Ref{Csize_t}),
        open_distance_handle(result), Csize_t(start),
        forward_raw, backward_raw, Csize_t(count), written, total),
        :graph_distance_page)
    written[] <= count || throw(ArgumentError("native distance page exceeded capacity"))
    (forward=map(value -> decode_weight(W, value), @view(forward_raw[1:Int(written[])])),
        backward=map(value -> decode_weight(W, value), @view(backward_raw[1:Int(written[])])),
        total=total[])
end

"""
Return one page of accepting-path arc posterior probabilities in provider order.

Defined for finite, nonzero path mass in probability, log, and count domains.
The returned probabilities are Julia-owned `Float64` values. A numeric
underflow/overflow is an explicit native failure, not an unmarked zero.
"""
function posterior_arcs(result::DistanceResult, local_id::Integer;
    offset::Integer=0, capacity::Integer=256)
    state = path_bound(local_id, :local_id)
    start = path_bound(offset, :offset)
    count = path_bound(capacity, :capacity; positive=true)
    state <= typemax(Csize_t) || throw(ArgumentError("local_id exceeds size_t"))
    start <= typemax(Csize_t) || throw(ArgumentError("offset exceeds size_t"))
    count <= 256 || throw(ArgumentError("posterior page capacity exceeds 256"))
    values = Vector{Float64}(undef, Int(count))
    written = Ref{Csize_t}(0)
    total = Ref{Csize_t}(0)
    checked(ccall(native(:lling_graph_posterior_arcs), UInt32,
        (Ptr{Cvoid}, Csize_t, Csize_t, Ptr{Float64},
            Csize_t, Ref{Csize_t}, Ref{Csize_t}),
        open_distance_handle(result), Csize_t(state), Csize_t(start),
        values, Csize_t(count), written, total), :graph_posterior_arcs)
    written[] <= count || throw(ArgumentError("native posterior page exceeded capacity"))
    (probabilities=values[1:Int(written[])], total=total[])
end

"""Return the probability of stopping at one local final state."""
function posterior_final(result::DistanceResult, local_id::Integer)
    state = path_bound(local_id, :local_id)
    state <= typemax(Csize_t) || throw(ArgumentError("local_id exceeds size_t"))
    output = Ref{Float64}(0)
    checked(ccall(native(:lling_graph_posterior_final), UInt32,
        (Ptr{Cvoid}, Csize_t, Ref{Float64}),
        open_distance_handle(result), Csize_t(state), output),
        :graph_posterior_final)
    output[]
end

decode_label(::Type{UInt8}, value::UInt64) = UInt8(value)
decode_label(::Type{Char}, value::UInt64) = Char(UInt32(value))
decode_label(::Type{UInt64}, value::UInt64) = value
"""Return type-stable outgoing arcs for `state`, preserving epsilon as `nothing`."""
function arcs(wfst::Wfst{L,W}, state::Integer; kwargs...) where {L,W}
    [WfstArc{L,W}(
        isnothing(arc.input) ? nothing : decode_label(L, arc.input),
        isnothing(arc.output) ? nothing : decode_label(L, arc.output),
        arc.target, decode_weight(W, arc.weight))
     for arc in VTI.arcs(wfst.native, state; kwargs...)]
end
"""Return a type-stable state snapshot, or `nothing` for an unknown ID."""
function state(wfst::Wfst{L,W}, id::Integer; kwargs...) where {L,W}
    info = VTI.state_info(wfst.native, id)
    isnothing(info) && return nothing
    WfstState{L,W}(UInt64(id), info.final,
        decode_weight(W, info.final_weight), arcs(wfst, id; kwargs...))
end
"""Return the frozen input-tape symbol table, or `nothing` for raw labels."""
input_symbols(wfst::Wfst) = wfst.input_symbols
"""Return the frozen output-tape symbol table, or `nothing` for raw labels."""
output_symbols(wfst::Wfst) = wfst.output_symbols

"""Return one independent retained resource for a WFST."""
resource(wfst::VTI.Wfst) = VTI.retain(wfst.resource)
resource(wfst::Wfst) = resource(wfst.native)

"""Copy a compatible immutable resource into a native lling-llang WFST."""
function import_wfst(::Type{L}, ::Type{W},
    source::Union{VTI.Resource,VTI.Wfst,Wfst};
    input_symbols=nothing, output_symbols=nothing) where {L,W<:AbstractScalarWeight}
    output = Ref{Ptr{Cvoid}}(C_NULL)
    raw = raw_resource(source)
    checked(ccall(native(:lling_wfst_import), UInt32,
        (VTI.VtResourceRaw, Ref{Ptr{Cvoid}}), raw, output), :wfst_import)
    adopt_native_wfst(output[], L, W; input_symbols, output_symbols)
end
function import_wfst(source::Wfst{L,W}) where {L,W}
    import_wfst(L, W, source;
        input_symbols=source.input_symbols,
        output_symbols=source.output_symbols)
end
function import_wfst(source::VTI.Wfst)
    import_wfst(label_type(VTI.unit_domain(source)),
        weight_type(VTI.weight_domain(source)), source)
end
function import_wfst(source::VTI.Resource)
    probe = VTI.wfstransducer(source)
    try
        import_wfst(label_type(VTI.unit_domain(probe)),
            weight_type(VTI.weight_domain(probe)), source)
    finally
        close(probe)
    end
end

"""
Lazily compose snapshots of two domain-compatible scalar WFST resources.

The product joins the first output tape to the second input tape and combines
matching weights with the declared built-in semiring's multiplication.
"""
function compose(first::Wfst{L,W}, second::Wfst{L,W}) where {L,W}
    if !isnothing(first.output_symbols) && !isnothing(second.input_symbols) &&
        first.output_symbols.symbols != second.input_symbols.symbols
        throw(ArgumentError("composition's middle-tape symbol tables differ"))
    end
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_wfst_compose), UInt32,
        (VTI.VtResourceRaw, VTI.VtResourceRaw, Ref{Ptr{Cvoid}}),
        raw_resource(first), raw_resource(second), output), :wfst_compose)
    adopt_native_wfst(output[], L, W;
        input_symbols=first.input_symbols,
        output_symbols=second.output_symbols)
end
function compose(::Wfst{L1,W1}, ::Wfst{L2,W2}) where {L1,W1,L2,W2}
    L1 == L2 || throw(ArgumentError(
        "composition requires equal label types; received $L1 and $L2"))
    throw(ArgumentError(
        "composition requires equal weight types; received $W1 and $W2"))
end
function compose(first::VTI.Wfst, second::VTI.Wfst)
    first_unit = VTI.unit_domain(first)
    first_weight = VTI.weight_domain(first)
    first_unit == VTI.unit_domain(second) || throw(ArgumentError(
        "composition requires equal label domains"))
    first_weight == VTI.weight_domain(second) || throw(ArgumentError(
        "composition requires equal weight domains"))
    compose(Wfst(first, label_type(first_unit), weight_type(first_weight)),
        Wfst(second, label_type(first_unit), weight_type(first_weight)))
end

"""Compose a raw VinaryTreeInterop WFST with a typed lling-llang WFST."""
function compose(first::VTI.Wfst, second::Wfst{L,W}) where {L,W}
    compose(Wfst(first, L, W), second)
end

"""Compose a typed lling-llang WFST with a raw VinaryTreeInterop WFST."""
function compose(first::Wfst{L,W}, second::VTI.Wfst) where {L,W}
    compose(first, Wfst(second, L, W))
end

"""Weighted intersection of verified scalar acceptors under a required four-axis budget."""
function acceptor_intersect(first::Wfst{L,W}, second::Wfst{L,W};
    budget::BudgetV2, pointer_form::Bool=false) where {L,W}
    for graph in (first, second)
        if !isnothing(graph.input_symbols) && !isnothing(graph.output_symbols) &&
            graph.input_symbols.symbols != graph.output_symbols.symbols
            throw(ArgumentError("acceptor input/output symbol tables differ"))
        end
    end
    if !isnothing(first.input_symbols) && !isnothing(second.input_symbols) &&
        first.input_symbols.symbols != second.input_symbols.symbols
        throw(ArgumentError("acceptor intersection symbol tables differ"))
    end
    output = Ref{Ptr{Cvoid}}(C_NULL)
    first_raw, second_raw = raw_resource(first), raw_resource(second)
    status = pointer_form ?
        ccall(native(:lling_wfst_acceptor_intersect_refs), UInt32,
            (Ref{VTI.VtResourceRaw}, Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
            Ref(first_raw), Ref(second_raw), Ref(budget), output) :
        ccall(native(:lling_wfst_acceptor_intersect), UInt32,
            (VTI.VtResourceRaw, VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
            first_raw, second_raw, Ref(budget), output)
    checked(status, :wfst_acceptor_intersect)
    adopt_native_wfst(output[], L, W;
        input_symbols=first.input_symbols,
        output_symbols=first.output_symbols)
end
function acceptor_intersect(::Wfst{L1,W1}, ::Wfst{L2,W2};
    budget::BudgetV2, pointer_form::Bool=false) where {L1,W1,L2,W2}
    L1 == L2 || throw(ArgumentError("acceptor intersection requires equal label types"))
    throw(ArgumentError("acceptor intersection requires equal weight types"))
end
function acceptor_intersect(first::VTI.Wfst, second::VTI.Wfst;
    budget::BudgetV2, pointer_form::Bool=false)
    first_unit, first_weight = VTI.unit_domain(first), VTI.weight_domain(first)
    first_unit == VTI.unit_domain(second) || throw(ArgumentError(
        "acceptor intersection requires equal label domains"))
    first_weight == VTI.weight_domain(second) || throw(ArgumentError(
        "acceptor intersection requires equal weight domains"))
    acceptor_intersect(Wfst(first, label_type(first_unit), weight_type(first_weight)),
        Wfst(second, label_type(first_unit), weight_type(first_weight));
        budget=budget, pointer_form=pointer_form)
end

function unary_wfst_call(operation::Symbol, source::Wfst, budget::BudgetV2;
    pointer_form::Bool=false)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    raw = raw_resource(source)
    raw_ref = Ref(raw)
    budget_ref = Ref(budget)
    if operation === :project_input
        status = pointer_form ?
            ccall(native(:lling_wfst_project_input_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_project_input), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :project_output
        status = pointer_form ?
            ccall(native(:lling_wfst_project_output_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_project_output), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :reverse
        status = pointer_form ?
            ccall(native(:lling_wfst_reverse_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_reverse), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :determinize
        status = pointer_form ?
            ccall(native(:lling_wfst_determinize_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_determinize), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :minimize
        status = pointer_form ?
            ccall(native(:lling_wfst_minimize_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_minimize), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :remove_epsilon
        status = pointer_form ?
            ccall(native(:lling_wfst_remove_epsilon_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_remove_epsilon), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :connect
        status = pointer_form ?
            ccall(native(:lling_wfst_connect_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_connect), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :closure
        status = pointer_form ?
            ccall(native(:lling_wfst_closure_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_closure), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    elseif operation === :closure_plus
        status = pointer_form ?
            ccall(native(:lling_wfst_closure_plus_ref), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw_ref, budget_ref, output) :
            ccall(native(:lling_wfst_closure_plus), UInt32,
                (VTI.VtResourceRaw, Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                raw, budget_ref, output)
    else
        throw(ArgumentError("unknown unary WFST operation: $operation"))
    end
    checked(status, operation)
    output[]
end

function rational_binary_wfst_call(operation::Symbol, first::Wfst, second::Wfst,
    budget::BudgetV2; pointer_form::Bool=false)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    first_raw = raw_resource(first)
    second_raw = raw_resource(second)
    first_ref = Ref(first_raw)
    second_ref = Ref(second_raw)
    budget_ref = Ref(budget)
    if operation === :union
        status = pointer_form ?
            ccall(native(:lling_wfst_union_refs), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{VTI.VtResourceRaw},
                    Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                first_ref, second_ref, budget_ref, output) :
            ccall(native(:lling_wfst_union), UInt32,
                (VTI.VtResourceRaw, VTI.VtResourceRaw,
                    Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                first_raw, second_raw, budget_ref, output)
    elseif operation === :concat
        status = pointer_form ?
            ccall(native(:lling_wfst_concat_refs), UInt32,
                (Ref{VTI.VtResourceRaw}, Ref{VTI.VtResourceRaw},
                    Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                first_ref, second_ref, budget_ref, output) :
            ccall(native(:lling_wfst_concat), UInt32,
                (VTI.VtResourceRaw, VTI.VtResourceRaw,
                    Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
                first_raw, second_raw, budget_ref, output)
    else
        throw(ArgumentError("unknown binary WFST operation: $operation"))
    end
    checked(status, operation)
    output[]
end

function require_parallel_symbols(first::Wfst, second::Wfst)
    for field in (:input_symbols, :output_symbols)
        left = getfield(first, field)
        right = getfield(second, field)
        if isnothing(left) != isnothing(right) ||
            (!isnothing(left) && left.symbols != right.symbols)
            throw(ArgumentError("rational WFST operands have different $field"))
        end
    end
end

"""Lazily accept paths from either graph, preserving both tape domains."""
function Base.union(first::Wfst{L,W}, second::Wfst{L,W};
    budget::BudgetV2=BudgetV2()) where {L,W}
    require_parallel_symbols(first, second)
    handle = rational_binary_wfst_call(:union, first, second, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=first.input_symbols,
        output_symbols=first.output_symbols)
end

"""Lazily accept paths from the first graph followed by the second."""
function concat(first::Wfst{L,W}, second::Wfst{L,W};
    budget::BudgetV2=BudgetV2()) where {L,W}
    require_parallel_symbols(first, second)
    handle = rational_binary_wfst_call(:concat, first, second, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=first.input_symbols,
        output_symbols=first.output_symbols)
end

"""Lazily accept zero or more repetitions of a weighted graph."""
function closure(source::Wfst{L,W}; budget::BudgetV2=BudgetV2()) where {L,W}
    handle = unary_wfst_call(:closure, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols,
        output_symbols=source.output_symbols)
end

"""Lazily accept one or more repetitions, including empty when the input does."""
function closure_plus(source::Wfst{L,W}; budget::BudgetV2=BudgetV2()) where {L,W}
    handle = unary_wfst_call(:closure_plus, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols,
        output_symbols=source.output_symbols)
end

"""Lazily keep input labels on both tapes; `budget` bounds input and potential output."""
function project_input(source::Wfst{L,W}; budget::BudgetV2=BudgetV2()) where {L,W}
    handle = unary_wfst_call(:project_input, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols,
        output_symbols=source.input_symbols)
end

"""Lazily keep output labels on both tapes; `budget` bounds input and potential output."""
function project_output(source::Wfst{L,W}; budget::BudgetV2=BudgetV2()) where {L,W}
    handle = unary_wfst_call(:project_output, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.output_symbols,
        output_symbols=source.output_symbols)
end

"""Constructively reverse a scalar WFST within an input-plus-output graph budget."""
function Base.reverse(source::Wfst{L,W}; budget::BudgetV2=BudgetV2()) where {L,W}
    handle = unary_wfst_call(:reverse, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols,
        output_symbols=source.output_symbols)
end

"""Materialize native determinization within a required four-axis budget."""
function determinize(source::Wfst{L,W}; budget::BudgetV2) where {L,W}
    handle = unary_wfst_call(:determinize, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols, output_symbols=source.output_symbols)
end

"""Materialize native minimization of a deterministic graph within a required budget."""
function minimize(source::Wfst{L,W}; budget::BudgetV2) where {L,W}
    handle = unary_wfst_call(:minimize, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols, output_symbols=source.output_symbols)
end

"""Materialize native epsilon removal within a required four-axis budget."""
function remove_epsilon(source::Wfst{L,W}; budget::BudgetV2) where {L,W}
    handle = unary_wfst_call(:remove_epsilon, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols, output_symbols=source.output_symbols)
end

"""Materialize native connect/trim within a required four-axis budget."""
function connect(source::Wfst{L,W}; budget::BudgetV2) where {L,W}
    handle = unary_wfst_call(:connect, source, budget)
    adopt_native_wfst(handle, L, W;
        input_symbols=source.input_symbols, output_symbols=source.output_symbols)
end

# Dynamic-semiring consumer -------------------------------------------------

"""Owned native adapter for one immutable host-defined lattice value."""
mutable struct DynamicLatticeValue
    handle::Ptr{Cvoid}
    closed::Bool
end

function open_handle(value::DynamicLatticeValue)
    value.closed && throw(NativeError(STATUS_CLOSED, :lattice_value,
        "dynamic lattice value is closed"))
    value.handle
end

function adopt_dynamic_lattice(handle::Ptr{Cvoid})
    handle == C_NULL && error("native lattice operation returned a null value")
    value = DynamicLatticeValue(handle, false)
    finalizer(finalize_close, value)
    value
end

"""Retain and validate a `vt.lattice.val.1` resource through lling-llang."""
function dynamic_lattice_value(resource::VTI.Resource)
    raw = Ref(VTI.raw_resource(resource))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_lattice_open), UInt32,
        (Ref{VTI.VtResourceRaw}, Ref{Ptr{Cvoid}}), raw, output), :lattice_open)
    adopt_dynamic_lattice(output[])
end

function close!(value::DynamicLatticeValue)
    value.closed && return nothing
    ccall(native(:lling_lattice_free), Cvoid, (Ptr{Cvoid},), value.handle)
    value.handle = C_NULL
    value.closed = true
    nothing
end

Base.close(value::DynamicLatticeValue) = close!(value)
Base.isopen(value::DynamicLatticeValue) = !value.closed

"""Return the stable provider-defined domain identifier."""
function lattice_domain_id(value::DynamicLatticeValue)
    output = Ref{VTI.VtInterfaceId}()
    checked(ccall(native(:lling_lattice_domain_id), UInt32,
        (Ptr{Cvoid}, Ref{VTI.VtInterfaceId}), open_handle(value), output),
        :lattice_domain_id)
    output[]
end

"""Return the provider's validated lattice capability flags."""
function lattice_flags(value::DynamicLatticeValue)
    output = Ref{UInt64}(0)
    checked(ccall(native(:lling_lattice_flags), UInt32,
        (Ptr{Cvoid}, Ref{UInt64}), open_handle(value), output), :lattice_flags)
    output[]
end

function binary_lattice(left::DynamicLatticeValue, right::DynamicLatticeValue,
    operation::Symbol)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(lattice_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ref{Ptr{Cvoid}}), open_handle(left),
        open_handle(right), output), operation)
    adopt_dynamic_lattice(output[])
end

"""Return the least upper bound of two same-domain dynamic values."""
lattice_join(left::DynamicLatticeValue, right::DynamicLatticeValue) =
    binary_lattice(left, right, :lling_lattice_join)
"""Return the greatest lower bound of two same-domain dynamic values."""
lattice_meet(left::DynamicLatticeValue, right::DynamicLatticeValue) =
    binary_lattice(left, right, :lling_lattice_meet)

"""Compare two same-domain dynamic values for exact semantic equality."""
function lattice_equal(left::DynamicLatticeValue, right::DynamicLatticeValue)
    output = Ref{UInt8}(0xff)
    checked(ccall(native(:lling_lattice_equal), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt8}), open_handle(left),
        open_handle(right), output), :lattice_equal)
    output[] == 1
end

function read_lattice_bytes(value::DynamicLatticeValue, operation::Symbol)
    written = Ref{Csize_t}(0)
    required = Ref{Csize_t}(0)
    checked(ccall(lattice_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{UInt8}, Csize_t, Ref{Csize_t}, Ref{Csize_t}),
        open_handle(value), C_NULL, 0, written, required), operation)
    output = Vector{UInt8}(undef, Int(required[]))
    checked(ccall(lattice_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{UInt8}, Csize_t, Ref{Csize_t}, Ref{Csize_t}),
        open_handle(value), output, length(output), written, required), operation)
    resize!(output, Int(written[]))
end

"""Return the provider's canonical encoding for a dynamic lattice value."""
lattice_stable_bytes(value::DynamicLatticeValue) =
    read_lattice_bytes(value, :lling_lattice_stable_bytes)
"""Return the provider's advisory diagnostic string."""
lattice_diagnostic(value::DynamicLatticeValue) =
    String(read_lattice_bytes(value, :lling_lattice_diagnostic))

function lattice_many(receiver::DynamicLatticeValue,
    others::AbstractVector{DynamicLatticeValue}, operation::Symbol)
    pointers = Ptr{Cvoid}[open_handle(value) for value in others]
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(lattice_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Ptr{Cvoid}}, Csize_t, Ref{Ptr{Cvoid}}),
        open_handle(receiver), pointers, length(pointers), output), operation)
    adopt_dynamic_lattice(output[])
end

"""Fold joins through the provider's bounded batch path when available."""
lattice_join_many(receiver::DynamicLatticeValue,
    others::AbstractVector{DynamicLatticeValue}) =
    lattice_many(receiver, others, :lling_lattice_join_many)
"""Fold meets through the provider's bounded batch path when available."""
lattice_meet_many(receiver::DynamicLatticeValue,
    others::AbstractVector{DynamicLatticeValue}) =
    lattice_many(receiver, others, :lling_lattice_meet_many)

"""Probe all lattice laws over at most sixteen representative values."""
function validate_lattice_laws(values::AbstractVector{DynamicLatticeValue})
    pointers = Ptr{Cvoid}[open_handle(value) for value in values]
    checked(ccall(native(:lling_lattice_validate_laws), UInt32,
        (Ptr{Ptr{Cvoid}}, Csize_t), pointers, length(pointers)),
        :lattice_validate_laws)
    nothing
end

"""Owned native adapter for one host-defined semiring operation context."""
mutable struct SemiringContext
    handle::Ptr{Cvoid}
    closed::Bool
end

"""One owned dynamic weight scoped to its exact `SemiringContext`."""
mutable struct SemiringWeight
    handle::Ptr{Cvoid}
    context::SemiringContext
    closed::Bool
end

function open_handle(context::SemiringContext)
    context.closed && throw(NativeError(STATUS_CLOSED, :semiring,
        "semiring context is closed"))
    context.handle
end

function open_handle(weight::SemiringWeight)
    weight.closed && throw(NativeError(STATUS_CLOSED, :semiring_weight,
        "semiring weight is closed"))
    open_handle(weight.context)
    weight.handle
end

function adopt_semiring_weight(context::SemiringContext, handle::Ptr{Cvoid})
    handle == C_NULL && error("native semiring operation returned a null weight")
    weight = SemiringWeight(handle, context, false)
    finalizer(finalize_close, weight)
    weight
end

"""Retain and validate a `vt.semiring.*1` resource through native lling-llang."""
function semiring_context(resource::VTI.Resource)
    raw = Ref(VTI.raw_resource(resource))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_semiring_open), UInt32,
        (Ref{VTI.VtResourceRaw}, Ref{Ptr{Cvoid}}), raw, output), :semiring_open)
    context = SemiringContext(output[], false)
    finalizer(finalize_close, context)
    context
end

function close!(context::SemiringContext)
    context.closed && return nothing
    ccall(native(:lling_semiring_free), Cvoid, (Ptr{Cvoid},), context.handle)
    context.handle = C_NULL
    context.closed = true
    nothing
end

function close!(weight::SemiringWeight)
    weight.closed && return nothing
    ccall(native(:lling_semiring_weight_free), Cvoid,
        (Ptr{Cvoid},), weight.handle)
    weight.handle = C_NULL
    weight.closed = true
    nothing
end

Base.close(context::SemiringContext) = close!(context)
Base.close(weight::SemiringWeight) = close!(weight)
Base.isopen(context::SemiringContext) = !context.closed
Base.isopen(weight::SemiringWeight) = !weight.closed

"""Return the provider's declared algebraic-property bitset."""
function semiring_properties(context::SemiringContext)
    output = Ref{UInt64}(0)
    checked(ccall(native(:lling_semiring_properties), UInt32,
        (Ptr{Cvoid}, Ref{UInt64}), open_handle(context), output),
        :semiring_properties)
    output[]
end

function semiring_identity(context::SemiringContext, operation::Symbol)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), open_handle(context), output), operation)
    adopt_semiring_weight(context, output[])
end

"""Construct the additive identity."""
semiring_zero(context::SemiringContext) =
    semiring_identity(context, :lling_semiring_zero)
"""Construct the multiplicative identity."""
semiring_one(context::SemiringContext) =
    semiring_identity(context, :lling_semiring_one)

function Base.copy(weight::SemiringWeight)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_semiring_weight_clone), UInt32,
        (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), open_handle(weight), output),
        :semiring_weight_clone)
    adopt_semiring_weight(weight.context, output[])
end

function binary_weight(context::SemiringContext, left::SemiringWeight,
    right::SemiringWeight, operation::Symbol)
    left.context === context || throw(ArgumentError("left weight has another context"))
    right.context === context || throw(ArgumentError("right weight has another context"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{Cvoid}, Ref{Ptr{Cvoid}}),
        open_handle(context), open_handle(left), open_handle(right), output), operation)
    adopt_semiring_weight(context, output[])
end

"""Add two weights in their exact shared context."""
semiring_plus(context::SemiringContext, left::SemiringWeight,
    right::SemiringWeight) = binary_weight(context, left, right,
    :lling_semiring_plus)
"""Multiply two weights in their exact shared context."""
semiring_times(context::SemiringContext, left::SemiringWeight,
    right::SemiringWeight) = binary_weight(context, left, right,
    :lling_semiring_times)
Base.:+(left::SemiringWeight, right::SemiringWeight) =
    semiring_plus(left.context, left, right)
Base.:*(left::SemiringWeight, right::SemiringWeight) =
    semiring_times(left.context, left, right)

function compare_weights(context::SemiringContext, left::SemiringWeight,
    right::SemiringWeight, operation::Symbol, epsilon::Union{Nothing,Float64}=nothing)
    left.context === context || throw(ArgumentError("left weight has another context"))
    right.context === context || throw(ArgumentError("right weight has another context"))
    output = Ref{UInt8}(0xff)
    code = isnothing(epsilon) ? ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt8}),
        open_handle(context), open_handle(left), open_handle(right), output) :
        ccall(semiring_native(operation), UInt32,
            (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{Cvoid}, Float64, Ref{UInt8}),
            open_handle(context), open_handle(left), open_handle(right), epsilon, output)
    checked(code, operation)
    output[] == 1
end

"""Return exact semantic equality."""
semiring_equal(context::SemiringContext, left::SemiringWeight,
    right::SemiringWeight) = compare_weights(context, left, right,
    :lling_semiring_equal)
"""Return provider-defined approximate equality at `epsilon`."""
semiring_approx_equal(context::SemiringContext, left::SemiringWeight,
    right::SemiringWeight, epsilon::Real) = compare_weights(context, left, right,
    :lling_semiring_approx_equal, Float64(epsilon))

function semiring_natural_order(context::SemiringContext, left::SemiringWeight,
    right::SemiringWeight)
    left.context === context || throw(ArgumentError("left weight has another context"))
    right.context === context || throw(ArgumentError("right weight has another context"))
    output = Ref{Int32}(typemin(Int32))
    checked(ccall(native(:lling_semiring_natural_order), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{Cvoid}, Ref{Int32}), open_handle(context),
        open_handle(left), open_handle(right), output), :semiring_natural_order)
    output[]
end

function partial_weight(context::SemiringContext, first::SemiringWeight,
    second::Union{Nothing,SemiringWeight}, operation::Symbol)
    first.context === context || throw(ArgumentError("weight has another context"))
    isnothing(second) || second.context === context ||
        throw(ArgumentError("second weight has another context"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    defined = Ref{UInt8}(0xff)
    code = isnothing(second) ? ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ref{Ptr{Cvoid}}, Ref{UInt8}),
        open_handle(context), open_handle(first), output, defined) :
        ccall(semiring_native(operation), UInt32,
            (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{Cvoid}, Ref{Ptr{Cvoid}}, Ref{UInt8}),
            open_handle(context), open_handle(first), open_handle(second), output, defined)
    checked(code, operation)
    defined[] == 0 ? nothing : adopt_semiring_weight(context, output[])
end

semiring_divide(context::SemiringContext, dividend::SemiringWeight,
    divisor::SemiringWeight) = partial_weight(context, dividend, divisor,
    :lling_semiring_divide)
semiring_left_divide(context::SemiringContext, value::SemiringWeight,
    divisor::SemiringWeight) = partial_weight(context, value, divisor,
    :lling_semiring_left_divide)
semiring_star(context::SemiringContext, value::SemiringWeight) =
    partial_weight(context, value, nothing, :lling_semiring_star)

function scalar_projection(context::SemiringContext, weight::SemiringWeight,
    operation::Symbol)
    weight.context === context || throw(ArgumentError("weight has another context"))
    output = Ref{Float64}(NaN)
    checked(ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ref{Float64}), open_handle(context),
        open_handle(weight), output), operation)
    output[]
end

semiring_numerical_value(context::SemiringContext, weight::SemiringWeight) =
    scalar_projection(context, weight, :lling_semiring_numerical_value)
semiring_probability(context::SemiringContext, weight::SemiringWeight) =
    scalar_projection(context, weight, :lling_semiring_to_probability)

function semiring_quantize(context::SemiringContext, weight::SemiringWeight,
    epsilon::Real)
    weight.context === context || throw(ArgumentError("weight has another context"))
    output = Ref{Int64}(0)
    checked(ccall(native(:lling_semiring_quantize), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Float64, Ref{Int64}), open_handle(context),
        open_handle(weight), Float64(epsilon), output), :lling_semiring_quantize)
    output[]
end

function semiring_closure_bound(context::SemiringContext)
    bound = Ref{Csize_t}(0)
    known = Ref{UInt8}(0xff)
    checked(ccall(native(:lling_semiring_closure_bound), UInt32,
        (Ptr{Cvoid}, Ref{Csize_t}, Ref{UInt8}), open_handle(context), bound, known),
        :lling_semiring_closure_bound)
    known[] == 0 ? nothing : Int(bound[])
end

function read_semiring_bytes(context::SemiringContext,
    weight::Union{Nothing,SemiringWeight}, operation::Symbol)
    isnothing(weight) || weight.context === context ||
        throw(ArgumentError("weight has another context"))
    handle = isnothing(weight) ? C_NULL : open_handle(weight)
    written = Ref{Csize_t}(0)
    required = Ref{Csize_t}(0)
    checked(ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{UInt8}, Csize_t, Ref{Csize_t}, Ref{Csize_t}),
        open_handle(context), handle, C_NULL, 0, written, required), operation)
    output = Vector{UInt8}(undef, Int(required[]))
    checked(ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{UInt8}, Csize_t, Ref{Csize_t}, Ref{Csize_t}),
        open_handle(context), handle, output, length(output), written,
        required), operation)
    resize!(output, Int(written[]))
end

"""Return the provider's canonical encoding for one weight."""
semiring_stable_bytes(context::SemiringContext, weight::SemiringWeight) =
    read_semiring_bytes(context, weight, :lling_semiring_stable_bytes)

"""Return the provider's advisory diagnostic for a weight or its domain."""
semiring_diagnostic(context::SemiringContext,
    weight::Union{Nothing,SemiringWeight}=nothing) =
    String(read_semiring_bytes(context, weight, :lling_semiring_diagnostic))

function semiring_many(context::SemiringContext,
    weights::AbstractVector{SemiringWeight}, operation::Symbol)
    all(weight -> weight.context === context, weights) ||
        throw(ArgumentError("every weight must share the exact context"))
    pointers = Ptr{Cvoid}[open_handle(weight) for weight in weights]
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(semiring_native(operation), UInt32,
        (Ptr{Cvoid}, Ptr{Ptr{Cvoid}}, Csize_t, Ref{Ptr{Cvoid}}),
        open_handle(context), pointers, length(pointers), output), operation)
    adopt_semiring_weight(context, output[])
end

"""Fold addition through the bounded provider batch path when available."""
semiring_plus_many(context::SemiringContext,
    weights::AbstractVector{SemiringWeight}) =
    semiring_many(context, weights, :lling_semiring_plus_many)

"""Fold multiplication through the bounded provider batch path when available."""
semiring_times_many(context::SemiringContext,
    weights::AbstractVector{SemiringWeight}) =
    semiring_many(context, weights, :lling_semiring_times_many)

function validate_semiring_laws(context::SemiringContext,
    weights::AbstractVector{SemiringWeight}; epsilon::Real=0.0)
    all(weight -> weight.context === context, weights) ||
        throw(ArgumentError("every law sample must share the exact context"))
    pointers = Ptr{Cvoid}[open_handle(weight) for weight in weights]
    checked(ccall(native(:lling_semiring_validate_laws), UInt32,
        (Ptr{Cvoid}, Ptr{Ptr{Cvoid}}, Csize_t, Float64), open_handle(context),
        pointers, length(pointers), Float64(epsilon)), :lling_semiring_validate_laws)
    nothing
end

# Host-semiring provider API -----------------------------------------------

"""Implement the semiring operations below to publish a Julia weight algebra."""
abstract type AbstractSemiringProvider end

function semiring_zero(provider::AbstractSemiringProvider)
    throw(MethodError(semiring_zero, (provider,)))
end
function semiring_one(provider::AbstractSemiringProvider)
    throw(MethodError(semiring_one, (provider,)))
end
function semiring_plus(provider::AbstractSemiringProvider, left, right)
    throw(MethodError(semiring_plus, (provider, left, right)))
end
function semiring_times(provider::AbstractSemiringProvider, left, right)
    throw(MethodError(semiring_times, (provider, left, right)))
end
semiring_equal(::AbstractSemiringProvider, left, right) = left == right
semiring_approx_equal(provider::AbstractSemiringProvider, left, right, epsilon) =
    semiring_equal(provider, left, right)
function semiring_natural_order(provider::AbstractSemiringProvider, left, right)
    throw(MethodError(semiring_natural_order, (provider, left, right)))
end
semiring_stable_bytes(provider::AbstractSemiringProvider, value) =
    throw(MethodError(semiring_stable_bytes, (provider, value)))
semiring_diagnostic(provider::AbstractSemiringProvider, value) =
    isnothing(value) ? sprint(show, provider) : sprint(show, value)
semiring_divide(::AbstractSemiringProvider, dividend, divisor) = nothing
semiring_left_divide(::AbstractSemiringProvider, value, divisor) = nothing
semiring_star(::AbstractSemiringProvider, value) = nothing
semiring_numerical_value(::AbstractSemiringProvider, value) = nothing
semiring_quantize(::AbstractSemiringProvider, value, epsilon) = nothing
semiring_probability(::AbstractSemiringProvider, value) = nothing
semiring_properties(::AbstractSemiringProvider) = UInt64(0)
semiring_closure_bound(::AbstractSemiringProvider) = nothing

mutable struct SemiringSlot
    value::Any
    generation::UInt64
    references::Int
    occupied::Bool
end

mutable struct SemiringProviderContext
    cookie::Ptr{Cvoid}
    references::Int
    implementation::AbstractSemiringProvider
    flags::UInt64
    owner_thread::Int
    call_active::Base.Threads.Atomic{Bool}
    properties::UInt64
    closure_bound::Union{Nothing,UInt}
    arena_lock::ReentrantLock
    slots::Vector{SemiringSlot}
    free_slots::Vector{Int}
    last_error::String
    base_table::Base.RefValue{VTI.VtSemiringVTable}
    division_table::Union{Nothing,Base.RefValue{VTI.VtSemiringDivisionVTable}}
    star_table::Union{Nothing,Base.RefValue{VTI.VtSemiringStarVTable}}
    numeric_table::Union{Nothing,Base.RefValue{VTI.VtSemiringNumericVTable}}
    properties_table::Base.RefValue{VTI.VtSemiringPropertiesVTable}
end

const SEMIRING_PROVIDERS = Dict{Ptr{Cvoid},SemiringProviderContext}()
const SEMIRING_PROVIDERS_LOCK = ReentrantLock()
const PROVIDER_COOKIE_LOCK = ReentrantLock()
const NEXT_PROVIDER_COOKIE = Ref{UInt}(1)
const SEMIRING_RESOURCE_TABLE = Ref{VTI.VtResourceVTable}()
const SEMIRING_CALLBACKS = Dict{Symbol,Ptr{Cvoid}}()

# The context word is an opaque, process-unique cookie, never an object address.
# A released callback can therefore never address a newly allocated provider
# after Julia moves/reuses an object. Exhaustion rejects publication rather
# than wrapping and accepting a stale handle as a different live resource.
function new_provider_cookie()
    lock(PROVIDER_COOKIE_LOCK) do
        value = NEXT_PROVIDER_COOKIE[]
        value == typemax(UInt) && throw(OverflowError("provider resource cookie"))
        NEXT_PROVIDER_COOKIE[] = value + UInt(1)
        Ptr{Cvoid}(value)
    end
end

semiring_provider_context(pointer::Ptr{Cvoid}) = lock(SEMIRING_PROVIDERS_LOCK) do
    get(SEMIRING_PROVIDERS, pointer, nothing)
end

function record_semiring_error!(context::SemiringProviderContext, error)
    # A user-defined exception can itself throw from showerror. Neither that
    # failure nor diagnostic allocation may unwind through an @cfunction.
    try
        message = try
            sprint(showerror, error)
        catch
            "semiring provider raised an unprintable exception"
        end
        lock(context.arena_lock) do
            context.last_error = message
        end
    catch
    end
    nothing
end

function semiring_call_gate(operation::Function, context::SemiringProviderContext)
    if context.flags & VTI.SEMIRING_FLAG_THREAD_BOUND != 0 &&
        Base.Threads.threadid() != context.owner_thread
        record_semiring_error!(context, ErrorException(
            "thread-bound semiring provider called from another Julia thread"))
        return Cint(VTI.STATUS_PROVIDER_ERROR)
    end
    context.flags & VTI.SEMIRING_FLAG_PARALLEL_REENTRANT != 0 &&
        return operation()
    if Base.Threads.atomic_cas!(context.call_active, false, true)
        record_semiring_error!(context, ErrorException(
            "semiring provider callback is concurrent or recursive"))
        return Cint(VTI.STATUS_PROVIDER_ERROR)
    end
    try
        operation()
    finally
        context.call_active[] = false
    end
end

function allocate_semiring_value(context::SemiringProviderContext, value)
    lock(context.arena_lock) do
        if isempty(context.free_slots)
            push!(context.slots, SemiringSlot(value, UInt64(1), 1, true))
            index = length(context.slots)
        else
            index = pop!(context.free_slots)
            slot = context.slots[index]
            slot.value = value
            slot.references = 1
            slot.occupied = true
        end
        VTI.VtSemiringValue(UInt64(index), context.slots[index].generation)
    end
end

function resolve_semiring_value(context::SemiringProviderContext,
    token::VTI.VtSemiringValue)
    lock(context.arena_lock) do
        index = Int(token.word0)
        1 <= index <= length(context.slots) ||
            throw(ArgumentError("semiring token slot is out of range"))
        slot = context.slots[index]
        slot.occupied && slot.generation == token.word1 ||
            throw(ArgumentError("semiring token is stale or already released"))
        slot.value
    end
end

function clone_semiring_value!(context::SemiringProviderContext,
    token::VTI.VtSemiringValue)
    lock(context.arena_lock) do
        index = Int(token.word0)
        1 <= index <= length(context.slots) ||
            throw(ArgumentError("semiring token slot is out of range"))
        slot = context.slots[index]
        slot.occupied && slot.generation == token.word1 ||
            throw(ArgumentError("semiring token is stale or already released"))
        slot.references == typemax(Int) && throw(OverflowError("weight reference count"))
        slot.references += 1
        token
    end
end

function release_semiring_values!(context::SemiringProviderContext,
    values::Ptr{VTI.VtSemiringValue}, count::Int)
    lock(context.arena_lock) do
        # Validate every borrowed token before changing a single reference.
        # An invalid token late in a batch must not partially consume earlier
        # owners, since the native caller may retry the whole failed batch.
        releases = Dict{Int,Int}()
        for offset in 1:count
            token = unsafe_load(values, offset)
            index = Int(token.word0)
            1 <= index <= length(context.slots) ||
                throw(ArgumentError("semiring token slot is out of range"))
            slot = context.slots[index]
            slot.occupied && slot.generation == token.word1 ||
                throw(ArgumentError("semiring token is stale or already released"))
            requested = get(releases, index, 0) + 1
            requested <= slot.references ||
                throw(ArgumentError("weight reference count underflow"))
            releases[index] = requested
        end
        # Reserve before mutating owners: a free-list growth failure must not
        # leave a prefix of a validated batch consumed.
        sizehint!(context.free_slots,
            Base.Checked.checked_add(length(context.free_slots), length(releases)))
        for (index, requested) in releases
            slot = context.slots[index]
            slot.references -= requested
            if slot.references == 0
                slot.value = nothing
                slot.occupied = false
                # Never wrap a generation: retiring this slot prevents ABA.
                if slot.generation != typemax(UInt64)
                    slot.generation += UInt64(1)
                    push!(context.free_slots, index)
                end
            end
        end
    end
    nothing
end

function semiring_resource_retain(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(SEMIRING_PROVIDERS_LOCK) do
            context = get(SEMIRING_PROVIDERS, pointer, nothing)
            if !isnothing(context)
                context.references == typemax(Int) ||
                    (context.references += 1)
            end
        end
    catch
    end
    nothing
end

function semiring_resource_release(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(SEMIRING_PROVIDERS_LOCK) do
            context = get(SEMIRING_PROVIDERS, pointer, nothing)
            if !isnothing(context) && context.references > 0
                context.references -= 1
                context.references == 0 && delete!(SEMIRING_PROVIDERS, pointer)
            end
        end
    catch
    end
    nothing
end

semiring_table_pointer(table::Base.RefValue{T}) where {T} =
    Ptr{Cvoid}(Base.unsafe_convert(Ptr{T}, table))

function semiring_resource_query(pointer::Ptr{Cvoid},
    id::Ptr{VTI.VtInterfaceId}, minimum::UInt32, output::Ptr{Ptr{Cvoid}})::Cint
    (pointer == C_NULL || id == C_NULL || output == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    context = semiring_provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        identifier = unsafe_load(id)
        table = identifier == VTI.SEMIRING_INTERFACE_ID && minimum <= VTI.SEMIRING_INTERFACE_VERSION ? context.base_table :
            identifier == VTI.SEMIRING_DIVISION_INTERFACE_ID && minimum <= VTI.SEMIRING_DIVISION_INTERFACE_VERSION ? context.division_table :
            identifier == VTI.SEMIRING_STAR_INTERFACE_ID && minimum <= VTI.SEMIRING_STAR_INTERFACE_VERSION ? context.star_table :
            identifier == VTI.SEMIRING_NUMERIC_INTERFACE_ID && minimum <= VTI.SEMIRING_NUMERIC_INTERFACE_VERSION ? context.numeric_table :
            identifier == VTI.SEMIRING_PROPERTIES_INTERFACE_ID && minimum <= VTI.SEMIRING_PROPERTIES_INTERFACE_VERSION ? context.properties_table : nothing
        isnothing(table) && return Cint(VTI.STATUS_UNSUPPORTED)
        unsafe_store!(output, semiring_table_pointer(table))
        Cint(VTI.STATUS_OK)
    catch error
        record_semiring_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function semiring_callback(operation::Function, context_pointer::Ptr{Cvoid})
    context_pointer == C_NULL && return Cint(VTI.STATUS_NULL_POINTER)
    context = semiring_provider_context(context_pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        semiring_call_gate(context) do
            operation(context)
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_semiring_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function semiring_identity_callback(pointer::Ptr{Cvoid}, output::Ptr{VTI.VtSemiringValue},
    operation::Function)::Cint
    output == C_NULL && return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        unsafe_store!(output, allocate_semiring_value(context,
            operation(context.implementation)))
    end
end
semiring_zero_callback(pointer::Ptr{Cvoid}, output::Ptr{VTI.VtSemiringValue})::Cint =
    semiring_identity_callback(pointer, output, semiring_zero)
semiring_one_callback(pointer::Ptr{Cvoid}, output::Ptr{VTI.VtSemiringValue})::Cint =
    semiring_identity_callback(pointer, output, semiring_one)

function semiring_clone_callback(pointer::Ptr{Cvoid}, value::Ptr{VTI.VtSemiringValue},
    output::Ptr{VTI.VtSemiringValue})::Cint
    (value == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        unsafe_store!(output, clone_semiring_value!(context, unsafe_load(value)))
    end
end

function semiring_release_callback(pointer::Ptr{Cvoid},
    values::Ptr{VTI.VtSemiringValue}, count::Csize_t)::Cint
    (count != 0 && values == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        release_semiring_values!(context, values, Int(count))
    end
end

function semiring_binary_callback(pointer::Ptr{Cvoid}, left::Ptr{VTI.VtSemiringValue},
    right::Ptr{VTI.VtSemiringValue}, output::Ptr{VTI.VtSemiringValue},
    operation::Function)::Cint
    (left == C_NULL || right == C_NULL || output == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        left_value = resolve_semiring_value(context, unsafe_load(left))
        right_value = resolve_semiring_value(context, unsafe_load(right))
        result = operation(context.implementation, left_value, right_value)
        unsafe_store!(output, allocate_semiring_value(context, result))
    end
end
semiring_plus_callback(p, l, r, o)::Cint =
    semiring_binary_callback(p, l, r, o, semiring_plus)
semiring_times_callback(p, l, r, o)::Cint =
    semiring_binary_callback(p, l, r, o, semiring_times)

function semiring_equal_callback(pointer::Ptr{Cvoid}, left::Ptr{VTI.VtSemiringValue},
    right::Ptr{VTI.VtSemiringValue}, output::Ptr{UInt8})::Cint
    (left == C_NULL || right == C_NULL || output == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        result = semiring_equal(context.implementation,
            resolve_semiring_value(context, unsafe_load(left)),
            resolve_semiring_value(context, unsafe_load(right)))
        unsafe_store!(output, UInt8(Bool(result)))
    end
end

function semiring_approx_callback(pointer::Ptr{Cvoid}, left::Ptr{VTI.VtSemiringValue},
    right::Ptr{VTI.VtSemiringValue}, epsilon::Float64, output::Ptr{UInt8})::Cint
    (left == C_NULL || right == C_NULL || output == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        result = semiring_approx_equal(context.implementation,
            resolve_semiring_value(context, unsafe_load(left)),
            resolve_semiring_value(context, unsafe_load(right)), epsilon)
        unsafe_store!(output, UInt8(Bool(result)))
    end
end

function semiring_order_callback(pointer::Ptr{Cvoid}, left::Ptr{VTI.VtSemiringValue},
    right::Ptr{VTI.VtSemiringValue}, output::Ptr{Int32})::Cint
    (left == C_NULL || right == C_NULL || output == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        order = Int32(semiring_natural_order(context.implementation,
            resolve_semiring_value(context, unsafe_load(left)),
            resolve_semiring_value(context, unsafe_load(right))))
        order in (VTI.SEMIRING_ORDER_BETTER, VTI.SEMIRING_ORDER_EQUAL,
            VTI.SEMIRING_ORDER_WORSE, VTI.SEMIRING_ORDER_INCOMPARABLE) ||
            throw(ArgumentError("natural order must be -1, 0, 1, or 2"))
        unsafe_store!(output, order)
    end
end

function write_semiring_bytes(output::Ptr{UInt8}, capacity::Csize_t,
    written::Ptr{Csize_t}, required::Ptr{Csize_t}, bytes)
    (written == C_NULL || required == C_NULL ||
        (capacity != 0 && output == C_NULL)) && throw(ArgumentError("null byte buffer"))
    values = Vector{UInt8}(bytes)
    unsafe_store!(required, Csize_t(length(values)))
    count = min(Int(capacity), length(values))
    count > 0 && unsafe_copyto!(output, pointer(values), count)
    unsafe_store!(written, Csize_t(count))
    nothing
end

function semiring_bytes_callback(pointer::Ptr{Cvoid}, value::Ptr{VTI.VtSemiringValue},
    output::Ptr{UInt8}, capacity::Csize_t, written::Ptr{Csize_t},
    required::Ptr{Csize_t}, operation::Function)::Cint
    value == C_NULL && return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        write_semiring_bytes(output, capacity, written, required,
            operation(context.implementation,
                resolve_semiring_value(context, unsafe_load(value))))
    end
end
semiring_stable_bytes_callback(p, v, o, c, w, r)::Cint =
    semiring_bytes_callback(p, v, o, c, w, r, semiring_stable_bytes)
function semiring_diagnostic_callback(pointer::Ptr{Cvoid},
    value::Ptr{VTI.VtSemiringValue}, output::Ptr{UInt8}, capacity::Csize_t,
    written::Ptr{Csize_t}, required::Ptr{Csize_t})::Cint
    semiring_callback(pointer) do context
        resolved = value == C_NULL ? nothing :
            resolve_semiring_value(context, unsafe_load(value))
        write_semiring_bytes(output, capacity, written, required,
            codeunits(semiring_diagnostic(context.implementation, resolved)))
    end
end

function semiring_many_callback(pointer::Ptr{Cvoid},
    values::Ptr{VTI.VtSemiringValue}, count::Csize_t,
    output::Ptr{VTI.VtSemiringValue}, operation::Function, identity::Function)::Cint
    (output == C_NULL || (count != 0 && values == C_NULL)) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        provider = context.implementation
        accumulator = identity(provider)
        for index in 1:Int(count)
            accumulator = operation(provider, accumulator,
                resolve_semiring_value(context, unsafe_load(values, index)))
        end
        unsafe_store!(output, allocate_semiring_value(context, accumulator))
    end
end
semiring_plus_many_callback(p, v, c, o)::Cint =
    semiring_many_callback(p, v, c, o, semiring_plus, semiring_zero)
semiring_times_many_callback(p, v, c, o)::Cint =
    semiring_many_callback(p, v, c, o, semiring_times, semiring_one)

function semiring_partial_binary_callback(pointer::Ptr{Cvoid},
    left::Ptr{VTI.VtSemiringValue}, right::Ptr{VTI.VtSemiringValue},
    output::Ptr{VTI.VtSemiringValue}, operation::Function)::Cint
    (left == C_NULL || right == C_NULL || output == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    context = semiring_provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        semiring_call_gate(context) do
            result = operation(context.implementation,
                resolve_semiring_value(context, unsafe_load(left)),
                resolve_semiring_value(context, unsafe_load(right)))
            isnothing(result) && return Cint(VTI.STATUS_END)
            unsafe_store!(output, allocate_semiring_value(context, result))
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_semiring_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end
semiring_divide_callback(p, l, r, o)::Cint =
    semiring_partial_binary_callback(p, l, r, o, semiring_divide)
semiring_left_divide_callback(p, l, r, o)::Cint =
    semiring_partial_binary_callback(p, l, r, o, semiring_left_divide)

function semiring_star_callback(pointer::Ptr{Cvoid}, value::Ptr{VTI.VtSemiringValue},
    output::Ptr{VTI.VtSemiringValue})::Cint
    (value == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    context = semiring_provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        semiring_call_gate(context) do
            result = semiring_star(context.implementation,
                resolve_semiring_value(context, unsafe_load(value)))
            isnothing(result) && return Cint(VTI.STATUS_END)
            unsafe_store!(output, allocate_semiring_value(context, result))
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_semiring_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function semiring_numeric_callback(pointer::Ptr{Cvoid}, value::Ptr{VTI.VtSemiringValue},
    output::Ptr{Float64}, operation::Function)::Cint
    (value == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    context = semiring_provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        semiring_call_gate(context) do
            result = operation(context.implementation,
                resolve_semiring_value(context, unsafe_load(value)))
            isnothing(result) && return Cint(VTI.STATUS_UNSUPPORTED)
            unsafe_store!(output, Float64(result))
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_semiring_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end
semiring_numerical_callback(p, v, o)::Cint =
    semiring_numeric_callback(p, v, o, semiring_numerical_value)
semiring_probability_callback(p, v, o)::Cint =
    semiring_numeric_callback(p, v, o, semiring_probability)

function semiring_quantize_callback(pointer::Ptr{Cvoid},
    value::Ptr{VTI.VtSemiringValue}, epsilon::Float64, output::Ptr{Int64})::Cint
    (value == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    context = semiring_provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        semiring_call_gate(context) do
            result = semiring_quantize(context.implementation,
                resolve_semiring_value(context, unsafe_load(value)), epsilon)
            isnothing(result) && return Cint(VTI.STATUS_UNSUPPORTED)
            unsafe_store!(output, Int64(result))
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_semiring_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function semiring_closure_callback(pointer::Ptr{Cvoid}, output::Ptr{Csize_t},
    known::Ptr{UInt8})::Cint
    (output == C_NULL || known == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    semiring_callback(pointer) do context
        bound = context.closure_bound
        unsafe_store!(output, Csize_t(something(bound, 0)))
        unsafe_store!(known, UInt8(!isnothing(bound)))
    end
end

function initialize_semiring_callbacks!()
    empty!(SEMIRING_CALLBACKS)
    SEMIRING_CALLBACKS[:retain] = @cfunction(semiring_resource_retain, Cvoid, (Ptr{Cvoid},))
    SEMIRING_CALLBACKS[:release] = @cfunction(semiring_resource_release, Cvoid, (Ptr{Cvoid},))
    SEMIRING_CALLBACKS[:query] = @cfunction(semiring_resource_query, Cint, (Ptr{Cvoid}, Ptr{VTI.VtInterfaceId}, UInt32, Ptr{Ptr{Cvoid}}))
    SEMIRING_CALLBACKS[:zero] = @cfunction(semiring_zero_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:one] = @cfunction(semiring_one_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:clone] = @cfunction(semiring_clone_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:release_values] = @cfunction(semiring_release_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Csize_t))
    SEMIRING_CALLBACKS[:plus] = @cfunction(semiring_plus_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:times] = @cfunction(semiring_times_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:equal] = @cfunction(semiring_equal_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}, Ptr{UInt8}))
    SEMIRING_CALLBACKS[:approx] = @cfunction(semiring_approx_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}, Float64, Ptr{UInt8}))
    SEMIRING_CALLBACKS[:order] = @cfunction(semiring_order_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}, Ptr{Int32}))
    SEMIRING_CALLBACKS[:stable] = @cfunction(semiring_stable_bytes_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{UInt8}, Csize_t, Ptr{Csize_t}, Ptr{Csize_t}))
    SEMIRING_CALLBACKS[:diagnostic] = @cfunction(semiring_diagnostic_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{UInt8}, Csize_t, Ptr{Csize_t}, Ptr{Csize_t}))
    SEMIRING_CALLBACKS[:plus_many] = @cfunction(semiring_plus_many_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Csize_t, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:times_many] = @cfunction(semiring_times_many_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Csize_t, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:divide] = @cfunction(semiring_divide_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:left_divide] = @cfunction(semiring_left_divide_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:star] = @cfunction(semiring_star_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{VTI.VtSemiringValue}))
    SEMIRING_CALLBACKS[:numerical] = @cfunction(semiring_numerical_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{Float64}))
    SEMIRING_CALLBACKS[:quantize] = @cfunction(semiring_quantize_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Float64, Ptr{Int64}))
    SEMIRING_CALLBACKS[:probability] = @cfunction(semiring_probability_callback, Cint, (Ptr{Cvoid}, Ptr{VTI.VtSemiringValue}, Ptr{Float64}))
    SEMIRING_CALLBACKS[:closure] = @cfunction(semiring_closure_callback, Cint, (Ptr{Cvoid}, Ptr{Csize_t}, Ptr{UInt8}))
    SEMIRING_RESOURCE_TABLE[] = VTI.VtResourceVTable(sizeof(VTI.VtResourceVTable),
        VTI.ABI_VERSION, 0, SEMIRING_CALLBACKS[:retain],
        SEMIRING_CALLBACKS[:release], SEMIRING_CALLBACKS[:query])
    nothing
end

"""
    semiring_provider(implementation; domain_id, division=false, star=false,
                      numeric=false, stable_bytes=false, batch=true,
                      parallel=false, thread_bound=true)

Publish arbitrary Julia weights through the host-semiring capability ABI.
Tokens use a recycling, generation-checked arena, and Julia methods run without
the arena lock. `parallel=true` is opt-in and incompatible with `thread_bound`.
Optional vtables are exposed only when explicitly enabled.
"""
function semiring_provider(implementation::AbstractSemiringProvider;
    domain_id::VTI.VtInterfaceId, division::Bool=false, star::Bool=false,
    numeric::Bool=false, stable_bytes::Bool=false, batch::Bool=true,
    parallel::Bool=false, thread_bound::Bool=true)
    parallel && thread_bound && throw(ArgumentError(
        "a provider cannot be both thread-bound and parallel-reentrant"))
    flags = (thread_bound ? VTI.SEMIRING_FLAG_THREAD_BOUND : UInt64(0)) |
        (parallel ? VTI.SEMIRING_FLAG_PARALLEL_REENTRANT : UInt64(0)) |
        (stable_bytes ? VTI.SEMIRING_FLAG_STABLE_BYTES : UInt64(0)) |
        (batch ? VTI.SEMIRING_FLAG_BATCH : UInt64(0))
    base = Ref(VTI.VtSemiringVTable(sizeof(VTI.VtSemiringVTable),
        VTI.SEMIRING_INTERFACE_VERSION, 0, flags, domain_id,
        SEMIRING_CALLBACKS[:zero], SEMIRING_CALLBACKS[:one],
        SEMIRING_CALLBACKS[:clone], SEMIRING_CALLBACKS[:release_values],
        SEMIRING_CALLBACKS[:plus], SEMIRING_CALLBACKS[:times],
        SEMIRING_CALLBACKS[:equal], SEMIRING_CALLBACKS[:approx],
        SEMIRING_CALLBACKS[:order], SEMIRING_CALLBACKS[:stable],
        SEMIRING_CALLBACKS[:diagnostic],
        batch ? SEMIRING_CALLBACKS[:plus_many] : C_NULL,
        batch ? SEMIRING_CALLBACKS[:times_many] : C_NULL))
    division_table = division ? Ref(VTI.VtSemiringDivisionVTable(
        sizeof(VTI.VtSemiringDivisionVTable), VTI.SEMIRING_DIVISION_INTERFACE_VERSION,
        0, SEMIRING_CALLBACKS[:divide], SEMIRING_CALLBACKS[:left_divide])) : nothing
    star_table = star ? Ref(VTI.VtSemiringStarVTable(sizeof(VTI.VtSemiringStarVTable),
        VTI.SEMIRING_STAR_INTERFACE_VERSION, 0, SEMIRING_CALLBACKS[:star])) : nothing
    numeric_table = numeric ? Ref(VTI.VtSemiringNumericVTable(
        sizeof(VTI.VtSemiringNumericVTable), VTI.SEMIRING_NUMERIC_INTERFACE_VERSION,
        0, SEMIRING_CALLBACKS[:numerical], SEMIRING_CALLBACKS[:quantize],
        SEMIRING_CALLBACKS[:probability])) : nothing
    properties = UInt64(semiring_properties(implementation))
    bound = semiring_closure_bound(implementation)
    isnothing(bound) || bound >= 0 || throw(ArgumentError("closure bound is negative"))
    properties_table = Ref(VTI.VtSemiringPropertiesVTable(
        sizeof(VTI.VtSemiringPropertiesVTable),
        VTI.SEMIRING_PROPERTIES_INTERFACE_VERSION, 0, properties,
        SEMIRING_CALLBACKS[:closure]))
    context = SemiringProviderContext(new_provider_cookie(), 1, implementation, flags,
        Base.Threads.threadid(), Base.Threads.Atomic{Bool}(false), properties,
        isnothing(bound) ? nothing : UInt(bound), ReentrantLock(),
        SemiringSlot[], Int[], "", base, division_table, star_table,
        numeric_table, properties_table)
    pointer = context.cookie
    lock(SEMIRING_PROVIDERS_LOCK) do
        SEMIRING_PROVIDERS[pointer] = context
    end
    raw = VTI.VtResourceRaw(pointer,
        Base.unsafe_convert(Ptr{VTI.VtResourceVTable}, SEMIRING_RESOURCE_TABLE))
    VTI.adopt_resource(raw; anchors=[context])
end

# Host-provider API ----------------------------------------------------------

"""Implement `wfst_start`, `wfst_state_count`, and `wfst_state` for this type."""
abstract type AbstractWfstProvider end

"""One type-stable provider arc. `nothing` denotes epsilon on either tape."""
struct ProviderArc{L,W<:AbstractScalarWeight}
    input::Union{Nothing,L}
    output::Union{Nothing,L}
    target::UInt64
    weight::W
    function ProviderArc{L,W}(input, output, target::Integer,
        weight=one(W)) where {L,W<:AbstractScalarWeight}
        unit_domain(L)
        weight_domain(W)
        target >= 0 || throw(ArgumentError("target cannot be negative"))
        target <= typemax(UInt64) || throw(ArgumentError("target is out of range"))
        typed_input = typed_provider_label(L, input)
        typed_output = typed_provider_label(L, output)
        new{L,W}(typed_input, typed_output, UInt64(target), typed_weight(W, weight))
    end
end
function typed_provider_label(::Type{L}, value) where {L}
    raw, present = wire_label(L, value, nothing)
    present == 0 ? nothing : decode_label(L, raw)
end
ProviderArc(input, output, target::Integer, weight=one(TropicalWeight)) =
    ProviderArc{Char,TropicalWeight}(input, output, target, weight)

"""One immutable state returned by a host provider."""
struct ProviderState{L,W<:AbstractScalarWeight}
    valid::Bool
    final::Bool
    final_weight::W
    arcs::Vector{ProviderArc{L,W}}
    function ProviderState{L,W}(; valid::Bool=true, final::Bool=false,
        final_weight=(final ? one(W) : zero(W)),
        arcs=ProviderArc{L,W}[]) where {L,W<:AbstractScalarWeight}
        unit_domain(L)
        weight_domain(W)
        value = typed_weight(W, final_weight)
        new{L,W}(valid, valid && final, valid && final ? value : zero(W),
            Vector{ProviderArc{L,W}}(arcs))
    end
end
ProviderState(; kwargs...) = ProviderState{Char,TropicalWeight}(; kwargs...)

"""Return a provider's non-negative start-state identifier."""
function wfst_start(provider::AbstractWfstProvider)
    throw(MethodError(wfst_start, (provider,)))
end
"""Return the provider's state count, or `nothing` when it is not known."""
wfst_state_count(::AbstractWfstProvider) = nothing
"""Expand `state` into one complete immutable `ProviderState`."""
function wfst_state(provider::AbstractWfstProvider, state::UInt64)
    throw(MethodError(wfst_state, (provider, state)))
end

mutable struct ProviderContext{L,W<:AbstractScalarWeight,P<:AbstractWfstProvider}
    cookie::Ptr{Cvoid}
    references::Int
    implementation::P
    unit_domain::VTI.UnitDomain
    weight_domain::VTI.WeightDomain
    flags::UInt64
    call_active::Base.Threads.Atomic{Bool}
    cache_lock::ReentrantLock
    states::Dict{UInt64,ProviderState{L,W}}
    last_error::String
    table::Base.RefValue{VTI.VtWfstVTable}
end

const PROVIDERS = Dict{Ptr{Cvoid},ProviderContext}()
const PROVIDERS_LOCK = ReentrantLock()
const RESOURCE_TABLE = Ref{VTI.VtResourceVTable}()
const CALLBACKS = Dict{Symbol,Ptr{Cvoid}}()

provider_context(pointer::Ptr{Cvoid}) = lock(PROVIDERS_LOCK) do
    get(PROVIDERS, pointer, nothing)
end

function record_error!(context::ProviderContext, error)
    try
        message = try
            sprint(showerror, error)
        catch
            "WFST provider raised an unprintable exception"
        end
        lock(context.cache_lock) do
            context.last_error = message
        end
    catch
    end
    nothing
end

function provider_call_gate(operation::Function, context::ProviderContext)
    context.flags & VTI.WFST_FLAG_PARALLEL_REENTRANT != 0 &&
        return operation()
    if Base.Threads.atomic_cas!(context.call_active, false, true)
        record_error!(context, ErrorException(
            "WFST provider callback is concurrent or recursive"))
        return Cint(VTI.STATUS_PROVIDER_ERROR)
    end
    try
        operation()
    finally
        context.call_active[] = false
    end
end

function provider_retain(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(PROVIDERS_LOCK) do
            context = get(PROVIDERS, pointer, nothing)
            if !isnothing(context)
                context.references == typemax(Int) ||
                    (context.references += 1)
            end
        end
    catch
    end
    nothing
end

function provider_release(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(PROVIDERS_LOCK) do
            context = get(PROVIDERS, pointer, nothing)
            if !isnothing(context) && context.references > 0
                context.references -= 1
                context.references == 0 && delete!(PROVIDERS, pointer)
            end
        end
    catch
    end
    nothing
end

function provider_query(pointer::Ptr{Cvoid}, id::Ptr{VTI.VtInterfaceId},
    minimum::UInt32, output::Ptr{Ptr{Cvoid}})::Cint
    (pointer == C_NULL || id == C_NULL || output == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    try
        unsafe_load(id) == VTI.WFST_INTERFACE_ID ||
            return Cint(VTI.STATUS_UNSUPPORTED)
        minimum <= VTI.WFST_INTERFACE_VERSION ||
            return Cint(VTI.STATUS_UNSUPPORTED)
        context = provider_context(pointer)
        isnothing(context) && return Cint(VTI.STATUS_CLOSED)
        unsafe_store!(output, Ptr{Cvoid}(Base.unsafe_convert(
            Ptr{VTI.VtWfstVTable}, context.table)))
        Cint(VTI.STATUS_OK)
    catch error
        context = provider_context(pointer)
        isnothing(context) || record_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function raw_provider(context::ProviderContext)
    VTI.VtResourceRaw(context.cookie,
        Base.unsafe_convert(Ptr{VTI.VtResourceVTable}, RESOURCE_TABLE))
end

function provider_snapshot(pointer::Ptr{Cvoid}, output::Ptr{VTI.VtResourceRaw})::Cint
    (pointer == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    try
        # Snapshot ownership must be acquired under the same registry lock as
        # the last release. A get-then-retain sequence can publish a dead
        # context when another thread closes the final owner in between.
        context = lock(PROVIDERS_LOCK) do
            value = get(PROVIDERS, pointer, nothing)
            if !isnothing(value)
                value.references == typemax(Int) &&
                    throw(OverflowError("provider reference count"))
                value.references += 1
            end
            value
        end
        isnothing(context) && return Cint(VTI.STATUS_CLOSED)
        unsafe_store!(output, raw_provider(context))
        Cint(VTI.STATUS_OK)
    catch error
        context = provider_context(pointer)
        isnothing(context) || record_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function provider_start(pointer::Ptr{Cvoid}, output::Ptr{UInt64})::Cint
    (pointer == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    context = provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        provider_call_gate(context) do
            value = wfst_start(context.implementation)
            value >= 0 || throw(ArgumentError("start state cannot be negative"))
            unsafe_store!(output, UInt64(value))
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function provider_count(pointer::Ptr{Cvoid}, output::Ptr{Csize_t},
    known::Ptr{UInt8})::Cint
    (pointer == C_NULL || output == C_NULL || known == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    context = provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        provider_call_gate(context) do
            value = wfst_state_count(context.implementation)
            if isnothing(value)
                unsafe_store!(output, Csize_t(0))
                unsafe_store!(known, UInt8(0))
            else
                value >= 0 || throw(ArgumentError("state count cannot be negative"))
                unsafe_store!(output, Csize_t(value))
                unsafe_store!(known, UInt8(1))
            end
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function cached_state(context::ProviderContext{L,W}, state::UInt64) where {L,W}
    cached = lock(context.cache_lock) do
        get(context.states, state, nothing)
    end
    cached === nothing || return cached
    # The customer callback deliberately runs outside the cache lock.
    expanded = wfst_state(context.implementation, state)
    expected = ProviderState{L,W}
    expanded isa expected || throw(ArgumentError(
        "wfst_state must return $expected for this provider"))
    # Cache an owned arc vector. A provider may retain the value it returned;
    # later mutation of that value must not rewrite an already-published ABI
    # state behind an immutable resource's back.
    owned = ProviderState{L,W}(valid=expanded.valid, final=expanded.final,
        final_weight=expanded.final_weight, arcs=expanded.arcs)
    lock(context.cache_lock) do
        get!(context.states, state, owned)
    end
end

function provider_state_info(pointer::Ptr{Cvoid}, state::UInt64,
    valid::Ptr{UInt8}, finality::Ptr{UInt8}, weight::Ptr{Float64})::Cint
    (pointer == C_NULL || valid == C_NULL || finality == C_NULL || weight == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    context = provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        provider_call_gate(context) do
            expanded = cached_state(context, state)
            unsafe_store!(valid, UInt8(expanded.valid))
            unsafe_store!(finality, UInt8(expanded.final))
            unsafe_store!(weight, raw_weight(expanded.final_weight))
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function provider_state_arcs(pointer::Ptr{Cvoid}, state::UInt64, start::Csize_t,
    output::Ptr{VTI.VtWfstArc}, capacity::Csize_t, written::Ptr{Csize_t},
    total::Ptr{Csize_t})::Cint
    (pointer == C_NULL || written == C_NULL || total == C_NULL ||
        (capacity != 0 && output == C_NULL)) && return Cint(VTI.STATUS_NULL_POINTER)
    context = provider_context(pointer)
    isnothing(context) && return Cint(VTI.STATUS_CLOSED)
    try
        provider_call_gate(context) do
            arcs = cached_state(context, state).arcs
            offset = Int(start)
            offset <= length(arcs) || throw(ArgumentError("arc offset exceeds total"))
            count = min(Int(capacity), length(arcs) - offset)
            for index in 1:count
                arc = arcs[offset + index]
                unsafe_store!(output, VTI.VtWfstArc(
                    something(arc.input, UInt64(0)), something(arc.output, UInt64(0)),
                    arc.target, raw_weight(arc.weight), UInt8(!isnothing(arc.input)),
                    UInt8(!isnothing(arc.output)), ntuple(_ -> UInt8(0), 6)), index)
            end
            unsafe_store!(written, Csize_t(count))
            unsafe_store!(total, Csize_t(length(arcs)))
            Cint(VTI.STATUS_OK)
        end
    catch error
        record_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function initialize_callbacks!()
    empty!(CALLBACKS)
    CALLBACKS[:retain] = @cfunction(provider_retain, Cvoid, (Ptr{Cvoid},))
    CALLBACKS[:release] = @cfunction(provider_release, Cvoid, (Ptr{Cvoid},))
    CALLBACKS[:query] = @cfunction(provider_query, Cint,
        (Ptr{Cvoid}, Ptr{VTI.VtInterfaceId}, UInt32, Ptr{Ptr{Cvoid}}))
    CALLBACKS[:snapshot] = @cfunction(provider_snapshot, Cint,
        (Ptr{Cvoid}, Ptr{VTI.VtResourceRaw}))
    CALLBACKS[:start] = @cfunction(provider_start, Cint, (Ptr{Cvoid}, Ptr{UInt64}))
    CALLBACKS[:count] = @cfunction(provider_count, Cint,
        (Ptr{Cvoid}, Ptr{Csize_t}, Ptr{UInt8}))
    CALLBACKS[:state_info] = @cfunction(provider_state_info, Cint,
        (Ptr{Cvoid}, UInt64, Ptr{UInt8}, Ptr{UInt8}, Ptr{Float64}))
    CALLBACKS[:state_arcs] = @cfunction(provider_state_arcs, Cint,
        (Ptr{Cvoid}, UInt64, Csize_t, Ptr{VTI.VtWfstArc}, Csize_t,
            Ptr{Csize_t}, Ptr{Csize_t}))
    RESOURCE_TABLE[] = VTI.VtResourceVTable(sizeof(VTI.VtResourceVTable),
        VTI.ABI_VERSION, 0, CALLBACKS[:retain], CALLBACKS[:release], CALLBACKS[:query])
    nothing
end

"""
    provider(Label, Weight, implementation; parallel=false, acyclic=false,
             input_symbols=nothing, output_symbols=nothing)

Expose an immutable Julia `AbstractWfstProvider` through `vt.scalar-wfst.1`.
State callbacks are cached once per state. Customer code never executes while
the facade holds its cache lock. Set `parallel=true` only when the provider is
safe for concurrent and reentrant calls.
"""
function provider(::Type{L}, ::Type{W}, implementation::P;
    parallel::Bool=false, acyclic::Bool=false,
    input_symbols=nothing, output_symbols=nothing) where
    {L,W<:AbstractScalarWeight,P<:AbstractWfstProvider}
    declared_unit_domain = unit_domain(L)
    declared_weight_domain = weight_domain(W)
    input_table = checked_symbols(L, input_symbols)
    output_table = checked_symbols(L, output_symbols)
    flags = VTI.WFST_FLAG_IMMUTABLE | VTI.WFST_FLAG_LAZY |
        (parallel ? VTI.WFST_FLAG_PARALLEL_REENTRANT : UInt64(0)) |
        (acyclic ? VTI.WFST_FLAG_ACYCLIC : UInt64(0))
    table = Ref(VTI.VtWfstVTable(sizeof(VTI.VtWfstVTable),
        VTI.WFST_INTERFACE_VERSION, UInt32(declared_unit_domain),
        UInt32(declared_weight_domain), 0,
        flags, CALLBACKS[:snapshot], CALLBACKS[:start], CALLBACKS[:count],
        CALLBACKS[:state_info], CALLBACKS[:state_arcs]))
    context = ProviderContext{L,W,P}(new_provider_cookie(), 1, implementation, declared_unit_domain,
        declared_weight_domain, flags, Base.Threads.Atomic{Bool}(false), ReentrantLock(),
        Dict{UInt64,ProviderState{L,W}}(), "", table)
    pointer = context.cookie
    lock(PROVIDERS_LOCK) do
        PROVIDERS[pointer] = context
    end
    raw = raw_provider(context)
    native_wfst = VTI.wfstransducer(
        VTI.adopt_resource(raw; anchors=[context]); take=true)
    Wfst(native_wfst, L, W; input_symbols=input_table, output_symbols=output_table)
end

provider(implementation::AbstractWfstProvider; kwargs...) =
    provider(Char, TropicalWeight, implementation; kwargs...)

function __init__()
    initialize_callbacks!()
    initialize_semiring_callbacks!()
    abi_version() == ABI_VERSION || error(
        "lling-llang ABI $(abi_version()) does not match Julia facade $ABI_VERSION")
    api_revision() >= API_REVISION || error(
        "lling-llang API revision $(api_revision()) is older than $API_REVISION")
end

end # module
