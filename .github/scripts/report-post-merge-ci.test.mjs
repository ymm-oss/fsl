// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import {
  POST_MERGE_LABEL,
  occurrenceMarker,
  reconcilePostMerge,
} from "./report-post-merge-ci.mjs";

class FakeClient {
  constructor({
    jobs = [],
    issues = [],
    comments = [],
    pulls = [],
    latestRun = null,
  } = {}) {
    this.jobs = jobs;
    this.issues = issues;
    this.comments = comments;
    this.pulls = pulls;
    this.latestRun = latestRun;
    this.labels = new Set();
    this.nextIssue = 100;
  }

  async ensureLabel(name) {
    this.labels.add(name);
  }

  async listJobs() {
    return this.jobs;
  }

  async latestCompletedWorkflowRun() {
    return this.latestRun;
  }

  async listIssues() {
    return this.issues;
  }

  async listAssociatedPullRequests() {
    return this.pulls;
  }

  async listIssueComments(number) {
    return this.comments.filter((comment) => comment.issue === number);
  }

  async createIssue(issue) {
    const created = {
      ...issue,
      number: this.nextIssue,
      state: "open",
    };
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
    event: "push",
    head_branch: "main",
    head_sha: "0123456789abcdef0123456789abcdef01234567",
    html_url: "https://github.com/ymm-oss/fsl/actions/runs/41",
    ...overrides,
  };
}

function failedWindowsJob(overrides = {}) {
  return {
    id: 90,
    name: "native Z3 4.16 (windows-latest)",
    conclusion: "failure",
    html_url: "https://github.com/ymm-oss/fsl/actions/jobs/90",
    steps: [
      { name: "Set up job", conclusion: "success" },
      { name: "Test pinned native solver", conclusion: "failure" },
    ],
    ...overrides,
  };
}

test("creates one actionable issue for a failed job", async () => {
  const client = new FakeClient({
    jobs: [
      failedWindowsJob(),
      {
        id: 99,
        name: "product gate",
        conclusion: "failure",
        html_url: "https://github.com/ymm-oss/fsl/actions/jobs/99",
        steps: [{ name: "Require complete product evidence", conclusion: "failure" }],
      },
    ],
    pulls: [
      {
        number: 247,
        html_url: "https://github.com/ymm-oss/fsl/pull/247",
        merged_at: "2026-07-26T00:00:00Z",
      },
    ],
  });

  const result = await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun(),
  });

  assert.deepEqual(result, {
    created: 1,
    updated: 0,
    closed: 0,
    failures: 1,
  });
  assert.deepEqual(client.labels, new Set([POST_MERGE_LABEL]));
  assert.equal(client.issues.length, 1);
  assert.match(client.issues[0].title, /windows-latest/);
  assert.doesNotMatch(client.issues[0].title, /^.*product gate.*$/);
  assert.match(client.issues[0].body, /Test pinned native solver/);
  assert.match(client.issues[0].body, /#247/);
  assert.doesNotMatch(client.issues[0].body, /log output/i);
});

test("deduplicates the same run and comments once for a later recurrence", async () => {
  const client = new FakeClient({ jobs: [failedWindowsJob()] });
  const firstRun = workflowRun();

  await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: firstRun,
  });
  await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: firstRun,
  });

  assert.equal(client.issues.length, 1);
  assert.equal(client.comments.length, 0);

  const laterRun = workflowRun({
    id: 42,
    head_sha: "1123456789abcdef0123456789abcdef01234567",
  });
  client.jobs = [failedWindowsJob({ id: 91 })];
  await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: laterRun,
  });

  assert.equal(client.issues.length, 1);
  assert.equal(client.comments.length, 1);
  assert.ok(client.comments[0].body.includes(occurrenceMarker(42, 91)));
});

test("closes the matching open issue after the job recovers", async () => {
  const client = new FakeClient({ jobs: [failedWindowsJob()] });
  await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun(),
  });

  client.jobs = [
    failedWindowsJob({
      id: 92,
      conclusion: "success",
      steps: [{ name: "Test pinned native solver", conclusion: "success" }],
    }),
  ];
  const result = await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun({
      id: 43,
      conclusion: "success",
      head_sha: "2123456789abcdef0123456789abcdef01234567",
    }),
  });

  assert.equal(result.closed, 1);
  assert.equal(client.issues[0].state, "closed");
  assert.match(client.comments[0].body, /Recovered on/);
});

test("reports a workflow-level failure when no failed job metadata exists", async () => {
  const client = new FakeClient({ jobs: [] });
  const result = await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun({ conclusion: "startup_failure" }),
  });

  assert.equal(result.created, 1);
  assert.equal(result.failures, 1);
  assert.match(client.issues[0].title, /product gate workflow/);
  assert.match(client.issues[0].body, /No failed step metadata/);
});

test("reports the product aggregate when it is the only failed job", async () => {
  const client = new FakeClient({
    jobs: [
      {
        id: 101,
        name: "product gate",
        conclusion: "failure",
        html_url: "https://github.com/ymm-oss/fsl/actions/jobs/101",
        steps: [
          {
            name: "Require complete product evidence",
            conclusion: "failure",
          },
        ],
      },
    ],
  });
  const result = await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun(),
  });

  assert.equal(result.created, 1);
  assert.equal(result.failures, 1);
  assert.match(client.issues[0].title, /\bproduct gate\b/);
  assert.match(client.issues[0].body, /Require complete product evidence/);
});

test("redirects an out-of-order event to the latest completed product gate", async () => {
  const client = new FakeClient({
    latestRun: {
      id: 50,
      workflow_id: 7,
      run_number: 13,
      run_attempt: 1,
      conclusion: "failure",
      event: "push",
      head_branch: "main",
      head_sha: "4123456789abcdef0123456789abcdef01234567",
      html_url: "https://github.com/ymm-oss/fsl/actions/runs/50",
    },
    jobs: [failedWindowsJob({ id: 95 })],
  });

  const result = await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun({ conclusion: "success" }),
  });

  assert.deepEqual(result, {
    created: 1,
    updated: 0,
    closed: 0,
    failures: 1,
    redirectedFromRunId: 41,
    reconciledRunId: 50,
  });
  assert.match(client.issues[0].body, /actions\/runs\/50/);
  assert.match(client.issues[0].body, /`4123456789ab`/);
});

test("reopens the canonical issue when a recovered job fails again", async () => {
  const client = new FakeClient({ jobs: [failedWindowsJob()] });
  await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun(),
  });

  client.jobs = [
    failedWindowsJob({
      id: 92,
      conclusion: "success",
      steps: [{ name: "Test pinned native solver", conclusion: "success" }],
    }),
  ];
  await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun({
      id: 43,
      run_number: 13,
      conclusion: "success",
      head_sha: "2123456789abcdef0123456789abcdef01234567",
    }),
  });
  assert.equal(client.issues[0].state, "closed");

  client.jobs = [failedWindowsJob({ id: 94 })];
  const result = await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun({
      id: 44,
      run_number: 14,
      head_sha: "3123456789abcdef0123456789abcdef01234567",
    }),
  });

  assert.equal(result.created, 0);
  assert.equal(result.updated, 1);
  assert.equal(client.issues.length, 1);
  assert.equal(client.issues[0].state, "open");
});

test("records recovery separately for repeated attempts of one workflow run", async () => {
  const client = new FakeClient({ jobs: [failedWindowsJob()] });
  await reconcilePostMerge({
    client,
    repository: "ymm-oss/fsl",
    workflowRun: workflowRun(),
  });

  for (const [runAttempt, conclusion, jobId] of [
    [2, "success", 96],
    [3, "failure", 97],
    [4, "success", 98],
  ]) {
    client.jobs = [
      failedWindowsJob({
        id: jobId,
        conclusion,
        steps: [
          {
            name: "Test pinned native solver",
            conclusion,
          },
        ],
      }),
    ];
    await reconcilePostMerge({
      client,
      repository: "ymm-oss/fsl",
      workflowRun: workflowRun({
        run_attempt: runAttempt,
        conclusion,
      }),
    });
  }

  const recoveryComments = client.comments.filter((comment) =>
    comment.body.includes("post-merge-ci-recovery"),
  );
  assert.equal(recoveryComments.length, 2);
  assert.notEqual(recoveryComments[0].body, recoveryComments[1].body);
  assert.equal(client.issues[0].state, "closed");
});
