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

tree_node_limit(budget::BudgetV2) =
    Int(min(budget.max_arcs, UInt64(typemax(Int))))

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

function flatten_tree(tree::Tree, ::Type{L}, symbols;
    max_nodes::Integer=typemax(Int)) where {L}
    nodes = AbiTreeNode[]
    pending = Tuple{Tree,Bool}[(tree, false)]
    active = IdDict{Any,Bool}()
    while !isempty(pending)
        current, leaving = pop!(pending)
        if leaving
            delete!(active, current.children)
            continue
        end
        haskey(active, current.children) && throw(ArgumentError(
            "an input tree contains a child cycle"))
        length(nodes) < max_nodes || throw(ArgumentError(
            "input tree exceeds the node budget"))
        label = tree_label(L, current.label, symbols)
        length(current.children) <= typemax(UInt64) || throw(ArgumentError(
            "tree rank exceeds UInt64"))
        push!(nodes, AbiTreeNode(TREE_SYMBOL_NODE, label,
            UInt64(length(current.children)), 0, 0))
        active[current.children] = true
        push!(pending, (current, true))
        for child in Iterators.reverse(current.children)
            push!(pending, (child, false))
        end
    end
    nodes
end

function flatten_pattern(pattern::TreePattern, ::Type{L}, symbols,
    ranks::Dict{UInt64,Int}, input_arity::Integer,
    num_states::Integer; check_variables::Bool=true,
    max_nodes::Integer=typemax(Int)) where {L}
    nodes = AbiTreeNode[]
    pending = Tuple{Union{TreePattern,TreeVariable},Bool}[(pattern, false)]
    active = IdDict{Any,Bool}()
    while !isempty(pending)
        current, leaving = pop!(pending)
        if leaving
            delete!(active, current.children)
            continue
        end
        length(nodes) < max_nodes || throw(ArgumentError(
            "tree pattern exceeds the node budget"))
        if current isa TreeVariable
            if check_variables
                current.state < num_states || throw(ArgumentError(
                    "tree variable state is outside the builder"))
                current.input_child <= input_arity || throw(ArgumentError(
                    "tree variable child is outside the rule's input rank"))
            end
            push!(nodes, AbiTreeNode(TREE_VARIABLE_NODE, 0, 0,
                current.state, current.input_child - 1))
        else
            haskey(active, current.children) && throw(ArgumentError(
                "a tree pattern contains a child cycle"))
            label = tree_label(L, current.label, symbols)
            arity = length(current.children)
            record_tree_rank!(ranks, label, arity)
            push!(nodes, AbiTreeNode(TREE_SYMBOL_NODE, label,
                UInt64(arity), 0, 0))
            active[current.children] = true
            push!(pending, (current, true))
            for child in Iterators.reverse(current.children)
                push!(pending, (child, false))
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
    existing = Base.Checked.checked_add(length(builder.rules),
        length(builder.patterns))
    limit = tree_node_limit(builder.budget)
    existing < limit || throw(ArgumentError(
        "tree transducer rule and pattern bound exhausted"))
    nodes = flatten_pattern(pattern, L, builder.symbols,
        next_output_ranks, input_arity, length(builder.states);
        max_nodes=limit - existing - 1)
    total = existing
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

function adopt_tree_results(output::Ptr{Cvoid}, ::Type{L}, ::Type{W}) where
    {L,W<:AbstractScalarWeight}
    count = Ref{Csize_t}(0)
    try
        checked(ccall(native(:lling_tree_results_info), UInt32,
            (Ptr{Cvoid}, Ref{Csize_t}), output, count),
            :tree_results_info)
        results = TreeResults{L,W}(output, Int(count[]), false)
        finalizer(finalize_close, results)
        results
    catch
        ccall(native(:lling_tree_results_free), Cvoid,
            (Ptr{Cvoid},), output)
        rethrow()
    end
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
    nodes = flatten_tree(input, L, graph.symbols;
        max_nodes=tree_node_limit(budget))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve nodes begin
        checked(ccall(native(:lling_tree_transduce), UInt32,
            (Ptr{Cvoid}, Ptr{AbiTreeNode}, UInt64, UInt64,
                Ref{BudgetV2}, Ref{Ptr{Cvoid}}),
            graph.handle, nodes, UInt64(length(nodes)), selected,
            Ref(budget), output), :tree_transduce)
    end
    adopt_tree_results(output[], L, W)
end

"""Apply one zero-based native rule directly, preserving `nothing` on failure."""
function tree_apply_rule(graph::TreeTransducer{L,W}, rule_index::Integer,
    input::Tree; budget::BudgetV2=DEFAULT_TREE_BUDGET) where {L,W}
    tree_handle(graph)
    finite_tree_budget(budget)
    rule_index >= 0 || throw(ArgumentError("rule index cannot be negative"))
    nodes = flatten_tree(input, L, graph.symbols;
        max_nodes=tree_node_limit(budget))
    applicable = Ref{UInt8}(0)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve nodes begin
        checked(ccall(native(:lling_tree_apply_rule), UInt32,
            (Ptr{Cvoid}, Csize_t, Ptr{AbiTreeNode}, UInt64,
                Ref{BudgetV2}, Ref{UInt8}, Ref{Ptr{Cvoid}}),
            graph.handle, Csize_t(rule_index), nodes, UInt64(length(nodes)),
            Ref(budget), applicable, output), :tree_apply_rule)
    end
    applicable[] == 0 ? nothing : adopt_tree_results(output[], L, W)
end

"""Instantiate a checked output pattern against an input tree natively."""
function tree_instantiate_pattern(graph::TreeTransducer{L,W},
    pattern::TreePattern, input::Tree;
    budget::BudgetV2=DEFAULT_TREE_BUDGET) where {L,W}
    tree_handle(graph)
    finite_tree_budget(budget)
    patterns = flatten_pattern(pattern, L, graph.symbols,
        Dict{UInt64,Int}(), length(input.children),
        tree_info(graph).num_states; check_variables=false,
        max_nodes=tree_node_limit(budget))
    nodes = flatten_tree(input, L, graph.symbols;
        max_nodes=tree_node_limit(budget))
    instantiated = Ref{UInt8}(0)
    output = Ref{Ptr{Cvoid}}(C_NULL)
    GC.@preserve patterns nodes begin
        checked(ccall(native(:lling_tree_instantiate_pattern), UInt32,
            (Ptr{Cvoid}, Ptr{AbiTreeNode}, UInt64, Ptr{AbiTreeNode},
                UInt64, Ref{BudgetV2}, Ref{UInt8}, Ref{Ptr{Cvoid}}),
            graph.handle, patterns, UInt64(length(patterns)), nodes,
            UInt64(length(nodes)), Ref(budget), instantiated, output),
            :tree_instantiate_pattern)
    end
    instantiated[] == 0 ? nothing : adopt_tree_results(output[], L, W)
end

"""Whether a Julia tree node has no children."""
tree_is_leaf(tree::Tree) = isempty(tree.children)

"""The number of immediate children of a Julia tree node."""
tree_arity(tree::Tree) = length(tree.children)

"""Lazy preorder visits with cycle detection for mutable child vectors."""
struct TreePreorder{L}
    root::Tree{L}
end
tree_preorder(tree::Tree) = TreePreorder(tree)

function Base.iterate(iterator::TreePreorder{L},
    state=(Tuple{Tree{L},Bool}[(iterator.root, false)],
        IdDict{Any,Bool}())) where {L}
    pending, active = state
    while !isempty(pending)
        node, leaving = pop!(pending)
        if leaving
            delete!(active, node.children)
            continue
        end
        haskey(active, node.children) && throw(ArgumentError(
            "a tree contains a child cycle"))
        active[node.children] = true
        push!(pending, (node, true))
        for child in Iterators.reverse(node.children)
            push!(pending, (child, false))
        end
        return (node, (pending, active))
    end
    nothing
end
Base.IteratorSize(::Type{<:TreePreorder}) = Base.SizeUnknown()

"""Count all nodes in a tree, including repeated shared subtrees."""
function tree_size(tree::Tree)
    count = 0
    for _ in tree_preorder(tree)
        count = Base.Checked.checked_add(count, 1)
    end
    count
end

"""Return the number of nodes on the longest root-to-leaf path."""
function tree_depth(tree::Tree{L}) where {L}
    pending = Tuple{Tree{L},Int,Bool}[(tree, 1, false)]
    active = IdDict{Any,Bool}()
    maximum = 0
    while !isempty(pending)
        node, depth, leaving = pop!(pending)
        if leaving
            delete!(active, node.children)
            continue
        end
        haskey(active, node.children) && throw(ArgumentError(
            "a tree contains a child cycle"))
        maximum = max(maximum, depth)
        active[node.children] = true
        push!(pending, (node, depth, true))
        next_depth = Base.Checked.checked_add(depth, 1)
        for child in Iterators.reverse(node.children)
            push!(pending, (child, next_depth, false))
        end
    end
    maximum
end

function tree_map_as(f, tree::Tree{L}, ::Type{M}, mapped_root::M) where {L,M}
    pending = Tuple{Tree{L},Bool,Union{Nothing,Some{M}}}[
        (tree, false, Some{M}(mapped_root))]
    active = IdDict{Any,Bool}()
    built = Tree{M}[]
    while !isempty(pending)
        node, leaving, cached = pop!(pending)
        if leaving
            delete!(active, node.children)
            children = Tree{M}[]
            for _ in node.children
                push!(children, pop!(built))
            end
            reverse!(children)
            push!(built, Tree{M}(cached.value, children))
            continue
        end
        haskey(active, node.children) && throw(ArgumentError(
            "a tree contains a child cycle"))
        active[node.children] = true
        label = isnothing(cached) ? convert(M, f(node.label)) : cached.value
        push!(pending, (node, true, Some{M}(label)))
        for child in Iterators.reverse(node.children)
            push!(pending, (child, false, nothing))
        end
    end
    only(built)
end

"""Map labels without recursion, preserving shape and child order."""
function tree_map(f, tree::Tree{L}) where {L}
    mapped_root = f(tree.label)
    tree_map_as(f, tree, typeof(mapped_root), mapped_root)
end

copy_tree(tree::Tree{L}) where {L} =
    tree_map_as(identity, tree, L, tree.label)

"""Copy the subtree selected by a one-based child path, or return `nothing`."""
function tree_subtree(tree::Tree, path::AbstractVector{<:Integer})
    node = tree
    for child_index in path
        1 <= child_index <= length(node.children) || return nothing
        node = node.children[Int(child_index)]
    end
    copy_tree(node)
end

"""Copy a tree with one subtree replaced at a one-based child path."""
function tree_replace(tree::Tree{L}, path::AbstractVector{<:Integer},
    replacement::Tree{L}) where {L}
    ancestors = Tuple{Tree{L},Int}[]
    node = tree
    for child_index in path
        1 <= child_index <= length(node.children) || return nothing
        push!(ancestors, (node, Int(child_index)))
        node = node.children[Int(child_index)]
    end
    rebuilt = copy_tree(replacement)
    while !isempty(ancestors)
        parent, selected = pop!(ancestors)
        children = Tree{L}[]
        for (index, child) in enumerate(parent.children)
            push!(children, index == selected ? rebuilt : copy_tree(child))
        end
        rebuilt = Tree{L}(parent.label, children)
    end
    rebuilt
end

"""The immediate rank of a tree output pattern."""
pattern_arity(pattern::TreePattern) = length(pattern.children)
pattern_is_leaf(pattern::TreePattern) = isempty(pattern.children)

function pattern_variable_occurrences(pattern::TreePattern)
    indices = UInt64[]
    pending = Tuple{Union{TreePattern,TreeVariable},Bool}[(pattern, false)]
    active = IdDict{Any,Bool}()
    while !isempty(pending)
        node, leaving = pop!(pending)
        if leaving
            delete!(active, node.children)
        elseif node isa TreeVariable
            push!(indices, node.input_child)
        else
            haskey(active, node.children) && throw(ArgumentError(
                "a tree pattern contains a child cycle"))
            active[node.children] = true
            push!(pending, (node, true))
            for child in Iterators.reverse(node.children)
                push!(pending, (child, false))
            end
        end
    end
    indices
end

"""Sorted one-based input child indices selected by an output pattern."""
pattern_variable_indices(pattern::TreePattern) =
    unique!(sort!(pattern_variable_occurrences(pattern)))

tree_is_deleting(rule::TreeRule) =
    length(pattern_variable_indices(rule.output_pattern)) < rule.input_arity
tree_is_copying(rule::TreeRule) = begin
    indices = pattern_variable_occurrences(rule.output_pattern)
    length(indices) > length(unique(indices))
end
tree_is_linear(rule::TreeRule) =
    !tree_is_deleting(rule) && !tree_is_copying(rule)

function tree_is_identity_like(rule::TreeRule)
    rule.input_symbol == rule.output_pattern.label || return false
    rule.input_arity == length(rule.output_pattern.children) || return false
    all(child isa TreeVariable && child.input_child == index
        for (index, child) in enumerate(rule.output_pattern.children))
end

"""Append an identity tree rule using Julia one-based child indices."""
function add_identity_rule!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    symbol, arity::Integer, weight=one(W)) where {L,W}
    0 <= arity <= typemax(Int) || throw(ArgumentError("invalid tree rank"))
    add_rule!(builder, state, symbol, arity,
        TreePattern(symbol, [TreeVariable(state, index)
            for index in 1:Int(arity)]), weight)
end

"""Append a relabeling rule that keeps each input child in place."""
function add_relabel_rule!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    input_symbol, output_symbol, arity::Integer,
    weight=one(W)) where {L,W}
    0 <= arity <= typemax(Int) || throw(ArgumentError("invalid tree rank"))
    add_rule!(builder, state, input_symbol, arity,
        TreePattern(output_symbol, [TreeVariable(state, index)
            for index in 1:Int(arity)]), weight)
end

"""Append a rule that discards all input children."""
function add_deletion_rule!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    input_symbol, input_arity::Integer, output_symbol,
    weight=one(W)) where {L,W}
    add_rule!(builder, state, input_symbol, input_arity,
        TreePattern(output_symbol), weight)
end

"""Append a rule that selects, copies, or reorders input children."""
function add_copy_rule!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    input_symbol, input_arity::Integer, output_symbol,
    child_indices::AbstractVector{<:Integer}, weight=one(W)) where {L,W}
    add_rule!(builder, state, input_symbol, input_arity,
        TreePattern(output_symbol, [TreeVariable(state, index)
            for index in child_indices]), weight)
end

"""Append a same-rank reordering rule from one-based child positions."""
function add_swap_rule!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    input_symbol, output_symbol,
    permutation::AbstractVector{<:Integer}, weight=one(W)) where {L,W}
    add_copy_rule!(builder, state, input_symbol, length(permutation),
        output_symbol, permutation, weight)
end

"""Append the native outer-rank-one flattening convenience rule."""
function add_flatten_rule!(builder::TreeTransducerBuilder{L,W}, state::Integer,
    symbol, inner_state::Integer, weight=one(W)) where {L,W}
    add_rule!(builder, state, symbol, 1,
        TreePattern(symbol, [TreeVariable(inner_state, 1)]), weight)
end
