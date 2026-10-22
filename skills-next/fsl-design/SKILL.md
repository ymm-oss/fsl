---
name: fsl-design
description: Write and verify FSL design-layer specs for engineers from an agreed requirements contract. Use for internal state/action design, decomposition, queues/outboxes, compose, refinement mappings to requirements (including requirements written with entities, events, rules, decision tables and a glossary), typestate checks, scenarios-to-testgen handoff, and implementation-conformance planning. Do not use for PM requirements authoring or business process discovery.
---

# FSL Design Layer

Use this skill after a requirements contract exists or the user explicitly asks
for engineering design. The design layer is an ordinary kernel `spec` headed
`fsl 5`, plus a refinement mapping back to the requirements layer. That mapping
is the **refinement seam, not a plain baton pass**: the design must refine the
(frozen) requirements contract, and `fslc refine` verifies the seam. When a
seam breaks, repair the design or the mapping — not the upper contract, unless
a human revises it.

Kernel syntax (state, init, actions, properties, `struct`, `Map`, `Seq`,
`compose`) is in `../fsl/references/syntax.md`; the refinement file and the
seam rules are in `../fsl/references/syntax.md` and
`../fsl/references/layers.md`. Use `fsl-design-review` instead when the user
asks for a design review, SOLID judgment, substitutability, or variant
comparison.

## What the `fsl 5` header changes here

- The kernel language is the same. The stricter rules of the edition are
  errors: `check` answers `result: "error"`, `kind: "unsupported_in_edition"`
  with `findings[]` (for example `legacy_string_metadata` for an action
  string `"REQ-1: text"`: write `@requirement("REQ-1", "text")` on the line
  before the declaration). `fslc migrate --edition 5 file.fsl` plans the
  edits and the header; `--write` applies them.
- A requirements spec of this edition is written in entities, events, rules
  and decision tables, but **the design refines its lowered kernel**, not its
  surface. The mapping names the lowered state and actions.
- The glossary is not read in a design spec or a refinement file. Write both
  in canonical (ASCII) names; a requirements spec written in Japanese has the
  same lowered names as its English spelling, and the same mapping refines
  either.

## Reading the upper contract

Run `fslc explain --readable requirements.fsl` (with a glossary, add
`--names canonical`). Its "State" and "Actions" sections are what you map:

| Requirements surface | Lowered name the mapping uses |
|---|---|
| `entity Return { amount: … }` | `return_amount: Map<Return, Amount>` |
| its `lifecycle` lines | `return_stage: Map<Return, ReturnStage>`; the enum's members are the stages, in order of first appearance |
| `event request = Customer requests Return with amount Draft -> Requested` | action `request(r: Return, amount: Amount)`: the instance first, then the inputs |
| `Order belongs to User as buyer via place` | `order_buyer: Map<Order, Option<User>>`; `place` gets the `User` instance as its second parameter |
| `P only after Q` | a history field `order_did_<events of Q>` |
| `resource stock: Qty` | `stock` |
| `allow` / `never` / a decision row | an extra `requires` on the event's action |
| `always` / `enters` / `assuming` | an invariant or transition property |

With an origin-aware `fslc`, `fslc explain --readable` also prints after each
`requires` where it came from (`← move Draft -> Requested`, `← decide REQ-… row
1: …`, `← rule REQ-… allow …`), and `fslc explain --lowering` gives the same as
JSON. Read them to see which rule or decision row a refinement failure on that
action is about (`../fsl-verify/SKILL.md`, the origin self-check).

## Example

The design below refines the requirements example of
`../fsl-requirements/SKILL.md`: requests split by amount into two review
queues, and the refund is a two-step exchange with a payment provider.

```fsl check
fsl 5
spec ReturnDesign {
  entity Return
  number Amount
  const AUTO_LIMIT = 1

  enum Phase { Drafting, AutoReview, ManagerReview, Cleared, Declined, RefundSent, RefundDone }
  struct Case { phase: Phase, amount: Amount }

  state {
    cases: Map<Return, Case>
  }
  init {
    forall r: Return { cases[r] = Case { phase: Drafting, amount: 0 } }
  }

  @requirement("REQ-RET-003", "a small return goes to automatic review")
  fair action request_small(r: Return, a: Amount) {
    requires cases[r].phase == Drafting
    requires a > 0
    requires a <= AUTO_LIMIT
    cases[r] = Case { phase: AutoReview, amount: a }
  }

  @requirement("REQ-RET-004", "a large return goes to manager review")
  fair action request_large(r: Return, a: Amount) {
    requires cases[r].phase == Drafting
    requires a > AUTO_LIMIT
    cases[r] = Case { phase: ManagerReview, amount: a }
  }

  @requirement("REQ-RET-003", "the system approves a return in automatic review")
  fair action auto_approve(r: Return) {
    requires cases[r].phase == AutoReview
    cases[r].phase = Cleared
  }

  @requirement("REQ-RET-004", "a manager approves a return in manager review")
  fair action mgr_approve(r: Return) {
    requires cases[r].phase == ManagerReview
    cases[r].phase = Cleared
  }

  @requirement("REQ-RET-004", "a manager rejects a return in manager review")
  fair action mgr_reject(r: Return) {
    requires cases[r].phase == ManagerReview
    cases[r].phase = Declined
  }

  @requirement("MODEL-RET-001", "the refund is sent to the payment provider")
  fair action refund_send(r: Return) {
    requires cases[r].phase == Cleared
    cases[r].phase = RefundSent
  }

  @requirement("MODEL-RET-002", "the payment provider confirms the refund")
  fair action refund_confirm(r: Return) {
    requires cases[r].phase == RefundSent
    cases[r].phase = RefundDone
  }

  @requirement("REQ-RET-002", "only a small return is in automatic review")
  invariant AutoReviewIsSmall {
    forall r: Return { cases[r].phase == AutoReview => cases[r].amount <= AUTO_LIMIT }
  }

  @requirement("REQ-RET-005", "a requested return keeps a positive amount")
  invariant AmountPositive {
    forall r: Return { cases[r].phase != Drafting => cases[r].amount > 0 }
  }

  terminal { forall r: Return { cases[r].phase == RefundDone or cases[r].phase == Declined } }
}

verify {
  instances Return = 2
  values Amount = 0..3
}
```

The mapping (a refinement file of its own, so shown as a fragment):

```fsl-fragment
fsl 5
refinement ReturnDesignRefinesRequirements {
  impl ReturnDesign
  abs  ReturnRequirements

  map return_stage[r: Return] =
    if cases[r].phase == Drafting then Draft
    else if cases[r].phase == AutoReview then Requested
    else if cases[r].phase == ManagerReview then Requested
    else if cases[r].phase == Cleared then Approved
    else if cases[r].phase == Declined then Rejected
    else Refunded
  map return_amount[r: Return] = cases[r].amount

  action request_small(r, a) -> request(r, a)
  action request_large(r, a) -> request(r, a)
  action auto_approve(r)     -> auto_approve(r)
  action mgr_approve(r)      -> mgr_approve(r)
  action mgr_reject(r)       -> reject(r)
  action refund_send(r)      -> refund(r)
  action refund_confirm(r)   -> stutter
}
```

`fslc refine design.fsl requirements.fsl mapping.fsl` returns `refines`
(depth 8), against the English requirements spec and against its Japanese
spelling alike. Removing `requires a <= AUTO_LIMIT` from `request_small` (and
the invariant that notices it) makes it `refinement_failed`,
`abs_requires_failed` on `auto_approve`: the design would approve a large
return automatically, which the upper decision table forbids.

- Name the design's own enum members apart from the upper stage names
  (`Drafting`, not `Draft`): a mapping that sees a member in two enums is an
  error (`ambiguous enum member`).
- `refine` carries safety. An upper `policy` / `leadsTo` is checked through the
  mapping only with `preserve progress { respond … by … }`; otherwise verify
  progress in the design itself.
- Tag each design declaration with the requirement it realizes:
  `@requirement("REQ-RET-003", "…")` with the upper spec's rule, decision
  table or row ID; `MODEL-` / `ASSUME-` for a design choice the requirements
  do not state.

## Boundary

Produce design-layer artifacts only:

- A kernel `spec` for internal design state and actions
- A refinement mapping from design to requirements when a stable requirement
  spec exists
- Verification/refinement commands and result interpretation
- Optional `testgen`, `replay`, or `typestate` handoff plan

Do not rewrite business policies or PM requirements to make the design pass.
If the upper contract is too strict or ambiguous, report the counterexample and
ask for a contract decision.

## Workflow

1. Confirm the upper contract: the requirements/business file that must remain
   stable, its lowered state and actions (above), which design actions
   correspond to which upper actions and which are true stutters, and the
   human-owned design judgments.
2. Write the design spec as kernel FSL headed `fsl 5`. It may include internal
   states, queues, outboxes, two-phase operations, retries, and decomposition
   absent from the requirements, as long as they map back. List the failures
   the design must exclude (processed twice, lost, out of order, inconsistent
   after a crash, wrong on retry, interleaved in the middle of a multi-step
   procedure, a quantity not conserved) and declare for each the property it
   would break (`../fsl/references/nl-to-syntax.md`); if neither the source nor
   the upper contract states the property, ask. Model transaction boundaries,
   queues/outboxes, and retries/idempotency as state and actions, not as one
   step or a comment. `acceptance` / `forbidden` belong to the requirements
   layer; a kernel `spec` that declares them is a parse error.
3. Tag every action and invariant with `@requirement("ID", "intent")`.
4. Verify the design itself: `fslc check`, `fslc lint <file>` (no findings),
   `fslc verify --depth 8`, `fslc verify --engine induction`; for
   traceability `fslc check <file> --strict-tags --requirements ids.txt`.
   A bounded `verified` says nothing past its depth and domain sizes.
5. Write the mapping file: `map abs_var[x: T] = expr` for every lowered state
   variable of the upper spec, `action impl(...) -> abs(...)` for visible
   effects, `action impl(...) -> stutter` only when the action changes no
   mapped state, and `preserve progress { respond AbsLeadsTo by impl_action }`
   when an upper progress property must hold through the mapping.
6. Run `fslc refine design.fsl requirements.fsl mapping.fsl`. Repair by
   deciding whether the design is wrong, the mapping is wrong, or the upper
   contract needs human revision.
7. If implementation anchoring is required, run `fslc testgen` and wire the
   Adapter, or use `fslc replay` against execution logs. Generated tests do not
   enumerate interleavings or crash points inside one action; plan
   implementation tests for those (`../fsl/references/impl.md` §9).

## Guardrails

- Treat `abs_requires_failed` as a design bypass of an upper guard — an
  `allow` / `never` rule or a decision row — until proven otherwise; the
  origin of that `requires` names which.
- Treat `stutter_changed_abs` as a false claim of internal-only behavior.
- Treat `progress_lost` as a lower-layer liveness failure: inspect the
  lasso/stall trace, then add/restore `fair` on the lower progress action or
  write a lower-layer ranked `leadsTo`.
- Do not model variants by editing the abstract spec each time; keep variant
  detail in the design layer and fold it through the mapping.
- When you change an existing design spec, re-run `fslc verify` on it and
  `fslc refine` against the unchanged requirements, and, when old and new
  share state and action names, `fslc diff old.fsl new.fsl --forbid
  behavior_removed,unknown` (plus the kinds your change policy rejects;
  `../fsl-verify/SKILL.md`). `refine` passes a change that only makes the
  design refuse more; `diff` reports it as `behavior_removed`.
- Report proof categories separately: design invariants proved, refinement to
  requirements proved, implementation conformance not yet proved unless
  `testgen` or `replay` has been anchored to real code/logs.
- Author the design spec as documentation: declare domains with `entity` /
  `number` (sizes in `verify`), tag every action/invariant, and prefer
  `forall x in coll { … }` over index quantifiers in invariant bodies.
