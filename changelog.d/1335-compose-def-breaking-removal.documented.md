Documented (#1335): `LANGUAGE.md`, `LANGUAGE.ja.md`, the `fsl` skill's
`syntax.md` and `DESIGN-def.md` now record the 5.0.0 compose `def` change as
a breaking removal. `fslc` 4.12.0 `check` accepted, with exit 0, an unused
compose-level `def` with a repeated definition or parameter name, direct or
mutual recursion, or a call in its body to an undefined predicate or with the
wrong arity; 5.0.0 rejects each with exit 2 and the direct-spec error. The
4.12 to 5.0 table of refused forms gains a row, and `DESIGN-def.md` records the
measured transitions. A new regression test in
`issue_1335_compose_predicate_expansion.rs` covers the forms that existing
tests did not.
