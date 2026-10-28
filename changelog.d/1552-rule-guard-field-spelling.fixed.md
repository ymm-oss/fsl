Fixed (#1552): A requirements `rule` condition that reads an entity field inside
`abs`, `min`, `max`, an index, a field access, a method, a collection literal,
an `is` expression, a quantifier, or an aggregate is no longer rejected with
`rule_conflict_unsatisfiable_guard`. Names bound by a quantifier or aggregate
shadow entity fields within their scope.
