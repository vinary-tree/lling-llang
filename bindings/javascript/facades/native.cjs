"use strict";
const { llingLlang } = require("@vinary-tree/javascript-runtime");
const { assertSameRuntime, assertWfstResource } = require("@vinary-tree/vinary-tree-interop");
const runtimeIdentity = llingLlang.runtimeIdentity;
const vectorWfst = llingLlang.vectorWfst.bind(llingLlang);
function compose(first, second) {
  assertWfstResource(first);
  assertWfstResource(second);
  assertSameRuntime(first, runtimeIdentity);
  assertSameRuntime(second, runtimeIdentity);
  return llingLlang.compose(first, second);
}
module.exports = { ...llingLlang, runtimeIdentity, vectorWfst, compose, default: llingLlang };
