"""Bounded scoring and work limits for native token-WFST decoding."""
struct LlmConfig
    beam_width::Int
    max_length::Int
    max_candidates_per_step::Int
    max_vocabulary::Int
    temperature::Float64
    length_penalty::Float64
end

function LlmConfig(; beam_width::Integer=8, max_length::Integer=128,
    max_candidates_per_step::Integer=65_536, max_vocabulary::Integer=262_144,
    temperature::Real=1.0, length_penalty::Real=0.0)
    0 < beam_width <= 65_536 || throw(ArgumentError("beam_width must be in 1:65536"))
    0 <= max_length <= 1_048_576 || throw(ArgumentError("max_length must be in 0:1048576"))
    0 < max_candidates_per_step <= 1_048_576 ||
        throw(ArgumentError("max_candidates_per_step must be in 1:1048576"))
    0 < max_vocabulary <= 1_048_576 ||
        throw(ArgumentError("max_vocabulary must be in 1:1048576"))
    isfinite(temperature) && temperature > 0 ||
        throw(ArgumentError("temperature must be finite and positive"))
    isfinite(length_penalty) && length_penalty >= 0 ||
        throw(ArgumentError("length_penalty must be finite and nonnegative"))
    LlmConfig(Int(beam_width), Int(max_length), Int(max_candidates_per_step),
        Int(max_vocabulary), Float64(temperature), Float64(length_penalty))
end

struct RawLlmConfig
    version::UInt32
    reserved::UInt32
    beam_width::Csize_t
    max_length::Csize_t
    max_candidates_per_step::Csize_t
    max_vocabulary::Csize_t
    temperature::Float64
    length_penalty::Float64
end
RawLlmConfig(config::LlmConfig) = RawLlmConfig(1, 0,
    config.beam_width, config.max_length, config.max_candidates_per_step,
    config.max_vocabulary, config.temperature, config.length_penalty)

"""One copied partial or accepting token hypothesis. Token IDs are zero based."""
struct LlmHypothesis
    tokens::Vector{UInt32}
    score::Float64
    accepting::Bool
end

"""Committed native decoder work and current beam size."""
struct LlmStats
    steps::UInt64
    score_calls::UInt64
    considered_tokens::UInt64
    candidates::UInt64
    hypotheses::Int
    finished::Bool
end

struct RawLlmStats
    steps::UInt64
    score_calls::UInt64
    considered_tokens::UInt64
    candidates::UInt64
    hypotheses::Csize_t
    finished::UInt8
    reserved::NTuple{7,UInt8}
end

@enum LlmStep::UInt32 begin
    LLM_ADVANCED = 0
    LLM_COMPLETE = 1
    LLM_EXHAUSTED = 2
    LLM_CANCELLED = 3
end

"""Exclusive native decoder; `cancel!` uses an independently owned atomic token."""
mutable struct LlmSession
    handle::Ptr{Cvoid}
    cancellation::Ptr{Cvoid}
    closed::Bool
end

function llm_handle(session::LlmSession)
    session.closed && throw(NativeError(STATUS_CLOSED, :llm_session, "decoder is closed"))
    session.handle
end

"""Capture a deterministic UInt64/TropicalWeight token acceptor as a decoder.

`source` remains owned by the caller. Native construction imports and validates
its complete graph under `budget`, then retains an independent copy. Each arc
must consume and emit the same in-vocabulary token, with no epsilon or
duplicate outgoing label. `vocab_size` counts zero-based token IDs.
"""
function llm_session(source; vocab_size::Integer,
    config::LlmConfig=LlmConfig(),
    budget::BudgetV2=BudgetV2(max_states=65_536, max_arcs=1_048_576,
        max_bytes=128 * 1024 * 1024, max_work=8_388_608))
    0 < vocab_size <= config.max_vocabulary ||
        throw(ArgumentError("vocab_size exceeds the configured range"))
    raw = Ref(raw_resource(source))
    native_config = Ref(RawLlmConfig(config))
    budget_ref = Ref(budget)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    status = GC.@preserve source raw native_config budget_ref ccall(
        native(:lling_llm_session_open_wfst), UInt32,
        (Ref{VTI.VtResourceRaw}, Csize_t, Ref{RawLlmConfig},
            Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
        raw, vocab_size, native_config, budget_ref, output)
    checked(status, :llm_session_open_wfst)
    token = Ref{Ptr{Cvoid}}(C_NULL)
    try
        checked(ccall(native(:lling_llm_session_cancellation), UInt32,
            (Ptr{Cvoid}, Ref{Ptr{Cvoid}}), output[], token),
            :llm_session_cancellation)
    catch
        ccall(native(:lling_llm_session_free), Cvoid, (Ptr{Cvoid},), output[])
        rethrow()
    end
    session = LlmSession(output[], token[], false)
    finalizer(finalize_close, session)
    session
end

"""Request cancellation, including while another task is scoring one step.

The caller keeps the session alive until that step returns. The native token
uses an atomic flag and does not borrow the mutable decoder.
"""
function cancel!(session::LlmSession)
    llm_handle(session)
    checked(ccall(native(:lling_llm_cancellation_request), UInt32,
        (Ptr{Cvoid},), session.cancellation), :llm_cancellation_request)
    session
end

function close!(session::LlmSession)
    session.closed && return nothing
    ccall(native(:lling_llm_session_free), Cvoid, (Ptr{Cvoid},), session.handle)
    ccall(native(:lling_llm_cancellation_free), Cvoid,
        (Ptr{Cvoid},), session.cancellation)
    session.handle = C_NULL
    session.cancellation = C_NULL
    session.closed = true
    nothing
end
Base.close(session::LlmSession) = close!(session)
Base.isopen(session::LlmSession) = !session.closed

mutable struct LlmScoreContext
    scorer::Any
    error::Any
end

function llm_score_callback(context::Ptr{Cvoid}, prefix::Ptr{UInt32},
    prefix_count::Csize_t, out_scores::Ptr{Float64}, score_count::Csize_t)::UInt32
    local callback_context
    try
        callback_context = Base.unsafe_pointer_to_objref(context)::LlmScoreContext
        tokens = prefix_count == 0 ? UInt32[] :
            copy(unsafe_wrap(Vector{UInt32}, prefix, Int(prefix_count); own=false))
        values = callback_context.scorer(tokens)
        length(values) == score_count ||
            throw(ArgumentError("model scorer returned the wrong vocabulary length"))
        scores = unsafe_wrap(Vector{Float64}, out_scores, Int(score_count); own=false)
        for index in eachindex(scores)
            value = Float64(values[index])
            (isfinite(value) || value == -Inf) ||
                throw(ArgumentError("model scores must be finite or -Inf"))
            scores[index] = value
        end
        return UInt32(0)
    catch error
        if @isdefined(callback_context)
            callback_context.error = error
        end
        return UInt32(1)
    end
end

const LLM_SCORE_CALLBACK = Ref{Ptr{Cvoid}}(C_NULL)

function initialize_llm_callback!()
    LLM_SCORE_CALLBACK[] = @cfunction(llm_score_callback, UInt32,
        (Ptr{Cvoid}, Ptr{UInt32}, Csize_t, Ptr{Float64}, Csize_t))
    nothing
end

"""Advance one token step with a Julia scorer over copied zero-based prefixes.

The scorer returns one dense log-probability per vocabulary token. Negative
infinity excludes a token. Callback exceptions are rethrown in Julia after
the native call, with the previous beam and statistics still intact.
"""
function llm_step!(session::LlmSession, scorer)
    context = LlmScoreContext(scorer, nothing)
    step = Ref{UInt32}(0)
    status = GC.@preserve context ccall(native(:lling_llm_session_step), UInt32,
        (Ptr{Cvoid}, Ptr{Cvoid}, Ptr{Cvoid}, Ref{UInt32}),
        llm_handle(session), LLM_SCORE_CALLBACK[],
        pointer_from_objref(context), step)
    context.error === nothing || throw(context.error)
    checked(status, :llm_session_step)
    LlmStep(step[])
end

"""Return cumulative committed work and the current beam size."""
function llm_stats(session::LlmSession)
    output = Ref{RawLlmStats}()
    checked(ccall(native(:lling_llm_session_stats), UInt32,
        (Ptr{Cvoid}, Ref{RawLlmStats}), llm_handle(session), output),
        :llm_session_stats)
    value = output[]
    LlmStats(value.steps, value.score_calls, value.considered_tokens,
        value.candidates, Int(value.hypotheses), value.finished != 0)
end

"""Copy the current partial hypotheses in native best-first order."""
function llm_hypotheses(session::LlmSession)
    count = llm_stats(session).hypotheses
    results = Vector{LlmHypothesis}(undef, count)
    for index in 0:count-1
        written = Ref{Csize_t}(0)
        required = Ref{Csize_t}(0)
        score = Ref{Float64}(0)
        accepting = Ref{UInt8}(0)
        checked(ccall(native(:lling_llm_session_hypothesis), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{UInt32}, Csize_t,
                Ref{Csize_t}, Ref{Csize_t}, Ref{Float64}, Ref{UInt8}),
            llm_handle(session), index, C_NULL, 0,
            written, required, score, accepting), :llm_session_hypothesis)
        tokens = Vector{UInt32}(undef, Int(required[]))
        status = GC.@preserve tokens ccall(native(:lling_llm_session_hypothesis), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{UInt32}, Csize_t,
                Ref{Csize_t}, Ref{Csize_t}, Ref{Float64}, Ref{UInt8}),
            llm_handle(session), index,
            isempty(tokens) ? C_NULL : pointer(tokens), length(tokens),
            written, required, score, accepting)
        checked(status, :llm_session_hypothesis)
        written[] == length(tokens) == required[] ||
            throw(ArgumentError("native hypothesis size changed during copy"))
        results[index + 1] = LlmHypothesis(tokens, score[], accepting[] != 0)
    end
    results
end
