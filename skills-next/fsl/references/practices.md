# Recommended practices — optional by risk

Use selectively; do not turn a small spec into an unrelated process project.

- Pair an important invariant with a boundary `reachable` witness; at requirements
  level use a positive `scenario`. A repaired guard must not quietly eliminate
  intended behavior.
- Split declarations by requirement rather than one giant conjunction; tags
  then identify which intent the counterexample concerns.
- Size domains for the interactions in question. Three entities may expose bugs
  two hide; test capacity-plus-one and depth 8 plus induction where relevant.
  No fixed size proves arbitrary populations.
- For high-risk intent fidelity, use independent prose-backtranslation against
  source, or independently authored dynamics/properties on an agreed schema and
  cross-replay witnesses. Shared parsing alone is not independence. Do not
  delegate or transmit source without the task's permission.
- A liveness negative control must actually permit a pending stall, deadlock or
  lasso. Removing fair is not a detector when every execution already terminates
  structurally.
- Calibrate known bad behavior against the implementation anchor too. A green
  positive trace alone cannot show that a conformance harness detects drift.
