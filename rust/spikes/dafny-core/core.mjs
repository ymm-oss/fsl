// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): one command from an FSL spec to a proved TypeScript core.
//
//   1. fslc kernel -> gen.mjs -> out/<Name>.gen.dfy     (symmetric types widened)
//   2. proof gates (build.sh): structure, dafny audit, dafny verify
//      with --ai, a failing proof is rewritten by the model (prove.mjs) and re-gated
//   3. dafny translate js -> out/<Name>.cjs; gen-ts.mjs -> out/<Name>.ts; tsc type-checks it
//   4. differential against the fslc Monitor, through out/<Name>.ts:
//      exhaustive at the spec's sizes, then seeded random sequences on a copy of
//      the spec whose widened types are --scale times larger (the generated
//      Dafny must not change with the sizes, so the same core is what is checked)
//
// Any failure exits nonzero. --out copies the core (<Name>.ts, .cjs, .d.cts) to
// an app; --watch re-runs on every change to the spec or the proof file.
//
// usage: node core.mjs <spec.fsl> [--widen T1,T2] [--proof file] [--ai [--attempts n]]
//                      [--random runs] [--length n] [--seed n] [--scale k] [--out dir] [--watch]

import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, readFileSync, statSync, watch, writeFileSync } from "node:fs";
import { basename, join, relative, resolve } from "node:path";
import { widenedTypes } from "./gen.mjs";

const here = new URL(".", import.meta.url).pathname;
const args = process.argv.slice(2);
const flag = (f) => args.includes(f);
const opt = (f, d) => (args.includes(f) ? args[args.indexOf(f) + 1] : d);
const specArg = args[0];
if (!specArg || specArg.startsWith("--")) {
  process.stderr.write("usage: node core.mjs <spec.fsl> [--widen T1,T2] [--ai] [--out dir] [--watch] ...\n");
  process.exit(2);
}
// Every fslc call runs from this directory, so Kernel source paths are relative to it.
const spec = relative(here, resolve(specArg));
const widen = opt("--widen", "");
const fslc = process.env.FSLC ?? join(here, "../../target/release/fslc");
const random = opt("--random", "50");
const length = opt("--length", "30");
const scale = Number(opt("--scale", 4));
const outDir = opt("--out", null);

function sh(label, cmd, cmdArgs, extra = {}) {
  const r = spawnSync(cmd, cmdArgs, { cwd: here, encoding: "utf8", maxBuffer: 64 << 20, ...extra });
  return { ok: r.status === 0, code: r.status, out: r.stdout ?? "", err: r.stderr ?? "", label };
}
const fail = (step, r) => {
  process.stderr.write(`${r.out}${r.err}\nFAILED at ${step} (exit ${r.code})\n`);
  return r.code === 2 ? 2 : 1;
};

function once() {
  mkdirSync(join(here, "out", "big"), { recursive: true });
  const k = sh("kernel", fslc, ["kernel", spec]);
  if (!k.ok) return fail("fslc kernel", k);
  const kernel = JSON.parse(k.out);
  const name = kernel.spec.name;
  const proof = opt("--proof", join("proofs", `${name}.proof.dfy`));
  const widened = widenedTypes(kernel, widen.split(",").filter(Boolean));

  // 1-3: translate, prove (optionally with the model), compile, generate TS.
  const b = flag("--ai")
    ? sh("prove", "node", ["prove.mjs", name, spec, ...(widen ? ["--widen", widen] : []), "--proof", proof, "--attempts", opt("--attempts", "3")], { stdio: ["ignore", "pipe", "inherit"] })
    : sh("build", "./build.sh", [name, spec, ...(widen ? [widen] : [])], { env: { ...process.env, PROOF: proof } });
  if (!b.ok) return fail(flag("--ai") ? "prove (model + gates)" : "proof gates (build.sh)", b);
  const tsc = sh("tsc", join(here, "node_modules/.bin/tsc"), [
    "--noEmit", "--strict", "--module", "nodenext", "--moduleResolution", "nodenext", "--target", "es2022",
    "--allowImportingTsExtensions", "--skipLibCheck", `out/${name}.ts`,
  ]);
  if (!tsc.ok) return fail("tsc out/" + name + ".ts", tsc);

  // 4a: exhaustive differential at the spec's own sizes.
  const ex = sh("exhaustive", "node", ["check.mjs", name]);
  if (!ex.ok) return fail("differential (exhaustive, spec sizes)", ex);
  const exr = JSON.parse(ex.out);

  // 4b: the spec with every widened type `scale` times larger.
  let src = readFileSync(join(here, spec), "utf8");
  const sizes = {};
  for (const t of widened) {
    const re = new RegExp(`^(\\s*(?:symmetric\\s+)?type\\s+${t}\\s*=\\s*0\\s*\\.\\.\\s*)(\\d+)`, "gm");
    const hits = [...src.matchAll(re)];
    if (hits.length !== 1) return fail(`enlarging ${t}`, { code: 1, out: "", err: `expected one 'type ${t} = 0..<hi>' in ${spec}, found ${hits.length}\n` });
    sizes[t] = (Number(hits[0][2]) + 1) * scale;
    src = src.replace(re, (_, head) => `${head}${sizes[t] - 1}`);
  }
  const bigSpec = join("out", "big", basename(spec));
  writeFileSync(join(here, bigSpec), src);
  const bk = sh("kernel (enlarged)", fslc, ["kernel", bigSpec]);
  if (!bk.ok) return fail("fslc kernel (enlarged spec)", bk);
  const bigKernel = join("out", "big", `${name}.kernel.json`);
  writeFileSync(join(here, bigKernel), bk.out);
  const bg = sh("gen (enlarged)", "node", ["gen.mjs", bigKernel, ...(widen ? ["--widen", widen] : [])]);
  const body = (text) => text.split("\n").slice(1).join("\n"); // line 1 names the source file
  if (!bg.ok || body(bg.out) !== body(readFileSync(join(here, "out", `${name}.gen.dfy`), "utf8")))
    return fail("generated Dafny depends on the sizes", { code: 1, out: bg.out, err: bg.err || "the enlarged spec translates to different Dafny\n" });
  const seed = opt("--seed", String(Math.floor(Math.random() * 2 ** 31)));
  const rn = sh("random", "node", ["check.mjs", name, "--random", random, "--length", length, "--seed", seed], { env: { ...process.env, KERNEL: join(here, bigKernel) } });
  if (!rn.ok) return fail(`differential (random, seed ${seed})`, rn);
  const rnr = JSON.parse(rn.out);

  if (outDir) {
    mkdirSync(outDir, { recursive: true });
    for (const ext of ["ts", "cjs", "d.cts"]) copyFileSync(join(here, "out", `${name}.${ext}`), join(outDir, `${name}.${ext}`));
  }
  console.log(
    JSON.stringify({
      spec: name,
      proved_for_every_size_of: widened,
      proof,
      core: outDir ? join(outDir, `${name}.ts`) : `out/${name}.ts`,
      exhaustive: { sizes: exr.sizes, reachable_states: exr.reachable_states, steps_checked: exr.steps_checked, divergences: exr.divergences },
      random: { sizes: rnr.sizes, seed: Number(seed), runs: Number(random), length: Number(length), steps_checked: rnr.steps_checked, divergences: rnr.divergences },
    }),
  );
  return 0;
}

if (!flag("--watch")) process.exit(once());

// --watch: re-run on change; never exits on a failing run.
let timer = null;
// Events queued while a (synchronous) run was going fire afterwards; re-run only
// when a watched file really changed since the last run started.
const watched = () => [join(here, spec), ...(flag("--ai") ? [] : readdirSync(join(here, "proofs")).map((f) => join(here, "proofs", f)))];
const stamp = () => watched().map((f) => statSync(f).mtimeMs).join(",");
let last = null;
const rerun = () => {
  clearTimeout(timer);
  timer = setTimeout(() => {
    const now = stamp();
    if (now === last) return;
    last = now;
    process.stderr.write(`\n--- ${new Date().toISOString()} ${spec}\n`);
    const code = once();
    process.stderr.write(code === 0 ? "ok\n" : `failed (exit ${code})\n`);
  }, 200);
};
rerun();
watch(join(here, spec), rerun);
// With --ai the model writes the proof file itself; watching it would re-run on its own write.
if (!flag("--ai")) watch(join(here, "proofs"), rerun);
