Unified (#1287): `verify`'s engine decisions read the typed result instead
of the rendered envelope. `--engine auto` falls back to BMC on
`fslc_rust::verification_output::explicit_budget_unknown`, the predicate
`render_explicit_output` itself uses for `unknown_budget`, and names the
state count from `ExplicitResult.states_explored`. The deferred concrete
probe and the induction base read the solved run's `first_bmc_failure`
before it is rendered. `--lemma` adopts a candidate on its `LemmaVerdict`,
whose word is the entry's `status`, and continues, stops and recommends
lemmas on the induction run's verdict (proved, counterexample to
induction, ranking failure) instead of comparing `result`, `status` and
`violation_kind` strings. A cached envelope is still read as JSON. Command
output, exit codes and cache-key derivation are unchanged.
