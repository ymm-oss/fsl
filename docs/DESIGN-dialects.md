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
requirements ReturnSystemReq {
  implements ReturnPolicy from "return_policy.fsl" {   // optional
    map cases[c: CaseId] = if sys[c].st == New then Requested else ...
    map refunded = paid_count
    // action correspondences are collected automatically from the maps clauses of branches and of ordinary actions
  }

  // types, states, init are kernel syntax as-is (no implicit state is created)
  type CaseId = 0..2
  type Amount = 0..3
  const AUTO_LIMIT = 1
  enum SSt { New, AutoApproved, MgrQueue, MgrApproved, MgrRejected, Paid }
  struct RCase { st: SSt, amount: Amount }
  state { sys: Map<CaseId, RCase>, paid_count: Int }
  init { ... }

  requirement REQ-1 "returns at or below the threshold are auto-approved" {
    action submit(c: CaseId, a: Amount) {
      requires sys[c].st == New
      requires a > 0
      branches {
        when a <= AUTO_LIMIT {
          sys[c] = RCase { st: AutoApproved, amount: a }
        } maps approve(c)
        when a > AUTO_LIMIT {
          sys[c] = RCase { st: MgrQueue, amount: a }
        } maps stutter
      }
    }
  }

  requirement REQ-2 "payment only after approval" {
    fair action pay(c: CaseId) maps refund(c) {
      requires sys[c].st == AutoApproved or sys[c].st == MgrApproved
      sys[c].st = Paid
      paid_count = paid_count + 1
    }
    invariant PaidLedger { paid_count == count(c: CaseId where sys[c].st == Paid) }
  }

  acceptance AC-1 "small amounts are auto-approved and paid" {
    submit(0, 1)
    pay(0)
    expect sys[0].st == Paid
  }
}
```

### 2.2 Expansion rules

1. `requirement <ID> "<text>" { items }` → lift the contained action /
   invariant / reachable / leadsTo to top level and attach
   `meta = {id: ID, text}` to each element (the Stage 1 mechanism). The ID is
   an identifier token of the form `REQ-1` (alphanumerics and hyphens).
2. `branches { when <cond> { stmts... } maps <abs-correspondence> ... }` →
   split the action per branch: `submit__b1`, `submit__b2` (the display names
   are of the form `submit[a <= AUTO_LIMIT]` — the display-name map reuses the
   compose mechanism). Diagnostics for a split branch keep the internal name and
   add `display_name` when the branch label is more useful to a human. Each split
   action = the original requires + the when condition + the branch body. The
   when conditions are **not checked for exhaustiveness or exclusivity** (left to
   the ordinary enabled semantics: if they overlap, both are enabled; if there is
   a gap, it is disabled — the coverage diagnostic detects it).
3. `maps <abs_action>(<args>) | stutter` (action modifier / inside branches) →
   compose it with the map group in the `implements` block to **internally
   generate a refinement AST**.
4. When an `implements ... { map ... }` is present, at verify / check time
   **also run a refine check against the upper layer**, and add
   `"implements": {"abs": "ReturnPolicy", "result": "refines" | {...violation}}`
   to the result JSON (even if refine fails, the verify result itself is
   returned separately). The refine-violation JSON passes through the
   `requirement` of the impl action involved (extension of §1.2).
5. `acceptance <ID> "<text>" { <action call>...  expect <expr> }` →
   (a) Schema: a fixed step sequence. At expansion time, **replay it with the
   concrete Monitor** and verify at check time that each step is ok and that
   expect is true at the end (failure is reported as a `kind: "acceptance"`
   error with the AC ID + the failing step).
   (b) Embed it in the scenarios output as a `kind: "acceptance"` scenario
   (steps / expected_states are built from the replay result) → it flows
   naturally into testgen.
6. The name of the expanded spec is the name of `requirements`. All other items
   (type/enum/struct/state/init/top-level actions, etc.) pass through unchanged.

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

  kpi refunded counts Return in Refunded   // → state refunded: Int +
                                           //   +1 on the refund transition + a consistency invariant

  policy PAY-1 "refunds only for approved cases" invariant {
    // stage(c) is available in expressions (c is an entity-typed bound variable)
    forall c: Return { stage(c) == Refunded => true }   // example
  }
  policy PAY-2 "every request is eventually adjudicated"
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
       requires x_stage[c] == A   x_stage[c] = B  [kpi update] }`
     (`by` goes into meta.text: `meta = {id: t, text: "by Manager"}`.
      Since it is not policy-derived, a separate field `"actor"` from
      requirement is also acceptable — for implementation simplicity it is fine
      to put "by Manager" into meta.text)
   - duplicate transition labels with the same name are a type error.
3. `kpi k counts X in S` → `state { k: Int }` + init 0 +
   `k = k + 1` on every transition that **enters** S (if there is a transition
   leaving S it is a type error — it is stated explicitly that decrementing KPIs
   are unsupported in v3) +
   `invariant _kpi_k { k == count(c: X where x_stage(c) == S) }` (automatic).
4. `policy <ID> "<text>" invariant { expr }` → invariant (with meta).
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
5. The `actor` declaration is a roster (used to validate the `by` of a
   transition; an undeclared actor is a type error). No other semantics.

### 3.3 Tests (tests/test_biz_dialect.py)

Use the return process of §3.1 as a fixture: check ok / verify verified /
induction proved / a policy-violating variant carries the requirement
(= policy ID+text) in the counterexample / automatic generation of the kpi
consistency invariant / the goal is witnessed as reachable / type errors for
an undeclared actor and a decrementing KPI / all existing tests unchanged.
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
