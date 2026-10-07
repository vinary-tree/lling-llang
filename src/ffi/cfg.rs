//! Bounded, owned CFG analysis over token sequences.

use super::{
    boundary, import_native_wfst_with_budget_and_stats, map_error, required_mut, set_error,
    wfst_domains, AbiScalarLabel, AbiScalarWeight, GraphBudget, LlingLlangStatus,
};
use crate::backend::HashMapBackend;
use crate::cfg::{
    EarleyParser, ForestChild, ForestNodeId, Grammar, NonTerminal, ParseAnalysis, ParseError,
    ParseLimits, Production, RuleId, Symbol,
};
use crate::lattice::{Edge, EdgeId, EdgeMetadata, Lattice, LatticeBuilder, Node, NodeId};
use crate::semiring::TropicalWeight;
use crate::wfst::{VectorWfst, Wfst};
use smallvec::SmallVec;
use vinary_tree_interop::VtResource;

/// RHS symbol tags in [`LlingCfgSymbol`].
pub const LLING_CFG_NONTERMINAL: u32 = 1;
/// Terminal tag in [`LlingCfgSymbol`].
pub const LLING_CFG_TERMINAL: u32 = 2;
/// Epsilon tag in [`LlingCfgSymbol`].
pub const LLING_CFG_EPSILON: u32 = 3;

/// One explicitly typed grammar symbol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlingCfgSymbol {
    /// One of the `LLING_CFG_*` symbol tags.
    pub kind: u32,
    /// Non-terminal index or terminal vocabulary ID; zero for epsilon.
    pub value: u32,
}

/// One grammar production with borrowed RHS storage during compilation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlingCfgRule {
    /// Zero-based non-terminal index on the left-hand side.
    pub lhs: u32,
    /// Borrowed RHS symbols; null only when `rhs_len` is zero.
    pub rhs: *const LlingCfgSymbol,
    /// Number of RHS symbols.
    pub rhs_len: usize,
    /// Finite log probability; the core parser currently treats it as metadata.
    pub log_prob: f32,
}

/// Strict per-query bounds; all fields must be initialized by the caller.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlingCfgParseLimits {
    /// Size of this structure for additive ABI validation.
    pub struct_size: u32,
    /// Set to one.
    pub version: u32,
    /// Maximum token count accepted before lattice construction.
    pub max_tokens: u64,
    /// Maximum distinct Earley chart items.
    pub max_chart_items: u64,
    /// Maximum packed forest nodes.
    pub max_forest_nodes: u64,
    /// Maximum charged parser operations.
    pub max_work: u64,
}

/// Strict graph-import and parse bounds for a scalar WFST lattice.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlingCfgWfstLimits {
    /// Size of this struct in bytes.
    pub struct_size: u32,
    /// Set to one.
    pub version: u32,
    /// One for input labels, two for output labels.
    pub tape: u32,
    /// Set to zero.
    pub reserved: u32,
    /// Maximum reachable states imported from the resource.
    pub max_states: u64,
    /// Maximum reachable arcs imported from the resource.
    pub max_arcs: u64,
    /// Maximum graph import allocation bytes.
    pub max_bytes: u64,
    /// Maximum graph provider visits and arc processing work.
    pub max_import_work: u64,
    /// Maximum distinct Earley chart items.
    pub max_chart_items: u64,
    /// Maximum packed forest nodes.
    pub max_forest_nodes: u64,
    /// Maximum charged parser operations.
    pub max_parse_work: u64,
}

/// One chart item in deterministic `(position, rule, dot, start)` order.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlingCfgChartItem {
    /// Lattice position at which this item is retained.
    pub position: u32,
    /// Production identifier.
    pub rule: u32,
    /// RHS prefix length already matched.
    pub dot: u32,
    /// Lattice position where the production began.
    pub start: u32,
}

/// Summary of one complete analysis, including rejected inputs.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlingCfgAnalysisInfo {
    /// One if the grammar accepts the token sequence.
    pub accepted: u8,
    /// Fixed to zero.
    pub reserved: [u8; 7],
    /// Distinct chart item count.
    pub chart_items: u64,
    /// Packed forest node count.
    pub forest_nodes: u64,
    /// Complete parse root count.
    pub roots: u64,
    /// Number of retained lattice edges.
    pub edges: u64,
    /// Charged parser operations.
    pub work: u64,
}

/// One packed forest node in stable numeric identifier order.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlingCfgForestNodeInfo {
    /// Production identifier.
    pub rule: u32,
    /// Starting lattice position.
    pub start: u32,
    /// Ending lattice position.
    pub end: u32,
    /// One if this node is a complete parse root.
    pub is_root: u8,
    /// Fixed to zero.
    pub reserved: [u8; 3],
    /// Number of packed child entries.
    pub children: u64,
}

/// Packed child tags: a lattice terminal edge or a derivation list.
pub const LLING_CFG_CHILD_TERMINAL: u32 = 1;
/// Packed derivation child tag.
pub const LLING_CFG_CHILD_DERIVATION: u32 = 2;

/// One packed child entry; derivation members are queried separately.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlingCfgForestChildInfo {
    /// `LLING_CFG_CHILD_TERMINAL` or `LLING_CFG_CHILD_DERIVATION`.
    pub kind: u32,
    /// Lattice edge identifier for terminals, zero for derivations.
    pub edge_id: u32,
    /// Number of forest-node members for derivations, zero for terminals.
    pub members: u64,
}

/// A compiled grammar with owned native storage.
pub struct LlingCfgGrammar {
    grammar: Grammar,
}

/// A captured chart and packed forest. It remains valid after the grammar is freed.
pub struct LlingCfgAnalysis {
    analysis: ParseAnalysis,
    chart_rows: Vec<LlingCfgChartItem>,
    roots: Vec<u32>,
    edge_labels: Vec<u32>,
}

fn capture_analysis(
    analysis: ParseAnalysis,
    edge_labels: Vec<u32>,
) -> Result<LlingCfgAnalysis, LlingLlangStatus> {
    let mut chart_rows = Vec::with_capacity(analysis.chart.len());
    for (position, item) in analysis.chart.items() {
        chart_rows.push(LlingCfgChartItem {
            position: position.0,
            rule: item.rule.index(),
            dot: u32::try_from(item.dot).map_err(|_| limit("CFG chart dot exceeds u32"))?,
            start: item.start.0,
        });
    }
    chart_rows.sort_unstable_by_key(|item| (item.position, item.rule, item.dot, item.start));
    let mut roots: Vec<_> = analysis.forest.roots().map(|root| root.id()).collect();
    roots.sort_unstable();
    Ok(LlingCfgAnalysis {
        analysis,
        chart_rows,
        roots,
        edge_labels,
    })
}

fn invalid(message: impl Into<String>) -> LlingLlangStatus {
    set_error(message);
    LlingLlangStatus::InvalidArgument
}

fn limit(message: impl Into<String>) -> LlingLlangStatus {
    set_error(message);
    LlingLlangStatus::LimitExceeded
}

fn parse_error(error: ParseError) -> LlingLlangStatus {
    match error {
        ParseError::LimitExceeded(_) => limit(error.to_string()),
        _ => invalid(error.to_string()),
    }
}

fn bounded(value: u64, name: &'static str) -> Result<usize, LlingLlangStatus> {
    usize::try_from(value).map_err(|_| limit(format!("{name} exceeds platform usize")))
}

unsafe fn borrowed<'a, T>(
    pointer: *const T,
    count: usize,
    name: &'static str,
) -> Result<&'a [T], LlingLlangStatus> {
    if count == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        set_error(format!("{name} is null with nonzero count"));
        return Err(LlingLlangStatus::NullPointer);
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, count) })
}

/// Compile an explicitly typed CFG and copy all rule storage. `max_rules` and
/// `max_rhs_symbols` bound memory before any RHS is copied.
///
/// # Safety
/// Non-null input pointers must be readable for their declared counts, and
/// `out_grammar` must point to writable storage.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_grammar_compile(
    start: u32,
    nonterminal_count: u32,
    rules: *const LlingCfgRule,
    rule_count: usize,
    max_rules: usize,
    max_rhs_symbols: usize,
    out_grammar: *mut *mut LlingCfgGrammar,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_grammar, "out_grammar")?;
        *output = std::ptr::null_mut();
        if nonterminal_count == 0 || nonterminal_count > u32::from(u16::MAX) + 1 {
            return Err(invalid(
                "nonterminal_count is outside the native CFG domain",
            ));
        }
        if start >= nonterminal_count {
            return Err(invalid("start non-terminal is outside the grammar"));
        }
        if rule_count > max_rules || rule_count > u32::MAX as usize {
            return Err(limit("grammar rule count exceeds its configured bound"));
        }
        let input = unsafe { borrowed(rules, rule_count, "rules")? };
        let mut productions = Vec::with_capacity(rule_count);
        let mut rhs_total = 0usize;
        for (index, rule) in input.iter().enumerate() {
            if rule.lhs >= nonterminal_count || !rule.log_prob.is_finite() {
                return Err(invalid("invalid CFG production lhs or log probability"));
            }
            rhs_total = rhs_total
                .checked_add(rule.rhs_len)
                .ok_or_else(|| limit("grammar RHS symbol count overflow"))?;
            if rhs_total > max_rhs_symbols {
                return Err(limit(
                    "grammar RHS symbol count exceeds its configured bound",
                ));
            }
            let source = unsafe { borrowed(rule.rhs, rule.rhs_len, "rule rhs")? };
            let mut rhs = SmallVec::with_capacity(source.len());
            for symbol in source {
                let decoded = match symbol.kind {
                    LLING_CFG_NONTERMINAL if symbol.value < nonterminal_count => {
                        Symbol::non_terminal(symbol.value as u16)
                    }
                    LLING_CFG_TERMINAL => Symbol::terminal(symbol.value),
                    LLING_CFG_EPSILON if symbol.value == 0 && source.len() == 1 => {
                        Symbol::epsilon()
                    }
                    _ => return Err(invalid("invalid CFG RHS symbol")),
                };
                rhs.push(decoded);
            }
            productions.push(Production::with_prob(
                RuleId::new(index as u32),
                NonTerminal::new(rule.lhs as u16),
                rhs,
                rule.log_prob,
            ));
        }
        let grammar = Grammar::new(
            NonTerminal::new(start as u16),
            productions,
            nonterminal_count as usize,
        )
        .map_err(|error| invalid(error.to_string()))?;
        *output = Box::into_raw(Box::new(LlingCfgGrammar { grammar }));
        Ok(())
    })
}

/// Free a compiled grammar. Null is accepted.
///
/// # Safety
/// `grammar` must be null or a live pointer returned by `lling_cfg_grammar_compile`.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_grammar_free(grammar: *mut LlingCfgGrammar) {
    if !grammar.is_null() {
        drop(unsafe { Box::from_raw(grammar) });
    }
}

/// Analyze a bounded token sequence and retain its chart and packed forest.
/// Rejection is a successful analysis with `accepted == 0`.
///
/// # Safety
/// The grammar and input pointers must be valid for this call, `limits` must
/// point to a complete configuration, and `out_analysis` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_parse_tokens(
    grammar: *const LlingCfgGrammar,
    tokens: *const u32,
    token_count: usize,
    limits: *const LlingCfgParseLimits,
    out_analysis: *mut *mut LlingCfgAnalysis,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_analysis, "out_analysis")?;
        *output = std::ptr::null_mut();
        let grammar = unsafe { grammar.as_ref() }.ok_or_else(|| {
            set_error("grammar is null");
            LlingLlangStatus::NullPointer
        })?;
        let limits = unsafe { limits.as_ref() }.ok_or_else(|| {
            set_error("limits is null");
            LlingLlangStatus::NullPointer
        })?;
        if limits.struct_size as usize != std::mem::size_of::<LlingCfgParseLimits>()
            || limits.version != 1
        {
            return Err(invalid(
                "CFG parse limits have an incompatible size or version",
            ));
        }
        if u64::try_from(token_count).map_or(true, |count| count > limits.max_tokens)
            || token_count >= u32::MAX as usize
        {
            return Err(limit("CFG token count exceeds its configured bound"));
        }
        if limits.max_forest_nodes > u64::from(u32::MAX)
            || limits.max_chart_items > u64::from(u32::MAX)
        {
            return Err(invalid("CFG item/node limits exceed representable IDs"));
        }
        let input = unsafe { borrowed(tokens, token_count, "tokens")? };
        let mut builder = LatticeBuilder::<TropicalWeight, _>::with_capacity(
            HashMapBackend::new(),
            token_count,
            1,
        );
        for (index, &token) in input.iter().enumerate() {
            builder.add_correction_by_id(
                index,
                index + 1,
                token,
                TropicalWeight::new(0.0),
                EdgeMetadata::default(),
            );
        }
        let lattice = builder.build(token_count);
        let parser = EarleyParser::new(&grammar.grammar);
        let analysis = parser
            .analyze_lattice_with_limits(
                &lattice,
                ParseLimits {
                    max_chart_items: bounded(limits.max_chart_items, "max_chart_items")?,
                    max_forest_nodes: bounded(limits.max_forest_nodes, "max_forest_nodes")?,
                    max_work: limits.max_work,
                },
            )
            .map_err(parse_error)?;
        *output = Box::into_raw(Box::new(capture_analysis(analysis, input.to_vec())?));
        Ok(())
    })
}

pub(super) fn parse_wfst_typed<L, W>(
    resource: VtResource,
    grammar: &LlingCfgGrammar,
    limits: &LlingCfgWfstLimits,
) -> Result<LlingCfgAnalysis, LlingLlangStatus>
where
    L: AbiScalarLabel,
    W: AbiScalarWeight,
{
    let mut budget = GraphBudget::new(
        Some(limits.max_states),
        Some(limits.max_arcs),
        Some(limits.max_bytes),
        Some(limits.max_import_work),
    );
    let (graph, stats): (VectorWfst<L, W>, _) =
        import_native_wfst_with_budget_and_stats(resource, &mut budget).map_err(map_error)?;
    if stats.states > u64::from(u32::MAX) || stats.arcs > u64::from(u32::MAX) {
        return Err(limit("CFG WFST state or arc ID exceeds UInt32"));
    }
    let mut nodes: Vec<_> = (0..graph.num_states())
        .map(|index| Node::new(NodeId(index as u32)))
        .collect();
    let mut edges = Vec::with_capacity(stats.arcs as usize);
    let mut edge_labels = Vec::with_capacity(stats.arcs as usize);
    let mut finals = Vec::with_capacity(graph.num_states());
    for state in 0..graph.num_states() {
        finals.push(graph.is_final(state as u32));
        for arc in graph.transitions(state as u32) {
            let selected = if limits.tape == 1 {
                &arc.input
            } else {
                &arc.output
            };
            let label = selected.as_ref().ok_or_else(|| {
                set_error("CFG WFST selected tape contains an epsilon arc");
                LlingLlangStatus::Unsupported
            })?;
            let label = u32::try_from(label.encode())
                .map_err(|_| limit("CFG WFST terminal label exceeds UInt32"))?;
            let edge_id = EdgeId(edges.len() as u32);
            let source = NodeId(state as u32);
            let target = NodeId(arc.to);
            nodes[state].outgoing.push(edge_id);
            nodes[target.0 as usize].incoming.push(edge_id);
            edges.push(Edge::new(
                edge_id,
                source,
                target,
                label,
                arc.weight.clone(),
                EdgeMetadata::default(),
            ));
            edge_labels.push(label);
        }
    }
    let lattice = Lattice::new(
        nodes,
        edges,
        NodeId(graph.start()),
        NodeId(graph.start()),
        HashMapBackend::new(),
    );
    if !lattice.is_acyclic() {
        set_error("CFG WFST graph contains a cycle");
        return Err(LlingLlangStatus::NonConvergent);
    }
    let parser = EarleyParser::new(&grammar.grammar);
    let analysis = parser
        .analyze_lattice_with_finals(
            &lattice,
            &finals,
            ParseLimits {
                max_chart_items: bounded(limits.max_chart_items, "max_chart_items")?,
                max_forest_nodes: bounded(limits.max_forest_nodes, "max_forest_nodes")?,
                max_work: limits.max_parse_work,
            },
        )
        .map_err(parse_error)?;
    capture_analysis(analysis, edge_labels)
}

/// Parse a bounded scalar WFST as a CFG lattice. All reachable final states
/// participate in one analysis; selected-tape epsilon arcs and cycles fail
/// explicitly before the analysis is published.
///
/// # Safety
/// All pointers must be readable or writable for their declared types. The
/// resource remains borrowed only during this call.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_parse_wfst_resource(
    grammar: *const LlingCfgGrammar,
    resource: *const VtResource,
    limits: *const LlingCfgWfstLimits,
    out_analysis: *mut *mut LlingCfgAnalysis,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_analysis, "out_analysis")?;
        *output = std::ptr::null_mut();
        let grammar = unsafe { grammar.as_ref() }.ok_or_else(|| {
            set_error("grammar is null");
            LlingLlangStatus::NullPointer
        })?;
        let resource = unsafe { resource.as_ref() }.ok_or_else(|| {
            set_error("resource is null");
            LlingLlangStatus::NullPointer
        })?;
        let limits = unsafe { limits.as_ref() }.ok_or_else(|| {
            set_error("limits is null");
            LlingLlangStatus::NullPointer
        })?;
        if limits.struct_size as usize != std::mem::size_of::<LlingCfgWfstLimits>()
            || limits.version != 1
            || limits.reserved != 0
            || !matches!(limits.tape, 1 | 2)
        {
            return Err(invalid(
                "CFG WFST limits have an incompatible size, version, or tape",
            ));
        }
        if limits.max_states > u64::from(u32::MAX)
            || limits.max_arcs > u64::from(u32::MAX)
            || limits.max_chart_items > u64::from(u32::MAX)
            || limits.max_forest_nodes > u64::from(u32::MAX)
        {
            return Err(invalid("CFG WFST limits exceed representable IDs"));
        }
        let (unit, weight) = wfst_domains(*resource).map_err(map_error)?;
        let analysis = super::cfg_wfst_dispatch(unit, weight, *resource, grammar, limits)?;
        *output = Box::into_raw(Box::new(analysis));
        Ok(())
    })
}

/// Free a captured analysis. Null is accepted.
///
/// # Safety
/// `analysis` must be null or a live pointer returned by a CFG parse call.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_free(analysis: *mut LlingCfgAnalysis) {
    if !analysis.is_null() {
        drop(unsafe { Box::from_raw(analysis) });
    }
}

/// Copy the scalar summary of a captured analysis.
///
/// # Safety
/// Both pointers must be valid for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_info(
    analysis: *const LlingCfgAnalysis,
    out_info: *mut LlingCfgAnalysisInfo,
) -> LlingLlangStatus {
    boundary(|| {
        let analysis = unsafe { analysis.as_ref() }.ok_or_else(|| {
            set_error("analysis is null");
            LlingLlangStatus::NullPointer
        })?;
        let output = required_mut(out_info, "out_info")?;
        *output = LlingCfgAnalysisInfo {
            accepted: u8::from(!analysis.analysis.forest.is_empty()),
            reserved: [0; 7],
            chart_items: analysis.chart_rows.len() as u64,
            forest_nodes: analysis.analysis.forest.num_nodes() as u64,
            roots: analysis.roots.len() as u64,
            edges: analysis.edge_labels.len() as u64,
            work: analysis.analysis.work,
        };
        Ok(())
    })
}

unsafe fn copy_page<T: Copy>(
    source: &[T],
    start: usize,
    out_items: *mut T,
    capacity: usize,
    out_written: *mut usize,
) -> Result<(), LlingLlangStatus> {
    let written = required_mut(out_written, "out_written")?;
    *written = 0;
    if capacity != 0 && out_items.is_null() {
        set_error("out_items is null with nonzero capacity");
        return Err(LlingLlangStatus::NullPointer);
    }
    let count = source.len().saturating_sub(start).min(capacity);
    if count != 0 {
        unsafe { std::ptr::copy_nonoverlapping(source.as_ptr().add(start), out_items, count) };
    }
    *written = count;
    Ok(())
}

/// Copy a bounded page of chart items in deterministic order.
///
/// # Safety
/// The analysis and output pointers must be valid for their declared sizes.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_chart_page(
    analysis: *const LlingCfgAnalysis,
    start: usize,
    out_items: *mut LlingCfgChartItem,
    capacity: usize,
    out_written: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let analysis = unsafe { analysis.as_ref() }.ok_or_else(|| {
            set_error("analysis is null");
            LlingLlangStatus::NullPointer
        })?;
        unsafe {
            copy_page(
                &analysis.chart_rows,
                start,
                out_items,
                capacity,
                out_written,
            )
        }
    })
}

/// Copy a bounded page of sorted complete-parse root identifiers.
///
/// # Safety
/// The analysis and output pointers must be valid for their declared sizes.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_root_page(
    analysis: *const LlingCfgAnalysis,
    start: usize,
    out_items: *mut u32,
    capacity: usize,
    out_written: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let analysis = unsafe { analysis.as_ref() }.ok_or_else(|| {
            set_error("analysis is null");
            LlingLlangStatus::NullPointer
        })?;
        unsafe { copy_page(&analysis.roots, start, out_items, capacity, out_written) }
    })
}

/// Copy a bounded page of lattice-edge token IDs in stable edge order.
///
/// # Safety
/// The analysis and output pointers must be valid for their declared sizes.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_edge_page(
    analysis: *const LlingCfgAnalysis,
    start: usize,
    out_items: *mut u32,
    capacity: usize,
    out_written: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let analysis = unsafe { analysis.as_ref() }.ok_or_else(|| {
            set_error("analysis is null");
            LlingLlangStatus::NullPointer
        })?;
        unsafe {
            copy_page(
                &analysis.edge_labels,
                start,
                out_items,
                capacity,
                out_written,
            )
        }
    })
}

/// Copy metadata for one packed forest node.
///
/// # Safety
/// The analysis and output pointers must be valid for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_forest_node(
    analysis: *const LlingCfgAnalysis,
    node_id: u32,
    out_node: *mut LlingCfgForestNodeInfo,
) -> LlingLlangStatus {
    boundary(|| {
        let analysis = unsafe { analysis.as_ref() }.ok_or_else(|| {
            set_error("analysis is null");
            LlingLlangStatus::NullPointer
        })?;
        let output = required_mut(out_node, "out_node")?;
        let node = analysis
            .analysis
            .forest
            .node(ForestNodeId::new(node_id))
            .ok_or_else(|| invalid("forest node ID is out of range"))?;
        *output = LlingCfgForestNodeInfo {
            rule: node.rule.index(),
            start: node.start.0,
            end: node.end.0,
            is_root: u8::from(analysis.roots.binary_search(&node_id).is_ok()),
            reserved: [0; 3],
            children: node.children.len() as u64,
        };
        Ok(())
    })
}

/// Copy one packed child entry; derivation members are separately indexed.
///
/// # Safety
/// The analysis and output pointers must be valid for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_forest_child(
    analysis: *const LlingCfgAnalysis,
    node_id: u32,
    child_index: usize,
    out_child: *mut LlingCfgForestChildInfo,
) -> LlingLlangStatus {
    boundary(|| {
        let analysis = unsafe { analysis.as_ref() }.ok_or_else(|| {
            set_error("analysis is null");
            LlingLlangStatus::NullPointer
        })?;
        let output = required_mut(out_child, "out_child")?;
        let child = analysis
            .analysis
            .forest
            .node(ForestNodeId::new(node_id))
            .and_then(|node| node.children.get(child_index))
            .ok_or_else(|| invalid("forest child index is out of range"))?;
        *output = match child {
            ForestChild::Terminal(edge) => LlingCfgForestChildInfo {
                kind: LLING_CFG_CHILD_TERMINAL,
                edge_id: edge.0,
                members: 0,
            },
            ForestChild::Derivation(members) => LlingCfgForestChildInfo {
                kind: LLING_CFG_CHILD_DERIVATION,
                edge_id: 0,
                members: members.len() as u64,
            },
        };
        Ok(())
    })
}

/// Copy one forest-node ID inside a packed derivation child.
///
/// # Safety
/// The analysis and output pointers must be valid for this call.
#[no_mangle]
pub unsafe extern "C" fn lling_cfg_analysis_derivation_member(
    analysis: *const LlingCfgAnalysis,
    node_id: u32,
    child_index: usize,
    member_index: usize,
    out_member: *mut u32,
) -> LlingLlangStatus {
    boundary(|| {
        let analysis = unsafe { analysis.as_ref() }.ok_or_else(|| {
            set_error("analysis is null");
            LlingLlangStatus::NullPointer
        })?;
        let output = required_mut(out_member, "out_member")?;
        let ForestChild::Derivation(members) = analysis
            .analysis
            .forest
            .node(ForestNodeId::new(node_id))
            .and_then(|node| node.children.get(child_index))
            .ok_or_else(|| invalid("forest child index is out of range"))?
        else {
            return Err(invalid("forest child is not a derivation"));
        };
        *output = members
            .get(member_index)
            .ok_or_else(|| invalid("derivation member index is out of range"))?
            .id();
        Ok(())
    })
}
