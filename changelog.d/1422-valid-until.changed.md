Changed (#1422): Causal evidence ingestion now rejects present
`period.valid_until` values unless they are strings with the exact ASCII
`YYYY-MM-DD` shape and a valid Gregorian date, after checking the artifact
digest. Missing freshness remains accepted but excluded from current support;
start/end timing exclusions and review-only assurance are unchanged.

Sweep now checks the normalized inclusive instances/value/depth grid before
allocating combinations or running cells, with a fixed limit of 10,000 cells.
Overflow or an oversized grid returns an exit-2 semantics error; admitted grids
retain their existing verification behavior.

Dynamic JSON-log negation of `i64::MIN` now returns the existing arithmetic
refusal instead of panicking. Replay reports `nonconformant` / `log_mapping`
with exit 1; causal observation reports `observation_replay_failed` with exit 2.
Representable negation, non-integer type errors, and other arithmetic are unchanged.
