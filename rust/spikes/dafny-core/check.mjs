// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): differential check of the generated TypeScript core against
// the fslc Monitor. It drives out/<Name>.ts — the same entry point an app
// imports, over the compiled Dafny — so the JSON conversion is checked too.
// Each step is judged by `fslc replay --trace` with the path that reached it:
//   core allows it  -> the observed post-state must be `conformant`
//   core refuses it -> the spec must answer `requires_failed` for the same step
//
// Two modes, both at the sizes of the spec in KERNEL (default out/<Name>.kernel.json):
//   exhaustive (default)  every reachable state x every action instance; agreement
//                         on all of them means equal transition relations there
//   --random <runs>       seeded random action sequences (--length, --seed), for
//                         specs enlarged beyond what exhaustive search can cover
//
// usage: node check.mjs <Name> [--mutate drop-guard:<action>|skip-effect:<action>]
//                              [--random <runs> --length <n> --seed <n>]

import { execFile } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);
const require = createRequire(import.meta.url);

const [name, ...rest] = process.argv.slice(2);
const opt = (flag) => (rest.includes(flag) ? rest[rest.indexOf(flag) + 1] : null);
const mutate = opt("--mutate");
const randomRuns = opt("--random") === null ? null : Number(opt("--random"));
const length = Number(opt("--length") ?? 30);
const seed = Number(opt("--seed") ?? Date.now() % 2 ** 31);
const here = new URL(".", import.meta.url).pathname;
const fslc = process.env.FSLC ?? join(here, "../../target/release/fslc");
// KERNEL lets the same core be checked against the spec at other sizes.
const kernel = JSON.parse(readFileSync(process.env.KERNEL ?? join(here, `out/${name}.kernel.json`), "utf8"));
// Kernels are produced from this directory (build.sh, core.mjs), so the source path is relative to it.
const spec = resolve(here, kernel.spec.source.file);

// CORE_CJS lets a negative control load a deliberately wrong compiled core
// under the generated wrapper; the copy sits under out/ so bignumber.js resolves.
let coreDir = join(here, "out");
if (process.env.CORE_CJS) {
  coreDir = join(here, "out", `.check-${process.pid}`);
  mkdirSync(coreDir, { recursive: true });
  copyFileSync(process.env.CORE_CJS, join(coreDir, `${name}.cjs`));
  copyFileSync(join(here, "out", `${name}.ts`), join(coreDir, `${name}.ts`));
  process.on("exit", () => rmSync(coreDir, { recursive: true, force: true }));
}
const api = await import(join(coreDir, `${name}.ts`));
const core = require(join(coreDir, `${name}.cjs`)); // the instance the wrapper imported

// ---- negative controls: alter the compiled core in memory -----------------
if (mutate) {
  const [kind, action] = mutate.split(":");
  const key = (n) => n.replace(/_/g, "__");
  const D = core.M.__default;
  if (typeof D[key(`${action}_Step`)] !== "function") throw new Error(`compiled core has no ${action}_Step`);
  if (kind === "drop-guard") D[key(`${action}_Enabled`)] = () => true;
  else if (kind === "skip-effect") D[key(`${action}_Apply`)] = (c, st) => st;
  else throw new Error(`unknown mutation ${kind}`);
}

// ---- sizes and action instances of the spec in KERNEL ---------------------
const types = new Map(kernel.types.map((t) => [t.name, t.definition]));
const values = (tyName) => {
  const d = types.get(tyName);
  return Array.from({ length: d.hi - d.lo + 1 }, (_, k) => d.lo + k);
};
const widened = (() => {
  const w = /\/\/ widened: (.*)/.exec(readFileSync(join(here, `out/${name}.gen.dfy`), "utf8"))[1];
  return w === "(none)" ? [] : w.split(", ");
})();
const sizes = Object.fromEntries(widened.map((t) => [t, values(t).length]));
const instances = kernel.actions.flatMap((a) => {
  let tuples = [[]];
  for (const p of a.parameters) tuples = tuples.flatMap((t) => values(p.type.name).map((v) => [...t, [p.name, v]]));
  return tuples.map((t) => ({ action: a.name, params: Object.fromEntries(t) }));
});

// ---- replay oracle --------------------------------------------------------
const dir = mkdtempSync(join(tmpdir(), "dafny-core-"));
let traceNo = 0;
const initial = api.init(sizes);
async function replay(events) {
  const file = join(dir, `t${traceNo++}.json`);
  writeFileSync(
    file,
    JSON.stringify({
      $schema: "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json",
      schema_version: "1.2.0",
      kernel_schema_version: "1.0.0",
      spec: kernel.spec.name,
      initial,
      events: events.map((e, k) => ({ tick: k + 1, ...e })),
    }),
  );
  try {
    const { stdout } = await run(fslc, ["replay", spec, "--trace", file], { maxBuffer: 64 << 20 });
    return JSON.parse(stdout);
  } catch (err) {
    if (err.code === 1 && err.stdout) return JSON.parse(err.stdout);
    // Exit 2: the observation itself is not a state of the spec (for example a
    // value outside its type). Counted as a divergence, never skipped.
    if (err.code === 2 && err.stdout) {
      const e = JSON.parse(err.stdout);
      return { result: "unrepresentable", violation: { kind: e.kind, message: e.message } };
    }
    throw new Error(`fslc replay exit ${err.code}: ${err.stderr || err.stdout}`);
  }
}

const divergences = [];
const show = (path) => path.map((e) => `${e.action}(${JSON.stringify(e.params)})`);
// One core step. Throws nothing: an exception from the core is a divergence.
function coreStep(state, inst, path) {
  try {
    return { next: api.step(sizes, state, { action: inst.action, params: inst.params }) };
  } catch (err) {
    divergences.push({ path: show(path), step: inst, verdict: "core_exception", violation: { kind: err.name, message: String(err.message) } });
    return null;
  }
}
const inBounds = (state) => {
  try {
    api.validate(sizes, state);
    return true;
  } catch {
    return false;
  }
};

const jobs = []; // { path, event, enabled } judged by replay
let checked = 0;
let explored = 0;

const first = await replay([]);
if (first.result !== "conformant") divergences.push({ step: "initial", verdict: first.result, violation: first.violation });

if (randomRuns === null) {
  // ---- exhaustive ---------------------------------------------------------
  const seen = new Set([JSON.stringify(initial)]);
  const queue = [{ state: initial, path: [] }];
  const STATE_LIMIT = 100000;
  while (queue.length) {
    if (seen.size > STATE_LIMIT) throw new Error(`more than ${STATE_LIMIT} reachable states at these sizes; use --random`);
    const { state, path } = queue.shift();
    for (const inst of instances) {
      const r = coreStep(state, inst, path);
      if (!r) continue;
      const event = { action: inst.action, params: inst.params, state: r.next ?? state };
      jobs.push({ path, event, enabled: r.next !== null });
      // A post-state outside the bounds is already a divergence (the Monitor
      // rejects it); do not explore from it, or a broken core can grow forever.
      if (r.next && inBounds(r.next)) {
        const key = JSON.stringify(r.next);
        if (!seen.has(key)) {
          seen.add(key);
          queue.push({ state: r.next, path: [...path, event] });
        }
      }
    }
  }
  explored = seen.size;
} else {
  // ---- random sequences ---------------------------------------------------
  let s = seed >>> 0; // mulberry32
  const rand = () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32;
  };
  const pick = (xs) => xs[Math.floor(rand() * xs.length)];
  const states = new Set([JSON.stringify(initial)]);
  for (let n = 0; n < randomRuns; n++) {
    let state = initial;
    const path = [];
    for (let k = 0; k < length; k++) {
      const a = pick(kernel.actions);
      const inst = { action: a.name, params: Object.fromEntries(a.parameters.map((p) => [p.name, pick(values(p.type.name))])) };
      const r = coreStep(state, inst, path);
      if (!r) break;
      if (r.next === null) {
        jobs.push({ path: [...path], event: { action: inst.action, params: inst.params, state }, enabled: false });
        continue;
      }
      const event = { action: inst.action, params: inst.params, state: r.next };
      jobs.push({ path: [...path], event, enabled: true });
      path.push(event);
      states.add(JSON.stringify(r.next));
      if (!inBounds(r.next)) break;
      state = r.next;
    }
  }
  explored = states.size;
}

async function judge({ path, event, enabled }) {
  const r = await replay([...path, event]);
  checked++;
  const ok = enabled ? r.result === "conformant" : r.result === "nonconformant" && r.violation?.kind === "requires_failed" && r.failed_at_event === path.length;
  if (!ok) divergences.push({ path: show(path), step: event, core_allows: enabled, verdict: r.result, violation: r.violation });
}
const pool = Array.from({ length: 8 }, async () => {
  while (jobs.length) await judge(jobs.shift());
});
await Promise.all(pool);
rmSync(dir, { recursive: true, force: true });

const report = {
  spec: kernel.spec.name,
  sizes,
  mode: randomRuns === null ? "exhaustive" : { random: randomRuns, length, seed },
  mutation: mutate,
  [randomRuns === null ? "reachable_states" : "distinct_states"]: explored,
  action_instances: instances.length,
  steps_checked: checked,
  divergences: divergences.length,
  first_divergence: divergences[0] ?? null,
};
console.log(JSON.stringify(report, null, 2));
process.exit(divergences.length ? 1 : 0);
