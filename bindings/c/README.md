# lling-llang C binding

The C17/C23 facade is the normative native boundary for building immutable
Unicode-scalar weighted finite-state transducers (WFSTs), importing and lazily
composing family WFST resources, and exporting retained `vt.scalar-wfst.1`
resources. The public surface is [`lling_llang.h`](../../include/lling_llang.h);
the exact preconditions and status sets are in the
[C ABI reference](../../docs/api/c-abi-reference.md).

## Installation and loading

Install the staged CMake/pkg-config package or build the FFI artifacts from
source. A source-layout build and the executable example use:

```sh
cargo build --no-default-features --features ffi
cc -std=c17 -Wall -Wextra -Werror \
  -Iinclude -I../liblevenshtein-rust/vinary-tree-interop/include \
  bindings/c/examples/compose_demo.c -Ltarget/debug -llling_llang \
  -Wl,-rpath,"$PWD/target/debug" -o target/lling-c-compose
target/lling-c-compose
```

At runtime, require exact `lling_abi_version()` equality and an
`lling_api_revision()` at least as new as the header. Never infer compatibility
from a shared-library filename.

## Executable conformance evidence

[`compose_demo.c`](examples/compose_demo.c) is compiled and run by CI. It builds
`a:x/0.5` and `x:z/0.25`, composes them, discovers the scalar-WFST interface,
observes the product arc `a:z/0.75`, and balances every owner. The four-library
family pipeline in duallity independently tests producer-to-adapter-to-composer
handoff.

![C ABI surface from host call through contained Rust implementation and retained family resource.](../../docs/diagrams/api/c-abi-surface.svg)

## API and data model

| Concept | Contract |
|---|---|
| `LlingWfstBuilder` | Mutable, single-owner builder. State IDs are returned by `add_state`; build consumes the graph but not the shell. |
| `LlingWfst` | Immutable WFST handle. Import copies a foreign snapshot; compose retains snapshots and expands product states lazily. |
| `VtResource` | Two-word `{context, vtable}` handle. `lling_wfst_resource` returns one owned retain implementing `vt.scalar-wfst.1`. |
| label | Optional Unicode scalar. `has_label == 0` denotes epsilon; otherwise the scalar must be valid. |
| weight | `double` interpreted under the advertised weight domain. The constructed/imported/composed C surface is tropical. |

The builder lifecycle is Open → Consumed. A failed build caused by a missing
start state leaves it Open, so the caller may repair and retry. State expansion
uses `state_info` and paged `state_arcs`; concatenate pages until the stable
`total` is reached.

## Ownership and snapshot law

- Free every builder and WFST exactly once.
- Release every resource returned by `lling_wfst_resource` exactly once.
- Builder build moves the graph in `$`\mathcal{O}(1)`$; the builder shell still
  requires `lling_wfst_builder_free`.
- Composition captures one immutable snapshot per input at construction. Input
  resources can then be released in any order; the product owns independent
  retains.
- Callback and page buffers are borrowed only for the documented call.

The complete capture/import/compose sequence is illustrated in
[`wfst-import-compose-sequence.svg`](../../docs/diagrams/architecture/wfst-import-compose-sequence.svg).

## Errors and failure containment

Every fallible function returns `LlingStatus`. Branch on the enum and copy
`lling_last_error_message()` immediately; its storage is thread-local and may be
replaced by the next call on that thread. Invalid arguments, null pointers,
contained panics, incompatible resources, provider faults, representation
limits, and consumed builders remain distinct. Rust panics never unwind through
C.

## Concurrency and reentrancy

Confine a mutable builder to one thread. Immutable WFSTs, retained snapshots,
and distinct handles are reentrant; do not concurrently free the same handle.
Foreign providers are serialized by default unless they explicitly advertise
parallel reentrancy. Lazy product publication is shared without a
resource-wide traversal lock.

## Performance and marshalling

Resource export and composition capture are `$`\mathcal{O}(1)`$ retain
operations. Import is `$`\mathcal{O}(|Q|+|E|)`$ because it validates and copies
every reachable state and arc. Prefer retained handoff to serialization, reserve
builder states when cardinality is known, and fetch arcs in the recommended
batch size. Do not optimize away interface/version validation at a foreign
provider boundary.

## Security and provider trust

Treat provider vtables, counts, state IDs, labels, weights, and page totals as
untrusted. The native boundary checks nullability, scalar validity, weight
domain, monotone paging, bounded allocation, and provider statuses. See the
[ABI trust model](../../docs/security/abi-trust-model.md) for the complete
threat matrix and containment rules.

## Compatibility and troubleshooting

The project ABI, project API revision, family ABI, interface version, and
package version are independent counters. If loading fails, check the native
artifact's OS/CPU, the bundled interop header/version, loader search path, and
package pins in that order. An incompatible-resource result usually means the
resource lacks Unicode-scalar `vt.scalar-wfst.1` or advertises another weight
domain.

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json) before changing a symbol or package.
2. Extend the C reference, this guide, negative tests, and executable example.
3. Run `python3 scripts/check-bindings.py` and
   `python3 scripts/check-binding-docs.py`.
4. Render PlantUML headlessly and verify local links and GitHub-safe math.
5. Stage a native package and test both shared and static consumers.
