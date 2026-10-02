Fixed (#1218): `fslc verify --instances` / `--values` (and every `fslc sweep`
cell) now replays the requirements `acceptance`/`forbidden` scenarios instead of
skipping them all. Previously the native CLI did not replay any scenario under bound
overrides, so a false acceptance yielded `verified` / exit 0 — even when the
override equalled the declared range — and `sweep` always passed it. The frozen
Python reference's per-scenario skip (#89), as `docs/manual/LANGUAGE.md`
specifies it, is now native: only a scenario that references a value outside
the overridden scope — one the override removed: inside the declared bounds,
outside the overridden ones (an action argument, an `expect` map index whose
`expect` evaluates in the declared world, or a forbidden final step whose
"rejection" tested no guard) — is skipped, with an
`acceptance_skipped` / `forbidden_skipped` warning whose `reference` names the
out-of-range reference; every other failure is the same exit-2 error
(`trace_type: "acceptance"` / `"forbidden"`) an unscoped run reports.
`requirement_traces` (#1008) is now built from those skips: absent when every
scenario was replayed, otherwise `not_evaluated` / `bounds_override` with
`skipped: [{kind, id, reference}]` instead of the scenario counts, and `sweep`
lists the union as `sweep.not_evaluated.skipped`. Native is deliberately
stricter than the frozen Python reference, which also skips references outside
the declared bounds, excuses any `expect` error that mentions an out-of-range
index, and counts an out-of-range forbidden final step as a rejection.
Migration: a scoped `verify` or a `sweep` that exited 0 only because its
scenarios were never replayed now exits 2 with the failing scenario — fix the
scenario or the spec (an unscoped `verify` already reported the same error).
