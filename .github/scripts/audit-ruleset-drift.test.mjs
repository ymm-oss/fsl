// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  RULESET_DRIFT_LABEL,
  auditAllRulesets,
  auditRuleset,
  compareRuleset,
  fetchObservation,
  reconcileRulesetDrift,
  rulesetDriftMarker,
  validateContract,
} from "./audit-ruleset-drift.mjs";

const fixture = JSON.parse(
  await readFile(new URL("./fixtures/ruleset-19090811.json", import.meta.url), "utf8"),
);
const contract = JSON.parse(
  await readFile(new URL("../ruleset-contract.json", import.meta.url), "utf8"),
);
const contractEntry = contract.rulesets[0];

function requiredStatusChecksRule(ruleset) {
  return ruleset.rules.find((rule) => rule.type === "required_status_checks");
}

function findingClasses(comparison) {
  return comparison.findings.map((finding) => finding.class);
}

// ---------------------------------------------------------------------------
// Fake clients (mirrors report-post-merge-ci.test.mjs's FakeClient pattern)
// ---------------------------------------------------------------------------

class FakeIssueClient {
  constructor({ issues = [], comments = [] } = {}) {
    this.issues = issues;
    this.comments = comments;
    this.labels = new Set();
    this.nextIssue = 500;
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

class FakeReadClient {
  constructor({ body, error } = {}) {
    this.body = body;
    this.error = error;
  }

  async getRuleset() {
    if (this.error) {
      throw this.error;
    }
    return this.body;
  }
}

function notFoundError() {
  const error = new Error("not found");
  error.status = 404;
  return error;
}

// ---------------------------------------------------------------------------
// validateContract
// ---------------------------------------------------------------------------

test("validateContract: the checked-in contract is valid", () => {
  assert.deepEqual(validateContract(contract), []);
});

test("validateContract: rejects a schema mismatch", () => {
  const mutated = structuredClone(contract);
  mutated.schema = "wrong";
  const errors = validateContract(mutated);
  assert.ok(errors.some((e) => e.includes("schema")));
});

test("validateContract: rejects an empty rulesets array", () => {
  const errors = validateContract({ schema: "fsl-ruleset-contract/1", rulesets: [] });
  assert.ok(errors.some((e) => e.includes("rulesets")));
});

test("validateContract: rejects an empty required_status_checks array", () => {
  const mutated = structuredClone(contract);
  mutated.rulesets[0].required_status_checks = [];
  const errors = validateContract(mutated);
  assert.ok(errors.some((e) => e.includes("required_status_checks")));
});

test("validateContract: rejects a duplicate (context, integration_id) pair", () => {
  const mutated = structuredClone(contract);
  mutated.rulesets[0].required_status_checks.push({ context: "merge readiness", integration_id: 15368 });
  const errors = validateContract(mutated);
  assert.ok(errors.some((e) => e.includes("duplicate")));
});

test("validateContract: rejects a deferred_contexts entry colliding with a required context", () => {
  const mutated = structuredClone(contract);
  mutated.rulesets[0].deferred_contexts.push({ context: "rust workspace", reason: "oops" });
  const errors = validateContract(mutated);
  assert.ok(errors.some((e) => e.includes("collides")));
});

test("validateContract: rejects a constituent_contexts entry colliding with a required context", () => {
  const mutated = structuredClone(contract);
  mutated.rulesets[0].constituent_contexts.push({
    context: "WASM",
    aggregate: "WASM",
    reason: "oops",
  });
  const errors = validateContract(mutated);
  assert.ok(errors.some((e) => e.includes("collides")));
});

test("validateContract: rejects a missing bypass_actors field", () => {
  const mutated = structuredClone(contract);
  delete mutated.rulesets[0].bypass_actors;
  const errors = validateContract(mutated);
  assert.ok(errors.some((e) => e.includes("bypass_actors")));
});

// ---------------------------------------------------------------------------
// compareRuleset: the four cases the issue demands
// ---------------------------------------------------------------------------

test("accepting: the verbatim fixture is clean against the checked-in contract", () => {
  const comparison = compareRuleset(contractEntry, fixture);
  assert.deepEqual(comparison, { verdict: "clean", findings: [] });
});

test("rejecting (missing): dropping a required context yields exactly one required-context-missing finding", () => {
  const mutated = structuredClone(fixture);
  const rule = requiredStatusChecksRule(mutated);
  rule.parameters.required_status_checks = rule.parameters.required_status_checks.filter(
    (rsc) => rsc.context !== "rust workspace",
  );

  const comparison = compareRuleset(contractEntry, mutated);

  assert.equal(comparison.verdict, "drift");
  const missing = comparison.findings.filter((f) => f.class === "required-context-missing");
  assert.equal(missing.length, 1);
  assert.match(missing[0].detail, /rust workspace/);
  assert.equal(comparison.findings.filter((f) => f.class === "required-context-unexpected").length, 0);
});

test("rejecting (renamed): renaming a required context yields one missing and one unexpected finding", () => {
  const mutated = structuredClone(fixture);
  const rule = requiredStatusChecksRule(mutated);
  const rsc = rule.parameters.required_status_checks.find(
    (entry) => entry.context === "semantic mutation (changed)",
  );
  rsc.context = "semantic mutation";

  const comparison = compareRuleset(contractEntry, mutated);

  const missing = comparison.findings.filter((f) => f.class === "required-context-missing");
  const unexpected = comparison.findings.filter((f) => f.class === "required-context-unexpected");
  assert.equal(missing.length, 1);
  assert.equal(unexpected.length, 1);
  assert.match(missing[0].detail, /semantic mutation \(changed\)/);
  assert.match(unexpected[0].detail, /"semantic mutation"/);
  assert.doesNotMatch(unexpected[0].detail, /deliberately deferred/);
});

test("blind control: editing the pull_request rule's allowed_merge_methods is invisible to the audit", () => {
  const mutated = structuredClone(fixture);
  const pullRequestRule = mutated.rules.find((rule) => rule.type === "pull_request");
  pullRequestRule.parameters.allowed_merge_methods = ["squash"];
  pullRequestRule.parameters.required_approving_review_count = 2;

  const comparison = compareRuleset(contractEntry, mutated);

  assert.deepEqual(comparison, { verdict: "clean", findings: [] });
});

// ---------------------------------------------------------------------------
// Fail-closed guards
// ---------------------------------------------------------------------------

test("fail-closed: an empty rules array yields empty-rules", () => {
  const mutated = structuredClone(fixture);
  mutated.rules = [];
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("empty-rules"));
});

test("fail-closed: an empty required_status_checks list yields required-contexts-empty", () => {
  const mutated = structuredClone(fixture);
  requiredStatusChecksRule(mutated).parameters.required_status_checks = [];
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("required-contexts-empty"));
});

test("fail-closed: a deleted bypass_actors field yields bypass-actors-unobserved, never an implied empty list", () => {
  const mutated = structuredClone(fixture);
  delete mutated.bypass_actors;
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("bypass-actors-unobserved"));
  assert.ok(!findingClasses(comparison).includes("bypass-actor-added"));
});

test("fail-closed: an added bypass actor yields bypass-actor-added", () => {
  const mutated = structuredClone(fixture);
  mutated.bypass_actors = [{ actor_id: 5, actor_type: "RepositoryRole", bypass_mode: "always" }];
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("bypass-actor-added"));
});

test("fail-closed: an added merge_queue rule yields rule-type-unexpected", () => {
  const mutated = structuredClone(fixture);
  mutated.rules.push({ type: "merge_queue", parameters: {} });
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  const unexpected = comparison.findings.filter((f) => f.class === "rule-type-unexpected");
  assert.equal(unexpected.length, 1);
  assert.match(unexpected[0].detail, /merge_queue/);
});

test("fail-closed: enforcement other than active yields an enforcement finding", () => {
  const mutated = structuredClone(fixture);
  mutated.enforcement = "evaluate";
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("enforcement"));
});

test("fail-closed: flipping strict_required_status_checks_policy yields strict-policy", () => {
  const mutated = structuredClone(fixture);
  requiredStatusChecksRule(mutated).parameters.strict_required_status_checks_policy = false;
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("strict-policy"));
});

test("fail-closed: flipping do_not_enforce_on_create yields enforce-on-create", () => {
  const mutated = structuredClone(fixture);
  requiredStatusChecksRule(mutated).parameters.do_not_enforce_on_create = true;
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("enforce-on-create"));
});

test("fail-closed: retargeting conditions.ref_name.include yields a conditions finding", () => {
  const mutated = structuredClone(fixture);
  mutated.conditions.ref_name.include = ["refs/heads/develop"];
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("conditions"));
});

test("fail-closed: a non-empty conditions.ref_name.exclude yields a conditions finding", () => {
  const mutated = structuredClone(fixture);
  mutated.conditions.ref_name.exclude = ["refs/heads/production"];
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("conditions"));
});

test("fail-closed: a duplicated required context yields required-context-duplicated", () => {
  const mutated = structuredClone(fixture);
  const rule = requiredStatusChecksRule(mutated);
  rule.parameters.required_status_checks.push({ context: "WASM", integration_id: 15368 });
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("required-context-duplicated"));
});

test("fail-closed: a same-named context reported by a different integration_id is missing + unexpected, not satisfied", () => {
  const mutated = structuredClone(fixture);
  const rule = requiredStatusChecksRule(mutated);
  const rsc = rule.parameters.required_status_checks.find((entry) => entry.context === "WASM");
  rsc.integration_id = 99999;
  const comparison = compareRuleset(contractEntry, mutated);
  assert.equal(comparison.verdict, "drift");
  assert.ok(findingClasses(comparison).includes("required-context-missing"));
  assert.ok(findingClasses(comparison).includes("required-context-unexpected"));
});

test("fail-closed: a schema-invalid contract fails validateContract", () => {
  const mutated = structuredClone(contract);
  mutated.schema = "fsl-ruleset-contract/2";
  assert.ok(validateContract(mutated).length > 0);
});

// ---------------------------------------------------------------------------
// fetchObservation classification
// ---------------------------------------------------------------------------

test("fetchObservation: a 404 classifies as ruleset-missing", async () => {
  const client = new FakeReadClient({ error: notFoundError() });
  const result = await fetchObservation(client, contractEntry.ruleset_id);
  assert.equal(result.ok, false);
  assert.equal(result.class, "ruleset-missing");
});

test("fetchObservation: a generic fetch failure classifies as api-unreadable", async () => {
  const client = new FakeReadClient({ error: new Error("network boom") });
  const result = await fetchObservation(client, contractEntry.ruleset_id);
  assert.equal(result.ok, false);
  assert.equal(result.class, "api-unreadable");
});

test("fetchObservation: a successful fetch returns the raw body untouched", async () => {
  const client = new FakeReadClient({ body: fixture });
  const result = await fetchObservation(client, contractEntry.ruleset_id);
  assert.equal(result.ok, true);
  assert.deepEqual(result.body, fixture);
});

// ---------------------------------------------------------------------------
// auditRuleset / auditAllRulesets: fetch failures still create the failure issue
// ---------------------------------------------------------------------------

test("live: an injected fetch failure is api-unreadable and still creates the failure issue", async () => {
  const readClient = new FakeReadClient({ error: new Error("network boom") });
  const issueClient = new FakeIssueClient();

  const { comparison, reconcile } = await auditRuleset({
    readClient,
    issueClient,
    entry: contractEntry,
    runId: "1",
    runUrl: "https://example.invalid/actions/runs/1",
  });

  assert.equal(comparison.verdict, "drift");
  assert.equal(comparison.findings[0].class, "api-unreadable");
  assert.equal(reconcile.action, "created");
  assert.equal(issueClient.issues.length, 1);
  assert.ok(issueClient.issues[0].body.includes(rulesetDriftMarker(String(contractEntry.ruleset_id))));
});

test("live: a 404 is ruleset-missing and still creates the failure issue", async () => {
  const readClient = new FakeReadClient({ error: notFoundError() });
  const issueClient = new FakeIssueClient();

  const { comparison, reconcile } = await auditRuleset({
    readClient,
    issueClient,
    entry: contractEntry,
    runId: "1",
    runUrl: "https://example.invalid/actions/runs/1",
  });

  assert.equal(comparison.findings[0].class, "ruleset-missing");
  assert.equal(reconcile.action, "created");
});

test("auditRuleset: records the raw observation exactly once on success, never on failure", async () => {
  let calls = 0;
  const record = async () => {
    calls += 1;
  };

  await auditRuleset({
    readClient: new FakeReadClient({ body: fixture }),
    issueClient: null,
    entry: contractEntry,
    runId: "1",
    runUrl: "u",
    recordObservation: record,
  });
  assert.equal(calls, 1);

  await auditRuleset({
    readClient: new FakeReadClient({ error: new Error("boom") }),
    issueClient: null,
    entry: contractEntry,
    runId: "1",
    runUrl: "u",
    recordObservation: record,
  });
  assert.equal(calls, 1);
});

test("auditAllRulesets: a clean live-shaped run against the real contract reports no drift and no issue mutation without a client", async () => {
  const readClient = new FakeReadClient({ body: fixture });
  const { anyDrift, results } = await auditAllRulesets({
    readClient,
    issueClient: null,
    contract,
    runId: "1",
    runUrl: "u",
  });
  assert.equal(anyDrift, false);
  assert.equal(results.length, 1);
  assert.equal(results[0].reconcile, null);
});

// ---------------------------------------------------------------------------
// Issue lifecycle: create / occurrence-dedupe / reopen / close-on-clean
// ---------------------------------------------------------------------------

function driftComparison(detail = "enforcement is evaluate") {
  return { verdict: "drift", findings: [{ class: "enforcement", detail }] };
}

const CLEAN = { verdict: "clean", findings: [] };
const TITLE = "[ruleset drift] main safety and CI diverged from .github/ruleset-contract.json";

test("issue lifecycle: creates one issue on first drift", async () => {
  const client = new FakeIssueClient();
  const result = await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "100",
    runUrl: "https://example.invalid/100",
    comparison: driftComparison(),
  });
  assert.equal(result.action, "created");
  assert.equal(client.issues.length, 1);
  assert.ok(client.labels.has(RULESET_DRIFT_LABEL));
  assert.equal(client.issues[0].title, TITLE);
});

test("issue lifecycle: a re-run with the same run id does not add a duplicate occurrence comment", async () => {
  const client = new FakeIssueClient();
  const comparison = driftComparison();
  await reconcileRulesetDrift({ client, key: "19090811", title: TITLE, runId: "100", runUrl: "u", comparison });
  const again = await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "100",
    runUrl: "u",
    comparison,
  });
  assert.equal(again.action, "unchanged");
  assert.equal(client.comments.length, 0);
  assert.equal(client.issues.length, 1);
});

test("issue lifecycle: a new run id on the same open issue adds exactly one occurrence comment", async () => {
  const client = new FakeIssueClient();
  const comparison = driftComparison();
  await reconcileRulesetDrift({ client, key: "19090811", title: TITLE, runId: "100", runUrl: "u", comparison });
  const result = await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "200",
    runUrl: "u2",
    comparison,
  });
  assert.equal(result.action, "updated");
  assert.equal(client.comments.length, 1);
});

test("issue lifecycle: reopens a closed issue on recurrence", async () => {
  const client = new FakeIssueClient();
  const comparison = driftComparison();
  await reconcileRulesetDrift({ client, key: "19090811", title: TITLE, runId: "100", runUrl: "u", comparison });
  client.issues[0].state = "closed";

  const result = await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "200",
    runUrl: "u2",
    comparison,
  });

  assert.equal(result.action, "updated");
  assert.equal(client.issues[0].state, "open");
});

test("issue lifecycle: closes with a recovery comment on the next clean audit", async () => {
  const client = new FakeIssueClient();
  await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "100",
    runUrl: "u",
    comparison: driftComparison(),
  });

  const result = await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "200",
    runUrl: "u2",
    comparison: CLEAN,
  });

  assert.equal(result.action, "closed");
  assert.equal(client.issues[0].state, "closed");
  assert.ok(client.comments.some((comment) => /Recovered/.test(comment.body)));
});

test("issue lifecycle: a clean audit with no existing issue does nothing", async () => {
  const client = new FakeIssueClient();
  const result = await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "100",
    runUrl: "u",
    comparison: CLEAN,
  });
  assert.equal(result.action, "none");
  assert.equal(client.issues.length, 0);
});

test("issue body names RULESET_AUDIT_TOKEN as the secret to rotate for bypass-actors-unobserved", async () => {
  const client = new FakeIssueClient();
  const comparison = {
    verdict: "drift",
    findings: [{ class: "bypass-actors-unobserved", detail: "bypass_actors was absent" }],
  };
  await reconcileRulesetDrift({ client, key: "19090811", title: TITLE, runId: "1", runUrl: "u", comparison });
  assert.match(client.issues[0].body, /RULESET_AUDIT_TOKEN/);
});

test("issue body does not name RULESET_AUDIT_TOKEN for an unrelated finding", async () => {
  const client = new FakeIssueClient();
  await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "1",
    runUrl: "u",
    comparison: driftComparison(),
  });
  assert.doesNotMatch(client.issues[0].body, /RULESET_AUDIT_TOKEN/);
});

test("issue body states the two legitimate exits and never includes token material or log text", async () => {
  const client = new FakeIssueClient();
  await reconcileRulesetDrift({
    client,
    key: "19090811",
    title: TITLE,
    runId: "1",
    runUrl: "u",
    comparison: driftComparison(),
  });
  const body = client.issues[0].body;
  assert.match(body, /revert the live ruleset/i);
  assert.match(body, /docs\/DESIGN-ci\.md/);
  assert.doesNotMatch(body, /ghp_|gho_|github_pat_/);
  assert.doesNotMatch(body, /::error|::warning/); // GitHub Actions log annotation syntax
});

test("issue lifecycle: contract-invalid findings still create an issue keyed independently of a ruleset id", async () => {
  const client = new FakeIssueClient();
  const comparison = {
    verdict: "drift",
    findings: [{ class: "contract-invalid", detail: "schema must be fsl-ruleset-contract/1" }],
  };
  const result = await reconcileRulesetDrift({
    client,
    key: "contract-invalid",
    title: "[ruleset drift] .github/ruleset-contract.json failed validation",
    runId: "1",
    runUrl: "u",
    comparison,
  });
  assert.equal(result.action, "created");
  assert.match(client.issues[0].body, /contract-invalid/);
});
