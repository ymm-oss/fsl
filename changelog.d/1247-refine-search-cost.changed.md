Changed (#1247, #1174): the concrete refinement check (the impl
self-consistency pre-pass and the correspondence walk behind `fslc refine`,
`fslc chain`, inline `implements`, `fslc diff`, and `fslc mutate`'s implements
oracle) costs less per state; what it explores and reports is unchanged
(stdout byte-identical). Expression reads such as `design[c].st` no longer
copy the whole state variable, a step copies the state once instead of four
to five times, the walk reuses one abstract `Monitor` instead of copying the
abstract model twice per transition, and both searches keep their visited
states and parent links packed (one canonical byte string per state instead
of three full copies). On `examples/e2e` with `instances Claim = 4`,
`values Amount = 0..3`, `--depth 8`, user CPU time drops from 14.6 s to 3.1 s
and peak memory from 939 MB to 190 MB; with `values Amount = 0..5` the run
still stops at the 50,000-state budget (`unknown_budget`), now at 34 MB
instead of 1.2 GB. No migration is needed.
