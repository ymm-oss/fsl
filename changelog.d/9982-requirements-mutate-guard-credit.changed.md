Changed (#9982): `fslc mutate --by-requirement` can credit a killed mutant of
a guard that a further declaration form adds to an action to the
requirement that guard formalizes, and leaves the generated actor note of
such an action out of `by_requirement`. Parsing, diagnostics, and command
output of published documents are unchanged.
