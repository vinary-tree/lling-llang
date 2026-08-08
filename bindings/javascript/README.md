# lling-llang JavaScript bindings

`@vinary-tree/lling-llang` owns the JavaScript, TypeScript, and ClojureScript
WFST facade. It delegates to the single `@vinary-tree/vinary-tree` runtime so
resources from `duallity` compose in O(1) without serialization. State
expansion crosses the runtime boundary once per state and returns one batched
arc array. Use `/wasm` in browsers and `/wasi` in Node WASI deployments.
