import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../", import.meta.url);
const packageJson = JSON.parse(await readFile(new URL("package.json", root)));

test("all lling-llang facades select the shared umbrella runtime", () => {
  assert.equal(packageJson.name, "@vinary-tree/lling-llang");
  assert.equal(packageJson.dependencies["@vinary-tree/vinary-tree"], "0.10.0");
  for (const entry of [".", "./typescript", "./clojurescript", "./wasm", "./wasi"]) {
    assert.ok(packageJson.exports[entry]);
  }
});

test("ClojureScript exposes idiomatic WFST operations", async () => {
  const source = await readFile(new URL("cljs/vinary_tree/lling_llang.cljs", root), "utf8");
  for (const name of ["vector-wfst", "add-state!", "set-start!", "set-final!", "add-arc!", "build!", "compose", "start", "state", "close!"]) {
    assert.ok(source.includes(`(defn ${name}`), `missing ${name}`);
  }
});
