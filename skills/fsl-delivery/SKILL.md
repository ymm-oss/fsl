---
name: fsl-delivery
description: Orchestrate an FSL-centered delivery workflow from business planning through PM requirements, engineering design, verification, refinement, scenarios, testgen/replay, and implementation conformance. Use when the user asks to drive a feature/project "from planning to implementation" with FSL as the backbone, connect business to requirements to design to implementation, create an end-to-end FSL development plan, or coordinate the existing fsl-business, fsl-requirements, fsl-design, and fsl-design-review skills.
---

# FSL Delivery

## Overview

Use this skill as the lifecycle coordinator for FSL-based delivery. It does not
replace the layer-specific skills; it routes work to them, preserves the layer
boundaries, and keeps proof, refinement, and implementation-conformance claims
separate.

FSL is the checkable contract spine, not the entire product process. Natural
language discovery, UI/API design, coding, and testing still happen in their
normal tools; FSL supplies the machine-checkable contracts and counterexamples
that constrain those artifacts.

## Routing

Before authoring a layer, read and follow the corresponding skill:

- Business process, As-Is/To-Be, controls, KPIs, goals: `../fsl-business/SKILL.md`
- PM/PdM requirements, acceptance criteria, forbidden flows, NFR/SLA:
  `../fsl-requirements/SKILL.md`
- Engineering design, internal state/actions, refinement mapping, testgen/replay
  handoff: `../fsl-design/SKILL.md`
- Design review, variants, substitutability, SOLID/LSP/OCP judgment:
  `../fsl-design-review/SKILL.md`
- Syntax, verifier commands, JSON repair protocol: `../fsl/SKILL.md` and
  `../fsl/reference.md`

Load only the skills needed for the current stage. Do not duplicate their syntax
rules in this skill.

## Workflow

1. Identify the entry point and scope:
   - No stable business/PM material: start with business discovery.
   - Stable business contract exists: start or update requirements.
   - Stable requirements exist: start or update design/refinement.
   - Code or logs exist: connect implementation conformance with `testgen` or
     `replay`.
   - User asks whether a change/variant is safe: use design review.
2. Post a compact delivery memo before writing new FSL:
   - target flow and risk reason
   - available source artifacts
   - planned FSL layers and filenames
   - upper contract for each lower layer
   - acceptance/forbidden examples and implementation anchor, if known
   - behavior-changing assumptions and open questions
3. Author or update one layer at a time. Do not let lower-layer detail leak upward:
   business speaks in business stages and controls; requirements speak in
   externally observable behavior; design speaks in internal mechanics.
4. Verify each layer before depending on it:
   - `fslc check <file>`
   - `fslc verify <file> --depth 8`
   - `fslc verify <file> --engine induction`
5. Connect layers downward. Each handoff is a **refinement seam (a contract), not a
   plain baton pass** — the seam itself is verified, and a green chain is exactly as
   strong as the soundness of these seams:
   - requirements implements/refines business (`implements`, reported under the
     `implements` field of the requirements `verify` JSON)
   - design refines requirements with an explicit mapping (`fslc refine`)
   - implementation conforms through generated tests or event-log replay
   - gate the whole chain at once with `fslc chain` when a manifest exists
6. Report proof categories separately. Never collapse "model is verified",
   "design refines requirements", and "implementation conforms" into one claim.

## Stage Gates

| Stage | Deliverable | Required checks |
|---|---|---|
| Business | `business` spec with policies, KPIs, goals | check, verify, induction |
| Requirements | `requirements` spec with REQ IDs, acceptance, forbidden, NFRs | check, verify, induction, scenarios; implements/refines business when present |
| Design | kernel `spec` plus mapping to requirements | check, verify, induction, refine |
| Implementation | Adapter or event log connected to real behavior | testgen pytest or replay; do not claim conformance before this |
| Review/change | proposal spec or before/after contract | verify each side, refine against frozen contract, optional mutate/vacuity |

For high-risk contracts, add:

- `fslc verify <file> --vacuity error`
- `fslc verify <file> --strict-tags`
- `fslc mutate <file> --by-requirement`
- `fslc explain <file> --readable`

## Claim Discipline

- A green business layer means the business model is internally consistent under
  the stated interpretation.
- A green requirements layer means the requirements model is internally
  consistent; if `implements` is green, it also preserves the business contract's
  checked safety obligations.
- A green design layer means the design model is internally consistent.
- A green `refine` means lower-layer observable behavior conforms to the upper
  contract for checked safety behavior.
- Liveness/progress does not automatically propagate through refinement. Verify
  response properties at each layer that owns progress.
- A green `testgen` pytest run or `replay` result is the implementation-conformance
  anchor. Without an Adapter or log, the implementation is not yet checked.

## Human Decision Gates

Stop and ask for a product/business/design decision before changing behavior when:

- a guard, exception, deadline, retry rule, ownership rule, or state transition is
  missing from the source
- a counterexample can be fixed either by weakening a contract or by changing the
  lower layer
- a refinement mapping or stutter correspondence is not obvious from the source
- an implementation Adapter must decide how to project real state into FSL state

Representation-only modeling choices, such as finite domain sizes for model
checking, may be chosen by the agent. Label them separately from product
assumptions.

## Output Format

When reporting progress or completion, include a compact status table:

| Layer | File | Result | Notes |
|---|---|---|---|
| Business | ... | proved/verified/not used | ... |
| Requirements | ... | proved + implements/refines | ... |
| Design | ... | proved + refines | ... |
| Implementation | ... | conformant/not anchored | ... |

Then list open human decisions and the next concrete command or file to inspect.

## Common Prompts

- "Use $fsl-delivery to take this cancellation flow from business policy to
  implementation conformance."
- "Use $fsl-delivery to decide which FSL layers we need for this feature."
- "Use $fsl-delivery to connect these requirements to a design spec and generated
  conformance tests."
- "Use $fsl-delivery to review whether this implementation-oriented design still
  preserves the PM requirements."
