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
load only the needed language and verifier detail. When working inside this
repository, read `examples/e2e/3_design.fsl` and `examples/e2e/3_refines_2.fsl`
for the main pattern, `examples/layers/` for a compact chain, and `examples/bank/`
or `examples/refinement_liveness/` when the task is mostly refinement. Use
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
3. Link every design declaration back to the requirement it serves with the
   canonical typed annotation, on the line before the declaration:
   `@requirement("REQ-CHECKOUT-001", "one-sentence intent")` before
   `invariant PaidLedger { ... }` / `action submit(...) { ... }`. The
   requirements spec owns the ID (`requirement REQ-CHECKOUT-001`); the design
   spec only links to it. Use a `MODEL-`/`ASSUME-`prefixed id for a design
   choice the requirements do not state. **Never use the `"REQ-1: text"` string
   slot** — `docs/DESIGN-id-policy.md` classifies it as non-canonical migration
   input and `fslc lint` reports it as `legacy_string_metadata`.
4. Verify the design itself with `fslc check`, `fslc verify`, and usually
   `fslc verify --engine induction`. Run `fslc lint <file>` as the tagging
   gate: it exits 1 on any non-canonical ID form, which `--strict-tags` accepts
   silently.
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

## Guardrails

- Treat `abs_requires_failed` as a design bypass of an upper guard until proven
  otherwise.
- Treat `stutter_changed_abs` as a false claim of internal-only behavior.
- Treat `progress_lost` as a lower-layer liveness failure: inspect the lasso/stall
  trace, then add/restore `fair` on the lower progress action or write a
  lower-layer ranked `leadsTo`.
- Do not model variants by editing the abstract spec each time; keep variant detail
  in the design layer and fold it through the mapping.
- Report proof categories separately: design invariants proved, refinement to
  requirements proved, implementation conformance not yet proved unless `testgen`
  or `replay` has been anchored to real code/logs.
- Author the design spec as documentation: declare domains with `entity`/`number`
  (sizes in the `verify` block, not in `type X = 0..N`), tag every action/invariant
  with `"ID: intent"` (prefix verification-only invariants `MODEL-`/`ASSUME-`), and
  prefer `forall x in coll { … }` over index quantifiers in invariant bodies. See
  `skills/fsl/references/layers.md` → "Authoring specs as readable documentation".
