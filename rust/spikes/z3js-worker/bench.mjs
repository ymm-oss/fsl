// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

import { solveRoundTrip, terminateSolverThreads } from "./solver.mjs";

const sizes = [100, 1_000, 10_000];
const samples = [];
for (const termCount of sizes) {
  samples.push(await solveRoundTrip(termCount));
}
await terminateSolverThreads();

console.log(
  JSON.stringify(
    {
      schema: "fsl-z3js-throughput.v1",
      runtime: process.version,
      samples,
      decisionInput: {
        metric: "termsPerSecond",
        note: "Compare this with Phase-1 BMC term counts before choosing per-term calls or batched SMT-LIB.",
      },
    },
    null,
    2,
  ),
);
