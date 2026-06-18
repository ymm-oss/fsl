---
name: fsl-business
description: Write and verify FSL business-layer specs for consultants, business owners, and PMs explicitly working on business-flow policy. Use for As-Is/To-Be process flows, actors, case stages, business policies, controls, KPIs, goals, and reform/control-preservation checks. Do not use for system requirements, UI/API/data-model design, architecture, implementation conformance, or low-level refinement unless the task is explicitly an As-Is/To-Be business control mapping.
---

# FSL Business Layer

Use this skill to turn business process material into a `business Name { ... }`
FSL file. Keep the output at the business/consulting level: process stages,
business transitions, policies, KPIs, goals, and control preservation evidence.

Before writing syntax, read `../fsl/SKILL.md` and `../fsl/reference.md` for the
shared FSL verifier workflow and language rules. When working inside this
repository, read `examples/consulting/` for As-Is/To-Be work,
`examples/pm/cancel_flow.fsl` for a PM-friendly business flow, and
`examples/e2e/1_business.fsl` when this business layer will be chained downstream.

## Boundary

Produce business-layer artifacts only:

- A `business` spec with `actor`, `entity`, `process`, `transition`, `policy`,
  `kpi`, and `goal`, plus a top-level `verify { instances Entity = N }` bound
- Verification commands/results, usually `fslc verify ... --engine induction`
- Optional As-Is/To-Be control preservation using `fslc refine` and a mapping file

Do not write system requirements, acceptance criteria, UI states, service/API
shape, data models, queues, outboxes, retry algorithms, implementation adapters, or
design refinement maps from business material. If the source asks for those, route
the work to `fsl-requirements` or `fsl-design` after the business layer is agreed.

## Workflow

1. Extract a business formalization memo in chat:
   - actors and external roles
   - business entity names and finite `verify { instances ... }` modeling sizes
   - process stages and allowed business transitions
   - policies/controls with IDs and verbatim source text
   - KPIs/goals and what counts as completion (`kpi name = count Entity in Stage`)
   - open questions about rule priority, exceptions, ownership, or timing
2. Ask for confirmation before encoding any interpretation that changes behavior.
   Representation-only modeling choices may be labeled and carried as comments.
3. Write the `business` spec. Prefer readable stage syntax:
   - `entity Claim` with `verify { instances Claim = 3 }`
   - `kpi paid_claims = count Claim in Paid`
   - `policy POL-1 "..." every Case in Source must eventually be Target`
   - `goal G "..." some Case can reach Target`
   - `goal G "..." all Case can be Target or OtherTarget`
4. Run `fslc check`, then `fslc verify --engine induction`. Use `--deadlock ignore`
   only for business process models where terminal stops are intentionally handled
   by goals/policies rather than kernel `terminal`.
5. If comparing As-Is and To-Be, keep them in separate files. Write an explicit
   mapping that states the business interpretation, then run `fslc refine
   tobe.fsl asis.fsl mapping.fsl`.
6. Report results in business language: which policy/control/KPI broke, the
   shortest reproduction trace, and which interpretation remains a human judgment.

## Guardrails

- Keep IDs and source text verbatim in `policy` declarations. Diagnostics carry
  this text back to the business reviewer.
- Do not invent missing exception policy, SLA semantics, escalation stages, or
  ownership rules to make the verifier green.
- A green result means the business spec is internally consistent under the stated
  interpretation. It does not prove downstream requirements, design, or code.
- If the user later asks for requirement IDs, acceptance criteria, or NFR deadlines,
  switch to `fsl-requirements` and treat this business file as the upper contract.
