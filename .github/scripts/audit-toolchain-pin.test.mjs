// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import {
  ACTION,
  DECLARED_EXEMPTIONS,
  EXPECTED_REF,
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
