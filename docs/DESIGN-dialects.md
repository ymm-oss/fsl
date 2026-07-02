# FSL v3 — Dialect Implementation Design (details of Stages 1–3 in DESIGN-layers.md)

Architecture and motivation are in DESIGN-layers.md. This document is an
implementation-level specification. Inviolable guiding principle:
**make no changes to the kernel semantics**. Dialects are AST expansion
(isomorphic to compose); metadata is plumbing into display only.

## Stage 1: Traceability Metadata Plumbing

### 1.1 Kernel syntax: declaration tags

For invariant / reachable / leadsTo / action (including fair) declarations,
allow **an optional string literal immediately before the opening `{`** of the block:

```fsl
invariant PerUserCap "REQ-7: per-user purchase cap" { ... }
reachable SoldOut "AC-3: sold-out state is reachable" { ... }
leadsTo Served "REQ-9: a cart is always processed" { P ~> Q }
action submit(c: Case, a: Amount) "REQ-1: at or below the threshold is auto-approved" { ... }
```

- The string is in `"ID: text"` form. **Split on the first `:`** to get
  `meta = {"id": "REQ-7", "text": "per-user purchase cap"}`.
  If there is no `:`, then `{"id": <whole string>, "text": null}`. Strip leading/trailing whitespace.
- Each AST → spec dict element (invariant/reachable/leadsto/action) gets a
  `meta` key (None if untagged).
- The output of existing specs (untagged) must remain **byte-for-byte unchanged**.

### 1.2 Pass-through into JSON output

Add `"requirement": {"id", "text"}` to output that involves elements carrying `meta`:

| Output | Source of requirement |
|---|---|
| violated (invariant / leadsTo) | meta of the violated invariant / leadsTo |
| violated (ensures / partial_op) | meta of the action in question |
| unknown_cti | meta of the invariant that was not inductive |
| coverage diagnostic object (covered: false) | meta of the action in question |
| scenarios reach_* / respond_* | meta of the property |
| scenarios cover_* | meta of the action |

- Auto-generated elements such as `_bounds_*` / `_partial_*` carry no meta (as before).
- Pass-through into refine output is done in Stage 2 (implements).
- Do not add new subcommands (`fslc trace` is considered in Stage 2 or later).

### 1.3 Tests (tests/test_meta.py)

The requirement field for a tagged invariant violation / coverage false /
unknown_cti / scenarios, a tag without a `:`, output invariance for untagged
specs (all existing tests green is the proof), and that check accepts tagged syntax.

## Stage 2: the fsl-req dialect

The file extension stays `.fsl` (discriminated by the top-level keyword).
Add `requirements <Name> { ... }` at top level, and the expander
**`expand_requirements(ast, base_dir) -> ast` in `src/fslc/dialects.py`**
converts it into kernel AST (an ordinary spec). The same wiring point as compose.

### 2.1 Syntax

```fsl
requirements ExpenseRequirements {
  implements ExpenseToBe from "1_business.fsl" { }     // identity refinement is synthesized when names match

  number Amount
  const AUTO_LIMIT = 1

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit       Draft     -> Submitted by Employee with a: Amount when a > 0 set amount = a covers REQ-1 "The applicant submits an expense claim by entering an amount"
    transition auto_approve Submitted -> Approved  by System  when amount <= AUTO_LIMIT covers REQ-2 "Claims at or below AUTO_LIMIT are auto-approved by the system"
    transition mgr_approve  Submitted -> Approved  by Manager when amount >  AUTO_LIMIT covers REQ-3 "Claims above AUTO_LIMIT are approved by a manager"
    transition reject       Submitted -> Rejected  by Manager when amount >  AUTO_LIMIT covers REQ-3 "Claims above AUTO_LIMIT may be rejected by a manager"
    transition pay          Approved  -> Paid      by Finance covers REQ-4 "Only approved claims are paid"
  }

  kpi paid_claims = count Claim in Paid

  acceptance AC-1 "Approval flow: a low-amount claim is auto-approved and paid" {
    submit(0, 1) auto_approve(0) pay(0)
    expect Claim 0 in Paid
  }
}

verify {
  instances Claim = 3
  values Amount = 0..3
}
```

The kernel-wrapper form remains valid for hard cases (multi-entity behavior,
conservation rules, SLA/time, or history not expressible as a carried field):
types/state/init, `requirement` blocks, `fair action`, `branches`, and explicit
`maps` still pass through as kernel syntax plus refinement metadata.

### 2.2 Expansion rules

1. `number X` plus `verify { values X = lo..hi }` → `type X = lo..hi`.
   `process E ...` plus `verify { instances E = N }` → `type E = 0..N-1`
   unless `entity E` already declared it explicitly.
2. `process E with f: T { stages ... initial ... transition ... }` →
   generate an `EStage` enum, an `e_stage: Map<E, EStage>` state map, carried
   field maps such as `amount: Map<E, Amount>`, deterministic init, and one fair
   action per transition. Transition `with a: T` becomes an action parameter,
   `when` becomes a requires clause, `set f = expr` becomes a field-map
   assignment for that entity, and `covers REQ-n "text"` attaches traceability.
3. `kpi k = count E in S` → no kernel state/action/invariant. The declaration is
   recorded as metadata for the projection
   `count(c: E where e_stage[c] == S)`.
4. `requirement <ID> "<text>" { items }` → lift the contained action /
   invariant / reachable / leadsTo to top level and attach
   `meta = {id: ID, text}` to each element (the Stage 1 mechanism). The ID is
   an identifier token of the form `REQ-1` (alphanumerics and hyphens).
5. `branches { when <cond> { stmts... } maps <abs-correspondence> ... }` →
   split the action per branch: `submit__b1`, `submit__b2` (the display names
   are of the form `submit[a <= AUTO_LIMIT]` — the display-name map reuses the
   compose mechanism). Diagnostics for a split branch keep the internal name and
   add `display_name` when the branch label is more useful to a human. Each split
   action = the original requires + the when condition + the branch body. The
   when conditions are **not checked for exhaustiveness or exclusivity** (left to
   the ordinary enabled semantics: if they overlap, both are enabled; if there is
   a gap, it is disabled — the coverage diagnostic detects it).
6. `maps <abs_action>(<args>) | stutter` (action modifier / inside branches) →
   compose it with the map group in the `implements` block to **internally
   generate a refinement AST**.
7. When an `implements ... { ... }` is present, at verify / check time **also
   run a refine check against the upper layer**, and add
   `"implements": {"abs": "ReturnPolicy", "result": "refines" | {...violation}}`
   to the result JSON (even if refine fails, the verify result itself is returned
   separately). An empty body auto-generates identity maps when process/action/
   stage names match. `maps auto` is allowed for same-name kernel-wrapper
   state/actions, explicit maps override it, and auto-mapped process transitions
   are statically actor-checked. The refine-violation JSON passes through the
   `requirement` of the impl action involved (extension of §1.2).
8. `acceptance <ID> "<text>" { <action call>...  expect <expr> }` and
   `expect <Entity> <id> in <Stage>` →
   (a) Schema: a fixed step sequence. At expansion time, **replay it with the
   concrete Monitor** and verify at check time that each step is ok and that
   expect is true at the end (failure is reported as a `kind: "acceptance"`
   error with the AC ID + the failing step).
   (b) Embed it in the scenarios output as a `kind: "acceptance"` scenario
   (steps / expected_states are built from the replay result) → it flows
   naturally into testgen.
9. The name of the expanded spec is the name of `requirements`. All other items
   (type/enum/struct/state/init/top-level actions, etc.) pass through unchanged.
10. `terminal { <expr> }` is one of these pass-through items (added as a
    `requirements_item` grammar alternative, #69) — it takes the generic
    `_expand_item` fallback (`return [item], []`), so no dialects.py case was
    added for it specifically. Only one `terminal` block is allowed per spec
    (the kernel's own rule, unchanged). If the spec uses `process E { ... }`,
    the predicate is written against the synthesized stage map
    (`e_stage[c]`, rule 2 above) — the natural-language `stage(c)` form is
    business-only (§3.2 rule 7).

### 2.3 Tests (tests/test_req_dialect.py)

Use the return requirements (almost the same as §2.1) as a fixture:
- check ok / verify verified (+ implements.refines) / induction proved
- the branch split appears in coverage with display names (`submit[a <= AUTO_LIMIT]`)
- in a variant seeded with a violation, the requirement {id, text} appears in the counterexample
- acceptance: the positive example appears in scenarios / a variant that makes
  expect false fails at check time with `kind: "acceptance"` and points to the AC ID
- a variant that breaks the implements mapping → implements.result is refinement_failed
- all existing tests unchanged

## Stage 3: the fsl-biz dialect

Add `business <Name> { ... }` at top level. The expander
`expand_business(ast) -> ast` (no references to other files).

### 3.1 Syntax

```fsl
business ReturnHandling {
  actor Customer, Manager                  // → enum Actor { Customer, Manager } (an informational annotation, omittable if not referenced)
  entity Return                            // → entity sort; verification size lives in verify

  process Return {                         // one process per entity type
    stages Requested, Approved, Rejected, Refunded   // → enum ReturnStage
    initial Requested
    transition approve  Requested -> Approved by Manager
    transition reject   Requested -> Rejected by Manager
    transition refund   Approved  -> Refunded by Manager
  }

  kpi refunded = count Return in Refunded  // → metadata projection:
                                           //   count(c: Return where stage(c) == Refunded)

  control CTRL-REFUND-REVIEW
    "refund payment must preserve review control"
    owner Manager
    severity high
    applies_to Return

  policy PAY-1 "refunds only for approved cases" invariant {
    // stage(c) is available in expressions (c is an entity-typed bound variable)
    forall c: Return { stage(c) == Refunded => true }   // example
  }
  policy PAY-2 "every request is eventually adjudicated"
    satisfies CTRL-REFUND-REVIEW
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can complete"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

### 3.2 Expansion rules

1. `entity X` plus `verify { instances X = N }` → `type X = 0..N-1`.
2. `process X { stages S1..Sn  initial Si  transition t A -> B by Actor }` →
   - `enum XStage { S1, ..., Sn }`
   - `state { x_stage: Map<X, XStage> }` (the variable name is the lowercased
     process name + `_stage`) + `init { forall c: X { x_stage[c] = Si } }`
   - per transition, `fair action <t>(c: X) "by <Actor>" {
       requires x_stage[c] == A   x_stage[c] = B }`
     (`by` goes into meta.text: `meta = {id: t, text: "by Manager"}`.
      Since it is not policy-derived, a separate field `"actor"` from
      requirement is also acceptable — for implementation simplicity it is fine
      to put "by Manager" into meta.text)
   - duplicate transition labels with the same name are a type error.
3. `kpi k = count X in S` → no kernel state/action/invariant. The declaration is
   recorded as metadata for the projection
   `count(c: X where x_stage[c] == S)`.
4. `control <ID> "<text>" [owner NAME] [severity NAME] [applies_to NAME]...`
   records business/governance metadata only. It does not generate kernel state
   or properties by itself. A control becomes checkable when a policy or goal
   declares `satisfies <ControlID>`.
5. `policy <ID> "<text>" [satisfies <ControlID>, ...] invariant { expr }` →
   invariant (with meta). The same optional `satisfies` clause is available on
   all policy forms and on goals. Unknown control references are type errors;
   declared but unused controls produce an `unused_control` warning. When a
   satisfied policy/goal fails, the JSON `requirement` object includes
   `controls: [{id, text}, ...]`.
   `policy ... responds { P ~> Q }` → leadsTo (with meta).
   `policy ... every <Entity> in <Stage> must eventually be <Stage> [or <Stage> ...]`
   is a readable alias for the common stage-response rule and expands to
   `forall c: Entity { stage(c) == Source ~> stage(c) == Target1 or ... }`.
   `goal <ID> "<text>" { expr }` → reachable (with meta).
   `goal ... some <Entity> can reach <Stage>` and
   `goal ... all <Entity> can be <Stage> [or <Stage> ...]` are readable aliases
   for the common existential/all-cases reachability checks.
   `stage(c)` in an expression is rewritten to `x_stage[c]` for the entity-typed
   bound variable c in question (the process is identified from the type of the
   binding; ambiguity is a type error).
6. The `actor` declaration is a roster (used to validate the `by` of a
   transition; an undeclared actor is a type error). No other semantics.
7. No `terminal` syntax exists in business (#69). Instead, after all
   processes are expanded, each process's **sink stages** — stages that are
   never a transition's source (`tr["src"]`) — are collected. If every
   process has >=1 sink, one kernel `terminal { }` item is generated: the
   conjunction (`and`), over processes, of
   `forall c: X { _any_stage(X, c, sinks) }` (reusing the same `_stage_is` /
   `_any_stage` helpers rule 5's `all <Entity> can be <Stage>` goal uses). If
   any process is cyclic (no sink at all), no terminal is generated for the
   whole spec and deadlock checking is unaffected — a cyclic process always
   has an enabled transition, so it can never contribute to a real deadlock
   anyway. This makes "stopping at a process's last stage" verify clean by
   default; `--deadlock ignore` is no longer required for a pure stage-graph
   business spec.

### 3.3 Governance catalog

`governance <Name> { ... }` is an optional top-level form for controls that sit
above one business process or are reused across multiple business specs:

```fsl
governance EnterpriseRefundControls {
  authority Finance owns CTRL-REFUND-REVIEW
  control CTRL-REFUND-REVIEW "refund payment must preserve review control"
    owner Finance
    severity high
    applies_to Return

  delegates ReturnHandling from "return_policy.fsl" {
    require CTRL-REFUND-REVIEW
    // optional when the business file already has `policy ... satisfies CTRL-REFUND-REVIEW`
    CTRL-REFUND-REVIEW is satisfied_by policy PAY-2
  }

  preservation AutoApproval {
    before AsIsExpense from "asis_expense.fsl"
    after  ToBeExpense from "tobe_expense.fsl"
    preserve CTRL-REFUND-REVIEW
    checked_by refinement "tobe_refines_asis.fsl"
  }
}
```

Expansion rules:

1. A governance catalog expands to a metadata-only kernel spec with generated
   no-op state/action/property. Kernel verification semantics are unchanged.
2. `delegates BusinessName from "file.fsl"` parses the referenced business spec,
   checks the name, and verifies that every `require CTRL` is satisfied either by
   business-side `policy/goal ... satisfies CTRL` metadata or by an explicit
   `CTRL is satisfied_by policy|goal ID` mapping. Missing references are type
   errors.
3. `preservation` validates the before/after/refinement file references. During
   `fslc check governance.fsl`, fslc runs `refine(after, before, mapping)` at
   depth 8 and reports the result under `governance.preservations`.

### 3.4 Tests (tests/test_governance_business.py)

Use the return process of §3.1 as a fixture: check ok / verify verified /
induction proved / a policy-violating variant carries the requirement
(= policy ID+text) and satisfied controls in the counterexample / KPI projection
metadata is recorded without generating a counter invariant / the goal is
witnessed as reachable / type errors for an undeclared actor, a decrementing KPI,
and an unknown satisfied control / governance delegates reject unsatisfied
controls / preservation runs the declared refinement / all existing tests unchanged.
Furthermore, `return_policy_biz.fsl`, which rewrites examples/layers'
return_policy.fsl in this dialect, can **be refined from the requirements layer**
by the existing return_refines.fsl (with adjusted abs names) (= demonstration
that the kernel spec after dialect expansion is equivalent to a hand-written one).

## Stage 4: three-layer dogfooding

The return domain across all three layers: fsl-biz (business) ← fsl-req
(requirements; implements) ← fsl (design; adding implementation details such as
persistence and retry, then refine) ← the Adapter implementation in examples.
Findings in DOGFOOD-4.md. Additions of the dialects to LANGUAGE.md and skills/fsl
are also done here.
