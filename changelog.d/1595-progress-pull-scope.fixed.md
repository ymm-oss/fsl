Preserved progress resolves abstract Int/Bool constants in P/Q, binder domains,
supported nested filters and decreases, retaining local scope. Impl-constant binder
collisions and read abstract constant/state ambiguities are located type errors;
unused maps/otherwise legal locals stay admissible. Violations with an abstract constant bound
and renamed impl constant, or a body constant without an impl namesake, now give
exit 1 `progress_lost` instead of exit 2 errors. Refs #1595.
