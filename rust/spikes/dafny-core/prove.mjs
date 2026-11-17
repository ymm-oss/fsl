// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): the AI half of "an AI writes the lemmas, Dafny decides".
//
// Runs build.sh. If the proof file is missing or fails a gate (structure,
// audit, verify), it asks a model for a new proof file, writes it, and runs
// the same unchanged build.sh again, up to --attempts times. The model can only
// write the proof file; whether it holds is decided by build.sh alone, so a
// wrong or cheating proof is rejected like a hand-written one. A spec whose
// obligations are false (a real bug) cannot pass, however many attempts.
//
// The model is the `claude` CLI in print mode with no tools (CLAUDE env to
// override the command). It runs only when the current proof fails.
//
// usage: node prove.mjs <Name> <spec.fsl> [--widen T1,T2] [--proof file] [--attempts 3] [--timeout-s 900]
// exit:  0 proved (by the existing or a new proof), 1 not proved, 2 spec not translatable,
//        3 the generated module itself fails (a gen.mjs defect, not a proof problem)

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const here = new URL(".", import.meta.url).pathname;
const [name, spec, ...rest] = process.argv.slice(2);
const opt = (flag, d) => (rest.includes(flag) ? rest[rest.indexOf(flag) + 1] : d);
const widen = opt("--widen", "");
const proof = opt("--proof", join("proofs", `${name}.proof.dfy`));
const attempts = Number(opt("--attempts", 3));
const timeoutS = Number(opt("--timeout-s", 900));
const claude = (process.env.CLAUDE ?? "claude").split(" ");

function build() {
  const r = spawnSync("./build.sh", [name, spec, ...(widen ? [widen] : [])], {
    cwd: here,
    encoding: "utf8",
    env: { ...process.env, PROOF: proof },
  });
  return { code: r.status, log: `${r.stdout}${r.stderr}` };
}

// A missing proof file fails the structure gate; give build.sh an empty one so
// the generated module still exists for the prompt.
const proofPath = join(here, proof);
if (!existsSync(proofPath)) {
  mkdirSync(dirname(proofPath), { recursive: true });
  writeFileSync(proofPath, "");
}

let r = build();
for (let n = 1; r.code !== 0; n++) {
  if (r.code === 2) {
    process.stderr.write(r.log);
    process.exit(2);
  }
  // An error or warning located in the generated module is a gen.mjs defect;
  // no proof file can fix it, so do not spend a model attempt on it.
  const genFault = new RegExp(`^out/${name}\\.gen\\.dfy\\(\\d+,\\d+\\): (Error|Warning)`, "m");
  if (genFault.test(r.log)) {
    process.stderr.write(`${r.log}\nthe generated module out/${name}.gen.dfy itself fails (a gen.mjs defect); not asking the model\n`);
    process.exit(3);
  }
  if (n > attempts) {
    process.stderr.write(`${r.log}\nnot proved after ${attempts} attempt(s); ${proof} holds the last one\n`);
    process.exit(1);
  }
  const generated = readFileSync(join(here, "out", `${name}.gen.dfy`), "utf8");
  const current = readFileSync(proofPath, "utf8");
  const prompt = `You write a Dafny proof module. Reply with the complete file in one \`\`\`dafny code block and nothing else.

The generated module below is a transition system translated from an FSL spec. Its
lemmas without bodies are the proof obligations: InitEstablishes and <action>_Preserves.
Write the refining module that gives every one of them a body that Dafny 4.11 verifies.

Rules (checked mechanically; a file that breaks one is rejected):
- The file is exactly: the line \`include "../out/${name}.gen.dfy"\`, then one module
  \`module ${name}Proof refines ${name}Spec {\` ... \`}\`. No other include or module.
- Inside it: bodies for the bodiless lemmas (repeat each signature without
  requires/ensures, as refinement allows), plus any helper lemmas or predicates
  you need. Do not redefine anything the generated module defines.
- No assume, no {:axiom}, no {:verify false}, no lemma left without a body.
- CountK functions recurse over i; facts about them usually need a helper lemma
  proved by induction on i (decreases <bound> - i).
- If an obligation is false (the spec allows a state outside a type bound or an
  invariant), no proof exists; still reply with your best attempt.

Generated module (out/${name}.gen.dfy):
\`\`\`dafny
${generated}\`\`\`

Current proof file (${proof}):
\`\`\`dafny
${current || "(empty)"}
\`\`\`

Result of checking it:
\`\`\`
${r.log.slice(-6000)}
\`\`\`
`;
  process.stderr.write(`attempt ${n}/${attempts}: asking ${claude[0]} for ${proof}\n`);
  const ai = spawnSync(claude[0], [...claude.slice(1), "-p", "--tools", "", "--no-session-persistence", "--output-format", "text"], {
    input: prompt,
    encoding: "utf8",
    timeout: timeoutS * 1000,
    maxBuffer: 16 << 20,
  });
  if (ai.status !== 0) {
    process.stderr.write(`${claude[0]} failed (status ${ai.status}${ai.error ? `, ${ai.error.message}` : ""}): ${ai.stderr}\n`);
    process.exit(1);
  }
  const m = /```dafny\n([\s\S]*?)```/.exec(ai.stdout);
  if (!m) {
    process.stderr.write(`attempt ${n}: no dafny block in the reply\n`);
    r = { code: 1, log: "the previous reply had no ```dafny code block" };
    continue;
  }
  const header = "// SPDX-License-Identifier: Apache-2.0\n// Written by a model through prove.mjs; accepted only because build.sh's gates pass.\n\n";
  writeFileSync(proofPath, m[1].startsWith("// SPDX") ? m[1] : header + m[1]);
  r = build();
}
process.stderr.write(`proved: ${proof}\n`);
