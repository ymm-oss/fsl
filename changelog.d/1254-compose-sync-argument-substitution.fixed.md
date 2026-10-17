Fixed (#1254): a compose sync action now substitutes its arguments into the
component action with capture-avoiding scoping. A statement `forall`'s
`where` and range bounds are substituted (they used to reach the Kernel with
the unbound parameter name, exit 2), its binding shadows a same-named
parameter (exit 2 on write-index distinctness before), and neither a
`forall` binding nor a `let` captures an argument or sync parameter spelled
like it. Migration: a compose whose sync argument shared a name with a
component's `forall` / `let` binding, or whose `where` read a compose state
variable named like the component parameter, could report `verified` for an
action that breaks the component's invariant; it now reports `violated`
(exit 1).
