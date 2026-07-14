// SPDX-License-Identifier: Apache-2.0

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { cp, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const execute = promisify(execFile);
const root = dirname(fileURLToPath(import.meta.url));
const workspace = resolve(root, "..");
const pkg = resolve(root, "pkg");
const dist = resolve(root, "dist");
await mkdir(pkg, { recursive: true });
await mkdir(dist, { recursive: true });

await execute("cargo", ["build", "-p", "fsl-wasm", "--target", "wasm32-unknown-unknown", "--release"], {
  cwd: workspace,
});
await execute("wasm-bindgen", [
  resolve(workspace, "target/wasm32-unknown-unknown/release/fsl_wasm.wasm"),
  "--target", "web",
  "--out-dir", pkg,
]);

await build({
  entryPoints: [resolve(root, "web/worker-entry.mjs")],
  outfile: resolve(dist, "worker.js"),
  bundle: true,
  format: "iife",
  platform: "browser",
  target: ["chrome120", "firefox120", "safari17"],
  define: { global: "globalThis", "import.meta.url": "self.location.href" },
});

const z3Build = resolve(root, "node_modules/z3-solver/build");
await cp(resolve(z3Build, "z3-built.js"), resolve(dist, "z3-built.js"));
await cp(resolve(z3Build, "z3-built.wasm"), resolve(dist, "z3-built.wasm"));
await cp(resolve(pkg, "fsl_wasm_bg.wasm"), resolve(dist, "fsl_wasm_bg.wasm"));
await cp(resolve(root, "web/index.html"), resolve(dist, "index.html"));
await cp(resolve(root, "web/client.mjs"), resolve(dist, "client.mjs"));
await cp(resolve(root, "web/cases.mjs"), resolve(dist, "cases.mjs"));
await cp(resolve(root, "web/worker-protocol.mjs"), resolve(dist, "worker-protocol.mjs"));
