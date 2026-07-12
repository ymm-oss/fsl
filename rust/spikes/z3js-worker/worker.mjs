// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

import { parentPort } from "node:worker_threads";

import { solveRoundTrip, terminateSolverThreads } from "./solver.mjs";

if (parentPort === null) {
  throw new Error("z3js Phase-0 spike must run inside a Worker");
}

parentPort.on("message", async ({ id, termCount }) => {
  try {
    const result = await solveRoundTrip(termCount);
    parentPort.postMessage({ id, ok: true, result });
  } catch (error) {
    parentPort.postMessage({
      id,
      ok: false,
      error: error instanceof Error ? error.stack : String(error),
    });
  } finally {
    await terminateSolverThreads();
  }
});
