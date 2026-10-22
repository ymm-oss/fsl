---
name: fsl-business
description: Write and verify FSL business-layer specs and governance catalogs for consultants, business owners, and PMs explicitly working on business-flow policy. Use for As-Is/To-Be process flows, actors, case stages, business policies, controls, KPIs, goals, governance catalogs that trace controls across business specs, and reform/control-preservation checks. Do not use for system requirements, UI/API/data-model design, architecture, implementation conformance, or low-level refinement unless the task is explicitly an As-Is/To-Be business control mapping.
---

# FSL Business Layer

Use this skill to turn business process material into a `business Name { ... }`
FSL file headed `fsl 5`, and, when controls must be traced across several
business specs, a `governance Name { ... }` catalog. Keep the output at the
business/consulting level: process stages, business transitions, policies,
controls, KPIs, goals, and control preservation evidence.

This skill owns the syntax of this layer (below). Expressions and the
`verify { }` block are those of `../fsl/references/syntax.md`; the explicit
`responds` / `goal { … }` forms behind the stage forms are in
`../fsl/references/layers.md`, section "business (the consulting layer)".

## What the `fsl 5` header changes here

- The business forms are the same as before: `process` / `stages` /
  `transition`. (The `entity … { lifecycle … }` / `event` forms belong to the
  requirements layer of this edition and are not business declarations.)
- The stricter rules of the edition are errors: `check` answers
  `result: "error"`, `kind: "unsupported_in_edition"`, with one entry per
  finding in `findings[]` and its `canonical_replacement` (for example
  `legacy_string_metadata` for a `"REQ-1: text"` string on a kernel
  declaration, where the typed `@requirement(...)` annotation belongs).
  `fslc migrate --edition 5 file.fsl` plans those edits and the header
  (`missing_edition_header`); `--write` applies them.
- The glossary and the Japanese keywords are read in requirements specs only.
  Write business names in ASCII English: they are the canonical names the
  requirements layer must repeat and implementations see. Put the source's
  Japanese terms in the quoted texts and in the requirements layer's glossary.

## Boundary

Produce business-layer artifacts only:

- A `business` spec with `actor`, `entity`, `process`, `transition`, optional
  `control`, `policy`, `kpi`, and `goal`, plus a top-level
  `verify { instances Entity = N }` bound
- Optional standalone `governance` catalog when the same controls must be
  traced across multiple business specs
- Verification commands/results, usually `fslc verify ... --engine induction`
- Optional As-Is/To-Be control preservation using `fslc refine` and a mapping
  file, or a `preservation` block of the governance catalog

Do not write system requirements, acceptance criteria, UI states, service/API
shape, data models, queues, outboxes, retry algorithms, implementation adapters,
or design refinement maps from business material. If the source asks for those,
route the work to `fsl-requirements` or `fsl-design` after the business layer is
agreed.

## Example

```fsl check
fsl 5
business ReturnToBe {
  actor Customer, Manager, Finance
  entity Return

  process Return {
    stages Draft, Requested, Approved, Refunded, Rejected
    initial Draft
    transition request Draft -> Requested by Customer
    transition decide_ok Requested -> Approved by Manager
    transition decide_ng Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Finance
  }

  kpi refunded_returns = count Return in Refunded

  control CTRL-RET-001 "Every refund is backed by an approval decision"
    owner Finance
    severity high
    applies_to Return

  policy POL-RET-001 "Every requested return is eventually decided"
    satisfies CTRL-RET-001
    every Return in Requested must eventually be Approved or Rejected
  policy POL-RET-002 "No return is refunded without approval"
    satisfies CTRL-RET-001
    every Return reaching Refunded must have passed through Approved
  goal GOAL-RET-001 "A return can be refunded" some Return can reach Refunded
}

verify {
  instances Return = 2
}
```

It checks, verifies to depth 8, proves with `--engine induction`, and lints
clean. The requirements example of `../fsl-requirements/SKILL.md` implements
it (that skill's "The seam to a business layer").

## Syntax of this layer

| Form | Meaning |
|---|---|
| **actor** `A, B` | the roster; every transition's `by` names one |
| **entity** `E` | an identity; the number of cases is `verify { instances E = N }` |
| **process** `E { stages …  initial S  transition t A -> B by Actor … }` | the case's stages and the allowed business moves; each transition becomes a fair action `t(c: E)` |
| **kpi** `name = count E in S` | a count of cases, a projection that adds no state |
| **control** `ID "text" owner A severity s applies_to E` | catalog metadata; it proves nothing by itself. A control nothing satisfies is an `unused_control` warning |
| **policy** `ID "text" [satisfies CTRL] every E in S must eventually be T {or T}` | a progress rule over the fair transitions |
| **policy** `ID "text" [satisfies CTRL] every E reaching T must have passed through W {or W}` | a no-bypass rule: a hidden history flag (`return_stage_via_Approved`) plus an invariant, and an auxiliary invariant that lets a compliant spec prove by induction |
| **goal** `ID "text" [satisfies CTRL] some E can reach S` / `all E can be S {or S}` | reachability |

Intended stops need no flag: a business spec derives `terminal { }` from each
process's sink stages (stages with no outgoing transition), so a run that ends
at `Refunded` / `Rejected` is not a deadlock. `--deadlock ignore` is not the
tool for intended stops.

A standalone governance catalog (one per file, so shown here as a fragment):

```fsl-fragment
fsl 5
governance ReturnGovernance {
  authority Finance owns CTRL-RET-001
  control CTRL-RET-001 "Every refund is backed by an approval decision" owner Finance severity high applies_to Return
  delegates ReturnToBe from "return_tobe.fsl" {
    require CTRL-RET-001
  }
  preservation RefundsStayApproved {
    before ReturnAsIs from "return_asis.fsl"
    after  ReturnToBe from "return_tobe.fsl"
    preserve CTRL-RET-001
    checked_by refinement "tobe_refines_asis.fsl"
  }
}
```

`fslc check governance.fsl` checks that each delegated spec exists and that
each `require CTRL` is satisfied by a `satisfies` in that spec (or an explicit
`CTRL is satisfied_by policy|goal ID`), and runs each `preservation` refinement
at depth 8. Each result is under `governance.preservations[].result`; the first
non-`refines` one is also the command's verdict (`refinement_failed`,
`impl_violated`, `unknown_budget` exit 1).

## Workflow

1. Extract a business formalization memo in chat:
   - actors and external roles
   - business entity names and finite `verify { instances ... }` sizes
   - process stages and allowed business transitions
   - policies/controls with IDs and verbatim source text
   - KPIs/goals and what counts as completion
   - open questions about rule priority, exceptions, ownership, or timing
2. Ask for confirmation before encoding any interpretation that changes
   behavior. Representation-only modeling choices may be labeled and carried as
   comments.
3. Write the `business` spec with the header `fsl 5`, ASCII English names, and
   IDs in the ID policy (`POL-SCOPE-001`, `GOAL-SCOPE-001`, `CTRL-SCOPE-001`).
   List `stages` in the order the requirements layer will meet them: a
   requirements spec of this edition orders its stages by first appearance in
   its `lifecycle` lines, and the two stage enums must be identical for the
   `implements` seam to type-check.
4. Run `fslc check`, then `fslc lint <file>` (exit 1 with `non_canonical_id` on
   an ID outside the ID policy), then `fslc verify --depth 8` and `fslc verify
   --engine induction`. Done means lint reports no findings as well as a clean
   verify. Which result counts as clean, what to do with `unknown_cti`, and the
   completion report format are in `../fsl-verify/SKILL.md`.
5. If comparing As-Is and To-Be, keep them in separate files. Write an explicit
   mapping that states the business interpretation, then run `fslc refine
   tobe.fsl asis.fsl mapping.fsl` (or a governance `preservation`).
6. Report results in business language: which policy/control/KPI broke, the
   shortest reproduction trace, and which interpretation remains a human
   judgment.

## Guardrails

- Keep IDs and source text verbatim in `control`, `policy` and `goal`
  declarations. Diagnostics carry the policy/goal text and the satisfied
  control text back to the business reviewer.
- Use `control` for governance/catalog ownership and `policy` / `goal
  satisfies` for the checkable business rule. A bare `control` proves nothing.
- Do not invent missing exception policy, SLA semantics, escalation stages, or
  ownership rules to make the verifier green.
- A green result means the business spec is internally consistent under the
  stated interpretation. It does not prove downstream requirements, design, or
  code.
- The handoff to `fsl-requirements` is a **refinement seam, not a plain baton
  pass**: the requirements spec `implements` this business spec, so this file
  becomes the frozen contract the lower layer must refine, checked at the
  lower layer's `verify`. Author it so that seam can hold, and tell the
  requirements author what the seam needs: the transition names and their
  parameter (`c`), the stage order, and any history flag a no-bypass policy
  adds (it must be mapped in the `implements` block).
- The business spec is the readable, verified process document: reviewers read
  the `.fsl` (or `fslc explain --readable`) directly instead of a separate
  prose write-up.
