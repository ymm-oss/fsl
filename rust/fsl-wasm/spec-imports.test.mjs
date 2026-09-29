// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import { specImports } from "./spec-imports.mjs";

test("every import form yields its path, including a string on the next line", () => {
  const source = [
    "compose S {",
    '  use A as a from "a.fsl"',
    "  use B as b",
    '    from "../b.fsl"',
    "}",
    "governance G {",
    "  preservation P {",
    '    before X from "x.fsl" // trailing comment',
    '    checked_by refinement "x_refines.fsl"',
    "  }",
    "}",
  ].join("\n");
  assert.deepEqual(specImports(source, "s.fsl"), ["a.fsl", "../b.fsl", "x.fsl", "x_refines.fsl"]);
});

// #1056: the corpus comment that broke the WASM job.
test("from \" inside a // comment is not an import", () => {
  const source = [
    "spec S {",
    '  // `forbidden` cannot tell "the guard rejected it" from "an unrelated',
    "  // invariant broke afterward\": FB-13 used to live here",
    '  // this is prose, quoted from "somewhere else"',
    "  state { x: Int }",
    "}",
  ].join("\n");
  assert.deepEqual(specImports(source, "s.fsl"), []);
});

test("// inside a string literal is not a comment", () => {
  const source = 'spec S {\n  meta { note: "see http://example" }\n  use A as a from "a.fsl"\n}';
  assert.deepEqual(specImports(source, "s.fsl"), ["a.fsl"]);
});

test("a string after from that does not close on its line throws, naming the file", () => {
  const source = 'compose S {\n  use A as a from "a.fsl\n  use B as b from "b.fsl"\n}';
  assert.throws(
    () => specImports(source, "examples/x/s.fsl"),
    /^Error: examples\/x\/s\.fsl:2: unterminated string literal/,
  );
});
