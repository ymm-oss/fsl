Fixed (#1335): compose-level `def` calls now expand with the same source-local
predicate expander as ordinary specs, before alias resolution and
sync argument substitution. Definitions are removed before Kernel lowering;
duplicate, unknown, recursive, wrong-arity, and capturing calls use the
ordinary spec diagnostics. Unused duplicate or recursive compose definitions
that previously passed `check` now reject, just as in an ordinary spec.
Previously valid compose predicates could stop at
an unlowered predicate call (exit 2) instead of receiving a verdict.
