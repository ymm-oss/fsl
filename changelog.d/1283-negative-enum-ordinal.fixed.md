Fixed (#1283): when an enum-valued state was left unconstrained (for example
its `init` assignment removed), `fslc verify --engine bmc` could fail with
`error` / `semantics` `negative enum ordinal in solver model` (exit 2) instead
of reporting the `_bounds_<state>` type-bound violation, depending on which
ordinal the solver picked. The counterexample projection now keeps a negative
ordinal as a raw integer witness, exactly like an ordinal past the last
member, so the run reports `violated` / `_bounds_<state>` (exit 1). In
`fslc mutate`, the same failure no longer surfaces as `build_spec`: the
built-in `assignment_remove` of such an init line no longer lists
`"build_spec"` in `--oracle-attribution` `killers`, and the equivalent
external mutant is now `killed` by `_bounds_<state>` instead of `invalid`, so
built-in and external verdicts agree. The built-in init-assignment special
case also no longer overwrites an oracle failure (`build_spec` / `internal`)
with `_bounds_<state>`; such a mutant keeps its failure in `killed_by`.
Migration: a spec that relied on the previous `error` exit 2 for this shape
now gets `violated` exit 1.
