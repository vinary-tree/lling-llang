# lling-llang C++ bindings

Header-only C++20 RAII facade over lling-llang's stable C ABI. One header —
[`include/lling_llang.hpp`](../../include/lling_llang.hpp) — wraps WFST
construction/resource exchange, complete dynamic-semiring and dynamic-lattice
consumers, and typed cancellation in eight ownership-aware types under
`namespace vinary_tree::lling_llang`:

| Type | Wraps | Discipline |
|---|---|---|
| `builder` | `LlingWfstBuilder*` | constructed ready; frees on scope exit; fluent edits |
| `wfst` | `LlingWfst*` | move-only; frees on destruction; `import`/`compose` factories |
| `resource` | `VtResource` | move-only owned retain; releases on destruction |
| `semiring_context` | `LlingSemiring*` | copyable shared operation context; retains its host provider until the final context or weight dies |
| `semiring_weight` | `LlingSemiringWeight*` | move-only owned provider token; explicit `clone()` is the only ownership duplication |
| `lattice_value` | `LlingLatticeValue*` | move-only retained host value; checked join/meet/folds and law probes |
| `cancellation` | `LlingCancellationV2*` | move-only; atomic first-reason-wins request; single release on destruction |
| `error` | `LlingLlangStatus` + message | thrown by `check()` on any non-OK status |

The pointer-free descriptor, budget, and outcome structures and their five
validators remain directly available from the included C header.

The semantics are exactly the C ABI's — statuses, ownership, capture-once
composition, lazy product states — documented in the
[C ABI reference](https://github.com/vinary-tree/lling-llang/blob/master/docs/api/c-abi-reference.md)
and the
[resource ABI architecture](https://github.com/vinary-tree/lling-llang/blob/master/docs/architecture/resource-abi.md).
Semiring capability negotiation, token ownership, optional refinements, and
threading are specified in the
[dynamic-semiring architecture](https://github.com/vinary-tree/lling-llang/blob/master/docs/architecture/dynamic-semirings.md).
Host lattice ownership, algebra, batching, validation, and threading are
specified in the
[dynamic-lattice architecture](https://github.com/vinary-tree/lling-llang/blob/master/docs/architecture/dynamic-lattices.md).
The facade never copies opaque provider values: it adds checked status
translation and RAII bookkeeping, allocating only where the API returns owned
bytes or assembles a bounded law-validation batch.

## Install

### CMake package (recommended)

Staged release packages (and `scripts/stage-native-package.sh` builds) ship
the library, both lling-llang headers, and CMake config files. The stable family
ABI header is supplied by the separately versioned `vinary-tree-interop`
package, which lets every installed consumer share exactly one ABI definition:

```cmake
find_package(lling-llang 4.0 CONFIG REQUIRED)
target_link_libraries(your_target PRIVATE lling-llang::lling-llang)
target_compile_features(your_target PRIVATE cxx_std_20)
```

The config performs `find_dependency(vinary-tree-interop 4.0 CONFIG)`.
Point `CMAKE_PREFIX_PATH` at both package roots when they are not installed
system-wide. The imported target propagates the interop include directory for
shared and static linkage; select the latter by setting
`LLING_LLANG_LINKAGE=STATIC` before `find_package`. This is precisely what
the CI installed-package smoke test verifies
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

    cancellation stop;
    stop.request(LLING_CANCELLATION_REQUESTED_V2);
    if (stop.reason() != LLING_CANCELLATION_REQUESTED_V2) return 2;

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

### Consume host-defined semiring values

`semiring_context::open` negotiates `vt.semiring.val1` and retains an
independent operation context. A weight keeps that context alive even after
all user-visible context copies have gone out of scope:

```cpp
// `semiring_resource` remains owned by its provider.
auto semiring = semiring_context::open(semiring_resource);
auto zero = semiring.zero();
auto one = semiring.one();
auto sum = zero.plus(one);
auto product = one.times(one);

std::array<const semiring_weight*, 2> operands{&one, &sum};
auto three = semiring.plus_many(operands);
auto repeated = semiring.times_many(operands);

if (!sum.equivalent(one)) return 2;
auto quotient = product.divide(one); // std::nullopt when undefined
auto closure = zero.star();           // std::nullopt when divergent

std::array<const semiring_weight*, 5> samples{&zero, &one, &sum, &product, &three};
semiring.validate_laws(samples, 1e-12);
const auto canonical_identity = product.stable_bytes();
const auto context_description = semiring.diagnostic();
const auto value_description = three.diagnostic();
```

The facade covers equality and approximate equality, natural order, division
and left division, Kleene star, numerical value, quantization, probability,
declared properties, closure bounds, stable bytes, context/value diagnostics,
provider-accelerated batch folds, and bounded law probes.
Partial provider operations map to `std::optional`; unavailable or malformed
capabilities remain typed native errors. Weights from different operation
contexts are rejected before a binary native call, even when their domain IDs
happen to match.

### Consume host-defined lattice values

`lattice_value::open` borrows a live `VtResource` for one call and retains an
independent owner. The resource may come from LLattice, Julia, Raku, C++, or
any provider implementing `vt.lattice.val.1`:

```cpp
// `left_resource` and `right_resource` remain owned by their producer.
auto left = lattice_value::open(left_resource);
auto right = lattice_value::open(right_resource);
auto upper = left.join(right);
auto lower = left.meet(right);

std::array<const lattice_value*, 2> remainder{&right, &lower};
auto folded = left.join_many(remainder);
std::array<const lattice_value*, 3> samples{&left, &right, &folded};
lattice_value::validate_laws(samples);

const auto identity = folded.stable_bytes();
const auto explanation = folded.diagnostic();
```

`domain_id()` returns the exact 16-byte carrier/law identifier; `flags()`
returns the validated provider capabilities; and `equivalent()` performs
semantic equality. Binary operations reject a domain mismatch before invoking
foreign code. Batches preserve associative left-fold order and the native
adapter caps each foreign callback at 256 operands. Law validation accepts at
most sixteen representative values and can falsify—but cannot prove—the
universal lattice laws.

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
- `semiring_context` copies share one native operation context. Every returned
  `semiring_weight` holds that context alive and consumes exactly one provider
  token on destruction. Weights are move-only; `clone()` invokes the provider's
  ownership callback instead of copying opaque token words.
- `lattice_value` is move-only for the same reason. `open`, `join`, `meet`,
  `join_many`, and `meet_many` each return one independent owner; every owner
  is released exactly once by its destructor.
- `cancellation` is move-only; its destructor nulls and releases its C slot.
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
diagnostic and whose `status()` is the exact `LlingLlangStatus`:

| `status()` | Typical trigger |
|---|---|
| `LLING_STATUS_INVALID_ARGUMENT` | absent state, non-tropical weight (NaN, `-INFINITY`), malformed label, `build()` without a start state |
| `LLING_STATUS_NULL_POINTER` | null handle or null resource words reaching the C layer |
| `LLING_STATUS_PANIC` | a contained Rust panic (never unwinds into C++) |
| `LLING_STATUS_INCOMPATIBLE_RESOURCE` | `import`/`compose` on a resource without Unicode/tropical `vt.scalar-wfst.1` |
| `LLING_STATUS_PROVIDER_ERROR` | a foreign provider failed or returned invalid output during capture |
| `LLING_STATUS_LIMIT_EXCEEDED` | graph exceeds native representation on `import` |
| `LLING_STATUS_CLOSED` | builder used after `build()`, or a raw cancellation slot released twice |

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
- `semiring_context` and its weights follow the provider's advertised thread
  discipline. The native adapter serializes a serial provider with a
  nonblocking admission gate and never holds a C++ mutex across host code.
- `lattice_value` is deliberately same-thread even when its provider advertises
  parallel reentrancy. The native adapter uses a nonblocking atomic admission
  gate for serial providers and holds no consumer mutex while host join/meet
  code executes.
- Cancellation request/reason calls may race; destruction may not race them.

## Zero-copy paths

- `builder::build()` **moves** the graph into the immutable handle — no
  copy, $`\mathcal{O}(1)`$.
- `retained_resource()` is an atomic refcount increment — $`\mathcal{O}(1)`$.
- `wfst::compose(a, b)` captures one snapshot retain per input and copies
  nothing; product states materialize lazily during traversal and are
  cached.
- `wfst::import(r)` is the one deliberate copy: it materializes a private
  eager graph from a foreign resource, touching each reachable state and
  arc exactly once.
- Semiring context open, context copy, weight move, and destruction are
  constant-time ownership operations. Algebraic cost is provider-defined;
  stable bytes allocate exactly their returned size after bounded negotiation.
- Lattice `open`, move, and destruction are constant-time retain/handle
  operations. Join/meet cost is provider-defined. Batch folds amortize the C
  boundary; stable bytes and diagnostics allocate exactly their returned size
  after a bounded two-call length negotiation.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `find_package(lling-llang ...)` fails | Add the staged/installed package root to `CMAKE_PREFIX_PATH` (the smoke test uses the `dist/lling-llang-<version>-<target>` prefix). |
| `vinary_tree_interop.h: No such file or directory` | With CMake, install `vinary-tree-interop` and place its prefix on `CMAKE_PREFIX_PATH`; the imported target propagates its include directory. For a manual compiler invocation, add `-I .../vinary-tree-interop/include` or define `VT_INTEROP_HEADER` to its location. |
| Undefined references to `lling_*` | Link `-llling_llang` (note the double `l`: lib + lling); with CMake, link the `lling-llang::lling-llang` target. |
| Shared library not found at run time | Set an rpath to the package's `lib/` directory or use the static library from the staged package. |
| `error: builder has already been consumed` | The builder was used after `build()`; construct a new one. |
| `LLING_STATUS_INVALID_ARGUMENT` from join/meet | The two lattice values use different 16-byte domains. |
| `LLING_STATUS_INVALID_ARGUMENT` from a semiring binary operation | The weights came from different operation contexts, or a provider returned a malformed token. |
| Empty `std::optional` from division/star | The provider reported that the operation is undefined or divergent; this is not an error. |
| `lattice byte length did not stabilize` | The provider violated immutable byte-length expectations across three bounded reads. |

## Version compatibility

- Requires C++20 (`std::exchange`, `[[nodiscard]]`; the package smoke test
  compiles with `cxx_std_20`).
- ABI v1, API revision 6: call `lling_abi_version()` /
  `lling_llang_api_revision()` for a runtime handshake when loading the library
  dynamically; the revision only grows within an ABI version (see the
  [C ABI reference](../../docs/api/c-abi-reference.md#version-constants-and-the-handshake)).
- Typed metadata carries the independent `LLING_ABI_V2 == 2` format version.
- The header pair (`lling_llang.h`, `lling_llang.hpp`) is drift-gated
  against `src/ffi.rs` and `bindings/api.json` by
  `python3 scripts/check-bindings.py`.

## Executable conformance evidence

[`tests/package_smoke.cpp`](tests/package_smoke.cpp) is built as a consumer of
the staged package, not against repository-private headers. It exercises the
move-only builder/WFST/resource lifecycle and a complete C++ max/min lattice
provider plus a complete C++ Boolean-semiring provider through the RAII
consumers. The semiring checks cover retained context lifetime, exact token
release, identities, addition, multiplication, clone, equality, approximate
equality, natural order, partial division, closure, numerical projections,
properties, stable bytes, closure bounds, and law validation. The lattice
checks cover retained
ownership, move safety, join, meet, both batch folds, equality, domain/flag
negotiation, stable bytes, diagnostics, law validation, and zero live values
after scope exit. CI runs it against the source-built shared library, and the
native-package release gate repeats it against staged shared and static
packages:

```sh
cmake -S bindings/cpp/tests/package -B target/lling-cpp-package
cmake --build target/lling-cpp-package
ctest --test-dir target/lling-cpp-package --output-on-failure
```

## Security and provider trust

RAII prevents local leaks but does not make an arbitrary `VtResource`
trustworthy. Import and composition validate the base vtable, interface ID and
version, unit/weight domains, state IDs, labels, weights, page totals, and
provider statuses before publishing native state. The lattice consumer also
validates its capability prefix, flags, domain preservation, status/Boolean
encodings, success/failure output ownership, byte bounds, and law samples.
Never construct `resource`
from manually copied raw words unless the corresponding retain has succeeded.
The complete boundary analysis is the
[ABI trust model](../../docs/security/abi-trust-model.md).

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json) and the C ABI reference first.
2. Preserve move-only ownership and total status-to-exception mapping.
3. Extend the package smoke test, including failure and teardown paths.
4. Run both binding gates and test the staged shared and static packages.
5. Update this guide whenever loading, ownership, concurrency, or compatibility changes.
