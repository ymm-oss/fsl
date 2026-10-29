Changed (#1515): breaking removal of executable local bindings with the same
name as any `const`, including non-integer constants. `check` now rejects
quantifier and aggregate binders, statement `forall`, `leadsTo` binders, action
parameters and `let` bindings, and `def` parameters, including their requirements
and compose-component forms. Rename the local name and its uses, for example
`forall N in 0..1 { N <= 1 }` to `forall k in 0..1 { k <= 1 }` when `const N`
exists. Diagnostics name the binding and locate its existing AST owner, not
the binding-name token; qualified component constants and legal state-name
shadowing remain available. General dependent range bounds are not added.
The original pre-migration regression inventory includes 152 removed inline
forms: 126 compose-capture
forms (#1442; e.g. `const x = false` with `action go(x: Bool) { requires x }`),
24 nested-aggregate forms (#1463; e.g. `const x = false` with
`sum(x in 0..1 of count(y in 0..x)) == 3`), and 2 direct/composed helpful-argument
forms (#1333; `const c = 0` with local `c` and `helpful step(c)`). Rename the
local name and its uses; 126 fresh compose controls remain accepted. Five
partial-init forms (#1481) remain error→error but now report const-shadowing
before ownership, changing diagnostic priority and owner locations. The
same rule also rejects documented implicit process c, next-edition entity
instance first-letter names and relation actor-instance params, at the source
process/entity/event declaration even when no local declaration was written.
Rename a conflicting const in those implicit-name cases. Hidden KPI projection
binders instead freshen only on collision. Nested init with const i=1 and
`forall i in 0..0 { forall j in 0..i { m[j] = true } }` is an additional removal:
it checked on base c32e7f5a and now refuses the outer forall at 5:10. Rename the
local and its bound uses. Def-call argument binders are checked before expansion
even if an unused formal erases them, with the same owner/message as retained
arguments. Fresh corpus/inline evidence is recorded in the design note's
verification snapshot, separately from these source removals.
