// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): differential check of the compiled Dafny core against the
// fslc Monitor. At the spec's own domain sizes it walks every state reachable
// in the JS core and tries every action instance there. Each step is judged by
// `fslc replay --trace` with the path that reached the state:
//   JS enabled  -> the observed post-state must be `conformant`
//   JS disabled -> the spec must answer `requires_failed` for the same step
// Agreement on every step from the shared initial state means the two
// transition relations are equal at these sizes.
//
// usage: node check.mjs <Name> [--mutate drop-guard:<action>|skip-effect:<action>]

import { execFile } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);
const require = createRequire(import.meta.url);
const BigNumber = require("bignumber.js");

const [name, ...rest] = process.argv.slice(2);
const mutate = rest[rest.indexOf("--mutate") + 1] && rest.includes("--mutate") ? rest[rest.indexOf("--mutate") + 1] : null;
const here = new URL(".", import.meta.url).pathname;
const fslc = process.env.FSLC ?? join(here, "../../target/release/fslc");
// KERNEL lets the same compiled core be checked against the spec at other sizes.
const kernel = JSON.parse(readFileSync(process.env.KERNEL ?? join(here, `out/${name}.kernel.json`), "utf8"));
// build.sh runs `fslc kernel` from this directory, so the source path is relative to it.
const spec = resolve(here, kernel.spec.source.file);
// CORE_CJS lets a negative control load a deliberately wrong core.
const { M } = require(process.env.CORE_CJS ?? join(here, `out/${name}.cjs`));
const D = M.__default;
// Dafny's JS backend doubles every underscore in an identifier.
const fn = (dafnyName) => {
  const f = D[dafnyName.replace(/_/g, "__")];
  if (typeof f !== "function") throw new Error(`compiled core has no ${dafnyName}`);
  return f;
};
const setFn = (dafnyName, f) => {
  fn(dafnyName);
  D[dafnyName.replace(/_/g, "__")] = f;
};

// ---- negative controls: alter the compiled core in memory -----------------
if (mutate) {
  const [kind, action] = mutate.split(":");
  if (kind === "drop-guard") setFn(`${action}_Enabled`, () => true);
  else if (kind === "skip-effect") setFn(`${action}_Apply`, (c, st) => st);
  else throw new Error(`unknown mutation ${kind}`);
}

// ---- Kernel types <-> Dafny values ----------------------------------------
const types = new Map(kernel.types.map((t) => [t.name, t.definition]));
const values = (tyName) => {
  const d = types.get(tyName);
  return Array.from({ length: d.hi - d.lo + 1 }, (_, k) => d.lo + k);
};
const config = (() => {
  const widened = /\/\/ widened: (.*)/.exec(readFileSync(join(here, `out/${name}.gen.dfy`), "utf8"))[1];
  const names = widened === "(none)" ? [] : widened.split(", ");
  // Check at the spec's own sizes: the Monitor only knows those.
  return M.Config.create_Config(...names.map((t) => new BigNumber(values(t).length)));
})();

function toObs(ty, v) {
  switch (ty.kind) {
    case "named":
    case "int":
      return v.toNumber();
    case "bool":
      return v;
    case "option":
      if (v.is_None) return ty.item.kind === "option" ? { kind: "none" } : null;
      return ty.item.kind === "option" ? { kind: "some", value: toObs(ty.item, v.dtor_value) } : toObs(ty.item, v.dtor_value);
    case "map": {
      const lo = types.get(ty.key.name).lo;
      return Object.fromEntries(Array.from(v, (x, k) => [String(k + lo), toObs(ty.value, x)]));
    }
    default:
      throw new Error(`type ${ty.kind}`);
  }
}
const observe = (st) =>
  Object.fromEntries(kernel.state.map((s) => [s.name, toObs(s.type, fn(`Get_${s.name}`)(st))]));

// ---- replay oracle --------------------------------------------------------
const dir = mkdtempSync(join(tmpdir(), "dafny-core-"));
let traceNo = 0;
async function replay(events) {
  const file = join(dir, `t${traceNo++}.json`);
  writeFileSync(
    file,
    JSON.stringify({
      $schema: "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json",
      schema_version: "1.2.0",
      kernel_schema_version: "1.0.0",
      spec: kernel.spec.name,
      initial: observe(D.Init(config)),
      events: events.map((e, k) => ({ tick: k + 1, ...e })),
    }),
  );
  try {
    const { stdout } = await run(fslc, ["replay", spec, "--trace", file]);
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

// ---- enumerate ------------------------------------------------------------
const instances = kernel.actions.flatMap((a) => {
  let tuples = [[]];
  for (const p of a.parameters) tuples = tuples.flatMap((t) => values(p.type.name).map((v) => [...t, [p.name, v]]));
  return tuples.map((t) => ({ action: a.name, params: Object.fromEntries(t) }));
});

const init = D.Init(config);
const seen = new Map([[JSON.stringify(observe(init)), { st: init, path: [] }]]);
const queue = [...seen.values()];
const jobs = [];
const divergences = [];
let checked = 0;

const first = await replay([]);
if (first.result !== "conformant") divergences.push({ step: "initial", verdict: first });

const STATE_LIMIT = 100000;
while (queue.length) {
  if (seen.size > STATE_LIMIT) throw new Error(`more than ${STATE_LIMIT} reachable states at the spec's sizes`);
  const { st, path } = queue.shift();
  for (const inst of instances) {
    const args = Object.values(inst.params).map((v) => new BigNumber(v));
    // Through the total Step entry point, the one a TypeScript caller uses.
    let enabled = false;
    let next = st;
    try {
      const r = fn(`${inst.action}_Step`)(config, st, ...args);
      enabled = r.is_Some;
      if (enabled) next = r.dtor_value;
    } catch (err) {
      // The core ran outside its proved precondition: a divergence, not a harness failure.
      divergences.push({ path: path.map((e) => `${e.action}(${JSON.stringify(e.params)})`), step: inst, js_enabled: enabled, verdict: "js_exception", violation: { kind: "js_exception", message: String(err.message) } });
      continue;
    }
    const event = { action: inst.action, params: inst.params, state: observe(next) };
    jobs.push({ path, event, enabled });
    // A post-state outside the type bounds is already a divergence (the Monitor
    // rejects it); do not explore from it, or a broken core can grow forever.
    if (enabled && fn("Wf")(config, next)) {
      const key = JSON.stringify(event.state);
      if (!seen.has(key)) {
        const node = { st: next, path: [...path, event] };
        seen.set(key, node);
        queue.push(node);
      }
    }
  }
}

async function judge({ path, event, enabled }) {
  const r = await replay([...path, event]);
  checked++;
  const ok = enabled ? r.result === "conformant" : r.result === "nonconformant" && r.violation?.kind === "requires_failed" && r.failed_at_event === path.length;
  if (!ok) divergences.push({ path: path.map((e) => `${e.action}(${JSON.stringify(e.params)})`), step: event, js_enabled: enabled, verdict: r.result, violation: r.violation });
}
const pool = Array.from({ length: 8 }, async () => {
  while (jobs.length) await judge(jobs.shift());
});
await Promise.all(pool);

const report = {
  spec: kernel.spec.name,
  mutation: mutate,
  reachable_states: seen.size,
  action_instances: instances.length,
  steps_checked: checked,
  divergences: divergences.length,
  first_divergence: divergences[0] ?? null,
};
console.log(JSON.stringify(report, null, 2));
process.exit(divergences.length ? 1 : 0);
