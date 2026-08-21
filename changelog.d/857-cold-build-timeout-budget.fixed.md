Fixed (#857): `rust tests` and `rust checks` have cold-build margin. A cold
`rust tests` shard was measured at 30m04s against its 30-minute budget and was
cancelled, turning the aggregate `rust workspace` required context red;
`docs/DESIGN-ci.md` had recorded that outcome as an unobserved risk. The budgets
move to 45 and 30 minutes, sized from measured cold durations (24m59s / 27m12s /
30m04s for the shards, 16m35s for `rust checks`) against warm baselines of
9-11m and 2m. #747 records why cold builds recur: two concurrent pull requests
exceed the 10 GiB Actions cache ceiling and evict each other, so a budget with
no cold margin turns a routine eviction into a failed required context.
