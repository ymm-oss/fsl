---
name: fsl-requirements
description: Write and verify FSL requirements-layer specs for PMs/PdMs from natural-language product requirements, acceptance criteria, forbidden flows, requirement IDs, traceability, and NFRs such as permissions, audit, capacity, reliability, and SLA/deadline behavior. Use when the deliverable is a requirements specification, not business process discovery, software architecture, internal design, or implementation conformance.
---

# FSL Requirements Layer

Use this skill to formalize PM/PdM requirements as a `requirements Name { ... }`
FSL file. The deliverable is a requirements contract that development can consume:
requirement IDs, source text, operations, guards, acceptance criteria, forbidden
flows, and optional conformance to an upper business layer.

Before writing syntax, read `../fsl/SKILL.md` and `../fsl/reference.md` for the
shared FSL verifier workflow and language rules. When working inside this
repository, read `examples/pm/`, `examples/layers/return_system.fsl`, and
`examples/e2e/2_requirements.fsl` for the requirements dialect. Read
`examples/nfr/` only when SLA/deadline behavior is in scope.

## Boundary

Produce requirements-layer artifacts only:

- A `requirements` spec with `requirement REQ-n "source text" { ... }`
- `acceptance` and `forbidden` blocks for positive and negative flows
- Optional `implements BusinessName from "business.fsl" { map ... }` when a
  confirmed business layer exists
- Scenario/test skeleton output from `fslc scenarios` when requested

Do not write internal design state, architecture, queues/outboxes, service modules,
database schemas, implementation adapters, or design-to-requirements refinement
maps unless the user explicitly moves to design work. If a requirement implies a
design decision but does not state it, ask rather than choosing.

## Workflow

1. Post a requirements formalization memo in chat before writing `.fsl`:
   - glossary: externally visible entities, states, actors, operations, enums
   - requirement normalization: trigger, constraint, exception, boundary
     implication for each requirement
   - acceptance and forbidden traces, with expected outcome
   - upper business contract, if any, and candidate `implements` mapping
   - assumptions and questions that affect behavior
2. Preserve source fidelity. Requirement IDs and verbatim text belong in
   `requirement REQ-n "..."` so diagnostics and scenarios trace back to the PM
   document.
3. Encode only externally observable product behavior. State names should describe
   product/system states, not engineering mechanisms.
4. Use `branches { when ... maps ... }` only when one user-visible operation has
   multiple requirement outcomes and the upper business correspondence is confirmed.
5. Put positive examples in `acceptance`; put must-reject procedures in
   `forbidden`. Use `forbidden` for missing-guard risk because a safety invariant
   often stays silent when an invalid operation is accepted.
6. For NFRs:
   - permissions: role guard plus invariant when needed
   - audit/capacity/reliability: ordinary invariants and response properties
   - SLA/deadline: use requirements `time` and `deadline`; confirm non-vacuity by
     checking that a tighter deadline fails
7. Run `fslc check`, `fslc verify`, and for stable requirements
   `fslc verify --engine induction`. Run `fslc scenarios` to hand development the
   acceptance/forbidden skeletons.

## Guardrails

- Stop at the requirements contract. Do not continue into design just because a
  design could be inferred.
- Do not add retries, storage rules, asynchronous jobs, queues, screen internals,
  or API shape unless the source explicitly requires them.
- A green `implements` result means the requirements contract conforms to the
  agreed business layer. It does not prove any design or implementation.
- When a counterexample suggests a repair, decide whether it is a missing
  requirement, a wrong interpretation, or an intentional exception before changing
  guards or invariants.
