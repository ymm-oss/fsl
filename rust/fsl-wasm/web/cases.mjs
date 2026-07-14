// SPDX-License-Identifier: Apache-2.0

// Browser smoke cases shared between the Worker client probe (client.mjs)
// and the native parity gate (test-browser.mjs). Each case carries the exact
// verify options both probes must use so the parity comparison stays honest.
export const cases = [
  {
    id: "verified",
    expected: "verified",
    options: { depth: 2, deadlock: "ignore" },
    source: `spec BrowserCounter {
  type K = 0..1
  state { x: K }
  init { x = 0 }
  action increment() { requires x == 0 x = 1 }
  invariant Bounded { x >= 0 and x <= 1 }
}`,
  },
  {
    id: "violated",
    expected: "violated",
    options: { depth: 2, deadlock: "ignore" },
    source: `spec BrowserBug {
  type K = 0..1
  state { x: K }
  init { x = 0 }
  action break_it() { requires x == 0 x = 1 }
  invariant StayZero { x == 0 }
}`,
  },
];
