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
import { classifyOutcome, ParityViolationError, ProbeTimeoutError } from "./browser-outcome.mjs";
import { specImports } from "./spec-imports.mjs";
import { waitForCompletion } from "./completion-wait.mjs";

function logPhase(phase, detail = "") {
  console.error(`[${new Date().toISOString()}] fsl-wasm browser phase=${phase}${detail ? ` ${detail}` : ""}`);
}

async function main() {
  assertNormalizerContract();

// Keep the body unindented: rust/fslc/tests/assurance/dialects.rs:31-41
// uses source-substring anchors for Worker parity and exclusion ownership.
const protocolError = workerMessageError({
  transportError: { kind: "initialization", message: "probe" },
});
if (protocolError?.message !== "initialization: probe" || workerMessageError({ envelope: {} })) {
  throw new Error("Worker transport-error protocol is not observable by the client");
}

const dist = fileURLToPath(new URL("./dist/", import.meta.url));
const repository = resolve(fileURLToPath(new URL("../../", import.meta.url)));
const nativeCommandTimeoutMs = 60_000;

async function command(executable, args, timeoutMs = 300_000, environment = process.env) {
  return new Promise((resolveCommand, reject) => {
    const process = spawn(executable, args, { cwd: repository, env: environment, stdio: ["ignore", "pipe", "pipe"] });
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
  const result = await command(nativeBinary, args, nativeCommandTimeoutMs,
    testCase.dependency_edition ? { ...process.env, FSLC_UNSTABLE: "fsl5", FSLC_CACHE: "off" } : process.env);
  if (testCase.id?.startsWith("1467/")) {
    await writeFile(join(dist, `issue-${testCase.id.replaceAll("/", "-")}-native-command.json`),
      JSON.stringify({ binary: nativeBinary, args, cwd: repository, ...result }, null, 2));
  }
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
  causal:
    "standalone causal models bypass dialect dispatch (docs/design/DESIGN-causal.md); native answers with the "
    + "causal envelope (`causal_model_checked`, no `versions` block) which the shared parity normalizer "
    + "cannot validate, so there is no comparable pair to build",
};
const unsupportedDocuments = new Map(Object.entries({
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
    // Standalone causal models bypass dialect dispatch (docs/design/DESIGN-causal.md);
    // detect them by their first significant declaration keyword.
    const stripped = (await readFile(path, "utf8"))
      .split("\n")
      .map((line) => line.replace(/\/\/.*$/, "").trim())
      .find((line) => line.length > 0);
    if (stripped !== undefined && /^causal\s/.test(stripped)) documentType = "causal";
  }
  if (documentType === "causal") {
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
  for (const imported of specImports(source, repositoryPath)) {
    files[imported] = await readFile(resolve(dirname(path), imported), "utf8");
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
// #1303: a failed governance preservation is the check/verify verdict
// (`refinement_failed`), not a nested field under an `ok`/`verified` envelope.
// Worker `verify` folds it only after its browser solver run, so this corpus
// document is the one place Worker and native verify envelopes for a failing
// preservation are compared; both its check and verify cases must stay.
const failedPreservationCase = "examples/refinement_liveness/governance_detects_safety_loss.fsl";
for (const cmd of ["check", "verify"]) {
  if (!parityCases.some((testCase) => testCase.path === failedPreservationCase && testCase.cmd === cmd)) {
    throw new Error(`${failedPreservationCase} (${cmd}) must remain in the parity corpus`);
  }
}
// #1599: progress-bearing preservation must stay in full native/Worker parity.
const progressMergeCase = "examples/refinement_liveness/governance_detects_progress_merge_collision.fsl";
for (const cmd of ["check", "verify"]) {
  if (!parityCases.some((testCase) => testCase.path === progressMergeCase && testCase.cmd === cmd)) {
    throw new Error(`${progressMergeCase} (${cmd}) must remain in the parity corpus`);
  }
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
// #1449: accepted current-edition warning finalization runs after the main
// browser BMC. Keep this verify case in full-envelope parity with depth 3 / warn
// and reject a vacuous agreement where both surfaces silently drop warnings.
const editionWarningCase = "examples/named_predicate.fsl";
const editionWarningParity = parityCases.find(
  (testCase) => testCase.path === editionWarningCase && testCase.cmd === "verify",
);
if (!editionWarningParity) {
  throw new Error(`${editionWarningCase} (verify) must remain in the parity corpus`);
}
const editionWarningNative = await nativeEnvelope(editionWarningParity);
if (editionWarningNative.result !== "verified"
    || !Array.isArray(editionWarningNative.warnings) || editionWarningNative.warnings.length === 0
    || editionWarningNative.warnings.filter((warning) => warning.code === "legacy_string_metadata"
      && warning.edition === "current" && warning.severity === "warning").length !== 1) {
  throw new Error(`${editionWarningCase} must verify with one current-edition metadata warning`);
}
// #1163: the Worker's `check` now runs the shared `check_stages`, including
// the lenient Agent analysis native `check` runs, and its `verify` rejects an
// Agent document the way native `verify` does. The corpus's one Agent
// document is therefore a compared parity case (check and verify), not a
// Worker-only exclusion probe; a silent re-exclusion fails here.
const agentParityCase = "examples/ai/recursive_support_agent.fsl";
if (!parityCases.some((testCase) => testCase.path === agentParityCase)) {
  throw new Error(`${agentParityCase} must remain in the parity corpus`);
}
// The only corpus input whose kernel-stage failure
// (`fsl_core::parse_kernel_source_with_file`) keeps its own located message
// instead of the generic substituted "spec has no state block" that
// refinement documents get (`kernel_load_error` in spec_load.rs) -- while its
// own top level parses. Every other located kernel-stage failure under
// specs/+examples/ is a causal document this harness still excludes as
// unsupported (refinement is now compared directly, #577; the Agent document
// never reaches the kernel stage, #1163), so without this
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
// #1457: require real verdicts, not parity between two unsupported errors.
// Positive cases force the symbolic route; negative controls also preserve
// the exact deterministic witness instead of only the normalizer's trace shape.
for (const binder of ["seq_literal", "set_statement"]) {
  const fixture = `rust/fslc/tests/fixtures/issue_1457_worker_${binder}.fsl`;
  const positive = await readFile(join(repository, fixture), "utf8");
  for (const violated of [false, true]) {
    const source = violated ? positive.replace("not done or flag", "not done or not flag") : positive;
    const path = relative(repository, join(dist, `issue-1457-${binder}-${violated}.fsl`)).split("\\").join("/");
    await writeFile(join(repository, path), source, "utf8");
    parityCases.push({
      id: `parity-${parityCases.length}`,
      cmd: "verify",
      path,
      source,
      source_file: path,
      files: {},
      options: { depth: 4, deadlock: "warn" },
      expected_status: violated ? 1 : 0,
      expected_result: violated ? "violated" : "verified",
      exact_collection_witness: true,
    });
  }
}
// Exact issue-pair rejection and one-time warning, Linux WASM integration only.
// The native test seam cannot run the pre-fix main browser BMC. Read the same
// fixture files as the shared Rust table, rather than maintaining byte copies.
// The existing batch protocol retains explicit unstable options; no product
// transport or solver-injection surface is changed here.
const dependencyRoot = await readFile(join(repository, "rust/fsl-wasm/tests/support/dependency_root.txt"), "utf8");
const dependencyVerify = JSON.parse(await readFile(join(repository, "rust/fsl-wasm/tests/support/dependency_verify.json"), "utf8"));
delete dependencyVerify._license;
const dependencyLegacy = await readFile(join(repository, editionWarningCase), "utf8");
const dependencyCases = [];
// #1467: real JS Worker batch, independent literal recursive value witnesses.
const recursiveCases = [];
const recursiveSource = `spec RecursivePacket {
  type K = 0..1
  struct P { a: K, o: Option<Option<K>> }
  state { c: Bool, p: P, absent: Option<Option<K>>, m: Map<K,P> }
  init { c = true p = P {a:0,o:some(some(0))} absent = none forall k: K { m[k] = P {a:k,o:some(none)} } }
  action go() { c = false p = P {a:1,o:some(none)} }
  invariant Capture { $P }
}
`;
for (const [variant, predicate] of [
  ["pass", "c or p.o == some(none)"],
  ["reject", "c or p.a == 0"],
  ["materialize", "c or Seq {m}.head()[0].o == some(none)"],
]) {
  const source = recursiveSource.replace("$P", predicate);
  const path = relative(repository, join(dist, `issue-1467-${variant}.fsl`)).split("\\").join("/");
  await writeFile(join(repository, path), source, "utf8");
  for (let repeat = 0; repeat < 2; repeat += 1) recursiveCases.push({
    id: `1467/${variant}/${repeat}`, variant, repeat, cmd: "verify", path, source,
    source_file: path, files: {}, options: { depth: 2, deadlock: "warn" },
  });
}
for (const [variant, header, expected_status] of [["N", 5, 2], ["W", 4, 0]]) {
  const dependencyChild = `fsl ${header}\n${dependencyLegacy}`;
  const directory = await mkdtemp(join(dist, "dependency-edition-"));
  const path = relative(repository, join(directory, "root_compose.fsl")).split("\\").join("/");
  await writeFile(join(repository, path), dependencyRoot, "utf8");
  await writeFile(join(directory, "dep_next_colon.fsl"), dependencyChild, "utf8");
  dependencyCases.push({
    id: `E01/${variant}/worker-verify`, variant, dependency_edition: true, cmd: "verify", path, source: dependencyRoot, source_file: path,
    files: { "dep_next_colon.fsl": dependencyChild }, unstable: ["fsl5"], options: { depth: 2, deadlock: "warn" }, expected_status,
  });
}
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
let profile;
let child;
let childClosed;
try {

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
logPhase("chrome", `executable=${chrome}`);
try {
  const version = await command(chrome, ["--version"], 2_000);
  logPhase("chrome-version", version.status === 0 ? version.stdout.trim() : "unavailable");
} catch {
  logPhase("chrome-version", "unavailable");
}

profile = await mkdtemp(join(tmpdir(), "fsl-wasm-chrome-"));
child = spawn(chrome, [
  "--headless=new", "--disable-gpu", "--disable-background-networking", "--no-sandbox",
  "--password-store=basic", "--use-mock-keychain",
  "--remote-debugging-port=0", `--user-data-dir=${profile}`,
  `http://127.0.0.1:${port}/`,
], { stdio: ["ignore", "pipe", "pipe"] });
child.once("spawn", () => logPhase("spawn", `pid=${child.pid}`));
childClosed = new Promise((resolve) => child.once("close", resolve));
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
// and even the completion deadline below is not actually a bound
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
// Deliberately no individual CDP-call retry: retain the diagnostic for the
// failed invocation. check-native-integration.sh may retry the whole browser
// command once on status 124, recording both attempts in the step summary.
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
      reject(new ProbeTimeoutError("cdp",
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
let dependencyEnvelopes;
const recursiveEnvelopes = [];
// #1434: send whole request objects through the actual batch -> run boundary.
// The single-request JS adapter destructures keys and cannot detect a typed
// legacy Rust field. Never use normalizeEnvelope for these comparisons.
async function publicationBoundary(socket) {
  const body = "spec S { state { flag: Bool = false } action stay() { flag = false } invariant Safe { not flag } }\n";
  const variants = [{}, { unstable: ["fsl5"] }, { unstable: ["later"] },
    { unstable: 42 }, { unstable: { fsl5: true } }, { unrelated_unknown: { arbitrary: [42] } }];
  const requests = [];
  for (const header of ["", "fsl 4\n", "fsl 5\n"]) {
    for (const extra of variants) {
      requests.push({ cmd: "check", source: header + body, source_file: "model.fsl", ...extra });
    }
  }
  const malformed = [
    { cmd: 42 }, { source: {} }, { source_file: 42 }, { files: [] }, { options: 42 },
    { options: { depth: "2" } }, { options: { deadlock: 42 } },
  ];
  for (const extra of malformed) {
    requests.push({ cmd: "check", source: body, source_file: "model.fsl", ...extra });
  }
  // Real verify, not a host serde/check substitute. Preserve every raw value.
  for (const extra of variants) {
    requests.push({ cmd: "verify", source: "fsl 5\n" + body, source_file: "model.fsl",
      options: { depth: 2, deadlock: "warn" }, ...extra });
  }
  const evaluated = await cdp(socket, "Runtime.evaluate", {
    expression: `new Promise((resolve, reject) => {
      const worker = new Worker('./worker.js');
      const timer = setTimeout(() => { worker.terminate(); reject(new Error('publication boundary timeout')); }, 120000);
      worker.onmessage = ({data}) => {
        if (data.envelopes || data.envelope || data.transportError) {
          clearTimeout(timer); worker.terminate();
          if (data.envelopes) resolve(data.envelopes); else reject(new Error(JSON.stringify(data)));
        }
      };
      worker.onerror = (event) => { clearTimeout(timer); worker.terminate(); reject(new Error(event.message)); };
      worker.postMessage({id:'publication-1434',batch:${JSON.stringify(requests)}});
    })`, awaitPromise: true, returnByValue: true,
  });
  if (evaluated.exceptionDetails) throw new Error(JSON.stringify(evaluated.exceptionDetails));
  const outputs = evaluated.result.value;
  console.log(JSON.stringify({ publication1434: { requests, outputs } }));
  if (!Array.isArray(outputs) || outputs.length !== requests.length) {
    throw new Error("publication request population did not execute");
  }
  for (let header = 0; header < 3; header += 1) {
    const reference = outputs[header * variants.length + 1];
    if (reference.result !== "ok" || reference.spec !== "S" || reference.warnings?.length !== 0) {
      throw new Error(`publication check oracle refused ${JSON.stringify(reference)}`);
    }
    const expected = { fsl: "1.0", result: "ok", spec: "S", warnings: [], versions: reference.versions };
    if (header > 0) expected.fsl_edition = header === 1 ? 4 : 5;
    if (header === 2) expected.edition = "next";
    const oracleDrift = differences(reference, expected);
    if (oracleDrift.length) throw new Error(`publication check literal packet drift ${JSON.stringify(oracleDrift)}`);
    for (let variant = 0; variant < variants.length; variant += 1) {
      const actual = outputs[header * variants.length + variant];
      const drift = differences(actual, reference);
      if (drift.length) throw new Error(`publication whole Worker packet drift ${JSON.stringify(drift)}`);
    }
  }
  const knownStart = 3 * variants.length;
  for (let index = 0; index < malformed.length; index += 1) {
    const actual = outputs[knownStart + index];
    if (actual.result !== "error" || actual.kind !== "io"
        || !actual.message?.startsWith("invalid request JSON: invalid type:")) {
      throw new Error(`malformed known Worker field was admitted ${JSON.stringify(actual)}`);
    }
  }
  const verifies = outputs.slice(knownStart + malformed.length);
  const stableVerify = (packet) => {
    const stable = structuredClone(packet);
    // These exact ten elapsed slots varied in two d83 native runs of this
    // same model/depth. Real Worker calibration remains unexecuted until its
    // dependencies exist; missing/extra/other changing cost slots FAIL closed.
    // Time spent executing/checking a property is ambient, never its verdict.
    const slots = [[stable.cost, "elapsed_s"], [stable.cost?.solver, "check_elapsed_s"],
      ...Array.from({ length: 8 }, (_, index) => [stable.cost?.properties?.[index], "elapsed_s"])];
    for (const [object, key] of slots) {
      if (!object || !Number.isFinite(object[key]) || object[key] < 0) {
        throw new Error(`missing/non-numeric live publication timing ${JSON.stringify(packet)}`);
      }
      object[key] = 0;
    }
    return stable;
  };
  for (const actual of verifies) {
    if (actual.result !== "verified" || actual.completeness !== "bounded" || actual.checked_to_depth !== 2
        || actual.fsl_edition !== 5 || actual.edition !== "next") {
      throw new Error(`publication real verify refused/promoted ${JSON.stringify(actual)}`);
    }
    const drift = differences(stableVerify(actual), stableVerify(verifies[1]));
    if (drift.length) throw new Error(`publication full verify packet drift ${JSON.stringify(drift)}`);
  }
}
{
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
  logPhase("devtools-ready", `port=${debugPort}`);
  try {
    details = await waitForCompletion({
      evaluate: async () => {
        const evaluated = await cdp(socket, "Runtime.evaluate", {
          expression: `(() => { const node = document.querySelector('#result'); return node ? { done: node.dataset.done, ok: node.dataset.ok, text: node.textContent, phase: node.dataset.phase } : null; })()`,
          returnByValue: true,
        });
        return evaluated.result.value;
      },
    });
    logPhase("client-completion", `client=${details.phase ?? "unknown"}`);
    // A fresh Worker for each new repeat avoids shared-runtime solver history.
    for (const [batchIndex, batch] of [dependencyCases, ...recursiveCases.map((testCase) => [testCase])].entries()) {
    const requests = batch.map(({ cmd, source, source_file, files, unstable, options }) => (
      { cmd, source, source_file, files, unstable, options }
    ));
    const evaluated = await cdp(socket, "Runtime.evaluate", {
      expression: `new Promise((resolve, reject) => {
        const worker = new Worker('./worker.js');
        const timer = setTimeout(() => { worker.terminate(); reject(new Error('dependency edition timeout')); }, 120000);
        worker.onmessage = ({data}) => {
          if (data.envelopes || data.envelope || data.transportError) {
            clearTimeout(timer); worker.terminate();
            if (data.envelopes) resolve(data.envelopes); else reject(new Error(JSON.stringify(data)));
          }
        };
        worker.onerror = (event) => { clearTimeout(timer); worker.terminate(); reject(new Error(event.message)); };
        worker.postMessage({id:'dependency-edition',batch:${JSON.stringify(requests)}});
      })`, awaitPromise: true, returnByValue: true,
    });
    if (evaluated.exceptionDetails) throw new Error(JSON.stringify(evaluated.exceptionDetails));
    if (batchIndex === 0) dependencyEnvelopes = evaluated.result.value;
    else recursiveEnvelopes.push(...evaluated.result.value);
    }
    await writeFile(join(dist, "issue-1467-worker-raw.json"), JSON.stringify({ cases: recursiveCases, envelopes: recursiveEnvelopes }, null, 2));
    await publicationBoundary(socket);
  } finally {
    socket.close();
  }
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
const recursiveNative = [];
for (const testCase of recursiveCases) recursiveNative.push(await nativeEnvelope(testCase));
await writeFile(join(dist, "issue-1467-native-raw.json"), JSON.stringify({ cases: recursiveCases, envelopes: recursiveNative }, null, 2));
// Observed in the first raw capture: wall clocks vary; all exclusions are live.
function recursiveStable(packet) {
  const value = structuredClone(packet);
  const clock = (object, key) => {
    if (!Object.hasOwn(object, key) || !Number.isFinite(object[key])) throw new Error(`missing clock ${key}`);
    delete object[key];
  };
  if (value.cost) {
    clock(value.cost, "elapsed_s"); clock(value.cost.solver, "check_elapsed_s");
    for (const property of value.cost.properties) clock(property, "elapsed_s");
  } else if (value.result !== "error") throw new Error("missing successful cost packet");
  return value;
}
// Repeat comparisons only: Z3's allocator footprint is ambient (observed
// 18.01 vs 18.02 MB between identical native runs) and not part of the
// verdict. It must be present, finite and positive in both packets, then is
// dropped; every other field, including conflicts/decisions/propagations,
// stays exact.
function recursiveRepeatStable(packet) {
  const value = recursiveStable(packet);
  if (value.cost) {
    const solver = value.cost.solver;
    if (!Object.hasOwn(solver, "memory_mb") || !Number.isFinite(solver.memory_mb)
      || solver.memory_mb <= 0) throw new Error("1467 missing positive allocator metric");
    delete solver.memory_mb;
  }
  return value;
}
const recursiveComparisons = [];
for (let index = 0; index < recursiveCases.length; index += 2) {
  const nativeDrift = differences(recursiveRepeatStable(recursiveNative[index]), recursiveRepeatStable(recursiveNative[index + 1]));
  const workerDrift = differences(recursiveRepeatStable(recursiveEnvelopes[index]), recursiveRepeatStable(recursiveEnvelopes[index + 1]));
  const left = recursiveStable(recursiveNative[index]);
  const right = recursiveStable(recursiveEnvelopes[index]);
  if (recursiveCases[index].variant === "materialize") {
    // #1467: matching serialized queries/results and correct raw-to-max mapping
    // yielded decisions 103/100 and propagations 106/109, stable in both repeats.
    // Effective defaults/term history are not proven identical; no sole cause
    // is established. Only these materialize cross-runtime maxima may differ.
    const counters = ["decisions", "propagations"];
    for (const packet of [recursiveNative[index], recursiveNative[index + 1],
      recursiveEnvelopes[index], recursiveEnvelopes[index + 1]]) {
      for (const key of counters) {
        const solver = packet.cost.solver;
        const value = solver[key];
        if (!Object.hasOwn(solver, key) || !Number.isFinite(value)
          || !Number.isInteger(value) || value < 0) {
          throw new Error(`1467 invalid materialize solver ${key}`);
        }
      }
    }
    for (const packet of [left, right]) {
      for (const key of counters) packet.cost.solver[key] = 0;
    }
  }
  // Observed runtime identity and allocator footprint differ, never semantic fields.
  for (const [packet, name, backend] of [[left, "fslc-rust", "native-z3"], [right, "fsl-wasm", "z3-solver-wasm"]]) {
    const metadata = { verifier: { name, version: "4.12.0" }, core: { name: "fsl-core", version: "4.12.0" },
      solver: { name: "z3", backend, version: "Z3 4.16.0.0" } };
    if (differences(packet.versions, metadata).length) throw new Error(`1467 runtime identity: ${name}`);
    if (packet.cost) {
      if (!Object.hasOwn(packet.cost.solver, "memory_mb") || !Number.isFinite(packet.cost.solver.memory_mb)
        || packet.cost.solver.memory_mb <= 0) throw new Error("1467 missing positive allocator metric");
      packet.cost.solver.memory_mb = 0; // Ambient allocator footprint, not part of the verdict.
    }
    packet.versions.verifier.name = "runtime"; packet.versions.solver.backend = "runtime";
  }
  recursiveComparisons.push({ id: recursiveCases[index].id, nativeDrift, workerDrift,
    cross: differences(left, right) });
}
await writeFile(join(dist, "issue-1467-comparisons.json"), JSON.stringify(recursiveComparisons, null, 2));
if (recursiveComparisons.some(({ nativeDrift, workerDrift, cross }) => nativeDrift.length || workerDrift.length || cross.length)) {
  throw new ParityViolationError(`1467 full same-route drift: ${JSON.stringify(recursiveComparisons)}`);
}
const literalFinal = { c: false, p: { a: 1, o: { kind: "some", value: null } }, absent: null,
  m: { "0": { a: 0, o: { kind: "some", value: null } }, "1": { a: 1, o: { kind: "some", value: null } } } };
for (const [index, testCase] of recursiveCases.entries()) {
  const nativeStatus = JSON.parse(await readFile(join(dist, `issue-${testCase.id.replaceAll("/", "-")}-native-command.json`), "utf8")).status;
  if (nativeStatus !== (testCase.variant === "reject" ? 1 : 0)) throw new ParityViolationError(`${testCase.id}: native exit ${nativeStatus}`);
  for (const envelope of [recursiveNative[index], recursiveEnvelopes[index]]) {
    const expected = testCase.variant === "reject" ? "violated" : "verified";
    if (envelope.result !== expected || envelope.completeness !== "bounded") {
      throw new ParityViolationError(`${testCase.id}: expected ${expected}/bounded: ${JSON.stringify(envelope)}`);
    }
    if (testCase.variant === "reject" && (envelope.invariant !== "Capture" || envelope.violation_kind !== "invariant"
      || envelope.violated_at_step !== 1 || envelope.loc.line !== 7 || envelope.loc.column !== 3
      || differences(envelope.trace[1].state, literalFinal).length)) {
      throw new ParityViolationError(`${testCase.id}: literal recursive witness drift`);
    }
  }
}
if (!Array.isArray(dependencyEnvelopes) || dependencyEnvelopes.length !== dependencyCases.length) {
  throw new ParityViolationError("dependency edition cases did not execute");
}
for (const [index, testCase] of dependencyCases.entries()) {
  const native = await nativeEnvelope(testCase);
  const worker = dependencyEnvelopes[index];
    // Exact independent finding, including every edit.
    // Display the reference spelling on both surfaces; do not exclude locations.
    for (const actual of [native, worker]) {
      const file = "dep_next_colon.fsl";
      const frontend = actual === native ? "fslc-rust" : "fsl-wasm";
      const backend = actual === native ? "native-z3" : "z3-solver-wasm";
      if (actual.versions?.verifier?.name !== frontend || actual.versions?.solver?.backend !== backend) {
        throw new ParityViolationError(`dependency runtime identity drift for ${frontend}`);
      }
       const finding = { code: "legacy_string_metadata", kind: "legacy_string_metadata", taxonomy: "deprecated",
           severity: testCase.variant === "N" ? "error" : "warning", edition: testCase.variant === "N" ? "next" : "current",
          message: "legacy string metadata is deprecated; use a typed annotation",
          loc: { file, line: 38, column: 38, end_line: 38, end_column: 85 },
          canonical_replacement: '@requirement("REQ-2", "only eligible claims are auto-approved")', machine_applicable: true,
          edits: [{ start: 770, end: 770, replacement: '@requirement("REQ-2", "only eligible claims are auto-approved")\n  ' },
             { start: 805, end: 852, replacement: "" }] };
       let expected = { fsl: "1.0", result: "error", kind: "unsupported_in_edition", edition: "next", fsl_edition: 5,
         findings: [finding], versions: actual.versions };
       if (testCase.variant === "W") {
          expected = structuredClone(dependencyVerify);
          expected.versions = actual.versions;
          expected.warnings.push(finding);
       }
       // Normalization validates runtime identity/cost and shapes non-unique
       // witnesses/state text; loc, verdict, assurance and extra keys stay exact.
      const drift = differences(normalizeEnvelope(actual), normalizeEnvelope(expected));
      if (drift.length) throw new ParityViolationError(JSON.stringify({ case: testCase.id, drift }));
    }
}
for (let index = 0; index < parityCases.length; index += 1) {
  const nativeEnvelope = native[index];
  const wasmEnvelope = browser.parityEnvelopes[index];
  const testCase = parityCases[index];
  if (testCase.exact_collection_witness) {
    for (const envelope of [nativeEnvelope, wasmEnvelope]) {
      if (envelope.result !== testCase.expected_result) {
        throw new ParityViolationError(`${testCase.path}: expected ${testCase.expected_result}, got ${envelope.result}`);
      }
    }
    if (testCase.expected_result === "violated") {
      const exact = differences(nativeEnvelope.trace, wasmEnvelope.trace);
      exact.push(...differences(nativeEnvelope.loc, wasmEnvelope.loc));
      if (exact.length > 0 || nativeEnvelope.invariant !== "Capture" || wasmEnvelope.invariant !== "Capture") {
        throw new ParityViolationError(`${testCase.path}: collection witness drift: ${JSON.stringify(exact)}`);
      }
    }
  }
  const normalizedNative = normalizeEnvelope(nativeEnvelope);
  let normalizedWasm;
  try {
    normalizedWasm = normalizeEnvelope(wasmEnvelope);
  } catch (error) {
    mismatches.push({
      schema: "fsl-native-wasm-parity-failure.v1",
      case: {
        path: parityCases[index].path,
        command: parityCases[index].cmd,
        options: parityCases[index].options,
      },
      differences: [{ path: "$", reason: `invalid WASM envelope: ${error.message}` }],
      native: nativeEnvelope,
      wasm: wasmEnvelope,
    });
    continue;
  }
  const envelopeDifferences = differences(normalizedNative, normalizedWasm);
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
// fails the surface parse (causal) -- it never produces an analysis. The day the
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
  mismatches.push({
    schema: "fsl-native-wasm-parity-failure.v1",
    case: { path: "unsupportedDocuments", command: "check", options: {} },
    differences: staleExclusions.map((reason) => ({ path: "$.exclusion", reason })),
  });
}
if (mismatches.length > 0) {
  const report = JSON.stringify({
    schema: "fsl-native-wasm-parity-failure.v1",
    mismatches,
  }, null, 2);
  const reportPath = join(tmpdir(), "fsl-native-wasm-parity-failure.json");
  await writeFile(reportPath, `${report}\n`, "utf8");
  throw new ParityViolationError(report, reportPath);
}
console.log(JSON.stringify({
  schema: "fsl-wasm-browser.v1",
  ok: true,
  cancelled: true,
  nativeParity: true,
  parityCases: parityCases.length,
  exclusionProbes: exclusionProbes.length,
}, null, 2));
} finally {
  if (child && child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  if (childClosed) await childClosed;
  if (server.listening) {
    await new Promise((resolve, reject) => {
      server.close((error) => error ? reject(error) : resolve());
    });
  }
  if (profile) await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
}

try {
  await main();
  logPhase("completion");
} catch (error) {
  if (error instanceof ProbeTimeoutError) logPhase("timeout", `mode=${error.mode}`);
  const classified = classifyOutcome(error);
  const status = classified.status;
  const diagnostic = { schema: "fsl-wasm-browser-outcome.v1", ...classified };
  delete diagnostic.status;
  console.error(JSON.stringify(diagnostic));
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = status;
}
