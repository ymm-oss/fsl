## Recommended practices (optional — by risk; may be skipped for small specs)

Unlike the core skill's "rules to always follow", this is **not mandatory**. Imposing
heavy procedures on every spec kills the lightweight loop, so apply them only to
important constraints and high-risk specs.

- **Pair with a positive example**: when you write an invariant, attach one
  `reachable` or `acceptance` near its boundary showing that "behavior that should
  be allowed is still possible." This lets you self-detect over-guarding
  (over-constraint) and vacuous invariants. Especially effective when a repair
  strengthened a guard. Example: attaching `reachable SoldOut { stock[0] == 0 }` to
  a stock-decrementing spec confirms "selling out is reachable = not over-guarded."
- **One requirement = one declaration**: avoid a huge conjunctive invariant and
  split declarations per requirement. The counterexample's `requirement` tag then
  bites, diagnostics are easier to read, and which requirement broke is clear in
  one round-trip.
- **Domain sizing**: for properties about interactions between entities, use at
  least 3 entities (with 2, symmetry hides bugs); make capacities values where you
  can try "limit + 1"; and standardize checks at depth 8 + induction.
- **Cross-validation (high-risk specs only)**: for specs where errors are serious,
  such as payments or permissions, (a) have a separate agent that has not seen the
  source translate the `.fsl` into natural language and reconcile it item-by-item
  against the requirements list, or (b) fix the state schema and have two agents
  independently write the dynamics + properties, then `replay` each other's
  `scenarios` against the other's spec to expose discrepancies. Costly, so use it
  selectively.
- **Liveness negative controls must be able to stall**: removing `fair` is not a
  useful probe when every maximal execution terminates structurally. A negative
  control for `leadsTo` must admit a lasso, deadlock, or other pending stall that
  the checked progress rule is expected to reject.
