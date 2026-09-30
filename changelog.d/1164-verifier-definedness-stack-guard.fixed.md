Fixed (#1164): `verify` (every engine: bmc, induction, explicit, auto) and
`check` no longer abort with a stack overflow (exit 134, no JSON envelope) on
deeply nested expressions. Six recursions over the expression tree ran
without the #620 stack guard and now enter `recursion::guard` at their cycle
entry: the concrete evaluator `fsl_runtime::eval`, the verifier's definedness
walk `evaluation_status_with_policy`, induction's `helpful`-argument fold
`eval_state_independent`, and `fsl-core`'s `extend_pattern_binding`,
`reserved::check_expr`, and `expr_text_with_origins`. On a release build a
left-nested `x + ... + x` invariant previously aborted every engine at 4000
terms and `check` at 15000; it now verifies at 20000. The `deep_nesting`
regression gains witnesses that force full evaluation (`+`, `and`, and `not`
chains) under all four engines and `check`, each shown to fail with its guard
removed. A still deeper tree aborts in the parse tree's derived `Clone`
(#1186).
