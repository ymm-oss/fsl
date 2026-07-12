// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

const output = document.querySelector("#result");
const worker = new Worker("./worker.js");
const timeout = setTimeout(() => {
  output.dataset.done = "true";
  output.dataset.ok = "false";
  output.textContent = "browser Worker timed out";
  worker.terminate();
}, 45_000);

worker.onmessage = ({ data }) => {
  clearTimeout(timeout);
  output.dataset.done = "true";
  output.dataset.ok = String(Boolean(data.ok && data.result?.crossOriginIsolated));
  output.textContent = JSON.stringify(data);
  worker.terminate();
};
worker.onerror = (event) => {
  clearTimeout(timeout);
  output.dataset.done = "true";
  output.dataset.ok = "false";
  output.textContent = event.message;
  worker.terminate();
};
worker.postMessage({ id: "browser-phase0", termCount: 1_000 });
