// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

import { Worker } from "node:worker_threads";

function runWorker(termCount) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./worker.mjs", import.meta.url), { type: "module" });
    const timeout = setTimeout(() => {
      worker.terminate();
      reject(new Error("z3-solver Worker timed out"));
    }, 60_000);
    worker.once("error", reject);
    worker.once("message", async (message) => {
      clearTimeout(timeout);
      await worker.terminate();
      if (message.ok) resolve(message.result);
      else reject(new Error(message.error));
    });
    worker.postMessage({ id: "phase0", termCount });
  });
}

const result = await runWorker(1_000);
if (result.verdict !== "sat" || result.model.x !== 42) {
  throw new Error(`unexpected solver result: ${JSON.stringify(result)}`);
}
console.log(JSON.stringify({ schema: "fsl-z3js-spike.v1", ...result }, null, 2));
