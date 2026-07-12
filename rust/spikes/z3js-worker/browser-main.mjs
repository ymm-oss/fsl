// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

const output = document.querySelector("#result");
const worker = new Worker("./worker.js");
const report = (payload) => {
  void fetch("./result", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  }).catch((error) => {
    output.dataset.done = "true";
    output.dataset.ok = "false";
    output.textContent = `failed to report browser Worker result: ${error}`;
  });
};

worker.onmessage = ({ data }) => {
  const ok = Boolean(data.ok && data.result?.crossOriginIsolated);
  output.dataset.done = "true";
  output.dataset.ok = String(ok);
  output.textContent = JSON.stringify(data);
  report({ ok, details: data });
  worker.terminate();
};
worker.onerror = (event) => {
  output.dataset.done = "true";
  output.dataset.ok = "false";
  output.textContent = event.message;
  report({ ok: false, error: event.message });
  worker.terminate();
};
worker.postMessage({ id: "browser-phase0", termCount: 1_000 });
