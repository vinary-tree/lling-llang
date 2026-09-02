//! Optimal, stack-safe, certified strong bisimulation for finite labelled
//! transition systems.
//!
//! The implementation specializes Valmari's relational-coarsest-partition
//! algorithm to lling-llang's `u32` action alphabet. It validates the complete
//! input before indexing, canonicalizes set-valued transitions with fixed-width
//! radix passes, and refines dense state and transition partitions through
//! explicit heap worklists. No operation is recursive in input depth.
//!
//! Every physical all/some/none split carries the modal predicate proved safe
//! in `proofs/coq/algorithms/StrongBisimulation.v`. The result therefore owns a
//! replayable input-bound certificate, one characteristic formula per final
//! block, and actual smaller-half resource counters.

mod partition;
mod replay;
mod validated;

use std::fmt;

use partition::{PartitionSplit, RefinablePartition};
use rustc_hash::FxHashMap;
use validated::ValidatedLts;

/// An observable action label.
pub type Action = u32;

/// A finite labelled transition system over states `0..num_states`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Lts {
    /// Number of states.
    pub num_states: usize,
    /// Set-valued labelled transitions `(source, action, target)`.
    pub transitions: Vec<(usize, Action, usize)>,
}

impl Lts {
    /// Build an LTS. Validation occurs at the certified-analysis boundary.
    pub fn new(num_states: usize, transitions: Vec<(usize, Action, usize)>) -> Self {
        Self {
            num_states,
            transitions,
        }
    }

    /// Fallibly compute the canonical coarsest strong-bisimulation partition.
    pub fn try_bisimulation(
        &self,
        initial_colors: &[usize],
    ) -> Result<Vec<usize>, BisimulationError> {
        Ok(CertifiedBisimulation::compute(self, initial_colors)?.blocks)
    }

    /// Compute the canonical coarsest strong-bisimulation partition.
    ///
    /// This compatibility surface preserves the historical return type. New
    /// callers handling untrusted inputs should use [`Self::try_bisimulation`]
    /// or [`CertifiedBisimulation::compute`] to receive typed validation errors.
    pub fn bisimulation(&self, initial_colors: &[usize]) -> Vec<usize> {
        self.try_bisimulation(initial_colors)
            .expect("LTS and initial coloring must be valid")
    }

    /// Verify that a supplied partition is a colored strong bisimulation.
    /// Malformed endpoints, colors, or partition vectors return `false`.
    pub fn is_bisimulation(&self, block_of: &[usize], colors: &[usize]) -> bool {
        let Ok(validated) = ValidatedLts::build(self, colors) else {
            return false;
        };
        stable_partition(&validated, block_of, colors).unwrap_or(false)
    }

    /// Fallibly query strong bisimilarity of two states.
    pub fn try_bisimilar(
        &self,
        left: usize,
        right: usize,
        initial_colors: &[usize],
    ) -> Result<bool, BisimulationError> {
        if left >= self.num_states {
            return Err(BisimulationError::InvalidQuery {
                state: left,
                states: self.num_states,
            });
        }
        if right >= self.num_states {
            return Err(BisimulationError::InvalidQuery {
                state: right,
                states: self.num_states,
            });
        }
        let blocks = self.try_bisimulation(initial_colors)?;
        Ok(blocks[left] == blocks[right])
    }

    /// Compatibility query for already validated state indices.
    pub fn bisimilar(&self, left: usize, right: usize, initial_colors: &[usize]) -> bool {
        self.try_bisimilar(left, right, initial_colors)
            .expect("LTS, colors, and queried states must be valid")
    }
}

/// Total failure modes for certified strong-bisimulation analysis and evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BisimulationError {
    /// The observation vector does not cover the state carrier exactly.
    ColorCount { states: usize, colors: usize },
    /// A transition source lies outside the state carrier.
    InvalidSource {
        transition: usize,
        source: usize,
        states: usize,
    },
    /// A transition target lies outside the state carrier.
    InvalidTarget {
        transition: usize,
        target: usize,
        states: usize,
    },
    /// A state query lies outside the state carrier.
    InvalidQuery { state: usize, states: usize },
    /// A partition does not assign exactly one block to every state.
    PartitionCount { states: usize, blocks: usize },
    /// Checked size or work accounting overflowed `usize`.
    ArithmeticOverflow { context: &'static str },
    /// A fallible heap reservation failed.
    AllocationFailed { context: &'static str },
    /// Evidence was presented for a different canonical input.
    EvidenceInputMismatch,
    /// Certificate replay diverged from its bound canonical refinement trace.
    InvalidCertificate { context: &'static str },
    /// A modal DAG contains an invalid or cyclic reference.
    InvalidWitness { node: usize },
    /// A supposedly unreachable internal invariant was violated.
    InternalInvariant { context: &'static str },
}

impl fmt::Display for BisimulationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ColorCount { states, colors } => write!(
                formatter,
                "initial coloring has {colors} entries for a {states}-state LTS"
            ),
            Self::InvalidSource {
                transition,
                source,
                states,
            } => write!(
                formatter,
                "transition {transition} has source {source} outside 0..{states}"
            ),
            Self::InvalidTarget {
                transition,
                target,
                states,
            } => write!(
                formatter,
                "transition {transition} has target {target} outside 0..{states}"
            ),
            Self::InvalidQuery { state, states } => {
                write!(formatter, "queried state {state} is outside 0..{states}")
            }
            Self::PartitionCount { states, blocks } => write!(
                formatter,
                "partition has {blocks} entries for a {states}-state LTS"
            ),
            Self::ArithmeticOverflow { context } => {
                write!(
                    formatter,
                    "arithmetic overflow while constructing {context}"
                )
            }
            Self::AllocationFailed { context } => {
                write!(formatter, "allocation failed while constructing {context}")
            }
            Self::EvidenceInputMismatch => {
                formatter.write_str("evidence is bound to a different canonical input")
            }
            Self::InvalidCertificate { context } => {
                write!(formatter, "certificate replay failed: {context}")
            }
            Self::InvalidWitness { node } => {
                write!(
                    formatter,
                    "modal witness contains an invalid reference at node {node}"
                )
            }
            Self::InternalInvariant { context } => {
                write!(
                    formatter,
                    "certified bisimulation invariant failed: {context}"
                )
            }
        }
    }
}

impl std::error::Error for BisimulationError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum FormulaNode {
    Color(usize),
    And(usize, usize),
    Not(usize),
    Diamond(Action, usize),
}

#[derive(Debug, Default)]
struct FormulaBuilder {
    nodes: Vec<FormulaNode>,
    interned: FxHashMap<FormulaNode, usize>,
}

impl FormulaBuilder {
    fn with_capacity(capacity: usize) -> Result<Self, BisimulationError> {
        let mut nodes = Vec::new();
        nodes
            .try_reserve(capacity)
            .map_err(|_| BisimulationError::AllocationFailed {
                context: "modal formula DAG",
            })?;
        let mut interned = FxHashMap::default();
        interned
            .try_reserve(capacity)
            .map_err(|_| BisimulationError::AllocationFailed {
                context: "modal formula interner",
            })?;
        Ok(Self { nodes, interned })
    }

    fn intern(&mut self, node: FormulaNode) -> Result<usize, BisimulationError> {
        if let Some(&existing) = self.interned.get(&node) {
            return Ok(existing);
        }
        self.nodes
            .try_reserve(1)
            .map_err(|_| BisimulationError::AllocationFailed {
                context: "modal formula DAG node",
            })?;
        self.interned
            .try_reserve(1)
            .map_err(|_| BisimulationError::AllocationFailed {
                context: "modal formula interner entry",
            })?;
        let id = self.nodes.len();
        self.nodes.push(node);
        self.interned.insert(node, id);
        Ok(id)
    }

    fn color(&mut self, color: usize) -> Result<usize, BisimulationError> {
        self.intern(FormulaNode::Color(color))
    }

    fn and(&mut self, left: usize, right: usize) -> Result<usize, BisimulationError> {
        self.intern(FormulaNode::And(left, right))
    }

    fn not(&mut self, inner: usize) -> Result<usize, BisimulationError> {
        self.intern(FormulaNode::Not(inner))
    }

    fn diamond(&mut self, action: Action, inner: usize) -> Result<usize, BisimulationError> {
        self.intern(FormulaNode::Diamond(action, inner))
    }

    fn truth(&mut self, seed: usize) -> Result<usize, BisimulationError> {
        let negated_seed = self.not(seed)?;
        let contradiction = self.and(seed, negated_seed)?;
        self.not(contradiction)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SplitPhase {
    Confined,
    Reaches,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CertifiedSplit {
    phase: SplitPhase,
    action: Action,
    target_formula: usize,
    predicate_formula: usize,
    parent_representative: usize,
    new_is_marked: bool,
    new_members: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SplitContext {
    phase: SplitPhase,
    action: Action,
    target_formula: usize,
}

#[derive(Debug, Default)]
struct NativeFrameAccount {
    active_input_frames: usize,
    maximum_input_frames: usize,
}

impl NativeFrameAccount {
    fn enter_input_driver(&mut self) -> Result<(), BisimulationError> {
        self.active_input_frames = self.active_input_frames.checked_add(1).ok_or(
            BisimulationError::ArithmeticOverflow {
                context: "input-shaped native frame account",
            },
        )?;
        self.maximum_input_frames = self.maximum_input_frames.max(self.active_input_frames);
        Ok(())
    }

    fn leave_input_driver(&mut self) -> Result<(), BisimulationError> {
        self.active_input_frames = self.active_input_frames.checked_sub(1).ok_or(
            BisimulationError::InternalInvariant {
                context: "input-shaped native frame account underflowed",
            },
        )?;
        Ok(())
    }
}

/// Replayable evidence binding the canonical input, every modal-safe split,
/// the characteristic formula DAG, and the final canonical partition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BisimulationCertificate {
    input_digest: [u8; 32],
    initial_blocks: Vec<usize>,
    initial_formula_for_block: Vec<usize>,
    trace: Vec<CertifiedSplit>,
    formulas: Vec<FormulaNode>,
    formula_for_block: Vec<usize>,
    canonical_blocks: Vec<usize>,
}

impl BisimulationCertificate {
    /// Replay deterministic modal-safe refinement and check final stability.
    pub fn replay(&self, lts: &Lts, colors: &[usize]) -> Result<Vec<usize>, BisimulationError> {
        let validated = ValidatedLts::build(lts, colors)?;
        replay::replay_certificate(self, &validated, colors)
    }

    /// Canonical digest of state count, deduplicated transitions, and colors.
    pub fn input_digest(&self) -> &[u8; 32] {
        &self.input_digest
    }

    /// Number of non-trivial modal-safe physical splits.
    pub fn split_count(&self) -> usize {
        self.trace.len()
    }
}

/// Actual accounting from the refinement core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BisimulationResources {
    state_charge_counts: Vec<usize>,
    transition_charge_counts: Vec<usize>,
    whole_partition_rescans: usize,
    maximum_native_frames: usize,
    state_partition_cells: usize,
    transition_partition_cells: usize,
    witness_dag_cells: usize,
}

impl BisimulationResources {
    /// Sum of real state and transition smaller-half charge events.
    pub fn charged_work(&self) -> usize {
        self.state_charge_counts.iter().copied().sum::<usize>()
            + self.transition_charge_counts.iter().copied().sum::<usize>()
    }

    /// Allocated element slots in the two refinable partition cores.
    pub fn core_heap_cells(&self) -> usize {
        self.state_partition_cells + self.transition_partition_cells
    }

    /// Whole-partition rescans performed inside refinement.
    pub fn whole_partition_rescans(&self) -> usize {
        self.whole_partition_rescans
    }

    /// Maximum input-shaped native frames used by refinement and evidence.
    pub fn maximum_native_frames(&self) -> usize {
        self.maximum_native_frames
    }

    /// Shared modal formula nodes retained by the certificate.
    pub fn witness_dag_cells(&self) -> usize {
        self.witness_dag_cells
    }

    /// Per-state smaller-half charges.
    pub fn state_charge_counts(&self) -> &[usize] {
        &self.state_charge_counts
    }

    /// Per-transition charges induced by smaller target-state blocks.
    pub fn transition_charge_counts(&self) -> &[usize] {
        &self.transition_charge_counts
    }
}

/// Exact certified strong-bisimulation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertifiedBisimulation {
    blocks: Vec<usize>,
    certificate: BisimulationCertificate,
    resources: BisimulationResources,
}

impl CertifiedBisimulation {
    /// Validate, canonicalize, refine, certify, and independently check an LTS.
    pub fn compute(lts: &Lts, colors: &[usize]) -> Result<Self, BisimulationError> {
        let validated = ValidatedLts::build(lts, colors)?;
        compute_validated(&validated, colors)
    }

    /// One canonical block identifier per state.
    pub fn blocks(&self) -> &[usize] {
        &self.blocks
    }

    /// Fallibly materialize the optional quadratic relation view.
    pub fn try_relation_matrix(&self) -> Result<Vec<bool>, BisimulationError> {
        let cells = self.blocks.len().checked_mul(self.blocks.len()).ok_or(
            BisimulationError::ArithmeticOverflow {
                context: "bisimulation relation matrix",
            },
        )?;
        let mut relation = reserved_vec(cells, "bisimulation relation matrix")?;
        for &left in &self.blocks {
            for &right in &self.blocks {
                relation.push(left == right);
            }
        }
        Ok(relation)
    }

    /// Materialize the optional quadratic relation view.
    ///
    /// Call [`Self::try_relation_matrix`] when allocation failure must remain a
    /// recoverable outcome.
    pub fn relation_matrix(&self) -> Vec<bool> {
        self.try_relation_matrix()
            .expect("relation matrix dimensions and allocation must be representable")
    }

    /// Replayable exactness evidence.
    pub fn certificate(&self) -> &BisimulationCertificate {
        &self.certificate
    }

    /// Actual work, heap, rescan, witness, and native-frame account.
    pub fn resources(&self) -> &BisimulationResources {
        &self.resources
    }

    /// Total query returning an oriented witness for a separated pair.
    pub fn try_witness(
        &self,
        left: usize,
        right: usize,
    ) -> Result<Option<DistinguishingWitness<'_>>, BisimulationError> {
        if left >= self.blocks.len() {
            return Err(BisimulationError::InvalidQuery {
                state: left,
                states: self.blocks.len(),
            });
        }
        if right >= self.blocks.len() {
            return Err(BisimulationError::InvalidQuery {
                state: right,
                states: self.blocks.len(),
            });
        }
        if self.blocks[left] == self.blocks[right] {
            return Ok(None);
        }

        let (root, negated) = if self.certificate.initial_blocks[left]
            != self.certificate.initial_blocks[right]
        {
            (
                self.certificate.initial_formula_for_block[self.certificate.initial_blocks[left]],
                false,
            )
        } else {
            let separating_split = self
                .certificate
                .trace
                .iter()
                .find_map(|split| {
                    let left_in_new = split.new_members.contains(&left);
                    let right_in_new = split.new_members.contains(&right);
                    (left_in_new != right_in_new).then_some((split, left_in_new))
                })
                .ok_or(BisimulationError::InternalInvariant {
                    context: "separated pair has no separating certificate split",
                })?;
            let (split, left_in_new) = separating_split;
            let left_satisfies = if left_in_new {
                split.new_is_marked
            } else {
                !split.new_is_marked
            };
            (split.predicate_formula, !left_satisfies)
        };
        Ok(Some(DistinguishingWitness {
            formulas: &self.certificate.formulas,
            root,
            negated,
            input_digest: self.certificate.input_digest,
        }))
    }

    /// Compatibility witness query for already validated state indices.
    pub fn witness(&self, left: usize, right: usize) -> Option<DistinguishingWitness<'_>> {
        self.try_witness(left, right).ok().flatten()
    }
}

/// An oriented shared-DAG modal witness.
#[derive(Clone, Copy, Debug)]
pub struct DistinguishingWitness<'a> {
    formulas: &'a [FormulaNode],
    root: usize,
    negated: bool,
    input_digest: [u8; 32],
}

impl DistinguishingWitness<'_> {
    /// Evaluate the witness through an explicit pushdown machine.
    pub fn evaluate(
        &self,
        lts: &Lts,
        colors: &[usize],
        state: usize,
    ) -> Result<bool, BisimulationError> {
        let validated = ValidatedLts::build(lts, colors)?;
        if validated.input_digest != self.input_digest {
            return Err(BisimulationError::EvidenceInputMismatch);
        }
        if state >= validated.state_count {
            return Err(BisimulationError::InvalidQuery {
                state,
                states: validated.state_count,
            });
        }
        let holds = evaluate_formula(self.formulas, self.root, &validated, colors, state)?;
        Ok(if self.negated { !holds } else { holds })
    }
}

fn compute_validated(
    validated: &ValidatedLts,
    colors: &[usize],
) -> Result<CertifiedBisimulation, BisimulationError> {
    let mut native_frames = NativeFrameAccount::default();
    native_frames.enter_input_driver()?;
    let state_count = validated.state_count;
    let transition_count = validated.transition_count();
    let mut state_partition = RefinablePartition::from_classes(colors)?;
    let initial_state_blocks = state_partition.block_count();
    let transition_classes = validated.transition_classes(state_partition.block_map())?;
    let mut transition_partition = RefinablePartition::from_classes(&transition_classes)?;

    let formula_capacity =
        state_count
            .checked_add(transition_count)
            .ok_or(BisimulationError::ArithmeticOverflow {
                context: "modal formula capacity",
            })?;
    let mut formula_builder = FormulaBuilder::with_capacity(formula_capacity)?;
    let mut formula_of_block = reserved_vec(state_count, "block characteristic formulas")?;
    for block in 0..initial_state_blocks {
        let representative = state_partition.members(block)[0];
        formula_of_block.push(formula_builder.color(colors[representative])?);
    }
    let mut initial_blocks = reserved_vec(state_count, "initial certificate partition")?;
    initial_blocks.extend_from_slice(state_partition.block_map());
    let mut initial_formula_for_block =
        reserved_vec(initial_state_blocks, "initial certificate formulas")?;
    initial_formula_for_block.extend_from_slice(&formula_of_block);

    let mut state_charges = filled_vec(state_count, 0usize, "state charge counters")?;
    let mut transition_charges =
        filled_vec(transition_count, 0usize, "transition charge counters")?;
    let mut source_counts = filled_vec(state_count, 0usize, "source counters")?;
    let mut source_new_group = filled_vec(state_count, usize::MAX, "source group scratch")?;
    let mut source_predicate = filled_vec(state_count, usize::MAX, "source predicate scratch")?;
    let mut link = filled_vec(transition_count, 0usize, "transition source-label links")?;
    let mut label_counts: Vec<usize> = reserved_vec(transition_count, "source-label counters")?;
    let mut label_guards: Vec<usize> = reserved_vec(transition_count, "source-label guards")?;
    let truth_formula = if transition_count == 0 {
        None
    } else {
        Some(formula_builder.truth(formula_of_block[0])?)
    };

    let mut previous_group = None;
    for (transition, edge) in validated.edges.iter().enumerate() {
        let key = (edge.dense_action, edge.source);
        if previous_group != Some(key) {
            label_counts.push(0);
            label_guards.push(truth_formula.ok_or(BisimulationError::InternalInvariant {
                context: "non-empty transition carrier has no truth formula",
            })?);
            previous_group = Some(key);
        }
        let group = label_counts.len() - 1;
        link[transition] = group;
        label_counts[group] =
            label_counts[group]
                .checked_add(1)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "source-label transition count",
                })?;
    }

    let mut trace = reserved_vec(state_count, "certified split trace")?;
    let mut state_splits = Vec::new();
    let mut transition_splits = Vec::new();
    let mut current_cluster = 0usize;
    let mut current_state_block = initial_state_blocks;

    while current_cluster < transition_partition.block_count() {
        let cluster = transition_partition.members(current_cluster);
        let first_transition = *cluster
            .first()
            .ok_or(BisimulationError::InternalInvariant {
                context: "transition partition contains an empty cluster",
            })?;
        let action = validated.edges[first_transition].action;
        let target_block = state_partition.block_of(validated.edges[first_transition].target);
        for &transition in cluster {
            let edge = validated.edges[transition];
            if edge.action != action || state_partition.block_of(edge.target) != target_block {
                return Err(BisimulationError::InternalInvariant {
                    context: "transition cluster is not action/target-block homogeneous",
                });
            }
            source_counts[edge.source] = source_counts[edge.source].checked_add(1).ok_or(
                BisimulationError::ArithmeticOverflow {
                    context: "transition cluster source count",
                },
            )?;
        }

        let target_formula = formula_of_block[target_block];

        // First physical split: sources whose complete current source-label
        // subgroup is confined to the selected part of its modal guard.
        for &transition in cluster {
            let source = validated.edges[transition].source;
            let count = source_counts[source];
            let group = link[transition];
            if count != 0 && count == label_counts[group] {
                let guard = label_guards[group];
                let outside_target = formula_builder.not(target_formula)?;
                let selected = formula_builder.and(guard, target_formula)?;
                let guarded_remainder = formula_builder.and(guard, outside_target)?;
                let reaches = formula_builder.diamond(action, selected)?;
                let reaches_remainder = formula_builder.diamond(action, guarded_remainder)?;
                let misses_remainder = formula_builder.not(reaches_remainder)?;
                source_predicate[source] = formula_builder.and(reaches, misses_remainder)?;
                label_guards[group] = selected;
                state_partition.mark(source);
                source_counts[source] = 0;
            }
        }
        state_partition.split_touched(Some(&mut state_charges), &mut state_splits)?;

        if !state_splits.is_empty() {
            record_splits(
                &state_partition,
                &mut formula_builder,
                &mut formula_of_block,
                &state_splits,
                SplitContext {
                    phase: SplitPhase::Confined,
                    action,
                    target_formula,
                },
                &source_predicate,
                &mut trace,
            )?;
        }

        // Second physical split: partial sources versus non-reaching sources;
        // simultaneously refine the source-label counters for later clusters.
        for &transition in cluster {
            let source = validated.edges[transition].source;
            if source_new_group[source] != usize::MAX {
                link[transition] = source_new_group[source];
                continue;
            }
            let count = source_counts[source];
            if count == 0 {
                continue;
            }
            let old_group = link[transition];
            if count >= label_counts[old_group] {
                return Err(BisimulationError::InternalInvariant {
                    context: "partial source-label group is not strict",
                });
            }
            let guard = label_guards[old_group];
            let outside_target = formula_builder.not(target_formula)?;
            let selected = formula_builder.and(guard, target_formula)?;
            let guarded_remainder = formula_builder.and(guard, outside_target)?;
            source_predicate[source] = formula_builder.diamond(action, selected)?;
            state_partition.mark(source);
            let new_group = label_counts.len();
            label_counts.push(count);
            label_guards.push(selected);
            label_counts[old_group] -= count;
            label_guards[old_group] = guarded_remainder;
            source_new_group[source] = new_group;
            link[transition] = new_group;
        }
        for &transition in cluster {
            let source = validated.edges[transition].source;
            source_counts[source] = 0;
            source_new_group[source] = usize::MAX;
        }
        state_partition.split_touched(Some(&mut state_charges), &mut state_splits)?;
        if !state_splits.is_empty() {
            record_splits(
                &state_partition,
                &mut formula_builder,
                &mut formula_of_block,
                &state_splits,
                SplitContext {
                    phase: SplitPhase::Reaches,
                    action,
                    target_formula,
                },
                &source_predicate,
                &mut trace,
            )?;
        }
        for &transition in cluster {
            source_predicate[validated.edges[transition].source] = usize::MAX;
        }

        current_cluster += 1;
        while current_state_block < state_partition.block_count() {
            for &state in state_partition.members(current_state_block) {
                for &transition in validated.reverse.edge_ids(state) {
                    transition_partition.mark(transition);
                    transition_charges[transition] = transition_charges[transition]
                        .checked_add(1)
                        .ok_or(BisimulationError::ArithmeticOverflow {
                            context: "transition smaller-target charge",
                        })?;
                }
            }
            transition_partition.split_touched(None, &mut transition_splits)?;
            current_state_block += 1;
        }
    }

    let (blocks, formula_for_block) = canonicalize_blocks(&state_partition, &formula_of_block)?;
    let state_partition_cells = state_partition.heap_cells();
    let transition_partition_cells = transition_partition.heap_cells();
    let formulas = formula_builder.nodes;
    let certificate = BisimulationCertificate {
        input_digest: validated.input_digest,
        initial_blocks,
        initial_formula_for_block,
        trace,
        formulas,
        formula_for_block,
        canonical_blocks: blocks.clone(),
    };
    let replayed = replay::replay_certificate(&certificate, validated, colors)?;
    if replayed != blocks {
        return Err(BisimulationError::InternalInvariant {
            context: "independent certificate replay changed the canonical partition",
        });
    }
    native_frames.leave_input_driver()?;
    let resources = BisimulationResources {
        state_charge_counts: state_charges,
        transition_charge_counts: transition_charges,
        whole_partition_rescans: 0,
        maximum_native_frames: native_frames.maximum_input_frames,
        state_partition_cells,
        transition_partition_cells,
        witness_dag_cells: certificate.formulas.len(),
    };

    Ok(CertifiedBisimulation {
        blocks,
        certificate,
        resources,
    })
}

fn record_splits(
    partition: &RefinablePartition,
    formulas: &mut FormulaBuilder,
    formula_of_block: &mut Vec<usize>,
    splits: &[PartitionSplit],
    context: SplitContext,
    predicate_for_source: &[usize],
    trace: &mut Vec<CertifiedSplit>,
) -> Result<(), BisimulationError> {
    trace
        .try_reserve(splits.len())
        .map_err(|_| BisimulationError::AllocationFailed {
            context: "certified split trace",
        })?;
    formula_of_block.try_reserve(splits.len()).map_err(|_| {
        BisimulationError::AllocationFailed {
            context: "block characteristic formulas",
        }
    })?;

    for split in splits {
        let predicate_formula = predicate_for_source[split.parent_representative];
        if predicate_formula == usize::MAX {
            return Err(BisimulationError::InternalInvariant {
                context: "marked split has no guarded modal predicate",
            });
        }
        let negated_predicate = formulas.not(predicate_formula)?;
        let parent_formula = formula_of_block[split.old_block];
        let marked_formula = formulas.and(parent_formula, predicate_formula)?;
        let unmarked_formula = formulas.and(parent_formula, negated_predicate)?;
        let (old_formula, new_formula) = if split.new_is_marked {
            (unmarked_formula, marked_formula)
        } else {
            (marked_formula, unmarked_formula)
        };
        formula_of_block[split.old_block] = old_formula;
        if split.new_block != formula_of_block.len() {
            return Err(BisimulationError::InternalInvariant {
                context: "fresh state block IDs are not contiguous",
            });
        }
        formula_of_block.push(new_formula);

        let members = partition.members(split.new_block);
        let mut new_members = reserved_vec(members.len(), "certificate child membership")?;
        new_members.extend_from_slice(members);
        trace.push(CertifiedSplit {
            phase: context.phase,
            action: context.action,
            target_formula: context.target_formula,
            predicate_formula,
            parent_representative: split.parent_representative,
            new_is_marked: split.new_is_marked,
            new_members,
        });
    }
    Ok(())
}

fn canonicalize_blocks(
    partition: &RefinablePartition,
    internal_formulas: &[usize],
) -> Result<(Vec<usize>, Vec<usize>), BisimulationError> {
    let state_count = partition.block_map().len();
    let mut internal_to_canonical =
        filled_vec(partition.block_count(), usize::MAX, "canonical block map")?;
    let mut blocks = filled_vec(state_count, 0usize, "canonical partition")?;
    let mut formula_for_block = reserved_vec(partition.block_count(), "canonical formulas")?;

    for state in 0..state_count {
        let internal = partition.block_of(state);
        if internal_to_canonical[internal] == usize::MAX {
            internal_to_canonical[internal] = formula_for_block.len();
            formula_for_block.push(internal_formulas[internal]);
        }
        blocks[state] = internal_to_canonical[internal];
    }
    Ok((blocks, formula_for_block))
}

fn stable_partition(
    validated: &ValidatedLts,
    block_of: &[usize],
    colors: &[usize],
) -> Result<bool, BisimulationError> {
    if block_of.len() != validated.state_count {
        return Err(BisimulationError::PartitionCount {
            states: validated.state_count,
            blocks: block_of.len(),
        });
    }
    let (signatures, offsets) = canonical_signatures(validated, block_of)?;
    let blocks = RefinablePartition::from_classes(block_of)?;
    for block in 0..blocks.block_count() {
        let members = blocks.members(block);
        let representative = members[0];
        let expected = &signatures[offsets[representative]..offsets[representative + 1]];
        for &state in &members[1..] {
            let actual = &signatures[offsets[state]..offsets[state + 1]];
            if colors[state] != colors[representative]
                || !same_observable_signature(actual, expected)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[inline]
fn same_observable_signature(left: &[SignatureEntry], right: &[SignatureEntry]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left_entry, right_entry)| {
            left_entry.dense_action == right_entry.dense_action
                && left_entry.target_block == right_entry.target_block
        })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SignatureEntry {
    source: usize,
    dense_action: usize,
    target_block: usize,
}

fn canonical_signatures(
    validated: &ValidatedLts,
    block_of: &[usize],
) -> Result<(Vec<SignatureEntry>, Vec<usize>), BisimulationError> {
    let mut signatures = reserved_vec(validated.transition_count(), "stability signatures")?;
    for edge in &validated.edges {
        signatures.push(SignatureEntry {
            source: edge.source,
            dense_action: edge.dense_action,
            target_block: block_of[edge.target],
        });
    }
    radix_sort_signatures(&mut signatures)?;
    signatures.dedup();

    let offset_count =
        validated
            .state_count
            .checked_add(1)
            .ok_or(BisimulationError::ArithmeticOverflow {
                context: "stability signature offsets",
            })?;
    let mut offsets = filled_vec(offset_count, 0usize, "stability signature offsets")?;
    for signature in &signatures {
        let slot =
            signature
                .source
                .checked_add(1)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "stability signature source offset",
                })?;
        offsets[slot] =
            offsets[slot]
                .checked_add(1)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "stability signature count",
                })?;
    }
    for state in 0..validated.state_count {
        offsets[state + 1] = offsets[state + 1].checked_add(offsets[state]).ok_or(
            BisimulationError::ArithmeticOverflow {
                context: "stability signature prefix sum",
            },
        )?;
    }
    Ok((signatures, offsets))
}

fn radix_sort_signatures(signatures: &mut Vec<SignatureEntry>) -> Result<(), BisimulationError> {
    if signatures.len() < 2 {
        return Ok(());
    }
    let mut scratch = filled_vec(
        signatures.len(),
        SignatureEntry::default(),
        "stability signature radix scratch",
    )?;
    let bytes = core::mem::size_of::<usize>();
    for pass in 0..bytes {
        radix_signature_pass(signatures, &mut scratch, |entry| {
            (entry.target_block >> (pass * 8)) & 0xff
        })?;
    }
    for pass in 0..bytes {
        radix_signature_pass(signatures, &mut scratch, |entry| {
            (entry.dense_action >> (pass * 8)) & 0xff
        })?;
    }
    for pass in 0..bytes {
        radix_signature_pass(signatures, &mut scratch, |entry| {
            (entry.source >> (pass * 8)) & 0xff
        })?;
    }
    Ok(())
}

fn radix_signature_pass(
    signatures: &mut Vec<SignatureEntry>,
    scratch: &mut Vec<SignatureEntry>,
    key: impl Fn(SignatureEntry) -> usize,
) -> Result<(), BisimulationError> {
    let mut counts = [0usize; 256];
    for &entry in signatures.iter() {
        let bucket = key(entry);
        counts[bucket] =
            counts[bucket]
                .checked_add(1)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "stability signature radix count",
                })?;
    }
    let mut total = 0usize;
    for count in &mut counts {
        let bucket = *count;
        *count = total;
        total = total
            .checked_add(bucket)
            .ok_or(BisimulationError::ArithmeticOverflow {
                context: "stability signature radix prefix sum",
            })?;
    }
    for &entry in signatures.iter() {
        let bucket = key(entry);
        scratch[counts[bucket]] = entry;
        counts[bucket] += 1;
    }
    core::mem::swap(signatures, scratch);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum EvalFrame {
    Enter {
        node: usize,
        state: usize,
    },
    ResolveNot {
        node: usize,
        state: usize,
        child: usize,
    },
    ResolveAnd {
        node: usize,
        state: usize,
        left: usize,
        right: usize,
    },
    ScanDiamond {
        node: usize,
        state: usize,
        action: Action,
        child: usize,
        cursor: usize,
    },
}

fn evaluate_formula(
    formulas: &[FormulaNode],
    root: usize,
    validated: &ValidatedLts,
    colors: &[usize],
    state: usize,
) -> Result<bool, BisimulationError> {
    if root >= formulas.len() {
        return Err(BisimulationError::InvalidWitness { node: root });
    }
    let mut values = FxHashMap::default();
    values
        .try_reserve(formulas.len().min(1024).max(1))
        .map_err(|_| BisimulationError::AllocationFailed {
            context: "witness PDA memo table",
        })?;
    let mut stack = reserved_vec(formulas.len().min(1024).max(1), "witness PDA stack")?;
    push_eval_frame(&mut stack, EvalFrame::Enter { node: root, state })?;

    while let Some(frame) = stack.pop() {
        match frame {
            EvalFrame::Enter { node, state } => {
                if values.contains_key(&(node, state)) {
                    continue;
                }
                let formula = *formulas
                    .get(node)
                    .ok_or(BisimulationError::InvalidWitness { node })?;
                match formula {
                    FormulaNode::Color(expected) => {
                        insert_formula_value(&mut values, node, state, colors[state] == expected)?;
                    }
                    FormulaNode::Not(child) => {
                        validate_child(node, child)?;
                        push_eval_frame(&mut stack, EvalFrame::ResolveNot { node, state, child })?;
                        push_eval_frame(&mut stack, EvalFrame::Enter { node: child, state })?;
                    }
                    FormulaNode::And(left, right) => {
                        validate_child(node, left)?;
                        validate_child(node, right)?;
                        push_eval_frame(
                            &mut stack,
                            EvalFrame::ResolveAnd {
                                node,
                                state,
                                left,
                                right,
                            },
                        )?;
                        push_eval_frame(&mut stack, EvalFrame::Enter { node: right, state })?;
                        push_eval_frame(&mut stack, EvalFrame::Enter { node: left, state })?;
                    }
                    FormulaNode::Diamond(action, child) => {
                        validate_child(node, child)?;
                        push_eval_frame(
                            &mut stack,
                            EvalFrame::ScanDiamond {
                                node,
                                state,
                                action,
                                child,
                                cursor: 0,
                            },
                        )?;
                    }
                }
            }
            EvalFrame::ResolveNot { node, state, child } => {
                let child_value = *values
                    .get(&(child, state))
                    .ok_or(BisimulationError::InvalidWitness { node: child })?;
                insert_formula_value(&mut values, node, state, !child_value)?;
            }
            EvalFrame::ResolveAnd {
                node,
                state,
                left,
                right,
            } => {
                let left_value = *values
                    .get(&(left, state))
                    .ok_or(BisimulationError::InvalidWitness { node: left })?;
                let right_value = *values
                    .get(&(right, state))
                    .ok_or(BisimulationError::InvalidWitness { node: right })?;
                insert_formula_value(&mut values, node, state, left_value && right_value)?;
            }
            EvalFrame::ScanDiamond {
                node,
                state,
                action,
                child,
                mut cursor,
            } => {
                let outgoing = validated.forward.edge_ids(state);
                let mut suspended = false;
                while cursor < outgoing.len() {
                    let transition_cursor = cursor;
                    let edge = validated.edges[outgoing[transition_cursor]];
                    cursor += 1;
                    if edge.action < action {
                        continue;
                    }
                    if edge.action > action {
                        break;
                    }
                    if let Some(&child_value) = values.get(&(child, edge.target)) {
                        if child_value {
                            insert_formula_value(&mut values, node, state, true)?;
                            suspended = true;
                            break;
                        }
                    } else {
                        push_eval_frame(
                            &mut stack,
                            EvalFrame::ScanDiamond {
                                node,
                                state,
                                action,
                                child,
                                cursor: transition_cursor,
                            },
                        )?;
                        push_eval_frame(
                            &mut stack,
                            EvalFrame::Enter {
                                node: child,
                                state: edge.target,
                            },
                        )?;
                        suspended = true;
                        break;
                    }
                }
                if !suspended {
                    insert_formula_value(&mut values, node, state, false)?;
                }
            }
        }
    }
    values
        .get(&(root, state))
        .copied()
        .ok_or(BisimulationError::InvalidWitness { node: root })
}

fn push_eval_frame(stack: &mut Vec<EvalFrame>, frame: EvalFrame) -> Result<(), BisimulationError> {
    stack
        .try_reserve(1)
        .map_err(|_| BisimulationError::AllocationFailed {
            context: "witness PDA stack frame",
        })?;
    stack.push(frame);
    Ok(())
}

fn insert_formula_value(
    values: &mut FxHashMap<(usize, usize), bool>,
    node: usize,
    state: usize,
    value: bool,
) -> Result<(), BisimulationError> {
    values
        .try_reserve(1)
        .map_err(|_| BisimulationError::AllocationFailed {
            context: "witness PDA memo entry",
        })?;
    values.insert((node, state), value);
    Ok(())
}

#[inline]
fn validate_child(parent: usize, child: usize) -> Result<(), BisimulationError> {
    if child >= parent {
        return Err(BisimulationError::InvalidWitness { node: parent });
    }
    Ok(())
}

fn reserved_vec<T>(capacity: usize, context: &'static str) -> Result<Vec<T>, BisimulationError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(capacity)
        .map_err(|_| BisimulationError::AllocationFailed { context })?;
    Ok(result)
}

fn filled_vec<T: Clone>(
    len: usize,
    value: T,
    context: &'static str,
) -> Result<Vec<T>, BisimulationError> {
    let mut result = reserved_vec(len, context)?;
    result.resize(len, value);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Action = 0;
    const B: Action = 1;
    const C: Action = 2;

    #[test]
    fn two_copies_of_a_b_are_bisimilar_and_certified() {
        let lts = Lts::new(6, vec![(0, A, 1), (1, B, 2), (3, A, 4), (4, B, 5)]);
        let colors = vec![0; 6];
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();
        assert_eq!(result.blocks[0], result.blocks[3]);
        assert_eq!(result.blocks[1], result.blocks[4]);
        assert_eq!(result.blocks[2], result.blocks[5]);
        assert!(lts.is_bisimulation(result.blocks(), &colors));
        assert_eq!(
            result.certificate().replay(&lts, &colors).unwrap(),
            result.blocks()
        );
    }

    #[test]
    fn branching_distinguishes_choice_placement_with_modal_witness() {
        let lts = Lts::new(
            9,
            vec![
                (0, A, 1),
                (1, B, 2),
                (1, C, 3),
                (4, A, 5),
                (4, A, 6),
                (5, B, 7),
                (6, C, 8),
            ],
        );
        let colors = vec![0; 9];
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();
        assert_ne!(result.blocks[0], result.blocks[4]);
        let witness = result.witness(0, 4).unwrap();
        assert_ne!(
            witness.evaluate(&lts, &colors, 0).unwrap(),
            witness.evaluate(&lts, &colors, 4).unwrap()
        );
    }

    #[test]
    fn initial_colors_and_self_loops_are_respected() {
        let colored = CertifiedBisimulation::compute(&Lts::new(2, vec![]), &[0, 1]).unwrap();
        assert_ne!(colored.blocks[0], colored.blocks[1]);

        let loops = Lts::new(3, vec![(0, A, 0), (1, A, 1), (2, B, 2)]);
        let result = CertifiedBisimulation::compute(&loops, &[0, 0, 0]).unwrap();
        assert_eq!(result.blocks[0], result.blocks[1]);
        assert_ne!(result.blocks[0], result.blocks[2]);
    }

    #[test]
    fn empty_lts_is_the_unique_empty_certificate() {
        let result = CertifiedBisimulation::compute(&Lts::new(0, vec![]), &[]).unwrap();
        assert!(result.blocks().is_empty());
        assert!(result.relation_matrix().is_empty());
        assert_eq!(result.resources.maximum_native_frames(), 1);
    }

    #[test]
    fn confinement_is_relative_to_the_source_label_subgroup() {
        let lts = Lts::new(
            10,
            vec![
                (2, 3, 7),
                (7, 0, 0),
                (3, 0, 0),
                (1, 3, 3),
                (1, 3, 1),
                (2, 3, 4),
            ],
        );
        let colors = vec![0; 10];
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();
        assert_ne!(result.blocks()[1], result.blocks()[2]);

        let validated = ValidatedLts::build(&lts, &colors).unwrap();
        for state in 0..lts.num_states {
            let block = result.blocks()[state];
            let root = result.certificate.formula_for_block[block];
            for candidate in 0..lts.num_states {
                assert_eq!(
                    evaluate_formula(
                        &result.certificate.formulas,
                        root,
                        &validated,
                        &colors,
                        candidate,
                    )
                    .unwrap(),
                    result.blocks()[candidate] == block,
                    "formula for block {block} disagrees at state {candidate}",
                );
            }
        }
    }

    #[test]
    fn independent_replay_rejects_each_mutated_evidence_layer() {
        let lts = Lts::new(4, vec![(0, A, 1), (1, B, 2), (3, A, 2)]);
        let colors = vec![0; 4];
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();
        assert!(!result.certificate.trace.is_empty());

        let mut wrong_input = result.certificate.clone();
        wrong_input.input_digest[0] ^= 1;
        assert!(matches!(
            wrong_input.replay(&lts, &colors),
            Err(BisimulationError::EvidenceInputMismatch)
        ));

        let mut wrong_split = result.certificate.clone();
        wrong_split.trace[0].new_members.push(lts.num_states);
        assert!(matches!(
            wrong_split.replay(&lts, &colors),
            Err(BisimulationError::InvalidCertificate { .. })
        ));

        let mut wrong_formula = result.certificate.clone();
        wrong_formula.formulas.pop();
        assert!(matches!(
            wrong_formula.replay(&lts, &colors),
            Err(BisimulationError::InvalidCertificate { .. })
        ));

        let mut wrong_partition = result.certificate.clone();
        wrong_partition.canonical_blocks[0] = usize::MAX;
        assert!(matches!(
            wrong_partition.replay(&lts, &colors),
            Err(BisimulationError::InvalidCertificate { .. })
        ));
    }
}
