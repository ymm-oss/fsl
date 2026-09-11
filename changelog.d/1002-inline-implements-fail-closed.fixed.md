Fixed (#1002)!: a requirements spec can declare an inline `implements Abs from
"..." { ... }`, which makes `check` and `verify` refine that spec against the
upper layer as part of the ordinary run. A failed refinement used to be
reported only under the nested `implements` key: the top-level `result` stayed
successful and the process still exited 0, so a gate reading either of them
passed a spec whose refinement had failed. The failing seam verdict
(`refinement_failed` or `impl_violated`) now becomes the top-level `result`
verbatim and the process exits 1, while `implements.violation` keeps the
seam-specific evidence. Five subcommands change their exit code from 0 to 1 on
such a spec: `check`, `verify`, `sweep` (whose `sweep_passed` becomes
`sweep_failed`), `mutate`, and `ledger`. `mutate` is worth naming separately
because its loss is silent rather than wrong: it re-emits the failing baseline
envelope and generates no mutants at all, so a spec with a broken seam stops
having a kill rate rather than having a lower one. `db check` and `domain
check` are unaffected because neither reaches verification on this input: both
reject a requirements document by kind (`expected a dbsystem document` /
`expected a domain document`, exit 2) before any kernel projection. A `verify`
scoped with `--property`, `--exclude-property`, or `--from-state` still omits
`implements` entirely and therefore still cannot gate the seam (#1008).

Those seven exit codes were measured, not derived: with a binary built from
`81b40e3b` (the previous `main`) and one built from `71613598`, each run in
its own checkout, against
`tests/fixtures/chain/requirements_broken_implements.fsl` -- blob `71510f81`
at the former and `e1129ccb` at the latter, differing by two added comment
lines and nothing else -- whose `implements ... from "business.fsl"` import
resolves to blob `ce15ac77` in both. The five went 0 -> 1; `db check` and
`domain check` returned exit 2 with the quoted messages on both binaries.

`fslc html` is worth one more line, because its report was actively misleading
rather than merely incomplete: on the same spec its top-level Result went from
a green `verified` badge to a red `refinement_failed` one. Its exit code is
unchanged -- `html` still exits 0 over a failing spec, which is #1009 and is
not fixed here -- so the badge and the exit code now disagree deliberately,
and only the badge has been corrected.
