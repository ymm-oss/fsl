Fixed (#1334): ordinary compose glue actions now resolve component aliases in
parameters, guards, postconditions, lets, statements, and assignment targets,
allowing direct component-state reads and writes through `alias.var` as designed.
Assignment indices are resolved in glue actions, sync additions, and compose
init alike. Sync additions that write component state through `alias.var`
now report an explicit restriction at the assignment rather than an unknown
update target. Compose init reports an explicit restriction for component
writes. Component invariants continue to check glue writes. Alias resolution
respects parameters, sequential lets, quantifiers, aggregates, patterns, and
temporal binders, so a local binding named like an alias keeps its referent.
