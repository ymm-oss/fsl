// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

import { cp, mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const root = dirname(fileURLToPath(import.meta.url));
const dist = resolve(root, "dist");
await mkdir(dist, { recursive: true });

await build({
  entryPoints: [resolve(root, "browser-worker-entry.mjs")],
  outfile: resolve(dist, "worker.js"),
  bundle: true,
  format: "iife",
  platform: "browser",
  target: ["chrome120", "firefox120", "safari17"],
  define: { global: "globalThis" },
});

const z3Build = resolve(root, "node_modules/z3-solver/build");
await cp(resolve(z3Build, "z3-built.js"), resolve(dist, "z3-built.js"));
await cp(resolve(z3Build, "z3-built.wasm"), resolve(dist, "z3-built.wasm"));
await cp(resolve(root, "browser-main.mjs"), resolve(dist, "browser-main.mjs"));
await writeFile(
  resolve(dist, "index.html"),
  `<!doctype html>
<html><head><meta charset="utf-8"><title>fsl z3js browser spike</title></head>
<body><pre id="result" data-done="false">pending</pre>
<script type="module" src="./browser-main.mjs"></script></body></html>\n`,
  "utf8",
);
