# Foreign-language bindings

**Navigation:** [documentation index](../README.md) ·
[C ABI reference](../api/c-abi-reference.md) ·
[resource architecture](../architecture/resource-abi.md) ·
[ABI trust model](../security/abi-trust-model.md)

lling-llang exposes one stable project ABI and three standalone facade guides.
JavaScript, TypeScript, and ClojureScript deliberately share one package and
one singleton runtime, so their guide documents both the common ownership laws
and each language's syntax.

| Guide | Languages | Package/boundary | Executable evidence |
|---|---|---|---|
| [C](../../bindings/c/README.md) | C17/C23 | CMake/pkg-config; direct `lling_*` | [`compose_demo.c`](../../bindings/c/examples/compose_demo.c) |
| [C++](../../bindings/cpp/README.md) | C++20+ | Header-only RAII over C | [`package_smoke.cpp`](../../bindings/cpp/tests/package_smoke.cpp) |
| [JavaScript family](../../bindings/javascript/README.md) | JavaScript, TypeScript, ClojureScript | npm; N-API/WebAssembly/WASI singleton runtime | [`facades.test.mjs`](../../bindings/javascript/test/facades.test.mjs) |

The shared semantic core is capture-once composition: a constructor validates
and retains an immutable input snapshot, then lazy traversal publishes product
states as demanded. The diagram shows the complete import/compose boundary:

![Snapshot capture, import, lazy composition, traversal, and release sequence.](../diagrams/architecture/wfst-import-compose-sequence.svg)

## Documentation governance

[`bindings/api.json`](../../bindings/api.json) is the machine-readable inventory.
The documentation gate rejects a missing guide or example, broken local link,
untagged code fence, placeholder, or absent operational topic:

```sh
python3 scripts/check-binding-docs.py
```

Every facade guide must cover installation/loading, executable evidence, API
and data domains, ownership/snapshots, errors, concurrency, performance,
security, compatibility, and the maintainer workflow. The package guide and
checked example ship together.
