// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

import {
  ACTION,
  DECLARED_EXEMPTIONS,
  EXPECTED_REF,
  MINIMUM_AUDITED_REFERENCES,
  MINIMUM_MSRV_REFERENCES,
  auditMsrvReference,
  auditReferences,
  auditWorkflowDirectory,
  collectCitations,
  collectReferences,
  collectToolchainInputs,
  declaredMsrv,
} from "./audit-toolchain-pin.mjs";

const WORKFLOWS = new URL("../workflows/", import.meta.url);
const CARGO_TOML = new URL("../../rust/Cargo.toml", import.meta.url);

// The suite must audit the repository the way the CLI does, MSRV included.
// Review found that scanning without the MSRV made the required lane skip the
// release contract entirely, so a floating release.yml passed the gate while
// only the standalone CLI caught it. Read the real declaration here.
const REPO_MSRV = declaredMsrv(await readFile(CARGO_TOML, "utf8"));

function usesLine(ref) {
  return `      - uses: ${ACTION}@${ref}`;
}

// --- accepting: the repository as committed ------------------------------------

test("the declared MSRV is readable, so the contract below is not vacuous", () => {
  assert.ok(REPO_MSRV, "rust/Cargo.toml must declare a rust-version");
});

test("the committed workflows agree on one pinned toolchain", async () => {
  // Passing the MSRV is what makes this exercise the release contract too.
  const { audited, exempted, findings } = await auditWorkflowDirectory(WORKFLOWS, {
    msrv: REPO_MSRV,
  });
  assert.deepEqual(findings, [], JSON.stringify(findings, null, 2));
  // A pin audit that audited nothing would pass vacuously. Assert it saw work,
  // and that the exempt reference is counted separately rather than reported as
  // if it were at the expected ref -- release.yml is pinned by commit SHA.
  assert.equal(audited, 11, `expected 11 audited references, saw ${audited}`);
  assert.equal(exempted, 1, `expected 1 exempt reference, saw ${exempted}`);
});

// --- release.yml is held to the MSRV contract, not skipped --------------------
//
// Review probed the earlier version by flipping release.yml to @stable and the
// audit reported nothing: "exempt" meant "anything goes here", so the MSRV
// guarantee could have been dropped silently. Each mutation below was measured
// against the replacement.

const RELEASE_SHA = "4cda84d5c5c54efe2404f9d843567869ab1699d4";

test("release.yml as committed satisfies the MSRV contract", () => {
  assert.deepEqual(Object.keys(DECLARED_EXEMPTIONS), ["release.yml"]);
  assert.deepEqual(
    auditMsrvReference({
      file: "release.yml",
      line: 54,
      ref: RELEASE_SHA,
      toolchainInput: "1.88.0",
      msrv: "1.88",
    }),
    [],
    "rust-version 1.88 must be satisfied by toolchain 1.88.0",
  );
});

test("flipping release.yml to a floating channel is rejected", () => {
  const findings = auditMsrvReference({
    file: "release.yml",
    line: 54,
    ref: "stable",
    toolchainInput: "1.88.0",
    msrv: "1.88",
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "msrv-ref-not-pinned");
  assert.match(findings[0].message, /40-character commit SHA/);
});

test("dropping the toolchain input from release.yml is rejected", () => {
  const findings = auditMsrvReference({
    file: "release.yml",
    line: 54,
    ref: RELEASE_SHA,
    toolchainInput: null,
    msrv: "1.88",
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "msrv-toolchain-missing");
});

test("a YAML alias is reported as unresolved, not as a disagreement", () => {
  // Review probed `toolchain: *msrv` with the anchor defined elsewhere in the
  // file. The earlier version reported msrv-disagreement, which named a
  // disagreement it had not established. A line scan cannot resolve an alias, so
  // the honest finding is that the equality is unproven.
  const findings = auditMsrvReference({
    file: "release.yml",
    line: 54,
    ref: RELEASE_SHA,
    toolchainInput: "*msrv",
    msrv: "1.88",
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "msrv-alias-unresolved");
  assert.match(findings[0].message, /cannot resolve it/);
  assert.doesNotMatch(
    findings[0].message,
    /must be built at the version/,
    "must not claim a disagreement it has not established",
  );
});

test("a release toolchain that drifts off the declared MSRV is rejected", () => {
  const findings = auditMsrvReference({
    file: "release.yml",
    line: 54,
    ref: RELEASE_SHA,
    toolchainInput: "1.89.0",
    msrv: "1.88",
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "msrv-disagreement");
});

test("a missing rust-version declaration is a finding, not a pass", () => {
  const findings = auditMsrvReference({
    file: "release.yml",
    line: 54,
    ref: RELEASE_SHA,
    toolchainInput: "1.88.0",
    msrv: null,
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "msrv-undeclared");
});

test("an env: toolchain is not an action input and must not satisfy the contract", () => {
  // Review probed this and it passed BOTH the suite and the standalone CLI:
  // the action receives no input from `env:`, so the audit was satisfied by
  // something that does nothing.
  const text = [
    `      - uses: ${ACTION}@${RELEASE_SHA}`,
    "        env:",
    "          toolchain: 1.88.0",
  ].join("\n");
  const references = collectReferences("release.yml", text);
  assert.equal(references.length, 1);
  const inputs = collectToolchainInputs(text, references);
  assert.equal(inputs.get(1), undefined, "env: must not be read as an input");

  const findings = auditReferences({
    references,
    citations: [],
    msrv: "1.88",
    toolchainInputs: inputs,
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "msrv-toolchain-missing");
});

// Probed by hand after the with:-mapping rule was added. Three of these were
// wrong on the first attempt -- two FALSE REJECTS (flow style, `with:` before
// `uses:`) and one FALSE ACCEPT (a key nested deeper than `with:`). A false
// reject matters as much as a false accept: it rejects a legitimate spelling,
// which invites someone to loosen the rule and reopen the hole.
// Every spelling below was probed against an earlier revision and MISSED, and
// actionlint accepts each one, so each was a false reject of valid workflow YAML.
// A false reject invites someone to loosen the rule and reopen the hole it
// exists to close, which is why they are controls rather than notes.
test("the with: mapping is recognised whatever its child indentation", () => {
  // GitHub does not mandate two-space indentation. An earlier revision required
  // exactly `with:` + 2 and therefore missed a four-space mapping.
  for (const pad of ["  ", "    ", "      "]) {
    const text = [
      `      - uses: ${ACTION}@${RELEASE_SHA}`,
      "        with:",
      `        ${pad}toolchain: 1.88.0`,
    ].join("\n");
    const references = collectReferences("release.yml", text);
    assert.equal(
      collectToolchainInputs(text, references).get(references[0].line),
      "1.88.0",
      `child indented by ${pad.length}`,
    );
  }
});

test("a flow mapping spanning several lines is recognised", () => {
  const text = [
    `      - uses: ${ACTION}@${RELEASE_SHA}`,
    "        with: {",
    "          toolchain: 1.88.0",
    "        }",
  ].join("\n");
  const references = collectReferences("release.yml", text);
  assert.equal(
    collectToolchainInputs(text, references).get(references[0].line),
    "1.88.0",
  );
});

test("a trailing comment after a flow mapping is not part of the value", () => {
  const text = [
    `      - uses: ${ACTION}@${RELEASE_SHA}`,
    "        with: {toolchain: 1.88.0} # MSRV, keep in sync with rust/Cargo.toml",
  ].join("\n");
  const references = collectReferences("release.yml", text);
  assert.equal(
    collectToolchainInputs(text, references).get(references[0].line),
    "1.88.0",
  );
});

test("a declared MSRV path with no reference at all is rejected", async () => {
  // Review deleted release.yml's toolchain step outright. The required lane
  // caught it because the test below asserts the count, but the CLI reported
  // `PASS -- ... 0 reference(s) held to the MSRV contract`: a declared contract
  // with nothing to apply it to enforces nothing.
  assert.equal(MINIMUM_MSRV_REFERENCES, Object.keys(DECLARED_EXEMPTIONS).length);
  const { exempted } = await auditWorkflowDirectory(WORKFLOWS, { msrv: REPO_MSRV });
  assert.ok(
    exempted >= MINIMUM_MSRV_REFERENCES,
    `expected at least ${MINIMUM_MSRV_REFERENCES} MSRV reference(s), saw ${exempted}`,
  );
});

test("the with: mapping is recognised in every legitimate spelling", () => {
  const cases = [
    ["block", `      - uses: ${ACTION}@${RELEASE_SHA}\n        with:\n          toolchain: 1.88.0`, "1.88.0"],
    ["flow", `      - uses: ${ACTION}@${RELEASE_SHA}\n        with: {toolchain: 1.88.0}`, "1.88.0"],
    [
      "comment between",
      `      - uses: ${ACTION}@${RELEASE_SHA}\n        with:\n          # pinned MSRV\n          toolchain: 1.88.0`,
      "1.88.0",
    ],
    [
      "with: before uses:",
      `      - with:\n          toolchain: 1.88.0\n        uses: ${ACTION}@${RELEASE_SHA}`,
      "1.88.0",
    ],
  ];
  for (const [label, text, expected] of cases) {
    const references = collectReferences("release.yml", text);
    assert.equal(references.length, 1, label);
    assert.equal(
      collectToolchainInputs(text, references).get(references[0].line),
      expected,
      label,
    );
  }
});

test("a toolchain key nested deeper than with: is not an action input", () => {
  const text = [
    `      - uses: ${ACTION}@${RELEASE_SHA}`,
    "        with:",
    "          extra:",
    "            toolchain: 1.88.0",
  ].join("\n");
  const references = collectReferences("release.yml", text);
  const inputs = collectToolchainInputs(text, references);
  assert.equal(
    inputs.get(references[0].line),
    undefined,
    "only a direct child of with: is an input",
  );
});

test("a with: toolchain in a LATER step is not attributed to this one", () => {
  const text = [
    `      - uses: ${ACTION}@${RELEASE_SHA}`,
    "      - uses: actions/checkout@v4",
    "        with:",
    "          toolchain: 1.88.0",
  ].join("\n");
  const references = collectReferences("release.yml", text);
  const inputs = collectToolchainInputs(text, references);
  assert.equal(inputs.get(1), undefined, "the next step's with: is not ours");
});

test("omitting the MSRV is a finding rather than a silent skip", () => {
  // The earlier version skipped the whole contract when msrv was undefined,
  // which is how the required lane came to never run it.
  const findings = auditReferences({
    references: [{ file: "release.yml", line: 54, ref: RELEASE_SHA }],
    citations: [],
    toolchainInputs: new Map([[54, "1.88.0"]]),
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "msrv-undeclared");
});

test("declaredMsrv reads rust-version and reports absence as null", () => {
  assert.equal(declaredMsrv('[workspace.package]\nrust-version = "1.88"\n'), "1.88");
  assert.equal(declaredMsrv("[workspace.package]\nedition = \"2021\"\n"), null);
});

test("collectToolchainInputs attributes the input to its own step", () => {
  const text = [
    `      - uses: ${ACTION}@${RELEASE_SHA}`,
    "        with:",
    "          toolchain: 1.88.0",
    "      - uses: actions/checkout@v4",
    "        with:",
    "          toolchain: 9.9.9",
  ].join("\n");
  const references = collectReferences("release.yml", text);
  assert.equal(references.length, 1);
  const inputs = collectToolchainInputs(text, references);
  assert.equal(inputs.get(1), "1.88.0", "must not read the next step's input");
});

test("an unrelated workflow with no toolchain reference is not a finding", () => {
  assert.deepEqual(collectReferences("site.yml", "jobs:\n  build:\n    steps: []\n"), []);
  assert.deepEqual(collectCitations("site.yml", "# nothing to see\n"), []);
});

// --- rejecting: each mutation this control exists to catch ---------------------

test("reverting one reference to @stable is rejected", () => {
  const findings = auditReferences({
    references: [
      { file: "ci.yml", line: 98, ref: EXPECTED_REF },
      { file: "ci.yml", line: 146, ref: "stable" },
    ],
    citations: [],
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "floating-channel");
  assert.equal(findings[0].line, 146);
  assert.match(findings[0].message, /turn `main` red with no repository change/);
});

test("nightly and beta are rejected for the same reason as stable", () => {
  for (const channel of ["nightly", "beta"]) {
    const findings = auditReferences({
      references: [{ file: "ci.yml", line: 98, ref: channel }],
      citations: [],
    });
    assert.equal(findings.length, 1, channel);
    assert.equal(findings[0].class, "floating-channel", channel);
  }
});

test("a split toolchain across two workflows is rejected", () => {
  const findings = auditReferences({
    references: [
      { file: "ci.yml", line: 98, ref: EXPECTED_REF },
      { file: "merge-readiness.yml", line: 26, ref: "1.97.0" },
    ],
    citations: [],
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "ref-disagreement");
  assert.equal(findings[0].file, "merge-readiness.yml");
  // The consequence is specific and worth stating in the diagnostic: the
  // restore-only cache key stops matching.
  assert.match(findings[0].message, /restore-only key match against ci\.yml/);
});

test("a citation left behind by a version bump is rejected", () => {
  const findings = auditReferences({
    references: [{ file: "merge-readiness.yml", line: 26, ref: EXPECTED_REF }],
    citations: [{ file: "merge-readiness.yml", line: 43, ref: "stable" }],
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "stale-citation");
  assert.equal(findings[0].line, 43);
  assert.match(findings[0].message, /must name what the code actually does/);
});

test("an undeclared workflow cannot inherit release.yml's exemption", () => {
  const findings = auditReferences({
    references: [{ file: "impostor-release.yml", line: 10, ref: "stable" }],
    citations: [],
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "floating-channel");
});

// --- the audit must not pass vacuously ----------------------------------------
//
// Review found the CLI reporting `PASS -- 0 audited reference(s)` against a
// workflow directory with no toolchain references. An audit that audited
// nothing and reported success is the exact defect this file exists to prevent,
// so the floor is enforced in the script and pinned here.

test("MINIMUM_AUDITED_REFERENCES matches what the repository actually has", async () => {
  const { audited } = await auditWorkflowDirectory(WORKFLOWS);
  assert.equal(audited, MINIMUM_AUDITED_REFERENCES);
});

test("an empty workflow directory audits nothing, which the CLI must reject", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "toolchain-pin-empty-"));
  t.after(() => rm(directory, { recursive: true, force: true }));

  const { audited, exempted, findings } = await auditWorkflowDirectory(
    pathToFileURL(`${directory}/`),
  );
  // The pure function has nothing to complain about -- there are no references
  // to disagree with. That is precisely why the count, not the findings list,
  // has to be what fails.
  assert.deepEqual(findings, []);
  assert.equal(audited, 0);
  assert.equal(exempted, 0);
  assert.ok(
    audited < MINIMUM_AUDITED_REFERENCES,
    "an empty directory must fall below the floor",
  );
});

// --- escape routes that an earlier revision of this audit did not see ----------
//
// Each of these was measured against the first implementation and passed
// silently. A pin audit with holes is worse than no audit, because it creates
// confidence the pin does not have. Do not relax collectReferences without
// re-running these.

test("a quoted uses value is collected, not skipped", () => {
  const text = `      - uses: "${ACTION}@stable"`;
  assert.deepEqual(collectReferences("probe.yml", text), [
    { file: "probe.yml", line: 1, ref: "stable" },
  ]);
  const findings = auditReferences({
    references: collectReferences("probe.yml", text),
    citations: [],
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "floating-channel");
});

test("a single-quoted uses value is collected too", () => {
  const text = `      - uses: '${ACTION}@stable'`;
  assert.deepEqual(collectReferences("probe.yml", text), [
    { file: "probe.yml", line: 1, ref: "stable" },
  ]);
});

test("a uses: sibling of - name: is collected without the list dash", () => {
  const text = ["      - name: install the toolchain", `        uses: ${ACTION}@stable`].join(
    "\n",
  );
  assert.deepEqual(collectReferences("probe.yml", text), [
    { file: "probe.yml", line: 2, ref: "stable" },
  ]);
});

test("whitespace before the colon does not hide the reference", () => {
  // `uses : action@ref` is valid YAML whose key is `uses` -- confirmed with a
  // YAML parser. An earlier revision required `uses:` exactly and missed it.
  const text = `      - uses : ${ACTION}@stable`;
  assert.deepEqual(collectReferences("probe.yml", text), [
    { file: "probe.yml", line: 1, ref: "stable" },
  ]);
  const findings = auditReferences({
    references: collectReferences("probe.yml", text),
    citations: [],
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "floating-channel");
});

test("an expression ref is rejected because it cannot be shown to be pinned", () => {
  const text = `      - uses: ${ACTION}@\${{ matrix.toolchain }}`;
  const references = collectReferences("probe.yml", text);
  assert.equal(references.length, 1);
  const findings = auditReferences({ references, citations: [] });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].class, "expression-ref");
  assert.match(findings[0].message, /cannot be shown to be\s+pinned/);
});

test("a trailing comment is not treated as part of the ref", () => {
  const text = `      - uses: ${ACTION}@1.98.0  # keep in sync with EXPECTED_REF`;
  assert.deepEqual(collectReferences("probe.yml", text), [
    { file: "probe.yml", line: 1, ref: "1.98.0" },
  ]);
  assert.deepEqual(
    auditReferences({ references: collectReferences("probe.yml", text), citations: [] }),
    [],
  );
});

// --- the collectors themselves -------------------------------------------------

test("collectReferences reads the ref and the one-based line", () => {
  const text = ["jobs:", "  a:", "    steps:", usesLine("1.98.0")].join("\n");
  assert.deepEqual(collectReferences("ci.yml", text), [
    { file: "ci.yml", line: 4, ref: "1.98.0" },
  ]);
});

test("collectCitations reads comments and collectReferences does not", () => {
  const text = `      # ${ACTION}@stable on ubuntu-latest\n${usesLine("1.98.0")}`;
  assert.deepEqual(collectCitations("merge-readiness.yml", text), [
    { file: "merge-readiness.yml", line: 1, ref: "stable" },
  ]);
  assert.deepEqual(collectReferences("merge-readiness.yml", text), [
    { file: "merge-readiness.yml", line: 2, ref: "1.98.0" },
  ]);
});
