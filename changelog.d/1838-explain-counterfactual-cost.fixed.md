Fixed (#1838): the counterfactual phases of `fslc explain` and `fslc html`
skip runs whose result is already known: one run with all invariants screens
out those no weakening breaks, fair removals that differ only in `fair` share
one analysis, a preempted target's concrete search runs once instead of once
per step count, and the reachable phase reuses the invariant phase's runs and
skips specs with no reachable. The shortcuts run on a separate Z3 context, and
an outcome whose trace the solver chose is recomputed from the runs made in
the former order, so output is unchanged byte for byte apart from
`cost.elapsed_s`. Unit tests pin the number of bounded runs and concrete
searches on a small spec and compare the output with making every run.
