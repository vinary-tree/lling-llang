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
    NativeError,
    WfstBuilder,
    ProviderArc,
    ProviderState,
    AbstractWfstProvider,
    abi_version,
    api_revision,
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
"""Return the ABI version exported by the loaded native library."""
abi_version() = UInt32(ccall(native(:lling_abi_version), UInt32, ()))
"""Return the additive API revision exported by the loaded native library."""
api_revision() = UInt32(ccall(native(:lling_api_revision), UInt32, ()))

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
    abi_version() == ABI_VERSION || error(
        "lling-llang ABI $(abi_version()) does not match Julia facade $ABI_VERSION")
    api_revision() >= API_REVISION || error(
        "lling-llang API revision $(api_revision()) is older than $API_REVISION")
end

end # module
