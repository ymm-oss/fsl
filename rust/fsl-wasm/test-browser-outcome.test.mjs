// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import { classifyOutcome, ParityViolationError, ProbeTimeoutError } from "./browser-outcome.mjs";

test("browser outcome status mapping is stable", () => {
  assert.deepEqual(classifyOutcome(new ProbeTimeoutError("cdp", "timed out")), {
    outcome: "probe_timeout",
    status: 124,
    mode: "cdp",
  });
  assert.deepEqual(classifyOutcome(new ParityViolationError("mismatch", "report.json")), {
    outcome: "parity_violation",
    status: 65,
  });
  assert.deepEqual(classifyOutcome(new Error("unexpected")), {
    outcome: "harness_failure",
    status: 1,
  });
});
