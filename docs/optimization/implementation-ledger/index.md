# WFST Features Implementation Ledger

This is a chronological scientific log of WFST feature implementations, organized
into phases. Each phase file uses the same hypothesis → baseline → measurement →
accept/reject protocol defined below. **The split is for navigation; the ledger
remains a single logical document — cross-phase comparisons remain meaningful and
phases should be read in order when reconstructing perf history.**

## Methodology

1. **Hypothesis**: Document expected behavior and complexity
2. **Implementation**: Code changes with complexity analysis
3. **Baseline**: Measure before implementation
4. **Verification**: Measure after, compare with p < 0.05 threshold
5. **Accept/Reject**: Merge if improvement confirmed, revert otherwise

## Hardware Configuration

See `/home/dylon/.claude/hardware-specifications.md` for full details.

**Benchmark Configuration**:

- Framework: Criterion 0.5
- Confidence interval: 95%
- Significance threshold: p < 0.05
- Minimum samples: 100 iterations
- Warmup: 3 seconds
- CPU governor: performance mode
- CPU affinity: `taskset -c 0-3`
- Resource limits: `systemd-run --user --scope -p MemoryMax=32G -p CPUQuota=400%`

## Phase Index

| Phase | Title | File |
|---|---|---|
| 1 | Foundation Algorithms (shortest-distance) | [phase-1-foundation-algorithms.md](phase-1-foundation-algorithms.md) |
| 2 | Core WFST Operations | [phase-2-core-wfst-operations.md](phase-2-core-wfst-operations.md) |
| 3 | Determinization & Minimization | [phase-3-determinization-minimization.md](phase-3-determinization-minimization.md) |
| 4 | Additional Semirings | [phase-4-additional-semirings.md](phase-4-additional-semirings.md) |
| 5 | CTC Topologies | [phase-5-ctc-topologies.md](phase-5-ctc-topologies.md) |
| 6 | Differentiable Operations | [phase-6-differentiable-operations.md](phase-6-differentiable-operations.md) |
| 7 | Optimizations | [phase-7-optimizations.md](phase-7-optimizations.md) |
