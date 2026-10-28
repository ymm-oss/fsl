Fixed (#1552): Requirements `rule` conditions now accept entity fields inside
`abs`, `min`, `max`, relation builtins such as `reachable`, indexes, field
accesses, method calls, `some(...)`, `Set`, `Seq` or struct literals, the operand
of `is`, quantifier filters or bodies, and aggregate filters or values. Names
bound by a quantifier or aggregate shadow entity fields within their scope.
Dynamic range bounds such as `exists x in 0..amount` remain rejected because
the bounds must be static.

The rule check now reads these conditions the way the event is lowered. A rule
previously accepted although its event could never occur, for example one that
reuses a field name as an `is` pattern inside a quantifier (#1561), is now
rejected with `rule_conflict_unsatisfiable_guard`.
