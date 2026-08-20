// SPDX-License-Identifier: Apache-2.0

import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export const CACHE_BUDGET_AUDIT_LABEL = "ci/cache-budget-audit";
export const TRUSTED_AUDIT_EVENTS = new Set([
  "push",
  "schedule",
  "workflow_dispatch",
]);

function shortSha(sha) {
  return sha?.slice(0, 12) ?? "unknown";
}

export function cacheBudgetAuditMarker() {
  return "<!-- cache-budget-audit -->";
}

export function occurrenceMarker(runId, runAttempt) {
  return `<!-- cache-budget-audit-occurrence:${runId}:${runAttempt ?? 1} -->`;
}

function recoveryMarker(runId, runAttempt, issueNumber) {
  return `<!-- cache-budget-audit-recovery:${runId}:${runAttempt ?? 1}:${issueNumber} -->`;
}

export function isNewerRun(candidate, current) {
  if (candidate.run_number !== current.run_number) {
    return candidate.run_number > current.run_number;
  }
  return (candidate.run_attempt ?? 1) > (current.run_attempt ?? 1);
}

export function isTrustedAuditRun(workflowRun, defaultBranch, repository) {
  return (
    TRUSTED_AUDIT_EVENTS.has(workflowRun.event) &&
    workflowRun.head_branch === defaultBranch &&
    workflowRun.head_repository?.full_name === repository
  );
}

export function latestTrustedAuditRun(runs, defaultBranch, repository) {
  return runs
    .filter((run) => isTrustedAuditRun(run, defaultBranch, repository))
    .reduce(
      (latest, run) => (!latest || isNewerRun(run, latest) ? run : latest),
      null,
    );
}

function issueBody({ repository, workflowRun }) {
  const occurrence = occurrenceMarker(workflowRun.id, workflowRun.run_attempt);
  const commitUrl = `https://github.com/${repository}/commit/${workflowRun.head_sha}`;

  return [
    cacheBudgetAuditMarker(),
    occurrence,
    "",
    "The trusted Actions cache-budget audit failed on the default branch.",
    "",
    `- Commit: [\`${shortSha(workflowRun.head_sha)}\`](${commitUrl})`,
    `- Workflow run: [${workflowRun.id}](${workflowRun.html_url})`,
    `- Trigger: \`${workflowRun.event}\``,
    `- Conclusion: \`${workflowRun.conclusion ?? "unknown"}\``,
    "",
    "Logs are intentionally not copied into this issue. Use the workflow link above.",
    "Keep this issue open until a later trusted cache-budget audit succeeds on the default branch.",
  ].join("\n");
}

function occurrenceComment({ workflowRun }) {
  return [
    occurrenceMarker(workflowRun.id, workflowRun.run_attempt),
    `The failure recurred on [\`${shortSha(workflowRun.head_sha)}\`](https://github.com/${workflowRun.repository.full_name}/commit/${workflowRun.head_sha}).`,
    "",
    `- Workflow run: [${workflowRun.id}](${workflowRun.html_url})`,
    `- Trigger: \`${workflowRun.event}\``,
    `- Conclusion: \`${workflowRun.conclusion ?? "unknown"}\``,
  ].join("\n");
}

function recoveryComment({ workflowRun, issue }) {
  return [
    recoveryMarker(workflowRun.id, workflowRun.run_attempt, issue.number),
    `Recovered on [\`${shortSha(workflowRun.head_sha)}\`](https://github.com/${workflowRun.repository.full_name}/commit/${workflowRun.head_sha}).`,
    "",
    `Cache-budget audit run [${workflowRun.id}](${workflowRun.html_url}) completed successfully. Reopen the issue if the audit fails again.`,
  ].join("\n");
}

async function issueContains(client, issue, marker) {
  if ((issue.body ?? "").includes(marker)) {
    return true;
  }
  const comments = await client.listIssueComments(issue.number);
  return comments.some((comment) => (comment.body ?? "").includes(marker));
}

/**
 * Reconcile one canonical cache-budget issue against the newest completed
 * trusted audit.  The trigger is deliberately not treated as authoritative:
 * a delayed workflow_run event is redirected to newer completed health.
 */
export async function reconcileCacheBudgetAudit({
  client,
  repository,
  defaultBranch,
  workflowRun,
}) {
  if (!isTrustedAuditRun(workflowRun, defaultBranch, repository)) {
    throw new Error("refusing to report an untrusted cache-budget audit run");
  }

  const triggeringRunId = workflowRun.id;
  const completedRuns = await client.listCompletedWorkflowRuns(
    workflowRun.workflow_id,
    defaultBranch,
  );
  const latest = latestTrustedAuditRun(
    [...completedRuns, workflowRun],
    defaultBranch,
    repository,
  );
  if (latest) {
    workflowRun = latest;
  }
  workflowRun.repository = { full_name: repository };

  await client.ensureLabel(CACHE_BUDGET_AUDIT_LABEL);
  const issues = await client.listIssues(CACHE_BUDGET_AUDIT_LABEL);
  const issue = issues.find(
    (candidate) =>
      !candidate.pull_request &&
      (candidate.body ?? "").includes(cacheBudgetAuditMarker()),
  );
  const failed = workflowRun.conclusion !== "success";
  let created = 0;
  let updated = 0;
  let closed = 0;

  if (failed) {
    if (!issue) {
      await client.createIssue({
        title: "[cache budget audit] Actions cache budget audit failed on main",
        body: issueBody({ repository, workflowRun }),
        labels: [CACHE_BUDGET_AUDIT_LABEL],
      });
      created = 1;
    } else {
      let changed = false;
      const occurrence = occurrenceMarker(workflowRun.id, workflowRun.run_attempt);
      if (!(await issueContains(client, issue, occurrence))) {
        await client.createIssueComment(issue.number, occurrenceComment({ workflowRun }));
        changed = true;
      }
      if (issue.state !== "open") {
        await client.updateIssue(issue.number, { state: "open" });
        issue.state = "open";
        changed = true;
      }
      if (changed) {
        updated = 1;
      }
    }
  } else if (issue?.state === "open") {
    const recovery = recoveryMarker(
      workflowRun.id,
      workflowRun.run_attempt,
      issue.number,
    );
    if (!(await issueContains(client, issue, recovery))) {
      await client.createIssueComment(
        issue.number,
        recoveryComment({ workflowRun, issue }),
      );
    }
    await client.updateIssue(issue.number, { state: "closed" });
    closed = 1;
  }

  const result = { created, updated, closed, failed };
  if (workflowRun.id !== triggeringRunId) {
    result.redirectedFromRunId = triggeringRunId;
    result.reconciledRunId = workflowRun.id;
  }
  return result;
}

export class GitHubRestClient {
  constructor({ token, repository }) {
    const [owner, repo] = repository.split("/");
    if (!owner || !repo) {
      throw new Error(`invalid GITHUB_REPOSITORY: ${repository}`);
    }
    this.token = token;
    this.owner = owner;
    this.repo = repo;
  }

  async request(method, path, body) {
    const response = await fetch(`https://api.github.com${path}`, {
      method,
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${this.token}`,
        "Content-Type": "application/json",
        "User-Agent": "fsl-cache-budget-audit-reporter",
        "X-GitHub-Api-Version": "2022-11-28",
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (response.status === 204) {
      return null;
    }
    const text = await response.text();
    if (!response.ok) {
      const error = new Error(
        `GitHub API ${method} ${path} failed (${response.status}): ${text}`,
      );
      error.status = response.status;
      throw error;
    }
    return text ? JSON.parse(text) : null;
  }

  repoPath(suffix) {
    return `/repos/${this.owner}/${this.repo}${suffix}`;
  }

  async listCompletedWorkflowRuns(workflowId, branch) {
    const response = await this.request(
      "GET",
      this.repoPath(
        `/actions/workflows/${workflowId}/runs?branch=${encodeURIComponent(branch)}&status=completed&per_page=100`,
      ),
    );
    return response.workflow_runs;
  }

  async ensureLabel(name) {
    const labelPath = this.repoPath(`/labels/${encodeURIComponent(name)}`);
    try {
      await this.request("GET", labelPath);
    } catch (error) {
      if (error.status !== 404) {
        throw error;
      }
      await this.request("POST", this.repoPath("/labels"), {
        name,
        color: "b60205",
        description: "Failure detected by the Actions cache budget audit",
      });
    }
  }

  async listIssues(label) {
    return this.request(
      "GET",
      this.repoPath(
        `/issues?state=all&labels=${encodeURIComponent(label)}&per_page=100`,
      ),
    );
  }

  async listIssueComments(number) {
    return this.request(
      "GET",
      this.repoPath(`/issues/${number}/comments?per_page=100`),
    );
  }

  async createIssue(issue) {
    return this.request("POST", this.repoPath("/issues"), issue);
  }

  async createIssueComment(number, body) {
    return this.request(
      "POST",
      this.repoPath(`/issues/${number}/comments`),
      { body },
    );
  }

  async updateIssue(number, update) {
    return this.request("PATCH", this.repoPath(`/issues/${number}`), update);
  }
}

async function main() {
  const event = JSON.parse(
    await readFile(process.env.GITHUB_EVENT_PATH, "utf8"),
  );
  const workflowRun = event.workflow_run;
  const repository = process.env.GITHUB_REPOSITORY;
  const defaultBranch = event.repository?.default_branch;
  if (!repository || !defaultBranch || !workflowRun) {
    throw new Error("workflow_run event, repository, and default branch are required");
  }
  if (!isTrustedAuditRun(workflowRun, defaultBranch, repository)) {
    console.log("Skipping untrusted cache-budget audit run.");
    return;
  }
  const result = await reconcileCacheBudgetAudit({
    client: new GitHubRestClient({
      token: process.env.GITHUB_TOKEN,
      repository,
    }),
    repository,
    defaultBranch,
    workflowRun,
  });
  console.log(JSON.stringify(result));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.stack ?? error.message);
    process.exitCode = 1;
  });
}
