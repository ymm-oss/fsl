// SPDX-License-Identifier: Apache-2.0

/* global importScripts */

importScripts("./z3-built.js");
const rawInitZ3 = globalThis.initZ3;
const z3ScriptUrl = new URL("./z3-built.js", self.location.href).href;
globalThis.initZ3 = (options = {}) => rawInitZ3({
  ...options,
  mainScriptUrlOrBlob: z3ScriptUrl,
  locateFile: (path) => new URL(path, self.location.href).href,
});

import initWasm, { internal_error as internalError, run } from "../pkg/fsl_wasm.js";
import { installZ3Bridge, terminateSolverThreads } from "./z3-bridge.mjs";

let initialized;

async function initialize() {
  if (!initialized) {
    initialized = (async () => {
      await installZ3Bridge();
      await initWasm({
        module_or_path: new URL("./fsl_wasm_bg.wasm", self.location.href),
      });
    })();
  }
  return initialized;
}

self.addEventListener("message", async ({ data }) => {
  const { id, batch, cmd, source, source_file, files, options } = data ?? {};
  let ready = false;
  try {
    self.postMessage({ id, progress: { phase: "initializing" } });
    await initialize();
    ready = true;
    if (Array.isArray(batch)) {
      const envelopes = [];
      for (const request of batch) {
        envelopes.push(JSON.parse(await run(JSON.stringify(request))));
      }
      self.postMessage({ id, envelopes });
      return;
    }
    self.postMessage({ id, progress: { phase: "verifying", depth: options?.depth ?? 8 } });
    const envelope = JSON.parse(
      await run(JSON.stringify({ cmd, source, source_file, files, options })),
    );
    self.postMessage({ id, envelope });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    self.postMessage(ready
      ? { id, envelope: JSON.parse(internalError(message)) }
      : { id, transportError: { kind: "initialization", message } });
  } finally {
    await terminateSolverThreads();
  }
});
