Changed (#1515): breaking removal of executable local bindings with the same
name as any `const`, including non-integer constants. `check` now rejects
quantifier and aggregate binders, statement `forall`, `leadsTo` binders, action
parameters and `let` bindings, and `def` parameters, including their requirements
and compose-component forms. Rename the local name and its uses, for example
`forall N in 0..1 { N <= 1 }` to `forall k in 0..1 { k <= 1 }` when `const N`
exists. Diagnostics name the binding and locate its existing AST owner, not
the binding-name token; qualified component constants and legal state-name
shadowing remain available. General dependent range bounds are not added.
