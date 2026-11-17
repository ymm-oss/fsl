// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): the part of the vitest API that `fslc testgen --target vitest`
// uses (test, expect(..).toStrictEqual / toBe), so testgen.mjs can run that
// suite without a test framework. runAll prints one JSON report and sets the
// exit code: 0 all passed, 1 a scenario failed.

import { deepStrictEqual } from "node:assert/strict";

const tests = [];
export const test = (name, body) => tests.push({ name, body });

export function expect(actual, message) {
  const where = message ? `${message}: ` : "";
  return {
    toStrictEqual(expected) {
      // The core returns frozen plain objects; compare as JSON values.
      deepStrictEqual(JSON.parse(JSON.stringify(actual ?? null)), expected, `${where}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    },
    toBe(expected) {
      if (!Object.is(actual, expected)) throw new Error(`${where}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    },
  };
}

export async function runAll(spec, tally) {
  const failures = [];
  for (const t of tests) {
    try {
      await t.body();
    } catch (err) {
      failures.push({ scenario: t.name, error: String(err?.message ?? err).slice(0, 600) });
    }
  }
  console.log(JSON.stringify({ spec, scenarios: tests.length, passed: tests.length - failures.length, steps_taken: tally.steps, forbidden_steps_checked: tally.forbidden, failed: failures.length, first_failure: failures[0] ?? null }, null, 2));
  process.exitCode = failures.length ? 1 : 0;
}
