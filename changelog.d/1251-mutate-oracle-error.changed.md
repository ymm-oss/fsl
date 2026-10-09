Changed (#1251), **breaking for `fslc mutate` consumers that read `status`,
`summary`, or `gate`**: a mutant the oracle could not judge is no longer
counted as killed. Z3 not being creatable (previously `killed_by:"internal"`),
a BMC call the solver could not decide (solver `unknown`, a backend failure, or
an unreadable model; previously `killed_by:"build_spec"` for built-ins and
`invalid` for external mutants), and an acceptance/forbidden or implements
oracle error (previously the error text, or `refinement`, as the built-in
killer and `invalid` for external mutants) now give `status:"error"`,
`killed_by:null`, and `error:{stage,message}` with `stage` one of `solver`,
`bmc`, `requirements`, `implements`. These mutants are excluded from both
sides of `kill_rate`, counted in the new `summary.errored` (also per source in
`summary.by_source`; like #1262's `summary.inconclusive`, the key appears only
when non-zero, so a fully decided run's envelope is unchanged), and listed in a
note. With `--fail-on-survivors` or
`--min-kill-rate`, any such mutant adds the gate violation `oracle_errors` and
the `gate.errored` count (when non-zero), so the gate fails closed (exit 1); without a gate
the exit code is unchanged. A BMC rejection that is a finding about the mutant
(for example an action body undefined in a reachable state) is still the
`build_spec` kill. A built-in mutant that does not lower/build is now
`invalid` with `invalid:{kind:"semantics",message}`, as an external one
already was, instead of a `build_spec` kill. `error` stays distinct from #1262's budget `inconclusive`; for one mutant
`killed` > `error` > `inconclusive` > `survived`, decided in one place for
built-in and external mutants. An implements oracle error (for example a
widened parameter domain the abstraction rejects, such as `type_bound_lo_minus1`
in #1262's fixture) is now `error` instead of a `refinement` kill. The
`_bounds_<state>` attribution
for a removed init assignment no longer overwrites an oracle error; this
refines #1283's rule, which kept any `build_spec`/`internal` killer: a
semantic `build_spec` kill is a judged outcome and is re-attributed to
`_bounds_<state>` like any other kill, while an oracle error (including the
former `internal`) keeps `status:"error"`. The
verifier marks solver failures with `VerifyError::is_solver_failure`.
Migration: read `summary.errored` / `status:"error"` alongside
`killed`/`survived`/`invalid` (`total` = `killed + survived + invalid +
errored + inconclusive`, absent counts read as 0); a gated run that now fails with `oracle_errors` had mutants whose
kills were never established — rerun, or fix the
oracle error reported in `error.message`.
