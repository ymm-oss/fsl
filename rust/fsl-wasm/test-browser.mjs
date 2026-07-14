// SPDX-License-Identifier: Apache-2.0

import { spawn } from "node:child_process";
import { createReadStream, existsSync, statSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";

import "./build.mjs";
import { cases } from "./web/cases.mjs";

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

const chrome = [
  process.env.CHROME_BIN,
  "/Users/rizumita/Library/Caches/ms-playwright/chromium_headless_shell-1208/chrome-headless-shell-mac-arm64/chrome-headless-shell",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
].filter(Boolean).find(existsSync);
if (!chrome) throw new Error("Chrome not found; set CHROME_BIN");

const profile = await mkdtemp(join(tmpdir(), "fsl-wasm-chrome-"));
const child = spawn(chrome, [
  "--headless=new", "--disable-gpu", "--disable-background-networking", "--no-sandbox",
  "--password-store=basic", "--use-mock-keychain",
  "--remote-debugging-port=0", `--user-data-dir=${profile}`,
  `http://127.0.0.1:${port}/`,
], { stdio: ["ignore", "pipe", "pipe"] });
const childClosed = new Promise((resolve) => child.once("close", resolve));
let stderr = "";
child.stderr.setEncoding("utf8");
child.stderr.on("data", (chunk) => { stderr += chunk; });
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
async function devtoolsPort() {
  for (let attempt = 0; attempt < 80; attempt += 1) {
    try {
      const [portLine] = (await readFile(join(profile, "DevToolsActivePort"), "utf8")).split("\n");
      return Number(portLine);
    } catch {
      await delay(100);
    }
  }
  throw new Error(`DevTools port did not appear: ${stderr}`);
}
let nextId = 1;
const pending = new Map();
function cdp(socket, method, params = {}) {
  const id = nextId;
  nextId += 1;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}
let details;
try {
  const debugPort = await devtoolsPort();
  const targets = await fetch(`http://127.0.0.1:${debugPort}/json/list`).then((response) => response.json());
  const page = targets.find((target) => target.type === "page");
  if (!page) throw new Error("Chrome did not expose a page target");
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data);
    if (!message.id || !pending.has(message.id)) return;
    const waiter = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(message.error.message));
    else waiter.resolve(message.result);
  });
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  await cdp(socket, "Runtime.enable");
  for (let attempt = 0; attempt < 360; attempt += 1) {
    const evaluated = await cdp(socket, "Runtime.evaluate", {
      expression: `(() => { const node = document.querySelector('#result'); return node ? { done: node.dataset.done, ok: node.dataset.ok, text: node.textContent } : null; })()`,
      returnByValue: true,
    });
    details = evaluated.result.value;
    if (details?.done === "true") break;
    await delay(250);
  }
  socket.close();
  if (details?.done !== "true") throw new Error(`browser probe timed out: ${stderr}`);
} finally {
  if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  await childClosed;
  await new Promise((resolve, reject) => {
    server.close((error) => error ? reject(error) : resolve());
  });
  await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
if (details.ok !== "true") {
  throw new Error(`FSL WASM Worker smoke failed: ${details.text}\n${stderr}`);
}
const browser = JSON.parse(details.text);
if (!browser.cancelled) throw new Error("Worker cancellation did not complete");
const nativeBinary = fileURLToPath(new URL("../target/debug/fslc", import.meta.url));
async function nativeVerdict(testCase, index) {
  const path = join(tmpdir(), `fsl-wasm-native-${process.pid}-${index}.fsl`);
  await writeFile(path, testCase.source, "utf8");
  const args = [
    "verify", path,
    "--depth", String(testCase.options.depth),
    "--deadlock", testCase.options.deadlock,
  ];
  return new Promise((resolve, reject) => {
    const childProcess = spawn(nativeBinary, args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    childProcess.stdout.setEncoding("utf8");
    childProcess.stderr.setEncoding("utf8");
    childProcess.stdout.on("data", (chunk) => { stdout += chunk; });
    childProcess.stderr.on("data", (chunk) => { stderr += chunk; });
    childProcess.on("error", reject);
    childProcess.on("close", () => {
      try { resolve(JSON.parse(stdout).result); }
      catch (error) { reject(new Error(`native CLI JSON failure: ${error}\n${stderr}`)); }
    });
  });
}
const native = [];
for (let index = 0; index < cases.length; index += 1) {
  native.push(await nativeVerdict(cases[index], index));
}
const wasm = browser.envelopes.map((envelope) => envelope.result);
if (JSON.stringify(native) !== JSON.stringify(wasm)) {
  throw new Error(`native/WASM verdict mismatch: native=${native} wasm=${wasm}`);
}
console.log(JSON.stringify({ schema: "fsl-wasm-browser.v1", ok: true, cancelled: true, nativeParity: true }, null, 2));
