// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): differential check of the generated TypeScript core against
// fslc's concrete semantics. It drives out/<Name>.ts — the same entry point an
// app imports, over the compiled Dafny — so the JSON conversion is checked too.
//
// Two modes, both at the sizes of the spec in KERNEL (default out/<Name>.kernel.json):
//   exhaustive (default)  one `fslc conformance` call lists every state reachable
//                         within --depth (default 64) and the outcome of every action
//                         instance in each. The core must accept each of those states
//                         (its Wf and Inv), allow exactly the `ok` ones with the same
//                         post-state, and refuse the `requires_failed` ones; any other
//                         outcome (e.g. `type_bound`) is a divergence. Agreement on all
//                         of them from the same initial state means equal transition
//                         relations within the depth; `closed` says the depth was not cut.
//   --random <runs>       seeded random action sequences (--length, --seed), each step
//                         judged by `fslc replay --trace` with the path that reached it
//                         (allowed -> `conformant`, refused -> `requires_failed`), for
//                         specs enlarged beyond what exhaustive search can cover
//
// usage: node check.mjs <Name> [--mutate drop-guard:<action>|skip-effect:<action>]
//                              [--depth <n>] [--random <runs> --length <n> --seed <n>]

import { execFile } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { availableParallelism, tmpdir } from "node:os";
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
// --depth bounds the exhaustive search for specs whose reachable states do not
// close (an unbounded Int counter); the report says whether it closed.
const depth = Number(opt("--depth") ?? 64);
let closed = true;
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
  if (d.kind === "enum") return d.members;
  if (d.kind !== "domain") throw new Error(`parameter of type ${tyName} (${d.kind})`);
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
const show = (path) => path.map((e) => (typeof e === "string" ? e : `${e.action}(${JSON.stringify(e.params)})`));
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

// The wrapper's JSON and fslc conformance's JSON differ only in options: the
// wrapper writes a non-nested Option as null | value, conformance always as
// { kind, value }. canon() compares with sorted keys.
const canon = (v) =>
  v === null || typeof v !== "object" ? JSON.stringify(v) : `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canon(v[k])}`).join(",")}}`;
function toConf(ty, v) {
  switch (ty.kind) {
    case "option": {
      const nested = ty.item.kind === "option";
      if (nested ? v.kind === "none" : v === null) return { kind: "none" };
      return { kind: "some", value: toConf(ty.item, nested ? v.value : v) };
    }
    case "map":
      return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, toConf(ty.value, x)]));
    case "seq":
    case "set":
      return v.map((x) => toConf(ty.item, x));
    case "named": {
      const d = types.get(ty.name);
      if (d.kind === "struct") return Object.fromEntries(d.fields.map((f) => [f.name, toConf(f.type, v[f.name])]));
      return v;
    }
    default:
      return v;
  }
}
function fromConf(ty, v) {
  switch (ty.kind) {
    case "option": {
      const nested = ty.item.kind === "option";
      if (v.kind === "none") return nested ? { kind: "none" } : null;
      return nested ? { kind: "some", value: fromConf(ty.item, v.value) } : fromConf(ty.item, v.value);
    }
    case "map":
      return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, fromConf(ty.value, x)]));
    case "seq":
    case "set":
      return v.map((x) => fromConf(ty.item, x));
    case "named": {
      const d = types.get(ty.name);
      if (d.kind === "struct") return Object.fromEntries(d.fields.map((f) => [f.name, fromConf(f.type, v[f.name])]));
      return v;
    }
    default:
      return v;
  }
}
const stateConf = (st) => Object.fromEntries(kernel.state.map((s) => [s.name, toConf(s.type, st[s.name])]));
const stateCore = (st) => Object.fromEntries(kernel.state.map((s) => [s.name, fromConf(s.type, st[s.name])]));

if (randomRuns === null) {
  // ---- exhaustive, against one `fslc conformance` call ---------------------
  const { stdout } = await run(fslc, ["conformance", spec, "--depth", String(depth)], { maxBuffer: 1 << 30 });
  const conf = JSON.parse(stdout);
  const known = new Set(conf.states.map((s) => canon(s.state)));
  const s0 = conf.states.find((s) => s.depth === 0);
  if (canon(stateConf(initial)) !== canon(s0.state)) divergences.push({ step: "initial", core: initial, spec: s0.state });
  const coreState = new Map();
  for (const s of conf.states) {
    const st = stateCore(s.state);
    try {
      // Every state fslc reaches satisfies the invariants it proved; the
      // generated Wf and Inv must accept it too.
      api.validate(sizes, st);
      coreState.set(s.id, st);
    } catch (err) {
      divergences.push({ state: s.id, verdict: "core_refuses_reachable_state", spec: s.state, message: String(err.message) });
    }
  }
  for (const v of conf.vectors) {
    const st = coreState.get(v.state);
    if (!st) continue;
    const inst = { action: v.action.name, params: v.action.params };
    const r = coreStep(st, inst, [`state ${v.state}`]);
    checked++;
    if (!r) continue;
    const o = v.outcome;
    const where = { state: v.state, step: inst, spec_outcome: o.kind, core_allows: r.next !== null };
    if (o.kind === "ok") {
      if (r.next === null) divergences.push({ ...where, verdict: "core_refuses_allowed_step" });
      else if (canon(stateConf(r.next)) !== canon(o.state)) divergences.push({ ...where, verdict: "post_state_differs", core: stateConf(r.next), spec: o.state });
      else if (!known.has(canon(o.state))) closed = false;
    } else if (o.kind === "requires_failed") {
      if (r.next !== null) divergences.push({ ...where, verdict: "core_allows_refused_step", core: stateConf(r.next) });
    } else {
      // type_bound and other failures: the spec has no such transition.
      divergences.push({ ...where, verdict: `spec_${o.kind}` });
    }
  }
  explored = conf.states.length;
} else {
  const first = await replay([]);
  if (first.result !== "conformant") divergences.push({ step: "initial", verdict: first.result, violation: first.violation });
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
  // One replay judges a run's whole path of allowed steps (it reports the first
  // event that differs); refused steps need one replay each, so at most
  // --refused-per-run of them per run are judged (default 3).
  const refusedPerRun = Number(opt("--refused-per-run") ?? 3);
  const states = new Set([JSON.stringify(initial)]);
  for (let n = 0; n < randomRuns; n++) {
    let state = initial;
    const path = [];
    const refused = [];
    for (let k = 0; k < length; k++) {
      const a = pick(kernel.actions);
      const inst = { action: a.name, params: Object.fromEntries(a.parameters.map((p) => [p.name, pick(values(p.type.name))])) };
      const r = coreStep(state, inst, path);
      if (!r) break;
      if (r.next === null) {
        refused.push({ path: [...path], event: { action: inst.action, params: inst.params, state }, enabled: false });
        continue;
      }
      path.push({ action: inst.action, params: inst.params, state: r.next });
      states.add(JSON.stringify(r.next));
      if (!inBounds(r.next)) break;
      state = r.next;
    }
    if (path.length) jobs.push({ path: path.slice(0, -1), event: path[path.length - 1], enabled: true, steps: path.length });
    for (let k = refused.length - 1, taken = 0; k >= 0 && taken < refusedPerRun; k -= Math.max(1, Math.floor(refused.length / refusedPerRun)), taken++)
      jobs.push(refused[k]);
  }
  explored = states.size;
}

async function judge({ path, event, enabled }) {
  const r = await replay([...path, event]);
  checked += enabled ? path.length + 1 : 1;
  const ok = enabled ? r.result === "conformant" : r.result === "nonconformant" && r.violation?.kind === "requires_failed" && r.failed_at_event === path.length;
  if (!ok) divergences.push({ path: show(path), step: event, core_allows: enabled, verdict: r.result, violation: r.violation });
}
const pool = Array.from({ length: availableParallelism() }, async () => {
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
  ...(randomRuns === null ? { closed, depth } : {}),
  action_instances: instances.length,
  steps_checked: checked,
  divergences: divergences.length,
  first_divergence: divergences[0] ?? null,
};
console.log(JSON.stringify(report, null, 2));
process.exit(divergences.length ? 1 : 0);
