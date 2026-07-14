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
    expect: { trace: true },
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
  {
    // A must-eventually policy that a drop transition breaks; the Worker
    // must report the leadsTo verdict and trace the native CLI reports.
    id: "business-leadsto",
    expected: "violated",
    expect: { kind: "leadsTo", trace: true },
    options: { depth: 2, deadlock: "ignore" },
    source: `business BrowserLeak {
  actor System
  entity Job

  process Job {
    stages Idle, Pending, Done, Dropped
    initial Idle

    transition submit Idle    -> Pending by System
    transition finish Pending -> Done    by System
    transition drop   Pending -> Dropped by System
  }

  policy POL-DONE "every submitted job must eventually complete"
    every Job in Pending must eventually be Done
}

verify {
  instances Job = 1
}`,
  },
  {
    // A `must eventually` obligation whose only path deadlocks: both
    // `deadlock_step` and `leadsto_violation` are set on the same BmcResult,
    // and with --deadlock error the Worker must report "deadlock" (matching
    // the native CLI's render_bmc_result branch order), not "leadsTo".
    id: "deadlock-beats-leadsto",
    expected: "violated",
    expect: { kind: "deadlock" },
    options: { depth: 2, deadlock: "error" },
    source: `spec BrowserStuckKernel {
  enum Stage { Idle, Pending, Done }

  state { stage: Stage }

  init { stage = Idle }

  action submit() {
    requires stage == Idle
    stage = Pending
  }

  leadsTo Progress { stage == Pending ~> stage == Done }
}`,
  },
  {
    // Struct-typed top-level state where one action changes only one field;
    // the Worker must not blow up rendering the trace for non-scalar state.
    id: "struct-state",
    expected: "violated",
    expect: { trace: true },
    options: { depth: 2, deadlock: "ignore" },
    source: `spec BrowserStructState {
  struct Job { status: Int, priority: Int }
  state { job: Job }
  init { job = Job { status: 0, priority: 0 } }
  action advance() {
    requires job.status == 0
    job.status = 1
  }
  invariant NeverAdvance { job.status == 0 }
}`,
  },
];
