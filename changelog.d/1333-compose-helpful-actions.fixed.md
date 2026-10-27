Fixed (#1333): a component's `leadsTo` helpful action now references that
component's prefixed action, and its arguments are rewritten in the same
binder scope as the temporal property. Composed ranking proofs can now run
instead of failing with an undeclared helpful action (exit 2). Internal or
unknown helpful actions remain errors rather than binding to glue actions.
