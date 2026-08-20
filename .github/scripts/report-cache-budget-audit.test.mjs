// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import {
  CACHE_BUDGET_AUDIT_LABEL,
  cacheBudgetAuditMarker,
  reconcileCacheBudgetAudit,
} from "./report-cache-budget-audit.mjs";

class FakeClient {
  constructor({ issues = [], comments = [], completedRuns = [] } = {}) {
    this.issues = issues;
    this.comments = comments;
    this.completedRuns = completedRuns;
    this.labels = new Set();
    this.nextIssue = 100;
  }

  async listCompletedWorkflowRuns() {
    return this.completedRuns;
  }

  async ensureLabel(name) {
    this.labels.add(name);
  }

  async listIssues() {
    return this.issues;
  }

  async listIssueComments(number) {
    return this.comments.filter((comment) => comment.issue === number);
  }

  async createIssue(issue) {
    const created = { ...issue, number: this.nextIssue, state: "open" };
    this.nextIssue += 1;
    this.issues.push(created);
    return created;
  }

  async createIssueComment(number, body) {
    this.comments.push({ issue: number, body });
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
