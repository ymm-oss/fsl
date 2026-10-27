Changed (#1442): `check` now validates each `leadsTo` helpful-action argument
with the ordinary expression typechecker against the corresponding action
parameter type, in the property's binder scope. Previously unchecked argument
expressions with wrong types, unknown names, or forbidden pattern shadowing
now reject before verification. Name and arity checks remain unchanged.
Diagnostics identify the helpful clause; individual argument spans and the
state-independence rule remain follow-ups in #1483.
Composition also preserves original-source pattern refusals after aggregate
normalization: prefixing cannot rescue a nonempty `count` or `sum` whose
pattern shadows an original global. Empty aggregates remain accepted when
their standalone source accepts them. No engine semantics or bypass is added.
