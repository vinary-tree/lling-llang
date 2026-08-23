# Resource ABI architecture — the scalar-WFST binding layer

How lling-llang produces, captures, and lazily composes WFSTs across the
vinary-tree family's C ABI. `src/bindings.rs` implements four cooperating
pieces — the `ScalarWfstProvider` trait, the `OwnedWfstResource` producer,
the `CapturedWfst` consumer, and the `CompositionResource` lazy product with
its `ProductRegistry` — all speaking the family's two-word `VtResource`
protocol with the `vt.scalar-wfst.1` interface. This document explains the
model, the laws each piece upholds, the product-state mathematics, the
capture-once semantics, the raw-`u32` status wire, and the deliberate
concurrency design (no resource-wide gate).

The C-callable surface over this layer is documented in the
[C ABI reference](../api/c-abi-reference.md); the family-neutral base
protocol is normative in the
[interop ABI reference](https://github.com/vinary-tree/vinary-tree-interop/blob/master/docs/abi-reference.md);
the adversarial analysis lives in the
[ABI trust model](../security/abi-trust-model.md).

---

## Terms & symbols

Symbols link to [`NOTATION.md`](../NOTATION.md); conventions in
[`STYLE.md`](../STYLE.md).

| Symbol / term | Meaning |
|---|---|
| $`T_i = (Q_i, \Sigma, q_0^{(i)}, F_i, E_i, \rho_i)`$ | The $`i`$-th component WFST: states, alphabet, start, finals, arcs, final-weight function. |
| $`\circ`$ | Composition: $`T_1 \circ T_2`$ matches $`T_1`$'s output tape against $`T_2`$'s input tape. |
| $`\Phi`$ | The epsilon-filter state set $`\{\varnothing, \epsilon_1, \epsilon_2\}`$ (sequencing filter, [Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)). |
| $`\varepsilon`$ | The empty label; on the wire, a presence flag of zero. |
| $`\oplus, \otimes, \bar{0}, \bar{1}`$ | Tropical semiring operations and identities: $`\min`$, $`+`$, $`+\infty`$, $`0`$. |
| **provider** | Any implementation behind a `VtResource` that answers the `vt.scalar-wfst.1` callbacks. |
| **capture** | Taking the provider's snapshot exactly once and holding one retain on it for the consumer's lifetime. |
| **snapshot** | The immutable revision produced by the `snapshot` callback; state identifiers are scoped to it. |
| **product state** | One triple $`(q_1, q_2, \phi)`$ of the lazy composition. |
| **retain / release** | The reference-counting pair every owned copy of a resource must balance ([Collins 1960](../BIBLIOGRAPHY.md#ref-collins1960)). |

## The model in one picture

![Component diagram of CompositionResource: the exported parallel/reentrant vt.scalar-wfst.1 resource contains the composition engine with its ProductRegistry, product-state cache, and epsilon filter, plus the two captured inputs; each captured input holds a retained foreign snapshot behind the trust boundary and pages arcs through leased 256-arc buffers.](../diagrams/architecture/composition-product-component.svg)

*Yellow = lling-llang engine; green = retained `VtResource` handles; red =
foreign snapshots across the trust boundary; amber = leased page memory.*

<details><summary>Text view</summary>

```text
exported vt.scalar-wfst.1 (PARALLEL_REENTRANT | IMMUTABLE | LAZY)
└─ CompositionResource
   ├─ left:  CapturedWfst ── retained snapshot A (foreign) ──▶ arc pages (≤256)
   ├─ right: CapturedWfst ── retained snapshot B (foreign) ──▶ arc pages (≤256)
   ├─ ProductRegistry  ids:(q₁,q₂,φ)→n  states:n→(q₁,q₂,φ)   (RwLock)
   ├─ cache            HashMap<u64, Arc<StateData>>           (RwLock)
   └─ EpsilonFilter    Φ = {∅, ε₁, ε₂}  (sequencing)
```

</details>

## Formal model: the lazy product

### Product state space

The composition of two captured WFSTs ranges over epsilon-filtered pairs:

```math
Q_\circ \;\subseteq\; Q_1 \times Q_2 \times \Phi,
\qquad
\Phi = \{\varnothing,\; \epsilon_1,\; \epsilon_2\},
\qquad
q_0^{\circ} = \bigl(q_0^{(1)},\, q_0^{(2)},\, \varnothing\bigr).
```

Only *discovered* triples exist: the registry starts with the start triple
and grows one entry per successor registered during expansion, so at any
instant $`\lvert Q_\circ\rvert \le 3\,\lvert Q_1\rvert \, \lvert Q_2\rvert`$
and in practice only the reachable, traversed region is ever materialized.

### Moves and weights

From product state $`(q_1, q_2, \phi)`$ the sequencing filter admits three
move kinds, with successor filter states
$`\varnothing \xrightarrow{\text{any}} \cdot`$,
$`\epsilon_1 \not\to \text{right-}\varepsilon`$,
$`\epsilon_2 \not\to \text{left-}\varepsilon`$ (blocking the duplicate
$`\varepsilon`$-interleavings that would otherwise multiply paths):

```math
\begin{aligned}
\textbf{left-}\varepsilon:\;& q_1 \xrightarrow{\,i : \varepsilon / w_1\,} q_1'
  &&\Longrightarrow\;
  (q_1, q_2, \phi) \xrightarrow{\,i : \varepsilon / w_1\,} (q_1', q_2, \epsilon_1),
\\
\textbf{right-}\varepsilon:\;& q_2 \xrightarrow{\,\varepsilon : o / w_2\,} q_2'
  &&\Longrightarrow\;
  (q_1, q_2, \phi) \xrightarrow{\,\varepsilon : o / w_2\,} (q_1, q_2', \epsilon_2),
\\
\textbf{match}:\;& q_1 \xrightarrow{\,i : m / w_1\,} q_1',\;\;
  q_2 \xrightarrow{\,m : o / w_2\,} q_2'
  &&\Longrightarrow\;
  (q_1, q_2, \phi) \xrightarrow{\,i : o \,/\, w_1 \otimes w_2\,} (q_1', q_2', \varnothing).
\end{aligned}
```

Weights extend tropically ($`w_1 \otimes w_2 = w_1 + w_2`$), and a product
state is final exactly when both components are, with

```math
\rho_\circ(q_1, q_2, \phi) \;=\; \rho_1(q_1) \otimes \rho_2(q_2)
\;=\; \rho_1(q_1) + \rho_2(q_2).
```

This is the classical epsilon-filter composition of
[Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002) /
[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009), in the lazy, cache-as-you-go
style of OpenFst's delayed composition
([Allauzen 2007](../BIBLIOGRAPHY.md#ref-allauzen2007)) — expressed over the
ABI's paged arc protocol instead of native arc iterators. The native
(non-ABI) counterpart is documented in
[Composition](../algorithms/composition.md).

### The ProductRegistry bijection

`ProductRegistry` maintains a bijection between discovered triples and the
dense `u64` identifiers the ABI exposes:

```math
\mathrm{ids} : Q_\circ \rightarrow \{0, 1, \dots, n-1\},
\qquad
\mathrm{states} = \mathrm{ids}^{-1},
\qquad
\mathrm{ids}\bigl(q_0^{\circ}\bigr) = 0,
```

with two invariants the implementation preserves under its `RwLock`:

1. **Stability** — once assigned, an identifier never changes or is reused;
   `states` only grows (`register` returns the existing id for a known
   triple).
2. **Density** — identifiers are consecutive from 0, so `num_states` can
   report the *discovered-so-far* count (flagged unknown, since traversal may
   discover more).

`register` is the only overflow point: more than $`2^{64}-1`$ discovered
triples is a `RepresentationLimit` (unreachable in practice — the registry
would exhaust memory first).

## Capture-once-per-input semantics

`CapturedWfst::capture(resource)` performs, per input, exactly this
sequence — once, at construction:

1. **Discover** `vt.scalar-wfst.1` on the *live* resource
   (`query_interface`, minimum version 1) and validate the returned vtable:
   `struct_size` at least the known layout, `abi_version` equal, all five
   operations present, `unit_domain == UNICODE_SCALAR`,
   `weight_domain == TROPICAL_F64`.
2. **Snapshot** through the provider's `snapshot` callback — the single
   point where the mutable-world revision is pinned. The returned resource
   owns one retain, held until the capture is dropped.
3. **Re-discover** the interface on the snapshot (the snapshot is its own
   resource and may expose a different vtable instance), and read `start`.

After construction the live input is never touched again: the composition
holds only snapshot retains, so callers may release their input retains
immediately and in any order. During traversal **zero** further snapshots
are taken; state expansion goes straight to the captured snapshot's
`state_info`/`state_arcs`. The unit test
`composition_construction_retains_inputs_without_expanding_them` pins the
strongest form: after `compose`, both providers have observed **no** state
expansion at all — construction is $`O(1)`$ in the size of both inputs.

![Sequence diagram of lling_wfst_compose: the caller enters the catch_unwind boundary, each input is discovered and snapshotted exactly once through its call gate, the registry is seeded with the start triple, and the handle returns without expanding any component state; the first traversal then expands component states through validated, paged callbacks and registers successor product states.](../diagrams/architecture/wfst-import-compose-sequence.svg)

*Yellow = lling-llang; red = foreign providers; amber = leased page buffers;
grey = the caller.*

<details><summary>Text view</summary>

```text
Construction (once per compose call)
  caller ──▶ lling_wfst_compose(first, second, &out)      [catch_unwind]
    for each input:  query_interface ▸ validate vtable
                     snapshot ▸ one retained immutable revision   ◀ once!
                     query_interface(snapshot) ▸ start
    registry := {(q₀⁽¹⁾, q₀⁽²⁾, ∅) ↦ 0};  cache := ∅
  ◀── OK + handle          (no component state expanded)

First traversal (through the exported vtable)
  state_arcs(0) ▸ registry.get(0) ▸ left.state(q₀⁽¹⁾) ▸ state_info + paged
  state_arcs (validated per arc) ▸ right.state(q₀⁽²⁾) ▸ filter moves ▸
  register successors ▸ cache ▸ return one contiguous product page
```

</details>

## Architecture & API

### `ScalarWfstProvider` — the project-owned lazy producer trait

```rust
pub trait ScalarWfstProvider: Send + Sync + 'static {
    fn weight_domain(&self) -> VtWeightDomain { VtWeightDomain::TropicalF64 }
    fn start(&self) -> Result<u64, VtStatus>;
    fn num_states(&self) -> Result<Option<usize>, VtStatus>;
    fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus>;
}
```

The Rust-side extension point: a sibling crate (or an application) hands
lling-llang a lazy WFST by implementing four methods, and
`OwnedWfstResource::from_provider` wraps it into a `VtResource` in $`O(1)`$.
`state` returns one *complete, bounded* state — validity, finality, final
weight, and the full outgoing arc vector — which the wrapper caches and
pages out through `state_arcs`. Implementations must tolerate concurrent
calls (the wrapper advertises `PARALLEL_REENTRANT`; expensive expansion
should be local or sharded, never behind one resource-wide mutex — that
would falsify the published claim). The provider may declare any of the
seven weight domains; the declared domain selects which of the seven static
export vtables the resource answers `query_interface` with.

### `OwnedWfstResource` — one owned retain, three payloads

`OwnedWfstResource` is the producer half: it owns exactly one retain of an
immutable resource whose context is an `Arc<ResourceContext>` over one of
three payloads:

| Payload | Constructor | Weight domain | State model |
|---|---|---|---|
| **Eager** | `from_wfst(VectorWfst)` — zero-copy move (also the `build` path of the C ABI) | `TROPICAL_F64` | dense native states, arcs converted per call |
| **Composition** | `compose(first, second)` | `TROPICAL_F64` | lazy product states via the registry |
| **Provider** | `from_provider(Arc<dyn ScalarWfstProvider>)` | provider-declared (any of the 7) | provider-expanded, cached |

The reference-counting glue is `Arc` itself: the exported `retain`/`release`
callbacks are `Arc::increment_strong_count` / `Arc::decrement_strong_count`,
`Clone` retains, `Drop` releases, and `into_raw` transfers the single retain
to a raw `VtResource` (the C ABI's `lling_wfst_resource` mints new retains
by cloning first). The exported `snapshot` is identity-with-retain — the
resource **is** its own immutable revision (`IMMUTABLE` flag), so capture is
$`O(1)`$ for every consumer downstream.

`start` on an eager payload reports `InvalidArgument` when the wrapped graph
has no start state — reachable only through the Rust `from_wfst` API, since
the C builder refuses to `build` without one.

### `CapturedWfst` — the consumer half

Holds the retained snapshot (`RawOwnedResource`, released on drop), the
discovered vtable pointer (valid exactly as long as the retain — the family
vtable-validity window), the start state, the per-input call gate, and an
`RwLock` cache of expanded states. Expansion validates **everything** a
provider says (next section) and stores immutable `Arc<StateData>` entries,
so every component state crosses the ABI at most once per capture.

### `CompositionResource` — the lazy product engine

Two `Arc<CapturedWfst>` plus the registry, the product cache, and the
epsilon filter. `state(id)` is read-through:

1. product-cache hit → done ($`O(1)`$);
2. otherwise resolve the triple via the registry (unknown id = provider-side
   caller error, `InvalidProviderOutput`), expand both component states
   (their own caches make this amortized-once), then take the registry
   **write** lock only for the pure in-memory successor registration — the
   three move passes above — and finally publish into the product cache.

Expanding one product state costs $`O(d_1 + d_2 + d_1 d_2)`$ for component
out-degrees $`d_1, d_2`$ (the match pass scans label pairs); each product
state pays it at most once.

## The raw-u32 status wire

Every interop callback **returns `u32`** on the Rust side, not the `VtStatus`
enum, and every status arriving from a foreign provider is decoded before
any typed use:

```rust
fn check_status(raw: u32) -> Result<(), BindingError> {
    let Some(status) = VtStatus::from_raw(raw) else {
        return Err(BindingError::InvalidProviderOutput(
            "provider returned an out-of-range status code",
        ));
    };
    if status.is_ok() { Ok(()) } else { Err(BindingError::Provider(status)) }
}
```

The rationale is a family hardening rule: reading an out-of-range
discriminant into a `#[repr(u32)]` Rust enum is undefined behavior *before
any check could run*, so the wire type must be the raw integer and the enum
may only be constructed through the total decoder `VtStatus::from_raw`
(producers encode with `to_raw`). An out-of-range status is therefore
classified as provider **misbehavior** — `InvalidProviderOutput`, surfacing
as `PROVIDER_ERROR` — never UB. The C header keeps its `VtStatus`-typed
returns: C enums are integer-typed, so the ABI is bit-identical and the
validation obligation is Rust's alone. lling-llang's own exported callbacks
are the mirror image: internal `VtStatus` results are encoded with
`.to_raw()` at every `extern "C"` return.

## Validation at ingestion — the acceptance predicates

Foreign replies are accepted only if they satisfy explicit predicates;
everything else is `InvalidProviderOutput` (surfacing as `PROVIDER_ERROR`).
For a state-info reply $`(v, f, \rho)`$ and an arc page
$`(\mathit{written}, \mathit{total})`$ requested at offset
$`\mathit{start}`$ with capacity $`c`$:

```math
\begin{aligned}
\textbf{state\_info}:\;&
  v \le 1 \;\land\; f \le 1 \;\land\; \mathrm{valid\_tropical}(\rho),
\\
\textbf{page}:\;&
  \mathit{written} \le c
  \;\land\; \mathit{start} \le \mathit{total}
  \;\land\; \mathit{start} + \mathit{written} \le \mathit{total}
  \;\land\; (\mathit{written} > 0 \lor \mathit{start} = \mathit{total}),
\\
\textbf{arc}:\;&
  \mathit{has\_in} \le 1 \;\land\; \mathit{has\_out} \le 1
  \;\land\; \mathit{reserved} = 0^{6}
  \;\land\; \mathrm{valid\_tropical}(w)
  \;\land\; \text{present labels are Unicode scalars},
\end{aligned}
```

where the weight predicate is **tropical validity**, not a mere NaN test:

```math
\mathrm{valid\_tropical}(w) \;\Longleftrightarrow\;
w \in \mathbb{R} \;\lor\; w = +\infty .
```

This is the F1/LLING-B2 hardening (commit `9d86eaf`): weights are validated
with `TropicalWeight::is_valid_raw` at **every** ingestion site — capture
expansion, import, and composition expansion — because a $`-\infty`$ that
slips a NaN-only check meets a $`+\infty`$ in composition and manufactures
NaN ($`+\infty + (-\infty) = \mathrm{NaN}`$ under IEEE-754). The regression
test `negative_infinity_tropical_weight_is_rejected_not_poisoned` pins both
paths; the finding's full record is in the
[bindings findings ledger](../scientific-ledger/bindings-findings-ledger.md).

Pages concatenate losslessly: expansion loops requesting
`VT_RECOMMENDED_ARC_BATCH` (256) arcs per call, requires `total` to stay
consistent, and accepts a state only when the concatenation reaches exactly
`total` arcs — $`\lceil \deg(q) / 256 \rceil`$ callbacks per state.

Two deliberate per-path nuances, documented as behavior (and recorded for
harmonization review):

- The **import** path (`import_tropical_wfst`) additionally requires every
  *reachable* state to report $`v = 1`$ (a dangling target is provider
  misbehavior), maps labels outside the Unicode scalar range to
  `RepresentationLimit` (`LIMIT_EXCEEDED`) rather than
  `InvalidProviderOutput`, and caps the copy at $`2^{32}-1`$ states — the
  native `StateId` width.
- The import page loop omits the $`\mathit{start} + \mathit{written} \le
  \mathit{total}`$ conjunct; an overshooting final page is still rejected,
  one iteration later, by the $`\mathit{start} \le \mathit{total}`$ check.

## Concurrency: deliberately no resource-wide gate

The design goal is stated at the top of `src/bindings.rs`: *"lazy
composition uses independently computed/cacheable product states rather than
a resource-wide sequential call gate."* Concretely:

- **The exported claim.** Every resource lling-llang exports advertises
  `PARALLEL_REENTRANT | IMMUTABLE | LAZY`. That first flag is a *claim
  consumers will rely on* — wrapping the whole resource in one mutex would
  falsify it and serialize every downstream consumer of every composed
  pipeline.
- **Gates are per captured input, and only where required.** A foreign
  provider that does **not** advertise `PARALLEL_REENTRANT` gets a
  `ProviderCallGate::Serial` — a mutex around *that provider's* callbacks
  only, created per captured input. A parallel/reentrant provider's gate is
  a no-op. Two captured inputs never share a gate; the product layer adds
  none of its own.
- **Caches are read-through `RwLock` maps with first-writer-wins
  publication.** Readers share; a miss expands *outside* the write lock and
  publishes with `entry(..).or_insert_with(..)`, so a racing duplicate
  expansion is discarded in favor of the first — benign, since expansions of
  an immutable snapshot are deterministic (equal data, and the ABI exposes
  values, not pointer identity).
- **No foreign callback ever runs under a write lock.** In
  `CompositionResource::state`, both component expansions complete *before*
  the registry write lock is taken; the locked section is pure in-memory
  successor registration. This is the deadlock-freedom keystone (invariant
  family LLING-COMP; a reentrant provider callback that re-entered the
  composition could otherwise deadlock on the registry).
- **Poisoning is absorbed.** Every lock acquisition uses
  `unwrap_or_else(PoisonError::into_inner)`: a panic in some other thread
  (already converted to a status at the ABI boundary) must not wedge
  subsequent callers — the protected data is either the pre-insertion state
  or a completed insertion, both valid.

The trade-off is explicitly *space for parallelism*: caches may transiently
hold a few identical `Arc<StateData>` allocations built by racing threads
(all but one immediately dropped), in exchange for wait-free reads on the
hot path and linear scaling of independent product-state expansions.

## Relation to the library

- Feature flags: `bindings-core` enables `src/bindings.rs` (and pulls the
  `vinary-tree-interop` types); `ffi` builds the C surface over it;
  `native-bindings-full` is the packaging alias. The crate builds as
  `rlib`, `cdylib`, and `staticlib`.
- The native, semiring-generic composition (with all filter variants) lives
  in [`composition`](../algorithms/composition.md); the ABI layer reuses its
  `EpsilonFilter` with the sequencing policy and specializes weights to
  tropical `f64`.
- Sibling repositories connect here: libdictenstein dictionaries become
  WFSTs in duallity, which exports `vt.scalar-wfst.1` resources this layer
  captures and composes — see the
  [family cross-links](../integration/README.md#family-resource-abi-cross-links).

## References

- [Mohri 2002](../BIBLIOGRAPHY.md#ref-mohri2002) — WFSTs in speech
  recognition: the transducer model and composition semantics.
- [Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) — the epsilon-filter
  composition algorithm implemented by the product engine.
- [Allauzen 2007](../BIBLIOGRAPHY.md#ref-allauzen2007) — OpenFst: the
  delayed (lazy) composition and arc-iteration design the ABI mirrors.
- [Collins 1960](../BIBLIOGRAPHY.md#ref-collins1960) — reference counting:
  the retain/release discipline behind `VtResource` and `Arc`.
