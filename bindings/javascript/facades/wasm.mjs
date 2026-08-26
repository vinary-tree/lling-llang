import { llingLlang } from "@vinary-tree/javascript-runtime/wasm";
import { assertSameRuntime, assertWfstResource } from "@vinary-tree/vinary-tree-interop";

export const runtimeIdentity = llingLlang.runtimeIdentity;
export const vectorWfst = llingLlang.vectorWfst.bind(llingLlang);
export function compose(first, second) {
  assertWfstResource(first);
  assertWfstResource(second);
  assertSameRuntime(first, runtimeIdentity);
  assertSameRuntime(second, runtimeIdentity);
  return llingLlang.compose(first, second);
}
export default { ...llingLlang, runtimeIdentity, vectorWfst, compose };
