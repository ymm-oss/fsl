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

const CACHE_PAGE_SIZE = 100;
// GitHub's standard Actions GITHUB_TOKEN quota is 1,000 requests/hour/repository.
// Keep 100 requests free for the rest of the workflow and bound the *whole*
// audit (usage plus two complete observations and one retry) to the remainder.
export const GITHUB_TOKEN_REQUEST_CEILING = 1_000;
export const CACHE_AUDIT_REQUEST_HEADROOM = 100;
export const CACHE_AUDIT_REQUEST_BUDGET =
  GITHUB_TOKEN_REQUEST_CEILING - CACHE_AUDIT_REQUEST_HEADROOM;
export const STABILITY_ATTEMPTS = 2;

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
  // order. Ascending order makes a pair of complete observations comparable.
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

export async function fetchStableCaches(api) {
  for (let attempt = 1; attempt <= STABILITY_ATTEMPTS; attempt += 1) {
    const first = await fetchCacheCollection(api);
    const second = await fetchCacheCollection(api);
    if (sameCacheCollection(first, second)) return second;
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
  return usageBytes;
}

async function main() {
  const token = process.env.GITHUB_TOKEN;
  const repo = process.env.GITHUB_REPOSITORY;

  if (!token || !repo) {
    console.error(
      "cache budget audit: GITHUB_TOKEN and GITHUB_REPOSITORY are required; refusing to report a healthy budget without observing one",
    );
    process.exit(1);
  }

  let requestCount = 0;
  let rateLimitRemaining = null;

  async function api(path) {
    if (requestCount >= CACHE_AUDIT_REQUEST_BUDGET) {
      throw new Error(
        `cache audit consumed its ${CACHE_AUDIT_REQUEST_BUDGET}-request budget; refusing to consume reserved rate-limit headroom`,
      );
    }
    const response = await fetch(`https://api.github.com/repos/${repo}${path}`, {
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
    const remaining = Number(response.headers.get("x-ratelimit-remaining"));
    if (!validNonNegativeSafeInteger(remaining)) {
      throw new Error(`GET ${path} returned no valid x-ratelimit-remaining header`);
    }
    rateLimitRemaining = remaining;
    return response.json();
  }

  let caches;
  let usageBytes = null;
  try {
    // Usage updates about every five minutes, so it is bytes-only conservative
    // evidence, never an identity/completeness reconciliation. Completeness is
    // two stable-order full collections whose IDs and inspected fields agree.
    const usage = await api("/actions/cache/usage");
    usageBytes = observedUsageBytes(usage);
    caches = await fetchStableCaches(async (path) => {
      const result = await api(path);
      // After the first page of any collection, its declared count defines a
      // bounded worst-case request plan (including the retry). Preserve the
      // configured 100-request headroom in the actual rate-limit bucket too.
      if (path.includes("page=1")) {
        const neededAfterThisResponse = maximumAuditRequests(result.total_count) - requestCount;
        if (rateLimitRemaining < neededAfterThisResponse + CACHE_AUDIT_REQUEST_HEADROOM) {
          throw new Error(
            `rate limit has ${rateLimitRemaining} requests remaining; need ${neededAfterThisResponse} ` +
              `for this bounded audit plus ${CACHE_AUDIT_REQUEST_HEADROOM} reserved`,
          );
        }
      }
      return result;
    });
  } catch (error) {
    // An unreadable or incomplete API listing is not evidence of a healthy cache budget.
    console.error(`cache budget audit: FAIL -- api-unreadable: ${error.message}`);
    process.exit(1);
  }

  const result = auditCacheBudget({ caches, usageBytes });
  console.log(formatReport(result));
  process.exit(result.ok ? 0 : 1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
