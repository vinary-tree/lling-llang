//! Independent deterministic checker for certified Valmari refinement traces.

use super::partition::{PartitionSplit, RefinablePartition};
use super::validated::ValidatedLts;
use super::{
    canonicalize_blocks, filled_vec, reserved_vec, stable_partition, BisimulationCertificate,
    BisimulationError, CertifiedSplit, FormulaBuilder, SplitContext, SplitPhase,
};

/// Replay the producer's physical refinement trace through a separate driver.
///
/// This checker deliberately never calls `compute_validated`. It rebuilds the
/// two refinable partitions, source-label subgroup guards, modal DAG, and
/// canonical output, comparing every physical split before accepting final
/// stability.
pub(super) fn replay_certificate(
    certificate: &BisimulationCertificate,
    validated: &ValidatedLts,
    colors: &[usize],
) -> Result<Vec<usize>, BisimulationError> {
    if certificate.input_digest != validated.input_digest {
        return Err(BisimulationError::EvidenceInputMismatch);
    }

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
                context: "certificate replay formula capacity",
            })?;
    let mut formula_builder = FormulaBuilder::with_capacity(formula_capacity)?;
    let mut formula_of_block = reserved_vec(state_count, "replayed block formulas")?;
    for block in 0..initial_state_blocks {
        let representative = state_partition.members(block)[0];
        formula_of_block.push(formula_builder.color(colors[representative])?);
    }
    if state_partition.block_map() != certificate.initial_blocks {
        return Err(BisimulationError::InvalidCertificate {
            context: "initial color partition differs from deterministic replay",
        });
    }
    if formula_of_block != certificate.initial_formula_for_block {
        return Err(BisimulationError::InvalidCertificate {
            context: "initial color formulas differ from deterministic replay",
        });
    }

    let mut source_counts = filled_vec(state_count, 0usize, "replayed source counters")?;
    let mut source_new_group =
        filled_vec(state_count, usize::MAX, "replayed source group scratch")?;
    let mut source_predicate =
        filled_vec(state_count, usize::MAX, "replayed source predicate scratch")?;
    let mut link = filled_vec(
        transition_count,
        0usize,
        "replayed transition source-label links",
    )?;
    let mut label_counts = reserved_vec(transition_count, "replayed source-label counters")?;
    let mut label_guards = reserved_vec(transition_count, "replayed source-label guards")?;
    let truth_formula = if transition_count == 0 {
        None
    } else {
        Some(formula_builder.truth(formula_of_block[0])?)
    };

    let mut previous_group = None;
    for (transition, edge) in validated.edges.iter().enumerate() {
        let key = (edge.dense_action, edge.source);
        if previous_group != Some(key) {
            label_counts.push(0usize);
            label_guards.push(truth_formula.ok_or(BisimulationError::InternalInvariant {
                context: "replay transition carrier has no truth formula",
            })?);
            previous_group = Some(key);
        }
        let group = label_counts.len() - 1;
        link[transition] = group;
        label_counts[group] =
            label_counts[group]
                .checked_add(1)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "replayed source-label transition count",
                })?;
    }

    let mut state_splits = Vec::new();
    let mut transition_splits = Vec::new();
    let mut trace_cursor = 0usize;
    let mut current_cluster = 0usize;
    let mut current_state_block = initial_state_blocks;

    while current_cluster < transition_partition.block_count() {
        let cluster = transition_partition.members(current_cluster);
        let first_transition = *cluster
            .first()
            .ok_or(BisimulationError::InvalidCertificate {
                context: "replayed transition partition contains an empty cluster",
            })?;
        let action = validated.edges[first_transition].action;
        let target_block = state_partition.block_of(validated.edges[first_transition].target);
        for &transition in cluster {
            let edge = validated.edges[transition];
            if edge.action != action || state_partition.block_of(edge.target) != target_block {
                return Err(BisimulationError::InvalidCertificate {
                    context: "replayed transition cluster is not homogeneous",
                });
            }
            source_counts[edge.source] = source_counts[edge.source].checked_add(1).ok_or(
                BisimulationError::ArithmeticOverflow {
                    context: "replayed transition cluster source count",
                },
            )?;
        }

        let target_formula = formula_of_block[target_block];
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
        state_partition.split_touched(None, &mut state_splits)?;
        replay_splits(
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
            certificate,
            &mut trace_cursor,
        )?;

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
                return Err(BisimulationError::InvalidCertificate {
                    context: "replayed partial source-label group is not strict",
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
        state_partition.split_touched(None, &mut state_splits)?;
        replay_splits(
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
            certificate,
            &mut trace_cursor,
        )?;
        for &transition in cluster {
            source_predicate[validated.edges[transition].source] = usize::MAX;
        }

        current_cluster += 1;
        while current_state_block < state_partition.block_count() {
            for &state in state_partition.members(current_state_block) {
                for &transition in validated.reverse.edge_ids(state) {
                    transition_partition.mark(transition);
                }
            }
            transition_partition.split_touched(None, &mut transition_splits)?;
            current_state_block += 1;
        }
    }

    if trace_cursor != certificate.trace.len() {
        return Err(BisimulationError::InvalidCertificate {
            context: "certificate contains trailing physical splits",
        });
    }
    let (blocks, formula_for_block) = canonicalize_blocks(&state_partition, &formula_of_block)?;
    if formula_builder.nodes != certificate.formulas {
        return Err(BisimulationError::InvalidCertificate {
            context: "modal formula DAG differs from deterministic replay",
        });
    }
    if formula_for_block != certificate.formula_for_block {
        return Err(BisimulationError::InvalidCertificate {
            context: "canonical block formulas differ from deterministic replay",
        });
    }
    if blocks != certificate.canonical_blocks {
        return Err(BisimulationError::InvalidCertificate {
            context: "canonical partition differs from deterministic replay",
        });
    }
    if !stable_partition(validated, &blocks, colors)? {
        return Err(BisimulationError::InvalidCertificate {
            context: "replayed final partition is not stable",
        });
    }
    Ok(blocks)
}

fn replay_splits(
    partition: &RefinablePartition,
    formulas: &mut FormulaBuilder,
    formula_of_block: &mut Vec<usize>,
    splits: &[PartitionSplit],
    context: SplitContext,
    predicate_for_source: &[usize],
    certificate: &BisimulationCertificate,
    trace_cursor: &mut usize,
) -> Result<(), BisimulationError> {
    formula_of_block.try_reserve(splits.len()).map_err(|_| {
        BisimulationError::AllocationFailed {
            context: "replayed block characteristic formulas",
        }
    })?;

    for split in splits {
        let predicate_formula = predicate_for_source[split.parent_representative];
        if predicate_formula == usize::MAX {
            return Err(BisimulationError::InvalidCertificate {
                context: "replayed marked split has no guarded modal predicate",
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
            return Err(BisimulationError::InvalidCertificate {
                context: "replayed fresh state block IDs are not contiguous",
            });
        }
        formula_of_block.push(new_formula);

        let expected =
            certificate
                .trace
                .get(*trace_cursor)
                .ok_or(BisimulationError::InvalidCertificate {
                    context: "certificate omits a physical split",
                })?;
        if !same_split(
            expected,
            split,
            context,
            predicate_formula,
            partition.members(split.new_block),
        ) {
            return Err(BisimulationError::InvalidCertificate {
                context: "physical split differs from deterministic replay",
            });
        }
        *trace_cursor =
            trace_cursor
                .checked_add(1)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "certificate replay trace cursor",
                })?;
    }
    Ok(())
}

fn same_split(
    expected: &CertifiedSplit,
    split: &PartitionSplit,
    context: SplitContext,
    predicate_formula: usize,
    new_members: &[usize],
) -> bool {
    expected.phase == context.phase
        && expected.action == context.action
        && expected.target_formula == context.target_formula
        && expected.predicate_formula == predicate_formula
        && expected.parent_representative == split.parent_representative
        && expected.new_is_marked == split.new_is_marked
        && expected.new_members == new_members
}
