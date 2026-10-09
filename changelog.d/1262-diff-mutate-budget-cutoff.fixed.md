Fixed (#1262), **breaking for `fslc diff` / `fslc mutate` consumers**: `fslc diff`
and the `fslc mutate` implements oracle no longer treat a refinement
correspondence check cut off by its fixed state budget (50,000 states, #1041)
as a pass. A `fslc diff` direction that hits the budget now reports
`result: "unknown_budget"` with `states_explored` (no `checked_to_depth`)
instead of `refines`, and the diff adds an `unknown_budget` finding (also in
`summary`) that fails the gate whatever `--forbid` says, like `impl_violated`;
`fslc diff --git` and `fslc approval diff` report the same and exit 1. Before,
both directions of such a diff could report `refines`, giving
`no_semantic_change` / exit 0 even under `--forbid behavior_removed` when the
difference lay beyond the budget. A `fslc mutate` mutant whose implements check
hits the budget is now `status: "inconclusive"` with
`inconclusive: {"reason": "unknown_budget", "states_explored": N}` and
`killed_by: null`, instead of `survived`; it is excluded from `kill_rate`,
counted in `summary.inconclusive` and `summary.by_source.*.inconclusive`
(present only when non-zero), and fails a requested `--fail-on-survivors` /
`--min-kill-rate` gate with the violation `inconclusive` (`gate.inconclusive`
carries the count). `RefinementCheck`'s fields are now private: Rust callers of
`fsl_runtime::check_refinement` read the outcome through `verdict()` and the
names/depth/action map through accessors. Migration: a diff that used to exit 0
with `no_semantic_change` on budget-scale specs now exits 1 with
`unknown_budget`; lower `--depth` or narrow both specs' `verify {}` domains. A
mutate consumer that assumed every mutant is `killed`, `survived` or `invalid`
must handle `inconclusive`; runs without a cut-off mutant print exactly the same
JSON as before.
