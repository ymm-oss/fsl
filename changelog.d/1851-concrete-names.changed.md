Changed (#1851, performance): an action's successor is written in place into one copy of its pre-state instead of being merged from a map of the written variables.
Deciding whether an `and`/`or`/`=>` chain can bind a pattern no longer allocates unless more than eight of its operands that are chains themselves wait to be walked at once.
