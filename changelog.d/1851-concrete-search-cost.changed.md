Changed (#1851, refs #1885, performance): the explicit engine, BMC's concrete boundary pre-pass and the vacuity reachability probe cost less per state, and the pre-pass and the probe hold less memory; what they explore and report is unchanged.
The pre-pass and probe queue store ids instead of full states.
A successor the search already stored, and every explicit frontier state but the root, is not checked against the state bounds and invariants again unless an invariant reads `old()`; the step's `trans` and `ensures` still are.
Concrete evaluation no longer copies the bindings for connectives and quantifiers that cannot bind, compares and indexes names, places and literals in place, and lists a typed binder's domain from its type name.
