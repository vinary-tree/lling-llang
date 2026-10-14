"""One tree node with children in their natural Julia order."""
struct Tree{L}
    label::L
    children::Vector{Tree{L}}
end

Tree(label::L) where {L} = Tree{L}(label, Tree{L}[])
Tree(label::L, children::AbstractVector{<:Tree{L}}) where {L} =
    Tree{L}(label, Tree{L}[children...])

"""A pattern variable: native state ID and Julia one-based input child."""
struct TreeVariable
    state::UInt64
    input_child::UInt64
    function TreeVariable(state::Integer, input_child::Integer)
        0 <= state < typemax(UInt32) || throw(ArgumentError(
            "tree variable state must fit a native state ID"))
        1 <= input_child <= typemax(UInt64) || throw(ArgumentError(
            "tree variable child index must be positive"))
        new(UInt64(state), UInt64(input_child))
    end
end

"""A right-hand-side output tree containing fixed subtrees or variables."""
struct TreePattern{L}
    label::L
    children::Vector{Union{TreePattern{L},TreeVariable}}
end

TreePattern(label::L) where {L} =
    TreePattern{L}(label, Union{TreePattern{L},TreeVariable}[])
function TreePattern(label::L, children::AbstractVector) where {L}
    typed = Union{TreePattern{L},TreeVariable}[]
    for child in children
        child isa TreePattern{L} || child isa TreeVariable || throw(ArgumentError(
            "a tree pattern child must be a matching pattern or TreeVariable"))
        push!(typed, child)
    end
    TreePattern{L}(label, typed)
end

"""One immutable native rule snapshot with a decoded output pattern."""
struct TreeRule{L,W<:AbstractScalarWeight}
    state::UInt64
    input_symbol::L
    input_arity::Int
    output_pattern::TreePattern{L}
    weight::W
end

struct AbiTreeNode
    kind::UInt64
    symbol::UInt64
    arity::UInt64
    state::UInt64
    variable_index::UInt64
end

struct AbiTreeRule
    state::UInt64
    input_symbol::UInt64
    input_arity::UInt64
    pattern_offset::UInt64
    pattern_len::UInt64
    weight::Float64
end

const TREE_SYMBOL_NODE = UInt64(0)
const TREE_VARIABLE_NODE = UInt64(1)
const DEFAULT_TREE_BUDGET = BudgetV2(max_states=10_000,
    max_arcs=100_000, max_bytes=64_000_000, max_work=1_000_000)

function finite_tree_budget(budget::BudgetV2)
    required = BUDGET_STATES | BUDGET_ARCS | BUDGET_BYTES | BUDGET_WORK
    (budget.header.flags & required) == required || throw(ArgumentError(
        "tree transducers require four finite budget axes"))
    budget
end

function tree_label(::Type{L}, value, symbols) where {L}
    wire, present = wire_label(L, value, symbols)
    present == 1 || throw(ArgumentError("a tree symbol cannot be epsilon"))
    wire
end

function record_tree_rank!(ranks::Dict{UInt64,Int}, label::UInt64,
    rank::Int)
    if haskey(ranks, label) && ranks[label] != rank
        throw(ArgumentError("a ranked tree symbol has conflicting arities"))
    end
    ranks[label] = rank
end

function flatten_tree(tree::Tree, ::Type{L}, symbols) where {L}
    nodes = AbiTreeNode[]
    pending = Tree[tree]
    while !isempty(pending)
        current = pop!(pending)
        label = tree_label(L, current.label, symbols)
        length(current.children) <= typemax(UInt64) || throw(ArgumentError(
            "tree rank exceeds UInt64"))
        push!(nodes, AbiTreeNode(TREE_SYMBOL_NODE, label,
            UInt64(length(current.children)), 0, 0))
        for child in Iterators.reverse(current.children)
            push!(pending, child)
        end
    end
    nodes
end

function flatten_pattern(pattern::TreePattern, ::Type{L}, symbols,
    ranks::Dict{UInt64,Int}, input_arity::Integer,
    num_states::Integer) where {L}
    nodes = AbiTreeNode[]
    pending = Union{TreePattern,TreeVariable}[pattern]
    while !isempty(pending)
        current = pop!(pending)
        if current isa TreeVariable
            current.state < num_states || throw(ArgumentError(
                "tree variable state is outside the builder"))
            current.input_child <= input_arity || throw(ArgumentError(
                "tree variable child is outside the rule's input rank"))
            push!(nodes, AbiTreeNode(TREE_VARIABLE_NODE, 0, 0,
                current.state, current.input_child - 1))
        else
            label = tree_label(L, current.label, symbols)
            arity = length(current.children)
            record_tree_rank!(ranks, label, arity)
            push!(nodes, AbiTreeNode(TREE_SYMBOL_NODE, label,
                UInt64(arity), 0, 0))
            for child in Iterators.reverse(current.children)
                push!(pending, child)
            end
        end
    end
    nodes
end

"""Checked mutable Julia construction of an immutable native tree transducer."""
mutable struct TreeTransducerBuilder{L,W<:AbstractScalarWeight}
    states::Vector{Union{Nothing,W}}
    start::Union{Nothing,UInt64}
    rules::Vector{AbiTreeRule}
    patterns::Vector{AbiTreeNode}
    input_ranks::Dict{UInt64,Int}
    output_ranks::Dict{UInt64,Int}
    symbols::Union{Nothing,SymbolTable}
    budget::BudgetV2
    closed::Bool
end

function TreeTransducerBuilder{L,W}(;
    symbols=nothing, budget::BudgetV2=DEFAULT_TREE_BUDGET) where
    {L,W<:AbstractScalarWeight}
    unit_domain(L)
    weight_domain(W)
    finite_tree_budget(budget)
    table = checked_symbols(L, symbols)
    TreeTransducerBuilder{L,W}(Union{Nothing,W}[], nothing,
        AbiTreeRule[], AbiTreeNode[], Dict{UInt64,Int}(),
        Dict{UInt64,Int}(), table, budget, false)
end

function tree_builder_open(builder::TreeTransducerBuilder)
    builder.closed && throw(NativeError(STATUS_CLOSED, :tree_builder,
        "tree transducer builder is closed"))
    builder
end

function tree_builder_state(builder::TreeTransducerBuilder, state::Integer)
    0 <= state < length(builder.states) || throw(ArgumentError(
        "state is outside the tree transducer builder"))
    UInt64(state)
end

function add_state!(builder::TreeTransducerBuilder)
    tree_builder_open(builder)
    UInt64(length(builder.states)) < min(builder.budget.max_states,
        UInt64(typemax(UInt32))) || throw(ArgumentError(
        "tree transducer state bound exhausted"))
    state = UInt64(length(builder.states))
    push!(builder.states, nothing)
    state
end

function set_start!(builder::TreeTransducerBuilder, state::Integer)
    tree_builder_open(builder)
    builder.start = tree_builder_state(builder, state)
    builder
end

function set_final!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    weight=one(W)) where {L,W}
    tree_builder_open(builder)
    builder.states[Int(tree_builder_state(builder, state)) + 1] =
        typed_weight(W, weight)
    builder
end

function clear_final!(builder::TreeTransducerBuilder, state::Integer)
    tree_builder_open(builder)
    builder.states[Int(tree_builder_state(builder, state)) + 1] = nothing
    builder
end

"""Add one ranked rule; pattern variables use one-based input child indices."""
function add_rule!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    input_symbol, input_arity::Integer, pattern::TreePattern,
    weight=one(W)) where {L,W}
    tree_builder_open(builder)
    state_id = tree_builder_state(builder, state)
    0 <= input_arity <= typemax(Int) || throw(ArgumentError(
        "input rank must fit a native array size"))
    input = tree_label(L, input_symbol, builder.symbols)
    next_input_ranks = copy(builder.input_ranks)
    next_output_ranks = copy(builder.output_ranks)
    record_tree_rank!(next_input_ranks, input, Int(input_arity))
    nodes = flatten_pattern(pattern, L, builder.symbols,
        next_output_ranks, input_arity, length(builder.states))
    total = Base.Checked.checked_add(length(builder.rules),
        length(builder.patterns))
    total = Base.Checked.checked_add(total, length(nodes))
    total = Base.Checked.checked_add(total, 1)
    UInt64(total) <= builder.budget.max_arcs || throw(ArgumentError(
        "tree transducer rule and pattern bound exhausted"))
    typed = typed_weight(W, weight)
    push!(builder.rules, AbiTreeRule(state_id, input, UInt64(input_arity),
        UInt64(length(builder.patterns)), UInt64(length(nodes)),
        raw_weight(typed)))
    append!(builder.patterns, nodes)
    builder.input_ranks = next_input_ranks
    builder.output_ranks = next_output_ranks
    builder
end

function close!(builder::TreeTransducerBuilder)
    builder.closed && return nothing
    empty!(builder.states)
    empty!(builder.rules)
    empty!(builder.patterns)
    builder.closed = true
    nothing
end
Base.close(builder::TreeTransducerBuilder) = close!(builder)
Base.isopen(builder::TreeTransducerBuilder) = !builder.closed

"""An immutable owned native tree transducer snapshot."""
mutable struct TreeTransducer{L,W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    symbols::Union{Nothing,SymbolTable}
    closed::Bool
end

function close!(graph::TreeTransducer)
    graph.closed && return nothing
    ccall(native(:lling_tree_free), Cvoid, (Ptr{Cvoid},), graph.handle)
    graph.handle = C_NULL
    graph.closed = true
    nothing
end
Base.close(graph::TreeTransducer) = close!(graph)
Base.isopen(graph::TreeTransducer) = !graph.closed

function tree_handle(graph::TreeTransducer)
    isopen(graph) || throw(NativeError(STATUS_CLOSED, :tree_transducer,
        "tree transducer is closed"))
    graph.handle
end

function build!(builder::TreeTransducerBuilder{L,W}) where {L,W}
    tree_builder_open(builder)
    isempty(builder.states) || !isnothing(builder.start) || throw(ArgumentError(
        "a nonempty tree transducer needs a start state"))
    flags = UInt8[isnothing(weight) ? 0 : 1 for weight in builder.states]
    weights = Float64[isnothing(weight) ? 0.0 : raw_weight(weight)
        for weight in builder.states]
    output = Ref{Ptr{Cvoid}}(C_NULL)
    start = isnothing(builder.start) ? UInt64(typemax(UInt32)) : builder.start
    frozen = frozen_copy(builder.symbols)
    rules = builder.rules
    patterns = builder.patterns
    GC.@preserve flags weights rules patterns begin
        checked(ccall(native(:lling_tree_new), UInt32,
            (UInt32, UInt32, UInt64, UInt64, Ptr{UInt8}, Ptr{Float64},
                Ptr{AbiTreeRule}, UInt64, Ptr{AbiTreeNode}, UInt64,
                Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
            UInt32(unit_domain(L)), UInt32(weight_domain(W)),
            UInt64(length(builder.states)), start, flags, weights,
            rules, UInt64(length(rules)), patterns,
            UInt64(length(patterns)), Ref(builder.budget), output),
            :tree_new)
    end
    graph = TreeTransducer{L,W}(output[], frozen, false)
    finalizer(finalize_close, graph)
    close!(builder)
    graph
end

"""Return state and rule counts plus the optional native start state."""
function tree_info(graph::TreeTransducer)
    states = Ref{Csize_t}(0)
    rules = Ref{Csize_t}(0)
    start = Ref{UInt64}(0)
    checked(ccall(native(:lling_tree_info), UInt32,
        (Ptr{Cvoid}, Ref{Csize_t}, Ref{Csize_t}, Ref{UInt64}),
        tree_handle(graph), states, rules, start), :tree_info)
    (num_states=Int(states[]), num_rules=Int(rules[]),
        start=start[] == UInt64(typemax(UInt32)) ? nothing : start[])
end

"""Read one native state as its final weight or `nothing`."""
function tree_state(graph::TreeTransducer{L,W}, state::Integer) where {L,W}
    0 <= state < typemax(UInt32) || throw(ArgumentError(
        "tree state must fit a native state ID"))
    final = Ref{UInt8}(0)
    weight = Ref{Float64}(0)
    checked(ccall(native(:lling_tree_state_info), UInt32,
        (Ptr{Cvoid}, UInt64, Ref{UInt8}, Ref{Float64}),
        tree_handle(graph), UInt64(state), final, weight), :tree_state_info)
    final[] == 1 ? decode_weight(W, weight[]) : nothing
end

function unflatten_pattern(nodes::Vector{AbiTreeNode}, ::Type{L}) where {L}
    pending = Union{TreePattern{L},TreeVariable}[]
    for node in Iterators.reverse(nodes)
        if node.kind == TREE_VARIABLE_NODE
            push!(pending, TreeVariable(node.state, node.variable_index + 1))
        else
            children = Union{TreePattern{L},TreeVariable}[]
            for _ in 1:Int(node.arity)
                push!(children, pop!(pending))
            end
            push!(pending, TreePattern{L}(decode_label(L, node.symbol),
                children))
        end
    end
    only(pending)::TreePattern{L}
end

"""Read one rule and decode only its requested output pattern."""
function tree_rule(graph::TreeTransducer{L,W}, index::Integer) where {L,W}
    index >= 0 || throw(ArgumentError("rule index cannot be negative"))
    raw = Ref{AbiTreeRule}()
    checked(ccall(native(:lling_tree_rule_info), UInt32,
        (Ptr{Cvoid}, Csize_t, Ref{AbiTreeRule}),
        tree_handle(graph), Csize_t(index), raw), :tree_rule_info)
    nodes = Vector{AbiTreeNode}(undef, Int(raw[].pattern_len))
    GC.@preserve nodes begin
        checked(ccall(native(:lling_tree_rule_pattern_copy), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{AbiTreeNode}, Csize_t),
            graph.handle, Csize_t(index), nodes, length(nodes)),
            :tree_rule_pattern_copy)
    end
    TreeRule{L,W}(raw[].state, decode_label(L, raw[].input_symbol),
        Int(raw[].input_arity), unflatten_pattern(nodes, L),
        decode_weight(W, raw[].weight))
end

struct TreeRuleIterator{G}
    graph::G
    count::Int
end
function Base.iterate(iterator::TreeRuleIterator, index::Int=0)
    index >= iterator.count && return nothing
    (tree_rule(iterator.graph, index), index + 1)
end
Base.length(iterator::TreeRuleIterator) = iterator.count
Base.IteratorSize(::Type{<:TreeRuleIterator}) = Base.HasLength()

"""Read rules lazily from an immutable native tree transducer."""
tree_rules(graph::TreeTransducer) =
    TreeRuleIterator(graph, tree_info(graph).num_rules)

"""Owned bounded outputs; iteration decodes one native tree at a time."""
mutable struct TreeResults{L,W<:AbstractScalarWeight}
    handle::Ptr{Cvoid}
    count::Int
    closed::Bool
end

function close!(results::TreeResults)
    results.closed && return nothing
    ccall(native(:lling_tree_results_free), Cvoid,
        (Ptr{Cvoid},), results.handle)
    results.handle = C_NULL
    results.closed = true
    nothing
end
Base.close(results::TreeResults) = close!(results)
Base.isopen(results::TreeResults) = !results.closed
Base.length(results::TreeResults) = results.count
Base.IteratorSize(::Type{<:TreeResults}) = Base.HasLength()

function tree_results_handle(results::TreeResults)
    isopen(results) || throw(NativeError(STATUS_CLOSED, :tree_results,
        "tree results are closed"))
    results.handle
end

function unflatten_tree(nodes::Vector{AbiTreeNode}, ::Type{L}) where {L}
    pending = Tree{L}[]
    for node in Iterators.reverse(nodes)
        children = Tree{L}[]
        for _ in 1:Int(node.arity)
            push!(children, pop!(pending))
        end
        push!(pending, Tree{L}(decode_label(L, node.symbol), children))
    end
    only(pending)
end

function tree_result(results::TreeResults{L,W}, index::Integer) where {L,W}
    1 <= index <= results.count || throw(ArgumentError(
        "tree result index is outside the output range"))
    handle = tree_results_handle(results)
    nodes = Ref{Csize_t}(0)
    weight = Ref{Float64}(0)
    checked(ccall(native(:lling_tree_result_info), UInt32,
        (Ptr{Cvoid}, Csize_t, Ref{Csize_t}, Ref{Float64}),
        handle, Csize_t(index - 1), nodes, weight), :tree_result_info)
    flat = Vector{AbiTreeNode}(undef, Int(nodes[]))
    GC.@preserve flat begin
        checked(ccall(native(:lling_tree_result_copy), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{AbiTreeNode}, Csize_t),
            handle, Csize_t(index - 1), flat, length(flat)),
            :tree_result_copy)
    end
    (unflatten_tree(flat, L), decode_weight(W, weight[]))
end

function Base.iterate(results::TreeResults, index::Int=1)
    index > results.count && return nothing
    (tree_result(results, index), index + 1)
end

"""Evaluate a tree under finite limits and return lazily decoded outputs."""
function tree_transduce(graph::TreeTransducer{L,W}, input::Tree;
    state::Union{Nothing,Integer}=nothing,
    budget::BudgetV2=DEFAULT_TREE_BUDGET) where {L,W}
    tree_handle(graph)
    finite_tree_budget(budget)
    selected = if isnothing(state)
        UInt64(typemax(UInt32))
    else
        0 <= state < typemax(UInt32) || throw(ArgumentError(
            "tree state must fit a native state ID"))
        UInt64(state)
    end
    nodes = flatten_tree(input, L, graph.symbols)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve nodes begin
        checked(ccall(native(:lling_tree_transduce), UInt32,
            (Ptr{Cvoid}, Ptr{AbiTreeNode}, UInt64, UInt64,
                Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
            graph.handle, nodes, UInt64(length(nodes)), selected,
            Ref(budget), output), :tree_transduce)
    end
    count = Ref{Csize_t}(0)
    try
        checked(ccall(native(:lling_tree_results_info), UInt32,
            (Ptr{Cvoid}, Ref{Csize_t}), output[], count),
            :tree_results_info)
        results = TreeResults{L,W}(output[], Int(count[]), false)
        finalizer(finalize_close, results)
        results
    catch
        ccall(native(:lling_tree_results_free), Cvoid,
            (Ptr{Cvoid},), output[])
        rethrow()
    end
end
