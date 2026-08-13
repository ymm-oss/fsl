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

function validNonNegativeSafeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

export async function fetchAllCaches(api) {
  const caches = [];
  const cacheIds = new Set();
  let totalCount;
  let maximumPages;

  for (let page = 1; ; page += 1) {
    if (maximumPages !== undefined && page > maximumPages) {
      throw new Error(
        `cache listing exceeded its fixed ${maximumPages}-page bound from total_count ${totalCount}`,
      );
    }

    const listing = await api(`/actions/caches?per_page=${CACHE_PAGE_SIZE}&page=${page}`);
    if (!Array.isArray(listing.actions_caches)) {
      throw new Error(`cache listing page ${page} has no actions_caches array`);
    }
    if (!validNonNegativeSafeInteger(listing.total_count)) {
      throw new Error(`cache listing page ${page} has no valid total_count`);
    }
    if (page === 1) {
      totalCount = listing.total_count;
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
      if (cacheIds.has(cache.id)) {
        throw new Error(`cache listing page ${page} repeats cache id ${cache.id}`);
      }
      cacheIds.add(cache.id);
      caches.push(cache);
    }

    if (page === maximumPages) return caches;
  }
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

  async function api(path) {
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
    return response.json();
  }

  let caches;
  let usageBytes = null;
  try {
    // The cache-list endpoint is paginated at 100 entries. All pages through
    // its stable total_count are required because rules 2–4 inspect individual
    // cache entries, not just the repository-wide usage total.
    caches = await fetchAllCaches(api);
    const usage = await api("/actions/cache/usage");
    usageBytes = observedUsageBytes(usage);
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
