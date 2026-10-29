Changed (#1515): breaking removal of executable local bindings with the same
name as any `const`, including non-integer constants. `check` now rejects
quantifier and aggregate binders, statement `forall`, `leadsTo` binders, action
parameters and `let` bindings, and `def` parameters, including their requirements
and compose-component forms. Rename the local name and its uses, for example
`forall N in 0..1 { N <= 1 }` to `forall k in 0..1 { k <= 1 }` when `const N`
exists. Diagnostics name the binding and locate its existing AST owner, not
the binding-name token; qualified component constants and legal state-name
shadowing remain available. General dependent range bounds are not added.
Checking original pre-migration regression fixtures on base `173e5f97` versus
implementation `76f665e3` measured 152 removed inline forms: 126 compose-capture
forms (#1442; e.g. `const x = false` with `action go(x: Bool) { requires x }`),
24 nested-aggregate forms (#1463; e.g. `const x = false` with
`sum(x in 0..1 of count(y in 0..x)) == 3`), and 2 direct/composed helpful-argument
forms (#1333; `const c = 0` with local `c` and `helpful step(c)`). Rename the
local name and its uses; 126 fresh compose controls remain accepted. Five
partial-init forms (#1481) remain error→error but now report const-shadowing
before ownership, changing diagnostic priority and owner locations. The
separate own-archive corpus sweep of all 597 forms in `specs/`, `examples/`
and `rust/fslc/tests/fixtures/` (base `173e5f97`, head `76f665e3`) found zero
ok→error transitions and zero other output differences; it does not negate
the inline breaking removals.
