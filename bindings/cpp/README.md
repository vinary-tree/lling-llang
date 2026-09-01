# lling-llang C++ bindings

Header-only C++20 RAII facade over lling-llang's stable C ABI. One header —
[`include/lling_llang.hpp`](../../include/lling_llang.hpp) — wraps the 17
`lling_*` functions in four move-aware types under
`namespace vinary_tree::lling_llang`:

| Type | Wraps | Discipline |
|---|---|---|
| `builder` | `LlingWfstBuilder*` | constructed ready; frees on scope exit; fluent edits |
| `wfst` | `LlingWfst*` | move-only; frees on destruction; `import`/`compose` factories |
| `resource` | `VtResource` | move-only owned retain; releases on destruction |
| `error` | `LlingStatus` + message | thrown by `check()` on any non-OK status |

The semantics are exactly the C ABI's — statuses, ownership, capture-once
composition, lazy product states — documented in the
[C ABI reference](https://github.com/vinary-tree/lling-llang/blob/master/docs/api/c-abi-reference.md)
and the
[resource ABI architecture](https://github.com/vinary-tree/lling-llang/blob/master/docs/architecture/resource-abi.md).
This facade adds zero overhead beyond a status check per call.

## Install

### CMake package (recommended)

Staged release packages (and `scripts/stage-native-package.sh` builds) ship
the library, both headers, the bundled `vinary_tree_interop.h`, and CMake
config files:

```cmake
find_package(lling-llang 0.2 CONFIG REQUIRED)
target_link_libraries(your_target PRIVATE lling-llang::lling-llang)
target_compile_features(your_target PRIVATE cxx_std_20)
```

Point `CMAKE_PREFIX_PATH` at the package root if it is not installed
system-wide. This is precisely what the CI package smoke test does
([`tests/package/CMakeLists.txt`](tests/package/CMakeLists.txt)).

### pkg-config

```sh
c++ -std=c++20 $(pkg-config --cflags lling-llang) demo.cpp \
    $(pkg-config --libs lling-llang)
```

### From source

```sh
cargo build --release --features ffi     # produces cdylib + staticlib
c++ -std=c++20 -I include \
    -I ../vinary-tree-interop/include \
    demo.cpp -L target/release -llling_llang
```

The C header includes the family interop header as
`#include VT_INTEROP_HEADER` (default `"vinary_tree_interop.h"`); define
`VT_INTEROP_HEADER` to relocate it.

## Quickstart

Build two transducers, compose them lazily, export the product as a family
resource (mirrors the CI smoke test
[`tests/package_smoke.cpp`](tests/package_smoke.cpp)):

```cpp
#include <lling_llang.hpp>
#include <cstdio>

int main() {
    using namespace vinary_tree::lling_llang;

    auto make = [](char32_t in, char32_t out, double w) {
        builder b;
        const auto q0 = b.add_state();
        const auto q1 = b.add_state();
        b.start(q0).final_state(q1).arc(q0, in, out, q1, w);
        auto graph = b.build();            // builder is consumed here
        return graph.retained_resource();  // independent owned retain
    };

    resource first = make(U'a', U'x', 0.5);
    resource second = make(U'x', U'z', 0.25);

    // Lazy composition: one snapshot per input, no product state expanded.
    auto product = wfst::compose(first.get(), second.get());
    // `first`/`second` may now go out of scope in any order — the product
    // holds its own snapshot retains.

    resource exported = product.retained_resource();
    // exported.get() is a VtResource implementing vt.scalar-wfst.1: hand it
    // to any family consumer, or walk it via the interop vtable (see the C
    // example in the C ABI reference for the paged arc loop).
    std::puts(exported.get().context != nullptr ? "ok" : "unexpected");
    return 0;
}   // RAII: exported releases, product frees — order-independent
```

Fluent builder notes: `final_state(q)` defaults the final weight to `0.0`
(the tropical semiring's one); `arc(...)` defaults the weight to `0.0`;
`epsilon(from, to, w)` adds an $`\varepsilon`$:$`\varepsilon`$ arc; labels
are `char32_t` Unicode scalar values (`U'🦀'` works).

## Ownership & memory model

Pure RAII — every wrapper releases exactly once, in its destructor:

- `builder` allocates in its constructor (throws `error` on failure) and
  frees on destruction, whether or not `build()` consumed it.
- `build()` returns a `wfst` and consumes the builder's graph; building
  twice throws `error` with status `LLING_STATUS_CLOSED`. A `build()` on a
  start-less graph throws with `LLING_STATUS_INVALID_ARGUMENT` and the
  builder remains editable — set a start state and build again.
- `wfst` and `resource` are move-only (copying a raw handle would double
  the release). Moved-from objects are empty and safe to destroy.
- `retained_resource()` mints an **independent** retain each call: the
  resource keeps the graph alive even after the `wfst` is destroyed, and
  vice versa. Teardown order is free.
- Nothing in this facade is re-entered by the library: destructors call
  plain C frees and never throw.

The builder's underlying lifecycle:

![LlingWfstBuilder lifecycle state machine: Open accepts edits; build moves to Consumed and emits the immutable WFST; builder calls after build report CLOSED; a build without a start state fails with INVALID_ARGUMENT and restores Open.](https://github.com/vinary-tree/lling-llang/raw/master/docs/diagrams/architecture/builder-lifecycle-state.svg)

*Yellow = mutable builder; amber = consumed builder; green = immutable
handle.*

## Errors

`check()` converts every non-OK status into a thrown
`vinary_tree::lling_llang::error` whose `what()` is the thread-local native
diagnostic and whose `status()` is the exact `LlingStatus`:

| `status()` | Typical trigger |
|---|---|
| `LLING_STATUS_INVALID_ARGUMENT` | absent state, non-tropical weight (NaN, `-INFINITY`), malformed label, `build()` without a start state |
| `LLING_STATUS_NULL_POINTER` | null handle or null resource words reaching the C layer |
| `LLING_STATUS_PANIC` | a contained Rust panic (never unwinds into C++) |
| `LLING_STATUS_INCOMPATIBLE_RESOURCE` | `import`/`compose` on a resource without Unicode/tropical `vt.scalar-wfst.1` |
| `LLING_STATUS_PROVIDER_ERROR` | a foreign provider failed or returned invalid output during capture |
| `LLING_STATUS_LIMIT_EXCEEDED` | graph exceeds native representation on `import` |
| `LLING_STATUS_CLOSED` | builder used after a successful `build()` |

Catch `const error&`; `status()` is `noexcept`. The void C calls wrapped by
destructors (`free`/`release`) have no failure mode, so destructors are
implicitly `noexcept`.

## Concurrency

- `builder` is single-threaded — confine each instance to one thread.
- `wfst` is immutable and thread-safe: concurrent `retained_resource()`
  calls and concurrent traversal of exported resources are supported
  (composed products expand product states in parallel; there is no
  resource-wide lock).
- Diagnostics are thread-local: an `error` thrown on one thread carries
  that thread's message, unaffected by failures elsewhere.

## Zero-copy paths

- `builder::build()` **moves** the graph into the immutable handle — no
  copy, $`O(1)`$.
- `retained_resource()` is an atomic refcount increment — $`O(1)`$.
- `wfst::compose(a, b)` captures one snapshot retain per input and copies
  nothing; product states materialize lazily during traversal and are
  cached.
- `wfst::import(r)` is the one deliberate copy: it materializes a private
  eager graph from a foreign resource, touching each reachable state and
  arc exactly once.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `find_package(lling-llang ...)` fails | Add the staged/installed package root to `CMAKE_PREFIX_PATH` (the smoke test uses the `dist/lling-llang-<version>-<target>` prefix). |
| `vinary_tree_interop.h: No such file or directory` | Add the interop include directory (`-I .../vinary-tree-interop/include`) or define `VT_INTEROP_HEADER` to its location. Packaged installs bundle it. |
| Undefined references to `lling_*` | Link `-llling_llang` (note the double `l`: lib + lling); with CMake, link the `lling-llang::lling-llang` target. |
| Shared library not found at run time | Set an rpath to the package's `lib/` directory or use the static library from the staged package. |
| `error: builder has already been consumed` | The builder was used after `build()`; construct a new one. |

## Version compatibility

- Requires C++20 (`std::exchange`, `[[nodiscard]]`; the package smoke test
  compiles with `cxx_std_20`).
- ABI v1, API revision 1: call `lling_abi_version()` /
  `lling_api_revision()` for a runtime handshake when loading the library
  dynamically; the revision only grows within an ABI version (see the
  [C ABI reference](../../docs/api/c-abi-reference.md#version-constants-and-the-handshake)).
- The header pair (`lling_llang.h`, `lling_llang.hpp`) is drift-gated
  against `src/ffi.rs` and `bindings/api.json` by
  `python3 scripts/check-bindings.py`.

## Executable conformance evidence

[`tests/package_smoke.cpp`](tests/package_smoke.cpp) is built as a consumer of
the staged package, not against repository-private headers. It exercises the
move-only builder/WFST/resource lifecycle and is run by the native-package
release gate:

```sh
cmake -S bindings/cpp/tests/package -B target/lling-cpp-package
cmake --build target/lling-cpp-package
ctest --test-dir target/lling-cpp-package --output-on-failure
```

## Security and provider trust

RAII prevents local leaks but does not make an arbitrary `VtResource`
trustworthy. Import and composition validate the base vtable, interface ID and
version, unit/weight domains, state IDs, labels, weights, page totals, and
provider statuses before publishing native state. Never construct `resource`
from manually copied raw words unless the corresponding retain has succeeded.
The complete boundary analysis is the
[ABI trust model](../../docs/security/abi-trust-model.md).

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json) and the C ABI reference first.
2. Preserve move-only ownership and total status-to-exception mapping.
3. Extend the package smoke test, including failure and teardown paths.
4. Run both binding gates and test the staged shared and static packages.
5. Update this guide whenever loading, ownership, concurrency, or compatibility changes.
