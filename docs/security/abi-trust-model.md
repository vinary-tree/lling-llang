# ABI trust model — foreign WFSTs as untrusted input

What lling-llang trusts, validates, and contains at its C ABI. The library
plays **both roles** of the family resource protocol: it *consumes* foreign
scalar-WFST providers (`lling_wfst_import`, `lling_wfst_compose`) and it
*produces* resources for foreign consumers (`lling_wfst_resource`). Each
role has its own boundary discipline. This document is the lling-llang
instantiation of the family-wide trust model, which is normative here:
[family security model](https://github.com/vinary-tree/liblevenshtein-rust/blob/master/vinary-tree-interop/docs/security-model.md)
(assets, trust zones, the containment law, the parallelism-by-claim
analysis, exhaustion vectors, and stated non-goals).

Mechanics of the layer under discussion:
[C ABI reference](../api/c-abi-reference.md) ·
[Resource ABI architecture](../architecture/resource-abi.md).

---

## Terms & symbols

Symbols link to [`NOTATION.md`](../NOTATION.md).

| Term | Meaning |
|---|---|
| **provider** | The implementation behind a `VtResource`: any library, in any language, honoring `vt.scalar-wfst.1`. |
| **trust boundary** | The vtable edge: every provider callback's *data* replies (statuses, counts, flags, labels, weights) are untrusted input. |
| **containment** | Faults become status codes; no unwinding crosses `extern "C"` in either direction. |
| $`\bar{0}`$, $`+\infty`$, $`-\infty`$ | The tropical additive identity is $`+\infty`$; $`-\infty`$ is *outside* the tropical carrier and is treated as hostile input. |
| **F1 / LLING-B2** | The confirmed ingestion finding motivating this document ([case study below](#the-motivating-case-f1--a--infty-that-nan-poisons-composition)). |

## Trust zones

```text
┌────────────────────────── process ──────────────────────────────────┐
│  C caller (application)                                             │
│      │  lling_* calls: statuses out, panics contained               │
│  ┌───▼──────────────────────────────┐    ┌────────────────────────┐ │
│  │ lling-llang (this crate)         │    │ foreign providers      │ │
│  │  ffi.rs boundary + bindings.rs   │───▶│ (libdictenstein,       │ │
│  │  validation at every ingestion   │◀───│  duallity, any C/C++/  │ │
│  │  + exported vtables              │    │  Rust implementation)  │ │
│  └──────────────────────────────────┘    └────────────────────────┘ │
│         every DATA reply validated ▲ trust boundary                 │
└──────────────────────────────────────────────────────────────────────┘
```

Three parties, three postures:

1. **The C caller** is trusted with pointer validity for lling-llang's own
   opaque handles (`LlingWfstBuilder*`, `LlingWfst*`) — the classical C
   contract: a forged or double-freed handle is undefined behavior, exactly
   as with `free(3)`. Everything *checkable* is checked: null pointers,
   argument domains, lifecycle state (`CLOSED`), label validity, weight
   validity.
2. **Foreign providers** are the untrusted-input class this document
   centers. lling-llang assumes only the *structural* family contract it
   cannot verify — that vtable pointers dereference to a vtable, that
   callbacks are callable, and that pointers stay valid while the resource
   is retained. Every **data-shaped** reply is validated before use
   (next section). A provider can therefore cause wrong *answers* or
   resource exhaustion, but not — through any data channel — memory
   unsafety in lling-llang.
3. **Foreign consumers** of lling-llang's exported resources receive claims
   lling-llang must keep: `IMMUTABLE` (the graph never changes),
   `PARALLEL_REENTRANT` (callbacks are safe concurrently — upheld by
   immutable payloads, `RwLock` caches, and the no-callback-under-write-lock
   rule), and `LAZY` (expansion on demand). The
   [architecture doc](../architecture/resource-abi.md#concurrency-deliberately-no-resource-wide-gate)
   details why these claims hold without a resource-wide gate.

## Validation duties — what a hostile provider can and cannot do

At capture, import, and every lazy expansion, all provider output passes
explicit acceptance predicates (stated as display math in the
[architecture doc](../architecture/resource-abi.md#validation-at-ingestion--the-acceptance-predicates)):

| Reply channel | Checks | On violation |
|---|---|---|
| status (`uint32_t`) | decoded via `VtStatus::from_raw`; out-of-range is rejected *before* any enum-typed use | `PROVIDER_ERROR` (never undefined behavior) |
| base vtable | `struct_size`, `abi_version`, `retain`/`release`/`query_interface` present | `INCOMPATIBLE_RESOURCE` |
| WFST vtable | `struct_size`, `interface_version`, all five ops present, `unit_domain`, `weight_domain` | `INCOMPATIBLE_RESOURCE` |
| snapshot | non-null resource words | `PROVIDER_ERROR` |
| `state_info` | flags $`\le 1`$; final weight in the tropical carrier | `PROVIDER_ERROR` |
| arc pages | $`\mathit{written} \le \mathit{capacity}`$, offsets never exceed `total`, no empty page before completion, `total` stable | `PROVIDER_ERROR` |
| arc fields | presence flags $`\le 1`$, reserved bytes zero, labels are Unicode scalars, weight in the tropical carrier | `PROVIDER_ERROR` |
| sizes | states beyond native limits, label overflow on import | `LIMIT_EXCEEDED` |

Residual capabilities an adversarial provider keeps, deliberately, because
no consumer-side check can remove them (they are bounded and availability-
class, per the family model): lying *consistently* about graph contents
(wrong answers in, wrong answers out), unbounded graph *shape* (state
explosion — the caller controls how much it traverses; composition
materializes only visited product states), and *blocking* inside a callback
(a stalled provider stalls the calling thread; run untrusted providers
out-of-process if this must be bounded).

## The motivating case: F1 — a $`-\infty`$ that NaN-poisons composition

The concrete, confirmed case that shaped these duties (ledger finding
LLING-B2, family pre-registered finding F1):

- **The hole.** ABI weight ingestion originally rejected only NaN
  (`weight.is_nan()`). IEEE-754 `f64` happily carries $`-\infty`$, which is
  *not* in the verified tropical carrier
  $`\mathbb{R} \cup \{+\infty\}`$.
- **The detonation.** A foreign arc weight of $`-\infty`$ meeting the
  tropical $`\bar{0} = +\infty`$ under path extension
  ($`\otimes = +`$) manufactures NaN:
  $`(+\infty) + (-\infty) = \mathrm{NaN}`$ — which then defeats
  $`\min`$-based path selection downstream (NaN compares as neither smaller
  nor larger), silently corrupting every shortest-distance answer over the
  composed machine; on paths using the checked constructor it instead
  panicked inside `TropicalWeight::new`.
- **The fix (landed, commit `9d86eaf`).** Every ingestion site — capture
  expansion, import, composition expansion — validates with
  `TropicalWeight::is_valid_raw` (finite **or** $`+\infty`$; NaN *and*
  $`-\infty`$ rejected) and classifies violations as
  `InvalidProviderOutput` → `PROVIDER_ERROR`. The regression test
  `negative_infinity_tropical_weight_is_rejected_not_poisoned` pins both
  the import path (clean rejection, no panic) and the composition path
  (expansion fails with a status, never yields a NaN arc).
- **The lesson, generalized.** *Representation validity is per-domain, not
  merely "not NaN".* Each of the seven family weight domains has its own
  carrier predicate (see the
  [weight-domain table](../api/c-abi-reference.md#weight-domains--semirings));
  a consumer must enforce the predicate of the domain it consumes at every
  ingestion site. The remaining tightening — the *builder* entry points
  still accept a `-INFINITY` literal and surface it as a caught `PANIC`
  rather than `INVALID_ARGUMENT` — is tracked under the same finding in the
  [bindings findings ledger](../scientific-ledger/bindings-findings-ledger.md).

## Panic containment

The family law — **no unwinding crosses `extern "C"`, in either
direction** — is implemented here exactly as the
[canon records](https://github.com/vinary-tree/liblevenshtein-rust/blob/master/vinary-tree-interop/docs/security-model.md#3-the-panic-and-exception-containment-law):

- **Every `lling_*` entry point** runs its body inside `boundary()`
  (`src/ffi.rs`): `catch_unwind(AssertUnwindSafe(..))` converts any Rust
  panic into `LLING_STATUS_PANIC` plus a thread-local diagnostic. A panic
  therefore never unwinds into C. `AssertUnwindSafe` is sound here because
  no lling-owned invariant outlives a caught panic unrepaired: builder and
  handle state either completed a step or is still valid (the documented
  half-update on the `-INFINITY` builder path leaves a *valid* builder —
  wrong only by policy, which is exactly what the LLING-B2 tightening
  closes).
- **Exported vtable callbacks** (`retain`, `release`, `query_interface`,
  and the five WFST operations in `src/bindings.rs`) are written to be
  panic-free by construction: null checks first, total status decoding,
  poison-absorbing lock acquisition, checked arithmetic, and internal
  errors returned as `VT_STATUS_PROVIDER_ERROR`. Should a defect defeat
  that discipline anyway, Rust's `extern "C"` abort shim ends the process —
  memory-safe, availability-fatal — rather than corrupting a foreign
  caller. `retain` and `release` have no status channel and are infallible
  by construction (atomic reference-count operations).
- **Symmetrically**, a foreign provider owes the same containment at its
  own vtable edge; lling-llang cannot catch a C++ exception thrown across a
  callback. The status channel exists precisely so contained provider
  faults arrive as `VtStatus` values — which lling-llang then decodes,
  range-checks, and maps totally into `LlingStatus`
  ([status table](../api/c-abi-reference.md#status-codes)).

## Threading trust

Serialization by default, parallelism by claim (family §4): a captured
provider that does not claim `PARALLEL_REENTRANT` is called through a
per-input serial gate — the gate's domain is *that captured provider*, not
the resource or the process, so independent inputs proceed concurrently. A
provider that claims the flag falsely corrupts only itself: lling-llang
shares no mutable memory with providers, and racy garbage re-enters through
the same validation as any other hostile output — wrong results, never
wrong memory. lling-llang's own exported claim of `PARALLEL_REENTRANT` is
kept by immutable payloads and lock-scoped caches, with no foreign callback
ever executed under a write lock
([design details](../architecture/resource-abi.md#concurrency-deliberately-no-resource-wide-gate)).

## Residual assumptions (explicit)

Inherited from the family model and restated so nobody relies on more:

1. Vtable and context pointers of a retained resource remain valid and
   callable — unverifiable in-process; a provider that violates it causes
   undefined behavior in any consumer, in any family library.
2. Refcount balance: `lling_resource_release` must be called exactly once
   per owned retain. Underflow is a provider-side refcount corruption;
   leaks are availability-class.
3. In-process isolation is a non-goal: a provider can exhaust memory or
   block a thread. Deployments needing hard bounds sandbox the provider
   out-of-process (family §8).

## References

- [Family security model](https://github.com/vinary-tree/liblevenshtein-rust/blob/master/vinary-tree-interop/docs/security-model.md)
  — the normative trust zones, containment law, claim analysis, exhaustion
  vectors, and non-goals this document instantiates.
- [Bindings findings ledger](../scientific-ledger/bindings-findings-ledger.md)
  — LLING-B2 (F1), the confirmed ingestion finding, with evidence and
  verification.
- [Collins 1960](../BIBLIOGRAPHY.md#ref-collins1960) — reference counting,
  the discipline behind retain/release balance.
- [Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002) — the WFST composition
  semantics whose integrity the weight validation protects.
