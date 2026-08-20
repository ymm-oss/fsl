// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  CACHE_BUDGET_AUDIT_LABEL,
  MAX_DETAILED_OCCURRENCE_COMMENTS,
  cacheBudgetAuditMarker,
  occurrenceMarker,
  reconcileCacheBudgetAudit,
} from "./report-cache-budget-audit.mjs";
import { HISTORICAL_SUMMARY_FIXTURES } from "./fixtures/cache-budget-audit-summary-fixtures.mjs";

const REPORTER_PATH = fileURLToPath(new URL("./report-cache-budget-audit.mjs", import.meta.url));
const TEST_PATH = fileURLToPath(import.meta.url);

class FakeClient {
  constructor({ issues = [], comments = [], completedRuns = [] } = {}) {
    this.issues = issues;
    this.comments = comments;
    this.completedRuns = completedRuns;
    this.labels = new Set();
    this.nextIssue = 100;
    this.nextComment = 1000;
    this.updatedComments = 0;
  }

  async listCompletedWorkflowRuns(_workflowId, _branch, page) {
    return this.completedRuns.slice((page - 1) * 100, page * 100);
  }

  async ensureLabel(name) {
    this.labels.add(name);
  }

  async listIssues(_label, page) {
    return this.issues.slice((page - 1) * 100, page * 100);
  }

  async listIssueComments(number, page) {
    return this.comments
      .filter((comment) => comment.issue === number)
      .slice((page - 1) * 100, page * 100);
  }

  async createIssue(issue) {
    const created = { ...issue, number: this.nextIssue, state: "open" };
    this.nextIssue += 1;
    this.issues.push(created);
    return created;
  }

  async createIssueComment(number, body) {
    this.comments.push({
      id: this.nextComment,
      issue: number,
      body,
      user: { login: "github-actions[bot]" },
    });
    this.nextComment += 1;
  }

  async updateIssueComment(commentId, body) {
    const comment = this.comments.find((candidate) => candidate.id === commentId);
    comment.body = body;
    this.updatedComments += 1;
  }

  async deleteIssueComment(commentId) {
    this.comments.splice(
      this.comments.findIndex((comment) => comment.id === commentId),
      1,
    );
  }

  async updateIssue(number, update) {
    Object.assign(
      this.issues.find((issue) => issue.number === number),
      update,
    );
  }
}

function workflowRun(overrides = {}) {
  return {
    id: 41,
    workflow_id: 7,
    run_number: 12,
    run_attempt: 1,
    conclusion: "failure",
    event: "schedule",
    head_branch: "main",
    head_repository: { full_name: "ymm-oss/fsl" },
    head_sha: "0123456789abcdef0123456789abcdef01234567",
    html_url: "https://github.com/ymm-oss/fsl/actions/runs/41",
    ...overrides,
  };
}

async function reconcile(client, run) {
  return reconcileCacheBudgetAudit({
    client,
    repository: "ymm-oss/fsl",
    defaultBranch: "main",
    workflowRun: run,
  });
}

function occurrenceSummaryBody({
  cursor = "12:1",
  count = "7",
  identities = "41:1",
  extraLines = [],
} = {}) {
  return [
    "<!-- cache-budget-audit-occurrence-summary -->",
    occurrenceMarker(41, 1),
    ...(cursor === null ? [] : [`<!-- cache-budget-audit-cursor:${cursor} -->`]),
    ...extraLines,
    "This rolling summary records coalesced failures and its displayed workflow run.",
    `Observable coalesced failed attempts since this summary was created: ${count}.`,
    `Recent observable coalesced identities: ${identities}.`,
  ].join("\n");
}

function historicalSummaryFixture(id) {
  const fixture = HISTORICAL_SUMMARY_FIXTURES.find((candidate) => candidate.id === id);
  assert.ok(fixture, `missing historical summary fixture '${id}'`);
  return fixture;
}

function clientWithOccurrenceSummary(bodies) {
  return new FakeClient({
    issues: [{ number: 100, state: "open", body: cacheBudgetAuditMarker() }],
    comments: bodies.map((body, index) => ({
      id: index + 1,
      issue: 100,
      body,
      user: { login: "github-actions[bot]" },
    })),
  });
}

async function assertOccurrenceSummaryRejected(
  client,
  expected,
  run = workflowRun({ id: 50, run_number: 13 }),
) {
  const issueBefore = structuredClone(client.issues);
  const commentsBefore = structuredClone(client.comments);

  await assert.rejects(
    reconcile(client, run),
    expected,
  );

  assert.deepEqual(client.issues, issueBefore);
  assert.deepEqual(client.comments, commentsBefore);
  assert.equal(client.labels.size, 0);
}

test("first failure files one canonical issue", async () => {
  const client = new FakeClient();

  const result = await reconcile(client, workflowRun());

  assert.deepEqual(result, { created: 1, updated: 0, closed: 0, failed: true });
  assert.equal(client.issues.length, 1);
  assert.ok(client.issues[0].body.includes(cacheBudgetAuditMarker()));
  assert.deepEqual(client.issues[0].labels, [CACHE_BUDGET_AUDIT_LABEL]);
});

test("the same run is idempotent", async () => {
  const client = new FakeClient();
  const run = workflowRun();

  await reconcile(client, run);
  const result = await reconcile(client, run);

  assert.deepEqual(result, { created: 0, updated: 0, closed: 0, failed: true });
  assert.equal(client.issues.length, 1);
  assert.equal(client.comments.length, 0);
});

test("a distinct later failure comments on the canonical issue", async () => {
  const client = new FakeClient();

  await reconcile(client, workflowRun());
  const result = await reconcile(
    client,
    workflowRun({ id: 42, run_number: 13, head_sha: "abcdef0123456789abcdef0123456789abcdef01" }),
  );

  assert.deepEqual(result, { created: 0, updated: 1, closed: 0, failed: true });
  assert.equal(client.issues.length, 1);
  assert.equal(client.comments.length, 1);
  assert.match(client.comments[0].body, /failure recurred/);
});

test("a later success closes the canonical issue", async () => {
  const client = new FakeClient();

  await reconcile(client, workflowRun());
  const result = await reconcile(
    client,
    workflowRun({ id: 42, run_number: 13, conclusion: "success" }),
  );

  assert.deepEqual(result, { created: 0, updated: 0, closed: 1, failed: false });
  assert.equal(client.issues[0].state, "closed");
  assert.match(client.comments[0].body, /Recovered on/);
});

test("a later failure reopens the canonical issue", async () => {
  const client = new FakeClient();

  await reconcile(client, workflowRun());
  await reconcile(client, workflowRun({ id: 42, run_number: 13, conclusion: "success" }));
  const result = await reconcile(client, workflowRun({ id: 43, run_number: 14 }));

  assert.deepEqual(result, { created: 0, updated: 1, closed: 0, failed: true });
  assert.equal(client.issues.length, 1);
  assert.equal(client.issues[0].state, "open");
  assert.equal(client.comments.length, 2);
});

test("a stale failure event reconciles newer recovered health instead", async () => {
  const client = new FakeClient();

  await reconcile(client, workflowRun());
  client.completedRuns = [
    workflowRun({ id: 42, run_number: 13, conclusion: "success" }),
  ];
  const result = await reconcile(client, workflowRun());

  assert.deepEqual(result, {
    created: 0,
    updated: 0,
    closed: 1,
    failed: false,
    redirectedFromRunId: 41,
    reconciledRunId: 42,
  });
  assert.equal(client.issues.length, 1);
  assert.equal(client.issues[0].state, "closed");
});

test("a marker beyond the first comment page remains idempotent", async () => {
  const run = workflowRun({ id: 42, run_number: 13 });
  const issue = {
    number: 100,
    state: "open",
    body: cacheBudgetAuditMarker(),
  };
  const comments = Array.from({ length: 100 }, (_value, index) => ({
    id: index + 1,
    issue: 100,
    body: `filler ${index}`,
    user: { login: "github-actions[bot]" },
  }));
  comments.push({
    id: 101,
    issue: 100,
    body: occurrenceMarker(run.id, run.run_attempt),
    user: { login: "github-actions[bot]" },
  });
  const client = new FakeClient({ issues: [issue], comments });

  const result = await reconcile(client, run);

  assert.deepEqual(result, { created: 0, updated: 0, closed: 0, failed: true });
  assert.equal(client.comments.length, 101);
  assert.ok(client.comments.some((comment) => comment.id === 101));
});

test("a non-bot occurrence marker cannot suppress the reporter audit trail", async () => {
  const client = new FakeClient();
  await reconcile(client, workflowRun());
  const run = workflowRun({ id: 42, run_number: 13 });
  client.comments.push({
    id: 1,
    issue: 100,
    body: occurrenceMarker(run.id, run.run_attempt),
    user: { login: "human-reviewer" },
  });

  const result = await reconcile(client, run);

  assert.equal(result.updated, 1);
  assert.equal(client.comments.length, 2);
  assert.equal(client.comments[1].user.login, "github-actions[bot]");
});

test("recurrence comments are bounded by a rolling summary", async () => {
  const client = new FakeClient();
  await reconcile(client, workflowRun());

  for (let offset = 1; offset <= MAX_DETAILED_OCCURRENCE_COMMENTS + 8; offset += 1) {
    await reconcile(
      client,
      workflowRun({
        id: 41 + offset,
        run_number: 12 + offset,
        head_sha: `${offset}`.padStart(40, "0"),
      }),
    );
  }

  assert.equal(client.comments.length, MAX_DETAILED_OCCURRENCE_COMMENTS + 1);
  assert.ok(client.updatedComments >= 7);
  assert.match(
    client.comments.at(-1).body,
    /cache-budget-audit-occurrence-summary/,
  );
  assert.ok(
    client.comments.at(-1).body.includes(
      occurrenceMarker(41 + MAX_DETAILED_OCCURRENCE_COMMENTS + 8, 1),
    ),
  );
});

test("recovery flapping cannot grow reporter comments without bound", async () => {
  const client = new FakeClient();
  await reconcile(client, workflowRun());
  let runId = 41;
  let runNumber = 12;

  for (let iteration = 0; iteration < MAX_DETAILED_OCCURRENCE_COMMENTS + 5; iteration += 1) {
    runId += 1;
    runNumber += 1;
    await reconcile(
      client,
      workflowRun({ id: runId, run_number: runNumber, conclusion: "success" }),
    );
    runId += 1;
    runNumber += 1;
    await reconcile(client, workflowRun({ id: runId, run_number: runNumber }));
  }

  assert.equal(client.comments.length, MAX_DETAILED_OCCURRENCE_COMMENTS + 2);
  assert.equal(
    client.comments.filter((comment) =>
      comment.body.includes("cache-budget-audit-recovery-summary"),
    ).length,
    1,
  );
});

test("coalesced intermediate failures retain a count and recent identities", async () => {
  const client = new FakeClient({
    completedRuns: [
      workflowRun({ id: 41, run_number: 12 }),
      workflowRun({ id: 42, run_number: 13 }),
    ],
  });

  await reconcile(client, workflowRun({ id: 43, run_number: 14 }));

  const summary = client.comments.find((comment) =>
    comment.body.includes("cache-budget-audit-occurrence-summary"),
  );
  assert.match(summary.body, /Observable coalesced failed attempts since this summary was created: 2\./);
  assert.match(summary.body, /41:1, 42:1/);
  assert.match(client.issues[0].body, /cache-budget-audit-occurrence:43:1/);
});

test("coalescing records only the latest same-run attempt exposed by the list API", async () => {
  const supersededAttempt = workflowRun({ id: 42, run_number: 13, run_attempt: 1 });
  const listedLatestAttempt = workflowRun({ id: 42, run_number: 13, run_attempt: 2 });
  const client = new FakeClient({
    completedRuns: [
      workflowRun({ id: 41, run_number: 12 }),
      listedLatestAttempt,
    ],
  });

  await reconcile(client, workflowRun({ id: 43, run_number: 14 }));

  const summary = client.comments.find((comment) =>
    comment.body.includes("cache-budget-audit-occurrence-summary"),
  );
  assert.match(summary.body, /Observable coalesced failed attempts since this summary was created: 2\./);
  assert.match(summary.body, /41:1, 42:2/);
  assert.doesNotMatch(summary.body, new RegExp(`${supersededAttempt.id}:1`));
});

test("coalescing retains an earlier triggering attempt absent from the list API", async () => {
  const issue = {
    number: 100,
    state: "open",
    body: cacheBudgetAuditMarker(),
  };
  const triggeringAttempt = workflowRun({ id: 42, run_number: 13, run_attempt: 1 });
  const listedLatestAttempt = workflowRun({
    id: 42,
    run_number: 13,
    run_attempt: 2,
    conclusion: "success",
  });
  const client = new FakeClient({
    issues: [issue],
    completedRuns: [listedLatestAttempt],
  });

  await reconcile(client, triggeringAttempt);

  const summary = client.comments.find((comment) =>
    comment.body.includes("cache-budget-audit-occurrence-summary"),
  );
  assert.match(summary.body, /Observable coalesced failed attempts since this summary was created: 1\./);
  assert.match(summary.body, /42:1/);
  assert.doesNotMatch(summary.body, /42:2/);
});

test("replaying an already-summarised run does not PATCH identical content", async () => {
  const client = new FakeClient({
    completedRuns: [workflowRun({ id: 41, run_number: 12 })],
  });
  const run = workflowRun({ id: 42, run_number: 13 });

  await reconcile(client, run);
  const updatesBeforeReplay = client.updatedComments;
  const result = await reconcile(client, run);

  assert.deepEqual(result, { created: 0, updated: 0, closed: 0, failed: true });
  assert.equal(client.updatedComments, updatesBeforeReplay);
});

test("historical summary fixtures have fixed provenance labels and need no Git history", async () => {
  const expectedProvenance = new Map([
    [
      "original-unqualified",
      {
        writerCommit: "cbb00dca5acf99742743a22dd33affa29378d85e",
        writerSha256: "3dc17ad18cd035bb9ef197742e283d47e4d6d00169941255aef837cb673185e9",
        outputSha256: "6ea77d6d6b95de9f6acbfec0a00cfd4d13aae0135d436969aa1de34d2937d649",
      },
    ],
    [
      "interval-qualified",
      {
        writerCommit: "0237fb1fe2b30911ddd5cdf60de1020810e72164",
        writerSha256: "d8618cd5d2bf8d8c99c8b0093d7e55b34fd37240f71ee30cfed32a85bea90833",
        outputSha256: "81b01087c076b23e2a804ff0398d62b84017780d73a07b0d72f113fbe12b1ddf",
      },
    ],
  ]);
  assert.deepEqual(
    HISTORICAL_SUMMARY_FIXTURES.map((fixture) => fixture.id),
    [...expectedProvenance.keys()],
  );
  for (const fixture of HISTORICAL_SUMMARY_FIXTURES) {
    assert.deepEqual(
      {
        writerCommit: fixture.provenance.writerCommit,
        writerSha256: fixture.provenance.writerSha256,
        outputSha256: fixture.provenance.outputSha256,
      },
      expectedProvenance.get(fixture.id),
    );
    assert.equal(
      fixture.provenance.captureCommand,
      `node .github/scripts/capture-cache-budget-audit-summary-fixture.mjs ${fixture.provenance.writerCommit}`,
    );
    assert.equal(
      createHash("sha256").update(fixture.body).digest("hex"),
      fixture.provenance.outputSha256,
    );
  }

  // merge-readiness.yml's automation-contracts checkout has fetch-depth: 0,
  // but this control must also pass in a shallow local clone. Run every
  // imported fixture path from a non-Git directory with no Git executable.
  if (process.env.CACHE_BUDGET_AUDIT_NO_GIT_CHILD === "1") {
    return;
  }
  const noGitDirectory = await mkdtemp(join(tmpdir(), "fsl-cache-audit-no-git-"));
  try {
    const result = spawnSync(process.execPath, ["--test", TEST_PATH], {
      cwd: noGitDirectory,
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: noGitDirectory,
        CACHE_BUDGET_AUDIT_NO_GIT_CHILD: "1",
      },
    });
    assert.equal(result.status, 0, result.stdout + result.stderr);
  } finally {
    await rm(noGitDirectory, { force: true, recursive: true });
  }
});

test("an original unqualified summary migrates to the qualified format", async () => {
  const originalFixture = historicalSummaryFixture("original-unqualified");
  assert.match(originalFixture.body, /Coalesced failed attempts: 1\./);
  const client = clientWithOccurrenceSummary([originalFixture.body]);

  await reconcile(client, workflowRun({ id: 50, run_number: 13 }));

  assert.match(
    client.comments[0].body,
    /Observable coalesced failed attempts since this summary was created: 1\./,
  );
  assert.doesNotMatch(client.comments[0].body, /Coalesced failed attempts: 1\./);
});

test("the direct-parent interval summary migrates to the current format", async () => {
  const parentSummary = historicalSummaryFixture("interval-qualified").body;
  assert.match(
    parentSummary,
    /Observable coalesced failed attempts in this summary interval: 1\./,
  );
  const client = clientWithOccurrenceSummary([parentSummary]);

  await reconcile(client, workflowRun({ id: 50, run_number: 14 }));

  assert.match(
    client.comments[0].body,
    /Observable coalesced failed attempts since this summary was created: 1\./,
  );
  assert.doesNotMatch(
    client.comments[0].body,
    /Observable coalesced failed attempts in this summary interval/,
  );
});

test("a bot quote of an occurrence marker cannot suppress a recurrence", async () => {
  const client = new FakeClient();
  await reconcile(client, workflowRun());
  const run = workflowRun({ id: 42, run_number: 13 });
  client.comments.push({
    id: 1,
    issue: 100,
    body: `Quoted reporter marker: ${occurrenceMarker(run.id, run.run_attempt)}`,
    user: { login: "github-actions[bot]" },
  });

  const result = await reconcile(client, run);

  assert.equal(result.updated, 1);
  assert.equal(
    client.comments.filter((comment) => comment.body.startsWith("<!-- cache-budget-audit-occurrence:")).length,
    1,
  );
});

test("a bot comment with a partial occurrence marker cannot suppress a recurrence", async () => {
  const client = new FakeClient();
  await reconcile(client, workflowRun());
  const run = workflowRun({ id: 42, run_number: 13 });
  client.comments.push({
    id: 1,
    issue: 100,
    body: "<!-- cache-budget-audit-occurrence:42:",
    user: { login: "github-actions[bot]" },
  });

  const result = await reconcile(client, run);

  assert.equal(result.updated, 1);
  assert.equal(
    client.comments.filter((comment) => comment.body.startsWith(occurrenceMarker(42, 1))).length,
    1,
  );
});

test("write-side identity deduplication preserves parser round trips", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ cursor: "12:1", count: "1", identities: "42:1" }),
  ]);
  client.completedRuns = [workflowRun({ id: 42, run_number: 13 })];

  await reconcile(client, workflowRun({ id: 43, run_number: 14, conclusion: "success" }));

  const summary = client.comments[0];
  assert.match(summary.body, /Observable coalesced failed attempts since this summary was created: 1\./);
  assert.match(summary.body, /Recent observable coalesced identities: 42:1\./);
  await reconcile(client, workflowRun({ id: 43, run_number: 14, conclusion: "success" }));
});

test("a safe-boundary count round-trips and overflow rejects before mutation", async () => {
  const boundaryClient = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: `${Number.MAX_SAFE_INTEGER - 1}`, identities: "none" }),
  ]);
  boundaryClient.completedRuns = [workflowRun({ id: 42, run_number: 13 })];

  await reconcile(boundaryClient, workflowRun({ id: 43, run_number: 14 }));
  assert.match(
    boundaryClient.comments[0].body,
    new RegExp(`since this summary was created: ${Number.MAX_SAFE_INTEGER}\\.`),
  );
  await reconcile(boundaryClient, workflowRun({ id: 43, run_number: 14 }));

  const overflowClient = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: `${Number.MAX_SAFE_INTEGER}`, identities: "none" }),
  ]);
  overflowClient.completedRuns = [workflowRun({ id: 42, run_number: 13 })];
  await assertOccurrenceSummaryRejected(
    overflowClient,
    /coalesced failure count would exceed the safe integer limit/,
    workflowRun({ id: 43, run_number: 14 }),
  );
});

test("duplicate future summaries are validated before consolidation", async () => {
  const futureSummary = occurrenceSummaryBody({ cursor: "999:1" });
  const client = clientWithOccurrenceSummary([futureSummary, futureSummary]);

  await assertOccurrenceSummaryRejected(
    client,
    /cursor must not be newer than observable trusted health/,
  );
  assert.equal(client.comments.length, 2);
});

test("duplicate identical summaries self-heal within the comment bound", async () => {
  const client = new FakeClient();
  await reconcile(client, workflowRun());
  for (let offset = 1; offset <= MAX_DETAILED_OCCURRENCE_COMMENTS + 1; offset += 1) {
    await reconcile(client, workflowRun({ id: 41 + offset, run_number: 12 + offset }));
  }
  const summary = client.comments.find((comment) =>
    comment.body.includes("cache-budget-audit-occurrence-summary"),
  );
  client.comments.push({
    id: 9999,
    issue: 100,
    body: summary.body,
    user: { login: "github-actions[bot]" },
  });

  await reconcile(
    client,
    workflowRun({ id: 99, run_number: 99, conclusion: "success" }),
  );

  assert.ok(client.comments.length <= MAX_DETAILED_OCCURRENCE_COMMENTS + 2);
  assert.equal(
    client.comments.filter((comment) =>
      comment.body.includes("cache-budget-audit-occurrence-summary"),
    ).length,
    1,
  );
});

test("a non-numeric coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "not-a-number" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("a negative coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "-1" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("an unsafe coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "9007199254740992" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("an empty coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([occurrenceSummaryBody({ count: "" })]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("an exponent-form coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([occurrenceSummaryBody({ count: "1e2" })]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("a hexadecimal coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([occurrenceSummaryBody({ count: "0x10" })]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("a whitespace-padded coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([occurrenceSummaryBody({ count: " 1" })]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("an exponent-form cursor fails closed", async () => {
  const client = clientWithOccurrenceSummary([occurrenceSummaryBody({ cursor: "1e2:1" })]);

  await assertOccurrenceSummaryRejected(
    client,
    /cursor run number must be a canonical decimal safe integer/,
  );
});

test("an unsafe coalesced identity fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "1", identities: "9007199254740992:1" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced identity run ID must be a canonical decimal safe integer/,
  );
});

test("a zero coalesced identity fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "0", identities: "0:0" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced identity run ID must be a canonical decimal safe integer/,
  );
});

test("a coalesced count smaller than its identities fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "1", identities: "41:1, 42:1" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must cover every recorded identity/,
  );
});

test("duplicate coalesced identities fail closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "2", identities: "41:1, 41:1" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced identities must be a unique bounded run_id:run_attempt list/,
  );
});

test("a coalesced identity with extra colons fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "1", identities: "41:1:2" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced identities must be a bounded run_id:run_attempt list/,
  );
});

test("more than twenty persisted coalesced identities fails closed", async () => {
  const identities = Array.from({ length: 21 }, (_value, index) => `${index + 1}:1`).join(", ");
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "21", identities }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced identities must be a unique bounded run_id:run_attempt list/,
  );
});

test("a missing coalesced identities line fails closed", async () => {
  const body = occurrenceSummaryBody().replace(
    "\nRecent observable coalesced identities: 41:1.",
    "",
  );
  const client = clientWithOccurrenceSummary([body]);

  await assertOccurrenceSummaryRejected(client, /coalesced identities must appear exactly once/);
});

test("a repeated coalesced identities line fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({
      extraLines: ["Recent observable coalesced identities: 41:1."],
    }),
  ]);

  await assertOccurrenceSummaryRejected(client, /coalesced identities must appear exactly once/);
});

test("extra cursor and count lines fail closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({
      extraLines: [
        "<!-- cache-budget-audit-cursor:999:1 -->",
        "Observable coalesced failed attempts since this summary was created: not-a-number.",
      ],
    }),
  ]);

  await assertOccurrenceSummaryRejected(client, /cursor marker must appear exactly once/);
});

test("an extra coalesced count line fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({
      extraLines: [
        "Observable coalesced failed attempts since this summary was created: not-a-number.",
      ],
    }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must appear exactly once/,
  );
});

test("a future cursor fails closed rather than moving backward", async () => {
  const client = clientWithOccurrenceSummary([occurrenceSummaryBody({ cursor: "999:1" })]);

  await assertOccurrenceSummaryRejected(
    client,
    /cursor must not be newer than observable trusted health/,
  );
});

test("whole-summary deletion deliberately starts a new cumulative interval", async () => {
  const client = clientWithOccurrenceSummary([occurrenceSummaryBody({ count: "7" })]);
  client.comments = [];
  client.completedRuns = [workflowRun({ id: 41, run_number: 12 })];

  await reconcile(client, workflowRun({ id: 50, run_number: 13 }));

  const summary = client.comments.find((comment) =>
    comment.body.includes("cache-budget-audit-occurrence-summary"),
  );
  assert.match(summary.body, /Observable coalesced failed attempts since this summary was created: 1\./);
  assert.match(summary.body, /41:1/);
});

test("a valid and malformed duplicate summary fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody(),
    occurrenceSummaryBody({ count: "not-a-number" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a canonical decimal safe integer/,
  );
});

test("a missing cursor with a valid count fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ cursor: null, count: "7" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /cursor marker must appear exactly once/,
  );
});

test("conflicting valid duplicate summaries fail closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "7" }),
    occurrenceSummaryBody({ count: "6" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /duplicate summaries disagree/,
  );
});

test("the executable wrapper reports an untrusted event on stdout and exits zero", async () => {
  const directory = await mkdtemp(join(tmpdir(), "fsl-cache-audit-reporter-"));
  const eventPath = join(directory, "event.json");
  await writeFile(
    eventPath,
    JSON.stringify({
      repository: { default_branch: "main" },
      workflow_run: workflowRun({ event: "pull_request" }),
    }),
  );

  try {
    const result = spawnSync(process.execPath, [REPORTER_PATH], {
      encoding: "utf8",
      env: {
        ...process.env,
        GITHUB_EVENT_PATH: eventPath,
        GITHUB_REPOSITORY: "ymm-oss/fsl",
      },
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, "Skipping untrusted cache-budget audit run.\n");
    assert.equal(result.stderr, "");
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});
