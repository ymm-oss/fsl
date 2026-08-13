// SPDX-License-Identifier: Apache-2.0
//
// Calibration suite for the Actions cache budget audit (issue #747).
//
// Every check has an accepting fixture and a rejecting fixture. The rejecting
// fixtures are the point: a checker demonstrated only on healthy input is not
// evidence it can detect the state it exists to detect. The
// `pull-request-cache-present` case in particular is the rejecting control for
// `ci.yml`'s `save-if` guard -- it reproduces the exact listing observed on
// 2026-08-06, when two concurrent pull requests held 6.94 GiB and 4.25 GiB of
// ref-scoped caches and `main` held no Rust build cache at all.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  auditCacheBudget,
  formatReport,
  CACHE_LIMIT_BYTES,
  GIB,
  REQUIRED_MAIN_ENTRIES,
} from "./audit-cache-budget.mjs";
import { fetchAllCaches } from "./run-cache-budget-audit.mjs";

const MAIN = "refs/heads/main";

function cache(key, ref, gib) {
  return { key, ref, size_in_bytes: Math.round(gib * GIB) };
}

function cacheBytes(key, ref, bytes) {
  return { key, ref, size_in_bytes: bytes };
}

function healthyListing() {
  // Exact bytes from `gh api actions/caches` (2026-08-12), not predicted or
  // rounded values -- `semantic-mutation`'s observed clean size in particular
  // was previously miscalibrated from an unverified prediction (2.2 GiB).
  // Product-gate run 31210570118, job 92972117510 (`mutation operators`)
  // restored `No cache found.` (a fully cold start, so it never touched the
  // mutants lane's scratch/evidence paths at all) and still saved this exact
  // 2,919,716,751-byte entry, showing that prediction was wrong; see
  // docs/DESIGN-ci.md, "Actions cache budget", for the full account. No
  // `fsl-logic` entry: that job is restore-only against `rust-workspace`
  // (same section) and a healthy state after that change has no dedicated
  // `fsl-logic` key at all.
  return [
    cacheBytes("v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53", MAIN, 1_605_761_517),
    cacheBytes("v0-rust-wasm-Linux-x64-e8b3ee54-09fbaf53", MAIN, 1_452_450_563),
    cacheBytes("v0-rust-semantic-mutation-Linux-x64-e8b3ee54-09fbaf53", MAIN, 2_919_716_751),
    // `rust-native-z3` is one shared key across a `[macos-15, windows-latest]`
    // matrix (`ci.yml`), so it needs one entry per platform to be healthy.
    cacheBytes("v0-rust-rust-native-z3-Darwin-arm64-f9b08cb2-09fbaf53", MAIN, 1_239_235_056),
    cacheBytes("v0-rust-rust-native-z3-Windows_NT-x64-af4551b0-09fbaf53", MAIN, 619_429_238),
  ];
}

function usageOf(listing) {
  return listing.reduce((total, entry) => total + entry.size_in_bytes, 0);
}

test("accepting: default-branch caches present, budget below threshold, no pull-request Rust caches", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, true, formatReport(result));
  assert.equal(
    formatReport(result),
    "cache budget audit: PASS -- budget within threshold, default-branch caches present, no pull-request-scoped Rust caches",
  );
  assert.deepEqual(result.findings, []);
});

test("rejecting: a pull-request-scoped ci.yml cache requires provenance review", () => {
  const caches = [
    ...healthyListing(),
    cache("v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/745/merge", 1.495),
  ];
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  const finding = result.findings.find((f) => f.code === "pull-request-cache-present");
  assert.ok(finding, formatReport(result));
  assert.match(finding.message, /refs\/pull\/745\/merge/);
  assert.match(finding.message, /rust-workspace/);
  assert.match(finding.message, /before the guard existed|later guard regression/);
});

test("rejecting: pagination finds a forbidden PR Rust cache beyond the first 100 entries", async () => {
  const firstPage = [
    ...healthyListing(),
    ...Array.from({ length: 95 }, (_, index) =>
      cache(`tool-cache-${index}`, MAIN, 0.001),
    ),
  ];
  const secondPage = [
    cache("v0-rust-rust-compile-Linux-x64-aaaaaaaa-bbbbbbbb", "refs/pull/9/merge", 0.08),
  ];
  const requested = [];
  const caches = await fetchAllCaches(async (path) => {
    requested.push(path);
    if (path.endsWith("page=1")) {
      return { total_count: 101, actions_caches: firstPage };
    }
    if (path.endsWith("page=2")) {
      return { total_count: 101, actions_caches: secondPage };
    }
    throw new Error(`unexpected path: ${path}`);
  });

  assert.deepEqual(requested, [
    "/actions/caches?per_page=100&page=1",
    "/actions/caches?per_page=100&page=2",
  ]);
  assert.equal(caches.length, 101);
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  assert.ok(
    result.findings.some((finding) => finding.code === "pull-request-rust-cache-present"),
    formatReport(result),
  );
});

test("rejecting: the 2026-08-06 listing that caused the outage", () => {
  // Reproduced from `gh api repos/ymm-oss/fsl/actions/caches`: two pull requests
  // holding every ci.yml key, and `main` holding only tool binaries.
  const caches = [
    cache("v0-rust-wasm-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/745/merge", 1.352),
    cache("v0-rust-fsl-logic-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/745/merge", 1.369),
    cache("v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/745/merge", 1.495),
    cache("v0-rust-core-contracts-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/745/merge", 0.05),
    cache("v0-rust-rust-compile-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/745/merge", 0.081),
    cache("v0-rust-semantic-mutation-Linux-x64-e8b3ee54-09fbaf5", "refs/pull/743/merge", 2.721),
    cache("v0-rust-wasm-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/743/merge", 1.352),
    cache("v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/743/merge", 1.495),
    cache("v0-rust-fsl-logic-Linux-x64-e8b3ee54-09fbaf53", "refs/pull/743/merge", 1.369),
    cache("Linux-cargo-nextest-0.9.143", MAIN, 0.008),
    cache("Linux-wasm-bindgen-cli-0.2.126", MAIN, 0.007),
    cache("node-cache-Linux-x64-npm-541c2caf", MAIN, 0.011),
  ];
  const result = auditCacheBudget({ caches, usageBytes: Math.round(9.96 * GIB) });
  assert.equal(result.ok, false);
  const codes = result.findings.map((finding) => finding.code);
  assert.ok(codes.includes("budget-exhausted"), formatReport(result));
  // Every required {key, platform} pair is missing from the default branch:
  // the two Linux critical-path keys (`rust-workspace`, `wasm`; this listing
  // predates rust-native-z3 joining REQUIRED_MAIN_ENTRIES and `fsl-logic` is
  // not a required key at all) plus both native-z3 platforms, absent here
  // entirely rather than merely missing from `main`.
  assert.equal(codes.filter((code) => code === "main-cache-absent").length, 4);
  // `ci.yml` shared keys (`wasm`, `rust-workspace`, `semantic-mutation`) on a
  // pull-request ref are flagged by rule 3: 2 + 3 across the two refs.
  assert.equal(codes.filter((code) => code === "pull-request-cache-present").length, 5);
  // `core-contracts`, `rust-compile`, and `fsl-logic` (not a `ci.yml` shared
  // key) are not covered by rule 3 -- this is exactly the gap the generic
  // pull-request-rust-cache rule (rule 4) closes: 2 `fsl-logic` + 1
  // `core-contracts` + 1 `rust-compile`.
  assert.equal(codes.filter((code) => code === "pull-request-rust-cache-present").length, 4);
});

test("rejecting: a non-ci.yml rust cache on a pull-request ref still fires the generic rule", () => {
  // `rust-compile` is not one of `CI_SHARED_KEYS`, so rule 3 alone would miss
  // it -- this is the exact shape `merge-readiness.yml` used to produce
  // before it went restore-only, and the shape any future workflow's
  // unguarded `Swatinem/rust-cache` step would produce again.
  const caches = [
    ...healthyListing(),
    cache("v0-rust-rust-compile-Linux-x64-aaaaaaaa-bbbbbbbb", "refs/pull/9/merge", 0.08),
  ];
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  const finding = result.findings.find((f) => f.code === "pull-request-rust-cache-present");
  assert.ok(finding, formatReport(result));
  assert.match(finding.message, /refs\/pull\/9\/merge/);
  assert.match(finding.message, /rust-compile/);
  // Rule 3 must not also fire for the same entry -- one finding, not two.
  assert.equal(
    result.findings.filter((f) => f.code === "pull-request-cache-present").length,
    0,
  );
});

test("rejecting: main missing the Windows_NT half of rust-native-z3 is not hidden by Darwin's presence", () => {
  const caches = healthyListing().filter((entry) => !entry.key.includes("-Windows_NT-"));
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  const missing = result.findings.filter((finding) => finding.code === "main-cache-absent");
  assert.equal(missing.length, 1, formatReport(result));
  assert.match(missing[0].message, /`rust-native-z3`/);
  assert.match(missing[0].message, /`Windows_NT`/);
});

test("rejecting: main missing the Darwin half of rust-native-z3 is not hidden by Windows_NT's presence", () => {
  const caches = healthyListing().filter((entry) => !entry.key.includes("-Darwin-"));
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  const missing = result.findings.filter((finding) => finding.code === "main-cache-absent");
  assert.equal(missing.length, 1, formatReport(result));
  assert.match(missing[0].message, /`rust-native-z3`/);
  assert.match(missing[0].message, /`Darwin`/);
});

test("accepting: a shared key containing an earlier platform-like substring is parsed from the tail, not misattributed", () => {
  // A lazy `(.+?)` stops at the *first* platform-like substring, so a shared
  // key such as `foo-Linux-bar` (hypothetical, but the bug class is real) would
  // misparse as shared key `foo` -- a reviewer reproduced this causing a real,
  // present main-branch entry to be reported `main-cache-absent`. Parsing
  // greedily from the tail (anchored at `$`) fixes it.
  const caches = [
    ...healthyListing(),
    cache("v0-rust-foo-Linux-bar-Linux-x64-aaaaaaaa-bbbbbbbb", MAIN, 0.01),
  ];
  const result = auditCacheBudget({
    caches,
    usageBytes: usageOf(caches),
    requiredMainEntries: [...REQUIRED_MAIN_ENTRIES, { key: "foo-Linux-bar", platform: "Linux" }],
  });
  assert.equal(result.ok, true, formatReport(result));
});

test("rejecting: budget at or above the threshold, even with a healthy ref layout", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({
    caches,
    usageBytes: Math.round(CACHE_LIMIT_BYTES * 0.85),
  });
  assert.equal(result.ok, false);
  assert.ok(result.findings.some((finding) => finding.code === "budget-exhausted"));
});

test("accepting: budget just below the threshold does not fire", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({
    caches,
    usageBytes: Math.round(CACHE_LIMIT_BYTES * 0.85) - 1,
  });
  assert.equal(
    result.findings.some((finding) => finding.code === "budget-exhausted"),
    false,
    formatReport(result),
  );
});

test("rejecting: a missing default-branch key is reported per key", () => {
  const caches = healthyListing().filter(
    (entry) => !entry.key.includes("-wasm-") || entry.ref !== MAIN,
  );
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  const missing = result.findings.filter((finding) => finding.code === "main-cache-absent");
  assert.equal(missing.length, 1);
  assert.match(missing[0].message, /`wasm`/);
});

test("rejecting: an absent usage total is never read as headroom", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({ caches, usageBytes: null });
  assert.equal(result.ok, false);
  assert.ok(result.findings.some((finding) => finding.code === "usage-unobserved"));
});

test("rejecting: an unreadable listing fails closed rather than passing as empty", () => {
  for (const caches of [undefined, null, "not-an-array"]) {
    const result = auditCacheBudget({ caches, usageBytes: 0 });
    assert.equal(result.ok, false);
    assert.deepEqual(
      result.findings.map((finding) => finding.code),
      ["listing-unreadable"],
    );
  }
});

test("a key whose shared-key cannot be parsed is not silently attributed", () => {
  const caches = [...healthyListing(), cache("some-unrelated-key", "refs/pull/1/merge", 1)];
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(
    result.findings.some((finding) => finding.code === "pull-request-cache-present"),
    false,
  );
});

test("formatReport names every finding code", () => {
  const result = auditCacheBudget({ caches: [], usageBytes: 0 });
  const report = formatReport(result);
  for (const finding of result.findings) {
    assert.ok(report.includes(finding.code), `${finding.code} missing from report`);
  }
});
