//! Total validation, fixed-width canonicalization, and sparse indexing.

use super::{Action, BisimulationError, Lts};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RawEdge {
    source: usize,
    action: Action,
    target: usize,
}

/// One canonical set-valued transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Edge {
    pub(super) source: usize,
    pub(super) action: Action,
    pub(super) dense_action: usize,
    pub(super) target: usize,
}

/// A compressed-sparse-row index whose payloads are canonical edge IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Csr {
    offsets: Vec<usize>,
    edge_ids: Vec<usize>,
}

impl Csr {
    #[inline]
    pub(super) fn edge_ids(&self, state: usize) -> &[usize] {
        &self.edge_ids[self.offsets[state]..self.offsets[state + 1]]
    }
}

/// An LTS whose endpoints, colors, labels, edge set, and sparse indices have
/// crossed the mandatory validation boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ValidatedLts {
    pub(super) state_count: usize,
    pub(super) edges: Vec<Edge>,
    pub(super) actions: Vec<Action>,
    pub(super) forward: Csr,
    pub(super) reverse: Csr,
    pub(super) input_digest: [u8; 32],
}

impl ValidatedLts {
    pub(super) fn build(lts: &Lts, colors: &[usize]) -> Result<Self, BisimulationError> {
        if colors.len() != lts.num_states {
            return Err(BisimulationError::ColorCount {
                states: lts.num_states,
                colors: colors.len(),
            });
        }

        let mut raw = reserved_vec(lts.transitions.len(), "canonical transitions")?;
        for (index, &(source, action, target)) in lts.transitions.iter().enumerate() {
            if source >= lts.num_states {
                return Err(BisimulationError::InvalidSource {
                    transition: index,
                    source,
                    states: lts.num_states,
                });
            }
            if target >= lts.num_states {
                return Err(BisimulationError::InvalidTarget {
                    transition: index,
                    target,
                    states: lts.num_states,
                });
            }
            raw.push(RawEdge {
                source,
                action,
                target,
            });
        }

        radix_sort_raw_edges(&mut raw)?;
        raw.dedup();

        let mut actions = reserved_vec(raw.len(), "dense action domain")?;
        let mut edges = reserved_vec(raw.len(), "validated transitions")?;
        let mut previous_action = None;
        let mut dense_action = 0usize;
        for edge in raw {
            if previous_action != Some(edge.action) {
                dense_action = actions.len();
                actions.push(edge.action);
                previous_action = Some(edge.action);
            }
            edges.push(Edge {
                source: edge.source,
                action: edge.action,
                dense_action,
                target: edge.target,
            });
        }

        let forward = build_csr(lts.num_states, &edges, |edge| edge.source, "forward CSR")?;
        let reverse = build_csr(lts.num_states, &edges, |edge| edge.target, "reverse CSR")?;
        let input_digest = digest(lts.num_states, &edges, colors);

        Ok(Self {
            state_count: lts.num_states,
            edges,
            actions,
            forward,
            reverse,
            input_digest,
        })
    }

    #[inline]
    pub(super) fn transition_count(&self) -> usize {
        self.edges.len()
    }

    /// Assign a dense deterministic class to every transition according to
    /// `(dense_action, current_target_block)`. The temporary edge-ID order is
    /// produced with fixed-width radix passes, so initialization remains
    /// linear even when labels are sparse `u32` values.
    pub(super) fn transition_classes(
        &self,
        target_blocks: &[usize],
    ) -> Result<Vec<usize>, BisimulationError> {
        let mut order = reserved_vec(self.edges.len(), "transition class order")?;
        order.extend(0..self.edges.len());
        let mut scratch = filled_vec(order.len(), 0usize, "transition class radix scratch")?;

        for pass in 0..core::mem::size_of::<usize>() {
            radix_ids_pass(&mut order, &mut scratch, |edge_id| {
                (target_blocks[self.edges[edge_id].target] >> (pass * 8)) & 0xff
            })?;
        }
        for pass in 0..core::mem::size_of::<usize>() {
            radix_ids_pass(&mut order, &mut scratch, |edge_id| {
                (self.edges[edge_id].dense_action >> (pass * 8)) & 0xff
            })?;
        }

        let mut classes = filled_vec(self.edges.len(), 0usize, "transition classes")?;
        let Some(&first_edge) = order.first() else {
            return Ok(classes);
        };
        let mut class = 0usize;
        let mut previous = (
            self.edges[first_edge].dense_action,
            target_blocks[self.edges[first_edge].target],
        );
        for edge_id in order {
            let key = (
                self.edges[edge_id].dense_action,
                target_blocks[self.edges[edge_id].target],
            );
            if key != previous {
                class = class
                    .checked_add(1)
                    .ok_or(BisimulationError::ArithmeticOverflow {
                        context: "transition class count",
                    })?;
                previous = key;
            }
            classes[edge_id] = class;
        }
        Ok(classes)
    }
}

fn radix_ids_pass(
    ids: &mut Vec<usize>,
    scratch: &mut Vec<usize>,
    key: impl Fn(usize) -> usize,
) -> Result<(), BisimulationError> {
    let mut counts = [0usize; 256];
    for &edge_id in ids.iter() {
        counts[key(edge_id)] += 1;
    }
    let mut total = 0usize;
    for count in &mut counts {
        let bucket = *count;
        *count = total;
        total = total
            .checked_add(bucket)
            .ok_or(BisimulationError::ArithmeticOverflow {
                context: "transition class radix prefix sum",
            })?;
    }
    for &edge_id in ids.iter() {
        let bucket = key(edge_id);
        scratch[counts[bucket]] = edge_id;
        counts[bucket] += 1;
    }
    core::mem::swap(ids, scratch);
    Ok(())
}

fn build_csr(
    state_count: usize,
    edges: &[Edge],
    state_of: impl Fn(&Edge) -> usize,
    context: &'static str,
) -> Result<Csr, BisimulationError> {
    let offset_count = state_count
        .checked_add(1)
        .ok_or(BisimulationError::ArithmeticOverflow { context })?;
    let mut offsets = filled_vec(offset_count, 0usize, context)?;
    for edge in edges {
        let slot = state_of(edge)
            .checked_add(1)
            .ok_or(BisimulationError::ArithmeticOverflow { context })?;
        offsets[slot] = offsets[slot]
            .checked_add(1)
            .ok_or(BisimulationError::ArithmeticOverflow { context })?;
    }
    for state in 0..state_count {
        offsets[state + 1] = offsets[state + 1]
            .checked_add(offsets[state])
            .ok_or(BisimulationError::ArithmeticOverflow { context })?;
    }

    let mut cursor = reserved_vec(state_count, context)?;
    cursor.extend_from_slice(&offsets[..state_count]);
    let mut edge_ids = filled_vec(edges.len(), 0usize, context)?;
    for (edge_id, edge) in edges.iter().enumerate() {
        let state = state_of(edge);
        edge_ids[cursor[state]] = edge_id;
        cursor[state] += 1;
    }
    Ok(Csr { offsets, edge_ids })
}

fn radix_sort_raw_edges(edges: &mut Vec<RawEdge>) -> Result<(), BisimulationError> {
    if edges.len() < 2 {
        return Ok(());
    }
    let mut scratch = filled_vec(edges.len(), RawEdge::default(), "transition radix scratch")?;
    let usize_bytes = core::mem::size_of::<usize>();
    let passes = usize_bytes
        .checked_mul(2)
        .and_then(|value| value.checked_add(core::mem::size_of::<Action>()))
        .ok_or(BisimulationError::ArithmeticOverflow {
            context: "transition radix pass count",
        })?;

    for pass in 0..passes {
        let mut counts = [0usize; 256];
        for edge in edges.iter() {
            counts[edge_byte(*edge, pass, usize_bytes) as usize] += 1;
        }
        let mut total = 0usize;
        for count in &mut counts {
            let bucket = *count;
            *count = total;
            total = total
                .checked_add(bucket)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "transition radix prefix sum",
                })?;
        }
        for edge in edges.iter().copied() {
            let bucket = edge_byte(edge, pass, usize_bytes) as usize;
            scratch[counts[bucket]] = edge;
            counts[bucket] += 1;
        }
        core::mem::swap(edges, &mut scratch);
    }
    Ok(())
}

#[inline]
fn edge_byte(edge: RawEdge, pass: usize, usize_bytes: usize) -> u8 {
    if pass < usize_bytes {
        ((edge.target >> (pass * 8)) & 0xff) as u8
    } else if pass < usize_bytes * 2 {
        ((edge.source >> ((pass - usize_bytes) * 8)) & 0xff) as u8
    } else {
        ((edge.action >> ((pass - usize_bytes * 2) * 8)) & 0xff) as u8
    }
}

fn digest(state_count: usize, edges: &[Edge], colors: &[usize]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"lling-llang/certified-strong-bisimulation/v1\0");
    hasher.update(&state_count.to_le_bytes());
    hasher.update(&edges.len().to_le_bytes());
    for edge in edges {
        hasher.update(&edge.source.to_le_bytes());
        hasher.update(&edge.action.to_le_bytes());
        hasher.update(&edge.target.to_le_bytes());
    }
    hasher.update(&colors.len().to_le_bytes());
    for &color in colors {
        hasher.update(&color.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
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

    #[test]
    fn validation_is_total_and_canonicalization_has_set_semantics() {
        let lts = Lts::new(3, vec![(2, 9, 1), (0, 4, 2), (2, 9, 1), (0, 4, 1)]);
        let validated = ValidatedLts::build(&lts, &[0, 0, 0]).unwrap();
        assert_eq!(validated.actions, [4, 9]);
        assert_eq!(validated.edges.len(), 3);
        assert_eq!(validated.edges[0].source, 0);
        assert_eq!(validated.edges[0].target, 1);
        assert_eq!(validated.edges[1].target, 2);
        assert_eq!(validated.edges[2].source, 2);
    }

    #[test]
    fn malformed_endpoints_are_never_skipped() {
        assert!(matches!(
            ValidatedLts::build(&Lts::new(1, vec![(1, 0, 0)]), &[0]),
            Err(BisimulationError::InvalidSource { .. })
        ));
        assert!(matches!(
            ValidatedLts::build(&Lts::new(1, vec![(0, 0, 1)]), &[0]),
            Err(BisimulationError::InvalidTarget { .. })
        ));
    }
}
