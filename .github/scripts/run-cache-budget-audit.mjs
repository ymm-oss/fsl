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

export async function fetchAllCaches(api) {
  const caches = [];
  let totalCount;

  for (let page = 1; ; page += 1) {
    const listing = await api(`/actions/caches?per_page=100&page=${page}`);
    if (!Array.isArray(listing.actions_caches)) {
      throw new Error(`cache listing page ${page} has no actions_caches array`);
    }
    if (page === 1) {
      totalCount = listing.total_count;
      if (!Number.isSafeInteger(totalCount) || totalCount < 0) {
        throw new Error("cache listing has no valid total_count");
      }
    }
    caches.push(...listing.actions_caches);
    if (caches.length === totalCount) return caches;
    if (caches.length > totalCount || listing.actions_caches.length === 0) {
      throw new Error(
        `cache listing ended inconsistently: expected ${totalCount} entries, received ${caches.length}`,
      );
    }
  }
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
    // its total_count are required because rules 2–4 inspect individual cache
    // entries, not just the repository-wide usage total.
    caches = await fetchAllCaches(api);
    const usage = await api("/actions/cache/usage");
    if (typeof usage.active_caches_size_in_bytes === "number") {
      usageBytes = usage.active_caches_size_in_bytes;
    }
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
