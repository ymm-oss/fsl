Changed (#1002): an inline `implements` refinement failure now folds into the
top-level `result` (`refinement_failed` or `impl_violated`) and exits 1, instead
of reporting success with the failure nested under `implements` only. The fold
happens where the verification envelope is produced, so it reaches every command
that reports a verification verdict: measured against a binary built from the
previous `main`, five subcommands change their exit code from 0 to 1 on a spec
with a failing seam -- `check`, `verify`, `sweep` (`sweep_passed` becomes
`sweep_failed`), `mutate` (which returns the baseline envelope without generating
mutants, because its baseline is no longer `verified`), and `ledger`. `db check`
and `domain check` are unaffected because neither reaches verification on this
input: both reject a requirements document by kind (`expected a dbsystem
document` / `expected a domain document`, exit 2) before any kernel projection.
A `verify` scoped with `--property`,
`--exclude-property`, or `--from-state` still omits `implements` entirely and
therefore still cannot gate the seam (#1008).
