Changed (#1149): `verify` no longer pays the cubic bounded lasso search for a
`leadsTo` with `decreases` whose ranking obligations hold -- the ranking
already shows every lasso probe is unsat. The deadlock-stall and `within`
probes still run, and the verdict, `completeness:"bounded"`, and witnesses are
unchanged; a ranking that fails, errors, times out (5 s per check), or panics
falls back to the full search. At
depth 8/12/16, `tests/fixtures/rust_port/ranked_leadsto.fsl` drops from
249/741/1649 `leadsTo` checks to 45/91/153 plus 3 ranking checks. Only `cost`
changes in the envelope: `cost.properties` gains a `leadsTo_rank` row.
