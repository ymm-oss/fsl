---
name: fsl-requirements
description: Write and verify FSL requirements-layer specs for PMs/PdMs from natural-language product requirements, acceptance criteria, forbidden flows, requirement IDs, traceability, decision tables, and NFRs such as permissions, audit, capacity, reliability, and SLA/deadline behavior, in English and optionally with a Japanese glossary and Japanese keywords. Use when the deliverable is a requirements specification, not business process discovery, software architecture, internal design, or implementation conformance.
---

# FSL Requirements Layer

Use this skill to formalize PM/PdM requirements as a `requirements Name { ... }`
FSL file headed `fsl 5`. The deliverable is a requirements contract that
development can consume: the vocabulary (actors, entities, their lifecycles),
the events, the rules and decision tables with their requirement IDs and
source text, acceptance and forbidden scenarios, and optional conformance to an
upper business layer.

This skill owns the syntax of this layer:

- `references/syntax.md` — every requirements member, one checked spec walked
  through member by member;
- `references/keywords.md` — the English / Japanese keyword table and the
  particles;
- `references/glossary.md` — how to write a `glossary` and the Japanese
  spelling of the example below.

Expressions (`and`, `or`, `not`, comparisons, arithmetic), the `verify { }`
block, and kernel members are those of `../fsl/references/syntax.md`. **Do not
use the requirements section of `../fsl/references/layers.md` or any
`process` / `transition` example**: that is the earlier form, and a file headed
`fsl 5` rejects it (`mixed_requirements_forms`). `fslc migrate --edition 5`
rewrites an existing process-form file into the forms below.

## English is canonical

- Write the spec with English names and English keywords. The names on the
  left of every declaration are the **canonical names**: ASCII, and the only
  names the kernel, generated tests, conformance vectors and implementations
  ever see.
- Japanese is a reading layer on top: a `glossary` gives a canonical name a
  Japanese alias, every keyword of the table has a Japanese spelling, and the
  particles が・を・で・は mark the words of a phrase. Add it after the English
  spec checks, never instead of it. Both spellings lower to the same kernel.
- Requirement, scenario and policy IDs stay ASCII (`REQ-RET-001`). Quoted
  source text is kept verbatim in its own language.
- Event names (`event request = …`) and the spec name are declared canonical;
  they reach implementations.

## Faithfulness gate (definition of done)

The two failures this layer exists to prevent are **dropping** a requirement (a
thin spec) and **inventing** a rule the source never stated. Hold both ends:

- **Cover the source.** Every source requirement maps to at least one checked
  declaration — a `rule`, a `decide` table or one of its rows, an event's
  `covers`, a `scenario`, a `forbidden`, a `goal`, a `policy` — or is recorded
  in the memo as out-of-scope or needs-decision.
- **Ground the spec.** A `rule ID "text"` and a `decide ID "text"` own their
  ID and carry the verbatim text; a decision row or an event adds
  `covers ID "text"` for the requirement it serves. Use a `MODEL-` / `ASSUME-`
  prefixed ID for an explicit modeling choice the source does not state.
- **Do not invent to reach green.** If `check` / `verify` needs a guard,
  bound, stage, actor, deadline or exception the source does not state, that
  is a question for the human, not a default you may pick.
- **Make it mechanical.** `fslc check <file> --strict-tags --requirements
  ids.txt` (the memo's IDs in a file, one per line) warns
  `unreferenced_requirement` for an ID nothing carries; done = zero of those
  and zero `untagged`, read from `warnings[]` of an exit-0 `ok`. `fslc lint`
  must also report no findings (`non_canonical_id` for an ID outside the ID
  policy).

## Boundary

Produce requirements-layer artifacts only: the `requirements` spec, its
scenarios and forbidden traces, the `implements` seam to a confirmed business
layer, and `fslc scenarios` output when requested. Do not write internal design
state, architecture, queues/outboxes, service modules, database schemas,
implementation adapters, or design-to-requirements refinement maps unless the
user explicitly moves to design work (`fsl-design`). If a requirement implies a
design decision but does not state it, ask rather than choosing.

Generating a stakeholder-facing requirements document from the checked spec is
a separate activity (`fsl-requirements-document`).

## Workflow

1. Post a requirements formalization memo in chat before writing `.fsl`:
   - terms: actors, entities, the stages each entity goes through, the verbs
     (one per thing an actor does), inputs and their ranges; with a Japanese
     source, the Japanese term next to each English name (this becomes the
     glossary, step 6)
   - requirement normalization: trigger, constraint, exception, boundary for
     each requirement
   - coverage map: every source requirement ID → the declaration that will
     carry it (rule, decide row, event covers, scenario, forbidden, goal,
     policy), or out-of-scope / needs-decision
   - acceptance and forbidden traces, with expected outcome
   - upper business contract, if any
   - assumptions and questions that affect behavior
2. Write the vocabulary and the lifecycles: `number`, `const`, `enum`,
   `actor`; one `entity` per thing with a lifecycle, each field with an
   initial value; one `lifecycle A -> B -> C` line per path. Then one `event`
   per lifecycle edge (`Actor verb Entity A -> B`), or `in S` for one that
   stays in its stage; `with field` for an input; `set` for other updates;
   `fault event` for something that happens to the system rather than a step
   anyone takes (not fair).
3. Write each rule the source states as a `rule ID "text" { … }` with the
   statement that says it: `allow … only when / only for`, `never … when /
   for`, `P only after Q`, `E enters T only from S`, `always …`, `deadline`.
   A rule that rejects an operation is a guard **and** a `forbidden` trace
   that tries it (`check` warns `never_rule_without_forbidden` when the trace
   is missing). Name every `always` / `enters` statement (`AmountPositive:
   always …`): the name is the property's name in results and diagnostics.
4. Where the source decides "who handles it / which way" by case, write a
   `decide ID "text" for Stage Entity { Case => Actor verb … }` table. `check`
   proves over the field domains that the rows cover every case
   (`decide_not_exhaustive`, with the uncovered values) and never overlap
   (`decide_overlap`, with the shared values). Close a real gap with a row or
   `otherwise`; close a gap no reachable state has with `assuming Q`, which
   `verify` then proves. Do not add a row the source does not state to make
   the table exhaustive — ask.
5. Add `measure`, `goal`, `policy`, then `scenario` (positive) and `forbidden
   … then rejected` (negative) traces, then the `verify { instances … values
   … }` bounds. If the spec has intended stopping points, declare them in
   `terminal { }` (step 7 shows why).
6. Optional Japanese layer: a `glossary` as the first item, Japanese keywords
   and particles (`references/glossary.md`). Re-run the whole check loop.
7. Check loop: `fslc check` → `fslc check --strict-tags --requirements
   ids.txt` → `fslc lint` → `fslc verify --depth 8` (read
   `implements.result` when there is an upper layer) → `fslc verify --engine
   induction` → `fslc scenarios`. A requirements spec of this edition gets no
   derived `terminal { }`: a run that ends with every entity in a final stage
   is reported as a `deadlock` warning until `terminal { }` names those
   stages. Reading the results, the origin self-check with `fslc explain`,
   `mutate`, and the completion report are in `../fsl-verify/SKILL.md`.
8. For SLA / deadline requirements, use `deadline` inside the rule with a
   `time { }` block (`references/syntax.md`) and confirm the deadline can
   fail: the variant without its `urgent` event must be `violated`.

## Example

A complete spec of this layer (checked as written):

```fsl check
fsl 5
requirements ReturnRequirements {
  number Amount
  const AUTO_LIMIT = 1
  actor Customer, System, Manager, Finance

  entity Return {
    amount: Amount = 0
    lifecycle Draft -> Requested -> Approved -> Refunded
    lifecycle Requested -> Rejected
  }

  concept SmallReturn = Return where amount <= AUTO_LIMIT
  concept LargeReturn = Return where amount > AUTO_LIMIT

  event request      = Customer requests Return with amount   Draft -> Requested
  event auto_approve = System approves Return                Requested -> Approved
  event mgr_approve  = Manager approves Return               Requested -> Approved
  event reject       = Manager rejects Return                Requested -> Rejected
  event refund       = Finance refunds Return                Approved -> Refunded

  rule REQ-RET-001 "A return is requested with a positive amount" {
    allow Customer requests Return only when input amount > 0
  }

  decide REQ-RET-002 "Who decides a requested return" for Requested Return {
    SmallReturn => System approves
      covers REQ-RET-003 "Returns at or below AUTO_LIMIT are approved by the system"
    LargeReturn => Manager approves or rejects
      covers REQ-RET-004 "Returns above AUTO_LIMIT are approved or rejected by a manager"
  }

  rule REQ-RET-005 "A requested return keeps a positive amount" {
    AmountPositive: always Return not in Draft has amount > 0
  }

  terminal { forall r: Return { stage(r) == Refunded or stage(r) == Rejected } }

  measure refunded_returns = count Return in Refunded
  goal GOAL-RET-001 "A return can be refunded" some Return can reach Refunded
  policy POL-RET-001 "Every requested return is eventually decided"
    every Return in Requested must eventually be Approved or Rejected

  scenario AC-RET-001 "A small return is approved by the system and refunded" {
    Customer requests r1 with amount 1
    System approves r1
    Finance refunds r1
    then r1 is Refunded
  }
  scenario AC-RET-002 "A large return is rejected by a manager" {
    Customer requests r1 with amount 2
    Manager rejects r1
    then r1 is Rejected
  }
  forbidden FB-RET-001 "A return of zero is not requested" {
    Customer requests r1 with amount 0
    then rejected
  }
  forbidden FB-RET-002 "A manager does not decide a small return" {
    Customer requests r1 with amount 1
    Manager approves r1
    then rejected
  }
}
verify {
  instances Return = 2
  values Amount = 0..3
}
```

What the lowering makes of it, which `fslc explain --readable` prints:

- `Return` becomes an identity type of 2 instances; its fields become
  `return_amount: Map<Return, Amount>` and `return_stage: Map<Return,
  ReturnStage>`, and the stages, in the order they first appear in the
  `lifecycle` lines, become the enum `ReturnStage`.
- Each event becomes a fair action of the same name with the entity instance
  as its first parameter (`request(r, amount)`). Its stage move becomes a
  `requires` and an assignment; `with amount` stores the input in the field.
- The `allow` adds `requires amount > 0` to `request`; the table adds
  `requires return_amount[r] <= AUTO_LIMIT` to `auto_approve` and the other
  row's condition to `mgr_approve` and `reject`. `AmountPositive` becomes an
  invariant.
- The `scenario` / `forbidden` blocks are acceptance and forbidden traces
  over those actions; `r1` is instance 0.

These lowered names are what a design layer maps to (`../fsl-design/`) and
what an `implements` block below this spec's business layer is written in.

## The seam to a business layer

`implements BusinessName from "business.fsl" { … }` is written at the top of
the spec (after the `glossary`, which must be the first item). `verify` checks
the seam and reports it as `implements.result`; it passes only on `refines`. For the example above
against the business spec of `../fsl-business/SKILL.md`:

```fsl-fragment
requirements ReturnRequirements {
  implements ReturnToBe from "return_tobe.fsl" {
    action request(r, amount) -> request(r)
    action auto_approve(r)    -> decide_ok(r)
    action mgr_approve(r)     -> decide_ok(r)
    action reject(r)          -> decide_ng(r)
    action refund(r)          -> refund(r)
    map return_stage_via_Approved[r: Return] =
      return_stage[r] == Approved or return_stage[r] == Refunded
  }
  …
}
```

- The block is written in **canonical, lowered names** (`return_stage`, the
  action parameters). A glossary alias there is an error
  (`alias_in_implements`).
- An event whose name matches a business transition is matched
  automatically only when the parameters match too; the business transition's
  parameter is `c`, so write the correspondence explicitly.
- The stage enum must be the same on both sides: list the lifecycle so the
  stages first appear in the order of the business `stages` line (otherwise
  `type 'ReturnStage' is declared differently in the impl and abs specs`).
- A business no-bypass policy (`must have passed through Approved`) adds a
  history flag to the business kernel (`return_stage_via_Approved`); map it.

## Guardrails

- Completeness is part of the contract: a spec that formalizes only some
  requirements is unfinished. Cover every source requirement or record why
  not — but never close a gap by inventing a rule the source does not state.
- Carry every rejection rule the source states in a checked declaration, not
  in prose alone: an `allow` / `never` / `only after` / decision row on the
  event it restricts, plus a `forbidden` trace that tries the rejected call.
  A rule that lives only in a quoted text or a `//` comment is checked by
  nothing, and the model then accepts every call the remaining guards allow.
  If an invariant or a forbidden trace catches the gap, add the rule the
  source states (or ask) — never delete the invariant or the trace.
- A decision table states the whole case split. If `check` reports a gap or
  an overlap, the source is ambiguous or the concepts are wrong; fix the
  concepts or ask. `otherwise` is for a source that says "everything else";
  `assuming` is for a gap that the rest of the spec already excludes, and
  `verify` must prove it.
- Stop at the requirements contract. The handoff downward is a **refinement
  seam**: the design layer refines this spec with `fslc refine`, so author it
  as a contract a lower layer can honor. Upward, `implements` is the same kind
  of seam to the business layer.
- Do not add retries, storage rules, asynchronous jobs, queues, screen
  internals, or API shape unless the source explicitly requires them.
- A green `implements` result means this contract conforms to the agreed
  business layer. It does not prove any design or implementation.
- When a counterexample suggests a repair, decide whether it is a missing
  requirement, a wrong interpretation, or an intentional exception before
  changing a guard, a row or an invariant.
