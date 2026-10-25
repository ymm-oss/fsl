Fixed (#1335): compose-level `def` calls now expand with the same source-local,
capture-safe predicate expander as ordinary specs, before alias resolution and
sync argument substitution. Definitions are removed before Kernel lowering;
duplicate, unknown, recursive, wrong-arity, and capturing calls retain the
ordinary spec diagnostics. Previously valid compose predicates could stop at
an unlowered predicate call (exit 2) instead of receiving a verdict.
