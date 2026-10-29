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
binders instead freshen only on collision: only then does the projection's
Kernel AST JSON name change; the public Kernel export is unchanged.
Nested init with const i=1 and
`forall i in 0..0 { forall j in 0..i { m[j] = true } }` is an additional removal:
it checked on base 46031415 and now refuses the outer forall at 5:10. Rename the
local and its bound uses. Def-call argument binders are checked before expansion
even if an unused formal erases them, with the same owner/message as retained
arguments. Fresh native check at base 46031415 versus measured ancestor 65bf2876
finds zero removals/other output differences across 603 corpus forms, with the
original 152 inline removals, 126 fresh accepted and five diagnostic changes.
Another 70 removal witnesses and 70 renamed controls cover the additional
boundaries; both hidden-KPI cases remain accepted. The measured ancestor has
identical Rust implementation/corpus sources to the documentation update.
The design note records this bounded evidence, not every removed program.
