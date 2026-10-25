Fixed (#1334): ordinary compose glue actions now resolve component aliases in
parameters, guards, postconditions, lets, statements, and assignment targets,
allowing direct component-state reads and writes through `alias.var` as designed.
Assignment indices are resolved in glue actions, sync additions, and compose
init alike. Sync additions and compose init still cannot write component state;
they now report an explicit restriction at the assignment rather than an
unknown update target. Component invariants continue to check glue writes.
