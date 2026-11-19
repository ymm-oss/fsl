Documented (#1921, refs #1889, #1933): four comments and design notes now
match the code. `find_boundary_violation` visits at most `max(budget, 2)`
states, not `budget`. `DESIGN-vacuity.md` and `find_boundary_violation_reaching`
say that the separate vacuity walk also stops once every candidate is decided,
while the shared pre-pass walk keeps going. `DESIGN-verification-cost.md` lists
the `partial_op`, `reachable_diagnosis` and `vacuity` cost kinds. The
`CheckedStateCarriers::new` comment names the two rules its soundness rests on;
a certificate for a cloned frame fails closed. No behavior changes.
