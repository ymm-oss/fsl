Breaking removal (#1682): in next-edition requirements, `check` refuses a `scenario` or `forbidden` step that names an input twice, on every route that loads the document, with exit 2, `kind: "semantics"`, `duplicate step input 'amount' at doc.fsl:43:44` and `loc` at the repeated name. Through an inline `implements` the report is `implements "rep.fsl" failed to check (duplicate step input 'v' at rep.fsl:15:30)` with `kind: "type"` and `loc` on the `implements`; `lint` and `fmt` omit `loc`.

These forms passed `check` before and kept only the last value, so swapping the values changed the result:

- `Employee submits claim1 with amount 2, amount 1`, in either order, with equal values, or written `Employee claim1 submits with ...`.
- `従業員 が 申請1 を 申請金額 2, 申請金額 1 で 提出する`, the same input in two `で` groups, or an alias with its canonical name.
- `Agent takes t1 with assignee 0, assignee 1`, the role of a relation event.

On those forms `verify`, `sweep`, `refine`, `kernel`, `migrate`, `document claims`, `document generate`, `scenarios`, `testgen`, `mutate`, `html`, `ledger`, `diff`, `explain`, `analyze`, `conformance`, `typestate`, `lint` and `fmt` exit 2 instead of their own results. A step with a parse error keeps that error, different inputs may come in any order, and a later step may name the same input again.

Migration: keep the one value you mean. There is no automatic migration.

The 646-form on-disk corpus has an empty transition set: base `1559e8eba90f73615d4efd00fcdccc30cf525b42`, head `596881fc5e944a2548eedfe1b55d52fbea8ee344`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

Rust API: `NextEditionRequirements` gains `repeated_step_inputs`.

The design note `docs/design/DESIGN-dialects.md` holds the measured forms and routes. The language references take this text when the next edition's reference is written (#1437).
