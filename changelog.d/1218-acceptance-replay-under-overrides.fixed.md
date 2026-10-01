Fixed (#1218): `fslc verify --instances` / `--values` (and every `fslc sweep`
cell) now replays the requirements `acceptance`/`forbidden` scenarios instead of
skipping them all. Previously the native CLI did not replay any scenario under bound
overrides, so a false acceptance yielded `verified` / exit 0 — even when the
override equalled the declared range — and `sweep` always passed it. The frozen
Python reference's per-scenario skip (#89), as `docs/manual/LANGUAGE.md`
specifies it, is now native: only a scenario that references a value outside
the overridden scope (an action argument outside its parameter domain, or an
`expect` map index outside its key domain) is skipped, with an
`acceptance_skipped` / `forbidden_skipped` warning whose `reference` names the
out-of-range reference; every other failure is the same exit-2 error
(`trace_type: "acceptance"` / `"forbidden"`) an unscoped run reports.
`requirement_traces` (#1008) is now built from those skips: absent when every
scenario was replayed, otherwise `not_evaluated` / `bounds_override` with
`skipped: [{kind, id, reference}]` instead of the scenario counts.
Migration: a scoped `verify` or a `sweep` that exited 0 only because its
scenarios were never replayed now exits 2 with the failing scenario — fix the
scenario or the spec (an unscoped `verify` already reported the same error).
