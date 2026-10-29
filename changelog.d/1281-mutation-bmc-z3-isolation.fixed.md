Fixed (#1281): `fslc mutate` now checks each mutant on a fresh solver context, so
a mutant's result no longer depends on the solver queries made for earlier
mutants in the same run. This can change which property a mutant is reported as
killed by (and with it the per-obligation and per-requirement tallies), and,
where an `ensures` counterexample is replayed, whether the mutant is killed;
each mutant now gets the result it gets when checked alone. Each mutation
solver call takes about 0.7 ms longer. The faster mutate work tracked in #1281
is still open.
