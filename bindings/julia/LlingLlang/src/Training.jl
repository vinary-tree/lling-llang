"""Bounded native LF-MMI controls for an owned acoustic score matrix."""
struct LfMmiConfig
    leaky_hmm_coefficient::Float64
    l2_regularize::Float64
    xent_regularize::Float64
    function LfMmiConfig(;
        leaky_hmm_coefficient::Real=0.1,
        l2_regularize::Real=0.0001,
        xent_regularize::Real=0.1)
        coefficients = Float64.((leaky_hmm_coefficient, l2_regularize,
            xent_regularize))
        all(value -> isfinite(value) && value >= 0, coefficients) ||
            throw(ArgumentError("LF-MMI coefficients must be finite and nonnegative"))
        new(coefficients...)
    end
end

struct LfMmiConfigRaw
    struct_size::UInt32
    version::UInt32
    leaky_hmm_coefficient::Float64
    l2_regularize::Float64
    xent_regularize::Float64
    use_chain_topology::UInt8
    reserved::NTuple{7,UInt8}
    subsampling_factor::UInt64
end

function raw_config(config::LfMmiConfig)
    LfMmiConfigRaw(sizeof(LfMmiConfigRaw), 1,
        config.leaky_hmm_coefficient, config.l2_regularize,
        config.xent_regularize, UInt8(1),
        ntuple(_ -> UInt8(0), 7), UInt64(3))
end

struct LfMmiSummaryRaw
    loss::Float64
    numerator_log_prob::Float64
    denominator_log_prob::Float64
    xent_loss::Float64
end

"""Native LF-MMI scalar result and an owned Julia training gradient matrix."""
struct LfMmiResult
    loss::Float64
    numerator_log_prob::Float64
    denominator_log_prob::Float64
    xent_loss::Float64
    gradients::Matrix{Float64}
end

"""The cooperative cancellation token was requested during native training."""
struct TrainingCancelledError <: Exception
    reason::CancellationReasonV2
end
Base.showerror(io::IO, error::TrainingCancelledError) =
    print(io, "native LF-MMI training cancelled: ", error.reason)

struct ArcGradientRaw
    from::UInt32
    arc_index::UInt32
    gradient::Float64
end

"""Build a bounded native CTC topology with blank label zero."""
function ctc_topology(vocab_size::Integer; topology::Symbol=:compact,
    budget::BudgetV2=BudgetV2(max_states=4096, max_arcs=131072,
        max_bytes=64 * 1024 * 1024, max_work=2097152),
    cancellation::Union{Nothing,CancellationV2}=nothing)
    1 <= vocab_size <= 4096 || throw(ArgumentError("CTC vocabulary must be in 1:4096"))
    variant = if topology === :correct
        UInt32(0)
    elseif topology === :compact
        UInt32(1)
    elseif topology === :minimal
        UInt32(2)
    elseif topology === :selfless_correct
        UInt32(3)
    elseif topology === :selfless_compact
        UInt32(4)
    else
        throw(ArgumentError("unknown CTC topology $(topology)"))
    end
    raw_budget = Ref(budget)
    token = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve cancellation begin
        status = ccall(native(:lling_ctc_topology), UInt32,
            (UInt32, Csize_t, Ref{BudgetV2}, Ptr{Cvoid}, Ref{Ptr{Cvoid}}),
            variant, vocab_size, raw_budget, token, output)
        if status != UInt32(STATUS_OK)
            if !isnothing(cancellation)
                reason = cancellation_reason(cancellation)
                isnothing(reason) || throw(TrainingCancelledError(reason))
            end
            checked(status, :ctc_topology)
        end
    end
    adopt_native_wfst(output[], UInt64, LogWeight)
end

"""An arc's derivative, addressed by source state and zero-based arc index."""
struct ArcGradient
    from::UInt32
    arc_index::UInt32
    gradient::Float64
end

"""Owned exact forward score and paged arc gradients for an acyclic log WFST."""
mutable struct LogGradientCursor
    handle::Ptr{Cvoid}
    score::Float64
    arc_count::Int
    closed::Bool
end
Base.isopen(cursor::LogGradientCursor) = !cursor.closed
function close!(cursor::LogGradientCursor)
    cursor.closed && return nothing
    ccall(native(:lling_log_gradients_free), Cvoid, (Ptr{Cvoid},), cursor.handle)
    cursor.handle = C_NULL
    cursor.closed = true
    nothing
end
Base.close(cursor::LogGradientCursor) = close!(cursor)

"""Capture a bounded acyclic `UInt64`/`LogWeight` graph and calculate arc derivatives."""
function log_gradient_cursor(source;
    budget::BudgetV2=BudgetV2(max_states=4096, max_arcs=131072,
        max_bytes=64 * 1024 * 1024, max_work=2097152),
    cancellation::Union{Nothing,CancellationV2}=nothing)
    raw = Ref(raw_resource(source))
    raw_budget = Ref(budget)
    token = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    score = Ref{Float64}(0)
    count = Ref{Csize_t}(0)
    GC.@preserve source cancellation begin
        status = ccall(native(:lling_log_gradients_open), UInt32,
            (Ref{VTI.VtResourceRaw}, Ref{BudgetV2}, Ptr{Cvoid},
             Ref{Ptr{Cvoid}}, Ref{Float64}, Ref{Csize_t}),
            raw, raw_budget, token, output, score, count)
        if status != UInt32(STATUS_OK)
            if !isnothing(cancellation)
                reason = cancellation_reason(cancellation)
                isnothing(reason) || throw(TrainingCancelledError(reason))
            end
            checked(status, :log_gradients_open)
        end
    end
    cursor = LogGradientCursor(output[], score[], Int(count[]), false)
    finalizer(finalize_close, cursor)
    cursor
end

"""Fetch at most `capacity` arc derivatives from `start` (zero-based)."""
function gradient_page(cursor::LogGradientCursor, start::Integer;
    capacity::Integer=1024)
    isopen(cursor) || throw(ArgumentError("log-gradient cursor is closed"))
    0 <= start <= cursor.arc_count || throw(BoundsError(cursor, start))
    0 < capacity <= 131072 || throw(ArgumentError("gradient page size must be in 1:131072"))
    buffer = Vector{ArcGradientRaw}(undef, min(capacity, cursor.arc_count - start))
    written = Ref{Csize_t}(0)
    checked(ccall(native(:lling_log_gradients_page), UInt32,
        (Ptr{Cvoid}, Csize_t, Ptr{ArcGradientRaw}, Csize_t, Ref{Csize_t}),
        cursor.handle, start, buffer, length(buffer), written),
        :log_gradients_page)
    [ArcGradient(item.from, item.arc_index, item.gradient)
        for item in @view buffer[1:Int(written[])]]
end

struct LogGradientPages
    cursor::LogGradientCursor
    page_size::Int
end
Base.length(pages::LogGradientPages) = cld(pages.cursor.arc_count, pages.page_size)
Base.eltype(::Type{LogGradientPages}) = Vector{ArcGradient}
Base.iterate(pages::LogGradientPages, start::Int=0) =
    start >= pages.cursor.arc_count ? nothing : begin
        page = gradient_page(pages.cursor, start; capacity=pages.page_size)
        (page, start + length(page))
    end

"""Iterate bounded native pages; close the cursor after use."""
function gradient_pages(cursor::LogGradientCursor; page_size::Integer=1024)
    0 < page_size <= 131072 || throw(ArgumentError("page_size must be in 1:131072"))
    LogGradientPages(cursor, Int(page_size))
end

"""Owned scalar score and all arc derivatives from paged native reads."""
struct LogGradientResult
    score::Float64
    arcs::Vector{ArcGradient}
end

"""Compute exact log-semiring path score and collect arc derivatives."""
function log_gradients(source; page_size::Integer=1024, kwargs...)
    cursor = log_gradient_cursor(source; kwargs...)
    try
        arcs = Vector{ArcGradient}()
        sizehint!(arcs, cursor.arc_count)
        for page in gradient_pages(cursor; page_size)
            append!(arcs, page)
        end
        LogGradientResult(cursor.score, arcs)
    finally
        close(cursor)
    end
end

"""
    lfmmi_loss(scores, numerator, denominator; config, budget, cancellation)

Call Rust LF-MMI over two immutable `UInt64`/`LogWeight` scalar-WFST resources.
`scores` is a contiguous `Matrix{Float64}` with frames in rows and PDF labels
in columns. Native import checks every PDF label, normalizes bounded full
epsilon arcs, and rejects out-of-range labels, non-finite results, and missing accepting paths. The
returned gradient matrix has the same shape and Julia column-major layout.
The native call retains the input resources for its duration; the caller owns
both the inputs and returned matrix. Cancellation is checked between bounded
phases; the Rust objective runs to completion once entered.
The auxiliary cross-entropy target is held fixed for its training gradient.
"""
function lfmmi_loss(scores::Matrix{Float64}, numerator, denominator;
    config::LfMmiConfig=LfMmiConfig(),
    budget::BudgetV2=BudgetV2(max_states=4096, max_arcs=131072,
        max_bytes=64 * 1024 * 1024, max_work=2097152),
    cancellation::Union{Nothing,CancellationV2}=nothing)
    frames, pdfs = size(scores)
    frames > 0 && pdfs > 0 ||
        throw(ArgumentError("LF-MMI scores require positive frame and PDF counts"))
    length(scores) <= 1_048_576 ||
        throw(ArgumentError("LF-MMI scores exceed the native cell limit"))
    all(value -> !isnan(value) && value != Inf, scores) ||
        throw(ArgumentError("LF-MMI scores must be finite or -Inf"))
    if config.xent_regularize > 0 || config.l2_regularize > 0
        all(isfinite, scores) ||
            throw(ArgumentError("regularized LF-MMI scores must be finite"))
    end
    gradients = Matrix{Float64}(undef, frames, pdfs)
    raw_numerator = Ref(raw_resource(numerator))
    raw_denominator = Ref(raw_resource(denominator))
    raw_budget = Ref(budget)
    raw_controls = Ref(raw_config(config))
    summary = Ref(LfMmiSummaryRaw(0, 0, 0, 0))
    token = isnothing(cancellation) ? C_NULL : open_handle(cancellation)
    GC.@preserve numerator denominator scores gradients cancellation begin
        status = ccall(native(:lling_lfmmi_loss_ref), UInt32,
            (Ref{VTI.VtResourceRaw}, Ref{VTI.VtResourceRaw},
             Ref{BudgetV2}, Ptr{Cvoid}, Ptr{Float64}, Csize_t, Csize_t,
             Csize_t, Csize_t, Ref{LfMmiConfigRaw}, Ref{LfMmiSummaryRaw},
             Ptr{Float64}, Csize_t, Csize_t, Csize_t),
            raw_numerator, raw_denominator, raw_budget, token,
            pointer(scores), frames, pdfs, 1, frames, raw_controls,
            summary, pointer(gradients), length(gradients), 1, frames)
        if status != UInt32(STATUS_OK)
            if !isnothing(cancellation)
                reason = cancellation_reason(cancellation)
                isnothing(reason) || throw(TrainingCancelledError(reason))
            end
            checked(status, :lfmmi_loss_ref)
        end
    end
    data = summary[]
    LfMmiResult(data.loss, data.numerator_log_prob,
        data.denominator_log_prob, data.xent_loss, gradients)
end
