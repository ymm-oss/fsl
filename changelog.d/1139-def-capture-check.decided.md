Decided (#1139): the `def` capture check stays as a semantics error with its
current message. It guards capture-avoiding substitution at expansion time,
not the runtime binder leak #1119 removed, so #1119 could not have made it
redundant. `tests/test_named_predicates.py` now pins the two expansions of a
rejected call to opposite verdicts; see `docs/DESIGN-pattern-binding-scope.md`.
