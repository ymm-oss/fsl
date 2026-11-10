// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import { exitStatusFor, ProbeTimerExpiredError } from "./browser-probe-outcome.mjs";

test("only the typed result-timer expiry has status 124", () => {
  assert.equal(exitStatusFor(new ProbeTimerExpiredError("result timer expired after 75s")), 124);
});

test("ordinary spike errors stay status 1", () => {
  for (const message of ["Chrome closed early", "worker failed", "result ok false", "build failed"]) {
    assert.equal(exitStatusFor(new Error(message)), 1, message);
  }
});

test("a timeout message alone cannot classify a spike error as timer expiry", () => {
  assert.equal(exitStatusFor(new Error("Chrome browser probe timeout after 75s")), 1);
});

test("a timer-error lookalike is not the production timer-error class", () => {
  const lookalike = {
    name: "ProbeTimerExpiredError",
    message: "result timer expired after 75s",
    constructor: ProbeTimerExpiredError,
  };
  assert.equal(lookalike instanceof ProbeTimerExpiredError, false);
  assert.equal(exitStatusFor(lookalike), 1);
  const renamedError = new Error("timer expired");
  renamedError.name = "ProbeTimerExpiredError";
  assert.equal(exitStatusFor(renamedError), 1);
});
