Changed (#1868, performance): `explain` and `html` no longer ask the
`action_coverage` probes and vacuity lanes in their counterfactual runs
(screening, per-invariant, replayed and reachable-phase runs), whose answers
the counterfactual analysis never read. Output is unchanged: the probes leave
no assertion behind, so the verdicts are the same, and a printed
counterfactual trace is re-solved from the assertions alone (#1940), so the
skipped queries cannot move it. `explain` stdout, stderr and exit code are
byte-identical apart from `elapsed_s` on every `.fsl` under `specs/`,
`examples/` and `rust/fslc/tests/fixtures/` that finishes in 120 s within
6 GB (three known slow specs not run), and the `html` report is byte-identical apart from `elapsed_s` and
`check_elapsed_s` on eight specs with printed counterfactual traces at depths
8 and 4. A counterfactual run used to fail, and contribute no counterfactual,
where a skipped question failed (a coverage probe answering `unknown`, a
failed push or pop, an error evaluating a vacuity lane or a backend error on
the check that gates the lanes; a vacuity lane answering `unknown` failed
nothing); such a run now contributes its result. No compared spec has one. On
`examples/agentic_rag/agentic_rag_design.fsl`, `html` takes about 275 s
instead of 852 s at the default depth 8 and 26 s instead of 112 s at
`--depth 4`, and `explain` about 266 s instead of 858 s at depth 8. The
witness run and `verify` keep both probes.
