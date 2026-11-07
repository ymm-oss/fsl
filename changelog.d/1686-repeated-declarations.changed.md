Changed (#1686, breaking): in a business or requirements document, an `instances` or `values` bound or a `kpi` written twice under one name is refused at the repeated declaration, whether or not the copies are equal, with exit 2, `kind: "semantics"`, `loc` at the second copy and `duplicate values bound for 'V' at doc.fsl:14:3`, `duplicate instances bound for 'E' at doc.fsl:13:3` or `duplicate kpi 'done' at doc.fsl:9:3`. In next-edition requirements a `concept` written twice is refused the same way, `duplicate concept 'SmallClaim' at doc.fsl:24:3`. An inline `implements` reports `implements "1_business.fsl" failed to check (duplicate kpi 'paid_claims' at 1_business.fsl:55:3)` with `kind: "type"` and `loc` on the `implements`.

These forms passed `check` before and kept one copy, so swapping the copies changed the result:

- Two `values` bounds of one number: in requirements the last was kept, and `verify` moved between `verified` and `violated`. Lowering a business document does not read `values`, and `check` went from exit 0 to exit 2 there too.
- Two `instances` bounds of one entity: the last was kept, and `check` moved between `ok` and an error.
- Two `concept` declarations of one name: the first was kept, and `check` moved between `ok` and a decision-table error.
- Two `kpi` declarations of one name: `analyze` reported only the last, and no verdict changed.

`verify` no longer returns `verified`, `proved` or `violated` on those forms, `refine` no longer returns `impl_violated`, `replay` no longer returns `conformant`, and `sweep`, `migrate`, `scenarios`, `testgen`, `mutate`, `html`, `ledger`, `diff`, `document`, `kernel`, `typestate`, `lint`, `fmt`, `explain`, `analyze` and `conformance` exit 2 instead of their own results. A `spec` with an `entity` or a `number` already refused a repeated bound and is unchanged. A `spec` with neither does not read its bounds, and repeating one there stays accepted.

Migration: keep the one declaration you mean. There is no automatic migration.

The 651-form on-disk corpus has an empty transition set: base `9c31458678cf6c70e77987c85c95f4bc0d8df503`, head `b2367d61fd8a410f9e1364301f1fab0d6b7c4cb7`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.
