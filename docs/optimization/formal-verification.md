# Formal Verification of the Optimizer Contract

This report identifies exactly what is proved, what is exhaustively checked in
finite models, how representative defects are detected, and what remains an
implementation-refinement obligation.

## Terms

| Term | Definition |
|---|---|
| **Rocq** | Interactive theorem prover used for unbounded constructive laws. |
| **TLA+** | Temporal Logic of Actions specification language used for lifecycle and concurrency. |
| **TLC** | Explicit-state model checker for finite TLA+ configurations. |
| **SMT** | Satisfiability Modulo Theories; Z3 checks small consistency and independence formulas. |
| **Kani** | Rust verifier that lowers proof harnesses to the C Bounded Model Checker (CBMC). |
| **positive model** | Intended state machine; every configured invariant must hold. |
| **negative control** | Deliberately broken model; the named invariant must fail for the expected reason. |
| **refinement** | Evidence connecting implementation behavior to the abstract contract. |

## Evidence stack

![Formal evidence stack from normative contract through proof tools to implementation refinement](../diagrams/optimization/formal-evidence-stack.svg)

[PlantUML source](../diagrams/optimization/formal-evidence-stack.puml)

*Yellow is the normative contract, red is theorem and temporal proof, purple is solver evidence, green is the resource-safe gate, and blue is the implementation boundary.*

<details><summary>Text view</summary>

<!-- vdl-disable-next-line ASCII001 -->
```text
Normative contract
 ├─ Rocq: unbounded denotational and algebraic laws
 ├─ TLA+/TLC: finite concurrent lifecycle exploration
 ├─ Z3: intended unsatisfiability plus constructive counterexample
 └─ Kani/CBMC: bit-precise bounded ownership behavior
          │
          ▼
resource-safe formal gate
          │
          ▼
Rust implementation must refine the checked contract
```

</details>

The tools are complementary. TLC state counts are not presented as unbounded
proofs. Rocq category and DAG theorems do not establish pointer validity. Kani
checks bounded executable behavior but does not replace the denotational proof.

## Unbounded Rocq theorems

All listed modules compile without `Admitted`, `Abort`, `Axiom`, `Conjecture`,
or proof parameters masquerading as global facts. `Assumptions.v` prints the
assumption closure of representative theorems.

| Module | Checked results |
|---|---|
| `optimizer/TapeSignatures.v` | Separate tape endpoints; signature identity and associativity; typed morphism associativity; constructive counterexample to input-only compatibility |
| `optimizer/RewriteSemantics.v` | Reflexive, symmetric, transitive exact rewrite; composable proof witnesses; precision and completeness no-promotion; exact publication implies denotational preservation |
| `optimizer/PlanDag.v` | Dependency paths strictly increase rank; valid plans are acyclic; no self edge; successful commit advances exactly once; out-of-order commit is rejected; existing provenance is a prefix after commit |
| `abi/OwnershipLifecycle.v` | Initial retain count; release at zero rejected; retain then release neutral; transfer count preservation; opaque ABI v1 observational equivalence |
| `wfst/LazyExpansion.v` | Six explicit expansion phases; exact observation and cacheability; normal/retry authorization; unique ownership; snapshot freshness; failure/cancellation reset; stale-completion rejection; finite iterative control |

## Finite TLA+/TLC exploration

The following counts are from the checked configurations committed beside each
specification. A **distinct state** is a unique reachable assignment after TLC
fingerprinting; **depth** is the longest shortest path in the complete state
graph.

| Model | Generated | Distinct | Depth | Principal invariants |
|---|---:|---:|---:|---|
| `OptimizerLifecycle.tla` | 831 | 607 | 14 | ranked DAG, dependency readiness, no precision/completeness promotion, canonical provenance prefix, terminal non-publication, exact confirmation |
| `LazyWfstLifecycle.tla` | 449 | 80 | 7 | no persistent entries for no-cache/zero-LRU, positive LRU capacity, exact finite LRU order, unique transient entry |
| `LazyExpansionLifecycle.tla` | 65,777 | 6,728 | 14 | unique expansion owner, captured/current snapshot agreement, exact empty/nonempty observation, retry only after retryable failure, cancellation ownership, bounded attempt count |
| `AbiOwnershipLifecycle.tla` | 3,757 | 912 | 13 | retain count equals owned clients, moved/released clients do not own, ABI version and identity stable, private relayout unobservable |

The two lazy models address different contracts. `LazyWfstLifecycle.tla`
models cache-policy storage and deliberately represents LRU order as a finite
sequence. An earlier timestamp encoding introduced irrelevant unbounded clock
symmetry and was rejected after diagnostic exploration. The checked model
preserves observable eviction order while keeping the state space finite.
`LazyExpansionLifecycle.tla` models semantic expansion, including the
difference between no transition record and an exact zero-length record.

## Negative controls

Every mutation is generated in ignored persistent evidence storage, checked,
and removed. A negative control passes only when TLC fails on the named
invariant; syntax errors, timeouts, and different invariant failures fail the
gate.

| Mutation | Required detection |
|---|---|
| No-cache insertion stores a persistent entry | `MemoryBounded` violation |
| Foreign provider callback acquires registry writer | `NoCallbackUnderRegWrite` violation |
| RRWM uses stale expert losses | `WeightsExact` violation |
| Cascade omits ordering constraint | `OrderingConstraints` violation |
| Optimizer commits an arbitrary finished node | `ProvenanceIsCanonicalPrefix` violation |
| LazyWfst policy change retains cache in no-cache mode | `NoCacheHasNoPersistentEntries` violation |
| Unexpanded state is reported as exact empty | `UnexpandedNeverAppearsEmpty` violation |
| A second worker claims an expanding state | `AtMostOneExpansionOwner` violation |
| Explicit retry starts after a nonretryable failure | `NonRetryableFailureIsTerminal` violation |
| An observation is published under a stale snapshot | `ObservableStateUsesCurrentSnapshot` violation |
| ABI release fails to decrement retain count | `RetainsEqualOwners` violation |

## Z3 dual transcript

`vco-e4-contracts.smt2` has six independent queries with the required result
sequence:

```text
unsat
unsat
sat
unsat
unsat
unsat
```

The satisfiable query is intentional: if the output-tape type is erased, equal
input domains can coexist with an incompatible left output and right input.
The unsatisfiable queries cover precision promotion, completeness promotion,
release at zero under a positive-retain precondition, out-of-order successful
commit, and publication after cancellation.

`vco-e4-lazy-expansion.smt2` adds fourteen named lifecycle-boundary queries.
Its exact transcript contains twelve `unsat` results and two constructive `sat`
witnesses. The unsatisfiable cases exclude false empty observation, duplicate
ownership, ordinary retry, nonretryable retry, begin after cancellation, stale
completion, stale observation, wrong-owner completion, and overlapping empty
and nonempty classification. The satisfiable witnesses demonstrate explicit
reset and valid explicit retry.

## Kani bounded ABI refinement

`proofs/kani/abi_ownership_model.rs` contains three harnesses:

1. six nondeterministic operations across three clients preserve exactly one
   retain per owned client;
2. release at zero returns `None`; and
3. ABI v1 public observations ignore private layout.

Kani 0.67.0 and CBMC 6.8.0 checked all three harnesses successfully. The
ownership harness uses unwind bound 7, sufficient for its six-iteration loop
and the three-element owner-count loop. [Delmas et al. 2026](../BIBLIOGRAPHY.md)
describes Kani's Rust-to-CBMC verification pipeline.

## Resource and storage envelope

The outer proof script self-enters a user systemd scope with:

- `MemoryMax=4G`;
- `MemorySwapMax=0`;
- `CPUQuota=400%`;
- `TasksMax=64`;
- one Rocq build job and one TLC worker; and
- a 120-second timeout per TLC model.

Kani self-enters a nested 2 GiB/no-swap scope with one job and a 120-second
wall-clock timeout. Java receives a 3 GiB heap ceiling. All logs, model metadata,
mutants, Kani targets, and tool temporary files live below the repository's
ignored `target/formal-verification/`; no material artifact uses a memory-backed
temporary directory.

## Verification boundary

The lazy-expansion Rust implementation now refines its lifecycle model through
89 bidirectionally mapped properties: 57 Rocq declarations, 18 TLC invariants,
and 14 SMT queries. Separate unit evidence covers panic rollback, concurrent
cancellation, independent-wrapper parallelism, saturating counters, and O(1)
intrusive LRU behavior. Later optimizer-plan and ABI work must still add
refinement evidence connecting their concrete scheduler and wire types to the
remaining models. The present proofs do not claim:

- memory safety for arbitrary foreign pointers that violate C preconditions;
- liveness under an unfair executor or a provider that never returns;
- asymptotic bounds for domain work inside a plan node;
- that every indexed candidate feed is a fibration; or
- that approximate and exact denotations are interchangeable.

## Reproduction

```bash
make verify-proofs
```

Inspect captured evidence under `target/formal-verification/logs/`. Successful
execution removes TLC state databases and generated mutants; logs remain for
diagnosis and are ignored by Git.

## References

- [Lamport 2002](../BIBLIOGRAPHY.md)
- [Delmas et al. 2026](../BIBLIOGRAPHY.md)
- [Mac Lane 1998](../BIBLIOGRAPHY.md)
