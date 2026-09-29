Fixed (#1164): `verify --engine bmc` and `--engine induction` no longer abort
with a stack overflow (exit 134, no JSON envelope) on a deeply nested
expression. The definedness walk that runs beside symbolic evaluation
(`evaluation_status_with_policy` in `fsl-verifier`) recursed over the
expression tree without the #620 stack guard; it now enters
`recursion::guard` like `eval` does. The `deep_nesting` regression gains bmc
and induction cases on a 600-deep invariant that abort on the unguarded
build.
