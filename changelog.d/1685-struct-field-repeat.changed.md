Changed (#1685, breaking): a struct literal that names a field twice is refused at the repeated field, in every position and edition, with exit 2, `kind: "parse"`, `diagnostic_code: "duplicate_struct_field"` and `duplicate field 'P.x' at 5:24`. A component reports `component "piece.fsl" failed to parse (duplicate field 'P.x' at piece.fsl:6:32) at root.fsl:2:3` with `kind: "semantics"`, an inline `implements` reports `implements "abs.fsl" failed to parse (duplicate field 'P.x' at abs.fsl:6:33)` with `kind: "type"` and `loc` on the `implements`, and a refinement mapping reports `duplicate field 'P.x'` with `kind: "type"` and `loc` at the field.

These forms passed `check` before and kept only the last value, so swapping the values reversed the verdict:

- `P { x: 1, x: 2, y: 0 }`, in either order or with equal values, in `init`, an action, a guard, a property, a `def`, a map value or a scenario, in any edition.
- The same literal in a compose component, whether its action is lifted, synchronized, or internal and unreferenced.
- The same literal in a refinement mapping or an inline `implements` abstraction.

An input that already failed now reports the repeat, so its kind can change: `P { x: Q { a: 1, a: 2 }, y: 0 }` from `type` to `parse`, and an undefined `Q { a: 1, a: 2 }` from `semantics` to `parse`. In `fslc-lsp` the refusal is a parse diagnostic at the repeated field, and like any parse error it leaves `documentSymbol` returning null until the literal is fixed.

`verify` no longer returns `violated`, `verified` or `proved` on those forms, `refine` no longer returns `refines`, `impl_violated` or `refinement_failed`, `replay` no longer returns `conformant` or `nonconformant`, `sweep --depth 0..1` no longer returns `sweep_failed` or `sweep_passed`, and `migrate`, `document claims`, `scenarios`, `mutate`, `testgen`, `html`, `ledger`, `diff`, `lint`, `fmt`, `explain`, `analyze`, `conformance`, `kernel` and `typestate` exit 2 instead of their own results. A `domain` value object is refused in `check` and every `domain` command, with `kind: "semantics"`: `domain expand` and `domain analyze` exited 0 before, and `expand` wrote both copies to a file `check` refused. `domain expand` now writes nothing.

Migration: keep the one value you mean. There is no automatic migration.

The 651-form on-disk corpus has an empty transition set: base `56354e477544f9be1f4477ddf036e5855d514bb5`, head `0d869b09886c1f627bf21bcc61394477dcceb4e7`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

Rust API: `fsl_syntax::DUPLICATE_STRUCT_FIELD`.
