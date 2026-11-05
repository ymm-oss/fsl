Changed (#1422): Causal evidence ingestion now rejects present
`period.valid_until` values unless they are strings with the exact ASCII
`YYYY-MM-DD` shape and a valid Gregorian date, after checking the artifact
digest. Missing freshness remains accepted but excluded from current support;
start/end timing exclusions and review-only assurance are unchanged.

`causal analyze` and `causal ledger` now refuse an `--as-of` value that is not
a real Gregorian date written as ASCII `YYYY-MM-DD` with `kind:"usage"` and
exit 2, before reading the model or evidence. This is a new refusal: values
such as `2099-02-30`, `2099-13-01`, `garbage`, an empty string, or
`2099-+4-01` used to exit 0 and count no evidence as stale. Date fields are now
read as fixed-width ASCII digits everywhere, so a signed `period.start` or
`period.end` such as `+026-01-01` reports `period_date_unparsable` instead of
being read as year 26.

Sweep now checks the normalized inclusive instances/value/depth grid before
allocating combinations or running cells, with a fixed limit of 10,000 cells.
Overflow or an oversized grid returns an exit-2 semantics error; admitted grids
retain their existing verification behavior.

Dynamic JSON-log negation of `i64::MIN` now returns the existing arithmetic
refusal instead of panicking. Replay reports `nonconformant` / `log_mapping`
with exit 1; causal observation reports `observation_replay_failed` with exit 2.
Representable negation, non-integer type errors, and other arithmetic are unchanged.

Observation producers now validate generated evidence before writing evidence or
lifecycle files. Invalid declared freshness returns the existing exit-2 schema
diagnostic, preserving existing output files; invalid-start timing remains soft.
