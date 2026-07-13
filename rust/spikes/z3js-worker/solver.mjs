// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

import { init } from "z3-solver";

let initialized;

async function context() {
  initialized ??= init();
  const { Context, em } = await initialized;
  return { api: new Context("fsl-phase0"), em };
}

export async function solveRoundTrip(termCount = 1_000) {
  const { api, em } = await context();
  const { Int, Solver } = api;
  const solver = new Solver();
  const x = Int.const("x");

  const constructStarted = performance.now();
  for (let i = 0; i < termCount; i += 1) {
    solver.add(x.add(i).ge(i));
  }
  solver.add(x.eq(42));
  const constructMs = performance.now() - constructStarted;

  const checkStarted = performance.now();
  const verdict = await solver.check();
  const checkMs = performance.now() - checkStarted;
  const model = solver.model();
  const value = model.eval(x, true).value();

  return {
    z3Version: "4.16.0",
    verdict: String(verdict),
    model: { x: Number(value) },
    termCount,
    constructMs,
    checkMs,
    termsPerSecond: Math.round((termCount / constructMs) * 1_000),
    canTerminateThreads: typeof em?.PThread?.terminateAllThreads === "function",
  };
}

export async function terminateSolverThreads() {
  if (!initialized) return;
  const { em } = await initialized;
  em?.PThread?.terminateAllThreads?.();
  initialized = undefined;
}
