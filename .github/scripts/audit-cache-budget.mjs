// SPDX-License-Identifier: Apache-2.0
//
// Actions cache budget audit (issue #747). GitHub gives a repository 10 GiB of
// Actions cache and evicts least-recently-used entries when it is exceeded.
// Caches are also ref-scoped: a run can restore its current ref's caches, its
// base branch's caches, and the default branch's caches. For a pull request to
// main, base and default are both main, so a sibling PR's cache remains
// unusable while still counting against the shared limit.
//
// `ci.yml`'s shared keys (originally four; `fsl-logic` was later folded into
// `rust-workspace`, see below) stored about 6.9 GiB per ref, so two concurrent
// pull requests exceeded the limit and evicted `main`'s caches. Every run then
// built cold -- measured +8 to +16 min per shard -- and each cold run saved a
// fresh ref-scoped copy, evicting more. Every step that still saves its own
// key now restricts that save to non-pull-request events; a step that never
// saves at all is restore-only (`save-if: false`) instead (see below). Rules 3
// and 4 below are what this audit uses to protect both shapes.
//
// This module is pure: it takes an already-fetched cache listing and returns
// findings. The workflow does the fetching. That split is what makes the
// rejecting fixtures in audit-cache-budget.test.mjs possible, and it is the
// same shape as audit-ruleset-drift.mjs.
//
// `merge-readiness.yml`'s `rust-compile` job, `merge-readiness.yml`'s
// `core-contracts` job, and `ci.yml`'s own `fsl-logic` job are each
// restore-only against `ci.yml`'s `rust-workspace` key (`save-if: false`) and
// therefore own no shared key of their own to add here. `ci.yml`'s
// `semantic-mutation-mutants` job is also restore-only (`save-if: false`),
// but reads `semantic-mutation` instead -- the key `semantic-mutation-operators`
// owns and saves, not `rust-workspace`. As of the `merge-readiness.yml` fix,
// no workflow in this repository ever saves a rust cache on a pull-request
// event -- rule 4 below is the general form of that invariant, covering any
// future workflow's rust-cache step as well as the keys `CI_SHARED_KEYS`
// declares.

export const GIB = 1024 ** 3;

// GitHub's per-repository Actions cache allowance.
export const CACHE_LIMIT_BYTES = 10 * GIB;

// Fraction of the limit at which the budget is already too tight to be safe.
// Not 100%: eviction begins when a *save* would exceed the limit, so a listing
// sitting at 90% is one save away from evicting something load-bearing. The
// observed failure had usage at 9.96 GiB, i.e. 99.6%.
export const BUDGET_WARN_FRACTION = 0.85;

// The three identities rule 3 below diagnoses by name: `rust-workspace`,
// `wasm`, `semantic-mutation`. This list is not derived from any single
// mechanism -- the three do not share a common origin (how each key is
// declared, whether it is "shared" in the same technical sense, or any other
// generation rule); it is simply the enumerated set rule 3 checks. Do not
// infer a rule for membership from the list's current contents. A
// `refs/pull/*` entry for one of these three is a violation of the current
// invariant; it may predate the guard or indicate a later guard regression, so
// inspect `created_at` and workflow provenance. This is the calibrated
// rejecting signal for the fix itself, not merely a hygiene check.
//
// Every other `v0-rust-*` key, including `rust-native-z3` and `fsl-logic`, is
// still covered against appearing on a `refs/pull/*` ref -- by rule 4 below,
// which matches on the raw key prefix and does not consult this list at all.
// What is *not* covered for a key outside this list is rule 2's default-branch
// presence requirement, which checks only the `{key, platform}` pairs in
// `REQUIRED_MAIN_ENTRIES`: `rust-native-z3` is in that list (so its absence
// from `main` is still caught); `fsl-logic` is not (it went restore-only and
// never saves a `main` copy of its own to require).
export const CI_SHARED_KEYS = [
  "rust-workspace",
  "wasm",
  "semantic-mutation",
];

// Entries whose absence on the default branch makes every pull request build
// cold. Per-platform, not just per-key: `rust-native-z3` is a single shared
// key across a `[macos-15, windows-latest]` matrix (`ci.yml`), so a key-only
// set would let the Darwin entry's presence hide a missing Windows_NT one --
// exactly the failure this audit never reported when the Windows cache was
// evicted to zero (issue #747). `rust-workspace`/`wasm`/`semantic-mutation`
// only ever run on `ubuntu-latest`, hence `Linux` for all three. `fsl-logic`
// is deliberately absent: it is restore-only against `rust-workspace` and
// never saves its own key, so requiring one here would always fail.
export const REQUIRED_MAIN_ENTRIES = [
  { key: "rust-workspace", platform: "Linux" },
  { key: "wasm", platform: "Linux" },
  { key: "semantic-mutation", platform: "Linux" },
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
  // `runner.os` spellings `Linux`/`macOS`/`Windows`. Only the os.type()
  // spellings below are accepted; a key with an unknown platform is never
  // attributed to a required entry.
  const match = /^v\d+-rust-(.+)-(Linux|Darwin|Windows_NT)-[^-]+-[0-9a-f]+-[0-9a-f]+$/.exec(
    key ?? "",
  );
  return match ? { sharedKey: match[1], platform: match[2] } : { sharedKey: null, platform: null };
}

function formatGiB(bytes) {
  return `${(bytes / GIB).toFixed(2)} GiB`;
}

/**
 * @param {{caches: Array<{key: string, ref: string, size_in_bytes: number, created_at: string}>,
 *          usageBytes: number|null,
 *          limitBytes?: number,
 *          warnFraction?: number,
 *          requiredMainEntries?: {key: string, platform: string}[],
 *          ciSharedKeys?: string[],
 *          defaultBranchRef?: string}} input
 * @returns {{findings: Array<{code: string, message: string}>,
 *            informational: Array<{code: string, message: string}>,
 *            ok: boolean}}
 */
export function auditCacheBudget({
  caches,
  usageBytes,
  limitBytes = CACHE_LIMIT_BYTES,
  warnFraction = BUDGET_WARN_FRACTION,
  requiredMainEntries = REQUIRED_MAIN_ENTRIES,
  ciSharedKeys = CI_SHARED_KEYS,
  defaultBranchRef = "refs/heads/main",
}) {
  const findings = [];
  const informational = [];

  if (!Array.isArray(caches)) {
    findings.push({
      code: "listing-unreadable",
      message:
        "the cache listing is absent or not an array; an unreadable listing is never read as an empty (healthy) cache set",
    });
    return { findings, informational, ok: false };
  }

  // 1. Budget headroom, judged over the *raw, physical* total -- unchanged
  //    from before issue #926. GitHub's budget and its least-recently-used
  //    eviction act on physical bytes sitting in the account, not on any
  //    repository-side classification of which bytes are "controllable";
  //    "self-healing" describes whether a lever this audit rewards can fix a
  //    generation, not whether it is currently occupying budget. An earlier
  //    version of this rule subtracted a de-duplicated {sharedKey, platform}
  //    total from judgment and was reverted after review found two executed
  //    counterexamples: (a) two same-identity 5 GiB generations physically
  //    filling the entire 10 GiB budget were judged as 5 GiB and passed, and
  //    (b) an independently-observed usage total already higher than the
  //    listing sum was reduced below threshold by subtracting listing-derived
  //    stale bytes that are not proven to be the same bytes the usage
  //    endpoint is counting -- the two observations are non-atomic (see the
  //    existing max(usageBytes, rawSummed) comment below), so a byte
  //    identified as stale in one cannot be assumed present, or absent, in
  //    the other. Both are real physical-budget risks; classifying them
  //    "controllable" must not make them invisible to `ok`.
  //
  //    Generation coexistence (issue #926, measured 2026-09-04) is still
  //    computed and reported below, but strictly as an *additional*
  //    diagnostic alongside a real `budget-exhausted` finding, never as a
  //    reduction applied before judgment. See docs/DESIGN-ci.md, "Generation
  //    coexistence (issue #926, measured 2026-09-04)".
  const rawSummed = caches.reduce((total, entry) => total + (entry.size_in_bytes ?? 0), 0);
  const rawEffective = typeof usageBytes === "number" ? Math.max(usageBytes, rawSummed) : rawSummed;

  // Diagnostic only, computed from the listing alone (the usage endpoint has
  // no per-entry breakdown to de-duplicate): at most one generation per
  // {sharedKey, platform} pair on the default branch is "current"; every
  // older generation in the same group is one `Swatinem/rust-cache` would not
  // restore today. Every non-main entry -- including any `refs/pull/*` cache
  // -- is left out of this grouping entirely (not merely "kept whole"): rules
  // 3 and 4 below already judge those in full, and de-duplication logic must
  // not touch the shape that caught the original #747 incident.
  const mainGenerationsByIdentity = new Map();
  for (const entry of caches) {
    if (entry.ref !== defaultBranchRef) continue;
    const { sharedKey, platform } = entryIdentity(entry.key);
    if (!sharedKey || !platform) continue;
    const identity = `${sharedKey}::${platform}`;
    const group = mainGenerationsByIdentity.get(identity) ?? [];
    group.push(entry);
    mainGenerationsByIdentity.set(identity, group);
  }

  let staleGenerationBytes = 0;
  let staleGenerationCount = 0;
  for (const group of mainGenerationsByIdentity.values()) {
    if (group.length < 2) continue;
    // Newest first: the generation Swatinem/rust-cache would actually restore
    // today. Every older generation in the same group is the one no lever
    // this audit rewards can fix -- it can only be waited out.
    const bySize = [...group].sort(
      (a, b) => Date.parse(b.created_at) - Date.parse(a.created_at),
    );
    for (const stale of bySize.slice(1)) {
      staleGenerationBytes += stale.size_in_bytes ?? 0;
      staleGenerationCount += 1;
    }
  }

  if (typeof usageBytes !== "number") {
    findings.push({
      code: "usage-unobserved",
      message:
        "the repository cache-usage endpoint returned no total; falling back to the listing sum, which is not independent headroom evidence and can differ because of observation-time changes or an incomplete listing. Absence of the total is not evidence of headroom.",
    });
  }

  // Always reported, purely descriptive, never a pass/fail input on its own:
  // how many generations beyond the newest per {sharedKey, platform} exist on
  // the default branch right now. This is visibility into ambient GitHub-side
  // churn, not a claim that those bytes are excluded from anything judged.
  informational.push({
    code: "generation-coexistence",
    message: `${caches.length} cache entr${
      caches.length === 1 ? "y" : "ies"
    } observed; ${staleGenerationCount} generation${
      staleGenerationCount === 1 ? "" : "s"
    } beyond the newest per {sharedKey, platform} pair on \`${defaultBranchRef}\` (${formatGiB(
      staleGenerationBytes,
    )}) -- diagnostic visibility only, not subtracted from the budget judgment below.`,
  });

  if (rawEffective >= limitBytes * warnFraction) {
    const limitDiagnostic =
      rawEffective > limitBytes
        ? `${formatGiB(rawEffective - limitBytes)} above the limit`
        : `${formatGiB(limitBytes - rawEffective)} remaining before the limit`;
    findings.push({
      code: "budget-exhausted",
      message: `cache usage is ${formatGiB(rawEffective)} of a ${formatGiB(
        limitBytes,
      )} limit (${Math.round(
        (rawEffective / limitBytes) * 100,
      )}%), at or above the ${Math.round(warnFraction * 100)}% threshold. ${formatGiB(
        rawEffective,
      )} is ${limitDiagnostic}; a sufficiently large save can trigger least-recently-used eviction, including a default-branch cache that a main-targeting pull request depends on.`,
    });
    // Diagnostic companion, only ever alongside the real finding above: how
    // much of this overage *might* be self-healing generation churn, phrased
    // as "up to" because the listing and the independently-observed usage
    // total are not atomic (see the rule-1 comment above) -- a byte this
    // computation calls stale is not proven to be counted, or not counted, in
    // `usageBytes`. This never changes `ok` on its own; it is only ever
    // present when `budget-exhausted` already is.
    if (staleGenerationBytes > 0) {
      findings.push({
        code: "generation-coexistence-partial-explanation",
        message: `up to ${formatGiB(staleGenerationBytes)} of this overage may be ${
          staleGenerationCount === 1 ? "a single generation" : `${staleGenerationCount} generations`
        } beyond the newest per {sharedKey, platform} pair on \`${defaultBranchRef}\` -- self-healing via GitHub's own least-recently-used eviction, not something a save-if/shared-key/deletion change in this repository controls. This does not reduce the budget-exhausted finding above: the listing and the independently-observed usage total are separate, non-atomic observations, so these bytes are not proven to be what the usage total is counting. See docs/DESIGN-ci.md, "Generation coexistence (issue #926, measured 2026-09-04)".`,
      });
    }
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
        message: `no \`${defaultBranchRef}\` cache for shared key \`${key}\` on platform \`${platform}\`. Actions caches are ref-scoped: a pull request can read its current ref, base branch, and default branch. For a main-targeting pull request those latter two are \`${defaultBranchRef}\`; without this entry each such pull request (or, for a matrix job, that platform's shard) builds cold.`,
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
        )}). This violates the current no-pull-request-save invariant. It may have been saved before the guard existed or may indicate a later guard regression; inspect created_at and workflow provenance.`,
      });
    }
  }

  // 4. Repository invariant since #752 and the `merge-readiness.yml`
  //    restore-only fix: no workflow saves a rust cache on a pull-request
  //    event, full stop -- not just `CI_SHARED_KEYS`' three declared keys. Any
  //    `v0-rust-*` key on a `refs/pull/*` ref is a violation of that current
  //    invariant; it may predate the guard or indicate a later regression, so
  //    inspect `created_at` and workflow provenance. This applies whether it is
  //    a known shared key (already reported by rule 3 above, and skipped here to
  //    avoid a duplicate finding) or a new one.
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

  return { findings, informational, ok: findings.length === 0 };
}

export function formatReport({ findings, informational = [], ok }) {
  const header = ok
    ? "cache budget audit: PASS -- budget within threshold, default-branch caches present, no pull-request-scoped Rust caches"
    : [
        `cache budget audit: FAIL -- ${findings.length} finding(s)`,
        ...findings.map((finding) => `  ${finding.code}: ${finding.message}`),
      ].join("\n");
  if (informational.length === 0) return header;
  // Informational lines never change PASS/FAIL and are labeled distinctly so
  // a reader (or a script) cannot mistake one for a judged finding.
  return [
    header,
    ...informational.map((entry) => `  informational/${entry.code}: ${entry.message}`),
  ].join("\n");
}
