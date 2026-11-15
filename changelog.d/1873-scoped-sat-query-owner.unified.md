Unified (#1873): a scoped satisfiability query (push, assert, check, pop) has
one owner in `fsl-verifier`, `scoped_query` (and `scoped_check`, which
returns the solver's answer unchanged); it pops the scope on every path.
`probe`/`probe_not`, `entailed`, `terms_are_unsat`, `proven_unsat`,
`partial_witness`, `build_witness` and `rank_measure_path_is_reachable` call
it instead of spelling the sequence out, and each states what a solver
`unknown` means to it as an `OnUnknown` argument: `Fail` with its own
message, or `NotProved`. Which error is reported when the closing `pop` fails
too, which the helpers did not agree on, stays each caller's, as a
`PopFailure` argument. `entailed` asks through `scoped_check_conjunction`,
which still builds the conjunction inside the scope, because Z3's search
statistics depend on it; if that conjunction cannot be built, the scope is
now closed instead of being left open. The solver sees the same queries in
the same order; command output and exit codes are unchanged. The inline obligation blocks of
`prove_ranked_leadstos_assuming` and of `prove_induction_scoped` keep their
own sequence.
