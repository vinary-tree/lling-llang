# Phase 5: CTC Topologies

**Branch**: `feature/ctc`
**Depends on**: Phase 3
**Started**: 2025-12-27
**Status**: COMPLETED

## Overview

Phase 5 implements various CTC (Connectionist Temporal Classification) topologies as WFSTs,
based on the NVIDIA Interspeech 2022 paper "CTC Variations Through New WFST Topologies".

CTC is used in end-to-end speech recognition to map acoustic features to label sequences.
Different topologies offer trade-offs between graph size and accuracy.

### Components

1. **Correct-CTC (T.fst)**: Standard complete graph topology
   - N states (one per vocabulary unit including blank)
   - N² arcs (complete graph with self-loops)
   - Best accuracy, largest graph

2. **Compact-CTC (Tcompact.fst)**: Reduced graph with blank back-off
   - N states
   - 3N-2 arcs
   - 1.5× smaller graph, same accuracy as Correct-CTC

3. **Minimal-CTC (Tminimal.fst)**: Smallest possible graph
   - 1 state
   - N arcs
   - 2× smaller graph, slight accuracy penalty (~0.2% WER)

4. **Selfless variants**: Remove non-blank self-loops
   - Better for wide context window models (Conformer)
   - Reduces arc count by N-1

---

## 5.1 CTC Topology Implementations

**Date**: 2025-12-27
**Status**: COMPLETED

### Hypothesis

Graph construction time should scale with arc count:

| Topology | States | Arcs | Expected Complexity |
|----------|--------|------|---------------------|
| Correct-CTC | N | N² | O(N²) |
| Compact-CTC | N | 3N-2 | O(N) |
| Minimal-CTC | 1 | N | O(N) |
| Selfless Correct | N | N²-(N-1) | O(N²) |
| Selfless Compact | N | 2N-1 | O(N) |

### Implementation

**Files created**:
- `src/ctc/mod.rs` (~85 lines): Module with re-exports and module-level tests
- `src/ctc/topologies.rs` (~510 lines): All topology implementations

**Key features**:
- Generic over semiring type (LogWeight, TropicalWeight, etc.)
- Pre-allocation for efficient construction
- Comprehensive documentation with examples
- CtcTopologyInfo struct for graph statistics

### Benchmark Results

**Configuration**: taskset -c 0-3, 100 samples, 3s warmup

| Vocab Size | Correct-CTC | Compact-CTC | Minimal-CTC | Speedup (C vs K) |
|------------|-------------|-------------|-------------|------------------|
| 10 | 936ns | 292ns | 68ns | 3.2× |
| 100 | 71.7µs | 2.16µs | 401ns | 33× |
| 500 | 4.48ms | 15.9µs | 1.58µs | 282× |
| 1000 | 17.3ms | 33.7µs | 3.47µs | 513× |

**Selfless variant results (vocab=1000)**:
- Selfless Correct-CTC: 17.6ms (similar to Correct)
- Selfless Compact-CTC: 25.8µs (1.3× faster than Compact)

### Analysis

1. **Correct-CTC scales quadratically** as expected from N² arcs
2. **Compact-CTC scales linearly** - 513× faster at N=1000
3. **Minimal-CTC is fastest** - only N arcs, single state
4. **Selfless variants** reduce arc count marginally for Correct, more for Compact

### Complexity Verification

| Topology | Theory | Observed | Match |
|----------|--------|----------|-------|
| Correct-CTC | O(N²) | ~17N² ns | ✓ |
| Compact-CTC | O(N) | ~34N ns | ✓ |
| Minimal-CTC | O(N) | ~3.5N ns | ✓ |

### Arc Count Verification

| Vocab Size | Correct | Compact | Minimal | Theory |
|------------|---------|---------|---------|--------|
| 10 | 100 | 28 | 10 | ✓ |
| 100 | 10,000 | 298 | 100 | ✓ |
| 500 | 250,000 | 1,498 | 500 | ✓ |
| 1000 | 1,000,000 | 2,998 | 1,000 | ✓ |

### Result

- [x] **ACCEPTED**: All topologies implemented with correct graph sizes
- [x] Complexity matches theoretical predictions
- [x] 15 unit tests passing
- [x] 21 benchmark cases added
- [x] Comprehensive documentation

---

