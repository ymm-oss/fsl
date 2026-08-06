// SPDX-License-Identifier: Apache-2.0
//
// Actions cache budget audit (issue #747). GitHub gives a repository 10 GiB of
// Actions cache and evicts least-recently-used entries when it is exceeded.
// Caches are also ref-scoped: a run can restore only its own ref's caches and
// the default branch's, so a pull request's cache is worthless to a sibling
// pull request while still counting against the shared limit.
//
// `ci.yml`'s four shared keys store about 6.9 GiB per ref, so two concurrent
// pull requests exceeded the limit and evicted `main`'s caches. Every run then
// built cold -- measured +8 to +16 min per shard -- and each cold run saved a
// fresh ref-scoped copy, evicting more. `ci.yml` now restricts saving to
// non-pull-request events, which is what this audit protects.
//
// This module is pure: it takes an already-fetched cache listing and returns
// findings. The workflow does the fetching. That split is what makes the
// rejecting fixtures in audit-cache-budget.test.mjs possible, and it is the
// same shape as audit-ruleset-drift.mjs.

export const GIB = 1024 ** 3;

// GitHub's per-repository Actions cache allowance.
export const CACHE_LIMIT_BYTES = 10 * GIB;

// Fraction of the limit at which the budget is already too tight to be safe.
// Not 100%: eviction begins when a *save* would exceed the limit, so a listing
// sitting at 90% is one save away from evicting something load-bearing. The
// observed failure had usage at 9.96 GiB, i.e. 99.6%.
export const BUDGET_WARN_FRACTION = 0.85;

// The shared keys `ci.yml` declares. A `refs/pull/*` entry for any of these
// means the `save-if` guard regressed -- this is the calibrated rejecting
// signal for the fix itself, not merely a hygiene check.
export const CI_SHARED_KEYS = [
  "rust-workspace",
  "wasm",
  "fsl-logic",
  "semantic-mutation",
];

// Keys whose absence on the default branch makes every pull request build cold.
// A subset of CI_SHARED_KEYS: these are the ones on the pre-merge critical path.
export const REQUIRED_MAIN_KEYS = ["rust-workspace", "wasm", "fsl-logic"];

function sharedKeyOf(key) {
  // Swatinem/rust-cache composes `v0-rust-<shared-key>-<platform>-<hashes>`.
  const match = /^v\d+-rust-(.+?)-(?:Linux|macOS|Windows)-/.exec(key ?? "");
  return match ? match[1] : null;
}

function formatGiB(bytes) {
  return `${(bytes / GIB).toFixed(2)} GiB`;
}

/**
 * @param {{caches: Array<{key: string, ref: string, size_in_bytes: number}>,
 *          usageBytes: number|null,
 *          limitBytes?: number,
 *          warnFraction?: number,
 *          requiredMainKeys?: string[],
 *          ciSharedKeys?: string[],
 *          defaultBranchRef?: string}} input
 * @returns {{findings: Array<{code: string, message: string}>, ok: boolean}}
 */
export function auditCacheBudget({
  caches,
  usageBytes,
  limitBytes = CACHE_LIMIT_BYTES,
  warnFraction = BUDGET_WARN_FRACTION,
  requiredMainKeys = REQUIRED_MAIN_KEYS,
  ciSharedKeys = CI_SHARED_KEYS,
  defaultBranchRef = "refs/heads/main",
}) {
  const findings = [];

  if (!Array.isArray(caches)) {
    findings.push({
      code: "listing-unreadable",
      message:
        "the cache listing is absent or not an array; an unreadable listing is never read as an empty (healthy) cache set",
    });
    return { findings, ok: false };
  }

  // 1. Budget headroom. Prefer the API's own usage figure when present; fall
  //    back to summing the listing, which undercounts if it was paginated.
  const summed = caches.reduce((total, entry) => total + (entry.size_in_bytes ?? 0), 0);
  const effective = typeof usageBytes === "number" ? usageBytes : summed;
  if (typeof usageBytes !== "number") {
    findings.push({
      code: "usage-unobserved",
      message:
        "the repository cache-usage endpoint returned no total; falling back to the sum of the listing, which undercounts when paginated. Absence of the total is not evidence of headroom.",
    });
  }
  if (effective >= limitBytes * warnFraction) {
    findings.push({
      code: "budget-exhausted",
      message: `cache usage is ${formatGiB(effective)} of a ${formatGiB(limitBytes)} limit (${Math.round(
        (effective / limitBytes) * 100,
      )}%), at or above the ${Math.round(warnFraction * 100)}% threshold. At this level a single save evicts a least-recently-used entry, and the default branch's caches are the ones every pull request depends on.`,
    });
  }

  // 2. The default branch must hold every key on the pre-merge critical path.
  const mainKeys = new Set(
    caches
      .filter((entry) => entry.ref === defaultBranchRef)
      .map((entry) => sharedKeyOf(entry.key))
      .filter(Boolean),
  );
  for (const key of requiredMainKeys) {
    if (!mainKeys.has(key)) {
      findings.push({
        code: "main-cache-absent",
        message: `no \`${defaultBranchRef}\` cache for shared key \`${key}\`. Actions caches are ref-scoped, so the default branch is the only cache every pull request can read; without it each pull request builds cold.`,
      });
    }
  }

  // 3. No pull-request-scoped cache for a `ci.yml` shared key. This is the
  //    rejecting control for `save-if`: if the guard is removed, these reappear.
  const ciKeys = new Set(ciSharedKeys);
  for (const entry of caches) {
    if (!/^refs\/pull\//.test(entry.ref ?? "")) continue;
    const key = sharedKeyOf(entry.key);
    if (key && ciKeys.has(key)) {
      findings.push({
        code: "pull-request-cache-present",
        message: `\`${entry.ref}\` holds a cache for \`ci.yml\`'s shared key \`${key}\` (${formatGiB(
          entry.size_in_bytes ?? 0,
        )}). \`ci.yml\` restricts saving to non-pull-request events precisely so this cannot happen; its presence means that guard regressed.`,
      });
    }
  }

  return { findings, ok: findings.length === 0 };
}

export function formatReport({ findings, ok }) {
  if (ok) return "cache budget audit: PASS -- budget within threshold, default-branch caches present, no pull-request-scoped ci.yml caches";
  return [
    `cache budget audit: FAIL -- ${findings.length} finding(s)`,
    ...findings.map((finding) => `  ${finding.code}: ${finding.message}`),
  ].join("\n");
}
