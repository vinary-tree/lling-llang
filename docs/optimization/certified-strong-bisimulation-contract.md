# Certified Strong-Bisimulation Contract

This document is the normative contract and acceptance record for the
`CertifiedBisimulation` implementation in
`src/symbolic/bisimulation.rs`. It computes strong bisimilarity for a finite
labelled transition system (LTS), rejects malformed inputs before indexing
them, emits independently replayable evidence, supplies a modal witness for
non-equivalence, and uses a constant native call stack.

The implementation replaces the repeated whole-signature rescan with a
specialized Valmari refinement machine. Ninety-seven formal obligations map
exhaustively to 13 required-green Rust properties. The implementation,
independent replay checker, property package, scale tests, benchmark matrix,
and headless allocation profile are all part of the acceptance boundary.

## Terms

| Term | Definition |
|---|---|
| **labelled transition system (LTS)** | A finite state set, an action-label set, and labelled directed transitions. |
| **strong bisimulation** | A relation whose related states have the same initial color and can match each other's transitions, label for label, into related states. |
| **partition** | Disjoint blocks of states; two states are related exactly when they share a block. |
| **splitter** | An action and target block whose predecessor set divides a current block. |
| **smaller-half rule** | Queue the smaller result of a split so each charged element moves at most logarithmically many times. |
| **compressed sparse row (CSR)** | Dense offsets plus contiguous edges for one traversal direction. |
| **certificate** | A modal-safe split trace plus a stable canonical final partition that an independent checker can replay. |
| **distinguishing witness** | An oriented modal formula that is true at one queried state and false at the other. |
| **formula DAG** | A hash-consed directed acyclic graph that shares repeated modal-formula subexpressions. |
| **canonical relation** | The state-pair equivalence relation, independent of incidental block identifiers or transition insertion order. |
| **native-stack safety** | The maximum number of native call frames is independent of input depth. |

The public action type remains an arbitrary `u32`. “Malformed label” in the
finite dense-core models means an internally fabricated dense label outside
the canonicalized action domain; every public `u32` label is representable.

## Semantic contract

Let the state set be $`\mathcal{S}`$, the action set be $`\mathcal{A}`$, the
transition relation be
$`T\subseteq\mathcal{S}\times\mathcal{A}\times\mathcal{S}`$,
and the initial coloring be $`c:\mathcal{S}\to\mathcal{C}`$. A relation
$`R\subseteq\mathcal{S}\times\mathcal{S}`$ is a colored strong bisimulation
when every $`(p,q)\in R`$ satisfies:

1. $`c(p)=c(q)`$;
2. for each $`(p,a,p')\in T`$, some $`(q,a,q')\in T`$ has
   $`(p',q')\in R`$; and
3. for each $`(q,a,q')\in T`$, some $`(p,a,p')\in T`$ has
   $`(p',q')\in R`$.

Strong bisimilarity is the union of all such relations. The Rocq theory proves
that it is reflexive, symmetric, and transitive. It also proves that a replayed
safe refinement contains every bisimilar pair, while a stable accepted
partition contains only bisimilar pairs. Therefore an accepted certificate is
exact, not merely sound.

Duplicate transitions have set semantics. Transition order, duplicate removal,
and injective action relabeling cannot change the relation. Initial colors are
observations: differently colored states never merge, even when both are
deadlocked.

Valmari's nondeterministic driver distinguishes three source categories within
the current source-label subgroup: every guarded transition reaches the
selected target region, some do, or none do. The subgroup guard records the
target region represented by that transition group. Transitions outside that
guard are irrelevant to confinement. The physical two-split sequence is
justified by guarded modal predicates rather than being misreported as a
single predecessor split. `StrongBisimulation.v` proves both predicates
saturated under strong bisimilarity. It follows that both
implementation-shaped splits preserve every bisimilar pair.

## Validated representation

![Certified strong-bisimulation flow from raw LTS validation through canonical dense CSR, Valmari partition refinement, certificate replay, and an equivalence relation or modal witness](../diagrams/optimization/strong-bisimulation-flow.svg)

*Blue is input/output, teal is canonical representation, green is refinement,
red is validation/proof checking, and purple is witness construction.*

[PlantUML source](../diagrams/optimization/strong-bisimulation-flow.puml)

<details><summary>Text view</summary>

```text
raw LTS + colors
  -> total endpoint/color validation
  -> deterministic dense labels + forward/reverse CSR
  -> refinable state and transition partitions
  -> smaller-half splitter worklist
  -> canonical blocks + replayed stable certificate
       -> same block: equivalence evidence
       -> different blocks: shared modal distinguishing DAG
```

</details>

Validation is a mandatory phase boundary:

- every source and target is checked against the state count before any array
  access;
- the color vector has exactly one entry per state;
- all allocation sizes and CSR prefix sums are checked for integer overflow;
- the empty LTS with an empty color vector is valid and has the unique empty
  partition;
- malformed endpoints, queries, and certificate indices return typed errors
  rather than being skipped or panicking; and
- an internal dense label is checked against the dense action count.

The canonicalizer compresses arbitrary `u32` action values to a deterministic
dense domain, sorts and deduplicates set-valued transitions with fixed-width
radix passes, and builds both forward and reverse CSR. Fixed-width word-RAM
radix passes keep preprocessing linear in states plus input transitions and
avoid randomized hashing in canonical output.

The reverse CSR is mandatory. Splitter refinement repeatedly asks which states
reach a selected target block under one action; scanning all outgoing
transitions or the whole partition for each splitter violates the work
contract.

## Algorithm decision

The production core specializes Valmari's refinable state-and-transition
partition algorithm for labelled nondeterministic systems
([Valmari 2010](../BIBLIOGRAPHY.md)). It meets the required
$`\mathcal{O}(m\log n)`$ refinement target with linear partition storage, handles
nondeterminism directly, and exposes split history for certificates and modal
witnesses. Paige and Tarjan provide the partition-refinement discipline
([Paige & Tarjan 1987](../BIBLIOGRAPHY.md)); Fernandez
documents an efficient bisimulation implementation
([Fernandez 1990](../BIBLIOGRAPHY.md)).

The replaced routine repeatedly constructed and sorted whole-state signatures.
The production data structures now implement refinable state and transition
partitions, reverse-CSR predecessor discovery, idempotent marking, smaller-half
charging, and deterministic canonicalization. The legacy routine remains only
as a benchmark-local comparator so the measured crossover is reproducible.

The maintained AutomataLib implementation was reviewed as an engineering
cross-check at commit
[`c371af4fce44505987b0da94de98991f76ce3f09`](https://github.com/LearnLib/automatalib/blob/c371af4fce44505987b0da94de98991f76ce3f09/util/src/main/java/net/automatalib/util/partitionrefinement/Valmari.java).
The Rust implementation must be derived from this contract and the primary
paper; it must not be a line-for-line port.

The partition-refinement lower bound explains the logarithmic-element charging
for this algorithmic family
([Groote, Martens & de Vink 2021](../BIBLIOGRAPHY.md)). No
alternative-algorithm benchmark is needed to re-establish that theoretical
decision. Production benchmarks instead measure constants, allocation traffic,
cache behavior, and crossover thresholds against the current baseline.

## Literate refinement algorithm

Every worklist, partition, and evaluator stack is an explicit contiguous
container.

```text
⟨ validate and canonicalize the LTS ⟩ ≡
    require colors.length = state_count
    for each raw transition (source, action, target):
        require source < state_count and target < state_count
    dense_action <- deterministic radix-compressed action IDs
    transitions <- radix-sort and deduplicate (source, dense_action, target)
    forward_csr <- build CSR grouped by source
    reverse_csr <- build CSR grouped by (dense_action, target)
```

```text
⟨ initialize refinable partitions ⟩ ≡
    state_partition <- canonical blocks grouped by initial color
    transition_partition <- transitions grouped by dense action and target block
    worklist <- initial transition splitters
    certificate <- empty split sequence
    witness_dag <- color atoms, hash-consed
```

```text
⟨ refine one splitter without a whole-partition scan ⟩ ≡
    splitter <- worklist.pop()
    touched_states <- reverse_csr predecessors selected by splitter
    touched_blocks <- count touched_states by current state block
    target_formula <- characteristic formula of splitter.target_block
    group_guard <- characteristic formula of the current source-label subgroup
    selected <- And(group_guard, target_formula)
    guarded_remainder <- And(group_guard, Not(target_formula))
    reaches <- Diamond(splitter.action, selected)
    confined <- And(reaches,
                    Not(Diamond(splitter.action, guarded_remainder)))
    for each touched block:
        split confined members from the remaining members in place
        split reaches members from non-reaching remaining members in place
        append both exact modal-safe splits to certificate
        append shared modal nodes that characterize the new blocks
        queue the smaller resulting subblock
    refine the source-label subgroup guard into selected and guarded remainder
    update only transition blocks incident to changed state blocks
```

```text
⟨ finalize and certify ⟩ ≡
    assign block IDs by first state in increasing state order
    replay every certificate split from the initial coloring
    require replay result = canonical final partition
    require final partition is stable in both transfer directions
    return partition, certificate, shared witness DAG, resource account
```

Refinement, certificate replay, modal evaluation, graph traversal, destruction,
and serialization must not use input-shaped native recursion. A specialized
explicit pushdown automaton carries evaluator program counters and child
indices. Partition refinement is an iterative worklist machine.

## Certificates and witnesses

A predecessor of a current block under one action is a safe splitter. More
generally, every modal predicate used by the nondeterministic Valmari driver is
saturated under strong bisimilarity. Splitting an equivalence relation by such
a predicate cannot separate a bisimilar pair. Replaying only modal-safe splits
proves the “contains bisimilarity” direction; checking final stability proves
the converse.

| Certificate field | Obligation |
|---|---|
| input digest | Binds state count, canonical transitions, and initial colors. |
| split sequence | Names the action, target block identity, affected parent, and exact child memberships. |
| canonical partition | Assigns block IDs by increasing first state. |
| stability evidence | Rejects a pair whose transfer obligation remains unsatisfied. |
| resource account | Records charges, partition cells, witness cells, whole-partition rescans, and maximum native frames. |

The witness language has color atoms, conjunction, negation, and labelled
diamond. The formula $`\langle a\rangle\varphi`$ holds at a state exactly when
it has an $`a`$-transition to a state satisfying $`\varphi`$. A split caused
by a state that can reach a characteristic target block while its peer cannot
yields an oriented distinguishing formula.

Witness selection is deterministic and formally specified. Differently
colored states use the left state's color atom. Otherwise, the query scans the
canonical physical-split trace and selects the first split whose exact child
membership separates the pair. It returns that split predicate when the left
state satisfies it and its negation otherwise. This earliest-trace rule avoids
evaluating a potentially much larger final class characteristic formula.
`differing_modal_predicate_has_oriented_witness` proves both orientations
sound.

Witnesses are hash-consed DAGs, not expanded trees and not one copy per state
pair. Evaluation is bottom-up or uses the explicit pushdown automaton.
Wißmann, Milius, and Schröder establish quasilinear generic modal-witness
construction
([Wißmann, Milius & Schröder 2022](../BIBLIOGRAPHY.md)).
Their formal setting is not silently treated as the labelled proof:
`StrongBisimulation.v` directly proves this labelled LTS construction.

## Canonical output

The semantic result is the relation, not mutable block numbers. The
implementation exposes a length-$`n`$ canonical block vector whose IDs are
assigned by the first state in each block. The full $`n\times n`$ Boolean
relation matrix is materialized only on explicit request; its unavoidable
quadratic cost is excluded from core resource claims.

Canonicality requires:

- identical relation and block vector under transition permutation;
- identical relation and block vector after duplicate removal;
- identical relation under injective relabeling of actions;
- deterministic certificate ordering after canonical input normalization; and
- no dependence on hash seeds, worker completion order, or addresses.

Rocq proves relation-matrix invariance under injective block relabeling. The
executable oracle also checks first-state block normalization, transition
permutation, duplicates, and injective action relabeling for every case.

## Resource contract

Let $`n`$ be the number of states and $`m`$ the number of distinct canonical
transitions.

| Component | Time | Auxiliary heap | Native stack |
|---|---:|---:|---:|
| validation, radix canonicalization, CSR | $`\mathcal{O}(n+m)`$ on fixed-width word-RAM | $`\mathcal{O}(n+m)`$ | $`\mathcal{O}(1)`$ |
| Valmari refinement core | $`\mathcal{O}(n+m\log\max(2,n))`$ | $`\mathcal{O}(n+m)`$ | $`\mathcal{O}(1)`$ |
| canonical block numbering | $`\mathcal{O}(n)`$ | $`\mathcal{O}(n)`$ | $`\mathcal{O}(1)`$ |
| replay certificate and shared modal DAG | $`\mathcal{O}((n+m)\log\max(2,n))`$ | $`\mathcal{O}((n+m)\log\max(2,n))`$ evidence | $`\mathcal{O}(1)`$ |
| optional relation matrix | $`\mathcal{O}(n^2)`$ | $`\mathcal{O}(n^2)`$ | $`\mathcal{O}(1)`$ |

Every state or transition charged through a smaller-half split has at most
$`\lfloor\log_2\max(1,n)\rfloor`$ charges. The resource account reports zero
whole-partition rescans and one native frame. Counters must be connected to
actual loop events and allocations; constants initialized to desired values
are not evidence.

### Preregistered benchmark protocol

The paired baseline is the replaced deterministic signature-rescan routine.
It is retained only inside the benchmark target; it is not a second production
implementation. No benchmark compares Valmari against another selected
partition-refinement algorithm.

| Shape | Construction | Paired state counts | Certified-only scale counts |
|---|---|---:|---:|
| deep chain | one transition from each state to its successor | 32, 64, 128, 256 | 4,096 and 32,768 |
| wide fan-out | one root reaches every other state | 128, 1,024, 8,192 | 65,536 |
| sparse carrier | one transition every 17 states with three colors | 1,024, 8,192, 65,536 | 131,072 |
| dense multi-label | every source reaches every target under four actions | 16, 32, 64 | 128 and 256 |

The paired measurements report construction-to-certified-result latency and
the observed crossover rather than requiring the richer certified API to win
at every small size. Certified-only measurements establish scaling where the
legacy chain would intentionally perform quadratic repeated rescans. The
acceptance record includes Criterion samples, actual work/allocation counters,
`/usr/bin/time -v` peak resident set size (RSS), and `heaptrack --record-only`
plus `heaptrack_print` output. Every executable remains inside the 4 GiB,
zero-swap systemd scope and uses repository-backed scratch storage.

### Measured acceptance record

The preregistered Criterion quick run completed in 236.86 seconds, including
the release build, with a maximum RSS of 854,756 KiB and zero swaps. It found
the expected deep-chain crossover between 32 and 64 states:

| Shape and size | Legacy median | Certified median | Interpretation |
|---|---:|---:|---|
| chain, 32 states | 27.106 µs | 48.549 µs | fixed certificate and replay cost dominates |
| chain, 64 states | 117.21 µs | 87.102 µs | certified refinement has crossed over |
| chain, 128 states | 514.83 µs | 161.60 µs | repeated rescans are already dominant |
| chain, 256 states | 2.1852 ms | 316.12 µs | certified path is approximately 6.9 times faster |
| wide fan-out, 8,192 states | 256.58 µs | 3.4241 ms | one-round legacy case exposes evidence overhead |
| sparse carrier, 65,536 states | 3.7312 ms | 12.550 ms | validation, certificate, and replay remain included |
| dense, 64 states | 76.187 µs | 3.9864 ms | benchmark compares partition-only legacy work with full certification |

Certified-only medians were 11.605 ms and 134.06 ms for chains of 4,096 and
32,768 states, 26.760 ms for a 65,536-state fan-out, 23.040 ms for a
131,072-state sparse carrier, and 16.283 ms and 103.20 ms for dense systems of
128 and 256 states. These measurements establish scale without asking the
quadratic legacy chain to exhaust the resource envelope.

A headless `heaptrack --record-only` run followed by `heaptrack_print`
measured the 32,768-state chain independently. The computation processed
32,767 canonical transitions, emitted 32,767 physical splits, charged 65,534
state/transition events, used one input-shaped native frame, and completed in
223.36 ms. The analyzer reported 81.73 MiB peak heap, 89.03 MiB peak RSS,
32,887 allocation calls, and nine temporary allocations. Its sole retained
544-byte allocation belongs to Rust runtime stack-overflow thread metadata,
not this implementation. The dominant retained structures are the production
and replay formula interners, which are required to return and independently
verify the proof-carrying result.

The final Rust acceptance matrix also passed formatting, locked
all-feature/all-target compilation, warnings-as-errors Clippy for both
all-feature and no-default configurations, the complete all-feature test
suite, and no-default certified property and scale suites. The all-feature
suite's largest process used 3,203,404 KiB RSS with zero swaps.
AddressSanitizer with LeakSanitizer and stack-use-after-return instrumentation
then passed the generated certificate/replay/witness property and all four
scale shapes. The instrumented run used 1,091,104 KiB peak RSS with zero swaps
and reported no memory error or leak.

## Parallelism and concurrency

Parallel execution is useful only when it preserves work efficiency,
canonicality, and memory bounds:

- validation and radix histograms may use deterministic chunks and prefix sums
  above a measured crossover threshold;
- predecessor counting may use thread-local touched-block arrays followed by a
  deterministic reduction;
- split commits remain exclusive and ordered by canonical splitter identity;
- modal DAG interning uses deterministic batch merge or canonical final IDs;
  and
- immutable validated CSR and completed results may implement `Send` and
  `Sync`, while mutable partitions remain private to a proved protocol.

The TLA+ model verifies the sequential lifecycle, not parallel execution.
Parallel refinement stays disabled until a separate concurrency refinement
proves race freedom, deterministic commit, bounded queues, cancellation, and
equivalence to the scalar result. A work-efficient scalar implementation is
the acceptance baseline.

## Public API obligations

| Surface | Required behavior |
|---|---|
| `CertifiedBisimulation::compute` | Returns a typed result; never skips malformed endpoints and never panics during input validation. |
| `blocks` | Returns one canonical block ID per state. |
| `try_relation_matrix` | Fallibly materializes the optional quadratic view after checked dimension arithmetic. |
| `certificate().replay` | Reconstructs the exact partition or rejects the first invalid split. |
| `try_witness(left, right)` | Returns no witness for equivalent states, a typed query error for an invalid state, and a sound oriented modal DAG for every separated pair. |
| `resources` | Exposes work, heap-cell, rescan, witness-cell, and native-frame measurements. |

Out-of-range queries, certificates bound to a different input, overflowed
allocation sizes, failed reservations, and malformed internal evidence return
typed errors. Compatibility methods retain the historical infallible return
types for callers that already guarantee valid inputs; untrusted inputs use
the `try_*` methods.

```rust
use lling_llang::symbolic::bisimulation::{CertifiedBisimulation, Lts};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lts = Lts::new(3, vec![(0, 7, 1), (0, 7, 2), (1, 7, 1)]);
    let result = CertifiedBisimulation::compute(&lts, &[0, 0, 0])?;

    let witness = result
        .try_witness(1, 2)?
        .expect("the states occupy different certified blocks");
    assert_ne!(
        witness.evaluate(&lts, &[0, 0, 0], 1)?,
        witness.evaluate(&lts, &[0, 0, 0], 2)?,
    );
    assert_eq!(
        result.certificate().replay(&lts, &[0, 0, 0])?,
        result.blocks(),
    );
    Ok(())
}
```

## Formal evidence and acceptance

![Evidence graph from the normative strong-bisimulation contract through Rocq, TLC, Z3, the exhaustive oracle, mutations, required-green properties, production refinement, independent replay, and measured resource acceptance](../diagrams/optimization/strong-bisimulation-evidence.svg)

*Yellow is the normative contract, red is proof and checking evidence, purple
is solver evidence, green is executable behavior, blue is the production core,
and orange is measured resource evidence.*

[PlantUML source](../diagrams/optimization/strong-bisimulation-evidence.puml)

<details><summary>Text view</summary>

```text
normative contract
  -> Rocq universal proofs + TLA+/TLC lifecycle + Z3 boundary controls
  -> exhaustive 97-row invariant ledger
  -> 13 required-green Rust properties
production Valmari core
  -> 13 required-green Rust properties
  -> independent replay and stability checker
5,124-case oracle -> 10 killed semantic mutants -> required-green properties
required-green properties + independent replay
  -> scale, Criterion, small-stack, and headless heap acceptance
```

</details>

| Evidence | Scope | Acceptance |
|---|---|---|
| `StrongBisimulation.v` | Unbounded semantics, plain and modal-safe replay, certificate exactness, labelled modal witnesses, validation, canonical relation, termination, and resource inequalities | Kernel compilation and zero proof escapes |
| `StrongBisimulationLifecycle.tla` | Selected finite edge sets and color maps; valid and three malformed dense-input modes | 14,916 valid distinct states plus 128 per malformed mode; safety and liveness hold |
| `vco-e4-strong-bisimulation.smt2` | Validation, refinement, witness, canonicality, work, heap, and stack boundaries | Exact 15-result transcript |
| executable oracle | Independent greatest fixed point versus partition refinement and stack-safe characteristic formulas | All 5,124 complete cases pass |
| mutation campaign | Endpoint, color, transfer, termination, duplicate, modal, certificate, and canonical-ID defects | All 10 fail for their named invariant |
| invariant ledger | Every Rocq theorem/lemma, configured TLC check, and named SMT control | 97 rows map to 13 Rust properties |
| implementation property package | Validation, equivalence, evidence, resource, empty-input, and deep-stack behavior | All 13 properties pass against `CertifiedBisimulation` |
| independent production replay | Rebuilds guarded formulas and physical splits without calling the producer; checks the final canonical partition and both transfer directions | Producer self-replay and mutation rejection pass |
| scale and stack tests | Deep chains, fan-out, sparse, dense, duplicate-heavy, and malformed systems | 100,000-state chain passes on a 64 KiB native stack with one input-shaped frame |
| benchmark and allocation profile | Preregistered legacy crossover matrix plus headless heap ownership | 4 GiB/no-swap cap respected; 89.03 MiB profiled peak RSS for the 32,768-state chain |
| Rust and sanitizer matrix | All-feature and no-default builds/tests plus AddressSanitizer and LeakSanitizer | Warnings denied, zero test failures, no sanitizer finding, and zero swaps |

The executable package lives under
`proofs/properties/strong_bisimulation`. Its gate requires all 13 tests to
run and pass; the historical missing-API red state remains available in Git
history rather than being mislabeled in the current tree.

## Security and failure behavior

A valid implementation:

- validates untrusted graph data before indexing;
- checks multiplication and prefix-sum overflow;
- makes optional quadratic output explicit and fallible;
- replays the input-bound certificate rather than trusting its digest;
- rejects invalid or cyclic witness references during evaluation;
- avoids randomized-output nondeterminism; and
- leaves caller input unchanged after every error.

Arithmetic overflow and allocation failure return an error and cannot publish
a partition as certified. The implementation does not spill analysis state;
verification and profiling scratch data use repository-backed storage, never a
memory-backed temporary filesystem.

## Implementation acceptance order

1. Prove the semantic, lifecycle, validation, evidence, and resource contracts.
2. Extract every formal invariant into a causally failing property baseline.
3. Implement total validation, radix label compression, and both CSR views.
4. Implement dual refinable partitions and smaller-half charging.
5. Compare exhaustively with the independent fixed-point oracle.
6. Add canonical numbering, independent certificate replay, guarded modal
   witnesses, and the stack-safe evaluator.
7. Turn all extracted properties green without weakening the ledger.
8. Run deep-chain, adversarial, sanitizer, profiler, and benchmark acceptance
   under the repository-backed 4 GiB/zero-swap envelope.
9. Preserve the scalar commit order because each refinement mutates shared
   partitions; parallel callers instead analyze independent immutable LTS
   values concurrently.

No step may weaken an already green invariant.

## Verification boundary

The Rocq, TLA+, and SMT artifacts specify and prove the abstract contract; they
are not a mechanical extraction of the Rust implementation. The refinement
argument is connected to production by the exhaustive oracle, 13 generated
properties, independent replay, mutation controls, and actual resource
counters. The module contains no `unsafe` block. Sanitizers and headless
profiling test memory behavior and constants, but do not turn those empirical
observations into universal theorems.

The contract is local to lling-llang's generic symbolic LTS. It introduces no
libcpg dependency and makes no CPG-specific claim. An independent adapter may
consume the result without coupling lling-llang to libcpg.

## References

- [Paige & Tarjan 1987](../BIBLIOGRAPHY.md)
- [Fernandez 1990](../BIBLIOGRAPHY.md)
- [Valmari 2010](../BIBLIOGRAPHY.md)
- [Groote, Martens & de Vink 2021](../BIBLIOGRAPHY.md)
- [Wißmann, Milius & Schröder 2022](../BIBLIOGRAPHY.md)
