// SPDX-License-Identifier: Apache-2.0

import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export const CACHE_BUDGET_AUDIT_LABEL = "ci/cache-budget-audit";
export const REPORTER_BOT_LOGIN = "github-actions[bot]";
export const MAX_DETAILED_OCCURRENCE_COMMENTS = 20;
export const MAX_RECENT_COALESCED_IDENTITIES = 20;
export const MAX_REPORTER_COMMENTS = MAX_DETAILED_OCCURRENCE_COMMENTS + 2;
export const TRUSTED_AUDIT_EVENTS = new Set([
  "push",
  "schedule",
  "workflow_dispatch",
]);

const CURRENT_COALESCED_COUNT_PREFIX = "Observable coalesced failed attempts since this summary was created";
const CURRENT_COALESCED_IDENTITIES_PREFIX = "Recent observable coalesced identities";
const LEGACY_COALESCED_COUNT_PREFIX = "Coalesced failed attempts";
const LEGACY_COALESCED_IDENTITIES_PREFIX = "Recent coalesced identities";

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

function coalescingCursorMarker(workflowRun) {
  return `<!-- cache-budget-audit-cursor:${workflowRun.run_number}:${workflowRun.run_attempt ?? 1} -->`;
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

function occurrenceSummaryComment({ workflowRun, occurrenceRun = workflowRun, coalesced }) {
  return [
    occurrenceSummaryMarker(),
    occurrenceMarker(occurrenceRun.id, occurrenceRun.run_attempt),
    coalescingCursorMarker(workflowRun),
    "This rolling summary records coalesced failures and its displayed workflow run.",
    `${CURRENT_COALESCED_COUNT_PREFIX}: ${coalesced.count}.`,
    `${CURRENT_COALESCED_IDENTITIES_PREFIX}: ${coalesced.identities.length ? coalesced.identities.join(", ") : "none"}.`,
    "",
    `- Workflow run: [${workflowRun.id}](${workflowRun.html_url})`,
    `- Trigger: \`${workflowRun.event}\``,
    `- Conclusion: \`${workflowRun.conclusion ?? "unknown"}\``,
  ].join("\n");
}

function runIdentity(workflowRun) {
  return `${workflowRun.id}:${workflowRun.run_attempt ?? 1}`;
}

function invalidOccurrenceSummary(message) {
  return new Error(`invalid cache-budget-audit occurrence summary: ${message}`);
}

function singleSummaryMatch(body, expression, field) {
  const matches = [...body.matchAll(expression)];
  if (matches.length !== 1) {
    throw invalidOccurrenceSummary(`${field} must appear exactly once`);
  }
  return matches[0];
}

function canonicalDecimalInteger(value, field, minimum) {
  const expression = minimum === 0 ? /^(?:0|[1-9]\d*)$/ : /^[1-9]\d*$/;
  if (typeof value !== "string" || !expression.test(value)) {
    throw invalidOccurrenceSummary(`${field} must be a canonical decimal safe integer`);
  }
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < minimum || String(number) !== value) {
    throw invalidOccurrenceSummary(`${field} must be a canonical decimal safe integer`);
  }
  return number;
}

function parsedCursor(comment) {
  const match = singleSummaryMatch(
    comment.body ?? "",
    /<!-- cache-budget-audit-cursor:([^: >]+):([^ >]+) -->/g,
    "cursor marker",
  );
  const runNumber = canonicalDecimalInteger(match[1], "cursor run number", 1);
  const runAttempt = canonicalDecimalInteger(match[2], "cursor run attempt", 1);
  return { run_number: runNumber, run_attempt: runAttempt };
}

function parsedIdentity(identity) {
  const [runId, runAttempt, ...rest] = identity.split(":");
  if (rest.length > 0 || runId === undefined || runAttempt === undefined) {
    throw invalidOccurrenceSummary("coalesced identities must be a bounded run_id:run_attempt list");
  }
  canonicalDecimalInteger(runId, "coalesced identity run ID", 1);
  canonicalDecimalInteger(runAttempt, "coalesced identity run attempt", 1);
  return identity;
}

function parsedCoalesced(comment) {
  const body = comment.body ?? "";
  // The immediately preceding writer used the legacy prefixes below. Read them
  // only to migrate existing summaries; remove this branch only in an explicit
  // schema migration after verifying that no legacy summaries remain.
  const countMatch = singleSummaryMatch(
    body,
    /(?:Observable coalesced failed attempts since this summary was created|Coalesced failed attempts): ([^\n]*)\./g,
    "coalesced failure count",
  );
  const identitiesMatch = singleSummaryMatch(
    body,
    /(?:Recent observable coalesced identities|Recent coalesced identities): ([^\n]*)\./g,
    "coalesced identities",
  );
  const count = canonicalDecimalInteger(countMatch[1], "coalesced failure count", 0);
  const identitiesText = identitiesMatch[1];
  const identities = identitiesText === "none" ? [] : identitiesText.split(", ").map(parsedIdentity);
  if (
    identities.length > MAX_RECENT_COALESCED_IDENTITIES ||
    new Set(identities).size !== identities.length
  ) {
    throw invalidOccurrenceSummary("coalesced identities must be a unique bounded run_id:run_attempt list");
  }
  if (count < identities.length) {
    throw invalidOccurrenceSummary("coalesced failure count must cover every recorded identity");
  }
  return { count, identities };
}

function parsedOccurrenceSummaryState(comment) {
  return { cursor: parsedCursor(comment), coalesced: parsedCoalesced(comment) };
}

function sameOccurrenceSummaryState(left, right) {
  return (
    left.cursor.run_number === right.cursor.run_number &&
    left.cursor.run_attempt === right.cursor.run_attempt &&
    left.coalesced.count === right.coalesced.count &&
    left.coalesced.identities.length === right.coalesced.identities.length &&
    left.coalesced.identities.every(
      (identity, index) => identity === right.coalesced.identities[index],
    )
  );
}

function recordedCursor(issue, comments, runs) {
  const bodies = [issue.body ?? "", ...comments.map((comment) => comment.body ?? "")];
  return runs
    .filter((run) => bodies.some((body) => body.includes(occurrenceMarker(run.id, run.run_attempt))))
    .reduce(
      (latest, run) => (!latest || isNewerRun(run, latest) ? run : latest),
      null,
    );
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

function hasReporterCommentPrefix(body) {
  return (
    body.startsWith("<!-- cache-budget-audit-occurrence:") ||
    body.startsWith(occurrenceSummaryMarker()) ||
    body.startsWith(recoverySummaryMarker())
  );
}

function isReporterComment(comment) {
  return (
    comment.user?.login === REPORTER_BOT_LOGIN &&
    hasReporterCommentPrefix(comment.body ?? "")
  );
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

async function canonicalSummary(client, comments, marker) {
  const summaries = comments.filter((comment) =>
    (comment.body ?? "").includes(marker),
  );
  if (summaries.length <= 1) {
    return summaries[0] ?? null;
  }
  const canonical = summaries.at(-1);
  await Promise.all(
    summaries.slice(0, -1).map((summary) => client.deleteIssueComment(summary.id)),
  );
  return canonical;
}

function occurrenceSummaryPlan(comments) {
  const summaries = comments.filter((comment) =>
    (comment.body ?? "").includes(occurrenceSummaryMarker()),
  );
  if (summaries.length <= 1) {
    if (summaries[0]) {
      parsedOccurrenceSummaryState(summaries[0]);
    }
    return { canonical: summaries[0] ?? null, redundant: [] };
  }

  const canonical = summaries.at(-1);
  const canonicalState = parsedOccurrenceSummaryState(canonical);
  if (
    summaries
      .slice(0, -1)
      .some((summary) => !sameOccurrenceSummaryState(parsedOccurrenceSummaryState(summary), canonicalState))
  ) {
    throw invalidOccurrenceSummary("duplicate summaries disagree");
  }
  return { canonical, redundant: summaries.slice(0, -1) };
}

async function consolidateOccurrenceSummary(client, plan) {
  await Promise.all(
    plan.redundant.map((summary) => client.deleteIssueComment(summary.id)),
  );
}

function nextCoalescedState(coalesced, failures) {
  if (coalesced.count > Number.MAX_SAFE_INTEGER - failures.length) {
    throw invalidOccurrenceSummary("coalesced failure count would exceed the safe integer limit");
  }
  return {
    count: coalesced.count + failures.length,
    identities: [
      ...coalesced.identities,
      ...failures.map(runIdentity),
    ].slice(-MAX_RECENT_COALESCED_IDENTITIES),
  };
}

async function canonicalRecoverySummary(client, comments) {
  return canonicalSummary(client, comments, recoverySummaryMarker());
}

async function enforceCommentBudget(client, comments, protectedCommentId = null) {
  const removable = comments.filter((comment) => comment.id !== protectedCommentId);
  while (comments.length > MAX_REPORTER_COMMENTS && removable.length > 0) {
    const comment = removable.shift();
    await client.deleteIssueComment(comment.id);
    comments.splice(comments.indexOf(comment), 1);
  }
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
  // GitHub's workflow-runs list endpoint exposes only the latest attempt for a
  // run ID. Preserve any distinct trusted triggering attempt too: coalesced
  // evidence covers attempts observable through either source, never inferred ones.
  const trustedRuns = [...new Map(
    [...completedRuns, workflowRun]
      .filter((run) => isTrustedAuditRun(run, defaultBranch, repository))
      .map((run) => [runIdentity(run), run]),
  ).values()].sort((left, right) =>
    isNewerRun(left, right) ? 1 : isNewerRun(right, left) ? -1 : 0,
  );
  const latest = latestTrustedAuditRun(
    trustedRuns,
    defaultBranch,
    repository,
  );
  if (latest) {
    workflowRun = latest;
  }
  workflowRun.repository = { full_name: repository };

  const issues = await listAllPages((page) =>
    client.listIssues(CACHE_BUDGET_AUDIT_LABEL, page),
  );
  let issue = issues.find(
    (candidate) =>
      !candidate.pull_request &&
      (candidate.body ?? "").includes(cacheBudgetAuditMarker()),
  );
  const failed = workflowRun.conclusion !== "success";
  let created = 0;
  let updated = 0;
  let closed = 0;

  let comments = issue ? await reporterComments(client, issue) : [];
  let summaryPlan = issue ? occurrenceSummaryPlan(comments) : { canonical: null, redundant: [] };
  let summary = summaryPlan.canonical;
  let cursor = issue ? (summary ? parsedCursor(summary) : recordedCursor(issue, comments, trustedRuns)) : null;
  if (cursor && isNewerRun(cursor, workflowRun)) {
    throw invalidOccurrenceSummary("cursor must not be newer than observable trusted health");
  }
  let coalesced = summary ? parsedCoalesced(summary) : { count: 0, identities: [] };
  let coalescedFailures = trustedRuns.filter(
    (run) =>
      run.conclusion !== "success" &&
      (!cursor || isNewerRun(run, cursor)) &&
      runIdentity(run) !== runIdentity(workflowRun),
  );
  nextCoalescedState(coalesced, coalescedFailures);
  if (summaryPlan.redundant.length > 0) {
    await consolidateOccurrenceSummary(client, summaryPlan);
    comments = comments.filter((comment) => !summaryPlan.redundant.includes(comment));
    summaryPlan = { canonical: summary, redundant: [] };
  }

  // A queue may coalesce a failure behind a later successful reporter run.
  // Preserve that evidence by creating the canonical issue from the newest
  // unseen failure, then immediately reconciling its latest health below.
  if (!issue && (failed || coalescedFailures.length > 0)) {
    const directFailure = failed ? workflowRun : coalescedFailures.at(-1);
    coalescedFailures = coalescedFailures.filter(
      (run) => runIdentity(run) !== runIdentity(directFailure),
    );
    await client.ensureLabel(CACHE_BUDGET_AUDIT_LABEL);
    issue = await client.createIssue({
      title: "[cache budget audit] Actions cache budget audit failed on main",
      body: issueBody({ repository, workflowRun: directFailure }),
      labels: [CACHE_BUDGET_AUDIT_LABEL],
    });
    created = 1;
    comments = [];
    cursor = directFailure;
  }

  if (failed) {
    if (issue && created === 0) {
      let changed = false;
      const occurrence = occurrenceMarker(workflowRun.id, workflowRun.run_attempt);
      if (!(await issueContains(client, issue, occurrence))) {
        comments = await reporterComments(client, issue);
        summary = occurrenceSummaryPlan(comments).canonical;
        const detailedOccurrences = comments.filter((comment) =>
          (comment.body ?? "").includes("<!-- cache-budget-audit-occurrence:") &&
          !(comment.body ?? "").includes(occurrenceSummaryMarker()),
        );
        if (detailedOccurrences.length < MAX_DETAILED_OCCURRENCE_COMMENTS) {
          await client.createIssueComment(
            issue.number,
            occurrenceComment({ workflowRun }),
          );
        } else {
          if (summary) {
            await client.updateIssueComment(
              summary.id,
              occurrenceSummaryComment({ workflowRun, coalesced }),
            );
          } else {
            await client.createIssueComment(
              issue.number,
              occurrenceSummaryComment({ workflowRun, coalesced }),
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
      const summary = await canonicalRecoverySummary(client, comments);
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

  if (issue) {
    comments = await reporterComments(client, issue);
    summary = occurrenceSummaryPlan(comments).canonical;
    coalesced = summary ? parsedCoalesced(summary) : coalesced;
    coalesced = nextCoalescedState(coalesced, coalescedFailures);
    if (summary || coalescedFailures.length > 0) {
      const occurrenceRun = failed
        ? workflowRun
        : coalescedFailures.at(-1) ?? workflowRun;
      if (summary) {
        const nextSummary = occurrenceSummaryComment({ workflowRun, occurrenceRun, coalesced });
        if (summary.body !== nextSummary) {
          await client.updateIssueComment(summary.id, nextSummary);
        }
      } else if (comments.length >= MAX_REPORTER_COMMENTS) {
        const replacement = comments.at(-1);
        await client.updateIssueComment(
          replacement.id,
          occurrenceSummaryComment({ workflowRun, occurrenceRun, coalesced }),
        );
      } else {
        await client.createIssueComment(
          issue.number,
          occurrenceSummaryComment({ workflowRun, occurrenceRun, coalesced }),
        );
      }
    }
    comments = await reporterComments(client, issue);
    await enforceCommentBudget(client, comments);
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

  async deleteIssueComment(commentId) {
    return this.request(
      "DELETE",
      this.repoPath(`/issues/comments/${commentId}`),
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
