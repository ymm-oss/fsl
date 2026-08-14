// SPDX-License-Identifier: Apache-2.0
//
// Fetches the live Actions cache listing and runs the pure audit in
// audit-cache-budget.mjs (issue #747). Kept separate from the audit itself so
// the audit can be calibrated offline against rejecting fixtures; this file
// holds only the network access and the process exit.
//
// Needs a token with `actions: read`. The workflow's own GITHUB_TOKEN has it,
// so unlike the ruleset audit this needs no PAT.

import { auditCacheBudget, formatReport } from "./audit-cache-budget.mjs";
import { pathToFileURL } from "node:url";

export const CACHE_PAGE_SIZE = 100;
// GitHub's standard Actions GITHUB_TOKEN quota is 1,000 requests/hour/repository.
// Keep 100 requests free for the rest of the workflow and bound the *whole*
// audit (usage plus two complete observations and one retry) to the remainder.
export const GITHUB_TOKEN_REQUEST_CEILING = 1_000;
export const CACHE_AUDIT_REQUEST_HEADROOM = 100;
export const CACHE_AUDIT_REQUEST_BUDGET =
  GITHUB_TOKEN_REQUEST_CEILING - CACHE_AUDIT_REQUEST_HEADROOM;
export const STABILITY_ATTEMPTS = 2;
// GitHub documents cache *usage* as updating about every five minutes, but it
// does not promise that waiting five minutes makes a cache listing an atomic
// snapshot. The retry is therefore a bounded, separate observation rather than
// a freshness claim: one second separates the requests without adding five
// minutes to a scheduled audit, and repeated disagreement still fails closed.
export const STABILITY_RETRY_DELAY_MS = 1_000;

function validNonNegativeSafeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function validNonEmptyString(value) {
  return typeof value === "string" && value.length > 0;
}

function listingRequests(totalCount) {
  return Math.max(1, Math.ceil(totalCount / CACHE_PAGE_SIZE)) + 1;
}

export function maximumAuditRequests(totalCount) {
  return 1 + STABILITY_ATTEMPTS * 2 * listingRequests(totalCount);
}

export function pageNumber(path) {
  return new URL(path, "https://api.github.com").searchParams.get("page");
}

function assertListingFitsBudget(totalCount) {
  const required = maximumAuditRequests(totalCount);
  if (required > CACHE_AUDIT_REQUEST_BUDGET) {
    throw new Error(
      `cache listing total_count ${totalCount} needs up to ${required} requests ` +
        `(usage plus two complete observations and one retry); audit budget is ${CACHE_AUDIT_REQUEST_BUDGET}`,
    );
  }
}

function cachePath(page) {
  // `created_at` is not changed by restoration/access, unlike last-accessed
  // order. GitHub documents it as the primary order only. A tied page boundary
  // can produce a duplicate ID, which is rejected immediately, or different
  // paired observations, which retry once and fail closed if still different;
  // an identical mixed state can repeat as an undetectable residual.
  return `/actions/caches?per_page=${CACHE_PAGE_SIZE}&sort=created_at&direction=asc&page=${page}`;
}

export async function fetchCacheCollection(api) {
  const caches = [];
  const cacheIds = new Set();
  let totalCount;
  let maximumPages;

  for (let page = 1; ; page += 1) {
    const listing = await api(cachePath(page));
    if (!Array.isArray(listing.actions_caches)) {
      throw new Error(`cache listing page ${page} has no actions_caches array`);
    }
    if (!validNonNegativeSafeInteger(listing.total_count)) {
      throw new Error(`cache listing page ${page} has no valid total_count`);
    }
    if (page === 1) {
      totalCount = listing.total_count;
      assertListingFitsBudget(totalCount);
      maximumPages = Math.max(1, Math.ceil(totalCount / CACHE_PAGE_SIZE));
    } else if (listing.total_count !== totalCount) {
      throw new Error(
        `cache listing total_count changed from ${totalCount} to ${listing.total_count} on page ${page}`,
      );
    }

    const expectedEntries = Math.min(
      CACHE_PAGE_SIZE,
      Math.max(0, totalCount - (page - 1) * CACHE_PAGE_SIZE),
    );
    if (listing.actions_caches.length !== expectedEntries) {
      throw new Error(
        `cache listing page ${page} has ${listing.actions_caches.length} entries; expected ${expectedEntries} from total_count ${totalCount}`,
      );
    }

    for (const cache of listing.actions_caches) {
      if (!validNonNegativeSafeInteger(cache?.id)) {
        throw new Error(`cache listing page ${page} has an entry with no valid id`);
      }
      if (!validNonEmptyString(cache.key)) {
        throw new Error(`cache listing page ${page} has an entry with no valid key`);
      }
      if (!validNonEmptyString(cache.ref)) {
        throw new Error(`cache listing page ${page} has an entry with no valid ref`);
      }
      if (!validNonNegativeSafeInteger(cache.size_in_bytes)) {
        throw new Error(`cache listing page ${page} has an entry with no valid size_in_bytes`);
      }
      if (cacheIds.has(cache.id)) {
        throw new Error(`cache listing page ${page} repeats cache id ${cache.id}`);
      }
      cacheIds.add(cache.id);
      caches.push(cache);
    }

    if (page === maximumPages) {
      const sentinelPage = page + 1;
      const sentinel = await api(cachePath(sentinelPage));
      if (!Array.isArray(sentinel.actions_caches)) {
        throw new Error(`cache listing sentinel page ${sentinelPage} has no actions_caches array`);
      }
      if (!validNonNegativeSafeInteger(sentinel.total_count)) {
        throw new Error(`cache listing sentinel page ${sentinelPage} has no valid total_count`);
      }
      if (sentinel.total_count !== 0 && sentinel.total_count !== totalCount) {
        throw new Error(
          `cache listing sentinel page ${sentinelPage} has total_count ${sentinel.total_count}; expected 0 or ${totalCount}`,
        );
      }
      if (sentinel.actions_caches.length !== 0) {
        throw new Error(
          `cache listing sentinel page ${sentinelPage} has ${sentinel.actions_caches.length} entries beyond total_count ${totalCount}`,
        );
      }
      return caches;
    }
  }
}

export function sameCacheCollection(first, second) {
  if (first.length !== second.length) return false;
  const secondById = new Map(second.map((entry) => [entry.id, entry]));
  return first.every((entry) => {
    const other = secondById.get(entry.id);
    return (
      other &&
      entry.key === other.key &&
      entry.ref === other.ref &&
      entry.size_in_bytes === other.size_in_bytes
    );
  });
}

export async function fetchStableCaches(
  api,
  { sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)) } = {},
) {
  for (let attempt = 1; attempt <= STABILITY_ATTEMPTS; attempt += 1) {
    const first = await fetchCacheCollection(api);
    const second = await fetchCacheCollection(api);
    if (sameCacheCollection(first, second)) return second;
    if (attempt < STABILITY_ATTEMPTS) {
      await sleep(STABILITY_RETRY_DELAY_MS);
    }
  }
  throw new Error(
    `two complete created_at-ordered cache observations disagreed after ${STABILITY_ATTEMPTS} attempts`,
  );
}

export function observedUsageBytes(usage) {
  if (!Object.hasOwn(usage, "active_caches_size_in_bytes")) return null;

  const usageBytes = usage.active_caches_size_in_bytes;
  if (!validNonNegativeSafeInteger(usageBytes)) {
    throw new Error("cache usage has no valid active_caches_size_in_bytes");
  }
  if (
    Object.hasOwn(usage, "active_caches_count") &&
    !validNonNegativeSafeInteger(usage.active_caches_count)
  ) {
    throw new Error("cache usage has no valid active_caches_count");
  }
  return usageBytes;
}

/**
 * Creates the live GitHub API boundary for the cache audit. It is exported so
 * the request cap, transport failures, and rate-limit headers are calibrated
 * without an actual network request.
 */
export function createCacheAuditApi({ token, repo, fetchImpl = fetch }) {
  let requestCount = 0;
  let rateLimitRemaining = null;

  async function request(path) {
    if (requestCount >= CACHE_AUDIT_REQUEST_BUDGET) {
      throw new Error(
        `cache audit consumed its ${CACHE_AUDIT_REQUEST_BUDGET}-request budget; refusing to consume reserved rate-limit headroom`,
      );
    }
    if (
      rateLimitRemaining !== null &&
      rateLimitRemaining <= CACHE_AUDIT_REQUEST_HEADROOM
    ) {
      throw new Error(
        `rate limit has ${rateLimitRemaining} requests remaining; refusing to consume ${CACHE_AUDIT_REQUEST_HEADROOM} reserved`,
      );
    }
    const response = await fetchImpl(`https://api.github.com/repos/${repo}${path}`, {
      headers: {
        authorization: `Bearer ${token}`,
        accept: "application/vnd.github+json",
        "x-github-api-version": "2022-11-28",
      },
    });
    if (!response.ok) {
      throw new Error(`GET ${path} -> ${response.status} ${response.statusText}`);
    }
    requestCount += 1;
    const rawRemaining = response.headers.get("x-ratelimit-remaining");
    if (rawRemaining === null || rawRemaining.trim() === "") {
      throw new Error(`GET ${path} returned no valid x-ratelimit-remaining header`);
    }
    // HTTP message syntax removes surrounding OWS before field-value
    // evaluation, but the value itself must be an ASCII decimal integer;
    // Number() would otherwise accept non-decimal syntaxes such as 0x3e8,
    // 1e3, and 0b1111101000.
    const normalizedRemaining = rawRemaining.trim();
    if (!/^[0-9]+$/.test(normalizedRemaining)) {
      throw new Error(`GET ${path} returned no valid x-ratelimit-remaining header`);
    }
    const remaining = Number(normalizedRemaining);
    if (!validNonNegativeSafeInteger(remaining)) {
      throw new Error(`GET ${path} returned no valid x-ratelimit-remaining header`);
    }
    rateLimitRemaining = remaining;
    return response.json();
  }

  return {
    request,
    get rateLimitRemaining() {
      return rateLimitRemaining;
    },
  };
}

export async function runCacheBudgetAudit({
  token = process.env.GITHUB_TOKEN,
  repo = process.env.GITHUB_REPOSITORY,
  fetchImpl = fetch,
  writeReport = (report) => console.log(report),
  writeError = (report) => console.error(report),
} = {}) {
  if (!token || !repo) {
    writeError(
      "cache budget audit: GITHUB_TOKEN and GITHUB_REPOSITORY are required; refusing to report a healthy budget without observing one",
    );
    return { ok: false };
  }

  const api = createCacheAuditApi({ token, repo, fetchImpl });
  let caches;
  let usageBytes = null;
  try {
    // Usage updates about every five minutes, so it is bytes-only conservative
    // evidence, never an identity/completeness reconciliation. Completeness is
    // two stable-order full collections whose IDs and inspected fields agree.
    const usage = await api.request("/actions/cache/usage");
    usageBytes = observedUsageBytes(usage);
    caches = await fetchStableCaches(async (path) => {
      const result = await api.request(path);
      // After the first page of any collection, its declared count defines a
      // current collection's remaining requests (including its sentinel).
      // This is not the new collection's total bound minus cumulative requests:
      // a later, smaller listing cannot make a required current continuation
      // negative. The next request also stops at/below the 100-request
      // headroom, so an unbounded future collection can never consume it.
      if (pageNumber(path) === "1") {
        if (!validNonNegativeSafeInteger(result.total_count)) {
          throw new Error("cache listing page 1 has no valid total_count");
        }
        const neededAfterThisResponse = listingRequests(result.total_count) - 1;
        if (
          api.rateLimitRemaining <
          neededAfterThisResponse + CACHE_AUDIT_REQUEST_HEADROOM
        ) {
          throw new Error(
            `rate limit has ${api.rateLimitRemaining} requests remaining; need ${neededAfterThisResponse} ` +
              `for this bounded audit plus ${CACHE_AUDIT_REQUEST_HEADROOM} reserved`,
          );
        }
      }
      return result;
    });
  } catch (error) {
    // An unreadable or incomplete API listing is not evidence of a healthy cache budget.
    writeError(`cache budget audit: FAIL -- api-unreadable: ${error.message}`);
    return { ok: false };
  }

  const result = auditCacheBudget({ caches, usageBytes });
  writeReport(formatReport(result));
  return result;
}

async function main() {
  const result = await runCacheBudgetAudit();
  process.exit(result.ok ? 0 : 1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
