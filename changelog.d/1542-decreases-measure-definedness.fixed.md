Fixed (#1542): native induction checks reached partial operations in `leadsTo`
`decreases` measures before using initial, successor or deadlock-side rank values.
A possibly undefined measure now produces an exit-2 semantics error naming the
operation instead of a fabricated rank value or proof. Short-circuit guards,
zero-slot Seq preflight errors and total-measure solver counts are preserved;
BMC falls back to its lasso search when ranking refuses the measure. The exit-2
refusal applies to the rank-proof phase; a real base-BMC counterexample remains
`violated` (exit 1) if its optional rank diagnosis is refused.
