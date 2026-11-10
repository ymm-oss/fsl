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
import { exitStatusFor } from "./browser-probe-outcome.mjs";
import { runBrowserProbe } from "./browser-probe-run.mjs";

async function main() {
const dist = fileURLToPath(new URL("./dist/", import.meta.url));
let resolveBrowserResult;
const browserResult = new Promise((resolve) => { resolveBrowserResult = resolve; });
const mime = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
};
const server = createServer((request, response) => {
  if (request.method === "POST" && request.url === "/result") {
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk) => { body += chunk; });
    request.on("end", () => {
      try {
        const result = JSON.parse(body);
        response.writeHead(204).end();
        resolveBrowserResult(result);
      } catch (error) {
        response.writeHead(400).end(String(error));
      }
    });
    return;
  }
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
const candidates = [
  process.env.CHROME_BIN,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
].filter(Boolean);
const chrome = candidates.find(existsSync);
if (!chrome) {
  throw new Error("Chrome not found; set CHROME_BIN to run the browser Worker probe");
}

const result = await runBrowserProbe({
  startServer: async () => {
    await new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => {
        server.removeListener("error", reject);
        resolve();
      });
    });
    return {
      result: browserResult,
      close: () => new Promise((resolve) => server.close(resolve)),
    };
  },
  makeProfile: () => mkdtemp(join(tmpdir(), "fsl-z3js-chrome-")),
  launchChrome: (profile) => {
    const { port } = server.address();
    const child = spawn(
      chrome,
      [
        "--headless=new",
        "--disable-gpu",
        "--disable-background-networking",
        "--no-sandbox",
        `--user-data-dir=${profile}`,
        `http://127.0.0.1:${port}/`,
      ],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
    let stderr = "";
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    return {
      closed: new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("close", (code, signal) => resolve({ code, signal }));
      }),
      kill: () => {
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
      },
      stderr: () => stderr,
    };
  },
  removeProfile: (profile) => rm(profile, { recursive: true, force: true }),
  timeoutMs: 75000,
});

if (!result.ok) {
  throw new Error(`browser Worker round trip failed: ${JSON.stringify(result)}`);
}
console.log(JSON.stringify({ schema: "fsl-z3js-browser-spike.v1", ok: true, details: result.details }, null, 2));
}

try {
  await main();
} catch (error) {
  console.error(error);
  process.exitCode = exitStatusFor(error);
}
