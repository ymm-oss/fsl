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

## Faithfulness gate (definition of done)

The two failures this layer exists to prevent are **dropping** a requirement (a
thin spec) and **inventing** a rule the source never stated. Hold both ends:

- **Cover the source.** Every requirement in the document maps to at least one
  transition / invariant / acceptance / forbidden, or is recorded in the memo as
  out-of-scope or needs-decision. The step-1 coverage map forces the hard
  requirements in instead of stopping at the happy path.
- **Ground the spec.** Every declaration carries provenance — `covers REQ-n "..."`
  to source text, or a `MODEL:` / `ASSUME-n:` tag for an explicit modeling choice.
  An untagged declaration is an ungrounded guess, not a style nit.
- **Do not invent to reach green.** If `check`/`verify` needs a guard, bound,
  state, actor, deadline, or exception the source does not state, that is a step-1
  question for the human — not a default you may pick to reach `verified`.
- **Make it mechanical.** `fslc check <file> --strict-tags` flags every `untagged`
  declaration (an invented rule) and every `unreferenced_requirement` (a
  requirement nothing checks); **done = zero of both**. Add `--requirements <ids>`
  (the memo's source-ID list) to also catch requirements omitted entirely. Keep
  provenance in the `.fsl` as tags, not a separate file.

## Boundary

Produce requirements-layer artifacts only:

- A `requirements` spec, preferably using the process+data profile:
  `process Entity with field: Type { transition ... with/when/set/covers ... }`
- `acceptance` and `forbidden` blocks for positive and negative flows
- Optional `implements BusinessName from "business.fsl" { }`, `maps auto`, or
  explicit maps when a confirmed business layer exists
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
   - coverage map: every source requirement ID → the FSL element that will carry
     it; mark anything you cannot cover out-of-scope or needs-decision (this is
     what keeps the spec from silently dropping requirements)
   - acceptance and forbidden traces, with expected outcome
   - upper business contract, if any, and candidate `implements` mapping
   - assumptions and questions that affect behavior
2. Preserve source fidelity. Requirement IDs and verbatim text belong in
   `covers REQ-n "..."` on process transitions, or in
   `requirement REQ-n "..."` for kernel-wrapper declarations, so diagnostics and
   scenarios trace back to the PM document. Tag any declaration that is not a
   direct source requirement `MODEL:` or `ASSUME-n:` so the strict-tags gate can
   tell intended modeling from fabrication.
3. Encode only externally observable product behavior. State names should describe
   product/system states, not engineering mechanisms.
4. Use the process+data profile first for a single-entity lifecycle:
   `number Amount`, `process Claim with amount: Amount`, transition `with`
   inputs, `when` guards, `set` field updates, and `covers` traceability. Put
   finite bounds in a top-level `verify` block with `instances Claim = N` and
   `values Amount = lo..hi`.
5. Use the kernel-wrapper form (`struct` / `state` / `init`, `fair action`,
   `branches`, explicit `maps`) only for hard cases: multi-entity behavior,
   conservation rules, SLA/time, history not expressible as a carried field, or
   a user-visible operation whose data-dependent outcomes need separate upper
   correspondences.
6. Put positive examples in `acceptance`; put must-reject procedures in
   `forbidden`. Use `forbidden` for missing-guard risk because a safety invariant
   often stays silent when an invalid operation is accepted. Prefer
   `expect Entity id in Stage` for process-stage checks; use `expect <expr>` for
   other predicates.
7. For `implements`:
   - `implements BusinessName from "business.fsl" { }` auto-generates identity
     refinement when process/action/stage names match
   - `maps auto` is allowed inside `implements` for same-name kernel-wrapper
     state/actions; explicit `map` and action correspondences override it
   - auto-mapped process transitions are actor-checked; a requirements transition
     whose actor differs from the business action's actor is a check-time error
8. For NFRs:
   - permissions: role guard plus invariant when needed
   - audit/capacity/reliability: ordinary invariants and response properties
   - SLA/deadline: use requirements `time` and `deadline`; confirm non-vacuity by
     checking that a tighter deadline fails
9. Run `fslc check --strict-tags` as the faithfulness gate — not done until it
   reports zero `untagged` and zero `unreferenced_requirement` (add
   `--requirements <ids>` from the memo to also catch fully omitted requirements).
   Then `fslc verify`, and for stable requirements
   `fslc verify --engine induction`. Run `fslc scenarios` to hand development the
   acceptance/forbidden skeletons.

## Guardrails

- Completeness is part of the contract: a spec that formalizes only some
  requirements is not "minimal," it is unfinished. Cover every source requirement
  or record why not (out-of-scope / needs-decision) — but never close a gap by
  inventing a rule the source does not state.
- Stop at the requirements contract. Do not continue into design just because a
  design could be inferred.
- Do not add retries, storage rules, asynchronous jobs, queues, screen internals,
  or API shape unless the source explicitly requires them.
- A green `implements` result means the requirements contract conforms to the
  agreed business layer. It does not prove any design or implementation.
- When a counterexample suggests a repair, decide whether it is a missing
  requirement, a wrong interpretation, or an intentional exception before changing
  guards or invariants.
