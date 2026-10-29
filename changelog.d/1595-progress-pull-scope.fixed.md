Preserved refinement progress now resolves abstract Int/Bool constants and
binders in the abstract spec's scope, including bounds, filters and decreases.
Binder capture of mapped implementation names or implementation constants is
rejected with a located type error instead of reporting a false refinement pass.
Violations with an abstract constant bound and renamed implementation constant,
or an abstract constant body read without an implementation namesake, now report
exit 1 `progress_lost` instead of exit 2 errors. Refs #1595.
