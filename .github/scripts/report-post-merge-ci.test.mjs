// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  GitHubRestClient,
  isTrustedDefaultBranchWorkflowRun,
  issueMarker,
  reconcilePostMerge as reconcilePostMergeImplementation,
  TRUSTED_WORKFLOW_EVENTS,
} from "./report-post-merge-ci.mjs";

const REPOSITORY = "ymm-oss/fsl";
const DEFAULT_BRANCH = "main";
const REPORTER_WORKFLOW = fileURLToPath(
  new URL("../workflows/post-merge-ci-reporter.yml", import.meta.url),
);

function reconcilePostMerge(options) {
  return reconcilePostMergeImplementation({
    defaultBranch: DEFAULT_BRANCH,
    ...options,
  });
}

class FakeClient {
  constructor({
    jobs = [],
    issues = [],
    comments = [],
    pulls = [],
    latestRun = null,
    jobsByRun = new Map(),
  } = {}) {
    this.jobs = jobs;
    this.issues = issues;
    this.comments = comments;
    this.pulls = pulls;
    this.latestRun = latestRun;
    this.jobsByRun = jobsByRun;
    this.labels = new Set();
    this.nextIssue = 100;
  }

  async ensureLabel(name) {
    this.labels.add(name);
  }

  async listJobs(runId) {
    return this.jobsByRun.get(runId) ?? this.jobs;
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
    head_repository: { full_name: REPOSITORY },
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
  assert.deepEqual(client.labels, new Set(["ci/post-merge"]));
  assert.equal(client.issues.length, 1);
  assert.match(client.issues[0].title, /windows-latest/);
  assert.doesNotMatch(client.issues[0].title, /^.*product gate.*$/);
  assert.match(client.issues[0].body, /Test pinned native solver/);
  assert.match(client.issues[0].body, /#247/);
  assert.doesNotMatch(client.issues[0].body, /log output/i);
});

/**
 * The `if:` expression of the **`reconcile` job specifically**, normalised to one
 * line the way a YAML folded scalar joins it.
 *
 * Three revisions of this control were bypassed, each teaching the same lesson.
 *
 * First it regex-scanned the whole file, so a commented-out predicate could
 * counterfeit a deleted one. Then it compared the SET of event-name tokens,
 * which is not the same thing as comparing the expression: review passed
 * `event == 'schedule' && false ||` (set unchanged, schedules rejected), and a
 * blank line followed by `&& ... != 'schedule'` (helper stopped at the blank
 * line, suffix ignored). It also located the first `if:` in the file, so a decoy
 * job carrying the approved expression let the real one drop `schedule`.
 *
 * All three recreated #847 with the control green and `actionlint` clean. So this
 * pins the whole expression by equality, anchored at `jobs.reconcile`, and reads
 * to the end of the block rather than to the first blank line.
 */
function reconcileIfExpression(workflow) {
  const lines = workflow.split("\n");

  const jobsAt = lines.findIndex((line) => /^jobs:\s*$/.test(line));
  assert.notEqual(jobsAt, -1, "the workflow must declare `jobs:`");
  const reconcileAt = lines.findIndex(
    (line, i) => i > jobsAt && /^\s{2}reconcile:\s*$/.test(line),
  );
  assert.notEqual(reconcileAt, -1, "the reporting job must be named `reconcile`");

  // The job's keys are indented further than its own name; anything at or below
  // that column starts a sibling job.
  const jobIndent = 2;
  let ifAt = -1;
  for (let i = reconcileAt + 1; i < lines.length; i += 1) {
    const line = lines[i];
    if (line.trim() === "") {
      continue;
    }
    const indent = line.length - line.trimStart().length;
    if (indent <= jobIndent) {
      break;
    }
    if (/^\s*if:\s*>-?\s*$/.test(line)) {
      ifAt = i;
      break;
    }
  }
  assert.notEqual(ifAt, -1, "`jobs.reconcile` must carry an `if:` block scalar");

  const keyIndent = lines[ifAt].length - lines[ifAt].trimStart().length;
  const body = [];
  for (let i = ifAt + 1; i < lines.length; i += 1) {
    const line = lines[i];
    // A blank line inside a block scalar does NOT end it. Stopping at one is how
    // a suffix after a blank line went unread.
    if (line.trim() === "") {
      continue;
    }
    const indent = line.length - line.trimStart().length;
    if (indent <= keyIndent) {
      break;
    }
    body.push(line.trim());
  }
  assert.ok(body.length > 0, "the `if:` block must not be empty");
  return body.join(" ").replace(/\s+/g, " ").trim();
}

/**
 * The exact expression `jobs.reconcile` must carry, built from
 * `TRUSTED_WORKFLOW_EVENTS` so the YAML and the JavaScript cannot drift apart.
 */
function expectedReconcileCondition() {
  const events = [...TRUSTED_WORKFLOW_EVENTS]
    .map((event) => `github.event.workflow_run.event == '${event}'`)
    .join(" || ");
  return [
    "github.event.workflow_run.head_repository.full_name == github.repository &&",
    "github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
    `(${events})`,
  ].join(" ");
}

test("the reconcile job's if: expression is exactly the trusted condition", async () => {
  const workflow = await readFile(REPORTER_WORKFLOW, "utf8");
  // Equality of the whole expression, not of the event tokens inside it. A set
  // comparison cannot see `&& false`, a negating suffix, or a decoy job.
  assert.equal(reconcileIfExpression(workflow), expectedReconcileCondition());
});

test("a commented-out event cannot counterfeit the workflow gate", () => {
  const counterfeit = [
    "# github.event.workflow_run.event == 'schedule'",
    "jobs:",
    "  reconcile:",
    "    if: >-",
    "      github.event.workflow_run.head_repository.full_name == github.repository &&",
    "      github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
    "      (github.event.workflow_run.event == 'push' ||",
    "      github.event.workflow_run.event == 'workflow_dispatch')",
    "    runs-on: ubuntu-latest",
  ].join("\n");
  assert.notEqual(reconcileIfExpression(counterfeit), expectedReconcileCondition());
});

test("a negated event inside the disjunction is rejected", () => {
  // `event == 'schedule' && false ||` keeps the token set intact and rejects
  // schedules. Review passed this through the previous control.
  const negated = [
    "jobs:",
    "  reconcile:",
    "    if: >-",
    "      github.event.workflow_run.head_repository.full_name == github.repository &&",
    "      github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
    "      (github.event.workflow_run.event == 'push' ||",
    "      github.event.workflow_run.event == 'schedule' && false ||",
    "      github.event.workflow_run.event == 'workflow_dispatch')",
    "    runs-on: ubuntu-latest",
  ].join("\n");
  assert.notEqual(reconcileIfExpression(negated), expectedReconcileCondition());
});

test("a suffix after a blank line inside the scalar is not ignored", () => {
  // YAML keeps this inside the block scalar; the earlier helper stopped reading
  // at the blank line and never saw the negation.
  const suffixed = [
    "jobs:",
    "  reconcile:",
    "    if: >-",
    "      github.event.workflow_run.head_repository.full_name == github.repository &&",
    "      github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
    "      (github.event.workflow_run.event == 'push' ||",
    "      github.event.workflow_run.event == 'schedule' ||",
    "      github.event.workflow_run.event == 'workflow_dispatch')",
    "",
    "      && github.event.workflow_run.event != 'schedule'",
    "    runs-on: ubuntu-latest",
  ].join("\n");
  assert.notEqual(reconcileIfExpression(suffixed), expectedReconcileCondition());
});

test("a decoy job carrying the approved expression is not read instead", () => {
  // The earlier helper took the FIRST `if:` in the file.
  const decoyed = [
    "jobs:",
    "  decoy:",
    "    if: >-",
    "      github.event.workflow_run.head_repository.full_name == github.repository &&",
    "      github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
    "      (github.event.workflow_run.event == 'push' ||",
    "      github.event.workflow_run.event == 'schedule' ||",
    "      github.event.workflow_run.event == 'workflow_dispatch')",
    "    runs-on: ubuntu-latest",
    "  reconcile:",
    "    if: >-",
    "      github.event.workflow_run.head_repository.full_name == github.repository &&",
    "      github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
    "      (github.event.workflow_run.event == 'push' ||",
    "      github.event.workflow_run.event == 'workflow_dispatch')",
    "    runs-on: ubuntu-latest",
  ].join("\n");
  assert.notEqual(reconcileIfExpression(decoyed), expectedReconcileCondition());
});

test("a condition folded across lines is still recognised", () => {
  const folded = [
    "jobs:",
    "  reconcile:",
    "    if: >-",
    "      github.event.workflow_run.head_repository.full_name == github.repository &&",
    "      github.event.workflow_run.head_branch == github.event.repository.default_branch &&",
    "      (github.event.workflow_run.event == 'push' ||",
    "      github.event.workflow_run.event",
    "      == 'schedule' ||",
    "      github.event.workflow_run.event == 'workflow_dispatch')",
    "    runs-on: ubuntu-latest",
  ].join("\n");
  assert.equal(reconcileIfExpression(folded), expectedReconcileCondition());
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
  assert.ok(client.comments[0].body.includes("<!-- post-merge-ci-occurrence:42:OTE -->"));
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
      head_repository: { full_name: REPOSITORY },
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

test("reports a trusted scheduled failure in the canonical post-merge issue", async () => {
  const client = new FakeClient({ jobs: [failedWindowsJob()] });
  const pushedFailure = workflowRun();
  await reconcilePostMerge({
    client,
    repository: REPOSITORY,
    workflowRun: pushedFailure,
  });

  const scheduledFailure = workflowRun({
    id: 42,
    run_number: 13,
    event: "schedule",
    head_sha: "1123456789abcdef0123456789abcdef01234567",
  });
  client.jobs = [failedWindowsJob({ id: 91 })];
  const result = await reconcilePostMerge({
    client,
    repository: REPOSITORY,
    workflowRun: scheduledFailure,
  });

  assert.equal(
    isTrustedDefaultBranchWorkflowRun({
      workflowRun: scheduledFailure,
      repository: REPOSITORY,
      defaultBranch: DEFAULT_BRANCH,
    }),
    true,
  );
  assert.deepEqual(result, {
    created: 0,
    updated: 1,
    closed: 0,
    failures: 1,
  });
  assert.equal(client.issues.length, 1);
  assert.ok(
    client.issues[0].body.includes(
      issueMarker(scheduledFailure.workflow_id, failedWindowsJob().name),
    ),
  );
  assert.match(client.comments[0].body, /- Event: `schedule`/);
});

test("does not let an older successful push close a newer scheduled failure", async () => {
  const olderSuccessfulPush = workflowRun({
    id: 49,
    run_number: 12,
    conclusion: "success",
  });
  const scheduledFailure = workflowRun({
    id: 50,
    run_number: 13,
    event: "schedule",
    html_url: "https://github.com/ymm-oss/fsl/actions/runs/50",
  });
  const client = new FakeClient({
    latestRun: olderSuccessfulPush,
    jobsByRun: new Map([
      [olderSuccessfulPush.id, [failedWindowsJob({ conclusion: "success" })]],
      [scheduledFailure.id, [failedWindowsJob()]],
    ]),
  });

  const result = await reconcilePostMerge({
    client,
    repository: REPOSITORY,
    workflowRun: scheduledFailure,
  });

  assert.deepEqual(result, {
    created: 1,
    updated: 0,
    closed: 0,
    failures: 1,
  });
  assert.equal(client.issues[0].state, "open");
  assert.match(client.issues[0].body, /actions\/runs\/50/);
});

test("queries each trusted event feed and selects the newer scheduled run", async (t) => {
  const olderSuccessfulPush = workflowRun({
    id: 49,
    run_number: 12,
    conclusion: "success",
  });
  const newerScheduledFailure = workflowRun({
    id: 50,
    run_number: 13,
    event: "schedule",
  });
  const runsByEvent = new Map([
    ["push", [olderSuccessfulPush]],
    ["schedule", [newerScheduledFailure]],
    ["workflow_dispatch", []],
  ]);
  const originalFetch = globalThis.fetch;
  const queriedEvents = [];
  globalThis.fetch = async (url) => {
    const requestUrl = new URL(url);
    const event = requestUrl.searchParams.get("event");
    queriedEvents.push(event);
    return new Response(
      JSON.stringify({ workflow_runs: runsByEvent.get(event) }),
      { status: 200 },
    );
  };
  t.after(() => {
    globalThis.fetch = originalFetch;
  });

  const client = new GitHubRestClient({
    token: "test-token",
    repository: REPOSITORY,
  });
  const latest = await client.latestCompletedWorkflowRun(7, DEFAULT_BRANCH);

  assert.equal(latest.id, newerScheduledFailure.id);
  assert.equal(latest.event, "schedule");
  assert.deepEqual(queriedEvents.sort(), [
    "push",
    "schedule",
    "workflow_dispatch",
  ]);
});

test("ignores non-default-branch and foreign-repository workflow runs", async () => {
  for (const ignoredRun of [
    workflowRun({ head_branch: "feature/future" }),
    workflowRun({ head_repository: { full_name: "attacker/fsl" } }),
  ]) {
    const client = new FakeClient({ jobs: [failedWindowsJob()] });
    const result = await reconcilePostMerge({
      client,
      repository: REPOSITORY,
      workflowRun: ignoredRun,
    });

    assert.equal(
      isTrustedDefaultBranchWorkflowRun({
        workflowRun: ignoredRun,
        repository: REPOSITORY,
        defaultBranch: DEFAULT_BRANCH,
      }),
      false,
    );
    assert.deepEqual(result, {
      created: 0,
      updated: 0,
      closed: 0,
      failures: 0,
      ignored: true,
    });
    assert.equal(client.labels.size, 0);
    assert.equal(client.issues.length, 0);
  }
});
