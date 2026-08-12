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
//
// `merge-readiness.yml` is restore-only against `ci.yml`'s own `rust-workspace`
// key (`save-if: false`) and therefore owns no shared key of its own to add
// here. As of that fix, no workflow in this repository ever saves a rust
// cache on a pull-request event -- rule 4 below is the general form of that
// invariant, covering any future workflow's rust-cache step as well as the
// four keys `ci.yml` declares.

export const GIB = 1024 ** 3;

// GitHub's per-repository Actions cache allowance.
export const CACHE_LIMIT_BYTES = 10 * GIB;

// Fraction of the limit at which the budget is already too tight to be safe.
// Not 100%: eviction begins when a *save* would exceed the limit, so a listing
// sitting at 90% is one save away from evicting something load-bearing. The
// observed failure had usage at 9.96 GiB, i.e. 99.6%.
export const BUDGET_WARN_FRACTION = 0.85;

// A single entry above this size is a standing risk to the whole budget
// regardless of the total, so it gets its own control rather than waiting for
// `BUDGET_WARN_FRACTION` to trip. Calibrated between the `semantic-mutation`
// key's designed floor (~2.2 GiB, predicted: two build trees --
// `rust/target/debug`'s deps and `rust/target/fault-operators/target`'s deps,
// both from `~/.cargo` -- structurally larger than any single-tree shared key
// such as `rust-workspace` at 1.495 GiB) and the defect value this key was
// actually observed holding, 2.719 GiB, once dead weight from a scratch build
// tree and stale evidence directories accumulated inside the cached path
// (fixed elsewhere in this branch). This is an added control, not a
// replacement for `BUDGET_WARN_FRACTION`, and does not change that threshold.
export const SINGLE_ENTRY_WARN_BYTES = 2.5 * GIB;

// The shared keys `ci.yml` declares. A `refs/pull/*` entry for any of these
// means the `save-if` guard regressed -- this is the calibrated rejecting
// signal for the fix itself, not merely a hygiene check.
export const CI_SHARED_KEYS = [
  "rust-workspace",
  "wasm",
  "fsl-logic",
  "semantic-mutation",
];

// Entries whose absence on the default branch makes every pull request build
// cold. Per-platform, not just per-key: `rust-native-z3` is a single shared
// key across a `[macos-15, windows-latest]` matrix (`ci.yml`), so a key-only
// set would let the Darwin entry's presence hide a missing Windows_NT one --
// exactly the failure this audit never reported when the Windows cache was
// evicted to zero (issue #747). `rust-workspace`/`wasm`/`fsl-logic` only ever
// run on `ubuntu-latest`, hence `Linux` for all three.
export const REQUIRED_MAIN_ENTRIES = [
  { key: "rust-workspace", platform: "Linux" },
  { key: "wasm", platform: "Linux" },
  { key: "fsl-logic", platform: "Linux" },
  { key: "rust-native-z3", platform: "Windows_NT" },
  { key: "rust-native-z3", platform: "Darwin" },
];

function entryIdentity(key) {
  // Swatinem/rust-cache composes `v0-rust-<shared-key>-<platform>-<arch>-<hash>-<hash>`.
  // Parsed from the tail, not the head: a lazy `(.+?)` stops at the *first*
  // platform-like substring it finds, so a shared key that happens to contain
  // one -- e.g. a hypothetical `foo-Linux-bar` -- would misparse as shared key
  // `foo`, and a reviewer reproduced this causing a real, present main-branch
  // entry to be reported `main-cache-absent` even though its cache existed in
  // the same listing. A greedy `(.+)` anchored at `$` instead finds the *last*
  // occurrence of the real trailing structure, which is the one
  // `Swatinem/rust-cache` actually appends. `platform` is `os.type()`'s actual
  // output, `Linux`/`Darwin`/`Windows_NT` -- never the GitHub Actions
  // `runner.os` spellings `Linux`/`macOS`/`Windows`. `Windows_NT` is tried
  // before the bare `Windows` it is a superset of, though the trailing anchor
  // below no longer strictly depends on that ordering to be correct.
  const match = /^v\d+-rust-(.+)-(Linux|Darwin|Windows_NT|macOS|Windows)-[^-]+-[0-9a-f]+-[0-9a-f]+$/.exec(
    key ?? "",
  );
  return match ? { sharedKey: match[1], platform: match[2] } : { sharedKey: null, platform: null };
}

function formatGiB(bytes) {
  return `${(bytes / GIB).toFixed(2)} GiB`;
}

/**
 * @param {{caches: Array<{key: string, ref: string, size_in_bytes: number}>,
 *          usageBytes: number|null,
 *          limitBytes?: number,
 *          warnFraction?: number,
 *          requiredMainEntries?: {key: string, platform: string}[],
 *          ciSharedKeys?: string[],
 *          defaultBranchRef?: string,
 *          singleEntryWarnBytes?: number}} input
 * @returns {{findings: Array<{code: string, message: string}>, ok: boolean}}
 */
export function auditCacheBudget({
  caches,
  usageBytes,
  limitBytes = CACHE_LIMIT_BYTES,
  warnFraction = BUDGET_WARN_FRACTION,
  requiredMainEntries = REQUIRED_MAIN_ENTRIES,
  ciSharedKeys = CI_SHARED_KEYS,
  defaultBranchRef = "refs/heads/main",
  singleEntryWarnBytes = SINGLE_ENTRY_WARN_BYTES,
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

  // 2. The default branch must hold every {key, platform} pair on the
  //    pre-merge critical path. Per-platform, not per-key: a shared key backed
  //    by a matrix job (`rust-native-z3`) can have one platform's entry
  //    present and another's absent, and a key-only set would let the former
  //    hide the latter.
  const mainEntries = new Set(
    caches
      .filter((entry) => entry.ref === defaultBranchRef)
      .map((entry) => entryIdentity(entry.key))
      .filter(({ sharedKey, platform }) => sharedKey && platform)
      .map(({ sharedKey, platform }) => `${sharedKey}::${platform}`),
  );
  for (const { key, platform } of requiredMainEntries) {
    if (!mainEntries.has(`${key}::${platform}`)) {
      findings.push({
        code: "main-cache-absent",
        message: `no \`${defaultBranchRef}\` cache for shared key \`${key}\` on platform \`${platform}\`. Actions caches are ref-scoped, so the default branch is the only cache every pull request can read; without it each pull request (or, for a matrix job, that platform's shard) builds cold.`,
      });
    }
  }

  // 3. No pull-request-scoped cache for a `ci.yml` shared key. This is the
  //    rejecting control for `save-if`: if the guard is removed, these reappear.
  const ciKeys = new Set(ciSharedKeys);
  const attributedPullRequestEntries = new Set();
  for (const entry of caches) {
    if (!/^refs\/pull\//.test(entry.ref ?? "")) continue;
    const { sharedKey } = entryIdentity(entry.key);
    if (sharedKey && ciKeys.has(sharedKey)) {
      attributedPullRequestEntries.add(entry);
      findings.push({
        code: "pull-request-cache-present",
        message: `\`${entry.ref}\` holds a cache for \`ci.yml\`'s shared key \`${sharedKey}\` (${formatGiB(
          entry.size_in_bytes ?? 0,
        )}). \`ci.yml\` restricts saving to non-pull-request events precisely so this cannot happen; its presence means that guard regressed.`,
      });
    }
  }

  // 4. Repository invariant since #752 and the `merge-readiness.yml`
  //    restore-only fix: no workflow saves a rust cache on a pull-request
  //    event, full stop -- not just the four keys `ci.yml` declares. Any
  //    `v0-rust-*` key on a `refs/pull/*` ref means that invariant broke
  //    somewhere, whether in a known shared key (already reported by rule 3
  //    above, and skipped here to avoid a duplicate finding) or a new one.
  for (const entry of caches) {
    if (attributedPullRequestEntries.has(entry)) continue;
    if (!/^refs\/pull\//.test(entry.ref ?? "")) continue;
    if (!/^v\d+-rust-/.test(entry.key ?? "")) continue;
    findings.push({
      code: "pull-request-rust-cache-present",
      message: `\`${entry.ref}\` holds a rust cache \`${entry.key}\` (${formatGiB(
        entry.size_in_bytes ?? 0,
      )}). No workflow may save a rust cache on a pull-request event; see docs/DESIGN-ci.md, "Actions cache budget".`,
    });
  }

  // 5. No single entry should approach the whole repository's budget by
  //    itself. This is independent of rules 1-4: a key can be entirely
  //    legitimate (declared, present only where expected, never on a
  //    pull-request ref) and still be silently regrowing into the same
  //    accumulation failure `semantic-mutation` had, before `budget-exhausted`
  //    would ever trip.
  for (const entry of caches) {
    const size = entry.size_in_bytes ?? 0;
    if (size < singleEntryWarnBytes) continue;
    findings.push({
      code: "entry-oversized",
      message: `\`${entry.key}\` on \`${entry.ref}\` is ${formatGiB(size)}, at or above the ${formatGiB(
        singleEntryWarnBytes,
      )} single-entry warning threshold. A key this size is a standing risk to the shared budget on its own, independent of the total.`,
    });
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
