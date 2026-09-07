Required (#1007): the `site reference freshness` context now also enforces
static manual-route integrity (`tests/test_site_manual_integrity.py`) and the
sitewide refresh contract (`tests/test_site_refresh_contract.py`) alongside the
existing generated-reference freshness snapshots. The context keeps its name,
ruleset membership, job, triggers, permissions, timeout, and concurrency, and
still runs on every pull request with no path filter; `fetch-depth: 0` was added
so the pinned-blob check can read local Git history. None of the three checks
establishes Rust/solver behavior, native CLI parity, browser rendering, or
assistive-technology behavior -- they are bounded documentation-artifact checks.
