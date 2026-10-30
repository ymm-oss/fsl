# Decide whether FSL fits

Two independent payoffs justify a spec:

- **Verification:** some ordering/combination of flags, permissions, retries,
  asynchronous work or operations can reach a forbidden state.
- **Documentation:** a stateful feature would receive a spec/design document
  anyway; make the source readable and checkable instead of duplicating prose.

Judge interaction, not line count. Three states plus cancel/retry can matter
more than a hundred linear states. A simple CRUD flow may be a thin, low-priority
verifiable document rather than inexpressible. High-risk-first is adoption
sequencing, not a rule forbidding broader documentation coverage.

1. Can you draw states and operations? Static display/decoration without state
   usually needs ordinary tests, not a fabricated machine.
2. Is there interaction risk or documentation payoff? If neither, recommend the
   simpler tool. If documentation alone justifies it, do not invent invariants
   to make the model look stronger.
3. Is the core finite/discrete? Real time, continuous quantities, probability
   and free-text correctness are not kernel semantics. Relative discrete
   deadlines can fit; wall-clock latency cannot be proved by them.

Keep **low priority** distinct from **out of scope**. Payments/refunds,
approvals, inventory/allocation, audit/permissions, queues, screen transitions,
double-submit and timeout/retry interactions often have verification payoff.

Connectivity is a second lens, not another gate. A thin requirements contract
can be valuable when its design must preserve a control. Use the
[connected workflow](layers.md) for genuine layers and verify the seams. Do
not manufacture business/requirements/design copies for a single hard altitude:
that is abstraction tax, not added assurance. Discovery, UI/API/visual design,
coding and testing still happen in their own tools.

FSL checks a model, not stakeholder intent or the product. Without an owner
for the rules, keep findings as questions. Without a faithful Adapter/log,
do not claim implementation conformance. Mutations that nothing kills are a
reason to inspect hollowness, not a reason to invent source rules.

Treat the resulting corpus as living documentation: recheck after changes,
gate seams for impact, and use the source/explain view for onboarding.
