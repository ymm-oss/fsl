#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { execFileSync } from "node:child_process";

const [commit] = process.argv.slice(2);
if (!/^[0-9a-f]{40}$/.test(commit ?? "")) {
  throw new Error("usage: capture-cache-budget-audit-summary-fixture.mjs <40-hex-writer-commit>");
}

class FixtureClient {
  constructor(completedRuns) {
    this.completedRuns = completedRuns;
    this.issues = [];
    this.comments = [];
    this.labels = new Set();
    this.nextIssue = 100;
    this.nextComment = 1000;
  }

  async listCompletedWorkflowRuns(_workflowId, _branch, page) {
    return this.completedRuns.slice((page - 1) * 100, page * 100);
  }

  async ensureLabel(label) {
    this.labels.add(label);
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
    this.comments.find((comment) => comment.id === commentId).body = body;
  }

  async deleteIssueComment(commentId) {
    this.comments.splice(
      this.comments.findIndex((comment) => comment.id === commentId),
      1,
    );
  }

  async updateIssue(number, update) {
    Object.assign(this.issues.find((issue) => issue.number === number), update);
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

// This is deliberately a capture tool, not a test dependency. Regeneration
// requires the original writer commit to be locally available; that deliberate
// prerequisite prevents a squash merge from silently changing test inputs.
const source = execFileSync(
  "git",
  ["show", `${commit}:.github/scripts/report-cache-budget-audit.mjs`],
  { encoding: "utf8" },
);
const { reconcileCacheBudgetAudit } = await import(
  `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`,
);
const client = new FixtureClient([workflowRun({ id: 41, run_number: 12 })]);
await reconcileCacheBudgetAudit({
  client,
  repository: "ymm-oss/fsl",
  defaultBranch: "main",
  workflowRun: workflowRun({ id: 42, run_number: 13 }),
});
const summary = client.comments.find((comment) =>
  comment.body.includes("cache-budget-audit-occurrence-summary"),
);
if (!summary) {
  throw new Error("historical writer did not produce an occurrence summary");
}
process.stdout.write(`${summary.body}\n`);
