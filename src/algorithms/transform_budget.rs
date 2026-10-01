//! Conservative native-transform work bounds for the checked C entry points.
//!
//! A work unit is one source-level graph/closure/partition visit. These bounds
//! deliberately over-reserve; they are not CPU instructions or elapsed time.
//! Every multiplication is checked so an unrepresentable bound is rejected.

#[derive(Clone, Copy, Debug)]
pub(crate) enum NativeTransformKind {
    Connect,
    RemoveEpsilon,
    Determinize { output_state_cap: u64 },
    Minimize { distance_iterations: u64 },
}

fn add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b)
}

fn mul(a: u64, b: u64) -> Option<u64> {
    a.checked_mul(b)
}

/// Bounds the graph-driven visits in the four native algorithms.
///
/// Let $`n`$ be the number of input states and $`a`$ the number of input arcs.
/// `connect` scans masks and adjacency, then rebuilds at most a constant number
/// of times. Epsilon removal has at most $`n^{2}a`$ pre-dedup expanded arcs;
/// its per-target bucket linear search can compare at most $`n^{3}a^{2}`$
/// pairs. Determinization processes at most $`\mathrm{cap}`$ subsets, each
/// with at most $`n`$ members and $`a`$ outgoing arcs/labels. Minimization's
/// partition can split at most $`n-1`$ times, re-enqueue at most $`na`$
/// predecessor blocks, and scan at most $`n+a`$ elements per block; its
/// shortest-distance pass visits at most $`\mathrm{iterations}`$ queues of at most $`a`$
/// arcs. Constants include setup, sorting, pushing, connect, and rebuild.
pub(crate) fn worst_case_work(kind: NativeTransformKind, n: u64, a: u64) -> Option<u64> {
    let n1 = add(n, 1)?;
    let a1 = add(a, 1)?;
    match kind {
        NativeTransformKind::Connect => mul(8, add(n1, a1)?),
        NativeTransformKind::RemoveEpsilon => mul(16, mul(mul(mul(n1, n1)?, n1)?, mul(a1, a1)?)?),
        NativeTransformKind::Determinize { output_state_cap } => {
            mul(16, mul(add(output_state_cap, 1)?, mul(n1, a1)?)?)
        }
        NativeTransformKind::Minimize {
            distance_iterations,
        } => {
            let partition = mul(64, mul(mul(n1, n1)?, mul(a1, a1)?)?)?;
            add(partition, mul(distance_iterations, a1)?)
        }
    }
}

/// Native shortest-distance pops at most this many queue entries for the
/// checked minimization path. This is a policy cap, not a convergence claim.
pub(crate) fn minimize_distance_iteration_cap(n: u64) -> Option<u64> {
    add(add(mul(n, n)?, n)?, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_and_overflowing_bounds_are_deterministic() {
        assert_eq!(
            worst_case_work(NativeTransformKind::Connect, 2, 1),
            Some(40)
        );
        assert_eq!(
            worst_case_work(NativeTransformKind::RemoveEpsilon, 2, 1),
            Some(1728)
        );
        assert_eq!(
            worst_case_work(
                NativeTransformKind::Determinize {
                    output_state_cap: 2
                },
                2,
                1
            ),
            Some(288)
        );
        assert_eq!(minimize_distance_iteration_cap(2), Some(7));
        assert_eq!(
            worst_case_work(
                NativeTransformKind::Minimize {
                    distance_iterations: 7
                },
                2,
                1
            ),
            Some(2318)
        );
        assert_eq!(
            worst_case_work(NativeTransformKind::RemoveEpsilon, u64::MAX, 1),
            None
        );
        assert_eq!(minimize_distance_iteration_cap(u64::MAX), None);
    }
}
