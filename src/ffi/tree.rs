//! Checked, immutable weighted tree transducers at the C boundary.

use super::{
    boundary, bounded_usize, decode_unit_domain, decode_weight_domain, graph_budget_from_v2,
    map_error, required_mut, set_error, AbiScalarLabel, AbiScalarWeight, ArcticWeight, BoolWeight,
    CountWeight, GraphBudget, LlingBudgetV2, LlingLlangStatus, LogWeight, ProbabilityWeight,
    SignedTropicalWeight, TropicalWeight, LLING_BUDGET_ARCS, LLING_BUDGET_BYTES,
    LLING_BUDGET_STATES, LLING_BUDGET_WORK,
};
use crate::tree_transducers::{
    Tree, TreeChild, TreePattern, TreeRule, TreeTransducerLimits, TreeTransducerOps,
    TreeTransducerRunError, VectorTreeTransducer, WeightedTreeTransducer,
};
use crate::wfst::{StateId, NO_STATE};
use std::collections::HashMap;
use std::hash::Hash;
use vinary_tree_interop::{VtUnitDomain, VtWeightDomain};

const ALL_BUDGET_AXES: u64 =
    LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK;
const SYMBOL_NODE: u64 = 0;
const VARIABLE_NODE: u64 = 1;

/// One preorder node of an input/output tree or a rule output pattern.
///
/// Symbol nodes use `symbol` and `arity` and require zero `state` and
/// `variable_index`. Pattern variable nodes use `state` and `variable_index`
/// and require zero `symbol` and `arity`. Input and output trees contain only
/// symbol nodes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlingTreeNode {
    /// Zero for a symbol, one for a pattern variable.
    pub kind: u64,
    /// Scalar label for a symbol node; zero for a variable.
    pub symbol: u64,
    /// Number of immediate children of a symbol node.
    pub arity: u64,
    /// State to apply to a variable's selected input child.
    pub state: u64,
    /// Zero-based selected input child for a variable node.
    pub variable_index: u64,
}

/// One rule; its output pattern is a nonempty slice of the flat node array.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LlingTreeRule {
    /// State in which this rule applies.
    pub state: u64,
    /// Input symbol read by the rule.
    pub input_symbol: u64,
    /// Required rank of the input symbol.
    pub input_arity: u64,
    /// Start of the output pattern in the flat pattern array.
    pub pattern_offset: u64,
    /// Number of preorder nodes in the output pattern.
    pub pattern_len: u64,
    /// Scalar semiring weight of this rule.
    pub weight: f64,
}

fn tree_budget(
    ptr: *const LlingBudgetV2,
) -> Result<(GraphBudget, LlingBudgetV2), LlingLlangStatus> {
    let budget = graph_budget_from_v2(ptr)?;
    let raw = unsafe { *ptr };
    if raw.header.flags & ALL_BUDGET_AXES != ALL_BUDGET_AXES {
        set_error("tree transducers require finite state, arc, byte, and work limits");
        return Err(LlingLlangStatus::InvalidArgument);
    }
    Ok((budget, raw))
}

fn checked_slice_len<T>(length: usize, name: &'static str) -> Result<(), LlingLlangStatus> {
    if length > isize::MAX as usize / std::mem::size_of::<T>() {
        set_error(format!("{name} exceeds the native slice size limit"));
        return Err(LlingLlangStatus::LimitExceeded);
    }
    Ok(())
}

fn map_tree_run_error(error: TreeTransducerRunError) -> LlingLlangStatus {
    set_error(error.to_string());
    LlingLlangStatus::LimitExceeded
}

fn invalid(message: impl Into<String>) -> LlingLlangStatus {
    set_error(message);
    LlingLlangStatus::InvalidArgument
}

fn insert_rank<L: Eq + Hash>(
    ranks: &mut HashMap<L, usize>,
    symbol: L,
    arity: usize,
) -> Result<(), LlingLlangStatus> {
    match ranks.entry(symbol) {
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(arity);
            Ok(())
        }
        std::collections::hash_map::Entry::Occupied(slot) if *slot.get() == arity => Ok(()),
        std::collections::hash_map::Entry::Occupied(_) => {
            Err(invalid("a ranked symbol is used with conflicting arities"))
        }
    }
}

fn decode_pattern<L: AbiScalarLabel + Eq + Hash>(
    nodes: &[LlingTreeNode],
    ranks: &mut HashMap<L, usize>,
) -> Result<TreePattern<L>, LlingLlangStatus> {
    let mut pending = Vec::<TreeChild<L>>::new();
    for node in nodes.iter().rev() {
        match node.kind {
            SYMBOL_NODE => {
                if node.state != 0 || node.variable_index != 0 {
                    return Err(invalid("a symbol pattern node has nonzero variable fields"));
                }
                let label = L::decode(node.symbol)
                    .ok_or_else(|| invalid("pattern symbol is outside the unit domain"))?;
                let arity = bounded_usize(node.arity, "pattern arity")?;
                if arity > pending.len() {
                    return Err(invalid("preorder pattern has too few child nodes"));
                }
                insert_rank(ranks, label.clone(), arity)?;
                let mut children = Vec::with_capacity(arity);
                for _ in 0..arity {
                    children.push(pending.pop().expect("arity checked against pending nodes"));
                }
                pending.push(TreeChild::subtree(TreePattern::new(label, children)));
            }
            VARIABLE_NODE => {
                if node.symbol != 0 || node.arity != 0 {
                    return Err(invalid("a pattern variable has nonzero symbol fields"));
                }
                let state = StateId::try_from(node.state)
                    .map_err(|_| invalid("pattern variable state exceeds native IDs"))?;
                let index = bounded_usize(node.variable_index, "variable index")?;
                pending.push(TreeChild::variable(state, index));
            }
            _ => return Err(invalid("pattern node kind must be zero or one")),
        }
    }
    if pending.len() != 1 {
        return Err(invalid("preorder pattern does not form exactly one tree"));
    }
    match pending.pop().expect("single root checked") {
        TreeChild::Subtree(pattern) => Ok(*pattern),
        TreeChild::Variable { .. } => Err(invalid("an output pattern needs a symbol root")),
    }
}

fn decode_tree<L: AbiScalarLabel + Eq + Hash>(
    nodes: &[LlingTreeNode],
    ranks: &HashMap<L, usize>,
) -> Result<Tree<L>, LlingLlangStatus> {
    let mut pending = Vec::<Tree<L>>::new();
    for node in nodes.iter().rev() {
        if node.kind != SYMBOL_NODE || node.state != 0 || node.variable_index != 0 {
            return Err(invalid("input trees contain only symbol nodes"));
        }
        let label = L::decode(node.symbol)
            .ok_or_else(|| invalid("input symbol is outside the unit domain"))?;
        let arity = bounded_usize(node.arity, "input arity")?;
        if arity > pending.len() {
            return Err(invalid("preorder input tree has too few child nodes"));
        }
        if ranks.get(&label).is_some_and(|&rank| rank != arity) {
            return Err(invalid("input symbol has the wrong ranked arity"));
        }
        let mut children = Vec::with_capacity(arity);
        for _ in 0..arity {
            children.push(pending.pop().expect("arity checked against pending nodes"));
        }
        pending.push(Tree::node(label, children));
    }
    if pending.len() != 1 {
        return Err(invalid("preorder input does not form exactly one tree"));
    }
    Ok(pending.pop().expect("single tree checked"))
}

trait TreeOps: Send + Sync {
    fn transduce(
        &self,
        input: &[LlingTreeNode],
        state: u64,
        mut_budget: GraphBudget,
    ) -> Result<Box<dyn TreeResultOps>, LlingLlangStatus>;
}

trait TreeResultOps: Send + Sync {
    fn len(&self) -> usize;
    fn info(&self, index: usize) -> Result<(usize, f64), LlingLlangStatus>;
    fn copy(&self, index: usize, output: &mut [LlingTreeNode]) -> Result<(), LlingLlangStatus>;
}

struct TypedTree<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    graph: VectorTreeTransducer<L, W>,
    input_ranks: HashMap<L, usize>,
}

struct TypedTreeResults<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    outputs: Vec<(Tree<L>, W)>,
}

impl<L, W> TreeOps for TypedTree<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    fn transduce(
        &self,
        input: &[LlingTreeNode],
        state: u64,
        mut mut_budget: GraphBudget,
    ) -> Result<Box<dyn TreeResultOps>, LlingLlangStatus> {
        let input_count =
            u64::try_from(input.len()).map_err(|_| LlingLlangStatus::LimitExceeded)?;
        let input_bytes = input
            .len()
            .checked_mul(std::mem::size_of::<Tree<L>>())
            .and_then(|size| u64::try_from(size).ok())
            .ok_or(LlingLlangStatus::LimitExceeded)?;
        mut_budget
            .charge(0, input_count, input_bytes, input_count)
            .map_err(map_error)?;
        let tree = decode_tree::<L>(input, &self.input_ranks)?;
        let requested_state = if state == u64::from(NO_STATE) {
            None
        } else {
            let state = StateId::try_from(state)
                .map_err(|_| invalid("tree state exceeds native state IDs"))?;
            if state as usize >= self.graph.num_states() {
                return Err(invalid("tree state is outside the graph's state range"));
            }
            Some(state)
        };
        let limits = TreeTransducerLimits {
            max_results: bounded_usize(mut_budget.remaining(0), "result limit")?,
            max_nodes: bounded_usize(mut_budget.remaining(1), "node limit")?,
            max_bytes: bounded_usize(mut_budget.remaining(2), "byte limit")?,
            max_work: bounded_usize(mut_budget.remaining(3), "work limit")?,
        };
        let outputs = if self.graph.num_states() == 0 {
            Vec::new()
        } else if let Some(state) = requested_state {
            self.graph
                .transduce_from_state_bounded(state, &tree, limits)
                .map_err(map_tree_run_error)?
        } else {
            self.graph
                .transduce_bounded(&tree, limits)
                .map_err(map_tree_run_error)?
        };
        Ok(Box::new(TypedTreeResults { outputs }))
    }
}

impl<L, W> TreeResultOps for TypedTreeResults<L, W>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    fn len(&self) -> usize {
        self.outputs.len()
    }

    fn info(&self, index: usize) -> Result<(usize, f64), LlingLlangStatus> {
        let (tree, weight) = self
            .outputs
            .get(index)
            .ok_or_else(|| invalid("tree result index is outside the output range"))?;
        let weight = weight.clone().encode().ok_or_else(|| {
            set_error("tree result weight is not exactly representable by the scalar ABI");
            LlingLlangStatus::LimitExceeded
        })?;
        Ok((tree.size(), weight))
    }

    fn copy(&self, index: usize, output: &mut [LlingTreeNode]) -> Result<(), LlingLlangStatus> {
        let (tree, _) = self
            .outputs
            .get(index)
            .ok_or_else(|| invalid("tree result index is outside the output range"))?;
        if output.len() < tree.size() {
            set_error("tree result buffer is smaller than the output tree");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        for (slot, node) in output.iter_mut().zip(tree.preorder()) {
            *slot = LlingTreeNode {
                kind: SYMBOL_NODE,
                symbol: node.label().encode(),
                arity: u64::try_from(node.arity()).map_err(|_| LlingLlangStatus::LimitExceeded)?,
                state: 0,
                variable_index: 0,
            };
        }
        Ok(())
    }
}

/// Immutable captured transducer and its lossless flat rule snapshot.
pub struct LlingTreeTransducer {
    num_states: usize,
    start: u64,
    final_flags: Vec<u8>,
    final_weights: Vec<f64>,
    rules: Vec<LlingTreeRule>,
    patterns: Vec<LlingTreeNode>,
    inner: Box<dyn TreeOps>,
}

/// Bounded native output collection read one tree at a time by the caller.
pub struct LlingTreeResults {
    inner: Box<dyn TreeResultOps>,
}

struct RawTree<'a> {
    num_states: usize,
    start: u64,
    final_flags: &'a [u8],
    final_weights: &'a [f64],
    rules: &'a [LlingTreeRule],
    patterns: &'a [LlingTreeNode],
}

fn make_typed<L, W>(
    raw: &RawTree<'_>,
    mut budget: GraphBudget,
) -> Result<LlingTreeTransducer, LlingLlangStatus>
where
    L: AbiScalarLabel + Eq + Hash,
    W: AbiScalarWeight,
{
    let rule_and_pattern_count = raw
        .rules
        .len()
        .checked_add(raw.patterns.len())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let bytes = raw
        .num_states
        .checked_mul(std::mem::size_of::<crate::tree_transducers::TransducerState<W>>() + 9)
        .and_then(|bytes| {
            raw.rules
                .len()
                .checked_mul(
                    std::mem::size_of::<TreeRule<L, W>>() + std::mem::size_of::<LlingTreeRule>(),
                )
                .and_then(|rules| bytes.checked_add(rules))
        })
        .and_then(|bytes| {
            raw.patterns
                .len()
                .checked_mul(
                    std::mem::size_of::<TreeChild<L>>() + std::mem::size_of::<LlingTreeNode>(),
                )
                .and_then(|patterns| bytes.checked_add(patterns))
        })
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    let work = raw
        .num_states
        .checked_add(rule_and_pattern_count)
        .and_then(|work| u64::try_from(work).ok())
        .ok_or(LlingLlangStatus::LimitExceeded)?;
    budget
        .charge(
            u64::try_from(raw.num_states).map_err(|_| LlingLlangStatus::LimitExceeded)?,
            u64::try_from(rule_and_pattern_count).map_err(|_| LlingLlangStatus::LimitExceeded)?,
            bytes,
            work,
        )
        .map_err(map_error)?;

    let mut graph = VectorTreeTransducer::<L, W>::new();
    for (&flag, &raw_weight) in raw.final_flags.iter().zip(raw.final_weights) {
        if flag > 1 {
            return Err(invalid("tree final flag must be zero or one"));
        }
        let state = graph
            .try_add_state()
            .map_err(|error| invalid(error.to_string()))?;
        if flag == 1 {
            let weight = W::decode(raw_weight)
                .ok_or_else(|| invalid("tree final weight is outside the semiring domain"))?;
            graph
                .try_set_final(state, weight)
                .map_err(|error| invalid(error.to_string()))?;
        }
    }
    if raw.num_states == 0 {
        if raw.start != u64::from(NO_STATE) || !raw.rules.is_empty() {
            return Err(invalid("an empty tree transducer needs no start or rules"));
        }
    } else {
        let start = StateId::try_from(raw.start)
            .map_err(|_| invalid("tree start state exceeds native IDs"))?;
        graph
            .try_set_start(start)
            .map_err(|error| invalid(error.to_string()))?;
    }

    let mut input_ranks = HashMap::new();
    let mut output_ranks = HashMap::new();
    for rule in raw.rules {
        let state = StateId::try_from(rule.state)
            .map_err(|_| invalid("tree rule state exceeds native IDs"))?;
        let input = L::decode(rule.input_symbol)
            .ok_or_else(|| invalid("tree rule input symbol is outside the unit domain"))?;
        let arity = bounded_usize(rule.input_arity, "rule input arity")?;
        let offset = bounded_usize(rule.pattern_offset, "pattern offset")?;
        let length = bounded_usize(rule.pattern_len, "pattern length")?;
        let end = offset
            .checked_add(length)
            .ok_or(LlingLlangStatus::LimitExceeded)?;
        if length == 0 || end > raw.patterns.len() {
            return Err(invalid(
                "rule pattern slice is empty or outside the flat array",
            ));
        }
        insert_rank(&mut input_ranks, input.clone(), arity)?;
        let pattern = decode_pattern::<L>(&raw.patterns[offset..end], &mut output_ranks)?;
        let weight = W::decode(rule.weight)
            .ok_or_else(|| invalid("tree rule weight is outside the semiring domain"))?;
        graph
            .try_add_rule(TreeRule::new(state, input, arity, pattern, weight))
            .map_err(|error| invalid(error.to_string()))?;
    }

    Ok(LlingTreeTransducer {
        num_states: raw.num_states,
        start: raw.start,
        final_flags: raw.final_flags.to_vec(),
        final_weights: raw.final_weights.to_vec(),
        rules: raw.rules.to_vec(),
        patterns: raw.patterns.to_vec(),
        inner: Box::new(TypedTree { graph, input_ranks }),
    })
}

/// Capture a ranked tree transducer from borrowed flat arrays.
///
/// Rule output patterns occupy nonempty preorder slices of `patterns`.
/// Input and output ranks are checked independently. The returned graph owns
/// both its native transducer and a lossless flat rule snapshot.
///
/// # Safety
///
/// Nonempty arrays must be readable for their declared lengths. `budget` must
/// be readable, `out_graph` writable, and every input/output region disjoint.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_new(
    unit_domain: u32,
    weight_domain: u32,
    num_states: u64,
    start: u64,
    final_flags: *const u8,
    final_weights: *const f64,
    rules: *const LlingTreeRule,
    num_rules: u64,
    patterns: *const LlingTreeNode,
    num_pattern_nodes: u64,
    budget: *const LlingBudgetV2,
    out_graph: *mut *mut LlingTreeTransducer,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_graph, "out_graph")?;
        let num_states = bounded_usize(num_states, "tree states")?;
        let num_rules = bounded_usize(num_rules, "tree rules")?;
        let num_pattern_nodes = bounded_usize(num_pattern_nodes, "pattern nodes")?;
        if num_states >= NO_STATE as usize {
            return Err(invalid("tree state count must fit native state IDs"));
        }
        checked_slice_len::<u8>(num_states, "tree final flags")?;
        checked_slice_len::<f64>(num_states, "tree final weights")?;
        checked_slice_len::<LlingTreeRule>(num_rules, "tree rules")?;
        checked_slice_len::<LlingTreeNode>(num_pattern_nodes, "tree pattern nodes")?;
        if (num_states != 0 && (final_flags.is_null() || final_weights.is_null()))
            || (num_rules != 0 && rules.is_null())
            || (num_pattern_nodes != 0 && patterns.is_null())
        {
            set_error("a required nonempty tree input array is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let (budget, _) = tree_budget(budget)?;
        let unit = decode_unit_domain(unit_domain)?;
        let weight = decode_weight_domain(weight_domain)?;
        let raw = RawTree {
            num_states,
            start,
            final_flags: if num_states == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(final_flags, num_states) }
            },
            final_weights: if num_states == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(final_weights, num_states) }
            },
            rules: if num_rules == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(rules, num_rules) }
            },
            patterns: if num_pattern_nodes == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(patterns, num_pattern_nodes) }
            },
        };
        let graph = dispatch_scalar_domains!(unit, weight, make_typed, &raw, budget)?;
        *output = Box::into_raw(Box::new(graph));
        Ok(())
    })
}

/// Return the captured state/rule counts and optional start-state sentinel.
///
/// # Safety
///
/// `graph` must be live; all output slots must be writable and disjoint.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_info(
    graph: *const LlingTreeTransducer,
    out_states: *mut usize,
    out_rules: *mut usize,
    out_start: *mut u64,
) -> LlingLlangStatus {
    boundary(|| {
        let states = required_mut(out_states, "out_states")?;
        let rules = required_mut(out_rules, "out_rules")?;
        let start = required_mut(out_start, "out_start")?;
        if graph.is_null() {
            set_error("tree graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let graph = unsafe { &*graph };
        *states = graph.num_states;
        *rules = graph.rules.len();
        *start = graph.start;
        Ok(())
    })
}

/// Read one state's final flag and scalar weight.
///
/// # Safety
///
/// `graph` must be live; output slots must be writable and disjoint.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_state_info(
    graph: *const LlingTreeTransducer,
    state: u64,
    out_final: *mut u8,
    out_weight: *mut f64,
) -> LlingLlangStatus {
    boundary(|| {
        let final_flag = required_mut(out_final, "out_final")?;
        let weight = required_mut(out_weight, "out_weight")?;
        if graph.is_null() {
            set_error("tree graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let graph = unsafe { &*graph };
        let index = bounded_usize(state, "tree state")?;
        let Some(&flag) = graph.final_flags.get(index) else {
            return Err(invalid("state is outside the tree graph"));
        };
        *final_flag = flag;
        *weight = graph.final_weights[index];
        Ok(())
    })
}

/// Read one rule descriptor. Its output pattern can be copied separately.
///
/// # Safety
///
/// `graph` must be live and `out_rule` a writable disjoint slot.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_rule_info(
    graph: *const LlingTreeTransducer,
    index: usize,
    out_rule: *mut LlingTreeRule,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_rule, "out_rule")?;
        if graph.is_null() {
            set_error("tree graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        *output = *unsafe { &*graph }
            .rules
            .get(index)
            .ok_or_else(|| invalid("rule index is outside the tree graph"))?;
        Ok(())
    })
}

/// Copy one rule's preorder output pattern into a caller buffer.
///
/// # Safety
///
/// `graph` must be live; `out_nodes` must contain `capacity` writable nodes
/// and be disjoint from the graph. A zero capacity may use a null pointer.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_rule_pattern_copy(
    graph: *const LlingTreeTransducer,
    index: usize,
    out_nodes: *mut LlingTreeNode,
    capacity: usize,
) -> LlingLlangStatus {
    boundary(|| {
        if graph.is_null() {
            set_error("tree graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let graph = unsafe { &*graph };
        let rule = graph
            .rules
            .get(index)
            .ok_or_else(|| invalid("rule index is outside the tree graph"))?;
        let offset = bounded_usize(rule.pattern_offset, "pattern offset")?;
        let length = bounded_usize(rule.pattern_len, "pattern length")?;
        if capacity < length {
            set_error("rule pattern buffer is too small");
            return Err(LlingLlangStatus::LimitExceeded);
        }
        if out_nodes.is_null() {
            set_error("rule pattern output buffer is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                graph.patterns[offset..offset + length].as_ptr(),
                out_nodes,
                length,
            );
        }
        Ok(())
    })
}

/// Evaluate a native tree transducer into a finite owned output collection.
///
/// `state == NO_STATE` selects the graph's start state. Other values select a
/// checked state. Budget states bound alternatives, arcs bound input and
/// materialized output nodes, bytes bound logical payload, and work bounds
/// input decoding plus native machine transitions. All axes must be finite.
///
/// # Safety
///
/// `graph` must be live; `input` must contain `num_nodes` readable preorder
/// nodes; `budget` must be readable; `out_results` must be a writable disjoint
/// slot. A zero input count may use a null pointer but is invalid as a tree.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_transduce(
    graph: *const LlingTreeTransducer,
    input: *const LlingTreeNode,
    num_nodes: u64,
    state: u64,
    budget: *const LlingBudgetV2,
    out_results: *mut *mut LlingTreeResults,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_results, "out_results")?;
        if graph.is_null() {
            set_error("tree graph is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let count = bounded_usize(num_nodes, "input tree nodes")?;
        checked_slice_len::<LlingTreeNode>(count, "input tree nodes")?;
        if count == 0 {
            return Err(invalid("an input tree needs one root node"));
        }
        if input.is_null() {
            set_error("input tree nodes are null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let (budget, _) = tree_budget(budget)?;
        let nodes = unsafe { std::slice::from_raw_parts(input, count) };
        let results = unsafe { &*graph }.inner.transduce(nodes, state, budget)?;
        *output = Box::into_raw(Box::new(LlingTreeResults { inner: results }));
        Ok(())
    })
}

/// Return the number of weighted output trees in a result collection.
///
/// # Safety
///
/// `results` must be live and `out_count` a writable disjoint slot.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_results_info(
    results: *const LlingTreeResults,
    out_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_count, "out_count")?;
        if results.is_null() {
            set_error("tree results are null");
            return Err(LlingLlangStatus::NullPointer);
        }
        *output = unsafe { &*results }.inner.len();
        Ok(())
    })
}

/// Return one output tree's node count and weight.
///
/// # Safety
///
/// `results` must be live; output slots must be writable and disjoint.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_result_info(
    results: *const LlingTreeResults,
    index: usize,
    out_nodes: *mut usize,
    out_weight: *mut f64,
) -> LlingLlangStatus {
    boundary(|| {
        let nodes = required_mut(out_nodes, "out_nodes")?;
        let weight = required_mut(out_weight, "out_weight")?;
        if results.is_null() {
            set_error("tree results are null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let (count, scalar_weight) = unsafe { &*results }.inner.info(index)?;
        *nodes = count;
        *weight = scalar_weight;
        Ok(())
    })
}

/// Copy one output tree as preorder symbol nodes.
///
/// # Safety
///
/// `results` must be live; `out_nodes` must contain `capacity` writable nodes
/// disjoint from the result collection.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_result_copy(
    results: *const LlingTreeResults,
    index: usize,
    out_nodes: *mut LlingTreeNode,
    capacity: usize,
) -> LlingLlangStatus {
    boundary(|| {
        if results.is_null() {
            set_error("tree results are null");
            return Err(LlingLlangStatus::NullPointer);
        }
        if capacity != 0 && out_nodes.is_null() {
            set_error("tree result output buffer is null");
            return Err(LlingLlangStatus::NullPointer);
        }
        let output = if capacity == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(out_nodes, capacity) }
        };
        unsafe { &*results }.inner.copy(index, output)
    })
}

/// Release a captured immutable tree transducer; null is a no-op.
///
/// # Safety
///
/// A non-null pointer must be live and uniquely owned.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_free(graph: *mut LlingTreeTransducer) {
    if !graph.is_null() {
        drop(unsafe { Box::from_raw(graph) });
    }
}

/// Release a bounded tree result collection; null is a no-op.
///
/// # Safety
///
/// A non-null pointer must be live and uniquely owned.
#[no_mangle]
pub unsafe extern "C" fn lling_tree_results_free(results: *mut LlingTreeResults) {
    if !results.is_null() {
        drop(unsafe { Box::from_raw(results) });
    }
}
