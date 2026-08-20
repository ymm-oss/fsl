// SPDX-License-Identifier: Apache-2.0

import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export const CACHE_BUDGET_AUDIT_LABEL = "ci/cache-budget-audit";
export const REPORTER_BOT_LOGIN = "github-actions[bot]";
export const MAX_DETAILED_OCCURRENCE_COMMENTS = 20;
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

function occurrenceSummaryMarker() {
  return "<!-- cache-budget-audit-occurrence-summary -->";
}

function recoverySummaryMarker() {
  return "<!-- cache-budget-audit-recovery-summary -->";
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

function occurrenceSummaryComment({ workflowRun }) {
  return [
    occurrenceSummaryMarker(),
    occurrenceMarker(workflowRun.id, workflowRun.run_attempt),
    "Detailed recurrence comments are capped at 20; this rolling summary records the latest recurrence.",
    "",
    `- Workflow run: [${workflowRun.id}](${workflowRun.html_url})`,
    `- Trigger: \`${workflowRun.event}\``,
    `- Conclusion: \`${workflowRun.conclusion ?? "unknown"}\``,
  ].join("\n");
}

function recoveryComment({ workflowRun, issue }) {
  return [
    recoverySummaryMarker(),
    recoveryMarker(workflowRun.id, workflowRun.run_attempt, issue.number),
    `Recovered on [\`${shortSha(workflowRun.head_sha)}\`](https://github.com/${workflowRun.repository.full_name}/commit/${workflowRun.head_sha}).`,
    "",
    `Cache-budget audit run [${workflowRun.id}](${workflowRun.html_url}) completed successfully. Reopen the issue if the audit fails again.`,
  ].join("\n");
}

async function listAllPages(fetchPage) {
  const entries = [];
  for (let page = 1; ; page += 1) {
    const pageEntries = await fetchPage(page);
    entries.push(...pageEntries);
    if (pageEntries.length < 100) {
      return entries;
    }
  }
}

function isReporterComment(comment) {
  return comment.user?.login === REPORTER_BOT_LOGIN;
}

async function reporterComments(client, issue) {
  // Unbounded pagination is acceptable for this scheduled operational path,
  // rather than a merge hot path: an old marker must not silently defeat
  // idempotency after 100 comments.
  const comments = await listAllPages((page) =>
    client.listIssueComments(issue.number, page),
  );
  return comments.filter(isReporterComment);
}

async function issueContains(client, issue, marker) {
  if ((issue.body ?? "").includes(marker)) {
    return true;
  }
  const comments = await reporterComments(client, issue);
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
  const completedRuns = await listAllPages((page) =>
    client.listCompletedWorkflowRuns(workflowRun.workflow_id, defaultBranch, page),
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
  const issues = await listAllPages((page) =>
    client.listIssues(CACHE_BUDGET_AUDIT_LABEL, page),
  );
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
        const comments = await reporterComments(client, issue);
        const detailedOccurrences = comments.filter((comment) =>
          (comment.body ?? "").includes("<!-- cache-budget-audit-occurrence:"),
        );
        if (detailedOccurrences.length < MAX_DETAILED_OCCURRENCE_COMMENTS) {
          await client.createIssueComment(
            issue.number,
            occurrenceComment({ workflowRun }),
          );
        } else {
          const summary = comments.find((comment) =>
            (comment.body ?? "").includes(occurrenceSummaryMarker()),
          );
          if (summary) {
            await client.updateIssueComment(
              summary.id,
              occurrenceSummaryComment({ workflowRun }),
            );
          } else {
            await client.createIssueComment(
              issue.number,
              occurrenceSummaryComment({ workflowRun }),
            );
          }
        }
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
      const comments = await reporterComments(client, issue);
      const summary = comments.find((comment) =>
        (comment.body ?? "").includes(recoverySummaryMarker()),
      );
      if (summary) {
        await client.updateIssueComment(
          summary.id,
          recoveryComment({ workflowRun, issue }),
        );
      } else {
        await client.createIssueComment(
          issue.number,
          recoveryComment({ workflowRun, issue }),
        );
      }
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

  async listCompletedWorkflowRuns(workflowId, branch, page) {
    const response = await this.request(
      "GET",
      this.repoPath(
        `/actions/workflows/${workflowId}/runs?branch=${encodeURIComponent(branch)}&status=completed&per_page=100&page=${page}`,
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

  async listIssues(label, page) {
    return this.request(
      "GET",
      this.repoPath(
        `/issues?state=all&labels=${encodeURIComponent(label)}&per_page=100&page=${page}`,
      ),
    );
  }

  async listIssueComments(number, page) {
    return this.request(
      "GET",
      this.repoPath(`/issues/${number}/comments?per_page=100&page=${page}`),
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

  async updateIssueComment(commentId, body) {
    return this.request(
      "PATCH",
      this.repoPath(`/issues/comments/${commentId}`),
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
