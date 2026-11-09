Fixed (#1799): in an FSL 5 `requirements` document, two `only after` rules
whose prerequisite events joined to one history field name (the events
`{a, b}` and a single event named `a_or_b` both gave `did_a_or_b`) shared
that field, so one rule's prerequisite satisfied the other: a scenario that
skipped the required event was `ok`, `verified` and `proved`, and a valid
order could be refused. Each prerequisite event set now has its own field;
sets whose names collide get numbered fields (`did_a_or_b_1`,
`did_a_or_b_2`) independent of rule order. Field names of documents without
such a collision are unchanged.
