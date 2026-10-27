// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { delimiter, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const temporaryRoot = join(root, ".codex/tmp");
const integrationScript = join(root, "tools/check-native-integration.sh");
const browserCommand = ["npm", "--prefix", "rust/fsl-wasm", "run", "test:browser"];
const setupCommands = [
  ["npm", "--prefix", "rust/spikes/z3js-worker", "ci"],
  ["npm", "--prefix", "rust/spikes/z3js-worker", "run", "probe"],
  ["npm", "--prefix", "rust/spikes/z3js-worker", "run", "probe:browser"],
  ["cargo", "build", "--manifest-path", "rust/Cargo.toml", "-p", "fslc-rust", "--bin", "fslc", "--locked"],
  ["npm", "--prefix", "rust/fsl-wasm", "ci"],
  ["npm", "--prefix", "rust/fsl-wasm", "run", "test:outcome"],
  ["npm", "--prefix", "rust/fsl-wasm", "run", "test:imports"],
];

function cleanEnvironment() {
  const env = { ...process.env };
  delete env.BASH_ENV;
  delete env.ENV;
  delete env.GITHUB_STEP_SUMMARY;
  return env;
}

function findBash() {
  const candidates = [...new Set([
    process.env.BASH,
    ...(process.env.PATH ?? "").split(delimiter).map((directory) => resolve(directory, "bash")),
  ].filter(Boolean))];
  const found = [];
  for (const candidate of candidates) {
    const result = spawnSync(candidate, ["--noprofile", "--norc", "-c", 'printf "%s" "$BASH_VERSION"'], {
      encoding: "utf8", env: cleanEnvironment(), timeout: 5_000,
    });
    if (result.error || result.status !== 0) continue;
    const version = result.stdout.trim();
    if (!/^\d+\./.test(version)) continue;
    if (Number(version.split(".")[0]) >= 4) return { executable: candidate };
    found.push(`${candidate}: ${version}`);
  }
  return { skip: `requires Bash 4 or newer; found ${found.join(", ") || "no working Bash on BASH/PATH"}` };
}

const bash = findBash();

function temporaryDirectory(t) {
  mkdirSync(temporaryRoot, { recursive: true });
  const directory = mkdtempSync(join(temporaryRoot, "browser-retry-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

// Used both directly (even without Bash 4) and as an actual stubbed browser
// child. No status 65 is injected here: the production comparison and typed
// classifier must produce it. Compare the entire normalized envelopes.
function writeParityControl(directory) {
  const path = join(directory, "real-parity.mjs");
  writeFileSync(path, `
import assert from "node:assert/strict";
import { differences, normalizeEnvelope } from ${JSON.stringify(new URL("./parity.mjs", import.meta.url).href)};
import { classifyOutcome, ParityViolationError } from ${JSON.stringify(new URL("./browser-outcome.mjs", import.meta.url).href)};
try {
  const native = {
    fsl: "1.0", result: "verified", loc: { line: 2, column: 3 },
    versions: {
      core: { name: "fsl-core", version: "2.7.0" },
      verifier: { name: "fslc-rust", version: "2.7.0" },
      solver: { name: "z3", version: "Z3 4.16.0.0", backend: "native-z3" },
    },
  };
  const worker = structuredClone(native);
  assert.deepEqual(differences(normalizeEnvelope(native), normalizeEnvelope(worker)), []);
  worker.loc.line = 9;
  const mismatch = differences(normalizeEnvelope(native), normalizeEnvelope(worker));
  assert.deepEqual(mismatch, [{ path: "$.loc.line", native: 2, wasm: 9 }]);
  if (mismatch.length > 0) throw new ParityViolationError(JSON.stringify({ differences: mismatch }));
} catch (error) {
  const classified = classifyOutcome(error);
  console.error(JSON.stringify({ classified, report: error.message }));
  process.exitCode = classified.status;
}
`);
  return path;
}

const stubSource = `#!${process.execPath}
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";
import { spawnSync } from "node:child_process";
const argv = [basename(process.argv[1]), ...process.argv.slice(2)];
const queues = JSON.parse(readFileSync(process.env.RETRY_QUEUES, "utf8"));
const queue = queues[JSON.stringify(argv)];
let action = "0";
let pastQueue = false;
if (queue) {
  const remaining = readFileSync(queue, "utf8").trim().split("\\n").filter(Boolean);
  pastQueue = remaining.length === 0;
  action = remaining.shift() ?? "0";
  writeFileSync(queue, remaining.length ? remaining.join("\\n") + "\\n" : "");
}
let status;
if (action === "parity") {
  const result = spawnSync(process.execPath, [process.env.RETRY_PARITY], { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.signal) throw new Error("parity control terminated: " + result.signal);
  process.stdout.write(result.stdout);
  process.stderr.write(result.stderr);
  status = result.status;
} else {
  status = Number(action);
}
appendFileSync(process.env.RETRY_LOG, JSON.stringify({ argv, action, status, pastQueue }) + "\\n");
process.exitCode = status;
`;

// Queues are keyed by complete argv, so another phase can get its own queue
// without replacing the actual integration entry point or retry logic.
function runIntegration(t, { browserStatuses, summary = true, initialSummary = "", setupFailure = false }) {
  const directory = temporaryDirectory(t);
  const bin = join(directory, "bin");
  mkdirSync(bin);
  for (const command of ["npm", "cargo"]) {
    writeFileSync(join(bin, command), stubSource, { mode: 0o755 });
  }
  const browserQueue = join(directory, "browser.queue");
  writeFileSync(browserQueue, browserStatuses.join("\n") + "\n");
  const queues = { [JSON.stringify(browserCommand)]: browserQueue };
  if (setupFailure) {
    const setupQueue = join(directory, "setup.queue");
    writeFileSync(setupQueue, "1\n");
    queues[JSON.stringify(setupCommands[5])] = setupQueue;
  }
  const queueConfig = join(directory, "queues.json");
  const log = join(directory, "invocations.jsonl");
  const summaryPath = join(directory, "summary.md");
  writeFileSync(queueConfig, JSON.stringify(queues));
  writeFileSync(log, "");
  writeFileSync(summaryPath, initialSummary);
  const env = {
    ...cleanEnvironment(), PATH: `${bin}${delimiter}${process.env.PATH ?? ""}`,
    RETRY_QUEUES: queueConfig, RETRY_LOG: log, RETRY_PARITY: writeParityControl(directory),
  };
  if (summary) env.GITHUB_STEP_SUMMARY = summaryPath;
  const result = spawnSync(bash.executable, [integrationScript, "wasm"], {
    cwd: root, env, encoding: "utf8", timeout: 15_000,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null, result.stderr);
  const invocations = readFileSync(log, "utf8").trim().split("\n").filter(Boolean).map(JSON.parse);
  return {
    ...result, invocations, summary: readFileSync(summaryPath, "utf8"),
    remaining: readFileSync(browserQueue, "utf8").trim().split("\n").filter(Boolean),
  };
}

function outcomeFor(status) {
  return status === 0 ? "pass" : status === 124 ? "probe_timeout"
    : status === 65 ? "parity_violation" : "harness_failure";
}

function assertBrowserRun(result, { statuses, calls, status, summary = true, prefix = "" }) {
  assert.equal(result.status, status, result.stderr);
  assert.deepEqual(result.invocations.map(({ argv }) => argv), [
    ...setupCommands, ...Array.from({ length: calls }, () => browserCommand),
  ], "setup runs once, only the final browser command can repeat");
  assert.ok(result.invocations.every(({ pastQueue }) => !pastQueue), "no call past a supplied queue");
  assert.deepEqual(result.remaining, statuses.slice(calls).map(String), "unconsumed statuses stay queued");
  const browserRuns = result.invocations.slice(setupCommands.length);
  assert.deepEqual(browserRuns.map((run) => run.status), statuses.slice(0, calls).map((value) => value === "parity" ? 65 : value));
  const outcome = outcomeFor(status);
  // The existing pass label is stdout; failure labels are stderr.
  assert.ok((status === 0 ? result.stdout : result.stderr).includes(
    `check-native-integration: wasm browser outcome=${outcome} status=${status}`,
  ));
  assert.ok(result.summary.startsWith(prefix), "existing summary content is preserved verbatim");
  if (!summary) {
    assert.equal(result.summary, prefix, "an unset summary variable does not write a summary");
    return;
  }
  const appended = result.summary.slice(prefix.length);
  if (calls === 2) {
    const lines = appended.split("\n");
    const retryIndex = lines.findIndex((line) => /probe_timeout/.test(line)
      && /attempt\D*1\b/i.test(line) && /retry/i.test(line));
    assert.notEqual(retryIndex, -1, "attempt 1 probe_timeout and retry are visible together");
    assert.ok(lines.slice(retryIndex + 1).some((line) => line.includes(outcome)), "final outcome follows retry history");
  } else {
    assert.doesNotMatch(appended, /retry/i);
    assert.ok(appended.includes(outcome), "summary includes the final outcome");
  }
}

const browserMatrix = [
  { statuses: [0], calls: 1, status: 0 },
  { statuses: [124, 0], calls: 2, status: 0 },
  { statuses: [124, 124, 0], calls: 2, status: 124 },
  { statuses: [65, 0], calls: 1, status: 65 },
  { statuses: [1, 0], calls: 1, status: 1 },
  { statuses: [124, 65, 0], calls: 2, status: 65 },
  { statuses: [124, 1, 0], calls: 2, status: 1 },
  { statuses: [2, 0], calls: 1, status: 2 },
  { statuses: [137, 0], calls: 1, status: 137 },
];

for (const entry of browserMatrix) {
  test(`final browser statuses ${entry.statuses}: ${entry.calls} calls, exit ${entry.status}`, { skip: bash.skip }, (t) => {
    assertBrowserRun(runIntegration(t, { browserStatuses: entry.statuses }), entry);
  });
}

for (const entry of [browserMatrix[0], browserMatrix[1], browserMatrix[2], browserMatrix[3]]) {
  test(`summary unset, final browser statuses ${entry.statuses}`, { skip: bash.skip }, (t) => {
    assertBrowserRun(runIntegration(t, { browserStatuses: entry.statuses, summary: false }), { ...entry, summary: false });
  });
}

test("a retried browser appends to an existing step summary", { skip: bash.skip }, (t) => {
  const prefix = "# Earlier integration evidence\nkeep this exact content\n";
  const entry = browserMatrix[1];
  assertBrowserRun(runIntegration(t, { browserStatuses: entry.statuses, initialSummary: prefix }), { ...entry, prefix });
});

test("setup failure stops before browser invocation", { skip: bash.skip }, (t) => {
  const result = runIntegration(t, { browserStatuses: [0], setupFailure: true });
  assert.equal(result.status, 1, result.stderr);
  assert.deepEqual(result.invocations.map(({ argv }) => argv), setupCommands.slice(0, 6));
  assert.deepEqual(result.remaining, ["0"]);
  assert.doesNotMatch(result.summary, /retry/i);
});

function assertRealParity(result) {
  assert.equal(result.status, 65, result.stderr);
  const diagnostic = result.stderr.split("\n").filter((line) => line.startsWith("{")).map(JSON.parse);
  const parity = diagnostic.find((entry) => entry.classified?.outcome === "parity_violation");
  assert.ok(parity, "production classifier supplies the parity outcome");
  assert.deepEqual(parity.classified, { outcome: "parity_violation", status: 65 });
  assert.deepEqual(JSON.parse(parity.report), { differences: [{ path: "$.loc.line", native: 2, wasm: 9 }] });
}

test("production stable-field comparison produces parity status 65 without Bash", (t) => {
  const result = spawnSync(process.execPath, [writeParityControl(temporaryDirectory(t))], {
    cwd: root, encoding: "utf8", timeout: 5_000,
  });
  assert.ifError(result.error);
  assertRealParity(result);
});

test("real parity violation is never retried or masked by a queued success", { skip: bash.skip }, (t) => {
  const statuses = ["parity", 0];
  const result = runIntegration(t, { browserStatuses: statuses });
  assertRealParity(result);
  assertBrowserRun(result, { statuses, calls: 1, status: 65 });
});
