Unfiltered outer `leadsTo` range binders can use already bound wrapper locals
with checked integer arithmetic and typed Boolean conditions. Preserve declared
binding types, expansion order and legacy diagnostics; refuse free state bounds
even when a const shares the name. `within` remains a nonnegative declaration
constant; collection/filtered wrappers and explicit liveness remain unsupported.
