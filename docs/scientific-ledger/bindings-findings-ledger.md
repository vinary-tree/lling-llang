# Bindings findings ledger — lling-llang

Append-only scientific record of defects, contract gaps, and version-coherence
findings in the lling-llang binding stack (the `lling_*` C ABI in `src/ffi.rs`,
the `vt.scalar-wfst.1` resource layer in `src/bindings.rs`, the C/C++ headers,
the JavaScript/TypeScript/ClojureScript facades, and the release machinery).
Every entry uses the uniform family schema:

```
Finding LLING-B<N> | date | component | class | severity | evidence | analysis | fix | verification | status
```

Statuses: `OPEN` (confirmed, fix scheduled), `FIXED` (fix landed, cite commit),
`RECORDED` (ledger-only by policy — no code change in this effort). Entries are
never rewritten; corrections append. Line references are pinned to baseline
commit `0fc05f0` (2026-08-08) unless noted otherwise.

## Index

| Finding | Date | Component | Class | Severity | Fix | Status |
|---|---|---|---|---|---|---|
| [LLING-B1](#finding-lling-b1) | 2026-08-08 | `.github/workflows/release-bindings.yml` | release-integrity | high | commit `988ea09` | FIXED |
| [LLING-B2](#finding-lling-b2) | 2026-08-08 | `src/bindings.rs`, `src/ffi.rs` (ABI weight ingestion) | abi-input-validation | high | `9d86eaf` (tropical capture/import); `83f9595` (builder surface); trait site + Rocq proof W4 | FIXED (builder + tropical paths) |
| [LLING-B3](#finding-lling-b3) | 2026-08-08 | release tagging vs. family version pins | version-coherence | medium | ledger-only (releases out of scope) | RECORDED |
| [LLING-B4](#finding-lling-b4) | 2026-08-09 | `src/ffi.rs` `lling_wfst_builder_build` | resource-lifecycle | medium | `83f9595` | FIXED |
| [LLING-B5](#finding-lling-b5) | 2026-08-09 | `src/ffi.rs` `lling_wfst_import` / `lling_wfst_compose` | resource-leak | high | `b1acb7e` | FIXED |
| [LLING-B6](#finding-lling-b6) | 2026-08-09 | `src/ffi.rs` `lling_wfst_builder_add_state` | state-mutation-on-failure | low | `b1acb7e` | FIXED |
| [LLING-B7](#finding-lling-b7) | 2026-08-09 | `src/bindings.rs` `import_tropical_wfst` paging | abi-paging (F3 lling side) | medium | `b1acb7e` | FIXED |
| [LLING-B8](#finding-lling-b8) | 2026-08-09 | label `> char::MAX` status (import vs expansion) | status-mapping-consistency | low | to be arbitrated by `StatusMapping.v` (#20) | UNDER REVIEW |
| [LLING-B9](#finding-lling-b9) | 2026-08-09 | wire vs native finality at `+∞` weight | contract-nuance | info | ledger-only (documented contract) | RECORDED |
| [LLING-B10](#finding-lling-b10) | 2026-08-09 | `.github/workflows/ci.yml` `rust` job | ci-integrity | high | `f84f784` | FIXED |
| [LLING-B11](#finding-lling-b11) | 2026-08-09 | `apiRevision` policy for the `−∞` status tightening | version-coherence | info | ledger-only (recorded decision) | RECORDED |

---

## Finding LLING-B1

| Field | Value |
|---|---|
| Finding | LLING-B1 (pre-registered family defect list, lling item 2) |
| Date | 2026-08-08 |
| Component | `.github/workflows/release-bindings.yml`, `native` job, CMake package-test step |
| Class | release-integrity (hardcoded derived value) |
| Severity | high — breaks every native release leg after any version bump |
| Fix | commit `988ea093210ea5bfaa9e84078dd85c3ceb6ce263` — `ci(release-bindings): derive staged package version from Cargo.toml` |
| Verification | `python3 scripts/check-bindings.py` guard 4; pre-fix run FAILs on exactly this check, post-fix run reports 6/6 PASS |
| Status | FIXED |

**Evidence.** At baseline `0fc05f0`, the workflow's package-test step hardcoded
the staged prefix:

```yaml
prefix="$PWD/dist/lling-llang-0.2.0-${{ matrix.target }}"     # workflow, was line 59
```

while the staging step it consumes derives the same directory name from the
manifest (`scripts/stage-native-package.sh:12-13`):

```bash
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
package_name="lling-llang-${version}-${target}"
```

**Analysis.** The two sides of one release pipeline computed the artifact name
from different sources: the stage script from `Cargo.toml`, the test step from
a string literal frozen at `0.2.0`. On the first release cut after a version
bump, staging writes `dist/lling-llang-<new>-<target>` while the CMake test
points `CMAKE_PREFIX_PATH` at the nonexistent `dist/lling-llang-0.2.0-<target>`,
failing every native leg — or worse, silently validating a stale artifact if
one survives in the workspace. A derived value must never be re-derived by
hand at a second site.

**Fix.** The workflow now derives `version` with the byte-identical `sed`
invocation the stage script uses, so both sides always agree:

```yaml
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
prefix="$PWD/dist/lling-llang-${version}-${{ matrix.target }}"
```

**Verification.** Guard 4 of `scripts/check-bindings.py` fails on any
`dist/lling-llang-<semver>` literal in the workflow and requires the
Cargo.toml-derived form. Scientific loop closed in-session on 2026-08-08:
gate run against the pre-fix tree reported exactly one failing check
(`[FAIL] workflow version-derivation guard [LLING-B1]`, exit 1); after commit
`988ea09` the gate reports `6/6 checks passed` (exit 0). The guard is a
permanent regression fence.

---

## Finding LLING-B2

| Field | Value |
|---|---|
| Finding | LLING-B2 (pre-registered family finding **F1**) |
| Date | 2026-08-08 |
| Component | `src/bindings.rs` (capture/expand, composition, import, provider trait), `src/ffi.rs` (builder weight entry points) |
| Class | abi-input-validation — domain predicate too weak at a trust boundary |
| Severity | high — semiring-domain contract violation ($`\mathrm{NaN}`$ egress) plus a panic path reachable from foreign input |
| Fix | scheduled wave W4 under invariant [LLING-BRIDGE-4]; formal home `proofs/coq/abi/WeightBridge.v` (obligation #16) |
| Verification | planned: adversarial correspondence tests (`tests/abi_weight_bridge_correspondence.rs`) + Rocq `repr_ok` theorems; code sites verified by inspection 2026-08-08 at `0fc05f0` (exact lines below) |
| Status | FIXED (tropical ingestion paths); builder-surface + provider-trait sites and the Rocq proof land W4 |

**Evidence.** The verified tropical domain is
$`\mathbb{R} \cup \{+\infty\}`$: `TropicalWeight::is_valid_raw`
(`src/semiring/basic/tropical.rs:46-48`) accepts a raw `f64` iff it is finite
or positive infinity, and `TropicalWeight::new`
(`src/semiring/basic/tropical.rs:56-58`) enforces it by panicking
(`.expect("tropical weight must be finite or +infinity")`) on $`-\infty`$ and
$`\mathrm{NaN}`$. Every ABI ingestion site, however, tests only
`is_nan()` — so $`-\infty`$, which is *not* in the domain, is admitted:

| # | Channel | Site (at `0fc05f0`) | What it does |
|---|---|---|---|
| 1 | capture/expand | `src/bindings.rs:262` — `if valid > 1 \|\| is_final > 1 \|\| final_weight.is_nan()` | admits a $`-\infty`$ final weight from a foreign `state_info` |
| 2 | capture/expand | `src/bindings.rs:304` — `\|\| arc.weight.is_nan()` | admits $`-\infty`$ arc weights from a foreign `state_arcs` page |
| 3 | composition | `src/bindings.rs:392` — `left.final_weight + right.final_weight` | raw IEEE-754 addition of two captured final weights |
| 4 | composition | `src/bindings.rs:455` — `weight: left_arc.weight + right_arc.weight` | raw IEEE-754 addition of two matched arc weights |
| 5 | import | `src/bindings.rs:970` — `\|\| !final_weight.is_finite() && !final_weight.is_infinite()` | rejects only $`\mathrm{NaN}`$ (a $`-\infty`$ value *is* infinite, so it passes) |
| 6 | import | `src/bindings.rs:981` — `TropicalWeight::new(final_weight)` | **panics** on the admitted $`-\infty`$ |
| 7 | import | `src/bindings.rs:1007` — `\|\| arc.weight.is_nan()` | admits $`-\infty`$ arc weights |
| 8 | import | `src/bindings.rs:1049` — `TropicalWeight::new(arc.weight)` | **panics** on the admitted $`-\infty`$ |
| 9 | builder | `src/ffi.rs:200` (`set_final`) and `src/ffi.rs:263` (`add_arc`) — `if weight.is_nan()` | NaN-only rejection at the C builder surface |
| 10 | builder | `src/ffi.rs:210` and `src/ffi.rs:274` — `TropicalWeight::new(weight)` | **panics** on $`-\infty`$; surfaces as `LLING_STATUS_PANIC` instead of `LLING_STATUS_INVALID_ARGUMENT` |
| 11 | provider trait | `src/bindings.rs:500-506` (`ProviderResource::state`) | copies in-process provider weights with no validation at all |

**Analysis.** Two distinct failure modes follow from the same weak predicate.

*Mode 1 — $`\mathrm{NaN}`$ manufactured inside composition (the pre-registered
F1 mechanism).* Sites 1-2 admit $`-\infty`$ into a `CapturedWfst`; sites 3-4
then combine weights with raw `f64` addition, bypassing `TropicalWeight`
entirely. Since $`+\infty`$ is a *legitimate* tropical value (the additive
identity $`\bar{0}`$, meaning "unreachable"), the poisoned operand meets a
legal one:

```math
(+\infty) + (-\infty) = \mathrm{NaN} \quad \text{(IEEE-754)}
```

Minimal reproducer: compose a left resource whose matched arc carries weight
$`+\infty`$ with a right resource whose matched arc carries weight
$`-\infty`$; the product arc at site 4 has weight $`\mathrm{NaN}`$. That
$`\mathrm{NaN}`$ egresses through lling-llang's *own* vtable
(`wfst_state_info` writes the raw final weight at `src/bindings.rs:774`;
`wfst_state_arcs` copies the arc page at `src/bindings.rs:804-811`) on a
resource that advertises `weight_domain = TropicalF64` — so lling-llang
violates the exact domain contract its own ingestion enforces against others
(`src/bindings.rs:262/304` reject $`\mathrm{NaN}`$ as
`InvalidProviderOutput`). Any downstream capture of the composed product —
including a second lling-llang instance — then fails, poisoning the pipeline
one hop away from the culprit.

*Mode 2 — panic instead of status.* Sites 5-8 admit $`-\infty`$ past the
import checks and then construct through the panicking `TropicalWeight::new`.
For C callers of `lling_wfst_import` the panic is downgraded by the
`boundary()` `catch_unwind` (`src/ffi.rs:75-84`) to `LLING_STATUS_PANIC` — the
wrong status class for well-formed-but-out-of-domain input, and one a hostile
provider can trigger at will. For pure-Rust callers of the public
`import_tropical_wfst` (`src/bindings.rs:932`) it is an uncaught panic — a
denial-of-service vector from foreign data. Sites 9-10 are the same pattern on
the builder surface (caller error, not foreign data, but the same wrong
status). Site 11 is the trusted in-process trait channel; it should be
harmonized under the same predicate when W4 lands.

**Fix (tropical paths landed — commit `9d86eaf`).** The composition
(`expand_state`) and import (`import_tropical_wfst`) paths are tropical-only
(`discover_wfst` rejects other domains), so all four weight-ingestion sites on
those paths (sites 1-2, 5, 7) now check `TropicalWeight::is_valid_raw`
(finite $`\lor`$ $`+\infty`$) instead of `is_nan` — a $`-\infty`$ weight is
rejected as `InvalidProviderOutput` before it can enter a capture, so sites
3-4 (the raw-`f64` composition additions) are safe by construction and can no
longer manufacture $`\mathrm{NaN}`$. Regression:
`negative_infinity_tropical_weight_is_rejected_not_poisoned` drives a
$`-\infty`$-arc provider through both import (rejected, no panic) and
composition (surfaced as a provider error during expansion, never a NaN arc).

**Remaining (wave W4).** Per-domain `repr_ok` rejection at EVERY
ingestion point, closed under invariant [LLING-BRIDGE-4]: for the tropical
specialization the predicate already exists as
`TropicalWeight::is_valid_raw` (finite $`\lor`$ $`+\infty`$). Sites 1-2, 5, 7
reject with `InvalidProviderOutput`; sites 9-10 reject with
`LLING_STATUS_INVALID_ARGUMENT` before construction; sites 3-4 become safe by
construction once $`-\infty`$ cannot enter a capture; site 11 adopts the same
predicate. Formal home: Rocq `proofs/coq/abi/WeightBridge.v` (obligation #16 —
`repr_ok`/decode per weight domain), with adversarial correspondence tests
mirroring each rejection lemma. Fix lands in wave W4 with those artifacts so
the change and its proof/test evidence are one reviewable increment.

**Update (2026-08-09, commit `83f9595`).** The builder-surface sites 9-10
landed: `lling_wfst_builder_set_final` and `lling_wfst_builder_add_arc` now
reject with `TropicalWeight::is_valid_raw` before construction, so a `−∞`
weight surfaces as `LLING_STATUS_INVALID_ARGUMENT` rather than the previous
`catch_unwind`-downgraded `LLING_STATUS_PANIC`. Regression:
`weight_ingestion_rejects_nan_and_negative_infinity`
(`tests/ffi_builder_matrix.rs`). Only the trusted in-process provider-trait
site (11) and the Rocq `WeightBridge.v` proof (#16) remain for W4; the general
per-domain `repr_ok` generalization is tracked under [LLING-BRIDGE-4].

---

## Finding LLING-B3

| Field | Value |
|---|---|
| Finding | LLING-B3 (family version-pin inconsistency list, lling item) |
| Date | 2026-08-08 |
| Component | git tag `v0.2.0` vs. crate/npm/CMake version pins referencing 0.2.0 |
| Class | version-coherence (tag cannot reproduce the pinned surface) |
| Severity | medium — no artifact is wrong today, but the 0.2.0 coordinate is unreproducible from its tag |
| Fix | ledger-only by plan decision #4 (releases fully out of scope for this effort); resolve at the next tagged release |
| Verification | `git ls-tree -r --name-only v0.2.0` contains zero paths under `bindings/`, no `src/ffi.rs`, no `src/bindings.rs`, no `include/lling_llang.h` (verified 2026-08-08) |
| Status | RECORDED |

**Evidence.** Tag `v0.2.0` points at `743127e23fee6647954b94b8dc6f1c3a4aa1dc79`
(2026-06-15), which predates the entire binding stack: the tagged tree contains
none of `bindings/`, `src/ffi.rs`, `src/bindings.rs`, `include/lling_llang.h`,
`include/lling_llang.hpp`, `cmake/`, `pkgconfig/`, or
`.github/workflows/release-bindings.yml`. Those files first exist at
`0fc05f01d40ddaab4b83d42946f26f81c27a7d3f` (2026-08-08). Yet the version
`0.2.0` is pinned across the family as if it named this surface:

- `Cargo.toml` — `version = "0.2.0"` (the crate carrying the new `ffi`/`bindings` modules);
- `bindings/javascript/package.json` — `"version": "0.2.0"` for `@vinary-tree/lling-llang`;
- `bindings/javascript/deps.cljs` — `:npm-deps {"@vinary-tree/lling-llang" "0.2.0"}`;
- `bindings/cpp/tests/package/CMakeLists.txt` — `find_package(lling-llang 0.2 CONFIG REQUIRED)`;
- sibling `liblevenshtein-rust/scripts/check-bindings.py` — pins `("@vinary-tree/lling-llang", "0.2.0")` (recorded here as external evidence; sibling repos own their own gates);
- `.github/workflows/release-bindings.yml` — triggers on `v*.*.*` tags, and `v0.2.0` already exists, so the binding release for 0.2.0 can never fire from its own tag.

**Analysis.** Nothing published is corrupt — no 0.2.0 binding artifact exists
on any registry. The incoherence is prospective: the coordinate
`lling-llang 0.2.0` / `@vinary-tree/lling-llang@0.2.0` is already claimed by a
tag that cannot build the surface the family pins expect from it. Publishing
0.2.0 from `master` would produce artifacts unreproducible from `v0.2.0`;
re-pointing the tag would rewrite published history (forbidden). The only
clean resolution is a new version (e.g. 0.2.1 or 0.3.0) tagged after the
binding waves land, with the family pins bumped in lockstep — release work,
which plan decision #4 places fully out of scope for this effort. Recorded so
the release owner inherits the constraint explicitly.

**Verification.** Reproduce with:

```bash
git log -1 --format='%H %ci' v0.2.0
git ls-tree -r --name-only v0.2.0 | grep -cE '^(bindings/|src/ffi\.rs|src/bindings\.rs|include/lling_llang)'   # prints 0
```

---

*Ledger created 2026-08-08 (wave W0). This file is an append-only Bucket-B
scientific record: it is deliberately excluded from
`docs/.mathlint-include.txt`, though entries are written MathJax-conformant
regardless.*

## Finding LLING-B4

| Field | Value |
|---|---|
| Finding | LLING-B4 (build consumed the builder on a null out-pointer) |
| Date | 2026-08-09 |
| Component | `src/ffi.rs` `lling_wfst_builder_build` |
| Class | resource-lifecycle (state destroyed on a validation-only failure) |
| Severity | medium — a recoverable caller error silently bricked the builder |
| Fix | commit `83f9595` (W4 test agent) |
| Verification | `build_with_null_out_pointer_preserves_builder` (`tests/ffi_builder_matrix.rs`) |
| Status | FIXED |

**Evidence.** At baseline the function took the graph before validating the
out-pointer:

```rust
let graph = builder.graph.take().ok_or(/* Closed */)?;   // consumes the builder
// ... start-state check ...
*required_mut(out_wfst, "out_wfst")? = Box::into_raw(/* ... */);   // may fail here
```

**Analysis.** `builder.graph.take()` moves the graph out of the builder before
`required_mut(out_wfst, ...)` runs. A null `out_wfst` therefore dropped the
taken graph and returned `NullPointer`, leaving the builder permanently
consumed — every subsequent call answered `Closed`. Pointer validation must
never destroy caller state.

**Fix.** The out-pointer is validated first, then the graph is taken, so a null
`out_wfst` leaves the builder fully intact. The observable failure precedence
is now builder-null → out-null → Closed → no-start.

---

## Finding LLING-B5

| Field | Value |
|---|---|
| Finding | LLING-B5 (import/compose leaked the materialized handle on a null out-pointer) |
| Date | 2026-08-09 |
| Component | `src/ffi.rs` `lling_wfst_import`, `lling_wfst_compose` |
| Class | resource-leak (RHS-before-place evaluation order) |
| Severity | high — a null out-pointer leaked heap and, for compose, two live snapshot retains |
| Fix | commit `b1acb7e` |
| Verification | `tests/ffi_out_pointer_safety.rs::compose_with_null_out_pointer_captures_nothing` and `import_with_null_out_pointer_does_no_provider_work` |
| Status | FIXED |

**Evidence.** Both functions wrote `*required_mut(out_wfst, ...)? =
Box::into_raw(Box::new(...))`. Rust evaluates the assignment's right operand
before the place, so the `Box::into_raw` fully materialized the `LlingWfst`
(for `import`, the imported graph; for `compose`, the `CompositionResource`
holding **two** captured snapshot retains) *before* `required_mut` tested the
out-pointer. On a null out-pointer the raw pointer was then dropped on the
floor — an unrecoverable leak of the handle and, for compose, of both provider
retains.

**Analysis.** Confirmed empirically (a standalone `rustc` evaluation-order
probe: RHS-first). This is the same shape as [LLING-B4], but here the leaked
value is heap the caller can never reach, and for compose it also pins two
foreign provider contexts alive forever.

**Fix.** Bind and validate the out-pointer first
(`let output = required_mut(out_wfst, "out_wfst")?;`), then materialize and
assign through it. For compose this additionally means the two snapshot
captures are never taken on the null-out path. Regressions prove, via the
metrics-instrumented in-repo provider, that the null-out path takes zero
snapshots and settles both retain ledgers to zero.

---

## Finding LLING-B6

| Field | Value |
|---|---|
| Finding | LLING-B6 (add_state added an orphan state on a null out-pointer) |
| Date | 2026-08-09 |
| Component | `src/ffi.rs` `lling_wfst_builder_add_state` |
| Class | state-mutation-on-failure |
| Severity | low — deterministic, safe, but leaves an unreachable orphan state |
| Fix | commit `b1acb7e` |
| Verification | `tests/ffi_out_pointer_safety.rs::add_state_with_null_out_pointer_adds_no_orphan` |
| Status | FIXED |

**Evidence.** `let state = graph(builder)?.add_state(); *required_mut(out_state,
...)? = state;` — the graph was mutated (a state added) before the out-pointer
was validated, so a null `out_state` returned `NullPointer` while leaving an
orphan state (and its consumed id) behind.

**Analysis.** The sibling of [LLING-B4]/[LLING-B5] on the builder surface: a
validation-only failure must not mutate the graph. Builder validity is still
checked first (`graph(builder)?`), preserving the builder → out precedence.

**Fix.** Validate the out-pointer before calling `add_state`. The regression
proves the next successful add returns the immediately following id — no orphan
consumed one.

---

## Finding LLING-B7

| Field | Value |
|---|---|
| Finding | LLING-B7 (family finding **F3**, lling side) |
| Date | 2026-08-09 |
| Component | `src/bindings.rs` `import_tropical_wfst` arc-paging loop |
| Class | abi-paging (acceptance predicate one conjunct too weak) |
| Severity | medium — an overshooting final page was accepted one iteration late |
| Fix | commit `b1acb7e` |
| Verification | full `--features "ffi test-utils"` suite green; the harmonized predicate matches `expand_state` verbatim |
| Status | FIXED |

**Evidence.** The import loop accepted a page with
`if written > page.len() || offset > total || (written == 0 && offset < total)`,
omitting the `offset + written > total` conjunct that the composition
expansion path (`expand_state`) already ran.

**Analysis.** Family finding F3 is the paging-acceptance asymmetry across the
three consumers; the proven arbiter is `ConsumerAcceptance.accepts_dec`
(`liblevenshtein-rust/docs/verification/abi/theories/ConsumerAcceptance.v`, the
llev side harmonized under LLEV-B8). On the import path a provider page whose
`written` overshoots `total − offset` was buffered and only rejected on the
next iteration (via `offset > total`), after one extra provider callback and up
to a page of extra buffered arcs. Two code paths in one crate applied two
different predicates to the one interop paging law.

**Fix.** The import loop now runs the identical `accepts_dec` predicate
(`written > page.len() || offset > total || offset.saturating_add(written) >
total || (written == 0 && offset < total)`), rejecting an overshooting final
page immediately. The lling-side acceptance predicate is now uniform with
`expand_state` and with the proven law.

---

## Finding LLING-B8

| Field | Value |
|---|---|
| Finding | LLING-B8 (label `> char::MAX` status asymmetry) |
| Date | 2026-08-09 |
| Component | `src/bindings.rs` — `import_tropical_wfst` vs `expand_state` label decoding |
| Class | status-mapping-consistency |
| Severity | low — both statuses are safe; only the code differs |
| Fix | to be arbitrated by the formal status model `proofs/coq/abi/StatusMapping.v` (obligation #20) |
| Verification | pinned by `label_beyond_char_max_pins_exact_statuses` (`tests/ffi_incompatible_resources.rs`) |
| Status | UNDER REVIEW |

**Evidence.** A foreign arc whose `input_label`/`output_label` exceeds
`char::MAX` decodes to `BindingError::RepresentationLimit` → `LIMIT_EXCEEDED`
on the import path (`char::from_u32(...).ok_or(RepresentationLimit)`), but to
`BindingError::InvalidProviderOutput` → `PROVIDER_ERROR` on the
capture/composition expansion path.

**Analysis.** Unlike [LLING-B7], there is no proof that arbitrates which status
is correct: an out-of-range label is defensibly *either* "a valid value beyond
our representable range" (`LIMIT_EXCEEDED`) *or* "a provider contract
violation" (`PROVIDER_ERROR`). Resolving it by fiat would be a judgment call
that rewrites a just-committed pin. The principled resolution is to let the
formal status model decide: obligation #20 (`StatusMapping.v`) defines the
canonical `VtStatus → LlingStatus` mapping and the ingress classification, and
the two paths will be harmonized to whichever class that model certifies, with
the pin updated to match. Deferring the *decision* to the model (within this
same wave) is not deferring the *work* — it is choosing the correct arbiter.

**Verification.** The current asymmetry is exactly pinned so the resolution is
a visible, reviewed change rather than a silent drift.

---

## Finding LLING-B9

| Field | Value |
|---|---|
| Finding | LLING-B9 (wire vs native finality at `+∞` weight) |
| Date | 2026-08-09 |
| Component | `src/ffi.rs` `lling_wfst_builder_set_final` wire semantics vs `MutableWfst::set_final` |
| Class | contract-nuance (documented) |
| Severity | informational |
| Fix | ledger-only — documented contract, no code change |
| Verification | `positive_infinity_survives_the_round_trip` (`tests/ffi_roundtrip_proptest.rs`) |
| Status | RECORDED |

**Evidence.** The C surface `lling_wfst_builder_set_final(s, +∞)` pins
`is_final = 1` at weight `+∞` on the exported wire, while the native
`MutableWfst::set_final` normalizes a `+∞` (`= zero`) final weight toward
non-final in some paths.

**Analysis.** `+∞` is the tropical additive identity (`zero` = "unreachable"),
so a state made final at weight `+∞` is final-but-unreachable — a legal but
degenerate configuration. The wire faithfully preserves what the builder was
told; the native constructor may treat zero-weight finality as non-final. This
is a semantics nuance, not a defect: the round trip is exact for every finite
and `+∞` weight, and the nuance only affects the interpretation of a
final-at-`+∞` state. Recorded so consumers relying on wire-exact finality know
the boundary; no behavior change.

---

## Finding LLING-B10

| Field | Value |
|---|---|
| Finding | LLING-B10 (the --all-features Rust CI job never checked out its sibling crates) |
| Date | 2026-08-09 |
| Component | `.github/workflows/ci.yml` `rust` job |
| Class | ci-integrity |
| Severity | high — the job could not compile on a clean GitHub runner |
| Fix | commit `f84f784` |
| Verification | the job now mirrors the `ffi` job's "Checkout dev sibling crates" step; YAML validated |
| Status | FIXED |

**Evidence.** `--all-features` enables the optional `vinary-tree-interop`,
`liblevenshtein`, `libdictenstein`, and `llattice` dependencies, all declared
as sibling path dependencies (`../liblevenshtein-rust/...`, `../libdictenstein`,
`../llattice`). The `rust` job did only `actions/checkout@v4` before running
`cargo check --all-features` / `clippy --all-features` / `test --all-features`.

**Analysis.** On a clean runner the sibling directories do not exist, so Cargo
cannot even read the path dependencies' manifests to resolve the graph — every
`--all-features` step fails before compilation. The `ffi` job already cloned the
siblings for exactly this reason; the `rust` job was missing the same step.

**Fix.** Added the identical "Checkout dev sibling crates" step (clone
`llattice`, `libdictenstein`, `liblevenshtein-rust` next to the workspace) to
the `rust` job, before the toolchain install. This is CI hygiene for the test
matrix, not release machinery (release workflows pin exact tags instead).

---

## Finding LLING-B11

| Field | Value |
|---|---|
| Finding | LLING-B11 (apiRevision policy for the `−∞` status tightening) |
| Date | 2026-08-09 |
| Component | `bindings/api.json`-class version policy vs commit `83f9595` |
| Class | version-coherence (recorded decision) |
| Severity | informational |
| Fix | ledger-only — recorded decision |
| Verification | n/a (policy record) |
| Status | RECORDED |

**Evidence.** Commit `83f9595` changed the observable status for a `−∞` builder
weight from `LLING_STATUS_PANIC` to `LLING_STATUS_INVALID_ARGUMENT`
([LLING-B2] update).

**Analysis.** This is a bug-fix tightening: the previous `PANIC` was the wrong
status class for well-formed-but-out-of-domain input, and no correct caller
could have depended on receiving `PANIC`. Under semantic-versioning-for-ABIs
practice a bug-fix that corrects an error class without changing any success
behavior does not warrant an `apiRevision` bump. Recorded decision: **no
`apiRevision` bump for this change**; the release owner inherits this note if a
different policy is later adopted. Releases are out of scope for this effort
(plan decision #4), so this is a record, not an action.
