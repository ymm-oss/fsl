// SPDX-License-Identifier: Apache-2.0

// The bridge's output-model re-solve (#1940) on the host, through z3-solver's
// Node build: the browser itself runs it only in `test:browser`.

import assert from "node:assert/strict";
import test from "node:test";
import { init } from "z3-solver/build/node.js";
import { installZ3Bridge, terminateSolverThreads } from "./web/z3-bridge.mjs";

await installZ3Bridge(init);
test.after(() => terminateSolverThreads());

const z3 = globalThis;
const int = () => z3.fslZ3Sort("int");
const bool = () => z3.fslZ3Sort("bool");
const equals = (left, right) => z3.fslZ3Binary("eq", left, z3.fslZ3IntValue(right));

test("the re-solve assumes the trackers in scope, as the check did", async () => {
  z3.fslZ3Reset();
  const x = z3.fslZ3Constant("tracked_x", int());
  const y = z3.fslZ3Constant("tracked_y", int());
  const kept = z3.fslZ3Constant("tracked_kept", bool());
  const popped = z3.fslZ3Constant("tracked_popped", bool());
  z3.fslZ3AssertAndTrack(equals(x, 5), kept);
  z3.fslZ3Push();
  z3.fslZ3AssertAndTrack(equals(y, 6), popped);
  z3.fslZ3Pop(1);
  z3.fslZ3Assert(z3.fslZ3Unary("not", equals(y, 6)));
  assert.equal(await z3.fslZ3Check([]), "sat");
  await z3.fslZ3ResolveOutputModel();
  // Without the tracker assumed, the fresh context sets it false.
  assert.equal(z3.fslZ3ModelEval(kept, true), true);
  assert.equal(z3.fslZ3ModelEval(x, false), 5);
  assert.notEqual(z3.fslZ3ModelEval(y, false), 6);
});
