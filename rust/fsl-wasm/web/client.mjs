// SPDX-License-Identifier: Apache-2.0

const output = document.querySelector("#result");
const heartbeat = setInterval(() => {
  output.dataset.tick = String(Number(output.dataset.tick ?? 0) + 1);
}, 100);

const cases = [
  {
    id: "verified",
    expected: "verified",
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
    source: `spec BrowserBug {
  type K = 0..1
  state { x: K }
  init { x = 0 }
  action break_it() { requires x == 0 x = 1 }
  invariant StayZero { x == 0 }
}`,
  },
];

function runCase(testCase) {
  return new Promise((resolve, reject) => {
    const worker = new Worker("./worker.js");
    worker.addEventListener("message", ({ data }) => {
      if (data.id !== testCase.id || !data.envelope) return;
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
      options: { depth: 2, deadlock: "ignore" },
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
      if (data.id !== "cancel-probe" || !data.progress) return;
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

try {
  const cancelled = await cancelAndRecover();
  const envelopes = [];
  for (const testCase of cases) envelopes.push(await runCase(testCase));
  const details = { crossOriginIsolated, cancelled, envelopes };
  output.dataset.done = "true";
  output.dataset.ok = String(
    crossOriginIsolated
      && cancelled
      && envelopes.every((envelope, index) => envelope.result === cases[index].expected),
  );
  output.textContent = JSON.stringify(details);
} catch (error) {
  output.dataset.done = "true";
  output.dataset.ok = "false";
  output.textContent = JSON.stringify({ error: error instanceof Error ? error.message : String(error) });
} finally {
  clearInterval(heartbeat);
}
