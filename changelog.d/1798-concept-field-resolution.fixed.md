Fixed (#1798): in an FSL 5 `requirements` document, a field name in a
`concept` condition is the stored entity field in every expression form. Inside
`abs(...)`, `min`/`max`, a method call such as `.contains(...)`, an `is`
pattern test, a quantifier, or `count`/`sum`, the name used to read the event
input of the same name, so an `only for <Concept>` guard, and a decision-table
row naming the concept, admitted an event the concept excludes. `check`, BMC
and induction then accepted such a spec wrongly; they now refuse it (for
example a scenario that relies on the excluded event fails with
`kind: "acceptance"`, exit 2). A quantifier or aggregate binder named like a
field still shadows the field inside its scope. One shared resolver with no
catch-all now spells fields for concepts, decision tables and event
expressions, so a new expression form cannot skip resolution.
