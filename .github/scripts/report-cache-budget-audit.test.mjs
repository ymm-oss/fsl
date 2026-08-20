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
  assert.equal(client.updatedComments, 7);
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
