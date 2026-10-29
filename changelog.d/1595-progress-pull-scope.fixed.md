Preserved refinement progress now resolves abstract Int/Bool constants and
binders in the abstract spec's scope, including bounds, filters and decreases.
Actual mapped-name capture by outer property/pattern binders, impl-constant binder
collisions, and abstract constant/state-or-local ambiguities are located type errors.
The ambiguous const/state bound witness gives refine exit 2 versus abstract-only
BMC exit 1 violated. Unused maps do not cause capture refusal; nested diagnostics
may still rename binders.
Violations with an abstract constant bound and renamed implementation constant,
or an abstract constant body read without an implementation namesake, now report
exit 1 `progress_lost` instead of exit 2 errors. Refs #1595.
