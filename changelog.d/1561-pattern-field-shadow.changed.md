Requirements now reject `is some(name)` declarations that collide with an entity
field or relation role before field substitution and rule-conflict analysis.
Rename both the pattern and its continuation to a fresh name: replace
`some(1) is some(amount) and amount == 1` with `some(1) is some(v) and v == 1`.
This formerly accepted input is a breaking removal; three already-refused #1552
variants now receive a located pattern-shadowing diagnostic instead of a rule
conflict. Legacy guards, assignment RHS, rule properties, measures, decision
assumptions/rows and concepts (even unused) share the
gate. Event-input and quantifier/aggregate shadowing rules remain unchanged.
The own-tree `FSLC_CACHE=off <binary> check <form>` comparison
ran twice on base `f8e3726e2ead231c89cd4f52eef3dd5d652c774d` and measured head
`13676f0bca83b2433cc974468bbb00c59aef25a9` (binary SHA256
`0cab1c914a05344d8958a6f9bd5ce5a17618f87f9f7e74e279b449fbca47f314`):
610 corpus forms unchanged, all 121 supplemental forms included (original63,
prior49, added fresh1, decision/concept6, exact review2). The 27 case-addressed
inline removals, not necessarily byte-unique programs, are:

- `control_is_pattern_field_name_satisfiable_matches_base` (1).
- `expression-0`, `expression-1`, `expression-2`, `expression-3`, `expression-4`,
  `expression-5`, `expression-6`, `expression-7`, `expression-8`, `expression-9`,
  `expression-10`, `expression-11`, `expression-12` (13).
- `argument-1`, `argument-4`, `argument-9` (3).
- `event-set`, `when`, `set`, `never`, `property`, `measure`, `role` (7).
- `decision-assuming`, `unused-concept`, `review-original-collision` (3).

The original and/implication/quantifier contradictions plus `argument-7` and
`decision-row` are five error→error diagnostic changes, not extra removals.
All 2,924 executions have no repeat differences or excluded fields; no other
required-test inputs were rewritten or omitted. Later integration commits are
not this measurement. (Refs #1561)
