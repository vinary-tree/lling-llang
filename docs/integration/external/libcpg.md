# libcpg Integration and Migration Guide

This guide tells implementers how to adopt llattice v2 and libvgraph in libcpg
and how to emit assurance-ready candidate reports. It is normative for the E7
migration but does not claim that the production adapters already exist.

## Intended dependency direction

![Component and dependency boundaries showing the independent libcpg and lling-llang cores plus the optional one-way integration](../../diagrams/optimization/libcpg-integration-boundaries.svg)

[PlantUML source](../../diagrams/optimization/libcpg-integration-boundaries.puml)

lling-llang is not part of libcpg. libcpg may optionally use lling-llang
through a feature-gated integration module or companion adapter, but libcpg's
core analyses and lling-llang's optimizer remain independently usable. The
allowed optional dependency points from libcpg integration code to
lling-llang's public API; lling-llang does not depend on libcpg. The companion
adapter owns Vinary-specific evidence framing. This avoids a dependency cycle
without prohibiting useful one-way composition.

## Migration sequence

![Formal-first E7 sequence with explicit blocking before production and independent verification after acceptance](../../diagrams/optimization/libcpg-refinement-gate.svg)

[PlantUML source](../../diagrams/optimization/libcpg-refinement-gate.puml)

The migration order is strict:

1. freeze exact repository revisions and dirty-file ownership;
2. prove dataflow, graph quotient, evidence, resource, and stack invariants;
3. exhaust the positive lifecycle and demonstrate the required mutant failure;
4. map every formal declaration/check to one unique Rust property;
5. create those properties and capture a failure caused by the absent
   implementation, not by syntax or harness failure;
6. implement the smallest lawful adapters that satisfy the full contract;
7. run differential, property, deep-stack, deterministic-concurrency, and
   preregistered performance acceptance; and
8. obtain independent trusted verification.

Steps 5–8 may not be collapsed. A green property suite created after the code
does not prove that the test could detect the missing behavior.

## Dataflow compatibility matrix

libcpg currently exposes two conceptually related but directionally different
contracts:

| libcpg surface | Existing observation | llattice v2 mapping | Required evidence |
|---|---|---|---|
| intraprocedural lattice | `join(left, right)` and `leq(left, right)` | `JoinSemilattice::join_assign` and induced `leq` | identical joined value and exact change flag |
| IFDS lattice | `join` plus `subsumes(container, contained)` | join plus `leq(contained, container)` | explicit argument reversal |
| IFDS initialization | `Default` | `Bottom` only for named lawful types | proof that default is the context-free least element |
| convergence | worklist termination in a concrete analysis | finite height, chain bound, widening, or stable witness | no generic termination claim from join laws alone |

Do not introduce a blanket conversion trait that silently identifies
`Default` with bottom or subsumption with same-direction order.

## Required adapter traits

The eventual concrete names may follow the destination repositories'
conventions, but objects must supply these semantic capabilities:

| Capability | Requirement |
|---|---|
| join | associative, commutative, idempotent operation |
| order | derived from join and tested for partial-order laws |
| mutation | return whether the stored value changed |
| initialization | explicit initial fact, or a separately lawful bottom type |
| transfer | monotone for analyses claiming Kildall-style convergence |
| completion | stable witness or declared analysis-specific convergence bound |
| precision | exact or approximate, never inferred from successful execution |
| completeness | complete or incomplete, never inferred from an empty worklist after cancellation |

Static dispatch and associated types are preferred on analysis hot paths.
Dynamic evidence objects belong at the assurance boundary.

## Structural graph adoption

The adapter must validate the libcpg projection before importing it:

```text
VALIDATE-AND-IMPORT(projection)
    validate canonical node order
    validate forward CSR offsets and targets
    validate reverse CSR offsets and targets
    validate reverse adjacency is the exact transpose
    import borrowed or owned canonical arrays
    return validated graph
```

The validation/import budget is linear in vertices plus edges. Once imported,
libvgraph supplies iterative SCC decomposition, total quotient fibers,
condensation, and deterministic wavefronts. libcpg retains:

- source-language and code-property graph semantics;
- call/return and interprocedural validity rules;
- analysis-specific transfer functions;
- diagnostic and source-location interpretation; and
- candidate finding construction.

libvgraph does not become a code-property-graph analysis engine. It is the
neutral structural substrate.

### Stack-safe matched-call analysis

Flat SCC, CSR validation, and Kildall worklists use heap-resident finite graph
machines. Interprocedural realizable-path analysis has matched call/return
semantics and may use a specialized iterative pushdown automaton. Its control
stack must be explicit heap state; mutual Rust recursion is forbidden.

## Assurance-ready report construction

A report must capture, not borrow from mutable global state:

| Field | Required content |
|---|---|
| subject | stable artifact/program identity |
| snapshot | immutable source and graph revision |
| configuration | complete semantics and resource-limit identity |
| tool | libcpg, optional lling-llang integration, adapter, and every behavior-affecting dependency revision |
| environment | target and semantic runtime/provider identity |
| digest | domain-separated digest over canonical framed report bytes |
| producer | authenticated producer identity |
| precision | exact or approximate |
| completeness | complete or incomplete |

The companion adapter validates the complete index before accepting a
guarantee. It does not infer omitted coordinates from process-local state.

## Concurrency

Concurrency is permitted at three boundaries:

1. independent immutable analysis requests;
2. independent nodes in a canonical condensation wavefront; and
3. independent report-validation batches.

Each worker writes private output. Ordered commit uses stable canonical keys.
Acceptance compares serial and parallel bytes across one, two, and multiple
workers with randomized completion delays. Shared mutable saturation is outside
this migration.

## Required-red property suites

`proofs/doc/libcpg-assurance-invariants.tsv` assigns all 122 formal obligations
to five Rust suites:

- `tests/libcpg_dataflow_migration_properties.rs`;
- `tests/libcpg_graph_quotient_properties.rs`;
- `tests/libcpg_evidence_assurance_properties.rs`;
- `tests/libcpg_evidence_lifecycle_properties.rs`; and
- `tests/libcpg_assurance_boundary_properties.rs`.

Every named property must first fail because its production behavior is absent
or wrong. Compile errors, missing fixtures, unconditional panic, ignored tests,
and unreachable assertions are invalid red evidence.

After implementation, the same properties must pass over generated lawful
algebras, graph permutations, deep graphs, stale evidence coordinates,
approximation/completeness combinations, worker counts, and resource caps.

## Compatibility and release

The migration is additive until parity is established:

- preserve public libcpg result semantics;
- preserve deterministic diagnostic and finding order;
- retain legacy trait adapters during the documented semver window;
- deprecate only after downstream source compatibility is demonstrated;
- remove duplicated algorithms only after differential and performance
  acceptance; and
- record exact llattice v2, libvgraph, libmorphism, libcpg, and adapter
  revisions in release evidence.

No compatibility shim or optional lling-llang integration may change
`CompleteExact` meaning, erase an evidence coordinate, allocate recursively,
or serialize native graph/dataflow hot paths through JSON.

## Operator reproduction

The formal baseline is reproduced from the lling-llang repository:

```bash
make verify-proofs
```

The command self-enters a 4 GiB, no-swap systemd scope, uses repository-local
temporary storage, serializes Rocq compilation, runs one TLC worker, verifies
the required mutant failure, compares exact Z3 transcripts, and retains logs
under `target/formal-verification/logs/`.

## References

- [Kildall 1973](../../BIBLIOGRAPHY.md)
- [Cousot & Cousot 1977](../../BIBLIOGRAPHY.md)
- [Tarjan 1972](../../BIBLIOGRAPHY.md)
- [Reps, Horwitz & Sagiv 1995](../../BIBLIOGRAPHY.md)
