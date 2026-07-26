// SPDX-License-Identifier: Apache-2.0

import { spawn } from "node:child_process";
import { createReadStream, existsSync, readdirSync, statSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { homedir, tmpdir } from "node:os";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import "./build.mjs";
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
const nativeCommandTimeoutMs = 60_000;

async function command(executable, args, timeoutMs = 300_000) {
  return new Promise((resolveCommand, reject) => {
    const process = spawn(executable, args, { cwd: repository, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    let settled = false;
    const finish = (callback, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      callback(value);
    };
    process.stdout.setEncoding("utf8");
    process.stderr.setEncoding("utf8");
    process.stdout.on("data", (chunk) => { stdout += chunk; });
    process.stderr.on("data", (chunk) => { stderr += chunk; });
    const timeout = setTimeout(() => {
      process.kill("SIGKILL");
      finish(reject, new Error(`${executable} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
    process.on("error", (error) => finish(reject, error));
    process.on("close", (status) => {
      finish(resolveCommand, { status, stdout, stderr });
    });
  });
}

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
  const result = await command(nativeBinary, args, nativeCommandTimeoutMs);
  if (testCase.expected_status !== undefined && result.status !== testCase.expected_status) {
    throw new Error(
      `native CLI exit mismatch for ${testCase.path}: expected ${testCase.expected_status}, got ${result.status}`,
    );
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`native CLI JSON failure: ${error}\n${result.stderr}`);
  }
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
// Documents the Worker has no verb for, and therefore cannot be compared
// against the native CLI. Each entry records the document type and the
// *measured* reason the exclusion holds, so the next reader does not re-derive
// it. `exclusionProbes` below turns each entry into a self-retiring one: the
// harness probes the Worker for every excluded document and fails loudly when
// the Worker's behaviour no longer matches the recorded premise, i.e. when the
// exclusion has become unnecessary (#568). This is a *capability* exclusion,
// never a tolerated-difference allowlist: the native<->Worker envelopes are
// still not compared for these documents, and no verdict, location, or
// exit-code difference is allowlisted anywhere. A new entry must also keep
// native answering non-error for that document (#577, enforced below): once
// native errors too, the Worker-only probe can no longer tell agreement from
// a coincidence, which is what let the 28 retired refinement entries go
// stale silently.
const unsupportedReasons = {
  agent:
    "native `check` runs the lenient fsl-ai agent analysis (result \"ok\", dialect fsl-ai-agent.v0); "
    + "the Worker has no agent path at all and stops at the kernel lowering gate",
  causal:
    "standalone causal models bypass dialect dispatch (docs/DESIGN-causal.md); native answers with the "
    + "causal envelope (`causal_model_checked`, no `versions` block) which the shared parity normalizer "
    + "cannot validate, so there is no comparable pair to build",
};
const unsupportedDocuments = new Map(Object.entries({
  "examples/ai/recursive_support_agent.fsl": "agent",
  "examples/causal/incident_response.fsl": "causal",
  "examples/causal/marketing_funnel.fsl": "causal",
  "examples/causal/subscription_retention.fsl": "causal",
}));
const observedUnsupported = new Set();
const exclusionProbes = [];
const parityCases = [];
for (const path of candidates) {
  const repositoryPath = relative(repository, path).split("\\").join("/");
  const classified = await command(surfaceBinary, [path]);
  let documentType = "parse-error";
  if (classified.status === 0) {
    const ast = JSON.parse(classified.stdout);
    documentType = Array.isArray(ast) ? ast[0] : ast.$type?.toLowerCase();
  } else {
    // Standalone causal models bypass dialect dispatch (docs/DESIGN-causal.md);
    // detect them by their first significant declaration keyword.
    const stripped = (await readFile(path, "utf8"))
      .split("\n")
      .map((line) => line.replace(/\/\/.*$/, "").trim())
      .find((line) => line.length > 0);
    if (stripped !== undefined && /^causal\s/.test(stripped)) documentType = "causal";
  }
  if (["agent", "causal"].includes(documentType)) {
    if (unsupportedDocuments.get(repositoryPath) !== documentType) {
      throw new Error(`unreviewed unsupported ${documentType} document: ${repositoryPath}`);
    }
    observedUnsupported.add(repositoryPath);
    // Not compared against native -- probed on the Worker alone below, so
    // that the day the Worker grows a verb for this document type the
    // recorded premise stops matching and the exclusion fails instead of
    // silently persisting (#568).
    const probe = {
      id: `exclusion-${exclusionProbes.length}`,
      cmd: "check",
      path: repositoryPath,
      source: await readFile(path, "utf8"),
      source_file: repositoryPath,
      files: {},
      options: {},
      documentType,
    };
    exclusionProbes.push(probe);
    // That Worker-only probe is sound only because native also answers
    // non-error for this document: agreement then forces the Worker
    // non-error too, which the probe below already catches -- no envelope
    // comparison needed. If native answered "error" instead, a Worker error
    // would be indistinguishable from real agreement, which is exactly the
    // blindness that let the 28 refinement entries retired in #577 go stale
    // silently (both sides erroring looked the same whether or not the two
    // errors actually matched). Checked here, before the ~6-minute Worker
    // run, because it depends on nothing but native.
    const probeNativeEnvelope = await nativeEnvelope(probe);
    if (probeNativeEnvelope.result === "error") {
      throw new Error(
        `${repositoryPath}: native's own result is "error" for this ${documentType} document, so `
        + `a Worker-only probe cannot distinguish real native<->Worker agreement from two unrelated `
        + `errors that both happen to be "error". Compare the pair explicitly (native vs Worker `
        + `envelope) or retire the entry, the way the 28 refinement entries were retired in #577.`,
      );
    }
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
const governanceErrorCase = "examples/gallery/errors/governance_missing_before.fsl";
if (!parityCases.some((testCase) => testCase.path === governanceErrorCase)) {
  throw new Error(`${governanceErrorCase} must remain in the parity corpus`);
}
// #577: anchors the retirement of the 28 stale refinement exclusions. Native
// and Worker check/verify envelopes now agree for every refinement document
// in the corpus (the exclusion premise measured in #568 no longer holds), so
// this document must be a compared parity case, not a Worker-only exclusion
// probe. If a future change silently re-excludes refinement documents, this
// fails instead of only showing up as a quiet drop in `parityCases.length`.
const retiredRefinementCase = "specs/cart_refines.fsl";
if (!parityCases.some((testCase) => testCase.path === retiredRefinementCase)) {
  throw new Error(`${retiredRefinementCase} must remain in the parity corpus`);
}
// The only corpus input whose kernel-stage failure
// (`fsl_core::parse_kernel_source_with_file`) keeps its own located message
// instead of the generic substituted "spec has no state block" that
// refinement documents get (`kernel_load_error` in spec_load.rs) -- while its
// own top level parses. Every other located kernel-stage failure under
// specs/+examples/ is an agent or causal document this harness still excludes
// as unsupported (refinement is now compared directly, #577), so without this
// case the Worker could classify a located kernel-stage message differently
// from native and no parity run would notice (issue #556).
const kernelStageCase = "examples/gallery/errors/semantics_compose_component_parse_failure.fsl";
if (!parityCases.some((testCase) => testCase.path === kernelStageCase)) {
  throw new Error(`${kernelStageCase} must remain in the parity corpus`);
}
const missingGovernanceDependency = "rust/fslc/tests/fixtures/governance_missing_dependency.fsl";
const missingGovernanceSource = await readFile(join(repository, missingGovernanceDependency), "utf8");
parityCases.push({
  id: `parity-${parityCases.length}`,
  cmd: "check",
  path: missingGovernanceDependency,
  source: missingGovernanceSource,
  source_file: missingGovernanceDependency,
  files: {},
  options: {},
  expected_status: 2,
});
await writeFile(
  join(dist, "parity-cases.json"),
  `${JSON.stringify([...parityCases, ...exclusionProbes])}\n`,
  "utf8",
);
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

// Playwright installs each downloaded browser under its own version-numbered
// directory, and the platform subdirectory inside it is itself
// version-dependent, not just architecture-dependent: measured on this box,
// chromium_headless_shell-1208 nests
// chrome-headless-shell-mac-arm64/chrome-headless-shell, while the older
// chromium_headless_shell-1187 nests chrome-mac/headless_shell -- a
// different directory name *and* a different executable basename, not a
// mac-arm64/mac-x64 suffix swap. Neither the version number nor the platform
// directory name is worth hardcoding (an Intel Mac's
// chrome-headless-shell-mac-x64 would silently never match a hardcoded
// mac-arm64 path), so every version directory's actual subdirectories are
// enumerated and both observed executable basenames are tried in each,
// rather than guessing at layouts nobody has measured.
function playwrightHeadlessShellCandidates() {
  const versionsRoot = join(homedir(), "Library/Caches/ms-playwright");
  if (!existsSync(versionsRoot)) return [];
  const versions = readdirSync(versionsRoot)
    .filter((name) => name.startsWith("chromium_headless_shell-"))
    .sort()
    .reverse();
  return versions.flatMap((version) => {
    const versionRoot = join(versionsRoot, version);
    let platforms;
    try {
      platforms = readdirSync(versionRoot, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .map((entry) => entry.name);
    } catch {
      return [];
    }
    return platforms.flatMap((platform) => [
      join(versionRoot, platform, "chrome-headless-shell"),
      join(versionRoot, platform, "headless_shell"),
    ]);
  });
}
const chrome = [
  process.env.CHROME_BIN,
  ...playwrightHeadlessShellCandidates(),
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
const cdpTimeoutMs = 30_000;
// Unlike command(), which has always had a timeout, a dropped or half-open
// CDP connection used to leave the returned promise unsettled forever: the
// message listener is the only thing that ever resolved or rejected a
// pending entry, so a response that never arrives hung node indefinitely,
// and the `for (attempt < 360)` probe loop below is not actually a bound
// once a single `await cdp(...)` inside it never returns (#584). Both the
// per-call timeout here and rejectPending's socket close/error handlers
// below turn that hang into a loud, named failure, which lets the
// surrounding `finally` still run and clean up the child process and
// profile directory.
// A CDP timeout is rare and not reproducible on demand (#587: one failure in
// five full local runs, none in CI), so the single message it prints is all
// the evidence anyone gets. Report what separates the candidate causes rather
// than only that time ran out:
//
//   - `call id` is the ordinal. id 1 is `Runtime.enable`, so a timeout there
//     means the connection never became usable, while a later id means the
//     page stopped answering mid-poll. That is the difference between
//     suspecting Chrome startup and suspecting the page's main thread.
//   - `socket` separates a transport still OPEN -- nobody is answering -- from
//     one already CLOSING/CLOSED, where the close handler lost the race.
//   - Chrome's stderr is already accumulated but is otherwise printed only on
//     a startup failure. A crash or renderer message in it names the cause.
//
// Deliberately no retry: retrying an unexplained stall would hide the event
// this diagnostic exists to capture, and a real hang would return as a slow
// pass instead of a failure.
const READY_STATE = ["CONNECTING", "OPEN", "CLOSING", "CLOSED"];
function cdp(socket, method, params = {}) {
  const id = nextId;
  nextId += 1;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(id);
      const state = READY_STATE[socket.readyState] ?? `unknown(${socket.readyState})`;
      const chromeStderr = stderr.trim();
      reject(new Error(
        `CDP method "${method}" timed out after ${cdpTimeoutMs}ms `
        + `(call id ${id}, socket ${state}, ${pending.size} other request(s) outstanding); `
        + `see issue 587. Chrome stderr: `
        + `${chromeStderr === "" ? "<empty>" : `\n${chromeStderr}`}`,
      ));
    }, cdpTimeoutMs);
    pending.set(id, {
      resolve: (value) => { clearTimeout(timeout); resolve(value); },
      reject: (error) => { clearTimeout(timeout); reject(error); },
    });
  });
}
function rejectPending(error) {
  for (const [id, waiter] of pending) {
    pending.delete(id);
    waiter.reject(error);
  }
}
let details;
try {
  const debugPort = await devtoolsPort();
  const targets = await fetch(`http://127.0.0.1:${debugPort}/json/list`).then((response) => response.json());
  const page = targets.find((target) => target.type === "page");
  if (!page) throw new Error("Chrome did not expose a page target");
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  socket.addEventListener("close", () => rejectPending(new Error("CDP socket closed with requests outstanding")));
  socket.addEventListener("error", () => rejectPending(new Error("CDP socket error with requests outstanding")));
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
// #568: an exclusion that is no longer needed must fail, not stay green. Each
// excluded document is probed on the Worker; the recorded premise is that the
// Worker cannot analyze it. `check` on a document the Worker has no verb for
// stops at the kernel lowering gate (refinement/agent) or fails the surface
// parse (causal) -- either way it never produces an analysis. The day the
// Worker gains the verb, `result` stops being `"error"` and this fails,
// naming the entry to remove.
//
// This Worker-only check is sound only because native also answers
// non-error for every excluded document, which is asserted separately in the
// candidate-collection loop above, before the Worker even runs (#577): when
// native is non-error, Worker/native agreement forces the Worker non-error
// too, so this check alone already catches staleness -- no envelope
// comparison needed. See the comment on that earlier assertion for why it
// would be unsound to skip.
const staleExclusions = [];
for (let index = 0; index < exclusionProbes.length; index += 1) {
  const probe = exclusionProbes[index];
  const envelope = browser.parityEnvelopes[parityCases.length + index];
  if (envelope?.result !== "error") {
    staleExclusions.push(
      `${probe.path}: the Worker now returns ${JSON.stringify(envelope?.result)} for this `
      + `${probe.documentType} document, so the unsupportedDocuments entry is stale. `
      + `Recorded reason: ${unsupportedReasons[probe.documentType]}. `
      + `Remove the entry and let the document join the compared corpus, or update the reason.`,
    );
  }
}
if (staleExclusions.length > 0) {
  throw new Error(`stale parity-corpus exclusions:\n${staleExclusions.join("\n")}`);
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
  exclusionProbes: exclusionProbes.length,
}, null, 2));
