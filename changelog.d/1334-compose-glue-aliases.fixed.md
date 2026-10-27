Fixed (#1334): ordinary compose glue actions now resolve component aliases in
parameters, guards, postconditions, lets, statements, and assignment targets,
allowing direct component-state reads and writes through `alias.var` as designed.
Assignment indices are resolved in glue actions, sync additions, and compose
init alike. Sync additions that write component state through `alias.var`
now report an explicit restriction at the assignment rather than an unknown
update target. Compose init can initialize component state through `alias.var`;
overlap with the component's own init is rejected by the existing duplicate
initialization check, never resolved by picking one assignment. Component
invariants continue to check glue writes. Alias resolution
respects parameters, model-normalized guard bindings, quantifiers, aggregates,
logical/guard pattern continuations, and temporal binders. A pattern payload is
visible on the right of `and` or `=>`, and a `requires` also exposes it to later
guards and the normalized body/ensures; neither statement nor expression `if`
introduces it into a branch. Guards see preceding bindings; body statements
and ensures see the final guard scope regardless of their source position, so a
local binding named like an alias keeps its referent.
