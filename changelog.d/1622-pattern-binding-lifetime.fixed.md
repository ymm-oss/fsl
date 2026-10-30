Fixed (#1622, #1577): pattern payloads now stay within their true continuation
in symbolic verification and concrete Monitor/solver-free BFS evaluation,
including raw/expected-type conditionals, guards and composed initializers.
Closed expressions restore outer values and declared binding types; legal
conjunctive requires flow and all existing source collision refusals remain.
Quantified diagnostic snapshots retain their matched continuation payloads in
both top-level and blame witnesses without reopening ordinary evaluation.
