Fixed (#1002)!: a requirements spec can declare an inline
`implements Abs from "..." { ... }`, which makes `check` and `verify` refine
that spec against the upper layer as part of the ordinary run. A failed
refinement used to be reported only under the nested `implements` key: the
top-level `result` stayed successful and the process still exited 0, so a gate
reading either of them passed a spec whose refinement had failed. The failing
seam verdict (`refinement_failed` or `impl_violated`) now becomes the top-level
`result` verbatim and the process exits 1, while `implements.violation` keeps
the seam-specific evidence. Measured against a binary built from the previous
`main`, five subcommands change their exit code from 0 to 1 on such a spec:
`check`, `verify`, `sweep` (whose `sweep_passed` becomes `sweep_failed`),
`mutate`, and `ledger`. `mutate` is worth naming separately because its loss is
silent rather than wrong: it re-emits the failing baseline envelope and
generates no mutants at all, so a spec with a broken seam stops having a kill
rate rather than having a lower one. `db check` and `domain check` are
unaffected because neither reaches verification on this input: both reject a
requirements document by kind (`expected a dbsystem document` / `expected a
domain document`, exit 2) before any kernel projection. A `verify` scoped with
`--property`, `--exclude-property`, or `--from-state` still omits `implements`
entirely and therefore still cannot gate the seam (#1008).
