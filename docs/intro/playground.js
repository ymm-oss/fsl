// SPDX-License-Identifier: Apache-2.0

const source = document.querySelector("#fsl-source");
const output = document.querySelector("#fsl-output");
const progress = document.querySelector("#fsl-progress");
const verifyButton = document.querySelector("#fsl-verify");
const cancelButton = document.querySelector("#fsl-cancel");
let worker;
let requestId = 0;

function freshWorker() {
  worker?.terminate();
  worker = new Worker("./fsl-wasm/worker.js");
  worker.addEventListener("message", ({ data }) => {
    if (data.id !== requestId) return;
    if (data.progress) {
      progress.textContent = `${data.progress.phase}${data.progress.depth == null ? "" : ` (depth ${data.progress.depth})`}`;
    }
    if (data.envelope) {
      output.textContent = JSON.stringify(data.envelope, null, 2);
      progress.textContent = "done";
      verifyButton.disabled = false;
      cancelButton.disabled = true;
    }
  });
  worker.addEventListener("error", ({ message }) => {
    output.textContent = JSON.stringify({ fsl: "1.0", result: "error", kind: "internal", message }, null, 2);
    verifyButton.disabled = false;
    cancelButton.disabled = true;
  });
}

verifyButton.addEventListener("click", () => {
  if (!crossOriginIsolated) {
    output.textContent = "This page must reload once to enable cross-origin isolation.";
    return;
  }
  requestId += 1;
  freshWorker();
  verifyButton.disabled = true;
  cancelButton.disabled = false;
  progress.textContent = "queued";
  output.textContent = "";
  worker.postMessage({
    id: requestId,
    cmd: "verify",
    source: source.value,
    options: { depth: 8, deadlock: "warn" },
  });
});

cancelButton.addEventListener("click", () => {
  worker?.terminate();
  worker = undefined;
  progress.textContent = "cancelled";
  verifyButton.disabled = false;
  cancelButton.disabled = true;
});
