Breaking removal (#1685): a struct literal that names a field twice is refused at the repeated field, in every position and edition, with exit 2, `kind: "parse"`, `diagnostic_code: "duplicate_struct_field"` and `duplicate field 'P.x' at 5:24`. A component reports `component "piece.fsl" failed to parse (duplicate field 'P.x' at piece.fsl:6:32) at root.fsl:2:3` with `kind: "semantics"`, an inline `implements` reports `implements "abs.fsl" failed to parse (duplicate field 'P.x' at abs.fsl:6:33)` with `kind: "type"` and `loc` on the `implements`, and a refinement mapping reports `duplicate field 'P.x'` with `kind: "type"` and `loc` at the field.

These forms passed `check` before and kept only the last value, so swapping the values reversed the verdict:

- `P { x: 1, x: 2, y: 0 }`, in either order or with equal values, in `init`, an action, a guard, a property, a `def`, a map value or a scenario, in any edition.
- The same literal in a compose component, whether its action is lifted, synchronized, or internal and unreferenced.
- The same literal in a refinement mapping or an inline `implements` abstraction.

`verify` no longer returns `violated`, `verified` or `proved` on those forms, `refine` no longer returns `refines`, `impl_violated` or `refinement_failed`, `replay` no longer returns `conformant` or `nonconformant`, and `scenarios`, `mutate`, `testgen`, `html`, `ledger`, `diff`, `lint`, `fmt`, `explain`, `analyze`, `conformance`, `kernel` and `typestate` exit 2 instead of their own results. A `domain` value object keeps its own refusal at `check`.

Migration: keep the one value you mean. There is no automatic migration.

The 646-form on-disk corpus has an empty transition set: base `3901764f0fd07c3e98a5cc78b8ca341a6d1276dd`, head `7d1fb36f653427bbfe13844d6b963720ab1df2d2`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

Rust API: `fsl_syntax::DUPLICATE_STRUCT_FIELD`.
