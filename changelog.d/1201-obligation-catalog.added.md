Added (#1201, #1202): `fsl_core::obligation::catalog`, a solver-free list of the
obligations a model owes -- one row per authored site and aspect (type bounds,
`invariant`/`trans`/`reachable`/`leadsTo`/`terminal` truth and definedness,
action guard/body/`ensures` definedness and `ensures` truth, `init`
satisfiability, deadlock freedom), each marked when it is statically vacuous.
Rows come from the model's declarations rather than from engine output, so a
check no engine performs still has a row. Nothing reads the catalog yet; CLI,
Worker and Public Kernel output are unchanged. See
`docs/design/DESIGN-obligation-catalog.md`.
