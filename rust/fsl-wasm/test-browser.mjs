// SPDX-License-Identifier: Apache-2.0

import { spawn } from "node:child_process";
import { createReadStream, existsSync, statSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import "./build.mjs";
import { cases } from "./web/cases.mjs";
import { assertNormalizerContract, differences, normalizeEnvelope } from "./parity.mjs";
import { workerMessageError } from "./web/worker-protocol.mjs";

assertNormalizerContract();

const protocolError = workerMessageError({
  transportError: { kind: "initialization", message: "probe" },
});
if (protocolError?.message !== "initialization: probe" || workerMessageError({ envelope: {} })) {
  throw new Error("Worker transport-error protocol is not observable by the client");
}

const dist = fileURLToPath(new URL("./dist/", import.meta.url));
const repository = resolve(fileURLToPath(new URL("../../", import.meta.url)));

async function command(executable, args) {
  return new Promise((resolveCommand, reject) => {
    const process = spawn(executable, args, { cwd: repository, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    process.stdout.setEncoding("utf8");
    process.stderr.setEncoding("utf8");
    process.stdout.on("data", (chunk) => { stdout += chunk; });
    process.stderr.on("data", (chunk) => { stderr += chunk; });
    process.on("error", reject);
    process.on("close", (status) => resolveCommand({ status, stdout, stderr }));
  });
}

async function collectFslFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return collectFslFiles(path);
    return entry.isFile() && entry.name.endsWith(".fsl") ? [path] : [];
  }));
  return nested.flat();
}

const surfaceBuild = await command("cargo", [
  "build", "--manifest-path", "rust/Cargo.toml", "-p", "fsl-syntax",
  "--bin", "fsl-parse-surface", "--locked",
]);
if (surfaceBuild.status !== 0) throw new Error(`surface classifier build failed: ${surfaceBuild.stderr}`);
const surfaceBinary = join(repository, "rust/target/debug/fsl-parse-surface");
const candidates = [
  ...await collectFslFiles(join(repository, "specs")),
  ...await collectFslFiles(join(repository, "examples")),
].sort();
const unsupportedDocuments = new Map(Object.entries({
  "examples/agentic_rag/agentic_rag_design_refines_requirements.fsl": "refinement",
  "examples/agentic_rag/agentic_rag_requirements_refines_business.fsl": "refinement",
  "examples/agentic_rag/negative/guard_bypass_refines_requirements.fsl": "refinement",
  "examples/agentic_rag/negative/liveness_drop_refines_requirements.fsl": "refinement",
  "examples/agentic_rag/negative/tool_approval_bypass_refines_requirements.fsl": "refinement",
  "examples/ai/recursive_support_agent.fsl": "agent",
  "examples/consulting/tobe_refines_asis.fsl": "refinement",
  "examples/e2e/3_refines_2.fsl": "refinement",
  "examples/gallery/adversarial/refine_mapping_boundary_map.fsl": "refinement",
  "examples/gallery/errors/refinement_failed_map.fsl": "refinement",
  "examples/layers/return_impl_refines.fsl": "refinement",
  "examples/multi_agent_system/multi_agent_design_refines_requirements.fsl": "refinement",
  "examples/multi_agent_system/multi_agent_requirements_refines_business.fsl": "refinement",
  "examples/nfr/sla_worker_refines.fsl": "refinement",
  "examples/refinement_chain/bot_refines_mid.fsl": "refinement",
  "examples/refinement_chain/mid_refines_top.fsl": "refinement",
  "examples/refinement_liveness/design_bypasses_control_refines.fsl": "refinement",
  "examples/refinement_liveness/design_drops_liveness_progress_refines.fsl": "refinement",
  "examples/refinement_liveness/design_drops_liveness_refines.fsl": "refinement",
  "examples/refinement_liveness/design_keeps_liveness_progress_refines.fsl": "refinement",
  "examples/refinement_liveness/design_keeps_liveness_refines.fsl": "refinement",
  "examples/ui_spike/ui_refines_req.fsl": "refinement",
  "examples/validation/order_refund_instant_refines.fsl": "refinement",
  "examples/validation/order_refund_windowed_refines.fsl": "refinement",
  "specs/bank_refines.fsl": "refinement",
  "specs/cart_refines.fsl": "refinement",
  "specs/seat_refines.fsl": "refinement",
}));
const observedUnsupported = new Set();
const parityCases = [];
for (const path of candidates) {
  const repositoryPath = relative(repository, path).split("\\").join("/");
  const classified = await command(surfaceBinary, [path]);
  let documentType = "parse-error";
  if (classified.status === 0) {
    const ast = JSON.parse(classified.stdout);
    documentType = Array.isArray(ast) ? ast[0] : ast.$type?.toLowerCase();
  }
  if (["agent", "refinement"].includes(documentType)) {
    if (unsupportedDocuments.get(repositoryPath) !== documentType) {
      throw new Error(`unreviewed unsupported ${documentType} document: ${repositoryPath}`);
    }
    observedUnsupported.add(repositoryPath);
    continue;
  }
  if (unsupportedDocuments.has(repositoryPath)) {
    throw new Error(`unsupported-document classification changed: ${repositoryPath}`);
  }
  const source = await readFile(path, "utf8");
  const files = {};
  for (const match of source.matchAll(/\b(?:from|refinement)\s+"([^"]+)"/g)) {
    files[match[1]] = await readFile(resolve(dirname(path), match[1]), "utf8");
  }
  for (const cmd of ["check", "verify"]) {
    parityCases.push({
      id: `parity-${parityCases.length}`,
      cmd,
      path: repositoryPath,
      source,
      source_file: repositoryPath,
      files,
      options: cmd === "verify" ? { depth: 3, deadlock: "warn" } : {},
    });
  }
}
for (const path of unsupportedDocuments.keys()) {
  if (!observedUnsupported.has(path)) {
    throw new Error(`reviewed unsupported document disappeared: ${path}`);
  }
}
const duplicateWriteCase = "examples/gallery/errors/semantics_duplicate_assignment.fsl";
if (!parityCases.some((testCase) => testCase.path === duplicateWriteCase)) {
  throw new Error(`${duplicateWriteCase} must remain in the parity corpus`);
}
await writeFile(join(dist, "parity-cases.json"), `${JSON.stringify(parityCases)}\n`, "utf8");
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
async function nativeEnvelope(testCase) {
  const args = testCase.cmd === "check"
    ? ["check", testCase.path]
    : [
      "verify", testCase.path,
      "--depth", String(testCase.options.depth),
      "--deadlock", testCase.options.deadlock,
      "--no-cache",
    ];
  return new Promise((resolve, reject) => {
    const childProcess = spawn(nativeBinary, args, {
      cwd: repository,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    childProcess.stdout.setEncoding("utf8");
    childProcess.stderr.setEncoding("utf8");
    childProcess.stdout.on("data", (chunk) => { stdout += chunk; });
    childProcess.stderr.on("data", (chunk) => { stderr += chunk; });
    childProcess.on("error", reject);
    childProcess.on("close", () => {
      try {
        const envelope = JSON.parse(stdout);
        resolve(envelope);
      } catch (error) { reject(new Error(`native CLI JSON failure: ${error}\n${stderr}`)); }
    });
  });
}
const native = [];
for (const testCase of parityCases) {
  native.push(await nativeEnvelope(testCase));
}
const mismatches = [];
for (let index = 0; index < parityCases.length; index += 1) {
  const nativeEnvelope = native[index];
  const wasmEnvelope = browser.parityEnvelopes[index];
  const envelopeDifferences = differences(
    normalizeEnvelope(nativeEnvelope),
    normalizeEnvelope(wasmEnvelope),
  );
  if (envelopeDifferences.length > 0) {
    mismatches.push({
      schema: "fsl-native-wasm-parity-failure.v1",
      case: {
        path: parityCases[index].path,
        command: parityCases[index].cmd,
        options: parityCases[index].options,
      },
      differences: envelopeDifferences,
      native: nativeEnvelope,
      wasm: wasmEnvelope,
    });
  }
}
if (mismatches.length > 0) {
  const report = JSON.stringify({
    schema: "fsl-native-wasm-parity-failure.v1",
    mismatches,
  }, null, 2);
  const reportPath = join(tmpdir(), "fsl-native-wasm-parity-failure.json");
  await writeFile(reportPath, `${report}\n`, "utf8");
  throw new Error(`${report}\nfull report: ${reportPath}`);
}
console.log(JSON.stringify({
  schema: "fsl-wasm-browser.v1",
  ok: true,
  cancelled: true,
  nativeParity: true,
  parityCases: parityCases.length,
}, null, 2));
