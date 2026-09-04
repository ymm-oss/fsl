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
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  auditCacheBudget,
  formatReport,
  BUDGET_WARN_FRACTION,
  CACHE_LIMIT_BYTES,
  CI_SHARED_KEYS,
  GIB,
  REQUIRED_MAIN_ENTRIES,
} from "./audit-cache-budget.mjs";
import {
  CACHE_AUDIT_REQUEST_BUDGET,
  CACHE_AUDIT_REQUEST_HEADROOM,
  CACHE_PAGE_SIZE,
  createCacheAuditApi,
  fetchCacheCollection as fetchAllCaches,
  fetchStableCaches,
  GITHUB_TOKEN_REQUEST_CEILING,
  maximumAuditRequests,
  observedUsageBytes,
  pageNumber,
  runCacheBudgetAudit,
  sameCacheCollection,
  STABILITY_ATTEMPTS,
  STABILITY_RETRY_DELAY_MS,
} from "./run-cache-budget-audit.mjs";

const MAIN = "refs/heads/main";
const BYTES_PER_GIB = 1_073_741_824;
const PASS_REPORT =
  "cache budget audit: PASS -- budget within threshold, default-branch caches present, no pull-request-scoped Rust caches\n" +
  "  informational/generation-coexistence: 5 cache entries observed; 0 generations beyond the newest per {sharedKey, platform} pair on `refs/heads/main` (0.00 GiB) -- diagnostic visibility only, not subtracted from the budget judgment below.";
const THRESHOLD_REPORT =
  "cache budget audit: FAIL -- 1 finding(s)\n" +
  "  budget-exhausted: cache usage is 8.50 GiB of a 10.00 GiB limit (85%), at or above the 85% threshold. 8.50 GiB is 1.50 GiB remaining before the limit; a sufficiently large save can trigger least-recently-used eviction, including a default-branch cache that a main-targeting pull request depends on.\n" +
  "  informational/generation-coexistence: 5 cache entries observed; 0 generations beyond the newest per {sharedKey, platform} pair on `refs/heads/main` (0.00 GiB) -- diagnostic visibility only, not subtracted from the budget judgment below.";
const PAGE_PATH = (page) =>
  `/actions/caches?per_page=100&sort=created_at&direction=asc&page=${page}`;
const RUNNER_PATH = fileURLToPath(new URL("./run-cache-budget-audit.mjs", import.meta.url));
const RUNNER_URL = pathToFileURL(RUNNER_PATH).href;

test("exported cache-audit constants have fixed protocol values", () => {
  assert.equal(GIB, 1_073_741_824);
  assert.equal(CACHE_LIMIT_BYTES, 10_737_418_240);
  assert.equal(BUDGET_WARN_FRACTION, 0.85);
  assert.deepEqual(CI_SHARED_KEYS, ["rust-workspace", "wasm", "semantic-mutation"]);
  assert.deepEqual(REQUIRED_MAIN_ENTRIES, [
    { key: "rust-workspace", platform: "Linux" },
    { key: "wasm", platform: "Linux" },
    { key: "semantic-mutation", platform: "Linux" },
    { key: "rust-native-z3", platform: "Windows_NT" },
    { key: "rust-native-z3", platform: "Darwin" },
  ]);
  assert.equal(CACHE_PAGE_SIZE, 100);
  assert.equal(GITHUB_TOKEN_REQUEST_CEILING, 1_000);
  assert.equal(CACHE_AUDIT_REQUEST_HEADROOM, 100);
  assert.equal(CACHE_AUDIT_REQUEST_BUDGET, 900);
  assert.equal(STABILITY_ATTEMPTS, 2);
  assert.equal(STABILITY_RETRY_DELAY_MS, 1_000);
});

test("pageNumber reads the page query parameter without substring matches", () => {
  const cases = [
    ["/actions/caches?per_page=100&page=1&sort=created_at", "1"],
    ["/actions/caches?sort=created_at&direction=asc&page=10&per_page=100", "10"],
    ["/actions/caches?per_page=100&sort=created_at", null],
  ];

  for (const [path, expected] of cases) {
    assert.equal(pageNumber(path), expected, path);
  }
});

// Arbitrary but fixed: no test that does not care about generation ordering
// needs to think about `created_at` at all. A test that does (the
// generation-coexistence de-duplication, issue #926) passes its own explicit
// `createdAt` per entry instead.
const DEFAULT_CREATED_AT = "2026-01-01T00:00:00Z";

function cache(key, ref, gib, createdAt = DEFAULT_CREATED_AT) {
  return { key, ref, size_in_bytes: Math.round(gib * BYTES_PER_GIB), created_at: createdAt };
}

function cacheBytes(key, ref, bytes, createdAt = DEFAULT_CREATED_AT) {
  return { key, ref, size_in_bytes: bytes, created_at: createdAt };
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

function withCacheIds(entries, firstId = 1) {
  return entries.map((entry, index) => ({ ...entry, id: firstId + index }));
}

function outOfRangeCachePage() {
  // Observed with:
  // gh api "/repos/ymm-oss/fsl/actions/caches?per_page=100&sort=created_at&direction=asc&page=1"
  // {"count":9,"total_count":9}
  // gh api "/repos/ymm-oss/fsl/actions/caches?per_page=100&sort=created_at&direction=asc&page=2"
  // {"count":0,"total_count":0}
  // An out-of-range page resets total_count to 0; it does not repeat the
  // first page's total_count.
  return { total_count: 0, actions_caches: [] };
}

function jsonResponse(body, options = {}) {
  const { status = 200, statusText = "OK" } = options;
  const remaining = Object.hasOwn(options, "remaining") ? options.remaining : "999";
  const headers = new Headers();
  if (remaining !== undefined) headers.set("x-ratelimit-remaining", remaining);
  return new Response(JSON.stringify(body), { status, statusText, headers });
}

function childEnvironment(overrides = {}) {
  const environment = { ...process.env, ...overrides };
  // Test subprocesses must not write profiles into their parent's coverage
  // directory: Node otherwise merges an independently instrumented CLI entry
  // into this suite's report and makes its aggregate coverage nondeterministic.
  delete environment.NODE_V8_COVERAGE;
  return environment;
}

test("child subprocess environments never inherit V8 coverage output", () => {
  const environment = childEnvironment({ NODE_V8_COVERAGE: "sentinel" });

  assert.equal(Object.hasOwn(environment, "NODE_V8_COVERAGE"), false);
});

function runExecutableRunner({ listing, usageBytes = usageOf(listing), remaining = "999" }) {
  // Run the actual CLI guard with a child-local fetch fixture. Importing after
  // assigning argv[1] enters the same `main()`/`process.exit` path as `node
  // run-cache-budget-audit.mjs`, while keeping its API observations offline.
  const fixture = `
    const listing = ${JSON.stringify(listing)};
    const usageBytes = ${JSON.stringify(usageBytes)};
    globalThis.fetch = async (url) => {
      const parsed = new URL(url);
      if (parsed.pathname.endsWith("/actions/cache/usage")) {
        return new Response(JSON.stringify({ active_caches_size_in_bytes: usageBytes }), {
          headers: { "x-ratelimit-remaining": ${JSON.stringify(remaining)} },
        });
      }
      if (parsed.searchParams.get("page") === "1") {
        return new Response(JSON.stringify({
          total_count: listing.length,
          actions_caches: listing,
        }), { headers: { "x-ratelimit-remaining": ${JSON.stringify(remaining)} } });
      }
      if (parsed.searchParams.get("page") === "2") {
        return new Response(JSON.stringify({ total_count: 0, actions_caches: [] }), {
          headers: { "x-ratelimit-remaining": ${JSON.stringify(remaining)} },
        });
      }
      throw new Error(\`unexpected cache page \${parsed.searchParams.get("page")}\`);
    };
    process.argv[1] = ${JSON.stringify(RUNNER_PATH)};
    await import(${JSON.stringify(RUNNER_URL)});
  `;
  return spawnSync(process.execPath, ["--input-type=module", "--eval", fixture], {
    encoding: "utf8",
    env: childEnvironment({
      GITHUB_TOKEN: "test-token",
      GITHUB_REPOSITORY: "owner/repo",
    }),
  });
}

async function runStableRunnerListing({ listing, usageBytes = usageOf(listing) }) {
  const reports = [];
  const errors = [];
  const paths = [];
  const result = await runCacheBudgetAudit({
    token: "test-token",
    repo: "owner/repo",
    writeReport: (report) => reports.push(report),
    writeError: (error) => errors.push(error),
    fetchImpl: async (url) => {
      const parsed = new URL(url);
      paths.push(parsed.pathname + parsed.search);
      if (parsed.pathname.endsWith("/actions/cache/usage")) {
        return jsonResponse({ active_caches_size_in_bytes: usageBytes });
      }
      if (parsed.searchParams.get("page") === "1") {
        return jsonResponse({
          total_count: listing.length,
          actions_caches: listing,
        });
      }
      if (parsed.searchParams.get("page") === "2") return jsonResponse(outOfRangeCachePage());
      throw new Error(`unexpected cache page ${parsed.searchParams.get("page")}`);
    },
  });
  return { errors, paths, reports, result };
}

function cachePage(totalCount, page) {
  const start = (page - 1) * 100;
  return withCacheIds(
    Array.from(
      { length: Math.min(100, Math.max(0, totalCount - start)) },
      (_, index) => cache(`tool-cache-${start + index}`, MAIN, 1),
    ),
    start + 1,
  );
}

test("accepting: default-branch caches present, budget below threshold, no pull-request Rust caches", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, true, formatReport(result));
  assert.equal(formatReport(result), PASS_REPORT);
  assert.deepEqual(result.findings, []);
  assert.deepEqual(result.informational, [
    {
      code: "generation-coexistence",
      message:
        "5 cache entries observed; 0 generations beyond the newest per {sharedKey, platform} pair on `refs/heads/main` (0.00 GiB) -- diagnostic visibility only, not subtracted from the budget judgment below.",
    },
  ]);
});

// Issue #926, measured 2026-09-04. Exact bytes from `gh cache list`: a
// healthy single-generation-per-key state (matching `healthyListing()`
// above, current hash suffixes) plus `rust-native-z3` holding two
// generations on each platform at once. The Darwin pair's root cause was
// independently confirmed by comparing both runs' own `Swatinem/rust-cache`
// restore logs (docs/DESIGN-ci.md, "Generation coexistence (issue #926,
// measured 2026-09-04)"); the Windows_NT pair is real `gh cache list` data
// showing the same two-generation shape, but its own CI logs were not
// separately diffed, so this fixture does not claim the identical mechanism
// was independently confirmed for that platform too.
function generationCoexistenceListing() {
  return [
    cacheBytes("v0-rust-semantic-mutation-Linux-x64-0b9fd15e-9efe1eb7", MAIN, 2_918_976_572),
    cacheBytes("v0-rust-rust-workspace-Linux-x64-0b9fd15e-9efe1eb7", MAIN, 1_606_856_511),
    cacheBytes("v0-rust-wasm-Linux-x64-0b9fd15e-9efe1eb7", MAIN, 1_451_597_557),
    // Older generation of each, created first.
    cacheBytes(
      "v0-rust-rust-native-z3-Darwin-arm64-cf75cde1-9efe1eb7",
      MAIN,
      1_240_460_598,
      "2026-08-27T11:08:31Z",
    ),
    cacheBytes(
      "v0-rust-rust-native-z3-Windows_NT-x64-e61e1838-9efe1eb7",
      MAIN,
      620_172_089,
      "2026-08-21T00:00:00Z",
    ),
    // Newer generation of each, created later -- the one Swatinem/rust-cache
    // would actually restore today.
    cacheBytes(
      "v0-rust-rust-native-z3-Darwin-arm64-2bc65c5a-9efe1eb7",
      MAIN,
      1_240_258_986,
      "2026-09-02T21:39:44Z",
    ),
    cacheBytes(
      "v0-rust-rust-native-z3-Windows_NT-x64-368f6b88-9efe1eb7",
      MAIN,
      620_209_508,
      "2026-09-03T21:01:35Z",
    ),
    cacheBytes("node-cache-Linux-x64-npm-541c2caf2ed5378dd8986252a0959abcf8229ef6fd20cc2315b07fb766123eef", MAIN, 12_193_019),
    cacheBytes("node-cache-Linux-x64-npm-e6d58c0aa2ebc53c41327e302ab36a33940d4fdecff9c231e58abfb54fc45d87", MAIN, 12_191_616),
    cacheBytes("Linux-cargo-nextest-0.9.143", MAIN, 9_069_679),
    cacheBytes("Linux-wasm-bindgen-cli-0.2.126", MAIN, 8_443_888),
  ];
}

test("rejecting: physical generation coexistence still fails the budget, with a diagnostic companion", () => {
  // Reverted design decision (review found two executed counterexamples in
  // the earlier controllable-footprint version: a same-identity 5+5 GiB pair
  // physically filling the 10 GiB budget judged as 5 GiB and passing; a
  // higher independently-observed usage total reduced below threshold by
  // subtracting listing-derived stale bytes not proven to be the same bytes
  // the usage endpoint counts). GitHub's budget and eviction act on physical
  // bytes: this fixture's raw total (9.07 GiB / 91%, the same shape the live
  // 2026-09-04 audit reported) must still fail, exactly as it did before any
  // #926 change, with the generation data now reported as an additional
  // diagnostic rather than a judgment input.
  const caches = generationCoexistenceListing();
  assert.equal(usageOf(caches), 9_740_430_023);
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });

  assert.equal(result.ok, false, formatReport(result));
  const budgetFinding = result.findings.find((f) => f.code === "budget-exhausted");
  assert.ok(budgetFinding, formatReport(result));
  assert.match(budgetFinding.message, /^cache usage is 9\.07 GiB of a 10\.00 GiB limit \(91%\)/);
  const diagnostic = result.findings.find(
    (f) => f.code === "generation-coexistence-partial-explanation",
  );
  assert.ok(diagnostic, formatReport(result));
  assert.match(diagnostic.message, /up to 1\.73 GiB/);
  assert.match(diagnostic.message, /2 generations/);
  assert.deepEqual(result.informational, [
    {
      code: "generation-coexistence",
      message:
        "11 cache entries observed; 2 generations beyond the newest per {sharedKey, platform} pair on `refs/heads/main` (1.73 GiB) -- diagnostic visibility only, not subtracted from the budget judgment below.",
    },
  ]);
});

test("accepting: deleting the superseded generation genuinely reduces physical usage and can restore PASS", () => {
  // The corrected design's other half: unlike the reverted version, deleting
  // a stale generation now *does* change the judged total, because it
  // genuinely shrinks what physically sits in the account -- the same
  // arithmetic GitHub's own eviction performs. This is not a reward for
  // manual deletion (docs/DESIGN-ci.md still records why manual deletion is
  // not the recommended response, per the issue's own main-cache-absent
  // regression), only proof the audit no longer manufactures a discrepancy
  // between what it judges and what physically exists.
  const withStale = generationCoexistenceListing();
  const withoutStale = withStale.filter((entry) => !/-(cf75cde1|e61e1838)-/.test(entry.key));
  assert.equal(withoutStale.length, withStale.length - 2);

  const resultWithStale = auditCacheBudget({ caches: withStale, usageBytes: usageOf(withStale) });
  const resultWithoutStale = auditCacheBudget({
    caches: withoutStale,
    usageBytes: usageOf(withoutStale),
  });

  assert.equal(resultWithStale.ok, false, formatReport(resultWithStale));
  assert.equal(resultWithoutStale.ok, true, formatReport(resultWithoutStale));
  assert.deepEqual(resultWithoutStale.findings, []);
});

test("rejecting: genuine growth is not hidden by generation-coexistence reporting", () => {
  // Grow the *kept* (newest) semantic-mutation generation alone -- a shape
  // with no stale-generation explanation at all -- and confirm
  // budget-exhausted fires on the raw total with no diagnostic companion
  // (since staleGenerationBytes for this key's group is 0; the z3 pairs
  // still contribute their own diagnostic).
  const caches = generationCoexistenceListing().map((entry) =>
    entry.key.includes("-semantic-mutation-")
      ? { ...entry, size_in_bytes: entry.size_in_bytes + 2_500_000_000 }
      : entry,
  );
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });

  assert.equal(result.ok, false, formatReport(result));
  const finding = result.findings.find((f) => f.code === "budget-exhausted");
  assert.ok(finding, formatReport(result));
  assert.match(finding.message, /^cache usage is/);
});

test("rejecting: two same-identity generations physically filling the budget must not pass (review counterexample 1)", () => {
  // Executed counterexample from independent review of the reverted
  // controllable-footprint design: two generations sharing one
  // {sharedKey, platform} pair, 5 GiB each, physically occupy the entire
  // 10 GiB budget. The reverted design judged only the newest generation
  // (5 GiB, 50%) and passed; GitHub's own budget and eviction act on both
  // physical blobs regardless of which one this audit calls "current."
  const caches = [
    cacheBytes("v0-rust-rust-workspace-Linux-x64-aaaaaaaa-11111111", MAIN, 5 * BYTES_PER_GIB, "2026-01-01T00:00:00Z"),
    cacheBytes("v0-rust-rust-workspace-Linux-x64-bbbbbbbb-11111111", MAIN, 5 * BYTES_PER_GIB, "2026-01-02T00:00:00Z"),
  ];
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });

  const finding = result.findings.find((f) => f.code === "budget-exhausted");
  assert.ok(finding, formatReport(result));
  assert.match(finding.message, /^cache usage is 10\.00 GiB of a 10\.00 GiB limit \(100%\)/);
});

test("rejecting: a listing sum lower than the independently-observed usage must not be reduced below it (review counterexample 2)", () => {
  // Executed counterexample from independent review: the cache-usage
  // endpoint and the cache listing are separate, non-atomic observations
  // (the existing max(usageBytes, rawSummed) protects against the listing
  // under-reporting a real higher usage). The reverted design subtracted
  // listing-derived "stale" bytes from that already-higher usage total,
  // assuming the two observations shared the same bytes -- an assumption
  // this audit has no basis for. A usage total of 10 GiB must still fail
  // even when the listing itself sums to far less and appears to explain
  // some of the gap via a stale generation.
  const caches = [
    cacheBytes("v0-rust-rust-native-z3-Darwin-arm64-aaaa0000-11111111", MAIN, 2 * BYTES_PER_GIB, "2026-01-01T00:00:00Z"),
    cacheBytes("v0-rust-rust-native-z3-Darwin-arm64-bbbb1111-11111111", MAIN, 2 * BYTES_PER_GIB, "2026-01-02T00:00:00Z"),
  ];
  const result = auditCacheBudget({ caches, usageBytes: 10 * BYTES_PER_GIB });

  const finding = result.findings.find((f) => f.code === "budget-exhausted");
  assert.ok(finding, formatReport(result));
  assert.match(finding.message, /^cache usage is 10\.00 GiB of a 10\.00 GiB limit \(100%\)/);
});

test("rejecting: de-duplication is scoped to the default branch, not to identity alone", () => {
  // A wrong implementation that groups by {sharedKey, platform} regardless of
  // ref would let a genuine pull-request-scoped duplicate of a `ci.yml`
  // shared key hide behind a main-branch generation of the same identity --
  // whether by tripping rule 3 or, worse, by silently excluding its bytes
  // from the raw total the way the reverted controllable-footprint design
  // once excluded a stale main-branch generation. Both are asserted here:
  // the finding still fires, and the reported total is the *full* sum
  // including this entry's bytes, not a value with anything subtracted.
  const base = generationCoexistenceListing();
  const prScopedBytes = 1_606_856_511;
  const caches = [
    ...base,
    cacheBytes("v0-rust-rust-workspace-Linux-x64-0b9fd15e-9efe1eb7", "refs/pull/9/merge", prScopedBytes),
  ];
  const expectedTotal = usageOf(base) + prScopedBytes;
  assert.equal(usageOf(caches), expectedTotal);

  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });

  assert.equal(
    result.findings.some((f) => f.code === "pull-request-cache-present"),
    true,
    formatReport(result),
  );
  // The full total (11.57 GiB) exceeds the 10 GiB limit outright, so this
  // also exercises budget-exhausted with an over-100% listing: proof the
  // PR-scoped entry's bytes were not silently excluded from judgment.
  const budgetFinding = result.findings.find((f) => f.code === "budget-exhausted");
  assert.ok(budgetFinding, formatReport(result));
  const expectedGiB = (expectedTotal / BYTES_PER_GIB).toFixed(2);
  assert.match(budgetFinding.message, new RegExp(`^cache usage is ${expectedGiB.replace(".", "\\.")} GiB `));
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
  const firstPage = withCacheIds(
    [
      ...healthyListing(),
      ...Array.from({ length: 95 }, (_, index) =>
        cache(`tool-cache-${index}`, MAIN, 0.001),
      ),
    ],
    1,
  );
  const secondPage = withCacheIds(
    [
      cache("v0-rust-rust-compile-Linux-x64-aaaaaaaa-bbbbbbbb", "refs/pull/9/merge", 0.08),
    ],
    101,
  );
  const requested = [];
  const caches = await fetchAllCaches(async (path) => {
    requested.push(path);
    if (path.endsWith("page=1")) {
      return { total_count: 101, actions_caches: firstPage };
    }
    if (path.endsWith("page=2")) {
      return { total_count: 101, actions_caches: secondPage };
    }
    if (path.endsWith("page=3")) {
      return outOfRangeCachePage();
    }
    throw new Error(`unexpected path: ${path}`);
  }, 101);

  assert.deepEqual(requested, [
    PAGE_PATH(1),
    PAGE_PATH(2),
    PAGE_PATH(3),
  ]);
  assert.equal(caches.length, 101);
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  assert.ok(
    result.findings.some((finding) => finding.code === "pull-request-rust-cache-present"),
    formatReport(result),
  );
});

test("accepting: a 101-entry created_at-ordered collection is complete without usage-count equality", async () => {
  const firstPage = withCacheIds(
    [
      ...healthyListing(),
      ...Array.from({ length: 95 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001)),
    ],
    1,
  );
  const secondPage = withCacheIds([cache("tool-cache-100", MAIN, 0.001)], 101);
  const requested = [];

  const caches = await fetchAllCaches(async (path) => {
    requested.push(path);
    if (path.endsWith("page=1")) return { total_count: 101, actions_caches: firstPage };
    if (path.endsWith("page=2")) return { total_count: 101, actions_caches: secondPage };
    if (path.endsWith("page=3")) return outOfRangeCachePage();
    throw new Error(`unexpected path: ${path}`);
  }, 101);

  assert.deepEqual(requested, [
    PAGE_PATH(1),
    PAGE_PATH(2),
    PAGE_PATH(3),
  ]);
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, true, formatReport(result));
});

test("accepting: an empty repository reads page one and its zero-entry sentinel", async () => {
  const requested = [];
  const caches = await fetchAllCaches(async (path) => {
    requested.push(path);
    if (path.endsWith("page=1") || path.endsWith("page=2")) {
      return outOfRangeCachePage();
    }
    throw new Error(`unexpected path: ${path}`);
  }, 0);

  assert.deepEqual(caches, []);
  assert.deepEqual(requested, [
    PAGE_PATH(1),
    PAGE_PATH(2),
  ]);
});

test("accepting: a sentinel may repeat the initial count with no entries", async () => {
  const entries = withCacheIds(healthyListing(), 1);
  const caches = await fetchAllCaches(async (path) => {
    if (path.endsWith("page=1")) return { total_count: 5, actions_caches: entries };
    if (path.endsWith("page=2")) return { total_count: 5, actions_caches: [] };
    throw new Error(`unexpected path: ${path}`);
  });

  assert.deepEqual(caches, entries);
});

test("accepting: a stale usage count does not invalidate stable listing identity", async () => {
  // GitHub documents active_caches_count as approximately five-minute data.
  // This models usage=9 while both immediate complete listings contain 5;
  // bytes remain available to the conservative pure audit, but count is not
  // passed to or compared by the completeness collector.
  const usage = { active_caches_count: 9, active_caches_size_in_bytes: 0 };
  const entries = withCacheIds(healthyListing(), 1);
  let collection = 0;
  const caches = await fetchStableCaches(async (path) => {
    if (path.endsWith("page=1")) {
      collection += 1;
      return { total_count: 5, actions_caches: entries };
    }
    if (path.endsWith("page=2")) return outOfRangeCachePage();
    throw new Error(`unexpected path: ${path}`);
  });

  assert.equal(usage.active_caches_count, 9);
  assert.equal(observedUsageBytes(usage), 0);
  assert.equal(collection, 2);
  assert.deepEqual(caches, entries);
});

test("accepting: a page-boundary mixed collection retries to a stable observation", async () => {
  // This uses the API's two 100-entry pages. The first collection has page one
  // from the old 200-entry set, then loses the required entry and appends a new
  // entry before page two. It therefore contains the deleted ID, the new ID,
  // and lacks the boundary ID -- a real mixed collection, not merely two
  // whole snapshots selected per request.
  const oldEntries = withCacheIds(
    [
      ...healthyListing(),
      ...Array.from({ length: 195 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001)),
    ],
    1,
  );
  const removedId = oldEntries.find((entry) => entry.key.includes("-semantic-mutation-")).id;
  const changedEntries = [
    ...oldEntries.filter((entry) => entry.id !== removedId),
    {
      ...oldEntries[0],
      id: 201,
      key: "v0-rust-rust-compile-Linux-x64-aaaaaaaa-bbbbbbbb",
      ref: "refs/pull/9/merge",
    },
  ];
  const mixedEntries = [
    ...oldEntries.slice(0, 100),
    ...changedEntries.slice(100),
  ];
  assert.ok(mixedEntries.some((entry) => entry.id === removedId));
  assert.ok(mixedEntries.some((entry) => entry.id === 201));
  assert.equal(mixedEntries.some((entry) => entry.id === 101), false);

  // `mixedEntries` is the actual first collection served below. Keeping the
  // asserted fixture and the page response on this same value means either
  // removing the new ID or replacing its page two with old data fails here.
  const snapshots = [mixedEntries, changedEntries, changedEntries, changedEntries];
  let collection = -1;
  const requested = [];
  const caches = await fetchStableCaches(
    async (path) => {
      requested.push(path);
      if (path.endsWith("page=1")) {
        collection += 1;
        return {
          total_count: 200,
          actions_caches: snapshots[collection].slice(0, 100),
        };
      }
      if (path.endsWith("page=2")) {
        return {
          total_count: 200,
          actions_caches: snapshots[collection].slice(100),
        };
      }
      if (path.endsWith("page=3")) return outOfRangeCachePage();
      throw new Error(`unexpected path: ${path}`);
    },
    { sleep: async () => {} },
  );

  assert.equal(collection, 3);
  assert.equal(requested.length, 12);
  assert.deepEqual(caches, changedEntries);
});

test("rejecting: different page-boundary mixed collections fail closed after retry", async () => {
  const oldEntries = withCacheIds(
    [
      ...healthyListing(),
      ...Array.from({ length: 195 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001)),
    ],
    1,
  );
  const removedId = oldEntries.find((entry) => entry.key.includes("-semantic-mutation-")).id;
  const changedAtFirstPage = [
    ...oldEntries.filter((entry) => entry.id !== removedId),
    { ...oldEntries[0], id: 201, key: "new-cache", ref: MAIN },
  ];
  const secondRemovedId = oldEntries.find((entry) => entry.id === 4).id;
  const changedAtSecondPage = [
    ...oldEntries.filter((entry) => entry.id !== secondRemovedId),
    { ...oldEntries[0], id: 202, key: "other-new-cache", ref: MAIN },
  ];
  const mixedOldThenFirstPageChange = [
    ...oldEntries.slice(0, 100),
    ...changedAtFirstPage.slice(100),
  ];
  const mixedOldThenSecondPageChange = [
    ...oldEntries.slice(0, 100),
    ...changedAtSecondPage.slice(100),
  ];
  // These asserted mixed collections are also the values that the mock pages
  // serve. Replacing page one with a complete changed snapshot invalidates
  // the rejecting control.
  const pagePairs = [
    [mixedOldThenFirstPageChange, mixedOldThenFirstPageChange],
    [mixedOldThenSecondPageChange, mixedOldThenSecondPageChange],
    [mixedOldThenFirstPageChange, mixedOldThenFirstPageChange],
    [mixedOldThenSecondPageChange, mixedOldThenSecondPageChange],
  ];
  for (const [description, pages, deletedId, newId] of [
    ["first", pagePairs[0], removedId, 201],
    ["second", pagePairs[1], secondRemovedId, 202],
  ]) {
    assert.deepEqual(pages[0], pages[1], `${description}: served pages reconstruct one collection`);
    assert.ok(pages[0].some((entry) => entry.id === deletedId), `${description}: deleted ID`);
    assert.ok(pages[0].some((entry) => entry.id === newId), `${description}: new ID`);
    assert.equal(pages[0].some((entry) => entry.id === 101), false, `${description}: missing ID`);
  }
  let collection = -1;

  await assert.rejects(
    fetchStableCaches(
      async (path) => {
        if (path.endsWith("page=1")) {
          collection += 1;
          return { total_count: 200, actions_caches: pagePairs[collection][0].slice(0, 100) };
        }
        if (path.endsWith("page=2")) {
          return { total_count: 200, actions_caches: pagePairs[collection][1].slice(100) };
        }
        if (path.endsWith("page=3")) return outOfRangeCachePage();
        throw new Error(`unexpected path: ${path}`);
      },
      { sleep: async () => {} },
    ),
    /two complete created_at-ordered cache observations disagreed after 2 attempts/,
  );
  assert.equal(collection, 3);
  assert.equal(
    sameCacheCollection(mixedOldThenFirstPageChange, mixedOldThenSecondPageChange),
    false,
  );
});

test("accepting: a transient differing pair waits once, then returns the stable second observation", async () => {
  const oldEntries = withCacheIds(healthyListing(), 1);
  const stableEntries = oldEntries.map((entry) =>
    entry.id === 1 ? { ...entry, size_in_bytes: entry.size_in_bytes + 1 } : entry,
  );
  const snapshots = [oldEntries, stableEntries, stableEntries, stableEntries];
  let collection = -1;
  const sleeps = [];

  const caches = await fetchStableCaches(
    async (path) => {
      if (path.endsWith("page=1")) {
        collection += 1;
        return { total_count: 5, actions_caches: snapshots[collection] };
      }
      if (path.endsWith("page=2")) return outOfRangeCachePage();
      throw new Error(`unexpected path: ${path}`);
    },
    { sleep: async (milliseconds) => sleeps.push(milliseconds) },
  );

  assert.equal(collection, 3);
  assert.deepEqual(caches, stableEntries);
  assert.equal(sameCacheCollection(oldEntries, stableEntries), false);
  assert.deepEqual(sleeps, [1_000]);
});

test("rejecting: default retry timer must resolve before the retry collection begins", async () => {
  const initial = withCacheIds(healthyListing(), 1);
  const changed = initial.map((entry) =>
    entry.id === 1 ? { ...entry, size_in_bytes: entry.size_in_bytes + 1 } : entry,
  );
  const snapshots = [initial, changed, changed, changed];
  const delays = [];
  let collection = -1;
  let resolveTimerCalled;
  let resolveTimer;
  const timerCalled = new Promise((resolve) => {
    resolveTimerCalled = resolve;
  });
  const originalSetTimeout = globalThis.setTimeout;
  globalThis.setTimeout = (callback, milliseconds, ...arguments_) => {
    delays.push(milliseconds);
    resolveTimer = () => callback(...arguments_);
    resolveTimerCalled();
    return {};
  };

  try {
    const stableCaches = fetchStableCaches(async (path) => {
      if (path.endsWith("page=1")) {
        collection += 1;
        return { total_count: 5, actions_caches: snapshots[collection] };
      }
      if (path.endsWith("page=2")) return outOfRangeCachePage();
      throw new Error(`unexpected path: ${path}`);
    });

    await timerCalled;
    assert.equal(
      collection,
      1,
      "the third collection must not begin until the one-second timer resolves",
    );
    resolveTimer();

    const caches = await stableCaches;
    assert.deepEqual(caches, changed);
  } finally {
    globalThis.setTimeout = originalSetTimeout;
  }

  assert.equal(collection, 3);
  assert.deepEqual(delays, [1_000]);
});

test("sameCacheCollection requires equal ID sets in either direction and ignores response order", () => {
  const collection = withCacheIds(healthyListing(), 1);
  const subset = collection.slice(0, -1);

  assert.equal(sameCacheCollection(subset, collection), false);
  assert.equal(sameCacheCollection(collection, subset), false);
  assert.equal(sameCacheCollection(collection, [...collection].reverse()), true);
});

test("rejecting: a tied created_at page boundary that reorders an ID fails closed", async () => {
  // GitHub documents created_at as the primary sort key, not a deterministic
  // secondary order for equal timestamps. If tied entries cross a page boundary
  // between requests, duplicate-ID validation must refuse to call it healthy.
  const entries = withCacheIds(
    Array.from({ length: 200 }, (_, index) => ({
      ...cache(`tied-cache-${index}`, MAIN, 0.001),
      created_at: "2026-08-13T00:00:00Z",
    })),
  );
  const reorderedSecondPage = [entries[99], ...entries.slice(101)];

  await assert.rejects(
    fetchAllCaches(async (path) => {
      if (path.endsWith("page=1")) return { total_count: 200, actions_caches: entries.slice(0, 100) };
      if (path.endsWith("page=2")) return { total_count: 200, actions_caches: reorderedSecondPage };
      throw new Error(`unexpected path: ${path}`);
    }),
    /repeats cache id 100/,
  );
});

test("rejecting: key, ref, and size changes independently prevent a stable collection", async () => {
  const initial = withCacheIds(healthyListing(), 1);
  const cases = [
    ["key", (entry) => ({ ...entry, key: `${entry.key}-changed` })],
    ["ref", (entry) => ({ ...entry, ref: "refs/heads/other" })],
    ["size_in_bytes", (entry) => ({ ...entry, size_in_bytes: entry.size_in_bytes + 1 })],
  ];

  for (const [field, mutate] of cases) {
    const changed = initial.map((entry) => (entry.id === 1 ? mutate(entry) : entry));
    let collection = -1;
    await assert.rejects(
      fetchStableCaches(
        async (path) => {
          if (path.endsWith("page=1")) {
            collection += 1;
            return {
              total_count: 5,
              actions_caches: collection % 2 === 0 ? initial : changed,
            };
          }
          if (path.endsWith("page=2")) return outOfRangeCachePage();
          throw new Error(`unexpected path: ${path}`);
        },
        { sleep: async () => {} },
      ),
      /two complete created_at-ordered cache observations disagreed after 2 attempts/,
      field,
    );
  }
});

test("rejecting: pagination rejects a total_count that changes after page one", async () => {
  const firstPage = withCacheIds(
    Array.from({ length: 100 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001)),
  );
  const secondPage = withCacheIds([cache("tool-cache-100", MAIN, 0.001)], 101);

  await assert.rejects(
    fetchAllCaches(async (path) => {
      if (path.endsWith("page=1")) return { total_count: 101, actions_caches: firstPage };
      if (path.endsWith("page=2")) return { total_count: 102, actions_caches: secondPage };
      throw new Error(`unexpected path: ${path}`);
    }, 101),
    /total_count changed from 101 to 102 on page 2/,
  );
});

test("rejecting: the collection reader rejects an unsafe initial total_count before computing pages", async () => {
  await assert.rejects(
    fetchAllCaches(async () => ({
      total_count: Number.MAX_SAFE_INTEGER + 1,
      actions_caches: [],
    })),
    /cache listing page 1 has no valid total_count/,
  );
});

test("rejecting: pagination rejects a duplicate cache id on a later page", async () => {
  const firstPage = withCacheIds(
    Array.from({ length: 100 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001)),
  );
  const duplicatePage = withCacheIds(
    Array.from({ length: 100 }, (_, index) => cache(`duplicate-cache-${index}`, MAIN, 0.001)),
  );

  await assert.rejects(
    fetchAllCaches(async (path) => {
      if (path.endsWith("page=1")) return { total_count: 200, actions_caches: firstPage };
      if (path.endsWith("page=2")) return { total_count: 200, actions_caches: duplicatePage };
      throw new Error(`unexpected path: ${path}`);
    }, 200),
    /repeats cache id 1/,
  );
});

test("rejecting: pagination rejects a short intermediate page even if a later page exists", async () => {
  const shortFirstPage = withCacheIds(
    Array.from({ length: 99 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001)),
  );
  const requested = [];

  await assert.rejects(
    fetchAllCaches(async (path) => {
      requested.push(path);
      if (path.endsWith("page=1")) return { total_count: 201, actions_caches: shortFirstPage };
      if (path.endsWith("page=2")) {
        throw new Error("the short first page must have been rejected before this continuation");
      }
      throw new Error(`unexpected path: ${path}`);
    }, 201),
    /page 1 has 99 entries; expected 100 from total_count 201/,
  );
  assert.deepEqual(requested, [PAGE_PATH(1)]);
});

test("rejecting: pagination rejects entries beyond the fixed maximum-page capacity", async () => {
  const firstPage = withCacheIds(
    Array.from({ length: 100 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001)),
  );
  const oversizedFinalPage = withCacheIds(
    [cache("tool-cache-100", MAIN, 0.001), cache("tool-cache-101", MAIN, 0.001)],
    101,
  );

  await assert.rejects(
    fetchAllCaches(async (path) => {
      if (path.endsWith("page=1")) return { total_count: 101, actions_caches: firstPage };
      if (path.endsWith("page=2")) return { total_count: 101, actions_caches: oversizedFinalPage };
      throw new Error(`the fixed two-page bound must prevent page ${path}`);
    }, 101),
    /page 2 has 2 entries; expected 1 from total_count 101/,
  );
});

test("rejecting: a nonempty sentinel exposes a later forbidden cache despite an underreported total_count", async () => {
  const firstPage = withCacheIds(healthyListing(), 1);
  const hiddenCache = withCacheIds(
    [cache("v0-rust-rust-compile-Linux-x64-aaaaaaaa-bbbbbbbb", "refs/pull/9/merge", 0.08)],
    6,
  );
  const requested = [];

  await assert.rejects(
    fetchAllCaches(async (path) => {
      requested.push(path);
      if (path.endsWith("page=1")) return { total_count: 5, actions_caches: firstPage };
      if (path.endsWith("page=2")) return { total_count: 5, actions_caches: hiddenCache };
      throw new Error(`unexpected path: ${path}`);
    }, 5),
    /sentinel page 2 has 1 entries beyond total_count 5/,
  );
  assert.deepEqual(requested, [
    PAGE_PATH(1),
    PAGE_PATH(2),
  ]);
});

test("rejecting: a same-count page-boundary replacement cannot hide a forbidden cache", async () => {
  const firstPage = withCacheIds(
    [...healthyListing(), ...Array.from({ length: 95 }, (_, index) => cache(`tool-cache-${index}`, MAIN, 0.001))],
    1,
  );
  const replacement = withCacheIds(
    [cache("v0-rust-rust-compile-Linux-x64-aaaaaaaa-bbbbbbbb", "refs/pull/9/merge", 0.08)],
    101,
  );

  await assert.rejects(
    fetchAllCaches(async (path) => {
      if (path.endsWith("page=1")) return { total_count: 100, actions_caches: firstPage };
      // The repository can remain at 100 active caches while one first-page
      // entry is deleted and this forbidden cache becomes the next page.
      if (path.endsWith("page=2")) return { total_count: 100, actions_caches: replacement };
      throw new Error(`unexpected path: ${path}`);
    }, 100),
    /sentinel page 2 has 1 entries beyond total_count 100/,
  );
});

test("rejecting: a 99,800-entry listing is rejected before a continuation request", async () => {
  const requested = [];

  await assert.rejects(
    fetchAllCaches(async (path) => {
      requested.push(path);
      if (path.endsWith("page=1")) return { total_count: 99_800, actions_caches: [] };
      throw new Error(`the request budget must reject before ${path}`);
    }),
    /total_count 99800 needs up to 3997 requests/,
  );
  assert.deepEqual(requested, [PAGE_PATH(1)]);
});

test("rejecting: 99,900 and 99,901 entries remain above the two-observation budget", () => {
  assert.ok(maximumAuditRequests(99_900) > 900);
  assert.ok(maximumAuditRequests(99_901) > 900);
});

test("accepting: the maximum retry-safe page count fits the unified request budget", () => {
  assert.equal(maximumAuditRequests(22_300), 897);
  assert.ok(maximumAuditRequests(22_301) > 900);
});

test("rejecting: shrinking mock proves legacy PASS predicate and corrected fail-closed headroom", async () => {
  // Before the fix, `maximumAuditRequests(5) - 226` became -217 here, so
  // `5 < -217 + 100` was false and this fixture reached a healthy pure audit.
  // The API wrapper now refuses the small collection's first page because its
  // current control state still needs the collection's sentinel request.
  const errors = [];
  let phase = "large";
  let smallRequests = 0;
  let requests = 0;
  const result = await runCacheBudgetAudit({
    token: "test-token",
    repo: "owner/repo",
    writeReport: () => assert.fail("the shrinking fixture must not report PASS"),
    writeError: (message) => errors.push(message),
    fetchImpl: async (url) => {
      requests += 1;
      const parsed = new URL(url);
      if (parsed.pathname.endsWith("/actions/cache/usage")) {
        return jsonResponse({ active_caches_size_in_bytes: 5 });
      }
      const page = Number(parsed.searchParams.get("page"));
      if (phase === "large") {
        if (page === 224) {
          phase = "small";
          return jsonResponse(outOfRangeCachePage());
        }
        return jsonResponse({
          total_count: 22_300,
          actions_caches: cachePage(22_300, page),
        });
      }
      if (page === 1) {
        return jsonResponse(
          { total_count: 5, actions_caches: withCacheIds(healthyListing()) },
          { remaining: String(5 - smallRequests++) },
        );
      }
      if (page === 2) return jsonResponse(outOfRangeCachePage());
      throw new Error(`unexpected page ${page}`);
    },
  });

  const legacyNeededAfterThisResponse = maximumAuditRequests(5) - 226;
  assert.equal(legacyNeededAfterThisResponse, -217);
  assert.equal(5 < legacyNeededAfterThisResponse + 100, false);
  assert.equal(result.ok, false);
  assert.equal(requests, 226);
  assert.match(
    errors.join("\n"),
    /rate limit has 5 requests remaining; need 1 for this bounded audit plus 100 reserved/,
  );
});

test("rejecting: the live runner fails closed for a 22,301-entry first page", async () => {
  const errors = [];
  const paths = [];
  const result = await runCacheBudgetAudit({
    token: "test-token",
    repo: "owner/repo",
    writeReport: () => assert.fail("an over-budget listing must not report PASS"),
    writeError: (message) => errors.push(message),
    fetchImpl: async (url) => {
      const parsed = new URL(url);
      paths.push(parsed.pathname + parsed.search);
      if (parsed.pathname.endsWith("/actions/cache/usage")) {
        return jsonResponse({ active_caches_size_in_bytes: 0 });
      }
      return jsonResponse({
        total_count: 22_301,
        actions_caches: cachePage(22_301, 1),
      });
    },
  });

  assert.equal(result.ok, false);
  assert.deepEqual(paths, [
    "/repos/owner/repo/actions/cache/usage",
    `/repos/owner/repo${PAGE_PATH(1)}`,
  ]);
  assert.match(errors.join("\n"), /total_count 22301 needs up to 901 requests/);
});

test("rejecting: the live runner reaches the 22,300-entry continuation and reports HTTP failure", async () => {
  const errors = [];
  const paths = [];
  const result = await runCacheBudgetAudit({
    token: "test-token",
    repo: "owner/repo",
    writeReport: () => assert.fail("a failed continuation must not report PASS"),
    writeError: (message) => errors.push(message),
    fetchImpl: async (url) => {
      const parsed = new URL(url);
      paths.push(parsed.pathname + parsed.search);
      if (parsed.pathname.endsWith("/actions/cache/usage")) {
        return jsonResponse({ active_caches_size_in_bytes: 0 });
      }
      if (parsed.searchParams.get("page") === "1") {
        return jsonResponse({
          total_count: 22_300,
          actions_caches: cachePage(22_300, 1),
        });
      }
      return jsonResponse({}, { status: 503, statusText: "Service Unavailable" });
    },
  });

  assert.equal(result.ok, false);
  assert.deepEqual(paths, [
    "/repos/owner/repo/actions/cache/usage",
    `/repos/owner/repo${PAGE_PATH(1)}`,
    `/repos/owner/repo${PAGE_PATH(2)}`,
  ]);
  assert.match(errors.join("\n"), /GET .*page=2 -> 503 Service Unavailable/);
});

test("rejecting: the live runner rejects invalid ordinary-page total_count values", async () => {
  const cases = [
    ["missing", { actions_caches: [] }],
    ["string", { total_count: "5", actions_caches: [] }],
    ["negative", { total_count: -1, actions_caches: [] }],
    ["fractional", { total_count: 0.5, actions_caches: [] }],
    ["unsafe", { total_count: Number.MAX_SAFE_INTEGER + 1, actions_caches: [] }],
  ];

  for (const [description, malformedPage] of cases) {
    const errors = [];
    const result = await runCacheBudgetAudit({
      token: "test-token",
      repo: "owner/repo",
      writeReport: () => assert.fail("a malformed page must not report PASS"),
      writeError: (message) => errors.push(message),
      fetchImpl: async (url) => {
        if (new URL(url).pathname.endsWith("/actions/cache/usage")) {
          return jsonResponse({ active_caches_size_in_bytes: 0 });
        }
        return jsonResponse(malformedPage);
      },
    });

    assert.equal(result.ok, false, description);
    assert.match(
      errors.join("\n"),
      /cache listing page 1 has no valid total_count/,
      description,
    );
  }
});

test("rejecting: malformed ordinary actions_caches envelopes are api-unreadable before continuation", async () => {
  for (const [description, malformedPage] of [
    ["missing", { total_count: 5 }],
    ["non-array", { total_count: 5, actions_caches: {} }],
  ]) {
    const errors = [];
    const cachePaths = [];
    const result = await runCacheBudgetAudit({
      token: "test-token",
      repo: "owner/repo",
      writeReport: () => assert.fail(`${description} listing must not report PASS`),
      writeError: (message) => errors.push(message),
      fetchImpl: async (url) => {
        const parsed = new URL(url);
        if (parsed.pathname.endsWith("/actions/cache/usage")) {
          return jsonResponse({ active_caches_size_in_bytes: 0 });
        }
        cachePaths.push(parsed.pathname.replace("/repos/owner/repo", "") + parsed.search);
        assert.equal(parsed.searchParams.get("page"), "1", `${description}: no continuation request`);
        return jsonResponse(malformedPage);
      },
    });

    assert.equal(result.ok, false, description);
    assert.deepEqual(cachePaths, [PAGE_PATH(1)], description);
    assert.match(
      errors.join("\n"),
      /cache budget audit: FAIL -- api-unreadable: cache listing page 1 has no actions_caches array/,
      description,
    );
  }
});

test("rejecting: the runner requires each credential before any transport call", async () => {
  for (const [description, credentials] of [
    ["token", { token: "", repo: "owner/repo" }],
    ["repository", { token: "test-token", repo: "" }],
  ]) {
    let transportCalls = 0;
    const errors = [];
    const result = await runCacheBudgetAudit({
      ...credentials,
      writeReport: () => assert.fail(`${description}: absent credentials must not report PASS`),
      writeError: (message) => errors.push(message),
      fetchImpl: async () => {
        transportCalls += 1;
        throw new Error("credential guard must reject before transport");
      },
    });

    assert.equal(result.ok, false, description);
    assert.equal(transportCalls, 0, description);
    assert.deepEqual(errors, [
      "cache budget audit: GITHUB_TOKEN and GITHUB_REPOSITORY are required; refusing to report a healthy budget without observing one",
    ]);
  }
});

test("runner returns the same healthy or unhealthy verdict that its report represents", async () => {
  const listing = withCacheIds(healthyListing(), 1);
  for (const [description, usageBytes, expectedOk, expectedReport] of [
    [
      "healthy",
      usageOf(listing),
      true,
      PASS_REPORT,
    ],
    [
      "over budget",
      9_126_805_504,
      false,
      THRESHOLD_REPORT,
    ],
  ]) {
    const { errors, reports, result } = await runStableRunnerListing({ listing, usageBytes });

    assert.equal(result.ok, expectedOk, description);
    assert.equal(reports.length, 1, description);
    assert.equal(reports[0], expectedReport, description);
    assert.deepEqual(errors, [], description);
  }
});

test("executable runner uses its default report sink for healthy and unhealthy verdicts", () => {
  const listing = withCacheIds(healthyListing(), 1);
  for (const [description, usageBytes, expectedStatus, expectedReport] of [
    ["healthy", usageOf(listing), 0, PASS_REPORT],
    ["over budget", 9_126_805_504, 1, THRESHOLD_REPORT],
  ]) {
    const child = runExecutableRunner({ listing, usageBytes });

    assert.equal(child.error, undefined, description);
    assert.equal(child.status, expectedStatus, description);
    assert.equal(child.stdout, `${expectedReport}\n`, description);
    assert.equal(child.stderr, "", description);
  }
});

test("rejecting: the executable runner exits nonzero and uses its default error sink when credentials are absent", () => {
  const environment = childEnvironment();
  delete environment.GITHUB_TOKEN;
  delete environment.GITHUB_REPOSITORY;
  const child = spawnSync(process.execPath, [RUNNER_PATH], {
    encoding: "utf8",
    env: environment,
  });

  assert.equal(child.error, undefined);
  assert.equal(child.status, 1);
  assert.equal(child.stdout, "");
  assert.equal(
    child.stderr,
    "cache budget audit: GITHUB_TOKEN and GITHUB_REPOSITORY are required; refusing to report a healthy budget without observing one\n",
  );
});

test("rejecting: the executable runner never emits PASS for malformed rate-limit header syntax", () => {
  const listing = withCacheIds(healthyListing(), 1);
  for (const remaining of [
    "0x3e8",
    "1e3",
    "+1000",
    "0b1111101000",
    "9007199254740992",
    "9".repeat(1_000),
  ]) {
    const child = runExecutableRunner({ listing, remaining });

    assert.equal(child.error, undefined, remaining);
    assert.equal(child.status, 1, remaining);
    assert.equal(child.stdout, "", remaining);
    assert.doesNotMatch(child.stdout, /cache budget audit: PASS/, remaining);
    assert.match(child.stderr, /no valid x-ratelimit-remaining header/, remaining);
  }
});

test("retrying runner sends exact transport metadata for usage, every listing page, and sentinels", async () => {
  const listing = withCacheIds([
    ...healthyListing(),
    ...Array.from({ length: 96 }, (_, index) =>
      cacheBytes(`tool-cache-${index}`, MAIN, 1),
    ),
  ]);
  const mismatchedListing = listing.map((entry, index) =>
    index === 0 ? { ...entry, size_in_bytes: entry.size_in_bytes + 1 } : entry,
  );
  const calls = [];
  const reports = [];
  const errors = [];
  let collection = 0;
  let collectionListing;
  const result = await runCacheBudgetAudit({
    token: "test-token",
    repo: "owner/repo",
    writeReport: (report) => reports.push(report),
    writeError: (error) => errors.push(error),
    fetchImpl: async (...arguments_) => {
      calls.push(arguments_);
      const parsed = new URL(arguments_[0]);
      if (parsed.pathname.endsWith("/actions/cache/usage")) {
        return jsonResponse({ active_caches_size_in_bytes: usageOf(listing) });
      }
      const page = Number(parsed.searchParams.get("page"));
      if (page === 1) {
        collection += 1;
        // Exercise every permitted retry: each earlier pair disagrees, while
        // the final pair agrees. This count follows the production retry
        // bound, but the URLs and all header values below remain literals.
        collectionListing =
          collection <= 2 * (STABILITY_ATTEMPTS - 1) && collection % 2 === 0
            ? mismatchedListing
            : listing;
      }
      if (page === 1) {
        return jsonResponse({
          total_count: collectionListing.length,
          actions_caches: collectionListing.slice(0, 100),
        });
      }
      if (page === 2) {
        return jsonResponse({
          total_count: collectionListing.length,
          actions_caches: collectionListing.slice(100),
        });
      }
      if (page === 3) return jsonResponse(outOfRangeCachePage());
      throw new Error(`unexpected cache page ${page}`);
    },
  });

  assert.equal(result.ok, true);
  // 101 entries here, not `PASS_REPORT`'s 5: the informational entry count is
  // part of the transport-metadata claim this test makes (every listing
  // entry, real or filler, reaches the audit), so it is asserted against the
  // actual listing length rather than the unrelated fixed constant.
  assert.deepEqual(reports, [
    "cache budget audit: PASS -- budget within threshold, default-branch caches present, no pull-request-scoped Rust caches\n" +
      `  informational/generation-coexistence: ${listing.length} cache entries observed; 0 generations beyond the newest per {sharedKey, platform} pair on \`refs/heads/main\` (0.00 GiB) -- diagnostic visibility only, not subtracted from the budget judgment below.`,
  ]);
  assert.deepEqual(errors, []);

  assert.deepEqual(calls, [
    "/actions/cache/usage",
    ...Array.from(
      { length: STABILITY_ATTEMPTS * 2 },
      () => [PAGE_PATH(1), PAGE_PATH(2), PAGE_PATH(3)],
    ).flat(),
  ].map((path) => [
    `https://api.github.com/repos/owner/repo${path}`,
    {
      headers: {
        authorization: "Bearer test-token",
        accept: "application/vnd.github+json",
        "x-github-api-version": "2022-11-28",
      },
    },
  ]));
});

test("rejecting: API wrapper rejects HTTP failures and missing or invalid rate-limit headers", async () => {
  const failed = createCacheAuditApi({
    token: "test-token",
    repo: "owner/repo",
    fetchImpl: async () => jsonResponse({}, { status: 401, statusText: "Unauthorized" }),
  });
  await assert.rejects(failed.request("/actions/cache/usage"), /401 Unauthorized/);

  for (const remaining of [
    undefined,
    "",
    "not-a-number",
    "-1",
    "1.5",
    "0x3e8",
    "1e3",
    "+1000",
    "0b1111101000",
    "9007199254740992",
    "9".repeat(1_000),
  ]) {
    const api = createCacheAuditApi({
      token: "test-token",
      repo: "owner/repo",
      fetchImpl: async () => jsonResponse({}, { remaining }),
    });
    await assert.rejects(
      api.request("/actions/cache/usage"),
      /no valid x-ratelimit-remaining header/,
      String(remaining),
    );
  }

  const whitespace = createCacheAuditApi({
    token: "test-token",
    repo: "owner/repo",
    fetchImpl: async () => jsonResponse({}, { remaining: " 1000 " }),
  });
  await whitespace.request("/actions/cache/usage");
  assert.equal(whitespace.rateLimitRemaining, 1_000);
});

test("rejecting: API wrapper preserves headroom and calibrates its independent 900-request cap", async () => {
  for (const remaining of ["100", "0"]) {
    let calls = 0;
    const lowRemaining = createCacheAuditApi({
      token: "test-token",
      repo: "owner/repo",
      fetchImpl: async () => {
        calls += 1;
        return jsonResponse({}, { remaining });
      },
    });
    await lowRemaining.request("/actions/cache/usage");
    await assert.rejects(
      lowRemaining.request("/actions/caches?page=1"),
      /refusing to consume 100 reserved/,
      remaining,
    );
    assert.equal(calls, 1, `${remaining}: headroom rejects before transport`);
  }

  let calls = 0;
  const capped = createCacheAuditApi({
    token: "test-token",
    repo: "owner/repo",
    fetchImpl: async () => {
      calls += 1;
      return jsonResponse({}, { remaining: "1000" });
    },
  });
  for (let request = 0; request < 900; request += 1) {
    await capped.request(`/test/${request}`);
  }
  await assert.rejects(capped.request("/test/over"), /consumed its 900-request budget/);
  assert.equal(calls, 900);
});

test("rejecting: a billion-entry count consumes no continuation request", async () => {
  const requested = [];

  await assert.rejects(
    fetchAllCaches(async (path) => {
      requested.push(path);
      if (path.endsWith("page=1")) return { total_count: 1_000_000_000, actions_caches: [] };
      throw new Error(`the request budget must reject before ${path}`);
    }),
    /total_count 1000000000 needs up to 40000005 requests/,
  );
  assert.deepEqual(requested, [PAGE_PATH(1)]);
});

test("rejecting: malformed required cache-entry fields are unreadable through the runner", async () => {
  const validEntries = withCacheIds(healthyListing(), 1);
  const cases = [
    ["missing id", (entry) => {
      const { id, ...withoutId } = entry;
      return withoutId;
    }, /no valid id/],
    ["negative id", (entry) => ({ ...entry, id: -1 }), /no valid id/],
    ["fractional id", (entry) => ({ ...entry, id: 1.5 }), /no valid id/],
    ["infinite id", (entry) => ({ ...entry, id: Number.POSITIVE_INFINITY }), /no valid id/],
    ["not-a-number id", (entry) => ({ ...entry, id: Number.NaN }), /no valid id/],
    ["unsafe id", (entry) => ({ ...entry, id: Number.MAX_SAFE_INTEGER + 1 }), /no valid id/],
    ["missing key", (entry) => {
      const { key, ...withoutKey } = entry;
      return withoutKey;
    }, /no valid key/],
    ["empty key", (entry) => ({ ...entry, key: "" }), /no valid key/],
    ["missing ref", (entry) => {
      const { ref, ...withoutRef } = entry;
      return withoutRef;
    }, /no valid ref/],
    ["empty ref", (entry) => ({ ...entry, ref: "" }), /no valid ref/],
    ["missing size", (entry) => {
      const { size_in_bytes, ...withoutSize } = entry;
      return withoutSize;
    }, /no valid size_in_bytes/],
    ["negative size", (entry) => ({ ...entry, size_in_bytes: -1 }), /no valid size_in_bytes/],
    ["string size", (entry) => ({ ...entry, size_in_bytes: "1" }), /no valid size_in_bytes/],
    ["fractional size", (entry) => ({ ...entry, size_in_bytes: 1.5 }), /no valid size_in_bytes/],
    ["not-a-number size", (entry) => ({ ...entry, size_in_bytes: Number.NaN }), /no valid size_in_bytes/],
    ["infinite size", (entry) => ({ ...entry, size_in_bytes: Number.POSITIVE_INFINITY }), /no valid size_in_bytes/],
    ["unsafe size", (entry) => ({ ...entry, size_in_bytes: Number.MAX_SAFE_INTEGER + 1 }), /no valid size_in_bytes/],
    ["missing created_at", (entry) => {
      const { created_at, ...withoutCreatedAt } = entry;
      return withoutCreatedAt;
    }, /no valid created_at/],
    ["empty created_at", (entry) => ({ ...entry, created_at: "" }), /no valid created_at/],
    ["non-string created_at", (entry) => ({ ...entry, created_at: 0 }), /no valid created_at/],
    ["numeric-string created_at", (entry) => ({ ...entry, created_at: "0" }), /no valid created_at/],
    ["garbage created_at", (entry) => ({ ...entry, created_at: "not-a-date" }), /no valid created_at/],
    // Date.parse alone would accept both of these: it rolls a nonexistent
    // calendar date over to the next valid one instead of rejecting it, and
    // it treats a bare year number as a valid instant.
    ["nonexistent calendar date created_at", (entry) => ({ ...entry, created_at: "2026-02-30T00:00:00Z" }), /no valid created_at/],
    ["missing timezone created_at", (entry) => ({ ...entry, created_at: "2026-01-01T00:00:00" }), /no valid created_at/],
  ];

  for (const [description, mutate, expectedError] of cases) {
    const requested = [];
    const malformedEntries = [...validEntries];
    malformedEntries[0] = mutate(malformedEntries[0]);
    await assert.rejects(
      fetchAllCaches(async (path) => {
        requested.push(path);
        if (path.endsWith("page=1")) return { total_count: 5, actions_caches: malformedEntries };
        throw new Error(`the malformed ${description} entry must reject before ${path}`);
      }, 5),
      expectedError,
      description,
    );
    assert.deepEqual(requested, [PAGE_PATH(1)], description);
  }
});

test("rejecting: malformed sentinel envelopes are unreadable through the runner", async () => {
  const firstPage = withCacheIds(healthyListing(), 1);
  const cases = [
    ["missing actions_caches", { total_count: 0 }, /has no actions_caches array/],
    ["non-array actions_caches", { total_count: 0, actions_caches: {} }, /has no actions_caches array/],
    ["missing total_count", { actions_caches: [] }, /has no valid total_count/],
    ["negative total_count", { total_count: -1, actions_caches: [] }, /has no valid total_count/],
    ["fractional total_count", { total_count: 0.5, actions_caches: [] }, /has no valid total_count/],
    [
      "non-finite total_count",
      { total_count: Number.POSITIVE_INFINITY, actions_caches: [] },
      /has no valid total_count/,
    ],
    ["incompatible total_count", { total_count: 6, actions_caches: [] }, /expected 0 or 5/],
  ];

  for (const [description, sentinel, expectedError] of cases) {
    await assert.rejects(
      fetchAllCaches(async (path) => {
        if (path.endsWith("page=1")) return { total_count: 5, actions_caches: firstPage };
        if (path.endsWith("page=2")) return sentinel;
        throw new Error(`the malformed ${description} sentinel must reject before ${path}`);
      }),
      expectedError,
      description,
    );
  }
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
  const result = auditCacheBudget({ caches, usageBytes: 10_694_468_567 });
  assert.equal(result.ok, false);
  const codes = result.findings.map((finding) => finding.code);
  assert.ok(codes.includes("budget-exhausted"), formatReport(result));
  // Every required {key, platform} pair is missing from the default branch:
  // the three Linux critical-path keys (`rust-workspace`, `wasm`,
  // `semantic-mutation`) plus both native-z3 platforms, absent here entirely
  // rather than merely missing from `main`.
  assert.equal(codes.filter((code) => code === "main-cache-absent").length, 5);
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
  // `rust-compile` is not one of the three declared ci.yml shared keys, so rule 3 alone would miss
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

test("rejecting: main missing semantic-mutation is not hidden by the other Linux entries", () => {
  const caches = healthyListing().filter((entry) => !entry.key.includes("-semantic-mutation-"));
  const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
  assert.equal(result.ok, false);
  const missing = result.findings.filter((finding) => finding.code === "main-cache-absent");
  assert.equal(missing.length, 1, formatReport(result));
  assert.match(missing[0].message, /`semantic-mutation`/);
  assert.match(missing[0].message, /`Linux`/);
});

test("the three declared shared keys each have one intended required main key/platform pair", () => {
  const sharedKeys = ["rust-workspace", "wasm", "semantic-mutation"];
  const requiredMainEntries = [
    { key: "rust-workspace", platform: "Linux" },
    { key: "wasm", platform: "Linux" },
    { key: "semantic-mutation", platform: "Linux" },
    { key: "rust-native-z3", platform: "Windows_NT" },
    { key: "rust-native-z3", platform: "Darwin" },
  ];
  const requiredSharedEntries = requiredMainEntries
    .filter((entry) => sharedKeys.includes(entry.key))
    .sort((left, right) => left.key.localeCompare(right.key));
  assert.deepEqual(requiredSharedEntries, [
    { key: "rust-workspace", platform: "Linux" },
    { key: "semantic-mutation", platform: "Linux" },
    { key: "wasm", platform: "Linux" },
  ]);
  assert.deepEqual(
    [...new Set(requiredSharedEntries.map((entry) => entry.key))].sort(),
    [...sharedKeys].sort(),
  );
});

test("rejecting: every declared shared key is independently required on main", () => {
  for (const key of ["rust-workspace", "wasm", "semantic-mutation"]) {
    const caches = healthyListing().filter((entry) => !entry.key.includes(`-${key}-`));
    const result = auditCacheBudget({ caches, usageBytes: usageOf(caches) });
    const missing = result.findings.filter((finding) => finding.code === "main-cache-absent");
    assert.equal(result.ok, false, key);
    assert.equal(missing.length, 1, `${key}: ${formatReport(result)}`);
    assert.match(missing[0].message, new RegExp(`\\\`${key}\\\``));
    assert.match(missing[0].message, /`Linux`/);
  }
});

test("accepting: cache keys recognize every os.type() platform spelling", () => {
  for (const platform of ["Linux", "Darwin", "Windows_NT"]) {
    const key = `v0-rust-platform-test-${platform}-x64-aaaaaaaa-bbbbbbbb`;
    const result = auditCacheBudget({
      caches: [cacheBytes(key, MAIN, 0)],
      usageBytes: 0,
      requiredMainEntries: [{ key: "platform-test", platform }],
      ciSharedKeys: [],
    });
    assert.equal(result.ok, true, platform);
  }
});

test("rejecting: runner.os and unknown cache-key platforms are never attributed as required entries", () => {
  for (const platform of ["macOS", "Windows", "FreeBSD"]) {
    const result = auditCacheBudget({
      caches: [cacheBytes(`v0-rust-platform-test-${platform}-x64-aaaaaaaa-bbbbbbbb`, MAIN, 0)],
      usageBytes: 0,
      requiredMainEntries: [{ key: "platform-test", platform }],
      ciSharedKeys: [],
    });

    assert.deepEqual(result.findings.map((finding) => finding.code), ["main-cache-absent"], platform);
  }
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
    requiredMainEntries: [
      { key: "rust-workspace", platform: "Linux" },
      { key: "wasm", platform: "Linux" },
      { key: "semantic-mutation", platform: "Linux" },
      { key: "rust-native-z3", platform: "Windows_NT" },
      { key: "rust-native-z3", platform: "Darwin" },
      { key: "foo-Linux-bar", platform: "Linux" },
    ],
  });
  assert.equal(result.ok, true, formatReport(result));
});

test("rejecting: budget at or above the threshold, even with a healthy ref layout", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({
    caches,
    usageBytes: 9_126_805_504,
  });
  assert.equal(result.ok, false);
  assert.ok(result.findings.some((finding) => finding.code === "budget-exhausted"));
});

test("rejecting: usage above the limit reports the overage, never negative remaining capacity", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({
    caches,
    usageBytes: 10_877_004_677,
  });
  const finding = result.findings.find((entry) => entry.code === "budget-exhausted");
  assert.ok(finding);
  assert.match(finding.message, /10\.13 GiB is 0\.13 GiB above the limit/);
  assert.doesNotMatch(finding.message, /-\d+\.\d+ GiB remains/);
});

test("accepting: 8.5 GiB minus one byte is below the threshold", () => {
  const caches = healthyListing();
  const result = auditCacheBudget({
    caches,
    usageBytes: 9_126_805_503,
  });
  assert.equal(
    result.findings.some((finding) => finding.code === "budget-exhausted"),
    false,
    formatReport(result),
  );
});

test("rejecting: a newer over-threshold listing cannot be hidden by an earlier lower usage observation", () => {
  const beforeReplacementUsage = 7_516_192_768;
  const listingAfterSameCountReplacement = healthyListing();
  const replacedIndex = listingAfterSameCountReplacement.findIndex((entry) =>
    entry.key.includes("-semantic-mutation-"),
  );
  listingAfterSameCountReplacement[replacedIndex] = {
    ...listingAfterSameCountReplacement[replacedIndex],
    size_in_bytes: 10_286_446_674 - (usageOf(listingAfterSameCountReplacement) - listingAfterSameCountReplacement[replacedIndex].size_in_bytes),
  };

  // The runner reads usage before listing. A same-count replacement between
  // endpoints can leave the first observation at 7.00 GiB while the later
  // listing totals 9.58 GiB; either observed total must fail the budget.
  assert.equal(usageOf(listingAfterSameCountReplacement), 10_286_446_674);
  const result = auditCacheBudget({
    caches: listingAfterSameCountReplacement,
    usageBytes: beforeReplacementUsage,
  });
  assert.equal(result.ok, false, formatReport(result));
  assert.ok(result.findings.some((finding) => finding.code === "budget-exhausted"));
  assert.match(formatReport(result), /9\.58 GiB/);
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
  const result = auditCacheBudget({ caches, usageBytes: observedUsageBytes({}) });
  assert.equal(result.ok, false);
  assert.ok(result.findings.some((finding) => finding.code === "usage-unobserved"));
});

test("rejecting: malformed or unsafe cache-usage totals cannot be reported as observed usage", () => {
  for (const value of [-1, 1.5, Number.POSITIVE_INFINITY, Number.NaN, Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(
      () => observedUsageBytes({ active_caches_size_in_bytes: value }),
      /no valid active_caches_size_in_bytes/,
      `${value} must be rejected`,
    );
  }
});

test("accepting: a valid stale usage count is not an identity or completeness condition", () => {
  assert.equal(
    observedUsageBytes({ active_caches_size_in_bytes: 0, active_caches_count: 9 }),
    0,
  );
});

test("rejecting: an unsafe cache-usage count is not a valid observation", () => {
  assert.throws(
    () =>
      observedUsageBytes({
        active_caches_size_in_bytes: 0,
        active_caches_count: Number.MAX_SAFE_INTEGER + 1,
      }),
    /no valid active_caches_count/,
  );
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

test("formatReport fixes the complete default FAIL report count, order, and delimiters", () => {
  const result = auditCacheBudget({
    caches: [
      cacheBytes(
        "v0-rust-rust-workspace-Linux-x64-aaaaaaaa-bbbbbbbb",
        "refs/pull/9/merge",
        1_073_741_824,
      ),
    ],
    usageBytes: 9_126_805_504,
  });

  assert.deepEqual(result.findings.map((finding) => finding.code), [
    "budget-exhausted",
    "main-cache-absent",
    "main-cache-absent",
    "main-cache-absent",
    "main-cache-absent",
    "main-cache-absent",
    "pull-request-cache-present",
  ]);
  assert.equal(
    formatReport(result),
    [
      "cache budget audit: FAIL -- 7 finding(s)",
      "  budget-exhausted: cache usage is 8.50 GiB of a 10.00 GiB limit (85%), at or above the 85% threshold. 8.50 GiB is 1.50 GiB remaining before the limit; a sufficiently large save can trigger least-recently-used eviction, including a default-branch cache that a main-targeting pull request depends on.",
      "  main-cache-absent: no `refs/heads/main` cache for shared key `rust-workspace` on platform `Linux`. Actions caches are ref-scoped: a pull request can read its current ref, base branch, and default branch. For a main-targeting pull request those latter two are `refs/heads/main`; without this entry each such pull request (or, for a matrix job, that platform's shard) builds cold.",
      "  main-cache-absent: no `refs/heads/main` cache for shared key `wasm` on platform `Linux`. Actions caches are ref-scoped: a pull request can read its current ref, base branch, and default branch. For a main-targeting pull request those latter two are `refs/heads/main`; without this entry each such pull request (or, for a matrix job, that platform's shard) builds cold.",
      "  main-cache-absent: no `refs/heads/main` cache for shared key `semantic-mutation` on platform `Linux`. Actions caches are ref-scoped: a pull request can read its current ref, base branch, and default branch. For a main-targeting pull request those latter two are `refs/heads/main`; without this entry each such pull request (or, for a matrix job, that platform's shard) builds cold.",
      "  main-cache-absent: no `refs/heads/main` cache for shared key `rust-native-z3` on platform `Windows_NT`. Actions caches are ref-scoped: a pull request can read its current ref, base branch, and default branch. For a main-targeting pull request those latter two are `refs/heads/main`; without this entry each such pull request (or, for a matrix job, that platform's shard) builds cold.",
      "  main-cache-absent: no `refs/heads/main` cache for shared key `rust-native-z3` on platform `Darwin`. Actions caches are ref-scoped: a pull request can read its current ref, base branch, and default branch. For a main-targeting pull request those latter two are `refs/heads/main`; without this entry each such pull request (or, for a matrix job, that platform's shard) builds cold.",
      "  pull-request-cache-present: `refs/pull/9/merge` holds a cache for `ci.yml`'s shared key `rust-workspace` (1.00 GiB). This violates the current no-pull-request-save invariant. It may have been saved before the guard existed or may indicate a later guard regression; inspect created_at and workflow provenance.",
      "  informational/generation-coexistence: 1 cache entry observed; 0 generations beyond the newest per {sharedKey, platform} pair on `refs/heads/main` (0.00 GiB) -- diagnostic visibility only, not subtracted from the budget judgment below.",
    ].join("\n"),
  );
});
