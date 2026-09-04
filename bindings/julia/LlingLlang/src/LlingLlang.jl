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
    WfstBuilder,
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
    add_state!,
    set_start!,
    set_final!,
    clear_final!,
    add_arc!,
    build!,
    import_wfst,
    compose,
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

"""Mutable Unicode/tropical WFST builder. `build!` consumes it on success."""
mutable struct WfstBuilder
    handle::Ptr{Cvoid}
    closed::Bool
end

function WfstBuilder(; size_hint::Integer=0)
    size_hint >= 0 || throw(ArgumentError("size_hint cannot be negative"))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_wfst_builder_new), UInt32,
        (Ref{Ptr{Cvoid}},), output), :wfst_builder_new)
    builder = WfstBuilder(output[], false)
    finalizer(finalize_close, builder)
    size_hint == 0 || reserve_states!(builder, size_hint)
    builder
end

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

function valid_weight(weight::Real)
    value = Float64(weight)
    (!isnan(value) && value != -Inf) ||
        throw(ArgumentError("tropical weights must be finite or +Inf"))
    value
end

"""Mark `state` final with a tropical final `weight`."""
function set_final!(builder::WfstBuilder, state::Integer, weight::Real=0.0)
    checked(ccall(native(:lling_wfst_builder_set_final), UInt32,
        (Ptr{Cvoid}, UInt32, Float64), open_handle(builder), state,
        valid_weight(weight)), :set_final)
    builder
end

"""Remove finality and its weight from `state`."""
function clear_final!(builder::WfstBuilder, state::Integer)
    checked(ccall(native(:lling_wfst_builder_clear_final), UInt32,
        (Ptr{Cvoid}, UInt32), open_handle(builder), state), :clear_final)
    builder
end

wire_label(::Nothing) = (UInt64(0), UInt8(0))
wire_label(value::Char) = (UInt64(value), UInt8(1))
function wire_label(value::AbstractString)
    characters = collect(value)
    length(characters) == 1 || throw(ArgumentError("a WFST label is one Unicode scalar"))
    wire_label(only(characters))
end
function wire_label(value::Integer)
    value >= 0 || throw(ArgumentError("a WFST label cannot be negative"))
    scalar = UInt32(value)
    isvalid(Char, scalar) || throw(ArgumentError("a WFST label must be a Unicode scalar"))
    (UInt64(scalar), UInt8(1))
end

"""
Append an arc from `from` to `target`.

Each label is one `Char`, one-character string, Unicode-scalar integer, or
`nothing` for epsilon. The default tropical weight is zero.
"""
function add_arc!(builder::WfstBuilder, from::Integer, input, output,
    target::Integer, weight::Real=0.0)
    input_value, has_input = wire_label(input)
    output_value, has_output = wire_label(output)
    checked(ccall(native(:lling_wfst_builder_add_arc), UInt32,
        (Ptr{Cvoid}, UInt32, UInt64, UInt8, UInt64, UInt8, UInt32, Float64),
        open_handle(builder), from, input_value, has_input, output_value,
        has_output, target, valid_weight(weight)), :add_arc)
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

function adopt_native_wfst(handle::Ptr{Cvoid})
    raw = Ref(VTI.VtResourceRaw(C_NULL, Ptr{VTI.VtResourceVTable}(C_NULL)))
    try
        checked(ccall(native(:lling_wfst_resource), UInt32,
            (Ptr{Cvoid}, Ref{VTI.VtResourceRaw}), handle, raw), :wfst_resource)
        VTI.wfstransducer(VTI.adopt_resource(raw[]); take=true)
    finally
        ccall(native(:lling_wfst_free), Cvoid, (Ptr{Cvoid},), handle)
    end
end

"""Consume `builder` and return its immutable interoperable WFST."""
function build!(builder::WfstBuilder)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_wfst_builder_build), UInt32,
        (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), open_handle(builder), output), :build)
    close!(builder)
    adopt_native_wfst(output[])
end

raw_resource(resource::VTI.Resource) = VTI.raw_resource(resource)
raw_resource(wfst::VTI.Wfst) = VTI.raw_resource(wfst.resource)

"""Return one independent retained resource for a WFST."""
resource(wfst::VTI.Wfst) = VTI.retain(wfst.resource)

"""Copy a compatible immutable resource into a native lling-llang WFST."""
function import_wfst(source::Union{VTI.Resource,VTI.Wfst})
    output = Ref{Ptr{Cvoid}}(C_NULL)
    raw = raw_resource(source)
    checked(ccall(native(:lling_wfst_import), UInt32,
        (VTI.VtResourceRaw, Ref{Ptr{Cvoid}}), raw, output), :wfst_import)
    adopt_native_wfst(output[])
end

"""
Lazily compose snapshots of two scalar WFST resources.

The product joins the first output tape to the second input tape and adds
matching tropical weights.
"""
function compose(first::Union{VTI.Resource,VTI.Wfst},
    second::Union{VTI.Resource,VTI.Wfst})
    output = Ref{Ptr{Cvoid}}(C_NULL)
    checked(ccall(native(:lling_wfst_compose), UInt32,
        (VTI.VtResourceRaw, VTI.VtResourceRaw, Ref{Ptr{Cvoid}}),
        raw_resource(first), raw_resource(second), output), :wfst_compose)
    adopt_native_wfst(output[])
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
    references::Int
    implementation::AbstractSemiringProvider
    flags::UInt64
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
const SEMIRING_RESOURCE_TABLE = Ref{VTI.VtResourceVTable}()
const SEMIRING_CALLBACKS = Dict{Symbol,Ptr{Cvoid}}()

semiring_provider_context(pointer::Ptr{Cvoid}) =
    unsafe_pointer_to_objref(pointer)::SemiringProviderContext

function record_semiring_error!(context::SemiringProviderContext, error)
    lock(context.arena_lock) do
        context.last_error = sprint(showerror, error)
    end
    nothing
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

function release_semiring_value!(context::SemiringProviderContext,
    token::VTI.VtSemiringValue)
    lock(context.arena_lock) do
        index = Int(token.word0)
        1 <= index <= length(context.slots) ||
            throw(ArgumentError("semiring token slot is out of range"))
        slot = context.slots[index]
        slot.occupied && slot.generation == token.word1 ||
            throw(ArgumentError("semiring token is stale or already released"))
        slot.references > 0 || throw(ArgumentError("weight reference count underflow"))
        slot.references -= 1
        if slot.references == 0
            slot.value = nothing
            slot.occupied = false
            slot.generation = slot.generation == typemax(UInt64) ? UInt64(1) :
                slot.generation + UInt64(1)
            push!(context.free_slots, index)
        end
    end
    nothing
end

function semiring_resource_retain(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(SEMIRING_PROVIDERS_LOCK) do
            semiring_provider_context(pointer).references += 1
        end
    catch
    end
    nothing
end

function semiring_resource_release(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(SEMIRING_PROVIDERS_LOCK) do
            context = semiring_provider_context(pointer)
            context.references -= 1
            context.references == 0 && delete!(SEMIRING_PROVIDERS, pointer)
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
    try
        operation(context)
        Cint(VTI.STATUS_OK)
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
        for index in 1:Int(count)
            release_semiring_value!(context, unsafe_load(values, index))
        end
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
    try
        result = operation(context.implementation,
            resolve_semiring_value(context, unsafe_load(left)),
            resolve_semiring_value(context, unsafe_load(right)))
        isnothing(result) && return Cint(VTI.STATUS_END)
        unsafe_store!(output, allocate_semiring_value(context, result))
        Cint(VTI.STATUS_OK)
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
    try
        result = semiring_star(context.implementation,
            resolve_semiring_value(context, unsafe_load(value)))
        isnothing(result) && return Cint(VTI.STATUS_END)
        unsafe_store!(output, allocate_semiring_value(context, result))
        Cint(VTI.STATUS_OK)
    catch error
        record_semiring_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function semiring_numeric_callback(pointer::Ptr{Cvoid}, value::Ptr{VTI.VtSemiringValue},
    output::Ptr{Float64}, operation::Function)::Cint
    (value == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    context = semiring_provider_context(pointer)
    try
        result = operation(context.implementation,
            resolve_semiring_value(context, unsafe_load(value)))
        isnothing(result) && return Cint(VTI.STATUS_UNSUPPORTED)
        unsafe_store!(output, Float64(result))
        Cint(VTI.STATUS_OK)
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
    try
        result = semiring_quantize(context.implementation,
            resolve_semiring_value(context, unsafe_load(value)), epsilon)
        isnothing(result) && return Cint(VTI.STATUS_UNSUPPORTED)
        unsafe_store!(output, Int64(result))
        Cint(VTI.STATUS_OK)
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
    context = SemiringProviderContext(1, implementation, flags, properties,
        isnothing(bound) ? nothing : UInt(bound), ReentrantLock(), SemiringSlot[],
        Int[], "", base, division_table, star_table, numeric_table, properties_table)
    pointer = pointer_from_objref(context)
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

"""One provider arc. `nothing` denotes epsilon on either tape."""
struct ProviderArc
    input::Union{Nothing,UInt64}
    output::Union{Nothing,UInt64}
    target::UInt64
    weight::Float64
    function ProviderArc(input, output, target::Integer, weight::Real=0.0)
        input_value = isnothing(input) ? nothing : first(wire_label(input))
        output_value = isnothing(output) ? nothing : first(wire_label(output))
        target >= 0 || throw(ArgumentError("target cannot be negative"))
        new(input_value, output_value, UInt64(target), valid_weight(weight))
    end
end

"""One immutable state returned by a host provider."""
struct ProviderState
    valid::Bool
    final::Bool
    final_weight::Float64
    arcs::Vector{ProviderArc}
    function ProviderState(; valid::Bool=true, final::Bool=false,
        final_weight::Real=(final ? 0.0 : Inf), arcs=ProviderArc[])
        value = valid_weight(final_weight)
        new(valid, valid && final, valid && final ? value : Inf,
            Vector{ProviderArc}(arcs))
    end
end

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

mutable struct ProviderContext
    references::Int
    implementation::AbstractWfstProvider
    unit_domain::VTI.UnitDomain
    weight_domain::VTI.WeightDomain
    flags::UInt64
    cache_lock::ReentrantLock
    states::Dict{UInt64,ProviderState}
    last_error::String
    table::Base.RefValue{VTI.VtWfstVTable}
end

const PROVIDERS = Dict{Ptr{Cvoid},ProviderContext}()
const PROVIDERS_LOCK = ReentrantLock()
const RESOURCE_TABLE = Ref{VTI.VtResourceVTable}()
const CALLBACKS = Dict{Symbol,Ptr{Cvoid}}()

provider_context(pointer::Ptr{Cvoid}) =
    unsafe_pointer_to_objref(pointer)::ProviderContext

function record_error!(context::ProviderContext, error)
    lock(context.cache_lock) do
        context.last_error = sprint(showerror, error)
    end
    nothing
end

function provider_retain(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(PROVIDERS_LOCK) do
            provider_context(pointer).references += 1
        end
    catch
    end
    nothing
end

function provider_release(pointer::Ptr{Cvoid})::Cvoid
    try
        lock(PROVIDERS_LOCK) do
            context = provider_context(pointer)
            context.references -= 1
            context.references == 0 && delete!(PROVIDERS, pointer)
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
        unsafe_store!(output, Ptr{Cvoid}(Base.unsafe_convert(
            Ptr{VTI.VtWfstVTable}, context.table)))
        Cint(VTI.STATUS_OK)
    catch error
        try record_error!(provider_context(pointer), error) catch end
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function raw_provider(context::ProviderContext)
    pointer = pointer_from_objref(context)
    VTI.VtResourceRaw(pointer,
        Base.unsafe_convert(Ptr{VTI.VtResourceVTable}, RESOURCE_TABLE))
end

function provider_snapshot(pointer::Ptr{Cvoid}, output::Ptr{VTI.VtResourceRaw})::Cint
    (pointer == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    try
        provider_retain(pointer)
        unsafe_store!(output, raw_provider(provider_context(pointer)))
        Cint(VTI.STATUS_OK)
    catch error
        try record_error!(provider_context(pointer), error) catch end
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function provider_start(pointer::Ptr{Cvoid}, output::Ptr{UInt64})::Cint
    (pointer == C_NULL || output == C_NULL) && return Cint(VTI.STATUS_NULL_POINTER)
    context = provider_context(pointer)
    try
        value = wfst_start(context.implementation)
        value >= 0 || throw(ArgumentError("start state cannot be negative"))
        unsafe_store!(output, UInt64(value))
        Cint(VTI.STATUS_OK)
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
    try
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
    catch error
        record_error!(context, error)
        Cint(VTI.STATUS_PROVIDER_ERROR)
    end
end

function cached_state(context::ProviderContext, state::UInt64)
    cached = lock(context.cache_lock) do
        get(context.states, state, nothing)
    end
    cached === nothing || return cached
    # The customer callback deliberately runs outside the cache lock.
    expanded = wfst_state(context.implementation, state)
    expanded isa ProviderState ||
        throw(ArgumentError("wfst_state must return ProviderState"))
    lock(context.cache_lock) do
        get!(context.states, state, expanded)
    end
end

function provider_state_info(pointer::Ptr{Cvoid}, state::UInt64,
    valid::Ptr{UInt8}, finality::Ptr{UInt8}, weight::Ptr{Float64})::Cint
    (pointer == C_NULL || valid == C_NULL || finality == C_NULL || weight == C_NULL) &&
        return Cint(VTI.STATUS_NULL_POINTER)
    context = provider_context(pointer)
    try
        expanded = cached_state(context, state)
        unsafe_store!(valid, UInt8(expanded.valid))
        unsafe_store!(finality, UInt8(expanded.final))
        unsafe_store!(weight, expanded.final_weight)
        Cint(VTI.STATUS_OK)
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
    try
        arcs = cached_state(context, state).arcs
        offset = Int(start)
        offset <= length(arcs) || throw(ArgumentError("arc offset exceeds total"))
        count = min(Int(capacity), length(arcs) - offset)
        for index in 1:count
            arc = arcs[offset + index]
            unsafe_store!(output, VTI.VtWfstArc(
                something(arc.input, UInt64(0)), something(arc.output, UInt64(0)),
                arc.target, arc.weight, UInt8(!isnothing(arc.input)),
                UInt8(!isnothing(arc.output)), ntuple(_ -> UInt8(0), 6)), index)
        end
        unsafe_store!(written, Csize_t(count))
        unsafe_store!(total, Csize_t(length(arcs)))
        Cint(VTI.STATUS_OK)
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
    provider(implementation; unit_domain=UNIT_UNICODE_SCALAR,
             weight_domain=WEIGHT_TROPICAL_F64, parallel=false, acyclic=false)

Expose an immutable Julia `AbstractWfstProvider` through `vt.scalar-wfst.1`.
State callbacks are cached once per state. Customer code never executes while
the facade holds its cache lock. Set `parallel=true` only when the provider is
safe for concurrent and reentrant calls.
"""
function provider(implementation::AbstractWfstProvider;
    unit_domain::VTI.UnitDomain=VTI.UNIT_UNICODE_SCALAR,
    weight_domain::VTI.WeightDomain=VTI.WEIGHT_TROPICAL_F64,
    parallel::Bool=false, acyclic::Bool=false)
    flags = VTI.WFST_FLAG_IMMUTABLE | VTI.WFST_FLAG_LAZY |
        (parallel ? VTI.WFST_FLAG_PARALLEL_REENTRANT : UInt64(0)) |
        (acyclic ? VTI.WFST_FLAG_ACYCLIC : UInt64(0))
    table = Ref(VTI.VtWfstVTable(sizeof(VTI.VtWfstVTable),
        VTI.WFST_INTERFACE_VERSION, UInt32(unit_domain), UInt32(weight_domain), 0,
        flags, CALLBACKS[:snapshot], CALLBACKS[:start], CALLBACKS[:count],
        CALLBACKS[:state_info], CALLBACKS[:state_arcs]))
    context = ProviderContext(1, implementation, unit_domain, weight_domain,
        flags, ReentrantLock(), Dict{UInt64,ProviderState}(), "", table)
    pointer = pointer_from_objref(context)
    lock(PROVIDERS_LOCK) do
        PROVIDERS[pointer] = context
    end
    raw = raw_provider(context)
    VTI.wfstransducer(VTI.adopt_resource(raw; anchors=[context]); take=true)
end

function __init__()
    initialize_callbacks!()
    initialize_semiring_callbacks!()
    abi_version() == ABI_VERSION || error(
        "lling-llang ABI $(abi_version()) does not match Julia facade $ABI_VERSION")
    api_revision() >= API_REVISION || error(
        "lling-llang API revision $(api_revision()) is older than $API_REVISION")
end

end # module
