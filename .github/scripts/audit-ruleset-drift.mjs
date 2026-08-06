// SPDX-License-Identifier: Apache-2.0

// Ruleset drift audit (issue #707, drift-detection half).
//
// This is configuration-conformance automation, not product verification: it never touches
// rust/, the Kernel, or fslc's JSON contract, so AGENTS.md's "one Rust-native entrypoint, no
// Python" clause (which governs tools/check-native-integration.sh) does not apply here. It
// exists because the `main` repository ruleset drifted silently once (docs/DESIGN-ci.md,
// "Required pre-merge contexts, and why the merge queue was rejected") and, without an audit,
// can drift again the same way.
//
// The comparison path (validateContract / compareRuleset) is pure: no network, no file IO, no
// process access. Everything that touches the world -- fetching the live ruleset, writing the
// raw observation, and the failure-issue lifecycle -- is a separate function that takes an
// injected client, mirroring report-post-merge-ci.mjs's reconcilePostMerge({ client, ... })
// shape. This script imports report-post-merge-ci.mjs's exported GitHubRestClient for the issue
// lifecycle rather than building a second REST client and a second issue-dedupe mechanism; that
// file's `import.meta.url` main-guard means importing it here does not execute its own CLI.

import { appendFile, readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

import { GitHubRestClient } from "./report-post-merge-ci.mjs";

export const RULESET_DRIFT_LABEL = "ci/ruleset-drift";

const DEFAULT_CONTRACT_URL = new URL("../ruleset-contract.json", import.meta.url);
const DEFAULT_OBSERVATION_PATH = "ruleset-observation.json";

class ContractError extends Error {}

// ---------------------------------------------------------------------------
// Pure: contract validation
// ---------------------------------------------------------------------------

/**
 * Fail-closed structural validation of the checked-in contract. Returns an array of error
 * strings; an empty array means the contract is valid. Never throws.
 */
export function validateContract(contract) {
  const errors = [];

  if (!contract || typeof contract !== "object" || Array.isArray(contract)) {
    return ["contract must be a JSON object"];
  }

  if (contract.schema !== "fsl-ruleset-contract/1") {
    errors.push(
      `schema must be "fsl-ruleset-contract/1", got ${JSON.stringify(contract.schema)}`,
    );
  }

  if (!Array.isArray(contract.rulesets) || contract.rulesets.length === 0) {
    errors.push("rulesets must be a non-empty array");
    return errors;
  }

  contract.rulesets.forEach((entry, index) => {
    const label = `rulesets[${index}]`;

    if (!Array.isArray(entry.required_status_checks) || entry.required_status_checks.length === 0) {
      errors.push(`${label}.required_status_checks must be a non-empty array`);
    } else {
      const seen = new Set();
      entry.required_status_checks.forEach((rsc, i) => {
        if (typeof rsc.context !== "string" || rsc.context.length === 0) {
          errors.push(`${label}.required_status_checks[${i}].context must be a non-empty string`);
        }
        if (!Number.isInteger(rsc.integration_id)) {
          errors.push(`${label}.required_status_checks[${i}].integration_id must be an integer`);
        }
        const key = `${rsc.context} ${rsc.integration_id}`;
        if (seen.has(key)) {
          errors.push(
            `${label}.required_status_checks has a duplicate (context, integration_id) pair: ${rsc.context} / ${rsc.integration_id}`,
          );
        }
        seen.add(key);
      });
    }

    const requiredNames = new Set(
      (Array.isArray(entry.required_status_checks) ? entry.required_status_checks : []).map(
        (rsc) => rsc.context,
      ),
    );

    for (const group of ["deferred_contexts", "constituent_contexts"]) {
      const list = entry[group];
      if (!Array.isArray(list)) {
        errors.push(`${label}.${group} must be an array`);
        continue;
      }
      for (const item of list) {
        if (requiredNames.has(item.context)) {
          errors.push(
            `${label}.${group} entry "${item.context}" collides with a required_status_checks context; a context cannot be both required and ${group}`,
          );
        }
      }
    }

    if (!Array.isArray(entry.bypass_actors)) {
      errors.push(`${label}.bypass_actors must be present and be an array`);
    }
  });

  return errors;
}

// ---------------------------------------------------------------------------
// Pure: ruleset comparison
// ---------------------------------------------------------------------------

function arraysEqualOrdered(a, b) {
  return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((v, i) => v === b[i]);
}

function canonicalStringify(value) {
  if (Array.isArray(value)) {
    return `[${value.map(canonicalStringify).join(",")}]`;
  }
  if (value && typeof value === "object") {
    const keys = Object.keys(value).sort();
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canonicalStringify(value[k])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function bypassActorsEqual(observed, expected) {
  if (!Array.isArray(observed) || !Array.isArray(expected) || observed.length !== expected.length) {
    return false;
  }
  const a = observed.map(canonicalStringify).sort();
  const b = expected.map(canonicalStringify).sort();
  return a.every((v, i) => v === b[i]);
}

// A context name may itself contain spaces or parentheses (e.g. "semantic mutation
// (changed)"), so the key is only ever used for map lookups -- never re-split back into its
// parts. The original { context, integration_id } pair is kept alongside the key for reporting.
function pairKey(rsc) {
  return JSON.stringify([rsc.context, rsc.integration_id]);
}

function compareRequiredContexts(contractEntry, observedContexts) {
  const findings = [];

  const observedByKey = new Map();
  const observedCounts = new Map();
  for (const rsc of observedContexts) {
    const key = pairKey(rsc);
    observedCounts.set(key, (observedCounts.get(key) ?? 0) + 1);
    observedByKey.set(key, rsc);
  }
  for (const [key, count] of observedCounts) {
    if (count > 1) {
      const rsc = observedByKey.get(key);
      findings.push({
        class: "required-context-duplicated",
        detail: `"${rsc.context}" (integration_id ${rsc.integration_id}) is reported ${count} times`,
      });
    }
  }

  const contractPairs = new Map(contractEntry.required_status_checks.map((rsc) => [pairKey(rsc), rsc]));
  const deferredNames = new Set((contractEntry.deferred_contexts ?? []).map((entry) => entry.context));

  for (const [key, rsc] of contractPairs) {
    if (!observedCounts.has(key)) {
      findings.push({
        class: "required-context-missing",
        detail: `"${rsc.context}" (integration_id ${rsc.integration_id}) is required by the contract but was not observed`,
      });
    }
  }

  for (const [key, rsc] of observedByKey) {
    if (!contractPairs.has(key)) {
      if (deferredNames.has(rsc.context)) {
        findings.push({
          class: "required-context-unexpected",
          detail: `"${rsc.context}" (integration_id ${rsc.integration_id}) is required in the live ruleset, but the contract records it as deliberately deferred (docs/DESIGN-ci.md, "Product gate contract"); requiring it pre-merge would deadlock every pull request`,
        });
      } else {
        findings.push({
          class: "required-context-unexpected",
          detail: `"${rsc.context}" (integration_id ${rsc.integration_id}) is required in the live ruleset but is not in the contract`,
        });
      }
    }
  }

  return findings;
}

/**
 * Pure comparison of one checked-in contract ruleset entry against one observed ruleset JSON
 * body (the raw GitHub API response shape). Never does IO. Returns { verdict, findings }, where
 * verdict is "clean" iff findings is empty.
 */
export function compareRuleset(contractEntry, observation) {
  const findings = [];

  if (observation.id !== contractEntry.ruleset_id) {
    findings.push({
      class: "ruleset-identity",
      detail: `id: expected ${JSON.stringify(contractEntry.ruleset_id)}, observed ${JSON.stringify(observation.id)}`,
    });
  }
  if (observation.name !== contractEntry.ruleset_name) {
    findings.push({
      class: "ruleset-identity",
      detail: `name: expected ${JSON.stringify(contractEntry.ruleset_name)}, observed ${JSON.stringify(observation.name)}`,
    });
  }
  if (observation.target !== contractEntry.target) {
    findings.push({
      class: "ruleset-identity",
      detail: `target: expected ${JSON.stringify(contractEntry.target)}, observed ${JSON.stringify(observation.target)}`,
    });
  }

  if (observation.enforcement !== "active") {
    findings.push({
      class: "enforcement",
      detail: `enforcement is ${JSON.stringify(observation.enforcement)}, expected "active"`,
    });
  }

  const include = observation.conditions?.ref_name?.include ?? [];
  const exclude = observation.conditions?.ref_name?.exclude ?? [];
  if (!arraysEqualOrdered(include, contractEntry.ref_name_include)) {
    findings.push({
      class: "conditions",
      detail: `conditions.ref_name.include is ${JSON.stringify(include)}, expected ${JSON.stringify(contractEntry.ref_name_include)}`,
    });
  }
  if (!arraysEqualOrdered(exclude, contractEntry.ref_name_exclude)) {
    findings.push({
      class: "conditions",
      detail: `conditions.ref_name.exclude is ${JSON.stringify(exclude)}, expected ${JSON.stringify(contractEntry.ref_name_exclude)}`,
    });
  }

  const rules = Array.isArray(observation.rules) ? observation.rules : [];
  if (rules.length === 0) {
    findings.push({ class: "empty-rules", detail: "the observed ruleset has no rules at all" });
  } else {
    const observedTypes = new Set(rules.map((rule) => rule.type));
    const expectedTypes = new Set(contractEntry.rule_types);

    for (const type of expectedTypes) {
      if (!observedTypes.has(type)) {
        findings.push({ class: "rule-type-missing", detail: `rule type "${type}" is missing` });
      }
    }
    for (const type of observedTypes) {
      if (!expectedTypes.has(type)) {
        findings.push({
          class: "rule-type-unexpected",
          detail: `rule type "${type}" is present but not in the contract`,
        });
      }
    }

    // `pull_request` rule parameters (review counts, allowed_merge_methods, ...) are the blind
    // control's subject and are deliberately never read here. Human review policy lives in
    // ruleset 19090821 and is a separate accepted process decision.

    const requiredStatusChecksRule = rules.find((rule) => rule.type === "required_status_checks");
    if (requiredStatusChecksRule) {
      const params = requiredStatusChecksRule.parameters ?? {};

      if (params.strict_required_status_checks_policy !== true) {
        findings.push({
          class: "strict-policy",
          detail: `strict_required_status_checks_policy is ${JSON.stringify(params.strict_required_status_checks_policy)}, expected true`,
        });
      }
      if (params.do_not_enforce_on_create !== false) {
        findings.push({
          class: "enforce-on-create",
          detail: `do_not_enforce_on_create is ${JSON.stringify(params.do_not_enforce_on_create)}, expected false`,
        });
      }

      const observedContexts = Array.isArray(params.required_status_checks)
        ? params.required_status_checks
        : [];
      if (observedContexts.length === 0) {
        findings.push({
          class: "required-contexts-empty",
          detail: "observed required_status_checks is empty",
        });
      } else {
        findings.push(...compareRequiredContexts(contractEntry, observedContexts));
      }
    }
  }

  if (!Object.prototype.hasOwnProperty.call(observation, "bypass_actors")) {
    findings.push({
      class: "bypass-actors-unobserved",
      detail:
        "bypass_actors was absent from the observation; absence of the field is never read as an empty list, because absence of evidence is not evidence of emptiness",
    });
  } else if (!bypassActorsEqual(observation.bypass_actors, contractEntry.bypass_actors)) {
    findings.push({
      class: "bypass-actor-added",
      detail: `bypass_actors is ${JSON.stringify(observation.bypass_actors)}, expected ${JSON.stringify(contractEntry.bypass_actors)}`,
    });
  }

  return { verdict: findings.length === 0 ? "clean" : "drift", findings };
}

// ---------------------------------------------------------------------------
// IO: live ruleset fetch (injected client)
// ---------------------------------------------------------------------------

/**
 * Minimal read client for the ruleset endpoint. Rulesets are readable unauthenticated for this
 * public repository, but `bypass_actors` is returned only to a caller with write access to the
 * ruleset -- GITHUB_TOKEN has no `administration` permission at all, so a fine-grained PAT
 * (RULESET_AUDIT_TOKEN) is required to observe that field. When RULESET_AUDIT_TOKEN is unset,
 * the caller falls back to GITHUB_TOKEN, which fails the bypass leg deliberately (see
 * `bypass-actors-unobserved` above) rather than silently skip it.
 */
export class RulesetReadClient {
  constructor({ token, repository }) {
    const [owner, repo] = repository.split("/");
    if (!owner || !repo) {
      throw new Error(`invalid GITHUB_REPOSITORY: ${repository}`);
    }
    this.token = token;
    this.owner = owner;
    this.repo = repo;
  }

  async getRuleset(rulesetId) {
    const response = await fetch(
      `https://api.github.com/repos/${this.owner}/${this.repo}/rulesets/${rulesetId}`,
      {
        headers: {
          Accept: "application/vnd.github+json",
          ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
          "User-Agent": "fsl-ruleset-drift-audit",
          "X-GitHub-Api-Version": "2022-11-28",
        },
      },
    );

    if (response.status === 404) {
      const error = new Error(`ruleset ${rulesetId} was not found`);
      error.status = 404;
      throw error;
    }

    const text = await response.text();
    if (!response.ok) {
      throw new Error(`GitHub API GET rulesets/${rulesetId} failed (${response.status}): ${text}`);
    }

    try {
      return JSON.parse(text);
    } catch (parseError) {
      throw new Error(`failed to parse ruleset ${rulesetId} response: ${parseError.message}`);
    }
  }
}

/**
 * Fetch one ruleset through an injected client and classify a failure without ever throwing.
 * A 404 is `ruleset-missing`; any other fetch or parse error is `api-unreadable`. Returns
 * `{ ok: true, body }` on success.
 */
export async function fetchObservation(readClient, rulesetId) {
  try {
    const body = await readClient.getRuleset(rulesetId);
    return { ok: true, body };
  } catch (error) {
    if (error && error.status === 404) {
      return { ok: false, class: "ruleset-missing", detail: `ruleset ${rulesetId} was not found (404)` };
    }
    return {
      ok: false,
      class: "api-unreadable",
      detail: `failed to fetch or parse ruleset ${rulesetId}: ${error.message}`,
    };
  }
}

// ---------------------------------------------------------------------------
// IO: issue lifecycle (injected client, shape mirrors reconcilePostMerge)
// ---------------------------------------------------------------------------

export function rulesetDriftMarker(key) {
  return `<!-- ruleset-drift:${key} -->`;
}

export function rulesetDriftOccurrenceMarker(runId) {
  return `<!-- ruleset-drift-occurrence:${runId} -->`;
}

function rulesetDriftRecoveryMarker(runId) {
  return `<!-- ruleset-drift-recovery:${runId} -->`;
}

function findingsList(findings) {
  return findings.map((f) => `- \`${f.class}\`: ${f.detail}`).join("\n");
}

export function buildRulesetDriftIssueBody({ key, runId, runUrl, artifactUrl, findings, rotateToken }) {
  const lines = [
    rulesetDriftMarker(key),
    rulesetDriftOccurrenceMarker(runId),
    "",
    "The ruleset drift audit found the live GitHub ruleset diverged from `.github/ruleset-contract.json`.",
    "",
    "Findings:",
    findingsList(findings),
    "",
    `Raw observation: [workflow run](${runUrl})${artifactUrl ? ` — [artifact](${artifactUrl})` : ""}.`,
    "",
    "There are two legitimate exits: revert the live ruleset to match the contract, or amend the",
    "contract, the fixture, and `docs/DESIGN-ci.md` together in one pull request if the live",
    "change was intentional. Editing the contract alone to match unexplained live state is the",
    "drift this audit catches, not a fix.",
  ];
  if (rotateToken) {
    lines.push(
      "",
      "This finding includes `bypass-actors-unobserved`: rotate the `RULESET_AUDIT_TOKEN` secret." +
        " A fine-grained PAT that lost administration-read access on this repository will drop" +
        " into this same failure, indistinguishable from a real bypass-actor change until rotated.",
    );
  }
  return lines.join("\n");
}

function buildRulesetDriftOccurrenceComment({ runId, runUrl, findings }) {
  return [
    rulesetDriftOccurrenceMarker(runId),
    `The drift audit recurred on [this run](${runUrl}).`,
    "",
    "Findings:",
    findingsList(findings),
  ].join("\n");
}

function buildRulesetDriftRecoveryComment({ runId, runUrl }) {
  return [
    rulesetDriftRecoveryMarker(runId),
    `Recovered: [this run](${runUrl}) found the live ruleset clean against the contract again.`,
    "",
    "Reopen this issue if drift recurs.",
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
 * Issue lifecycle for one ruleset's comparison result. Deliberately does not reuse
 * reconcilePostMerge itself -- its keying (workflow id + job name), recovery predicate, and
 * out-of-order run redirection are workflow-run-shaped and meaningless for a single ruleset
 * comparison -- but keeps the same *shape*: one canonical issue per key, an occurrence marker so
 * re-runs are idempotent, reopen on recurrence, close with a recovery comment on the next clean
 * audit. `client` only needs the same interface GitHubRestClient already exports:
 * ensureLabel/listIssues/listIssueComments/createIssue/createIssueComment/updateIssue.
 */
export async function reconcileRulesetDrift({ client, key, title, runId, runUrl, artifactUrl, comparison }) {
  await client.ensureLabel(RULESET_DRIFT_LABEL);

  const marker = rulesetDriftMarker(key);
  const issues = await client.listIssues(RULESET_DRIFT_LABEL);
  const issue = issues.find((candidate) => (candidate.body ?? "").includes(marker));

  if (comparison.verdict === "clean") {
    if (!issue || issue.state !== "open") {
      return { action: "none" };
    }
    const recovery = rulesetDriftRecoveryMarker(runId);
    if (!(await issueContains(client, issue, recovery))) {
      await client.createIssueComment(issue.number, buildRulesetDriftRecoveryComment({ runId, runUrl }));
    }
    await client.updateIssue(issue.number, { state: "closed" });
    return { action: "closed", issueNumber: issue.number };
  }

  const rotateToken = comparison.findings.some((finding) => finding.class === "bypass-actors-unobserved");

  if (!issue) {
    const created = await client.createIssue({
      title,
      body: buildRulesetDriftIssueBody({
        key,
        runId,
        runUrl,
        artifactUrl,
        findings: comparison.findings,
        rotateToken,
      }),
      labels: [RULESET_DRIFT_LABEL],
    });
    return { action: "created", issueNumber: created.number };
  }

  const occurrence = rulesetDriftOccurrenceMarker(runId);
  let changed = false;
  if (!(await issueContains(client, issue, occurrence))) {
    await client.createIssueComment(
      issue.number,
      buildRulesetDriftOccurrenceComment({ runId, runUrl, findings: comparison.findings }),
    );
    changed = true;
  }
  if (issue.state !== "open") {
    await client.updateIssue(issue.number, { state: "open" });
    changed = true;
  }
  return { action: changed ? "updated" : "unchanged", issueNumber: issue.number };
}

// ---------------------------------------------------------------------------
// Orchestration: fetch (or accept) -> record raw observation -> compare -> reconcile
// ---------------------------------------------------------------------------

/**
 * Audits one contract ruleset entry end to end. `readClient` must expose `getRuleset(id)`.
 * `issueClient` is optional (omit it for an offline/no-mutation run); when present it must
 * expose GitHubRestClient's interface. `recordObservation`, when given, is awaited on a
 * successful fetch BEFORE compareRuleset runs, so the raw observation is always durable before
 * classification happens -- never the other way around.
 */
export async function auditRuleset({ readClient, issueClient, entry, runId, runUrl, artifactUrl, recordObservation }) {
  const fetchResult = await fetchObservation(readClient, entry.ruleset_id);

  let comparison;
  if (fetchResult.ok) {
    if (recordObservation) {
      await recordObservation(fetchResult.body);
    }
    comparison = compareRuleset(entry, fetchResult.body);
  } else {
    comparison = { verdict: "drift", findings: [{ class: fetchResult.class, detail: fetchResult.detail }] };
  }

  let reconcile = null;
  if (issueClient) {
    reconcile = await reconcileRulesetDrift({
      client: issueClient,
      key: String(entry.ruleset_id),
      title: `[ruleset drift] ${entry.ruleset_name} diverged from .github/ruleset-contract.json`,
      runId,
      runUrl,
      artifactUrl,
      comparison,
    });
  }

  return { comparison, reconcile };
}

export async function auditAllRulesets({ readClient, issueClient, contract, runId, runUrl, artifactUrl, recordObservation }) {
  const results = [];
  let anyDrift = false;
  for (const entry of contract.rulesets) {
    const result = await auditRuleset({
      readClient,
      issueClient,
      entry,
      runId,
      runUrl,
      artifactUrl,
      recordObservation,
    });
    results.push({ rulesetId: entry.ruleset_id, ...result });
    if (result.comparison.verdict !== "clean") {
      anyDrift = true;
    }
  }
  return { anyDrift, results };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

async function loadContract() {
  let raw;
  try {
    raw = await readFile(DEFAULT_CONTRACT_URL, "utf8");
  } catch (error) {
    throw new ContractError(`failed to read .github/ruleset-contract.json: ${error.message}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    throw new ContractError(`failed to parse .github/ruleset-contract.json: ${error.message}`);
  }
  const errors = validateContract(parsed);
  if (errors.length > 0) {
    throw new ContractError(
      `.github/ruleset-contract.json failed validation:\n${errors.map((e) => `- ${e}`).join("\n")}`,
    );
  }
  return parsed;
}

function printReport(label, comparison) {
  if (comparison.verdict === "clean") {
    console.log(`${label}: clean (0 findings)`);
    return;
  }
  console.log(`${label}: drift (${comparison.findings.length} finding(s))`);
  for (const finding of comparison.findings) {
    console.log(`  - ${finding.class}: ${finding.detail}`);
  }
}

function parseArgs(argv) {
  if (argv.length === 0) {
    return { observationPath: undefined };
  }
  if (argv.length === 2 && argv[0] === "--observation" && argv[1]) {
    return { observationPath: argv[1] };
  }
  console.error("usage: node audit-ruleset-drift.mjs [--observation FILE]");
  process.exit(2);
  throw new Error("unreachable");
}

async function handleContractInvalid(error, args) {
  console.error(error.message);
  if (args.observationPath) {
    // Offline mode never mutates issues, even on a contract failure.
    return;
  }
  const repository = process.env.GITHUB_REPOSITORY;
  const token = process.env.GITHUB_TOKEN;
  if (!repository || !token) {
    return;
  }
  const client = new GitHubRestClient({ token, repository });
  const runId = process.env.GITHUB_RUN_ID ?? "local";
  const runUrl = `https://github.com/${repository}/actions/runs/${runId}`;
  await reconcileRulesetDrift({
    client,
    key: "contract-invalid",
    title: "[ruleset drift] .github/ruleset-contract.json failed validation",
    runId,
    runUrl,
    comparison: { verdict: "drift", findings: [{ class: "contract-invalid", detail: error.message }] },
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));

  let contract;
  try {
    contract = await loadContract();
  } catch (error) {
    await handleContractInvalid(error, args);
    process.exit(1);
    return;
  }

  if (args.observationPath) {
    let body;
    try {
      const raw = await readFile(args.observationPath, "utf8");
      body = JSON.parse(raw);
    } catch (error) {
      const comparison = {
        verdict: "drift",
        findings: [
          {
            class: "api-unreadable",
            detail: `failed to read or parse observation file ${args.observationPath}: ${error.message}`,
          },
        ],
      };
      printReport(args.observationPath, comparison);
      process.exit(1);
      return;
    }
    const comparison = compareRuleset(contract.rulesets[0], body);
    printReport(contract.rulesets[0].ruleset_name, comparison);
    process.exit(comparison.verdict === "clean" ? 0 : 1);
    return;
  }

  const repository = process.env.GITHUB_REPOSITORY;
  if (!repository) {
    console.error("GITHUB_REPOSITORY is required for a live audit run");
    process.exit(2);
    return;
  }

  const githubToken = process.env.GITHUB_TOKEN;
  const rulesetToken = process.env.RULESET_AUDIT_TOKEN || githubToken;
  const readClient = new RulesetReadClient({ token: rulesetToken, repository });
  const issueClient = githubToken ? new GitHubRestClient({ token: githubToken, repository }) : null;

  const runId = process.env.GITHUB_RUN_ID ?? "local";
  const runUrl = `https://github.com/${repository}/actions/runs/${runId}`;
  const observationPath = process.env.RULESET_OBSERVATION_PATH ?? DEFAULT_OBSERVATION_PATH;

  const recordObservation = async (body) => {
    await writeFile(observationPath, `${JSON.stringify(body, null, 2)}\n`, "utf8");
    if (process.env.GITHUB_STEP_SUMMARY) {
      const digest = createHash("sha256").update(JSON.stringify(body)).digest("hex");
      await appendFile(
        process.env.GITHUB_STEP_SUMMARY,
        `Raw ruleset observation written to \`${observationPath}\` (sha256 \`${digest}\`) before classification.\n`,
      );
    }
  };

  const { anyDrift, results } = await auditAllRulesets({
    readClient,
    issueClient,
    contract,
    runId,
    runUrl,
    recordObservation,
  });

  for (const entry of contract.rulesets) {
    const result = results.find((r) => r.rulesetId === entry.ruleset_id);
    printReport(entry.ruleset_name, result.comparison);
  }

  process.exit(anyDrift ? 1 : 0);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
