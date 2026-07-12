// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

/* global importScripts */

importScripts("./z3-built.js");

// Emscripten cannot infer `document.currentScript` when the primary Z3 module
// itself is initialized inside a Worker. Give it the stable URL used to spawn
// its internal pthread Workers and to locate the sibling WASM asset.
const rawInitZ3 = globalThis.initZ3;
const z3ScriptUrl = new URL("./z3-built.js", self.location.href).href;
globalThis.initZ3 = (options = {}) => rawInitZ3({
  ...options,
  mainScriptUrlOrBlob: z3ScriptUrl,
  locateFile: (path) => new URL(path, self.location.href).href,
});

import { init } from "z3-solver/build/browser.js";

self.onmessage = async ({ data: { id, termCount } }) => {
  try {
    const { Context, em } = await init();
    const { Int, Solver } = new Context("fsl-browser-phase0");
    const solver = new Solver();
    const x = Int.const("x");
    const started = performance.now();
    for (let i = 0; i < termCount; i += 1) {
      solver.add(x.add(i).ge(i));
    }
    solver.add(x.eq(42));
    const constructMs = performance.now() - started;
    const verdict = String(await solver.check());
    const model = Number(solver.model().eval(x, true).value());
    self.postMessage({
      id,
      ok: verdict === "sat" && model === 42,
      result: {
        verdict,
        model: { x: model },
        constructMs,
        termCount,
        crossOriginIsolated: self.crossOriginIsolated,
        canTerminateThreads: typeof em?.PThread?.terminateAllThreads === "function",
      },
    });
    em?.PThread?.terminateAllThreads?.();
  } catch (error) {
    self.postMessage({
      id,
      ok: false,
      error: error instanceof Error ? error.stack : String(error),
    });
  }
};
