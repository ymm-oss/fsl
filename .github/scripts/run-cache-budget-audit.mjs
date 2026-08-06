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
  // 100 is the maximum page size. A repository at the 10 GiB limit holds far
  // fewer entries than that, and `usageBytes` below comes from the API's own
  // total, so a truncated listing cannot understate the budget finding.
  const listing = await api("/actions/caches?per_page=100");
  caches = listing.actions_caches;
  const usage = await api("/actions/cache/usage");
  if (typeof usage.active_caches_size_in_bytes === "number") {
    usageBytes = usage.active_caches_size_in_bytes;
  }
} catch (error) {
  // An unreadable API is not evidence of a healthy cache budget.
  console.error(`cache budget audit: FAIL -- api-unreadable: ${error.message}`);
  process.exit(1);
}

const result = auditCacheBudget({ caches, usageBytes });
console.log(formatReport(result));
process.exit(result.ok ? 0 : 1);
