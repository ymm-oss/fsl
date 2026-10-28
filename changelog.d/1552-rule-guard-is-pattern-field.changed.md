Changed (#1552, breaking): `check` now rejects previously accepted unsatisfiable
requirements `rule` guards that reuse an entity field's name as an `is` pattern
inside a quantifier body or aggregate value. The conflict check reads that name
as the field, not the payload, matching the event's lowering (#1561 tracks the
pattern-name rule itself). For example,
`amount == 0 and (exists x: Amount where x == 1 { some(x) is some(amount) and amount == 1 })`
now fails with exit 2, `kind: "semantics"`, and
`rule_conflict_unsatisfiable_guard`: the event requires the field to be both 0
and 1. No spec or example in the repository has this removed shape; the
597-form corpus sweep showed no acceptance transitions. Migration: to test the
payload rather than the field, replace `some(x) is some(amount) and amount == 1`
with `some(x) is some(v) and v == 1`, using a pattern name that is not a field;
with the outer condition unchanged, this means the field is 0 and the matched
payload is 1, and the rewritten example passes `check`.
