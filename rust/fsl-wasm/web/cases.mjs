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
  {
    // Bool constants go through fslZ3Constant's bool branch.
    id: "bool-state",
    expected: "verified",
    options: { depth: 2, deadlock: "ignore" },
    source: `spec BrowserToggle {
  state { armed: Bool }
  init { armed = false }
  action arm() { requires armed == false armed = true }
  invariant ArmedOnce { armed == true or armed == false }
}`,
  },
  {
    // count aggregation lowers to if-then-else terms (fslZ3Ite).
    id: "business-kpi",
    expected: "verified",
    options: { depth: 2, deadlock: "ignore" },
    source: `business BrowserFlow {
  actor System
  entity Job

  process Job {
    stages Pending, Done
    initial Pending

    transition finish Pending -> Done by System
  }

  kpi done = count Job in Done

  policy POL-DONE "every pending job must eventually complete"
    every Job in Pending must eventually be Done
}

verify {
  instances Job = 1
}`,
  },
];
