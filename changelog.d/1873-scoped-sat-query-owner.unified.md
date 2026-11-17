Unified (#1873): a scoped satisfiability query (push, assert, check, pop) has
one owner in `fsl-verifier`, `scoped_query` (and `scoped_check`, which
returns the solver's answer unchanged); it pops the scope on every path.
`probe`/`probe_not`, `entailed`, `terms_are_unsat`, `proven_unsat`,
`partial_witness`, `build_witness` and `rank_measure_path_is_reachable` call
it instead of spelling the sequence out, and each states what a solver
`unknown` means to it at the call site: an `OnUnknown` argument (`Fail` with
its own message, or `NotProved`), or, in `probe`, whose classification the P2
mutation scope examines in that body, its own `match`. Which error is reported when the closing `pop` fails
too, which the helpers did not agree on, stays each caller's, as a
`PopFailure` argument. `entailed` asks through `scoped_check_conjunction`,
which still builds the conjunction inside the scope, because Z3's search
statistics depend on it; if that conjunction cannot be built, the scope is
now closed instead of being left open. The solver sees the same queries in
the same order; command output and exit codes are unchanged. Four of the six
inline obligation blocks of `prove_ranked_leadstos_assuming` (helpful
fairness, sticky enabledness, plain and helpful progress) also use the owner,
keeping their `unknown` errors and pop-failure precedence; when an assertion,
check or model read fails inside one of them, the scope is now popped before
the error is returned, and a failing pop takes precedence. Non-negativity,
no-deadlock and `prove_induction_scoped` keep their own sequence, because they
interleave term construction with assertions after the push.
