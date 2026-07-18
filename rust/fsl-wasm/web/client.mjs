// SPDX-License-Identifier: Apache-2.0

import { cases } from "./cases.mjs";
import { workerMessageError } from "./worker-protocol.mjs";

const output = document.querySelector("#result");
const heartbeat = setInterval(() => {
  output.dataset.tick = String(Number(output.dataset.tick ?? 0) + 1);
}, 100);

function requestWorker(message, resultField, timeoutMs = 60_000) {
  return new Promise((resolve, reject) => {
    const worker = new Worker("./worker.js");
    const finish = (callback, value) => {
      clearTimeout(timeout);
      worker.terminate();
      callback(value);
    };
    const timeout = setTimeout(() => {
      finish(reject, new Error(`${message.id} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
    worker.addEventListener("message", ({ data }) => {
      if (data.id !== message.id) return;
      const error = workerMessageError(data);
      if (error) {
        finish(reject, error);
        return;
      }
      if (!data[resultField]) return;
      finish(resolve, data[resultField]);
    });
    worker.addEventListener("error", (event) => {
      finish(reject, new Error(event.message));
    });
    worker.postMessage(message);
  });
}

function runCase(testCase) {
  return requestWorker({
    id: testCase.id,
    cmd: testCase.cmd ?? "verify",
    source: testCase.source,
    source_file: testCase.source_file,
    files: testCase.files,
    options: testCase.options,
  }, "envelope");
}

async function runBatches(testCases, width = 4) {
  const checks = testCases.filter((testCase) => testCase.cmd === "check");
  const byId = new Map();
  if (checks.length > 0) {
    const envelopes = await requestWorker({
      id: "check-batch",
      batch: checks.map(({ cmd, source, source_file, files, options }) => (
        { cmd, source, source_file, files, options }
      )),
    }, "envelopes");
    if (envelopes.length !== checks.length) {
      throw new Error("check batch returned an incomplete envelope set");
    }
    checks.forEach((testCase, index) => byId.set(testCase.id, envelopes[index]));
  }
  const verifies = testCases.filter((testCase) => testCase.cmd !== "check");
  for (let index = 0; index < verifies.length; index += width) {
    const batch = verifies.slice(index, index + width);
    const envelopes = await Promise.all(batch.map(runCase));
    batch.forEach((testCase, offset) => byId.set(testCase.id, envelopes[offset]));
  }
  return testCases.map((testCase) => byId.get(testCase.id));
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
      if (data.progress?.phase !== "verifying") return;
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
  const parityCases = await fetch("./parity-cases.json").then((response) => response.json());
  const parityEnvelopes = await runBatches(parityCases);
  const details = { crossOriginIsolated, cancelled, envelopes, parityEnvelopes };
  output.dataset.done = "true";
  output.dataset.ok = String(
    crossOriginIsolated
      && cancelled
      && envelopes.every((envelope, index) => caseHolds(cases[index], envelope))
      && parityEnvelopes.length === parityCases.length,
  );
  output.textContent = JSON.stringify(details);
} catch (error) {
  output.dataset.done = "true";
  output.dataset.ok = "false";
  output.textContent = JSON.stringify({ error: error instanceof Error ? error.message : String(error) });
} finally {
  clearInterval(heartbeat);
}
