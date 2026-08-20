// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

import {
  ACTION,
  DECLARED_EXEMPTIONS,
  EXPECTED_REF,
  MINIMUM_AUDITED_REFERENCES,
  auditReferences,
  auditWorkflowDirectory,
  collectCitations,
  collectReferences,
} from "./audit-toolchain-pin.mjs";

const WORKFLOWS = new URL("../workflows/", import.meta.url);

function usesLine(ref) {
  return `      - uses: ${ACTION}@${ref}`;
}

// --- accepting: the repository as committed ------------------------------------

test("the committed workflows agree on one pinned toolchain", async () => {
  const { audited, exempted, findings } = await auditWorkflowDirectory(WORKFLOWS);
  assert.deepEqual(findings, [], JSON.stringify(findings, null, 2));
  // A pin audit that audited nothing would pass vacuously. Assert it saw work,
  // and that the exempt reference is counted separately rather than reported as
  // if it were at the expected ref -- release.yml is pinned by commit SHA.
  assert.equal(audited, 11, `expected 11 audited references, saw ${audited}`);
  assert.equal(exempted, 1, `expected 1 exempt reference, saw ${exempted}`);
});

test("the declared exemption is release.yml and it is not audited", async () => {
  assert.deepEqual(Object.keys(DECLARED_EXEMPTIONS), ["release.yml"]);
  const findings = auditReferences({
    references: [
      { file: "release.yml", line: 54, ref: "4cda84d5c5c54efe2404f9d843567869ab1699d4" },
    ],
    citations: [],
  });
  assert.deepEqual(findings, []);
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
