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

  // 1. Budget headroom, judged over the *controllable* footprint (issue
  //    #926): at most one generation per {sharedKey, platform} pair on the
  //    default branch, keeping only the most recently created generation.
  //    `Swatinem/rust-cache` hashes the runner's entire installed-toolchain
  //    list into its key, not only the one version a workflow pins, so a
  //    `macos-15`/`windows-latest` runner-image update regenerates a key even
  //    though nothing this repository controls changed (measured
  //    2026-09-04: `rust/Cargo.lock` and `.github/workflows/ci.yml` were
  //    byte-identical across a coexistence window, and both runs resolved the
  //    pinned toolchain to the identical rustc commit). GitHub's own
  //    least-recently-used eviction already reclaims a superseded generation
  //    on its own schedule; judging the raw total would report a defect
  //    nobody in this repository can fix by any means this audit is willing
  //    to reward, and the shortest way to silence that false alarm --
  //    deleting the older generation -- has already caused a *worse*,
  //    previously observed failure (main-cache-absent) for this exact
  //    reason. See docs/DESIGN-ci.md, "Generation coexistence (issue #926,
  //    measured 2026-09-04)".
  //
  //    Only default-branch entries attributable to a {sharedKey, platform}
  //    pair are de-duplicated this way. Every other entry -- including any
  //    `refs/pull/*` cache -- is counted individually and in full: the
  //    original #747 incident was many *different* refs each holding their
  //    own ref-scoped cache, not multiple generations of one key on one ref,
  //    and de-duplication must not blunt detection of that shape (rules 3
  //    and 4 below still see every such entry).
  const rawSummed = caches.reduce((total, entry) => total + (entry.size_in_bytes ?? 0), 0);
  const rawEffective = typeof usageBytes === "number" ? Math.max(usageBytes, rawSummed) : rawSummed;

  const mainGenerationsByIdentity = new Map();
  let unattributedMainCount = 0;
  for (const entry of caches) {
    if (entry.ref !== defaultBranchRef) continue;
    const { sharedKey, platform } = entryIdentity(entry.key);
    if (!sharedKey || !platform) {
      unattributedMainCount += 1;
      continue;
    }
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

  // Informational only, never a pass/fail input: the raw, undeduplicated
  // total and how many stale generations were excluded from judgment below.
  // Deleting a stale generation changes this number; it changes nothing the
  // audit judges (see rule 1 below), so there is no reward for doing so.
  informational.push({
    code: "raw-cache-footprint",
    message: `raw cache total (all refs, all generations) is ${formatGiB(rawEffective)} of a ${formatGiB(
      limitBytes,
    )} limit (${Math.round((rawEffective / limitBytes) * 100)}%) across ${caches.length} entr${
      caches.length === 1 ? "y" : "ies"
    }; ${staleGenerationCount} superseded generation${
      staleGenerationCount === 1 ? "" : "s"
    } on \`${defaultBranchRef}\` (${formatGiB(staleGenerationBytes)}) excluded from the controllable total below as self-healing, not judged.`,
  });

  const controllableEffective = Math.max(0, rawEffective - staleGenerationBytes);
  if (controllableEffective >= limitBytes * warnFraction) {
    const limitDiagnostic =
      controllableEffective > limitBytes
        ? `${formatGiB(controllableEffective - limitBytes)} above the limit`
        : `${formatGiB(limitBytes - controllableEffective)} remaining before the limit`;
    findings.push({
      code: "budget-exhausted",
      message: `controllable cache usage is ${formatGiB(controllableEffective)} of a ${formatGiB(
        limitBytes,
      )} limit (${Math.round(
        (controllableEffective / limitBytes) * 100,
      )}%), at or above the ${Math.round(warnFraction * 100)}% threshold. ${formatGiB(
        controllableEffective,
      )} is ${limitDiagnostic}; a sufficiently large save can trigger least-recently-used eviction, including a default-branch cache that a main-targeting pull request depends on.`,
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
