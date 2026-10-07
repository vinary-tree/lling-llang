"""A named context-free non-terminal, distinct from a terminal token."""
struct CfgNonterminal
    name::String
    function CfgNonterminal(name::AbstractString)
        isempty(name) && throw(ArgumentError("CFG non-terminal name cannot be empty"))
        new(String(name))
    end
end

"""A named context-free terminal token."""
struct CfgTerminal
    name::String
    function CfgTerminal(name::AbstractString)
        isempty(name) && throw(ArgumentError("CFG terminal name cannot be empty"))
        new(String(name))
    end
end

const CfgSymbol = Union{CfgNonterminal,CfgTerminal}

"""An explicitly typed production; an empty RHS is an epsilon production."""
struct CfgRule
    lhs::CfgNonterminal
    rhs::Vector{CfgSymbol}
    log_probability::Float32
    function CfgRule(lhs::CfgNonterminal, rhs::AbstractVector;
        log_probability::Real=0)
        all(symbol -> symbol isa CfgSymbol, rhs) || throw(ArgumentError(
            "CFG RHS entries must be CfgNonterminal or CfgTerminal"))
        probability = Float32(log_probability)
        isfinite(probability) || throw(ArgumentError("CFG log probability must be finite"))
        new(lhs, CfgSymbol[rhs...], probability)
    end
end

struct CfgSymbolRaw
    kind::UInt32
    value::UInt32
end

struct CfgRuleRaw
    lhs::UInt32
    rhs::Ptr{CfgSymbolRaw}
    rhs_len::Csize_t
    log_prob::Float32
end

"""Compiled native grammar. Close it explicitly when finished."""
mutable struct CfgGrammar
    handle::Ptr{Cvoid}
    terminals::Dict{String,UInt32}
    closed::Bool
end
Base.isopen(grammar::CfgGrammar) = !grammar.closed
function close!(grammar::CfgGrammar)
    grammar.closed && return nothing
    ccall(native(:lling_cfg_grammar_free), Cvoid, (Ptr{Cvoid},), grammar.handle)
    grammar.handle = C_NULL
    grammar.closed = true
    nothing
end
Base.close(grammar::CfgGrammar) = close!(grammar)

function cfg_handle(grammar::CfgGrammar)
    isopen(grammar) || throw(NativeError(STATUS_CLOSED, :cfg_grammar,
        "CFG grammar is closed"))
    grammar.handle
end

"""
Compile typed productions with explicit native rule and RHS storage bounds.

The terminal vocabulary is assigned stable IDs by first appearance in `rules`,
or from a complete `terminal_ids` map to align an existing WFST label domain.
Unknown query tokens use an out-of-vocabulary ID and yield a rejected analysis.
"""
function compile_cfg(start::CfgNonterminal, rules::AbstractVector{CfgRule};
    max_rules::Integer=length(rules),
    max_rhs_symbols::Integer=sum(length(rule.rhs) for rule in rules),
    terminal_ids::AbstractDict=Dict{String,UInt32}())
    max_rules >= 0 || throw(ArgumentError("max_rules must be nonnegative"))
    max_rhs_symbols >= 0 || throw(ArgumentError(
        "max_rhs_symbols must be nonnegative"))
    length(rules) <= max_rules || throw(ArgumentError("CFG rule bound exceeded"))
    sum(length(rule.rhs) for rule in rules) <= max_rhs_symbols || throw(
        ArgumentError("CFG RHS bound exceeded"))

    provided = Dict{String,UInt32}()
    for (name, id) in terminal_ids
        name isa AbstractString && id isa Integer &&
            0 <= id <= typemax(UInt32) || throw(ArgumentError(
            "terminal_ids must map terminal names to UInt32 IDs"))
        provided[String(name)] = UInt32(id)
    end
    length(Set(values(provided))) == length(provided) || throw(
        ArgumentError("terminal_ids must be unique"))
    nonterminals = Dict{String,UInt32}()
    terminals = Dict{String,UInt32}()
    function nonterminal_id(name::String)
        get!(nonterminals, name) do
            length(nonterminals) <= typemax(UInt16) || throw(ArgumentError(
                "CFG non-terminal count exceeds native domain"))
            UInt32(length(nonterminals))
        end
    end
    function terminal_id(name::String)
        get!(terminals, name) do
            length(terminals) < typemax(UInt32) || throw(ArgumentError(
                "CFG terminal count exceeds native domain"))
            if isempty(provided)
                UInt32(length(terminals))
            else
                get(provided, name) do
                    throw(ArgumentError(
                        "terminal_ids must include every CFG terminal"))
                end
            end
        end
    end
    start_id = nonterminal_id(start.name)
    rhs_buffers = Vector{Vector{CfgSymbolRaw}}(undef, length(rules))
    raw_rules = Vector{CfgRuleRaw}(undef, length(rules))
    for (index, rule) in enumerate(rules)
        lhs = nonterminal_id(rule.lhs.name)
        rhs = CfgSymbolRaw[]
        for symbol in rule.rhs
            if symbol isa CfgNonterminal
                push!(rhs, CfgSymbolRaw(CFG_NONTERMINAL,
                    nonterminal_id(symbol.name)))
            else
                push!(rhs, CfgSymbolRaw(CFG_TERMINAL,
                    terminal_id(symbol.name)))
            end
        end
        rhs_buffers[index] = rhs
        raw_rules[index] = CfgRuleRaw(lhs,
            isempty(rhs) ? Ptr{CfgSymbolRaw}(C_NULL) : pointer(rhs),
            Csize_t(length(rhs)), rule.log_probability)
    end

    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve rhs_buffers raw_rules begin
        checked(ccall(native(:lling_cfg_grammar_compile), UInt32,
            (UInt32, UInt32, Ptr{CfgRuleRaw}, Csize_t, Csize_t, Csize_t,
                Ref{Ptr{Cvoid}}),
            start_id, UInt32(length(nonterminals)), raw_rules,
            Csize_t(length(raw_rules)), Csize_t(max_rules),
            Csize_t(max_rhs_symbols), output), :cfg_grammar_compile)
    end
    grammar = CfgGrammar(output[], terminals, false)
    finalizer(finalize_close, grammar)
    grammar
end

"""Return the stable numeric terminal ID used in token and WFST lattices."""
function cfg_terminal_id(grammar::CfgGrammar, terminal::CfgTerminal)
    cfg_handle(grammar)
    get(grammar.terminals, terminal.name) do
        throw(ArgumentError("terminal is absent from compiled CFG"))
    end
end

"""Hard resource limits for one native CFG parse."""
struct CfgLimits
    max_tokens::UInt64
    max_chart_items::UInt64
    max_forest_nodes::UInt64
    max_work::UInt64
    function CfgLimits(; max_tokens::Integer=10_000,
        max_chart_items::Integer=100_000,
        max_forest_nodes::Integer=100_000,
        max_work::Integer=1_000_000)
        values = (max_tokens, max_chart_items, max_forest_nodes, max_work)
        all(value -> 0 <= value <= typemax(UInt32), values[1:3]) || throw(
            ArgumentError("CFG token, chart, and forest limits must fit UInt32"))
        0 <= max_work <= typemax(UInt64) || throw(ArgumentError(
            "CFG work limit must fit UInt64"))
        new(UInt64(max_tokens), UInt64(max_chart_items),
            UInt64(max_forest_nodes), UInt64(max_work))
    end
end

struct CfgLimitsRaw
    struct_size::UInt32
    version::UInt32
    max_tokens::UInt64
    max_chart_items::UInt64
    max_forest_nodes::UInt64
    max_work::UInt64
end

"""Hard graph-import and parser bounds for a CFG analysis of a WFST."""
struct CfgWfstLimits
    max_states::UInt64
    max_arcs::UInt64
    max_bytes::UInt64
    max_import_work::UInt64
    max_chart_items::UInt64
    max_forest_nodes::UInt64
    max_parse_work::UInt64
    function CfgWfstLimits(; max_states::Integer=10_000,
        max_arcs::Integer=100_000, max_bytes::Integer=16_000_000,
        max_import_work::Integer=1_000_000,
        max_chart_items::Integer=100_000,
        max_forest_nodes::Integer=100_000,
        max_parse_work::Integer=1_000_000)
        values = (max_states, max_arcs, max_bytes, max_import_work,
            max_chart_items, max_forest_nodes, max_parse_work)
        all(value -> 0 <= value <= typemax(UInt64), values) || throw(
            ArgumentError("CFG WFST limits must be nonnegative UInt64 values"))
        all(value -> value <= typemax(UInt32),
            (max_states, max_arcs, max_chart_items, max_forest_nodes)) || throw(
            ArgumentError("CFG WFST state, arc, chart, and forest limits must fit UInt32"))
        new(UInt64.(values)...)
    end
end

struct CfgWfstLimitsRaw
    struct_size::UInt32
    version::UInt32
    tape::UInt32
    reserved::UInt32
    max_states::UInt64
    max_arcs::UInt64
    max_bytes::UInt64
    max_import_work::UInt64
    max_chart_items::UInt64
    max_forest_nodes::UInt64
    max_parse_work::UInt64
end

struct CfgInfoRaw
    accepted::UInt8
    reserved::NTuple{7,UInt8}
    chart_items::UInt64
    forest_nodes::UInt64
    roots::UInt64
    edges::UInt64
    work::UInt64
end

struct CfgChartItemRaw
    position::UInt32
    rule::UInt32
    dot::UInt32
    start::UInt32
end

"""One Earley chart item; positions and rule IDs use native zero-based IDs."""
struct CfgChartItem
    position::UInt32
    rule::UInt32
    dot::UInt32
    start::UInt32
end
CfgChartItem(raw::CfgChartItemRaw) = CfgChartItem(raw.position,
    raw.rule, raw.dot, raw.start)

struct CfgForestNodeRaw
    rule::UInt32
    start::UInt32
    stop::UInt32
    is_root::UInt8
    reserved::NTuple{3,UInt8}
    children::UInt64
end

"""One packed forest node with a stable zero-based native ID."""
struct CfgForestNode
    id::UInt32
    rule::UInt32
    start::UInt32
    stop::UInt32
    is_root::Bool
    child_count::Int
end

struct CfgForestChildRaw
    kind::UInt32
    edge_id::UInt32
    members::UInt64
end

"""A terminal edge in a parse forest node."""
struct CfgTerminalChild
    edge_id::UInt32
    token::UInt32
end

"""One packed derivation of child forest nodes."""
struct CfgDerivationChild
    members::Vector{UInt32}
end

"""Owned native chart and forest; the grammar can close after parsing."""
mutable struct CfgAnalysis
    handle::Ptr{Cvoid}
    edge_labels::Vector{UInt32}
    closed::Bool
end
Base.isopen(analysis::CfgAnalysis) = !analysis.closed
function close!(analysis::CfgAnalysis)
    analysis.closed && return nothing
    ccall(native(:lling_cfg_analysis_free), Cvoid, (Ptr{Cvoid},), analysis.handle)
    analysis.handle = C_NULL
    analysis.closed = true
    nothing
end
Base.close(analysis::CfgAnalysis) = close!(analysis)

function cfg_handle(analysis::CfgAnalysis)
    isopen(analysis) || throw(NativeError(STATUS_CLOSED, :cfg_analysis,
        "CFG analysis is closed"))
    analysis.handle
end

"""Parse a vector of vocabulary IDs, returning acceptance and a bounded chart/forest."""
function parse_cfg(grammar::CfgGrammar, tokens::Vector{UInt32};
    limits::CfgLimits=CfgLimits())
    raw_limits = Ref(CfgLimitsRaw(UInt32(sizeof(CfgLimitsRaw)), 1,
        limits.max_tokens, limits.max_chart_items,
        limits.max_forest_nodes, limits.max_work))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve tokens begin
        checked(ccall(native(:lling_cfg_parse_tokens), UInt32,
            (Ptr{Cvoid}, Ptr{UInt32}, Csize_t, Ref{CfgLimitsRaw},
                Ref{Ptr{Cvoid}}),
            cfg_handle(grammar), tokens, Csize_t(length(tokens)),
            raw_limits, output), :cfg_parse_tokens)
    end
    analysis = CfgAnalysis(output[], copy(tokens), false)
    finalizer(finalize_close, analysis)
    analysis
end

function parse_cfg(grammar::CfgGrammar, tokens::AbstractVector{<:Integer};
    limits::CfgLimits=CfgLimits())
    all(token -> 0 <= token <= typemax(UInt32), tokens) || throw(
        ArgumentError("CFG token IDs must fit UInt32"))
    parse_cfg(grammar, UInt32[tokens...]; limits)
end

"""Parse named tokens using the grammar's stable terminal vocabulary."""
function parse_cfg(grammar::CfgGrammar, tokens::AbstractVector{<:AbstractString};
    limits::CfgLimits=CfgLimits())
    ids = UInt32[get(grammar.terminals, String(token), typemax(UInt32))
        for token in tokens]
    parse_cfg(grammar, ids; limits)
end

"""
Analyze an acyclic scalar WFST against a CFG using one bounded native forest.

Selected tape labels must be non-epsilon UInt32 terminal IDs from
`cfg_terminal_id`. Every reachable final state participates in the analysis.
"""
function parse_cfg(grammar::CfgGrammar, wfst::Union{Wfst,VTI.Wfst};
    tape::Symbol=:input, limits::CfgWfstLimits=CfgWfstLimits())
    tape_id = tape === :input ? UInt32(1) :
        tape === :output ? UInt32(2) : throw(ArgumentError(
            "CFG WFST tape must be :input or :output"))
    raw_limits = Ref(CfgWfstLimitsRaw(UInt32(sizeof(CfgWfstLimitsRaw)),
        1, tape_id, 0, limits.max_states, limits.max_arcs,
        limits.max_bytes, limits.max_import_work,
        limits.max_chart_items, limits.max_forest_nodes,
        limits.max_parse_work))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    raw = Ref(raw_resource(wfst))
    GC.@preserve wfst raw begin
        checked(ccall(native(:lling_cfg_parse_wfst_resource), UInt32,
            (Ptr{Cvoid}, Ref{VTI.VtResourceRaw}, Ref{CfgWfstLimitsRaw},
                Ref{Ptr{Cvoid}}),
            cfg_handle(grammar), raw, raw_limits, output),
            :cfg_parse_wfst_resource)
    end
    analysis = CfgAnalysis(output[], UInt32[], false)
    finalizer(finalize_close, analysis)
    analysis.edge_labels = cfg_edge_labels(analysis)
    analysis
end

"""Return acceptance, retained sizes, and charged parser work."""
function cfg_info(analysis::CfgAnalysis)
    output = Ref{CfgInfoRaw}()
    checked(ccall(native(:lling_cfg_analysis_info), UInt32,
        (Ptr{Cvoid}, Ref{CfgInfoRaw}), cfg_handle(analysis), output), :cfg_analysis_info)
    raw = output[]
    (accepted=raw.accepted != 0, chart_items=Int(raw.chart_items),
        forest_nodes=Int(raw.forest_nodes), roots=Int(raw.roots),
        edges=Int(raw.edges),
        work=raw.work)
end

struct CfgChartIterator
    analysis::CfgAnalysis
    batch_size::Int
end
Base.IteratorSize(::Type{CfgChartIterator}) = Base.HasLength()
Base.eltype(::Type{CfgChartIterator}) = CfgChartItem
Base.length(iterator::CfgChartIterator) = cfg_info(iterator.analysis).chart_items

function Base.iterate(iterator::CfgChartIterator,
    state::Tuple{Int,Vector{CfgChartItemRaw},Int}=(0, CfgChartItemRaw[], 1))
    offset, batch, index = state
    if index > length(batch)
        buffer = Vector{CfgChartItemRaw}(undef, iterator.batch_size)
        written = Ref{Csize_t}(0)
        checked(ccall(native(:lling_cfg_analysis_chart_page), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{CfgChartItemRaw}, Csize_t,
                Ref{Csize_t}), cfg_handle(iterator.analysis), Csize_t(offset),
            buffer, Csize_t(length(buffer)), written), :cfg_analysis_chart_page)
        resize!(buffer, Int(written[]))
        isempty(buffer) && return nothing
        batch = buffer
        index = 1
        offset += length(batch)
    end
    CfgChartItem(batch[index]), (offset, batch, index + 1)
end

"""Iterate the complete chart in bounded native pages and stable order."""
function cfg_chart(analysis::CfgAnalysis; batch_size::Integer=256)
    batch_size > 0 || throw(ArgumentError("batch_size must be positive"))
    CfgChartIterator(analysis, Int(batch_size))
end

"""Return sorted complete-parse root IDs."""
function cfg_roots(analysis::CfgAnalysis)
    count = cfg_info(analysis).roots
    result = Vector{UInt32}(undef, count)
    written = Ref{Csize_t}(0)
    checked(ccall(native(:lling_cfg_analysis_root_page), UInt32,
        (Ptr{Cvoid}, Csize_t, Ptr{UInt32}, Csize_t, Ref{Csize_t}),
        cfg_handle(analysis), 0, result, Csize_t(count), written),
        :cfg_analysis_root_page)
    Int(written[]) == count || error("native CFG root page was incomplete")
    result
end

"""Return token IDs for native lattice edges in stable edge order."""
function cfg_edge_labels(analysis::CfgAnalysis)
    count = cfg_info(analysis).edges
    result = Vector{UInt32}(undef, count)
    written = Ref{Csize_t}(0)
    checked(ccall(native(:lling_cfg_analysis_edge_page), UInt32,
        (Ptr{Cvoid}, Csize_t, Ptr{UInt32}, Csize_t, Ref{Csize_t}),
        cfg_handle(analysis), 0, result, Csize_t(count), written),
        :cfg_analysis_edge_page)
    Int(written[]) == count || error("native CFG edge page was incomplete")
    result
end

"""Read one packed forest node by its zero-based native ID."""
function cfg_forest_node(analysis::CfgAnalysis, node_id::Integer)
    0 <= node_id <= typemax(UInt32) || throw(ArgumentError(
        "forest node ID is outside UInt32"))
    output = Ref{CfgForestNodeRaw}()
    checked(ccall(native(:lling_cfg_analysis_forest_node), UInt32,
        (Ptr{Cvoid}, UInt32, Ref{CfgForestNodeRaw}),
        cfg_handle(analysis), UInt32(node_id), output),
        :cfg_analysis_forest_node)
    raw = output[]
    CfgForestNode(UInt32(node_id), raw.rule, raw.start, raw.stop,
        raw.is_root != 0, Int(raw.children))
end

"""Read the exact packed terminal and derivation entries of one forest node."""
function cfg_forest_children(analysis::CfgAnalysis, node_id::Integer)
    node = cfg_forest_node(analysis, node_id)
    result = Union{CfgTerminalChild,CfgDerivationChild}[]
    sizehint!(result, node.child_count)
    for child_index in 0:(node.child_count - 1)
        output = Ref{CfgForestChildRaw}()
        checked(ccall(native(:lling_cfg_analysis_forest_child), UInt32,
            (Ptr{Cvoid}, UInt32, Csize_t, Ref{CfgForestChildRaw}),
            cfg_handle(analysis), node.id, Csize_t(child_index), output),
            :cfg_analysis_forest_child)
        raw = output[]
        if raw.kind == CFG_CHILD_TERMINAL
            edge_index = Int(raw.edge_id) + 1
            edge_index <= length(analysis.edge_labels) || error(
                "native CFG terminal edge exceeds captured tokens")
            push!(result, CfgTerminalChild(raw.edge_id,
                analysis.edge_labels[edge_index]))
        elseif raw.kind == CFG_CHILD_DERIVATION
            members = Vector{UInt32}(undef, Int(raw.members))
            for member_index in eachindex(members)
                member = Ref{UInt32}(0)
                checked(ccall(native(:lling_cfg_analysis_derivation_member), UInt32,
                    (Ptr{Cvoid}, UInt32, Csize_t, Csize_t, Ref{UInt32}),
                    cfg_handle(analysis), node.id, Csize_t(child_index),
                    Csize_t(member_index - 1), member),
                    :cfg_analysis_derivation_member)
                members[member_index] = member[]
            end
            push!(result, CfgDerivationChild(members))
        else
            error("native CFG child kind is invalid")
        end
    end
    result
end
