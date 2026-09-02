# lling-llang JavaScript bindings

`@vinary-tree/lling-llang` is the JavaScript, TypeScript, and ClojureScript
facade for lling-llang's scalar WFSTs (weighted finite-state transducers):
build Unicode/tropical transducers, export them as family resources, and
compose them lazily with WFSTs produced by sibling packages — in process,
in $`\mathcal{O}(1)`$, without serialization.

The facade owns no native code. It delegates to the single
`@vinary-tree/javascript-runtime` shared runtime, so a WFST produced by the
`duallity` facade and a WFST built here live in the *same* runtime instance
and compose by handle passing. State expansion crosses the runtime boundary
once per state and returns one batched arc array.

- Package: [`@vinary-tree/lling-llang`](https://www.npmjs.com/package/@vinary-tree/lling-llang) (npm)
- Repository: [`vinary-tree/lling-llang`](https://github.com/vinary-tree/lling-llang), directory `bindings/javascript`
- Native contract: the same semantics as the
  [C ABI](https://github.com/vinary-tree/lling-llang/blob/master/docs/api/c-abi-reference.md)
  and the
  [resource ABI architecture](https://github.com/vinary-tree/lling-llang/blob/master/docs/architecture/resource-abi.md)

## Install

```sh
npm install @vinary-tree/lling-llang
```

Requirements and pins (enforced by `scripts/check-bindings.py`):

| Constraint | Value |
|---|---|
| Node.js | `>= 22.14` |
| `@vinary-tree/vinary-tree-interop` | exact `4.0.0-rc.6` (guards + shared types) |
| `@vinary-tree/javascript-runtime` | exact `4.0.0-rc.6` (the shared runtime that hosts the native code) |
| Package version | `4.0.0-rc.6` — always equal to the Rust crate version |

Pick your entry point per environment:

| Import | Environment | Runtime backing |
|---|---|---|
| `@vinary-tree/lling-llang` | Node.js (default) | native N-API |
| `@vinary-tree/lling-llang/typescript` | TypeScript projects | same, typed by `index.d.ts` |
| `@vinary-tree/lling-llang/clojurescript` | ClojureScript (JS interop) | same |
| `@vinary-tree/lling-llang/wasm` | browsers | WebAssembly build of the runtime |
| `@vinary-tree/lling-llang/wasi` | Node WASI deployments | WASI build of the runtime |
| `vinary-tree.lling-llang` (CLJS namespace) | idiomatic ClojureScript | wraps the native facade |

Use exactly **one** of native / `wasm` / `wasi` per resource-sharing domain:
resources carry their runtime's identity and refuse to mix (see
[Errors](#errors)).

## Quickstart

Build two single-arc transducers `a:x/0.5` and `x:z/0.25`, compose them
lazily, and walk the product (`a:z/0.75` — tropical weights add along a
path):

```js
import { vectorWfst, compose } from "@vinary-tree/lling-llang";

function singleArc(input, output, weight) {
  const builder = vectorWfst();
  const q0 = builder.addState();
  const q1 = builder.addState();
  builder.setStart(q0);
  builder.setFinal(q1, 0.0);
  builder.addArc(q0, input, output, q1, weight);
  const wfst = builder.build();
  builder.close();
  return wfst;
}

const first = singleArc("a", "x", 0.5);
const second = singleArc("x", "z", 0.25);

const product = compose(first, second); // lazy: snapshots now, states later
first.close();                          // the product retained its own
second.close();                         //   snapshots — inputs may close

const start = product.start();          // bigint state id
const state = product.state(start);     // { valid, final, finalWeight, arcs }
for (const arc of state.arcs) {
  console.log(arc.input, ":", arc.output, "/", arc.weight, "->", arc.target);
}
product.close();
```

TypeScript is the same surface with types (`WfstBuilder`, `Wfst`,
`WfstState`, `WfstArc`, `WeightDomain` from `index.d.ts`). ClojureScript has
an idiomatic namespace:

```clojure
(ns example
  (:require [vinary-tree.lling-llang :as lling]))

(let [b (lling/vector-wfst)
      q0 (lling/add-state! b)
      q1 (lling/add-state! b)]
  (lling/set-start! b q0)
  (lling/set-final! b q1)                  ; weight defaults to 0
  (lling/add-arc! b q0 "a" "x" q1 0.5)
  (let [w (lling/build! b)]
    (prn (lling/state w (lling/start w)))
    (lling/close! w)))
```

Epsilon labels are `null` on either side of `addArc` (and in returned arcs).
Labels are single Unicode scalar values — one-character strings like `"a"`
or `"🦀"`.

## Ownership & memory model

Handles (`WfstBuilder`, `Wfst`) wrap native objects owned by the umbrella
runtime:

- **Call `close()` deterministically, once, when done.** Closing releases
  the native memory immediately. Do not rely on garbage collection for
  timely release — treat `close()` like closing a file descriptor.
- `builder.build()` **consumes** the builder's graph (the native builder is
  then in its Closed lifecycle state); still call `builder.close()` to free
  the shell.
- `compose(first, second)` captures one immutable snapshot of each input at
  construction; the product owns those snapshots, so closing the inputs
  afterwards never invalidates the product. Order of teardown does not
  matter.
- `Wfst` values are immutable; `state(id)` never mutates, and repeated calls
  are served from the native product cache.

The native builder lifecycle behind `vectorWfst()`:

![LlingWfstBuilder lifecycle state machine: Open accepts edits; build moves to Consumed and emits the immutable WFST; builder calls after build report the Closed state; a build without a start state fails and restores Open.](https://github.com/vinary-tree/lling-llang/raw/master/docs/diagrams/architecture/builder-lifecycle-state.svg)

*Yellow = mutable builder; amber = consumed builder; green = immutable
handle.*

## Errors

JavaScript has a single error channel: **failed operations throw.**

| Source | Thrown | When |
|---|---|---|
| facade guard | `TypeError("resource does not implement vt.scalar-wfst.1")` | `compose` received a non-WFST resource (e.g. a dictionary) |
| facade guard | `TypeError("resource belongs to a different Vinary Tree runtime")` | mixing native / `wasm` / `wasi` instances, or another copy of the runtime |
| runtime | `Error` carrying the native diagnostic | every non-OK native status: invalid argument (absent state, non-tropical weight such as `NaN` or `-Infinity`, malformed label, missing start), builder already consumed, incompatible resource, provider failure during lazy expansion, representation limits, contained panics |

The native diagnostics are the same messages C callers read from
`lling_last_error_message()`; the full status taxonomy is the
[`LlingStatus` table](https://github.com/vinary-tree/lling-llang/blob/master/docs/api/c-abi-reference.md#status-codes).
Both guards throw **before** anything crosses into native code.

## Concurrency

JavaScript execution is single-threaded per runtime instance; every facade
call is synchronous. The native layer underneath is thread-safe and the
`Wfst` handles are immutable, but this package neither creates threads nor
requires you to think about them. Web Workers / worker_threads each load
their own runtime instance — handles must not be passed between instances
(the identity guard will refuse them).

## Zero-copy paths

- **Resource handoff is $`\mathcal{O}(1)`$.** `compose` accepts any object implementing
  `vt.scalar-wfst.1` from the *same* runtime — including WFSTs produced by
  the `duallity` facade. The handoff passes a retained handle; no graph is
  copied or serialized.
- **Lazy product.** Composition materializes nothing at construction; each
  product state is expanded on first visit and cached natively.
- **Batched expansion.** `state(id)` returns the state's complete arc array
  in one boundary crossing (one native call per state, not per arc).

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `TypeError: resource belongs to a different Vinary Tree runtime` | Two runtime instances in one process (native + `wasm`, duplicated `node_modules`, or a worker). Share one instance per resource domain; `npm dedupe` if the runtime is installed twice. |
| `TypeError: resource does not implement vt.scalar-wfst.1` | You passed a dictionary or other non-WFST resource to `compose`. |
| `Error: builder has already been consumed` | Builder used after `build()`. Create a new builder. |
| `Error: WFST has no start state` | Call `setStart` before `build()` — the builder remains usable after this failure. |
| Native module fails to load on Node | Check `node -p process.versions.node` is `>= 22.14`; reinstall so the platform-specific runtime binary matches your OS/arch. |
| Browser bundling | Import the `/wasm` subpath; ensure your bundler serves the runtime's `.wasm` asset. |
| Node WASI | Import the `/wasi` subpath and follow the shared runtime's WASI notes in the [family bindings guide](https://github.com/vinary-tree/liblevenshtein-rust/blob/master/docs/language-bindings.md). |

## Version compatibility

- Package `4.0.0-rc.6` = crate `4.0.0-rc.6`; native ABI v1, API revision 5 — the same
  contract the [C ABI reference](https://github.com/vinary-tree/lling-llang/blob/master/docs/api/c-abi-reference.md)
  documents (the API revision only grows within an ABI version).
- `@vinary-tree/*` dependencies are exact pins; the drift gate
  (`python3 scripts/check-bindings.py`) enforces package/crate version
  equality, export-map integrity, facade export parity across
  `d.ts`/`mjs`/`cjs`/`cljs`, and the presence of both runtime-identity and
  interface guards in every facade.

## Executable conformance evidence

[`test/facades.test.mjs`](test/facades.test.mjs) exercises the public native,
TypeScript, ClojureScript, WebAssembly, and WASI entry points against one
instrumented runtime contract. Run the same package command used in CI:

```sh
npm test --prefix bindings/javascript
```

The test is facade-level: it verifies export parity, runtime identity
rejection, interface guards, lazy composition, state expansion, and
deterministic close behavior without importing repository-private modules.

## Security and provider trust

Treat resource-like JavaScript objects as untrusted. The facade verifies the
singleton runtime identity and `vt.scalar-wfst.1` interface before delegating;
the native layer then validates provider output, scalar labels, weights, state
IDs, page bounds, and statuses. Never bypass these guards by reaching into a
handle's private fields or moving handles across workers/runtime instances.
See the [ABI trust model](../../docs/security/abi-trust-model.md).

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json), `package.json`, declarations, and every entry point together.
2. Keep JavaScript, TypeScript, and ClojureScript exports semantically identical.
3. Add positive, negative, teardown, and cross-runtime tests to `facades.test.mjs`.
4. Run `python3 scripts/check-bindings.py`, `python3 scripts/check-binding-docs.py`, and the npm suite.
5. Verify native, WebAssembly, and WASI packaging without weakening the identity guard.
