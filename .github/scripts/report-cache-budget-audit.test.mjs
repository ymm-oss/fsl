// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import {
  CACHE_BUDGET_AUDIT_LABEL,
  MAX_DETAILED_OCCURRENCE_COMMENTS,
  cacheBudgetAuditMarker,
  occurrenceMarker,
  reconcileCacheBudgetAudit,
} from "./report-cache-budget-audit.mjs";

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
} = {}) {
  return [
    "<!-- cache-budget-audit-occurrence-summary -->",
    occurrenceMarker(41, 1),
    ...(cursor === null ? [] : [`<!-- cache-budget-audit-cursor:${cursor} -->`]),
    "Detailed recurrence comments are capped at 20; this rolling summary records the latest recurrence.",
    `Coalesced failed attempts: ${count}.`,
    `Recent coalesced identities: ${identities}.`,
  ].join("\n");
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

async function assertOccurrenceSummaryRejected(client, expected) {
  const issueBefore = structuredClone(client.issues);
  const commentsBefore = structuredClone(client.comments);

  await assert.rejects(
    reconcile(client, workflowRun({ id: 50, run_number: 13 })),
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
  assert.equal(client.comments.length, MAX_DETAILED_OCCURRENCE_COMMENTS + 2);
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
  assert.match(summary.body, /Coalesced failed attempts: 2\./);
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
  assert.match(summary.body, /Coalesced failed attempts: 2\./);
  assert.match(summary.body, /41:1, 42:2/);
  assert.doesNotMatch(summary.body, new RegExp(`${supersededAttempt.id}:1`));
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
    /coalesced failure count must be a non-negative safe integer/,
  );
});

test("a negative coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "-1" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a non-negative safe integer/,
  );
});

test("an unsafe coalesced count fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody({ count: "9007199254740992" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a non-negative safe integer/,
  );
});

test("a valid and malformed duplicate summary fails closed", async () => {
  const client = clientWithOccurrenceSummary([
    occurrenceSummaryBody(),
    occurrenceSummaryBody({ count: "not-a-number" }),
  ]);

  await assertOccurrenceSummaryRejected(
    client,
    /coalesced failure count must be a non-negative safe integer/,
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
