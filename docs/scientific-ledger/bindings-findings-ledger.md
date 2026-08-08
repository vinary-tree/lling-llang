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
| [LLING-B2](#finding-lling-b2) | 2026-08-08 | `src/bindings.rs`, `src/ffi.rs` (ABI weight ingestion) | abi-input-validation | high | scheduled wave W4 [LLING-BRIDGE-4] | OPEN |
| [LLING-B3](#finding-lling-b3) | 2026-08-08 | release tagging vs. family version pins | version-coherence | medium | ledger-only (releases out of scope) | RECORDED |

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
| Status | OPEN |

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

**Fix (scheduled, not in this wave).** Per-domain `repr_ok` rejection at every
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
