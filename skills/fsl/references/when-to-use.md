## First, decide whether FSL fits (self-check)

Before reaching for a spec, run this filter. FSL is not for every task, and forcing
it where it does not fit wastes effort and produces hollow specs. **This is a
judgment aid, not a gate**: when neither payoff below applies, say so and recommend
the better tool (usually ordinary tests) instead of writing FSL anyway.

**Two payoffs justify writing FSL, and either alone is enough — this is not a single
verification-ROI gate:**

- **Verification payoff**: can some order of operations or combination of flags reach
  a state that must never happen?
- **Documentation payoff**: would this feature get a spec or design doc written for it
  regardless? If so, write that doc as FSL — the `.fsl` source replaces the prose doc
  you would write anyway, so it is simultaneously what people read and what the
  verifier checks, with zero drift between the two.

"Out of scope" is reserved for what FSL cannot express (gate 3 below), not for
"verification ROI too low." A linear-path or CRUD feature that would be documented
regardless is in scope as a thin verifiable doc, even with no forbidden state to
prove. Because of the documentation payoff, broad coverage across
business/requirements/design layers is the default target, not the exception —
replacing a prose doc needs no per-feature verification-ROI proof; treat high-risk-
first as adoption *sequencing* when capacity is limited, not as where coverage stops.

**The verification test — judge by _interaction_, not size:** can some **order of
operations or combination of flags** reach a state that must never happen? Even 3
states + 2 flags qualify if back / cancel / retry / permission branching is involved;
a hundred states on one linear path do not by verification payoff alone — check the
documentation payoff above before ruling a feature out.

Three gates, top to bottom. Gates 1 and 3 stop on genuine inexpressibility (no
payoff rescues those); at gate 2, check the documentation payoff before ruling a
feature out of scope:

1. **State machine?** Can you draw boxes (states) + arrows (operations)? No →
   nothing to model (static display, decoration with no state to speak of). Out of
   scope; recommend ordinary tests.
2. **Interaction can reach a bad state, or would this be documented anyway?** order /
   flags / permissions / async / retry combine into a forbidden state → verification
   payoff, write it. Otherwise, a linear path or simple CRUD flow that would still
   get a spec/design doc → documentation payoff, write it as a thin verifiable doc
   (low priority, not out of scope). Neither → tests usually suffice.
3. **Finite & discrete?** real-time values, probability, continuous quantities, or
   free-text meaning are **not** the core. No → the core won't fit the model; FSL is
   at most an aid. (SLA is fine only as a *relative, discrete-step* deadline, not a
   wall-clock value — see [layers](layers.md).)

Keep "**low priority** (possible but thin)" distinct from "**out of scope** (not
expressible)." High-yield: payments/refunds, approvals/send-backs,
inventory/allocation, permissions/audit, queues/async, SLA/timeout/retry, screen
transitions / double-submit / unsaved-changes. Out of scope: real time, probability,
continuous money, free-text correctness, absolute latency — what FSL cannot express,
not merely what scores low on verification payoff alone.

**The second lens — one of FSL's primary uses, not a fourth gate: is there
connectivity value?** The three gates score a spec *as a single island*, but that is
only half of when FSL pays off. FSL's distinctive edge over classic formal methods is
that it *also* verifies *cross-layer alignment*, so a spec that rates "low priority
(possible but thin)" on its own can be high-value once connection is the point — a
requirement provably honored by the design, a regulatory control that still bites at
the lowest layer, an As-Is→To-Be change that preserves a control. When that alignment
is the deliverable, author the layers and gate the seams with `chain` (the connected
   [connected workflow](layers.md) even if any single layer is thin; verifying the connection is a
primary use, not a tail-end advanced topic. The converse — the brake on writing *too*
much — is the **abstraction tax**: if there is really only one hard altitude, do *not*
manufacture three layers — you would just write the same thing three times at
different verbosity. Island-shaped hard spots stay single-spec, exactly as before. So
the wider you write across genuine layers, the more alignment you can mechanically
manage — but this stays a judgment lens, never a mandate to manufacture a layer that
does not genuinely exist (FSL formalizes the contract layers that are actually
there; natural-language discovery, UI/API/visual design, coding, and testing still
happen in their own tools). (Same criterion in the manual's "When to Use FSL"
chapter.)

Even past the gates, the value is conditional: **FSL checks the spec, not the
product.** If no human owns the rules, or (for conformance) no faithful Adapter/log
is feasible, keep it to lightweight pre-implementation review and do not claim
implementation conformance. A spec that no mutation kills is hollow comfort — check
`fslc mutate` kill-rate (a very low kill-rate signals a hollow spec).

The resulting spec corpus is not a one-time deliverable: treat it as a living single
source of truth, re-verified on every change (CI regression, drift detection, and
cross-layer change-impact via `refine`), and read directly — by humans and by AI — as
onboarding context for the flow it documents.

Full rationale, plus the per-feature vs per-project distinction, is the manual's
"When to Use FSL" chapter (`docs/intro/when-to-use.{ja,en}.html`).
