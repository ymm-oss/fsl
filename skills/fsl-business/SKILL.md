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

- A `business` spec with `actor`, `entity`, `process`, `transition`, optional
  `control`, `policy`, `kpi`, and `goal`, plus a top-level
  `verify { instances Entity = N }` bound
- Optional standalone `governance` catalog when the same controls must be traced
  across multiple business specs
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
   - `control CTRL-1 "..." owner Finance severity high applies_to Claim`
   - `policy POL-1 "..." satisfies CTRL-1 every Case in Source must eventually be Target`
   - `policy POL-2 "..." satisfies CTRL-1 every Case reaching Completed must have passed
     through AwaitingApproval` — the no-bypass form; desugars to an invisible history
     flag plus a kernel invariant, without descending to `requirements`
   - `goal G "..." some Case can reach Target`
   - `goal G "..." all Case can be Target or OtherTarget`
4. Run `fslc check`, then `fslc verify --engine induction`. A pure stage-graph
   business spec needs no flag for intentional stops: `terminal { }` is derived
   automatically from each process's sink stages (stages with no outgoing
   `transition`), so a spec that ends cleanly at those stages verifies with no
   flag. `--deadlock ignore` is not the tool for intended stops — it silences all
   deadlock detection, including unintended ones.
5. If comparing As-Is and To-Be, keep them in separate files. Write an explicit
   mapping that states the business interpretation, then run `fslc refine
   tobe.fsl asis.fsl mapping.fsl`.
6. Report results in business language: which policy/control/KPI broke, the
   shortest reproduction trace, and which interpretation remains a human judgment.

## Guardrails

- Keep IDs and source text verbatim in `control` and `policy` declarations.
  Diagnostics carry policy/goal text and satisfied control text back to the
  business reviewer.
- Use `control` for governance/catalog ownership and `policy`/`goal satisfies`
  for the checkable business rule. A bare `control` does not prove anything by
  itself.
- Do not invent missing exception policy, SLA semantics, escalation stages, or
  ownership rules to make the verifier green.
- A green result means the business spec is internally consistent under the stated
  interpretation. It does not prove downstream requirements, design, or code.
- If the user later asks for requirement IDs, acceptance criteria, or NFR deadlines,
  switch to `fsl-requirements` and treat this business file as the upper contract.
  The handoff is a **refinement seam, not a plain baton pass**: the requirements
  layer will `implements` this business spec, so this file becomes the frozen contract
  that the lower layer must refine, and the seam is verified at the lower layer's
  `verify` time. Author this spec so that seam can hold — do not leave controls/goals
  that only a later weakening could satisfy.
