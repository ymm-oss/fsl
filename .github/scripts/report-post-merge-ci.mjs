// SPDX-License-Identifier: Apache-2.0

import { appendFile, readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export const POST_MERGE_LABEL = "ci/post-merge";
export const TRUSTED_WORKFLOW_EVENTS = new Set([
  "push",
  "schedule",
  "workflow_dispatch",
]);

const FAILURE_CONCLUSIONS = new Set([
  "action_required",
  "cancelled",
  "failure",
  "stale",
  "startup_failure",
  "timed_out",
]);

class ApiError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

function encodeKey(value) {
  return Buffer.from(String(value), "utf8").toString("base64url");
}

export function issueMarker(workflowId, jobName) {
  return `<!-- post-merge-ci:${workflowId}:${encodeKey(jobName)} -->`;
}

function workflowMarkerPrefix(workflowId) {
  return `<!-- post-merge-ci:${workflowId}:`;
}

export function occurrenceMarker(runId, jobId) {
  return `<!-- post-merge-ci-occurrence:${runId}:${encodeKey(jobId)} -->`;
}

export function isNewerRun(candidate, current) {
  if (candidate.run_number !== current.run_number) {
    return candidate.run_number > current.run_number;
  }
  return (candidate.run_attempt ?? 1) > (current.run_attempt ?? 1);
}

export function isTrustedDefaultBranchWorkflowRun({
  workflowRun,
  repository,
  defaultBranch,
}) {
  return (
    workflowRun.head_repository?.full_name === repository &&
    workflowRun.head_branch === defaultBranch &&
    TRUSTED_WORKFLOW_EVENTS.has(workflowRun.event)
  );
}

function recoveryMarker(runId, runAttempt, issueNumber) {
  return `<!-- post-merge-ci-recovery:${runId}:${runAttempt}:${issueNumber} -->`;
}

export function failingJobs(workflowRun, jobs) {
  const failures = jobs.filter((job) => FAILURE_CONCLUSIONS.has(job.conclusion));
  const leafFailures = failures.filter((job) => job.name !== "product gate");
  if (leafFailures.length > 0) {
    return leafFailures;
  }
  if (failures.length > 0 || workflowRun.conclusion === "success") {
    return failures;
  }

  return [
    {
      id: `workflow-${workflowRun.id}`,
      name: "product gate workflow",
      conclusion: workflowRun.conclusion ?? "unknown",
      html_url: workflowRun.html_url,
      steps: [],
    },
  ];
}

function failedStepNames(job) {
  return (job.steps ?? [])
    .filter((step) => FAILURE_CONCLUSIONS.has(step.conclusion))
    .map((step) => step.name);
}

function shortSha(sha) {
  return sha.slice(0, 12);
}

function pullRequestSummary(pullRequests) {
  const merged = pullRequests.filter((pull) => pull.merged_at);
  if (merged.length === 0) {
    return "No associated merged pull request was returned by GitHub.";
  }

  return merged
    .map((pull) => `[#${pull.number}](${pull.html_url})`)
    .join(", ");
}

export function buildIssueBody({ repository, workflowRun, job, pullRequests }) {
  const marker = issueMarker(workflowRun.workflow_id, job.name);
  const occurrence = occurrenceMarker(workflowRun.id, job.id);
  const steps = failedStepNames(job);
  const commitUrl = `https://github.com/${repository}/commit/${workflowRun.head_sha}`;
  const jobLine = job.html_url
    ? `[${job.name}](${job.html_url})`
    : job.name;

  return [
    marker,
    occurrence,
    "",
    "The trusted post-merge product gate failed on the default branch.",
    "",
    `- Commit: [\`${shortSha(workflowRun.head_sha)}\`](${commitUrl})`,
    `- Workflow run: [${workflowRun.id}](${workflowRun.html_url})`,
    `- Event: \`${workflowRun.event}\``,
    `- Job: ${jobLine}`,
    `- Conclusion: \`${job.conclusion}\``,
    `- Associated PR: ${pullRequestSummary(pullRequests)}`,
    `- Failed steps: ${steps.length > 0 ? steps.map((step) => `\`${step}\``).join(", ") : "No failed step metadata was available."}`,
    "",
    "Logs are intentionally not copied into this issue. Use the workflow and job links above.",
    "Keep this issue open until a later product-gate run proves that this job recovered on `main`.",
  ].join("\n");
}

function occurrenceComment({ workflowRun, job }) {
  return [
    occurrenceMarker(workflowRun.id, job.id),
    `The failure recurred on [\`${shortSha(workflowRun.head_sha)}\`](https://github.com/${workflowRun.repository.full_name}/commit/${workflowRun.head_sha}).`,
    "",
    `- Workflow run: [${workflowRun.id}](${workflowRun.html_url})`,
    `- Event: \`${workflowRun.event}\``,
    `- Job: [${job.name}](${job.html_url ?? workflowRun.html_url})`,
    `- Conclusion: \`${job.conclusion}\``,
  ].join("\n");
}

function recoveryComment({ workflowRun, issue }) {
  return [
    recoveryMarker(
      workflowRun.id,
      workflowRun.run_attempt ?? 1,
      issue.number,
    ),
    `Recovered on [\`${shortSha(workflowRun.head_sha)}\`](https://github.com/${workflowRun.repository.full_name}/commit/${workflowRun.head_sha}).`,
    "",
    `Product-gate run [${workflowRun.id}](${workflowRun.html_url}) completed successfully for this job. Reopen the issue if the failure recurs.`,
  ].join("\n");
}

async function issueContains(client, issue, marker) {
  if ((issue.body ?? "").includes(marker)) {
    return true;
  }
  const comments = await client.listIssueComments(issue.number);
  return comments.some((comment) => (comment.body ?? "").includes(marker));
}

export async function reconcilePostMerge({
  client,
  repository,
  defaultBranch,
  workflowRun,
}) {
  if (
    !isTrustedDefaultBranchWorkflowRun({
      workflowRun,
      repository,
      defaultBranch,
    })
  ) {
    return { created: 0, updated: 0, closed: 0, failures: 0, ignored: true };
  }

  const triggeringRunId = workflowRun.id;
  const latestCompletedRun = await client.latestCompletedWorkflowRun(
    workflowRun.workflow_id,
    workflowRun.head_branch,
  );
  if (
    latestCompletedRun &&
    isTrustedDefaultBranchWorkflowRun({
      workflowRun: latestCompletedRun,
      repository,
      defaultBranch,
    }) &&
    isNewerRun(latestCompletedRun, workflowRun)
  ) {
    workflowRun = latestCompletedRun;
  }
  workflowRun.repository = { full_name: repository };

  await client.ensureLabel(POST_MERGE_LABEL);

  const [jobs, issues, pullRequests] = await Promise.all([
    client.listJobs(workflowRun.id),
    client.listIssues(POST_MERGE_LABEL),
    client.listAssociatedPullRequests(workflowRun.head_sha),
  ]);

  const failures = failingJobs(workflowRun, jobs);
  const failureNames = new Set(failures.map((job) => job.name));
  const matchingIssues = issues.filter(
    (issue) =>
      !issue.pull_request &&
      (issue.body ?? "").includes(workflowMarkerPrefix(workflowRun.workflow_id)),
  );
  let created = 0;
  let updated = 0;
  let closed = 0;

  for (const job of failures) {
    const marker = issueMarker(workflowRun.workflow_id, job.name);
    const occurrence = occurrenceMarker(workflowRun.id, job.id);
    const issue = matchingIssues.find((candidate) =>
      (candidate.body ?? "").includes(marker),
    );

    if (!issue) {
      await client.createIssue({
        title: `[post-merge CI] ${job.name} failed on main`,
        body: buildIssueBody({ repository, workflowRun, job, pullRequests }),
        labels: [POST_MERGE_LABEL],
      });
      created += 1;
      continue;
    }

    let changed = false;
    if (!(await issueContains(client, issue, occurrence))) {
      await client.createIssueComment(
        issue.number,
        occurrenceComment({ workflowRun, job }),
      );
      changed = true;
    }
    if (issue.state !== "open") {
      await client.updateIssue(issue.number, { state: "open" });
      issue.state = "open";
      changed = true;
    }
    if (changed) {
      updated += 1;
    }
  }

  for (const issue of matchingIssues) {
    if (issue.state !== "open") {
      continue;
    }
    const currentJob = jobs.find((job) =>
      (issue.body ?? "").includes(
        issueMarker(workflowRun.workflow_id, job.name),
      ),
    );
    const recovered =
      workflowRun.conclusion === "success" ||
      (currentJob?.conclusion === "success" && !failureNames.has(currentJob.name));

    if (!recovered) {
      continue;
    }

    const marker = recoveryMarker(
      workflowRun.id,
      workflowRun.run_attempt ?? 1,
      issue.number,
    );
    if (!(await issueContains(client, issue, marker))) {
      await client.createIssueComment(
        issue.number,
        recoveryComment({ workflowRun, issue }),
      );
    }
    await client.updateIssue(issue.number, { state: "closed" });
    closed += 1;
  }

  const result = { created, updated, closed, failures: failures.length };
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
        "User-Agent": "fsl-post-merge-ci-reporter",
        "X-GitHub-Api-Version": "2022-11-28",
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });

    if (response.status === 204) {
      return null;
    }

    const text = await response.text();
    if (!response.ok) {
      throw new ApiError(
        response.status,
        `GitHub API ${method} ${path} failed (${response.status}): ${text}`,
      );
    }
    return text ? JSON.parse(text) : null;
  }

  repoPath(suffix) {
    return `/repos/${this.owner}/${this.repo}${suffix}`;
  }

  async listJobs(runId) {
    const response = await this.request(
      "GET",
      this.repoPath(`/actions/runs/${runId}/jobs?filter=latest&per_page=100`),
    );
    return response.jobs;
  }

  async latestCompletedWorkflowRun(workflowId, branch) {
    const responses = await Promise.all(
      [...TRUSTED_WORKFLOW_EVENTS].map((event) =>
        this.request(
          "GET",
          this.repoPath(
            `/actions/workflows/${workflowId}/runs?branch=${encodeURIComponent(branch)}&event=${encodeURIComponent(event)}&status=completed&per_page=100`,
          ),
        ),
      ),
    );
    return responses.flatMap((response) => response.workflow_runs).reduce(
      (latest, run) => (!latest || isNewerRun(run, latest) ? run : latest),
      null,
    );
  }

  async ensureLabel(name) {
    const labelPath = this.repoPath(`/labels/${encodeURIComponent(name)}`);
    try {
      await this.request("GET", labelPath);
    } catch (error) {
      if (!(error instanceof ApiError) || error.status !== 404) {
        throw error;
      }
      await this.request("POST", this.repoPath("/labels"), {
        name,
        color: "b60205",
        description: "Failure detected by the post-merge product gate",
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

  async listAssociatedPullRequests(sha) {
    return this.request(
      "GET",
      this.repoPath(`/commits/${sha}/pulls?per_page=100`),
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
    return this.request(
      "PATCH",
      this.repoPath(`/issues/${number}`),
      update,
    );
  }
}

async function main() {
  const event = JSON.parse(
    await readFile(process.env.GITHUB_EVENT_PATH, "utf8"),
  );
  const workflowRun = event.workflow_run;
  const repository =
    event.repository?.full_name ?? process.env.GITHUB_REPOSITORY;

  if (!isTrustedDefaultBranchWorkflowRun({
    workflowRun,
    repository,
    defaultBranch: event.repository.default_branch,
  })) {
    console.log("Ignoring a workflow run that is not a trusted default-branch run.");
    return;
  }

  const client = new GitHubRestClient({
    token: process.env.GITHUB_TOKEN,
    repository,
  });
  const result = await reconcilePostMerge({
    client,
    repository,
    defaultBranch: event.repository.default_branch,
    workflowRun,
  });
  const summary = `Post-merge CI reconciliation: ${JSON.stringify(result)}`;
  console.log(summary);

  if (process.env.GITHUB_STEP_SUMMARY) {
    await appendFile(process.env.GITHUB_STEP_SUMMARY, `${summary}\n`);
  }
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  await main();
}
