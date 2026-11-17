// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): run fslc's own testgen suite against the generated core.
//
// `fslc testgen --target vitest` emits the spec's scenarios (cover, reach,
// forbidden) and a random walk, with the oracle states baked in by the fslc
// Monitor. This file takes that suite unchanged except for two wirings:
//   - `makeAdapter()` drives out/<Name>.ts (init / step / state as observed),
//   - the `vitest` import is a small local runner (test, expect), so no test
//     framework is needed.
// A step the suite expects to succeed must be taken by the core (a refused one
// fails the scenario, even when the expected state would not show it); a
// forbidden step must be refused, reported as kind "requires_failed". Only the
// requirements and business dialects declare `forbidden` scenarios; a kernel
// `spec` cannot, so for the specs this spike translates that branch is unused
// and the report's forbidden_steps_checked is 0.
//
// usage: node testgen.mjs <Name> [--depth n] [--mutate drop-guard:<a>|skip-effect:<a>|refuse:<a>]
//   CORE_CJS: load a deliberately wrong compiled core under the generated wrapper
// exit: 0 every scenario passes, 1 a scenario fails, 2 harness error

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

const require = createRequire(import.meta.url);
const [name, ...rest] = process.argv.slice(2);
const opt = (flag) => (rest.includes(flag) ? rest[rest.indexOf(flag) + 1] : null);
const here = new URL(".", import.meta.url).pathname;
const fslc = process.env.FSLC ?? join(here, "../../target/release/fslc");
const kernel = JSON.parse(readFileSync(join(here, `out/${name}.kernel.json`), "utf8"));
const spec = resolve(here, kernel.spec.source.file);
const harness = (msg) => {
  process.stderr.write(`${msg}\n`);
  process.exit(2);
};

// Same layout as check.mjs: a CORE_CJS copy sits under out/ so bignumber.js resolves.
const dir = join(here, "out", `.testgen-${process.pid}`);
mkdirSync(dir, { recursive: true });
process.on("exit", () => rmSync(dir, { recursive: true, force: true }));
copyFileSync(process.env.CORE_CJS ?? join(here, "out", `${name}.cjs`), join(dir, `${name}.cjs`));
copyFileSync(join(here, "out", `${name}.ts`), join(dir, `${name}.ts`));

const mutate = opt("--mutate");
if (mutate) {
  const core = require(join(dir, `${name}.cjs`)); // the instance the wrapper will import
  const [kind, action] = mutate.split(":");
  const key = (n) => n.replace(/_/g, "__");
  const D = core.M.__default;
  if (typeof D[key(`${action}_Step`)] !== "function") harness(`compiled core has no ${action}_Step`);
  if (kind === "drop-guard") D[key(`${action}_Enabled`)] = () => true;
  else if (kind === "skip-effect") D[key(`${action}_Apply`)] = (c, st) => st;
  else if (kind === "refuse") D[key(`${action}_Enabled`)] = () => false;
  else harness(`unknown mutation ${kind}`);
}

const types = new Map(kernel.types.map((t) => [t.name, t.definition]));
const widened = (() => {
  const w = /\/\/ widened: (.*)/.exec(readFileSync(join(here, `out/${name}.gen.dfy`), "utf8"))[1];
  return w === "(none)" ? [] : w.split(", ");
})();
const sizes = Object.fromEntries(widened.map((t) => [t, types.get(t).hi - types.get(t).lo + 1]));

// ---- the suite, as fslc emits it ------------------------------------------
const suite = join(dir, `${name}.test.ts`);
try {
  execFileSync(fslc, ["testgen", spec, "--target", "vitest", "-o", suite, ...(opt("--depth") ? ["--depth", opt("--depth")] : [])], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
} catch (err) {
  harness(`fslc testgen failed (exit ${err.status}): ${err.stdout}${err.stderr}`);
}
let text = readFileSync(suite, "utf8");
const wire = (from, to, label) => {
  const n = text.split(from).length - 1;
  if (n !== 1) harness(`testgen output changed shape: expected one ${label}, found ${n}`);
  text = text.replace(from, to);
};
wire('import { test, expect } from "vitest";', 'import { test, expect, runAll } from "../../testgen-runner.mjs";', "vitest import");
wire(
  'function makeAdapter(): Adapter {\n  throw new Error("wire your implementation: implement makeAdapter()");\n}',
  `import * as core from "./${name}.ts";
const SIZES = ${JSON.stringify(sizes)};
function makeAdapter(): Adapter {
  let state: unknown = null;
  return {
    reset() { state = core.init(SIZES); },
    step(action, params) {
      const next = core.step(SIZES, state as never, { action, params } as never);
      if (next === null) return { ok: false, kind: "requires_failed" };
      state = next;
      return { ok: true };
    },
    observe() { return state as Record<string, unknown>; },
  };
}
// A step the suite does not mark forbidden must be taken.
const TALLY = (globalThis as any).__testgen;
function mustStep(action: string, params: Record<string, unknown>): void {
  TALLY.steps++;
  const r = adapter.step(action, params) as StepResult;
  if (!r.ok) throw new Error(\`the core refused \${action} \${JSON.stringify(params)}, which the spec allows\`);
}`,
  "makeAdapter stub",
);
// The forbidden step is `const result = adapter.step(...)`, checked by assertRejected.
const forbidden = /const result = adapter\.step\(/g;
const forbiddenSites = (text.match(forbidden) ?? []).length;
text = text.replace(forbidden, "const result = (TALLY.forbidden++, adapter.step)(");
// The remaining `adapter.step(...)` statements are the allowed steps.
const allowed = /^(\s*)adapter\.step\(/gm;
const steps = (text.match(allowed) ?? []).length;
if (steps === 0) harness("testgen output has no steps");
text = text.replace(allowed, "$1mustStep(");
if (forbiddenSites !== (text.match(/assertRejected\(result, /g) ?? []).length) harness("testgen output changed shape: forbidden steps and assertRejected differ");
globalThis.__testgen = { steps: 0, forbidden: 0 };
writeFileSync(suite, `${text}\nawait runAll(${JSON.stringify(kernel.spec.name)}, globalThis.__testgen);\n`);

// The runner reports and sets the exit code; anything else is a harness error.
await import(suite).catch((err) => harness(`running the suite failed: ${err?.stack ?? err}`));
