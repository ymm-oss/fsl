// SPDX-License-Identifier: Apache-2.0

import { cases } from "./cases.mjs";
import { workerMessageError } from "./worker-protocol.mjs";

const output = document.querySelector("#result");
const heartbeat = setInterval(() => {
  output.dataset.tick = String(Number(output.dataset.tick ?? 0) + 1);
}, 100);

function runCase(testCase) {
  return new Promise((resolve, reject) => {
    const worker = new Worker("./worker.js");
    worker.addEventListener("message", ({ data }) => {
      if (data.id !== testCase.id) return;
      const error = workerMessageError(data);
      if (error) {
        worker.terminate();
        reject(error);
        return;
      }
      if (!data.envelope) return;
      worker.terminate();
      resolve(data.envelope);
    });
    worker.addEventListener("error", (event) => {
      worker.terminate();
      reject(new Error(event.message));
    });
    worker.postMessage({
      id: testCase.id,
      cmd: "verify",
      source: testCase.source,
      options: testCase.options,
    });
  });
}

function cancelAndRecover() {
  return new Promise((resolve, reject) => {
    const worker = new Worker("./worker.js");
    const timeout = setTimeout(() => {
      worker.terminate();
      reject(new Error("cancel probe did not initialize"));
    }, 30_000);
    worker.addEventListener("message", ({ data }) => {
      if (data.id !== "cancel-probe") return;
      const error = workerMessageError(data);
      if (error) {
        clearTimeout(timeout);
        worker.terminate();
        reject(error);
        return;
      }
      if (!data.progress) return;
      clearTimeout(timeout);
      worker.terminate();
      resolve(true);
    });
    worker.addEventListener("error", (event) => {
      clearTimeout(timeout);
      worker.terminate();
      reject(new Error(event.message));
    });
    worker.postMessage({
      id: "cancel-probe",
      cmd: "verify",
      source: cases[0].source,
      options: { depth: 64, deadlock: "ignore" },
    });
  });
}

function caseHolds(testCase, envelope) {
  if (envelope.result !== testCase.expected) return false;
  const expect = testCase.expect ?? {};
  if (expect.kind && envelope.violation_kind !== expect.kind) return false;
  if (expect.trace && !(Array.isArray(envelope.trace) && envelope.trace.length > 0)) return false;
  return true;
}

try {
  const cancelled = await cancelAndRecover();
  const envelopes = [];
  for (const testCase of cases) envelopes.push(await runCase(testCase));
  const details = { crossOriginIsolated, cancelled, envelopes };
  output.dataset.done = "true";
  output.dataset.ok = String(
    crossOriginIsolated
      && cancelled
      && envelopes.every((envelope, index) => caseHolds(cases[index], envelope)),
  );
  output.textContent = JSON.stringify(details);
} catch (error) {
  output.dataset.done = "true";
  output.dataset.ok = "false";
  output.textContent = JSON.stringify({ error: error instanceof Error ? error.message : String(error) });
} finally {
  clearInterval(heartbeat);
}
