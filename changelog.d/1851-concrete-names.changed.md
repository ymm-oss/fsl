Changed (#1851, performance): an action's successor is written in place into one copy of its pre-state instead of being merged from a map of the written variables.
Deciding whether an `and`/`or`/`=>` chain can bind a pattern no longer allocates unless more than eight of its operands that are chains themselves wait to be walked at once.
The explicit engine takes the vacuity probe's verdicts from its own walk, as BMC's pre-pass does, when that walk checked every bound, was not stopped by a violation or its state budget, and stored fewer states than the probe's budget; otherwise the probe walks on its own as before.
Cloning an enum or struct value shares its type and member names instead of copying them.
A quantifier rebinds its binder in place instead of inserting its name again for every candidate.
