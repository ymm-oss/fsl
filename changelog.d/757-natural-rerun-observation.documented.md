Documented (#757): recorded the first natural production partial re-run — a GitHub-side
artifact-service transient failed one test shard on a docs-only PR, `gh run rerun --failed`
re-ran only that shard, and the aggregator accepted the mixed-attempt cohort
(`attempts=1,1,2`), recovering in one shard's time instead of a full three-shard re-run.
