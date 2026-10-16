---
name: fsl-design
description: Write and verify FSL design-layer specs for engineers from an agreed requirements contract. Use for internal state/action design, decomposition, queues/outboxes, compose, refinement mappings to requirements, typestate checks, scenarios-to-testgen handoff, and implementation-conformance planning. Do not use for PM requirements authoring or business process discovery.
---

# FSL Design Layer

Use this skill after a requirements contract exists or the user explicitly asks
for engineering design. The design layer is an ordinary kernel `spec` plus an
optional refinement mapping back to the requirements layer. That mapping is the
**refinement seam, not a plain baton pass**: the design must refine the (frozen)
requirements contract, and `fslc refine` verifies the seam. When a seam breaks,
repair the design or the mapping — not the upper contract, unless a human revises it.

Before writing syntax, read `../fsl/SKILL.md`, then use its reference index to
load only the needed language and verifier detail. The main pattern: a kernel
`spec` that declares `entity`/`number` domains (sizes in `verify`), tags each
action and invariant with `@requirement(...)`, and adds internal detail the
requirements do not have (review substages, payment submit/confirm, an outbox);
plus a `refinement` file that maps it back — `map claim_stage[c: Claim] = if
design[c].st == DesignAutoReview then Submitted else ...` collapses internal
stages onto the requirements stage, `action pay_submit(c) -> pay(c)` names the
visible step, and `action pay_confirm(c) -> stutter` hides an internal one.
Refinement syntax is in `../fsl/references/syntax.md`; the seam rules are in
`../fsl/references/layers.md`. Use
`fsl-design-review` instead when the user asks for a design review, SOLID
judgment, substitutability, or variant comparison.

## Boundary

Produce design-layer artifacts only:

- A kernel `spec` for internal design state and actions
- A refinement mapping from design to requirements when a stable requirement spec
  exists
- Verification/refinement commands and result interpretation
- Optional `testgen`, `replay`, or `typestate` handoff plan

Do not rewrite business policies or PM requirements to make the design pass. If
the upper contract is too strict or ambiguous, report the counterexample and ask
for a contract decision.

## Workflow

1. Confirm the upper contract:
   - the requirements/business file that must remain stable
   - externally observable state and actions
   - which internal actions correspond to upper actions and which are true stutters
   - open/closed boundaries and human-owned design judgments
2. Write the design spec as kernel FSL. It may include internal states, queues,
   outboxes, two-phase operations, retries, and decomposition that are absent from
   requirements, as long as they map back to the upper contract.
   Write the design so that the errors you want caught break something the
   verifier checks. `fslc verify` and `fslc refine` report a design error
   when it breaks something they evaluate, within the depth and domain sizes
   of the run. For `fslc verify` that includes declared properties
   (`invariant`, `trans`, `leadsTo`, `ensures`, `reachable`) and automatic
   checks (type bounds, partial operations, deadlock detection); `fslc refine`
   adds the correspondence to the upper contract. `acceptance` and `forbidden`
   scenarios belong to the requirements layer — a kernel `spec` that declares
   them is a parse error. They run when the requirements spec is checked; the
   design reaches that contract through `refine`, which carries safety only and
   does not replay an acceptance trace on the design. This list is
   illustrative, not complete:
   `../fsl/references/commands.md` is the reference for what each command
   checks and returns. An error that breaks nothing they evaluate is not
   reported, so an intent that no declaration states is checked only where
   an automatic check happens to cover it. A run that reports no violation
   has not thereby passed: `unknown_cti` and `unknown_budget` mean the check
   did not decide. So list the failures this design must exclude — for
   example: processed twice, lost, out of order, inconsistent after a crash,
   wrong on retry, interleaved in the middle of a multi-step procedure, a
   quantity not conserved — and for each one declare the property it would
   break, using the construct that fits that property (see
   `../fsl/references/nl-to-syntax.md`). The fit depends on what is meant:
   "not lost" can be a safety property (the item is always held somewhere) or
   a progress property (the item is eventually delivered), and the two are
   written differently. If neither the source nor the upper contract states
   the property, ask; do not invent it. Where the design has the following
   mechanisms, model them as state and actions rather than folding them into
   one step or leaving them in a comment (the order is not a ranking):
   - **Transaction boundaries**: one `action` per step the implementation
     performs atomically, with what must hold between those steps declared as
     properties.
   - **Queues and outboxes**: the queue held in `state`, with what the
     contract requires of it declared as properties (for example: nothing is
     lost, nothing is duplicated, only the head is processed).
   - **Retries and idempotency**: for example an unknown outcome, the
     idempotency key, and a release that is separate from its acknowledgement.
3. Link every design declaration back to the requirement it serves with the
   canonical typed annotation, on the line before the declaration:
   `@requirement("REQ-CHECKOUT-001", "one-sentence intent")` before
   `invariant PaidLedger { ... }` / `action submit(...) { ... }`. The
   requirements spec owns the ID (`requirement REQ-CHECKOUT-001`); the design
   spec only links to it. Use a `MODEL-`/`ASSUME-`prefixed id for a design
   choice the requirements do not state. **Never use the `"REQ-1: text"` string
   slot** — it is non-canonical migration input and `fslc lint` reports it as
   `legacy_string_metadata`.
4. Verify the design itself with `fslc check`, `fslc verify`, and usually
   `fslc verify --engine induction`. Run `fslc lint <file>` as the tagging
   gate: it exits 1 on any non-canonical ID form, which `--strict-tags` accepts
   silently.
   A bounded `verified` says nothing past the depth and the domain sizes it
   ran at, and a design with many interleavings can hold its first violation
   beyond the default depth. If induction returns `unknown_cti`, follow
   `../fsl/references/commands.md`: try `--engine explicit` for closure before
   hunting lemmas. If neither closes within the budget, raise `--depth` as far
   as the budget allows and report the deepest depth that returned `verified`
   as the bound of the claim. For properties about interactions between
   entities, see "Domain sizing" in `../fsl/references/practices.md`: a result
   at a smaller size, even a closed `proved`, does not cover a larger one.
5. Write the mapping file:
   - `map abs_var = expr` or `map abs_var[x: T] = expr`
   - `action impl_action(...) -> abs_action(...)` for visible effects
   - `action impl_action(...) -> stutter` only when the action is confirmed not to
     change abstract observable state
   - `preserve progress { respond AbsLeadsTo by impl_action, ... }` when an
     upper `leadsTo` must be checked through the mapping at refine time
6. Run `fslc refine design.fsl requirements.fsl mapping.fsl`. Repair by deciding
   whether the design is wrong, the mapping is wrong, or the upper contract needs
   human revision.
7. If implementation anchoring is required, run `fslc testgen` and wire the
   Adapter, or use `fslc replay` against execution logs.
   Generated tests and conformance vectors are sequences of design actions
   and do not enumerate interleaving or crash points inside one; where the
   design declares a procedure atomic, plan implementation tests that
   interleave or crash inside it (see `../fsl/references/impl.md` §9).

## Guardrails

- Treat `abs_requires_failed` as a design bypass of an upper guard until proven
  otherwise.
- Treat `stutter_changed_abs` as a false claim of internal-only behavior.
- Treat `progress_lost` as a lower-layer liveness failure: inspect the lasso/stall
  trace, then add/restore `fair` on the lower progress action or write a
  lower-layer ranked `leadsTo`.
- Do not model variants by editing the abstract spec each time; keep variant detail
  in the design layer and fold it through the mapping.
- When you change an existing design spec, re-run `fslc verify` on the changed
  design and `fslc refine` against the unchanged requirements, and, when the
  old and new design share state and action names, also run
  `fslc diff old_design.fsl new_design.fsl --forbid behavior_removed`.
  `refine` passes a change that only makes the design refuse more (a
  strengthened guard); `diff` reports it as `behavior_removed`. Decide whether
  that refusal is intended before accepting or reverting the change (see
  `../fsl-design-review/SKILL.md`, Step 3). These checks are bounded by their
  depth: a violation that first appears beyond the checked depth is not
  reported. An `unknown` finding or an `unknown_budget` result from `diff` is
  undecided; do not read it as "no behavior removed".
- Report proof categories separately: design invariants proved, refinement to
  requirements proved, implementation conformance not yet proved unless `testgen`
  or `replay` has been anchored to real code/logs.
- Author the design spec as documentation: declare domains with `entity`/`number`
  (sizes in the `verify` block, not in `type X = 0..N`), tag every action/invariant
  with `@requirement("ID", "intent")` as in step 3 (prefix verification-only
  invariants `MODEL-`/`ASSUME-`), and
  prefer `forall x in coll { … }` over index quantifiers in invariant bodies. See
  `skills/fsl/references/layers.md` → "Authoring specs as readable documentation".
