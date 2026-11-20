Changed (#1868, performance): `explain` and `html` no longer ask the
`action_coverage` probes and vacuity lanes in their counterfactual runs
(screening, per-invariant, replayed and reachable-phase runs), whose answers
the counterfactual analysis never read. Output is unchanged: both probe
families are scoped queries that assert nothing, so the verdicts are the same,
and a printed counterfactual trace is re-solved from the assertions alone
(#1940), so the skipped queries cannot move it. `explain` stdout, stderr and
exit code are byte-identical apart from the wall-clock and memory fields on
every `.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` that
finishes in 120 s within 6 GB, and `explain` and the `html` report are on eight
specs with printed counterfactual traces at depths 8 and 4. On
`examples/agentic_rag/agentic_rag_design.fsl`, `html` takes about 275 s
instead of 852 s at the default depth 8 and 26 s instead of 112 s at
`--depth 4`, and `explain` about 266 s instead of 858 s at depth 8. The
witness run and `verify` keep both probes.
