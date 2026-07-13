// SPDX-License-Identifier: Apache-2.0

import { cp, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import "./build.mjs";

const root = dirname(fileURLToPath(import.meta.url));
const target = resolve(root, "../../docs/intro/fsl-wasm");
await mkdir(target, { recursive: true });
for (const asset of ["worker.js", "fsl_wasm_bg.wasm", "z3-built.js", "z3-built.wasm"]) {
  await cp(resolve(root, "dist", asset), resolve(target, asset));
}
