// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

import { spawn } from "node:child_process";
import { createReadStream, existsSync, statSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

import "./build-browser.mjs";

const dist = fileURLToPath(new URL("./dist/", import.meta.url));
const mime = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
};
const server = createServer((request, response) => {
  const relative = request.url === "/" ? "index.html" : request.url.slice(1).split("?")[0];
  const path = join(dist, relative);
  if (!path.startsWith(dist) || !existsSync(path) || !statSync(path).isFile()) {
    response.writeHead(404).end("not found");
    return;
  }
  response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  response.setHeader("Cross-Origin-Resource-Policy", "same-origin");
  response.setHeader("Content-Type", mime[extname(path)] ?? "application/octet-stream");
  createReadStream(path).pipe(response);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address();

const candidates = [
  process.env.CHROME_BIN,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
].filter(Boolean);
const chrome = candidates.find(existsSync);
if (!chrome) {
  server.close();
  throw new Error("Chrome not found; set CHROME_BIN to run the browser Worker probe");
}

const profile = await mkdtemp(join(tmpdir(), "fsl-z3js-chrome-"));
const child = spawn(
  chrome,
  [
    "--headless=new",
    "--disable-gpu",
    "--disable-background-networking",
    "--no-sandbox",
    "--dump-dom",
    "--virtual-time-budget=60000",
    `--user-data-dir=${profile}`,
    `http://127.0.0.1:${port}/`,
  ],
  { stdio: ["ignore", "pipe", "pipe"] },
);
let stdout = "";
let stderr = "";
child.stdout.setEncoding("utf8");
child.stderr.setEncoding("utf8");
child.stdout.on("data", (chunk) => { stdout += chunk; });
child.stderr.on("data", (chunk) => { stderr += chunk; });
let status;
try {
  status = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`Chrome browser probe timed out after 75s: ${stderr}`));
    }, 75_000);
    child.once("error", reject);
    child.once("close", (code) => {
      clearTimeout(timeout);
      resolve(code);
    });
  });
} finally {
  server.close();
  await rm(profile, { recursive: true, force: true });
}

if (status !== 0) {
  throw new Error(`Chrome failed (${status}): ${stderr}`);
}
const match = stdout.match(/<pre id="result" data-done="true" data-ok="(true|false)">([^<]*)<\/pre>/);
if (!match) {
  throw new Error(`browser probe did not finish:\n${stdout}\n${stderr}`);
}
const details = match[2]
  .replaceAll("&quot;", '"')
  .replaceAll("&amp;", "&")
  .replaceAll("&lt;", "<")
  .replaceAll("&gt;", ">");
if (match[1] !== "true") {
  throw new Error(`browser Worker round trip failed: ${details}`);
}
console.log(JSON.stringify({ schema: "fsl-z3js-browser-spike.v1", ok: true, details: JSON.parse(details) }, null, 2));
