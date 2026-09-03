# lling-llang for Python

Build and lazily compose weighted finite-state transducers (WFSTs), then let
ordinary Python classes define custom automata, lattices, and semirings that
the native lling-llang engine can consume.

A WFST is a directed graph whose arcs may consume an input label, produce an
output label, and carry a weight. A lattice supplies join and meet operations
for partially ordered values. A semiring supplies addition for alternative
paths and multiplication for consecutive path segments. lling-llang keeps
these semantic domains explicit so incompatible resources fail before
composition rather than silently producing meaningless scores.

The Python package uses the stable Vinary Tree resource ABI implemented by
`vinary-tree-interop`. Python objects remain Python-owned; the ABI passes
retained resource handles and generation-checked value tokens instead of
casting Python objects into Rust layouts.

![Python provider capture, lazy composition, traversal, and release.](https://raw.githubusercontent.com/vinary-tree/lling-llang/v4.0.0-rc.6/docs/diagrams/architecture/wfst-import-compose-sequence.svg)

## Install

Install the release candidate and its exact interop dependency from PyPI:

```sh
python -m pip install --pre lling-llang==4.0.0rc6
```

For a coordinated source checkout, build the native library and install both
Python projects:

```sh
cargo build --release --no-default-features --features python-bindings
python -m pip install -e ../vinary-tree-interop/bindings/python
LLING_LLANG_PREBUILT_LIBRARY="$PWD/target/release/liblling_llang.so" \
  python -m pip install -e bindings/python
```

Use `liblling_llang.dylib` on macOS and `lling_llang.dll` on Windows.
`LLING_LLANG_LIBRARY` selects a library at import time.
`LLING_LLANG_PREBUILT_LIBRARY` selects the library embedded while building a
wheel.

## Quickstart

`WfstBuilder` validates Unicode scalar labels and tropical weights before
crossing the native boundary. `None` denotes epsilon on either tape:

```python
import lling_llang as lling

with lling.WfstBuilder(size_hint=2) as builder:
    source = builder.add_state()
    target = builder.add_state()
    builder.set_start(source).set_final(target, weight=0.0)
    builder.add_arc(source, "a", "b", target, weight=0.25)
    graph = builder.build()

with graph:
    arc = graph.arcs(graph.start)[0]
    assert arc.input_label == ord("a")
    assert arc.output_label == ord("b")
```

`build()` consumes the builder only after a successful native build. A
validation failure leaves it usable, so callers can correct the graph and try
again.

## API and data model

The public surface is organized around five ownership-safe groups:

| Group | Principal API | Purpose |
|---|---|---|
| Native WFST | `WfstBuilder`, `Wfst`, `import_wfst`, `compose` | Construct, import, traverse, and lazily compose Unicode/tropical graphs |
| Host WFST | `ScalarWfstSnapshot`, `ScalarWfstResource` | Let Python implement immutable custom automata |
| Host lattice | `LatticeProvider`, `LatticeResource`, `LatticeValue` | Export Python join/meet values and consume them through native validation |
| Host semiring | `SemiringProvider`, `SemiringResource`, `SemiringContext` | Export a Python weight algebra with negotiated optional capabilities |
| Control/evidence | `Budget`, `Outcome`, `WfstDescriptor`, `Cancellation` | Validate bounded execution, replay identity, and cancellation state |

`Wfst.state_info(state)` reads finality without materializing arcs.
`Wfst.arcs(state)` drains the provider's paged arc callback with a progress
check. `Wfst.state(state)` combines both into an immutable
`ScalarWfstState`. `state_count` is `None` for genuinely lazy graphs;
`len(graph)` therefore raises instead of pretending the currently reached
frontier is complete.

Generate the API reference directly from the typed package:

```sh
pdoc --output-directory target/python-api lling_llang
```

## Custom providers

A custom WFST implements `start()`, `num_states()`, and `state()`. The
`state()` method returns one complete immutable state or `None` for an
unknown identifier. The provider facade snapshots once and caches validated
state expansions; mutating a published provider violates its snapshot
contract.

```python
class Rewrite:
    def start(self) -> int:
        return 0

    def num_states(self) -> int:
        return 2

    def state(self, state: int) -> lling.ScalarWfstState | None:
        if state == 0:
            arc = lling.ScalarWfstArc("b", "c", 1, 0.75)
            return lling.ScalarWfstState(None, (arc,))
        if state == 1:
            return lling.ScalarWfstState(0.0, ())
        return None


provider = Rewrite()
with lling.ScalarWfstResource(
    lambda: provider, lazy=False, acyclic=True
) as rewrite_resource:
    with lling.compose(graph, rewrite_resource) as product:
        outgoing = product.arcs(product.start)
```

Custom lattice values implement `join`, `meet`, `equal`, and `diagnostic`.
Supplying `stable_bytes` enables deterministic interchange with non-Python
implementations. Every `DomainId` is exactly sixteen bytes and identifies
both the encoding and its laws.

Custom semirings implement `zero`, `one`, `plus`, `times`, `equal`,
`approximately_equal`, `natural_order`, `stable_bytes`, and `diagnostic`.
Complete optional method groups add division, Kleene star, or numerical
projection capabilities. `SemiringOptions.properties` declares only laws the
provider actually satisfies; `validate_laws` tries to falsify those
declarations over one to sixteen representative values.

The complete, runnable provider implementations are in
[`examples/custom_providers.py`](https://github.com/vinary-tree/lling-llang/blob/v4.0.0-rc.6/bindings/python/examples/custom_providers.py).

## Ownership & memory model

Every owning facade is a context manager and has an idempotent `close()`.
Finalizers are a leak-containment fallback, not the primary lifecycle.

- A built or imported `Wfst` owns one retained immutable resource.
- `compose` captures independent snapshots of both inputs. The inputs may
  close immediately after successful composition.
- A `LatticeValue` owns one immutable lattice resource. Join, meet, and folds
  return new independently owned values.
- A `SemiringContext` independently retains its provider. Every
  `SemiringWeight` owns one generation-checked provider token associated with
  exactly that context.
- A borrowed `LatticeOperand` is valid only during its provider callback.
  Retaining it is an error.

Close semiring weights before their context when practical. Weight resources
retain enough native ownership to remain leak-safe, but operations require
the original open context to preserve domain identity.

## Errors

`NativeError` carries the stable `Status`, operation name, and a copied native
diagnostic. Copying matters because another ABI call may replace the
thread-local native error.

Python provider exceptions never unwind through C or Rust. The provider
facade catches them, records `last_callback_error`, and returns
`PROVIDER_ERROR`. Consumers also reject malformed booleans, unknown enum
discriminants, invalid Unicode scalars, inconsistent optional results,
non-progressing arc pages, cross-domain lattice values, and cross-context
semiring weights.

## Concurrency

Scalar WFST providers are serialized unless `parallel_reentrant=True` is
explicitly declared. Opt in only if every callback and all provider-visible
state are concurrently callable and reentrant. Customer callbacks execute
without the facade registry lock.

Dynamic lattice and semiring consumer handles are bound to their creating
thread. This matches the conservative requirements of Python-hosted callbacks
and rejects accidental migration deterministically. Provider arena locks cover
only token lookup, allocation, and publication; algebra callbacks run outside
them.

`Cancellation` is the exception: it is thread-safe and first-reason-wins, so
a worker or coordinator may request cancellation without changing an
already-recorded cause.

## Zero-copy paths

Resource handoff passes two machine words and increments a retain; it does not
copy a graph. Lazy composition stores two captured snapshots and expands only
reachable product states. Arc callbacks fill caller-owned contiguous pages.

Python provider state and canonical byte results must cross the language
boundary, so those values are validated and copied. The package never claims
zero-copy where CPython ownership makes copying necessary.

## Security and provider trust

Treat foreign resources and customer callbacks as untrusted capabilities.
Before use, the consumer validates the base ABI, interface identifier,
interface version, vtable size, reserved fields, required callbacks, unit and
weight domains, and threading flags. Each callback result is validated before
publication.

Provider code can still consume CPU, allocate memory, block, or return a graph
with unbounded reachable expansion. Apply application-level budgets and
cancellation, and do not mark an unknown-size provider `acyclic=True` without
establishing that invariant.

The detailed trust boundaries are documented in the
[ABI trust model](https://github.com/vinary-tree/lling-llang/blob/v4.0.0-rc.6/docs/security/abi-trust-model.md) and
[resource ABI architecture](https://github.com/vinary-tree/lling-llang/blob/v4.0.0-rc.6/docs/architecture/resource-abi.md).

## Troubleshooting

- **The native library cannot be loaded:** set `LLING_LLANG_LIBRARY` to the
  absolute built library, or install a wheel matching the current platform.
- **A resource reports closed:** keep the provider open until the consuming
  constructor succeeds. Successful import or composition owns an independent
  retain.
- **A dynamic algebra operation fails on another thread:** construct and use
  its `LatticeValue` or `SemiringContext` on the same thread.
- **Composition rejects a resource:** verify Unicode-scalar labels,
  tropical-f64 weights, and matching intermediate tape semantics.
- **`len(graph)` fails:** use `state_count` for a lazy provider and traverse
  from `start`; a finite count was not advertised.

## Version compatibility

The Python distribution uses PEP 440 spelling `4.0.0rc6`; the coordinated
Rust and source tag use SemVer spelling `4.0.0-rc.6`. The package requires
the exact same Python release of `vinary-tree-interop`.

At import, the facade requires native ABI version 1 and API revision 5 or
newer. Structure sizes are checked before any object construction. Additive
native revisions remain acceptable; an ABI-major mismatch fails import.

## Executable conformance evidence

Run the strict type, style, native integration, and executable-guide gates:

```sh
cargo build --release --no-default-features --features python-bindings
export LLING_LLANG_LIBRARY="$PWD/target/release/liblling_llang.so"
export PYTHONPATH="$PWD/bindings/python/src:$PWD/../vinary-tree-interop/bindings/python/src"
ruff check bindings/python
ruff format --check bindings/python
pyright -p bindings/python/pyrightconfig.json
python -m unittest discover -s bindings/python/tests -v
python bindings/python/examples/custom_providers.py
python scripts/check-bindings.py
python scripts/check-binding-docs.py
```

The integration suite proves transactional builder failure, eager import,
lazy composition after provider closure, paged traversal, independent
snapshots, lattice batching and law probes, optional semiring capabilities,
context identity, deterministic encodings, and same-thread enforcement.

## Maintainer workflow

1. Change the native ABI model and implementation together.
2. Bind every modeled `lling_*` symbol with an exact ctypes signature.
3. Extend unit and malformed-provider tests before advertising a capability.
4. Update this guide and the executable example with any public behavior.
5. Run the gates above and build a wheel from the exact source revision.
6. Test the wheel in a clean environment against the exact interop wheel.
7. Publish only from an immutable release tag through the protected PyPI
   trusted-publishing environment.

The machine-readable authority is [`bindings/api.json`](https://github.com/vinary-tree/lling-llang/blob/v4.0.0-rc.6/bindings/api.json). The
family documentation hub is
[`docs/bindings/README.md`](https://github.com/vinary-tree/lling-llang/blob/v4.0.0-rc.6/docs/bindings/README.md).
