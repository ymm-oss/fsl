Fixed (#1622, #1577): pattern payloads now stay within their true continuation
in symbolic verification and concrete Monitor/solver-free BFS evaluation,
including raw/expected-type conditionals, guards and composed initializers.
Closed expressions restore outer values and declared binding types; legal
conjunctive requires flow and all existing source collision refusals remain.
Diagnostic evidence retains successful payload matches actually reached in selected
candidate filters and closed bodies, including non-exporting children, eager
arguments, optimized indices and nonquantified fallback, in both top-level and
blame witnesses. Evidence never supplies identifier lookup; skipped children,
other candidates and nested binder-local captures remain excluded. Original
binder values and first-observed keys retain their diagnostic identity.
