"""Policy for a retained native Unicode dictionary correction layer."""
struct CorrectionConfig
    max_distance::Int
    max_corrections_per_word::Int
    min_word_length::Int
    cost_per_edit::Float64
    substitution_multiplier::Float64
    transposition_multiplier::Float64
    exact_match_boost::Float64
    enable_transpositions::Bool
    case_insensitive::Bool
    keep_original::Bool
    normalization::Symbol
end

function CorrectionConfig(; max_distance::Integer=2,
    max_corrections_per_word::Integer=10, min_word_length::Integer=2,
    cost_per_edit::Real=1.0, substitution_multiplier::Real=1.0,
    transposition_multiplier::Real=1.0, exact_match_boost::Real=0.0,
    enable_transpositions::Bool=true, case_insensitive::Bool=false,
    keep_original::Bool=true, normalization::Symbol=:exact)
    for (name, value) in ((:max_distance, max_distance),
        (:max_corrections_per_word, max_corrections_per_word),
        (:min_word_length, min_word_length))
        0 <= value <= 1_048_576 ||
            throw(ArgumentError("$name must be in 0:1048576"))
    end
    for (name, value) in ((:cost_per_edit, cost_per_edit),
        (:substitution_multiplier, substitution_multiplier),
        (:transposition_multiplier, transposition_multiplier))
        isfinite(value) && value >= 0 ||
            throw(ArgumentError("$name must be finite and nonnegative"))
    end
    isfinite(exact_match_boost) ||
        throw(ArgumentError("exact_match_boost must be finite"))
    normalization in (:exact, :unicode_lowercase_keys) ||
        throw(ArgumentError("normalization must be :exact or :unicode_lowercase_keys"))
    case_insensitive && normalization != :unicode_lowercase_keys &&
        throw(ArgumentError("case-insensitive queries require lowercase stored keys"))
    CorrectionConfig(Int(max_distance), Int(max_corrections_per_word),
        Int(min_word_length), Float64(cost_per_edit),
        Float64(substitution_multiplier), Float64(transposition_multiplier),
        Float64(exact_match_boost), enable_transpositions, case_insensitive,
        keep_original, normalization)
end

struct RawCorrectionConfig
    version::UInt32
    reserved::UInt32
    max_distance::Csize_t
    max_corrections_per_word::Csize_t
    min_word_length::Csize_t
    cost_per_edit::Float64
    substitution_multiplier::Float64
    transposition_multiplier::Float64
    exact_match_boost::Float64
    enable_transpositions::UInt8
    case_insensitive::UInt8
    keep_original::UInt8
    normalization::UInt8
    reserved_flags::NTuple{4,UInt8}
end

RawCorrectionConfig(config::CorrectionConfig) = RawCorrectionConfig(
    1, 0, config.max_distance, config.max_corrections_per_word,
    config.min_word_length, config.cost_per_edit,
    config.substitution_multiplier, config.transposition_multiplier,
    config.exact_match_boost, UInt8(config.enable_transpositions),
    UInt8(config.case_insensitive), UInt8(config.keep_original),
    UInt8(config.normalization == :unicode_lowercase_keys),
    (0x00, 0x00, 0x00, 0x00))

"""One owned UTF-8 correction word and its native edit cost."""
struct CorrectionCandidate
    word::String
    cost::Float64
end

"""Native correction layer pinned to one immutable dictionary revision."""
mutable struct CorrectionDictionary
    handle::Ptr{Cvoid}
    closed::Bool
end

function correction_handle(dictionary::CorrectionDictionary)
    dictionary.closed && throw(NativeError(STATUS_CLOSED, :correction_dictionary,
        "correction dictionary is closed"))
    dictionary.handle
end

"""Retain a Unicode-scalar dictionary resource for native correction.

Pass a `VinaryTreeInterop.Dictionary` or resource, including a
`Libdictenstein.snapshot` result. The native layer pins that revision; later
changes to the producer do not change these queries. `normalization` declares
the producer's stored-key contract and is checked before a query is run.
"""
function correction_dictionary(source::Union{VTI.Dictionary,VTI.Resource};
    config::CorrectionConfig=CorrectionConfig())
    raw = Ref(raw_resource(source))
    policy = Ref(RawCorrectionConfig(config))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    status = GC.@preserve source raw policy ccall(
        native(:lling_correction_dictionary_open), UInt32,
        (Ref{VTI.VtResourceRaw}, Ref{RawCorrectionConfig}, Ref{Ptr{Cvoid}}),
        raw, policy, output)
    checked(status, :correction_dictionary_open)
    dictionary = CorrectionDictionary(output[], false)
    finalizer(finalize_close, dictionary)
    dictionary
end

function close!(dictionary::CorrectionDictionary)
    dictionary.closed && return nothing
    ccall(native(:lling_correction_dictionary_free), Cvoid,
        (Ptr{Cvoid},), dictionary.handle)
    dictionary.handle = C_NULL
    dictionary.closed = true
    nothing
end
Base.close(dictionary::CorrectionDictionary) = close!(dictionary)
Base.isopen(dictionary::CorrectionDictionary) = !dictionary.closed

"""Return bounded, distance-ordered corrections from the native Rust layer."""
function corrections(dictionary::CorrectionDictionary, word::AbstractString)
    bytes = Vector{UInt8}(codeunits(String(word)))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    status = GC.@preserve bytes ccall(
        native(:lling_correction_dictionary_query), UInt32,
        (Ptr{Cvoid}, Ptr{UInt8}, Csize_t, Ref{Ptr{Cvoid}}),
        correction_handle(dictionary),
        isempty(bytes) ? C_NULL : pointer(bytes), length(bytes), output)
    checked(status, :correction_dictionary_query)
    copied_correction_result(output[])
end

function copied_correction_result(result::Ptr{Cvoid})
    try
        count = Ref{Csize_t}(0)
        checked(ccall(native(:lling_correction_result_count), UInt32,
            (Ptr{Cvoid}, Ref{Csize_t}), result, count),
            :correction_result_count)
        results = Vector{CorrectionCandidate}(undef, Int(count[]))
        for index in eachindex(results)
            written = Ref{Csize_t}(0)
            required = Ref{Csize_t}(0)
            cost = Ref{Float64}(0)
            checked(ccall(native(:lling_correction_result_item), UInt32,
                (Ptr{Cvoid}, Csize_t, Ptr{UInt8}, Csize_t,
                    Ref{Csize_t}, Ref{Csize_t}, Ref{Float64}),
                result, index - 1, C_NULL, 0, written, required, cost),
                :correction_result_item)
            buffer = Vector{UInt8}(undef, Int(required[]))
            status = GC.@preserve buffer ccall(
                native(:lling_correction_result_item), UInt32,
                (Ptr{Cvoid}, Csize_t, Ptr{UInt8}, Csize_t,
                    Ref{Csize_t}, Ref{Csize_t}, Ref{Float64}),
                result, index - 1,
                isempty(buffer) ? C_NULL : pointer(buffer), length(buffer),
                written, required, cost)
            checked(status, :correction_result_item)
            written[] == length(buffer) && required[] == length(buffer) ||
                throw(ArgumentError("native correction size changed during copy"))
            results[index] = CorrectionCandidate(String(buffer), cost[])
        end
        results
    finally
        ccall(native(:lling_correction_result_free), Cvoid,
            (Ptr{Cvoid},), result)
    end
end

"""Configuration for a retained native character-confusion layer."""
struct ConfusionConfig
    matrix::Symbol
    max_corrections::Int
    threshold::Float64
    function ConfusionConfig(matrix::Symbol, max_corrections::Integer,
        threshold::Real)
        matrix in (:qwerty, :dvorak, :ocr, :mobile, :custom) ||
            throw(ArgumentError("unknown confusion matrix"))
        0 <= max_corrections <= 4096 ||
            throw(ArgumentError("max_corrections must be in 0:4096"))
        value = Float64(threshold)
        isfinite(value) && 0 < value <= 1 ||
            throw(ArgumentError("threshold must be finite and in (0, 1]"))
        new(matrix, Int(max_corrections), value)
    end
end

function ConfusionConfig(; matrix::Symbol=:qwerty,
    max_corrections::Integer=10, threshold::Real=0.01)
    ConfusionConfig(matrix, max_corrections, threshold)
end

struct RawConfusionConfig
    version::UInt32
    matrix_kind::UInt32
    max_corrections::Csize_t
    threshold::Float64
end
RawConfusionConfig(config::ConfusionConfig) = RawConfusionConfig(1,
    UInt32(findfirst(==(config.matrix),
        (:qwerty, :dvorak, :ocr, :mobile, :custom)) - 1),
    config.max_corrections, config.threshold)

"""One substitution, insertion, or deletion probability in a custom matrix.

For substitution, provide both scalars; for insertion, only `observed`; for
deletion, only `intended`. `probability` is the observed-error probability.
"""
struct ConfusionRule
    operation::Symbol
    observed::Union{Nothing,Char}
    intended::Union{Nothing,Char}
    probability::Float64
    function ConfusionRule(operation::Symbol,
        observed::Union{Nothing,Char}, intended::Union{Nothing,Char},
        probability::Real)
        valid = (operation == :substitution && observed !== nothing && intended !== nothing) ||
            (operation == :insertion && observed !== nothing && intended === nothing) ||
            (operation == :deletion && observed === nothing && intended !== nothing)
        valid || throw(ArgumentError("confusion rule has an invalid operation or scalar"))
        value = Float64(probability)
        isfinite(value) && 0 < value <= 1 ||
            throw(ArgumentError("confusion probability must be finite and in (0, 1]"))
        new(operation, observed, intended, value)
    end
end

struct RawConfusionRule
    observed::UInt32
    intended::UInt32
    operation::UInt32
    reserved::UInt32
    probability::Float64
end
RawConfusionRule(rule::ConfusionRule) = RawConfusionRule(
    isnothing(rule.observed) ? 0 : UInt32(rule.observed),
    isnothing(rule.intended) ? 0 : UInt32(rule.intended),
    UInt32(findfirst(==(rule.operation),
        (:substitution, :insertion, :deletion)) - 1),
    0, rule.probability)

"""Retained native confusion layer. Queries return owned correction costs."""
mutable struct ConfusionLayer
    handle::Ptr{Cvoid}
    closed::Bool
end

function correction_handle(layer::ConfusionLayer)
    layer.closed && throw(NativeError(STATUS_CLOSED, :confusion_layer,
        "confusion layer is closed"))
    layer.handle
end

"""Create a native confusion layer with a built-in or custom matrix.

Custom rules are copied during construction. Built-in matrices accept no
rules. Calls are exclusive to one layer owner; results outlive the layer.
"""
function confusion_layer(; config::ConfusionConfig=ConfusionConfig(),
    rules::AbstractVector{ConfusionRule}=ConfusionRule[])
    (config.matrix == :custom) == !isempty(rules) ||
        throw(ArgumentError("custom matrices require rules; built-ins take none"))
    length(rules) <= 65_536 ||
        throw(ArgumentError("custom matrix exceeds 65536 rules"))
    raw_config = Ref(RawConfusionConfig(config))
    raw_rules = RawConfusionRule.(rules)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    status = GC.@preserve raw_config raw_rules ccall(
        native(:lling_confusion_layer_open), UInt32,
        (Ref{RawConfusionConfig}, Ptr{RawConfusionRule}, Csize_t,
            Ref{Ptr{Cvoid}}),
        raw_config, isempty(raw_rules) ? C_NULL : pointer(raw_rules),
        length(raw_rules), output)
    checked(status, :confusion_layer_open)
    layer = ConfusionLayer(output[], false)
    finalizer(finalize_close, layer)
    layer
end

function close!(layer::ConfusionLayer)
    layer.closed && return nothing
    ccall(native(:lling_confusion_layer_free), Cvoid,
        (Ptr{Cvoid},), layer.handle)
    layer.handle = C_NULL
    layer.closed = true
    nothing
end
Base.close(layer::ConfusionLayer) = close!(layer)
Base.isopen(layer::ConfusionLayer) = !layer.closed

"""Return native confusion candidates ordered by decreasing probability."""
function corrections(layer::ConfusionLayer, word::AbstractString)
    bytes = Vector{UInt8}(codeunits(String(word)))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    status = GC.@preserve layer bytes ccall(
        native(:lling_confusion_layer_query), UInt32,
        (Ptr{Cvoid}, Ptr{UInt8}, Csize_t, Ref{Ptr{Cvoid}}),
        correction_handle(layer), isempty(bytes) ? C_NULL : pointer(bytes),
        length(bytes), output)
    checked(status, :confusion_layer_query)
    copied_correction_result(output[])
end

const CorrectionSource = Union{CorrectionDictionary,ConfusionLayer}

"""A bounded sequence of retained native word-correction layers.

The pipeline borrows its layers; the caller closes them after the pipeline is
no longer used. Intermediate candidates retain the lowest accumulated cost
for each word. Exceeding either explicit work bound raises an error.
"""
struct CorrectionPipeline{L<:Tuple}
    layers::L
    max_candidates::Int
    max_queries::Int
    keep_original::Bool
end

function correction_pipeline(layers::CorrectionSource...;
    max_candidates::Integer=1024, max_queries::Integer=4096,
    keep_original::Bool=true)
    isempty(layers) && throw(ArgumentError("correction pipeline needs a layer"))
    0 < max_candidates <= 4096 ||
        throw(ArgumentError("max_candidates must be in 1:4096"))
    0 < max_queries <= 1_048_576 ||
        throw(ArgumentError("max_queries must be in 1:1048576"))
    CorrectionPipeline(layers, Int(max_candidates),
        Int(max_queries), keep_original)
end

function correction_handle(pipeline::CorrectionPipeline)
    foreach(correction_handle, pipeline.layers)
    nothing
end

Base.isopen(pipeline::CorrectionPipeline) = all(isopen, pipeline.layers)

"""Compose native candidate generations in layer order with additive costs."""
function corrections(pipeline::CorrectionPipeline, word::AbstractString)
    correction_handle(pipeline)
    frontier = Dict(String(word) => 0.0)
    queries = 0
    for layer in pipeline.layers
        next = Dict{String,Float64}()
        for (candidate, base_cost) in sort!(collect(frontier); by=first)
            if pipeline.keep_original
                next[candidate] = min(get(next, candidate, Inf), base_cost)
                length(next) <= pipeline.max_candidates ||
                    throw(ArgumentError("correction pipeline candidate limit exceeded"))
            end
            queries += 1
            queries <= pipeline.max_queries ||
                throw(ArgumentError("correction pipeline query limit exceeded"))
            for output in corrections(layer, candidate)
                total = base_cost + output.cost
                isfinite(total) ||
                    throw(ArgumentError("correction pipeline cost overflowed"))
                next[output.word] = min(get(next, output.word, Inf), total)
                length(next) <= pipeline.max_candidates ||
                    throw(ArgumentError("correction pipeline candidate limit exceeded"))
            end
        end
        frontier = next
        isempty(frontier) && break
    end
    ordered = [CorrectionCandidate(candidate, cost)
        for (candidate, cost) in frontier]
    sort!(ordered; by=value -> (value.cost, value.word))
end

"""One native decoder over bounded correction candidates and their costs.

The vocabulary is copied so token IDs and candidate words remain stable for
the lifetime of the session. `llm_hypotheses` returns zero-based token IDs;
index `words` with `Int(token) + 1` to recover the chosen word.
"""
mutable struct CorrectionLlmSession
    decoder::LlmSession
    words::Vector{String}
    candidate_costs::Vector{Pair{Int,Float64}}
    edit_weight::Float64
    model_weight::Float64
    temperature::Float64
end

"""Compose a retained correction layer with the native constrained decoder.

`words` is the model's ordered vocabulary. Only native corrections found
in that vocabulary become accepting one-token paths. The WFST is imported by
the decoder, then released. The returned session owns its constraint and can
outlive the dictionary or confusion layer. `edit_weight` multiplies correction
cost; `model_weight`
multiplies the scorer's log probability. Both must be finite and nonnegative.
"""
function correction_llm_session(source::Union{CorrectionSource,CorrectionPipeline},
    query::AbstractString, words::AbstractVector{<:AbstractString};
    config::LlmConfig=LlmConfig(), edit_weight::Real=1.0,
    model_weight::Real=1.0,
    budget::BudgetV2=BudgetV2(max_states=65_536, max_arcs=1_048_576,
        max_bytes=128 * 1024 * 1024, max_work=8_388_608))
    correction_handle(source)
    0 < length(words) <= config.max_vocabulary ||
        throw(ArgumentError("vocabulary size exceeds the configured range"))
    edit_scale = Float64(edit_weight)
    model_scale = Float64(model_weight)
    isfinite(edit_scale) && edit_scale >= 0 ||
        throw(ArgumentError("edit_weight must be finite and nonnegative"))
    isfinite(model_scale) && model_scale >= 0 ||
        throw(ArgumentError("model_weight must be finite and nonnegative"))
    vocabulary = String.(words)
    length(Set(vocabulary)) == length(vocabulary) ||
        throw(ArgumentError("vocabulary words must be unique"))
    by_word = Dict(word => index - 1 for (index, word) in enumerate(vocabulary))
    candidates = corrections(source, query)
    selected = Pair{Int,Float64}[]
    for candidate in candidates
        index = get(by_word, candidate.word, nothing)
        isnothing(index) || push!(selected, index => candidate.cost)
    end
    isempty(selected) && throw(ArgumentError(
        "correction layer returned no candidate in the model vocabulary"))

    builder = WfstBuilder{UInt64,TropicalWeight}(size_hint=2)
    graph = nothing
    try
        root = add_state!(builder)
        terminal = add_state!(builder)
        set_start!(builder, root)
        set_final!(builder, terminal, TropicalWeight(0))
        for (index, _) in selected
            add_arc!(builder, root, UInt64(index), UInt64(index), terminal,
                TropicalWeight(0))
        end
        graph = build!(builder)
        decoder = llm_session(graph; vocab_size=length(vocabulary), config, budget)
        CorrectionLlmSession(decoder, vocabulary, selected,
            edit_scale, model_scale, config.temperature)
    finally
        close(builder)
        isnothing(graph) || close(graph)
    end
end

"""Advance one correction step using the language model's dense log scores.

The native decoder supplies the prefix to `scorer`. For each legal candidate,
the combined score is `model_weight * log_probability / temperature -
edit_weight * correction_cost`. Callback errors preserve the last committed beam.
"""
function llm_step!(session::CorrectionLlmSession, scorer)
    llm_step!(session.decoder, prefix -> begin
        raw = scorer(prefix)
        length(raw) == length(session.words) ||
            throw(ArgumentError("model scorer returned the wrong vocabulary length"))
        scores = collect(Float64, raw)
        for (index, cost) in session.candidate_costs
            value = scores[index + 1]
            (isfinite(value) || value == -Inf) ||
                throw(ArgumentError("model scores must be finite or -Inf"))
            value == -Inf && continue
            combined = session.model_weight * value -
                session.edit_weight * cost * session.temperature
            isfinite(combined) ||
                throw(ArgumentError("combined correction score is not finite"))
            scores[index + 1] = combined
        end
        scores
    end)
end

llm_hypotheses(session::CorrectionLlmSession) = llm_hypotheses(session.decoder)
llm_stats(session::CorrectionLlmSession) = llm_stats(session.decoder)
cancel!(session::CorrectionLlmSession) = (cancel!(session.decoder); session)
close!(session::CorrectionLlmSession) = close!(session.decoder)
Base.close(session::CorrectionLlmSession) = close!(session)
Base.isopen(session::CorrectionLlmSession) = isopen(session.decoder)
