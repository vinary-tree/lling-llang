# Optimizer and ABI Security Contracts

The optimizer treats plans, rewrite claims, cancellation signals, budgets, and
foreign resource handles as validated inputs; publication is a security
boundary, not a formatting step.

## Terms

| Term | Definition |
|---|---|
| **retain** | One owned reference keeping an opaque resource alive. |
| **transfer** | Movement of an ownership obligation without changing total retain count. |
| **opaque ABI** | Interface whose public observation excludes private representation layout. |
| **claim promotion** | Changing an approximate or incomplete result to exact or complete without new evidence. |
| **ordered commit** | Emitting results only at the next canonical sequence position. |
| **terminal outcome** | `Cancelled`, `BudgetExceeded`, or `Failed`; none may publish. |

## Threat model

| Threat | Consequence | Contractual mitigation |
|---|---|---|
| Tape-domain confusion | Semantically invalid stages compose because labels look alike | Separate input and output domains; exact compatibility equality |
| Forged exactness | Approximate output enters an exact cache or proof path | Conservative claim meet plus independent denotational witness |
| Dependency cycle | Deadlock, nontermination, or native-stack exhaustion | Finite plan plus strictly increasing natural-number rank |
| Completion-order race | Nondeterministic reports, hashes, or cache keys | Ordered provenance commit independent of worker finish order |
| Budget bypass | Unbounded resource consumption | Monotone reservation before dispatch; terminal budget outcome |
| Cancellation resurrection | Cancelled work later becomes externally visible | No transition from cancellation to publication |
| Retain underflow | Use-after-free or double release | Partial release; owned handle required; one retain per owner invariant |
| Layout disclosure | ABI v1 clients depend on private representation | Public observation omits private layout; compatibility compares public view only |
| Provider reentrancy under lock | Deadlock at a foreign callback boundary | Provider callback occurs without registry writer lock |

## Ownership law

Let $`R`$ be the retain count and $`O`$ the set of clients in the `Owned`
state. Every reachable bounded ABI state satisfies

```math
R = \lvert O \rvert.
```

The transitions preserve this equation:

- acquire and clone add one owner and one retain;
- transfer removes one owner and adds another without changing $`R`$;
- release requires an owner and positive $`R`$, then removes both; and
- private relayout changes neither side.

The Rocq model proves the corresponding unbounded arithmetic laws. TLA+/TLC
checks interleavings across three clients, and Kani checks six arbitrary
executable operations with bit-precise overflow, array-bound, and panic checks.

## Opaque ABI v1 compatibility

An implementation state has a private layout token $`L`$ and public observation
$`P`$. Version-one compatibility is

```math
\mathrm{compatible}_{v1}(s,t)
\;\Longleftrightarrow\;
P(s)=P(t).
```

No theorem equates private layouts. Therefore a relayout is permitted when ABI
version, status behavior, resource identity, ownership, and other documented
public observations remain unchanged. Pointer provenance and C calling
preconditions remain boundary obligations described by the
[ABI trust model](abi-trust-model.md).

## Publication law

Publication requires a completed plan, complete canonical provenance, and a
completion witness. If the precision claim is exact, an independent exact
confirmation is additionally required. Formally:

```math
\mathrm{Published}
\Longrightarrow
\mathrm{Witnessed}
\land \mathrm{Finished}=V
\land
(\mathrm{Precision}=\mathrm{Exact}
 \Longrightarrow \mathrm{ExactConfirmed}).
```

Cancellation, budget exhaustion, and failure are mutually exclusive with
publication. Partial artifacts may be retained for diagnostics only when their
claim and terminal outcome remain explicit; they cannot enter exact caches.

## Resource-safe verification

Formal tools are themselves resource consumers. The repository gate enforces
RSS ceilings, disables swap, serializes proof/model work, bounds TLC wall time,
and stores all evidence on persistent repository storage. This prevents a proof
attempt from degrading host availability or moving substantial state into a
memory-backed temporary filesystem.

## Review checklist

- Does every transformation preserve separate input and output domains?
- Can any code path promote precision or completeness without new evidence?
- Does every plan edge increase the certified rank?
- Is worker completion separated from canonical commit?
- Are cancellation and budget checks made before dispatch and publication?
- Does every handle own exactly one retain, including clones and snapshots?
- Does a move transfer rather than duplicate an ownership obligation?
- Can an ABI v1 client observe any newly introduced private field?
- Are provider callbacks outside composition write locks?
- Do positive models and expected-failure mutants both pass their gates?

## References

- [Lamport 2002](../BIBLIOGRAPHY.md)
- [Delmas et al. 2026](../BIBLIOGRAPHY.md)
