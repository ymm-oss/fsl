Changed (#1940, breaking): a printed witness or trace no longer depends on
which other queries the verifier asked in the same session. Every model read
for output (reachable witnesses, violation, deadlock, `leadsTo` and
partial-operation traces and the warnings that quote them, `last_action`,
induction CTIs and the `hint`/`suggested_invariants` derived from them, rank
failures, invariant-implication counterexamples, `explain` witnesses and
counterfactual traces) is re-solved on a fresh Z3 context. With the same
binary (the same Z3 build), output is a function of the spec, the property
and the step, so a change that only adds, removes or reorders solver queries
keeps it byte-identical. This is a one-time change of witness and trace
contents in the JSON output of `verify`, `explain` and the commands built on
them; verdicts, steps, coverage, vacuity and exit codes are unchanged, as are
`cost.solver.checks` and the per-property check counts. `cost.solver`
`conflicts`, `decisions` and `propagations` can change. The browser Worker
keeps the solver's own model. `fsl_solver::SmtSolver` gains
`resolve_output_model` (default: no-op) and `fsl_solver_z3::Z3Solver` gains
`verdicts_only`.
