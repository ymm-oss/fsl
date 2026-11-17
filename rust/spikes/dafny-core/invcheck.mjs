// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): compare the generated invariants with fslc's, on states that
// are not reachable. The step differential (check.mjs) shows the core accepts
// every state fslc reaches, so a generated Inv is not too strong there; it
// cannot show Inv is not too weak, because reachable states satisfy both.
//
// This samples states inside the type bounds: reachable states with one value
// changed, and uniformly random ones. For each, the core says whether `validate`
// accepts it (Wf holds by construction, so that is Inv). fslc decides on a copy
// of the spec whose `init` is replaced by that state: `fslc verify --depth 0`
// reports `violated` at step 0 exactly when the state breaks an invariant. Any
// disagreement fails the check: the core accepting is a too-weak Inv, the core
// refusing a too-strong one.
//
// usage: node invcheck.mjs <Name> [--samples 200] [--seed n]
// exit: 0 agree, 1 disagree, 2 harness error

import { execFile } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);
const [name, ...rest] = process.argv.slice(2);
const opt = (flag, d) => (rest.includes(flag) ? rest[rest.indexOf(flag) + 1] : d);
const samples = Number(opt("--samples", 200));
const seed = Number(opt("--seed", 1));
const here = new URL(".", import.meta.url).pathname;
const fslc = process.env.FSLC ?? join(here, "../../target/release/fslc");
const kernel = JSON.parse(readFileSync(join(here, `out/${name}.kernel.json`), "utf8"));
const specPath = resolve(here, kernel.spec.source.file);
const source = readFileSync(specPath, "utf8");
const api = await import(process.env.WRAPPER ?? join(here, "out", `${name}.ts`));
const types = new Map(kernel.types.map((t) => [t.name, t.definition]));
const widened = (() => {
  const w = /\/\/ widened: (.*)/.exec(readFileSync(join(here, `out/${name}.gen.dfy`), "utf8"))[1];
  return w === "(none)" ? [] : w.split(", ");
})();
const range = (d) => Array.from({ length: d.hi - d.lo + 1 }, (_, k) => d.lo + k);
const sizes = Object.fromEntries(widened.map((t) => [t, range(types.get(t)).length]));

// ---- seeded sampling inside the type bounds (wrapper JSON shape) -----------
let s = seed >>> 0;
const rand = () => {
  s = (s + 0x6d2b79f5) >>> 0;
  let t = s;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32;
};
const pick = (xs) => xs[Math.floor(rand() * xs.length)];
function sample(ty) {
  switch (ty.kind) {
    case "int":
      return pick([-2, -1, 0, 1, 2, 3, 5, 8, 13]);
    case "bool":
      return rand() < 0.5;
    case "named": {
      const d = types.get(ty.name);
      if (d.kind === "domain") return pick(range(d));
      if (d.kind === "enum") return pick(d.members);
      return Object.fromEntries(d.fields.map((f) => [f.name, sample(f.type)]));
    }
    case "option": {
      const nested = ty.item.kind === "option";
      if (rand() < 0.4) return nested ? { kind: "none" } : null;
      return nested ? { kind: "some", value: sample(ty.item) } : sample(ty.item);
    }
    case "map":
      return Object.fromEntries(range(types.get(ty.key.name)).map((k) => [String(k), sample(ty.value)]));
    case "seq":
      return Array.from({ length: Math.floor(rand() * (ty.capacity + 1)) }, () => sample(ty.item));
    case "set": {
      const all = ty.item.kind === "named" && types.get(ty.item.name).kind === "domain" ? range(types.get(ty.item.name)) : null;
      if (!all) throw new Error("Set of a non-domain type");
      return all.filter(() => rand() < 0.5);
    }
  }
  throw new Error(`type ${ty.kind}`);
}
const randomState = () => Object.fromEntries(kernel.state.map((v) => [v.name, sample(v.type)]));
// Change one leaf of a state to a fresh in-bounds value.
function perturb(state) {
  const v = pick(kernel.state);
  const copy = JSON.parse(JSON.stringify(state));
  const ty = v.type;
  if (ty.kind === "map") {
    const k = pick(Object.keys(copy[v.name]));
    const vt = ty.value;
    const d = vt.kind === "named" ? types.get(vt.name) : null;
    if (d?.kind === "struct") {
      const f = pick(d.fields);
      copy[v.name][k][f.name] = sample(f.type);
    } else copy[v.name][k] = sample(vt);
  } else copy[v.name] = sample(ty);
  return copy;
}

// ---- rendering a state as an FSL init block -------------------------------
function lit(ty, v) {
  switch (ty.kind) {
    case "int":
      return String(v);
    case "bool":
      return v ? "true" : "false";
    case "named": {
      const d = types.get(ty.name);
      if (d.kind === "domain") return String(v);
      if (d.kind === "enum") return v;
      return `${ty.name} { ${d.fields.map((f) => `${f.name}: ${lit(f.type, v[f.name])}`).join(", ")} }`;
    }
    case "option": {
      const nested = ty.item.kind === "option";
      if (nested ? v.kind === "none" : v === null) return "none";
      return `some(${lit(ty.item, nested ? v.value : v)})`;
    }
    case "seq":
      return `Seq { ${v.map((x) => lit(ty.item, x)).join(", ")} }`;
    case "set":
      return `Set { ${v.map((x) => lit(ty.item, x)).join(", ")} }`;
  }
  throw new Error(`literal of ${ty.kind}`);
}
function initBlock(state) {
  const lines = [];
  for (const v of kernel.state) {
    if (v.type.kind === "map") for (const [k, x] of Object.entries(state[v.name])) lines.push(`    ${v.name}[${k}] = ${lit(v.type.value, x)}`);
    else lines.push(`    ${v.name} = ${lit(v.type, state[v.name])}`);
  }
  return `init {\n${lines.join("\n")}\n  }`;
}
const initAt = (() => {
  const m = /(^|\n)\s*init\s*\{/.exec(source);
  if (!m) throw new Error("no init block");
  const start = source.indexOf("init", m.index);
  let depth = 0;
  for (let k = source.indexOf("{", start); k < source.length; k++) {
    if (source[k] === "{") depth++;
    else if (source[k] === "}" && --depth === 0) return [start, k + 1];
  }
  throw new Error("unbalanced init block");
})();
// Only invariants are judged: `reachable` (never witnessed at depth 0) and
// `leadsTo` blocks are dropped from the copy, after the init block.
function dropBlocks(text) {
  const re = /(^|\n)(\s*)(?:reachable|leadsTo)\s+[A-Za-z_][A-Za-z0-9_]*\s*\{/g;
  let out = "";
  let last = 0;
  for (let m; (m = re.exec(text)); ) {
    const start = m.index + m[1].length;
    if (start < last) continue;
    let depth = 0;
    let k = text.indexOf("{", start);
    for (; k < text.length; k++) {
      if (text[k] === "{") depth++;
      else if (text[k] === "}" && --depth === 0) break;
    }
    out += text.slice(last, start);
    last = k + 1;
    re.lastIndex = last;
  }
  return out + text.slice(last);
}
const withInit = (state) => source.slice(0, initAt[0]) + initBlock(state) + dropBlocks(source.slice(initAt[1]));

// ---- collect states ------------------------------------------------------
const { stdout } = await run(fslc, ["conformance", specPath, "--depth", "8"], { maxBuffer: 1 << 30 });
const reachable = JSON.parse(stdout).states.map((x) => x.state);
const fromConf = (ty, v) => {
  if (ty.kind === "option") {
    const nested = ty.item.kind === "option";
    if (v.kind === "none") return nested ? { kind: "none" } : null;
    return nested ? { kind: "some", value: fromConf(ty.item, v.value) } : fromConf(ty.item, v.value);
  }
  if (ty.kind === "map") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, fromConf(ty.value, x)]));
  if (ty.kind === "seq" || ty.kind === "set") return v.map((x) => fromConf(ty.item, x));
  if (ty.kind === "named" && types.get(ty.name).kind === "struct")
    return Object.fromEntries(types.get(ty.name).fields.map((f) => [f.name, fromConf(f.type, v[f.name])]));
  return v;
};
const reachableCore = reachable.map((st) => Object.fromEntries(kernel.state.map((v) => [v.name, fromConf(v.type, st[v.name])])));
const states = new Map();
for (let k = 0; states.size < samples && k < samples * 20; k++) {
  const st = k % 2 === 0 ? perturb(pick(reachableCore)) : randomState();
  states.set(JSON.stringify(st), st);
}

// ---- judge ---------------------------------------------------------------
const dir = mkdtempSync(join(tmpdir(), "dafny-inv-"));
let n = 0;
const disagreements = [];
const tally = { both_hold: 0, both_break: 0 };
async function judge(st) {
  let core = true;
  try {
    api.validate(sizes, st);
  } catch (err) {
    if (err?.constructor?.name !== "CoreInputError") throw err;
    core = false;
  }
  const file = join(dir, `s${n++}.fsl`);
  writeFileSync(file, withInit(st));
  let out;
  try {
    out = (await run(fslc, ["verify", file, "--depth", "0"], { maxBuffer: 64 << 20 })).stdout;
  } catch (err) {
    out = err.stdout;
    if (err.code !== 1) throw new Error(`fslc verify exit ${err.code}: ${(err.stdout || err.stderr).slice(0, 400)}`);
  }
  const r = JSON.parse(out);
  let spec;
  if (["proved", "verified"].includes(r.result)) spec = true;
  else if (r.result === "violated" && r.violated_at_step === 0 && r.violation_kind === "invariant") spec = false;
  else throw new Error(`fslc verify gave ${r.result} (${r.violation_kind ?? ""}) for a planted state`);
  if (core === spec) tally[core ? "both_hold" : "both_break"]++;
  else disagreements.push({ state: st, core_accepts: core, fslc: spec ? "holds" : `breaks ${r.invariant}`, verdict: core ? "generated Inv too weak" : "generated Inv too strong" });
}
const queue = [...states.values()];
let harness = null;
await Promise.all(
  Array.from({ length: availableParallelism() }, async () => {
    while (queue.length && !harness) {
      try {
        await judge(queue.shift());
      } catch (err) {
        harness = err;
      }
    }
  }),
);
rmSync(dir, { recursive: true, force: true });
if (harness) {
  console.error(String(harness.message ?? harness));
  process.exit(2);
}
console.log(JSON.stringify({ spec: kernel.spec.name, seed, states: states.size, ...tally, disagreements: disagreements.length, first_disagreement: disagreements[0] ?? null }, null, 2));
process.exit(disagreements.length ? 1 : 0);
