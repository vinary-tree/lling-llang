import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../", import.meta.url);
const packageJson = JSON.parse(await readFile(new URL("package.json", root)));

test("all lling-llang facades select the shared JavaScript runtime", () => {
  assert.equal(packageJson.name, "@vinary-tree/lling-llang");
  assert.equal(packageJson.dependencies["@vinary-tree/javascript-runtime"], "4.0.0-rc.6");
  for (const entry of [".", "./typescript", "./clojurescript", "./wasm", "./wasi"]) {
    assert.ok(packageJson.exports[entry]);
  }
});

// C1 (identity/version): the facade version and its pinned runtime/interop
// dependency versions are internally consistent — a consumer resolving this
// package gets exactly the shared runtime and interop the facade was cut for.
test("C1 — the facade pins a coherent version and dependency set", () => {
  assert.match(packageJson.version, /^\d+\.\d+\.\d+-rc\.\d+$/);
  assert.equal(packageJson.dependencies["@vinary-tree/vinary-tree-interop"], "4.0.0-rc.6");
  assert.equal(packageJson.dependencies["@vinary-tree/javascript-runtime"], "4.0.0-rc.6");
});

// Every path referenced by the exports map must resolve to a real file — no
// export entry may dangle.
test("C1 — every exports entry resolves to a file on disk", async () => {
  const paths = new Set();
  const collect = (value) => {
    if (typeof value === "string") {
      if (value.startsWith("./")) paths.add(value);
    } else if (value && typeof value === "object") {
      for (const nested of Object.values(value)) collect(nested);
    }
  };
  collect(packageJson.exports);
  assert.ok(paths.size > 0, "exports map yielded no file paths");
  for (const relative of paths) {
    await access(new URL(relative, root)); // throws if missing
  }
});

// The four runtime facades exist in BOTH module systems (ESM + CommonJS), so a
// consumer on either loader reaches the same surface.
test("C2 — each runtime facade ships as both .mjs and .cjs", async () => {
  for (const facade of ["native", "typescript", "wasm", "wasi", "clojurescript"]) {
    const mjs = await readFile(new URL(`facades/${facade}.mjs`, root), "utf8");
    assert.ok(mjs.length > 0, `facades/${facade}.mjs is empty`);
    assert.ok(mjs.includes("export"), `facades/${facade}.mjs has no ESM export`);
    if (facade === "wasm" || facade === "wasi") {
      continue; // wasm/wasi ship ESM-only (no .cjs in facades/)
    }
    const cjs = await readFile(new URL(`facades/${facade}.cjs`, root), "utf8");
    assert.ok(
      cjs.includes("module.exports") || cjs.includes("exports."),
      `facades/${facade}.cjs has no CommonJS export`,
    );
  }
});

// The typed surface (index.d.ts) declares the WFST operations the runtime
// facades provide, so a TypeScript consumer sees the same API the JS resolves.
test("C1 — index.d.ts declares the WFST operation surface", async () => {
  const dts = await readFile(new URL("index.d.ts", root), "utf8");
  for (const name of [
    "runtimeIdentity",
    "vectorWfst",
    "compose",
    "WfstBuilder",
    "Wfst",
    "WfstArc",
    "WfstState",
  ]) {
    assert.ok(dts.includes(name), `index.d.ts is missing ${name}`);
  }
});

test("ClojureScript exposes idiomatic WFST operations", async () => {
  const source = await readFile(new URL("cljs/vinary_tree/lling_llang.cljs", root), "utf8");
  for (const name of ["vector-wfst", "add-state!", "set-start!", "set-final!", "add-arc!", "build!", "compose", "start", "state", "close!"]) {
    assert.ok(source.includes(`(defn ${name}`), `missing ${name}`);
  }
});
