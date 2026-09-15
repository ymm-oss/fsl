Fixed (#1041)!: `check`/`verify`'s inline `implements` correspondence search
(`check_refinement`) is now bounded by a fixed internal state-count budget
(`IMPLEMENTS_SEARCH_BUDGET`, 50,000 states; no CLI flag, and not shared with
`--explicit-budget`, a different search). Exceeding it reports top-level
`unknown_budget` and exit 1 on both entry points instead of running
unbounded. Observable change: an inline `implements` search over a large
enough domain now exits 1 with `unknown_budget` where it previously grew RSS
without bound (P1 measured ~3.8 GB on one corpus-scale domain, with a flat
control run confirming the growth was specific to the search). A search over
50,000 states now ends in `unknown_budget` / exit 1 where an environment with
enough memory could previously have run it to completion and exited 0, so this
is recorded as breaking even though no spec in this repository's corpus reaches
the budget: the population that would be affected is users' own specs, which
cannot be measured from here. The budget
constant is included in `verify`'s cache key so a future change to it cannot
replay a stale verdict.
