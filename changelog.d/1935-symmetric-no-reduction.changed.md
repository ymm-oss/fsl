Changed (#1935, breaking): `symmetric type` / `symmetric enum` is still
accepted, and the Kernel JSON keeps `types[].symmetric`, but fslc no longer
uses it for any reduction. `leadsTo` lasso and stall search used to constrain
the loop head or the stalled state to a canonical permutation of each
symmetric type's per-entity rows. That is sound only for a model invariant
under renaming, and nothing checked it: order comparisons or literals of the
type, arithmetic on it, a `verify --from-state` snapshot (documented as
disabling the reduction, which it did not), maps of two symmetric types that
mention each other, and guards linking two symmetric types each hid a genuine
violation, so a violated `leadsTo` could be reported `verified`, `proved` or
`refines` (exit 0) by `verify` with any engine, `refine` progress checks and
the other commands that search `leadsTo` lassos. Those specs now report the
violation (exit 1). A spec with `symmetric` now gets exactly the result,
witnesses and counterexamples of the same spec without it; in
`examples/agentic_rag` verdicts are unchanged and some witnesses and CTIs swap
`r = 0` / `r = 1`. For the 661 corpus files without `symmetric`, `verify` and
`--engine induction` output is unchanged apart from solver statistics under
`cost` in 3 files. Six regression tests from the issue's survey compare each
`symmetric` spec with its plain twin; with the reduction restored, all six
fail.
