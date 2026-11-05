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
   A carried field's type `T` is one of three kinds, each with its own
   initialization rule (no invented default for Bool/enum):
   - `number` (declared via `number T` + `verify { values T = lo..hi }`):
     unchanged from the original profile — defaults to the domain's `lo`
     when `f: T` has no initializer; `f: T = <const-expr>` is an optional
     explicit initializer, const-evaluated at desugar time (out-of-bounds
     values are caught by the kernel's ordinary type-bound invariant, not by
     a separate desugar-time bounds check). Omission emits the stable
     `implicit_initial_value` migration warning with the selected lower bound
     and a machine-applicable insertion edit.
   - `Bool`: **requires** an explicit initializer, `f: Bool = true` or
     `f: Bool = false`; the field-map value type is the kernel `Bool`, not a
     0/1 domain. Missing the initializer is a check-time error.
   - an `enum` declared in the same requirements spec: **requires** an
     explicit initializer that is one of that enum's members, `f: T = Member`;
     the assignment reuses the same `("var", Member)` pattern the synthesized
     stage-init already uses. An initializer that names a non-member, or a
     missing initializer, is a check-time error.
   Any other type is rejected with "carried process field must be a number,
   Bool, or enum type". Carried fields are never auto-mapped for refinement —
   only the stage map (`e_stage`) participates in `maps auto`.
3. `kpi k = count E in S` → no kernel state/action/invariant. The declaration is
   validated and retained as typed `ProjectionDef` metadata whose expression is
   the shared `Aggregate::Count` plus a typed Binder for
   `count(c: E where e_stage[c] == S)` when c does not meet a const. This c is
   not a writable source local: on collision the projection alone uses the
   existing reserved fresh-name allocator, with its index renamed consistently.
   Native `explain` reads this projection's public metadata;
   no ghost counter or consistency invariant is generated.
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
   to the result JSON. A failing seam becomes the top-level `result`
   (`refinement_failed` or `impl_violated`) and exits 1; only `refines` leaves
   the command's ordinary success envelope. An empty body auto-generates identity maps when process/action/
   stage names match. `maps auto` is allowed for same-name kernel-wrapper
   state/actions, explicit maps override it, and auto-mapped process transitions
   are statically actor-checked. Explicit `implements` items, action/branch
   `maps`, and implicit identity/stutter mappings all lower to the shared typed
   action-correspondence IR; there is no dialect-specific action validator. The
   refine-violation JSON passes through the
   `requirement` of the impl action involved (extension of §1.2).
8. `acceptance <ID> "<text>" { <action call>...  expect <expr> }` and
   `expect <Entity> <id> in <Stage>` →
   (a) Schema: a fixed step sequence. At expansion time, **replay it with the
   concrete Monitor** and verify at check time that each step is ok and that
   expect is true at the end (failure is reported as a `kind: "acceptance"`
   error with the AC ID + the failing step).
   Step arguments are not enum ordinals (DESIGN-forbidden.md:120); wrong-kind acceptance arguments carry `wrong_kind_argument`, `message` (`argument <value> for '<param>' of '<action>' is not a value of type '<type>' in acceptance, so no guard was evaluated`), a hint, and the step's `loc`, with kind and exit status unchanged (#1346).
   (b) Embed it in the scenarios output as a `kind: "acceptance"` scenario
   (steps / expected_states are built from the replay result) → it flows
   naturally into testgen.
9. The name of the expanded spec is the name of `requirements`. All other items
   (type/enum/struct/state/init/top-level actions, etc.) pass through unchanged.
10. `terminal { <expr> }` is one of these pass-through items (added as a
    `requirements_item` grammar alternative (#69). Only one `terminal` block
    is allowed per spec (the kernel's own rule, unchanged). The shared stage
    resolver accepts `stage(c)` in this and every other requirements expression
    context, derives the process from `c`'s entity type, and rewrites it to
    `e_stage[c]`. Requirements never derive terminal states from process sinks.
11. A qualified process declaration (`process claims.Claim`) keeps its shared
    `SymbolPath`. Several process paths may have the same final entity segment;
    unqualified `stage(c)` then reports every candidate, while
    `claims.Claim.stage(c)` selects the exact path. Business and requirements
    use the same `Expr::Stage` node and resolver, and public Kernel provenance
    records both the lowered state symbol and source accessor span.

**Breaking removal (#1552):** for requirements `rule ... only when` conditions,
`check` now rejects previously accepted unsatisfiable guards that reuse an entity
field's name as an `is` pattern inside a quantifier body or aggregate value. The
guard copy now reads that name as the field, just as the event's lowering does,
not as the matched payload. For example,
`amount == 0 and (exists x: Amount where x == 1 { some(x) is some(amount) and amount == 1 })`
was accepted even though its event could never occur: the field must be both 0
and 1. #1552 rejected it with `rule_conflict_unsatisfiable_guard`; #1561 now
rejects the field-name pattern first, with exit 2, `kind: "semantics"`, and the
shared pattern-shadowing diagnostic asking for a rename. To test the payload instead, use a non-field
pattern name: replace `some(x) is some(amount) and amount == 1` with
`some(x) is some(v) and v == 1`. Keeping the outer condition and quantifier, this
means the field is 0 and there is an `x` equal to 1 whose matched payload `v`
equals 1; the rewritten example passes `check`. #1561 also removes the formerly
accepted `some(1) is some(amount) and amount == 1`: both lowering paths now refuse
this ambiguous source before losing its field spelling. Rename the pattern and
continuation to fresh `v`; the event is covered. Legacy guards, assignment RHS,
rule properties, measures, decision assumptions/rows, concept conditions (even
unused) and relation roles share that source admission rule.
Event Typed/Range inputs retain their bare-field exclusions without adding typed
solver metadata for Range. Ordinary quantifier/aggregate shadowing and patterns
local to definitions expanded after field substitution are unchanged. See
[the scope amendment](DESIGN-pattern-binding-scope.md#6-requirements-field-pattern-freshness-1561)
for diagnostic owners and measured breaking inventory.

**Breaking removal (#1569):** in legacy requirements with a process field
`amount`, `check` now rejects a transition guard such as
`exists c: Amount { stage(c) == Approved and amount == c }`. The non-entity
binder shadows the transition's implicit entity parameter `c`; a field named
`c` is not required. Previously, capture-avoiding substitution renamed the binder
without renaming the stage operand: `check` accepted the guard and `verify`
reported `verified`, but `stage(c)` silently read the transition's own instance.
Now `check`, `kernel`, and `verify` fail with exit 2, `kind: "semantics"`, and
`stage(c) refers to type 'Amount', which has no process`. The same invalid guard
with the binder and its uses consistently renamed to `d` already failed this way.
`stage(...)` requires an entity parameter or binder. To read the transition's own
stage, use `exists n: Amount { stage(c) == Approved and amount == n }`, keeping
`c` for the entity and a different name for the non-entity binder; this rewritten
guard passes `check`. No checked-in spec or example has the removed shape; the
597-form base `1844fa74` / head `7b9b34b3` sweep found no corpus acceptance
transitions.

**Breaking removal (#1600):** for next-edition (`fsl 5`) requirements, the
declared-name walk of #1256 (`rust/fsl-core/src/declared_names.rs`) now also
visits relation roles, resource names, and measure names, so `check` rejects
`Claim belongs to User as __owner via approve`, `resource __r: Amount = 0`, and
`measure __m = count Claim in Approved` with exit 2, `kind: "name"`, located at
the declared name. The message is the #1256 one with the position
`relation role name`, `resource name`, or `measure name`, for example
`relation role name '__owner' contains '__', which is reserved for the names FSL generates ...`.
Previously the walk missed these three, so they passed `check`. No spec or
example in the repository has such a name. Migration: rename the role,
resource, or measure without `__` and update its references.

**Breaking removal (#1602/#1603):** the same #1256 reservation also covers
goal and policy IDs in business and requirements documents, requirement IDs
(`rule` in the next edition), scenario IDs (`acceptance` in legacy), forbidden
IDs, decision-table IDs (`decide`), and glossary aliases. Both editions apply
the rule wherever the construct exists. Names containing `__` in these positions
previously passed `check`; they now fail with exit 2, `kind: "name"`, and a
structured `loc` at the written ID or alias, subject to the legacy locator
limitation below. The existing separator message uses the position `goal`,
`policy`, `requirement`, `scenario`, `forbidden`,
`decision`, or `glossary alias`. Rename the ID or alias without `__` and update
its references; generated-name references remain allowed.

Next-edition rule headers retain their IDs and source ID spans before statement
desugaring. Only those headers supply next-edition rule IDs to the declaration
check, not deadline requirement copies, guard annotations or property effects.
Lifecycle-implied rules and rules already covered by an event are still checked
at the exact written header ID. A preceding `covers` reference
is not the rule declaration and cannot supply its error location.

Explicitly written `requirement` blocks remain supported and their IDs are
checked in both editions, including `fsl 5` documents with or without deadlines.
Only a generated requirement copy is excluded: its ID must match a retained
rule header whose ID token span is contained in that requirement's span.

Known legacy-syntax location limitation (including explicit blocks in `fsl 5`):
split-spelled requirement, acceptance and
forbidden IDs such as `REQ - __1` are normalized before the fallback locator
searches for them. Rejection is unchanged, but `loc` may point to the declaration
keyword or a later repeated normalized spelling instead of the ID tokens. This
limitation is not fixed here; it follows the written syntax, not the edition.
Next-edition `rule` headers use exact token spans instead.

Written next-edition rule-property names containing `__` or naming a literal
(such as `none:`) were already rejected. Their diagnostic changes from
`kind: "semantics"` without `loc` to `kind: "name"` with structured `loc` at
the statement's start (`RuleProperty.span`), not the label. Exit 2,
`diagnostic_code: "invalid_rule_property_name"`, and the diagnostic wording are
unchanged. When checking a file, the new origin also changes the message suffix
from `at L:C` to `at <source-file>:L:C`, like other located name errors.

The transition's implicit c is a source-visible local under #1515: a same-named
const is rejected, even when the source never declares a local c. Rename the
const and its constant references; retain `stage(c)` for the entity instance.
Next-edition entities use their lowercased first letter for the visible instance
name, and relation events take an implicit instance of their target actor by
that naming rule. These names also reject matching consts. A generated init
reports the source process/entity declaration, not an unwritten binder token;
the relation-parameter refusal reports the source event. Hidden projection
binders are not covered by this visible-name restriction.

#### Repeated scenario step inputs (#1682)

**Breaking removal (#1682).** The desugaring of a `scenario` or `forbidden` step
collected its inputs into a map by name, so a repeated name kept its last value and
dropped the others. Binding happens there and nowhere else: the event call a step
becomes has one argument per input, so no later check could see the dropped value,
which was not even typed (`with amount true, amount 1` passed). The desugaring now
records each repeated name with its span in
`NextEditionRequirements::repeated_step_inputs`, and `lower_requirements` refuses the
first one before lowering.

Each form below ran with `FSLC_UNSTABLE=fsl5 FSLC_CACHE=off` from a copy of the fixture
directory named, on base `3901764f0fd07c3e98a5cc78b8ca341a6d1276dd` and on head `6dd96a59f9ab19a546d59703e34d742920f2bc88`.

| Fixture and step | Base `check` | Head `check`, exit 2, `kind: "semantics"` |
|---|---|---|
| `e2e/fsl5_en_decide.fsl`, `with amount 2, amount 1` | exit 0, `ok`, as `with amount 1` | `duplicate step input 'amount'`, `loc` 43:44 |
| the same, `with amount 1, amount 2` | exit 2, `acceptance` at 44:5, as `with amount 2` | the same |
| the same, `with amount 1, amount 1` or `with amount true, amount 1` | exit 0, `ok` | the same, 43:44 and 43:47 |
| the same, `forbidden` with `with amount 2, amount 1` | exit 0, `ok` | the same, at that step |
| `e2e/fsl5_ja_particles.fsl`, `申請金額 2, 申請金額 1 で` | exit 0, `ok` | `duplicate step input '申請金額（amount）'`, `loc` 75:25 |
| the same, `申請金額 2 で 申請金額 1 で` | exit 0, `ok` | the same, 75:26 |
| `relations/fsl5_en.fsl`, `with assignee 0, assignee 1`, either order | exit 0, `ok` | `duplicate step input 'assignee'`, `loc` 37:37 |

The refusal is a semantic error rather than a parse error because the document parses
and the step's grammar is not at fault. It comes after the parse-time step errors, which
keep their kind and text: an unknown verb, a step no event matches, an input the event
does not take, a missing input. So `with nope 2, nope 1` still reports the missing
`amount`, and `with amount 1, nope 2, nope 3` still reports
`event 'submit' has no input 'nope'` at the last `nope`.

On `with amount 2, amount 1`, base and head on these routes:

| Route | Base | Head |
|---|---|---|
| `verify` on bmc and explicit | exit 0, `verified` | exit 2 |
| `verify` on induction | exit 0, `proved` | exit 2 |
| `kernel` versions 1 and 2, `typestate` | exit 0 | exit 2; version 1 and `typestate` omit the file from the message, version 2 names it by a longer path |
| `explain`, `analyze`, `conformance`, `scenarios`, `testgen`, `mutate`, `html`, `ledger`, `diff`, `fmt` | exit 0 | exit 2 |
| `lint`, `fmt --check` | exit 1 | exit 2 |
| `sweep --depth 0..1` | exit 0, `sweep_passed` | exit 2 |
| `migrate` | exit 0, `migrated` | exit 2, no `loc` |
| `document claims`, `document generate` | exit 0 | exit 2, no `loc` and no file in the message |
| `refine` with the document as implementation or abstraction | exit 0, `refines` | exit 2, `loc` in the document |
| `replay --trace` | exit 2, `io`, the trace was malformed | exit 2, `semantics`: the document is loaded first |
| `check` of an inline `implements` of the document | exit 0 | exit 2, `kind: "type"`, `loc` on the `implements`, `implements "rep.fsl" failed to check (duplicate step input 'v' at rep.fsl:15:30)` |

`lint`, `fmt`, `fmt --check` and `migrate` report the same message with no `loc`. `document` omits
the file name as well. `kind: "type"` on the
inline `implements` is the kind of every failure of the other file on that path. A
`compose` cannot `use` a requirements document (`compose use must reference a spec`, base
and head), so no component route exists to refuse. The step form is read only in an
`fsl 5` `requirements` document. `fslc-lsp` and the Worker were not run.

Other surfaces that take named values, measured on base. Only the first changes here.

| Surface | Behavior on a repeat | Outcome |
|---|---|---|
| Scenario or forbidden step, with or without particles, instance first, relation role | last value kept | refused here |
| `name(args)` step call | positional, nothing to repeat; an extra argument fails at the step (`acceptance`) | no change |
| Two actors or two instances in a marked step | refused, `both ... take が (actor) in a scenario step` | no change |
| `set f = 1, f = 2` on an event | refused, `an action may not assign the same state location more than once` | no change |
| Event input `with a, a` | refused; #1680 owns its message and location | no change |
| `implements` map, same action twice or a repeated parameter | refused, `duplicate action correspondence`, `must match the impl action parameter names` | no change |
| Entity field or `const` declared twice | refused | no change |

Repeated struct literal fields are #1685. Repeated `verify` bounds and `concept` and `kpi`
declarations are #1686. Each is outside this change: a struct literal cannot reach a step,
because an event input of struct type is refused
(`action parameter type is not a finite scalar`), and a declaration has no formal receiving
an actual.

Migration: keep the one value you mean. There is no automatic migration.

Rust API: `NextEditionRequirements` gains `repeated_step_inputs`. Nothing outside the
parser constructs that struct.

On-disk transition set (empty). Base `1559e8eba90f73615d4efd00fcdccc30cf525b42`, head
`596881fc5e944a2548eedfe1b55d52fbea8ee344`, each binary built from its own tree in its own target
directory with Rust 1.98.0. Every `.fsl` under `specs/`, `examples/` and
`rust/fslc/tests/fixtures/` (646 forms: 23/195/428) ran with `FSLC_CACHE=off` and its own
tree as cwd, as `fslc check <path>` stable and with `FSLC_UNSTABLE=fsl5`, and as
`fslc kernel <path> --kernel-version 1|2`, twice per side. All 2584 pairs had equal complete
stdout, stderr and exit with no field excluded, and no run differed from its repeat: 0
ok-to-error and 0 error-to-ok transitions. No required test's inline fixtures were rewritten.

**Owed to #1437.** This removal is recorded only here and in
`changelog.d/1682-repeated-step-inputs.changed.md`: the next edition's header may not appear in
published material until #1434 lifts the gate (`rust/fslc/tests/unstable_gate_leak.rs`), so
`docs/manual/LANGUAGE.md`, `docs/manual/LANGUAGE.ja.md` and `skills/fsl/references/syntax.md` do not
carry it yet. When #1437 writes the `fsl 5` reference, those three owe this text, regenerating
`docs/intro/language.{en,ja}.html`: a `scenario` or `forbidden` step that names an input twice is
refused with exit 2, `kind: "semantics"`, `duplicate step input 'amount' at doc.fsl:43:44` and `loc` at
the repeated name, covering `with`, instance first, a `で` list, two `で` groups, an alias with its
canonical name, and a relation role. `lint`, `fmt` and `migrate` omit `loc`. `document` omits `loc` and
the file. `kernel` version 1 and `typestate` omit the file from the message, and `kernel` version 2 prints a longer path. An inline `implements`
reports `implements "rep.fsl" failed to check (duplicate step input 'v' at rep.fsl:15:30)`, `kind:
"type"`, with `loc` on the `implements`. A step with a parse error keeps that error. Different inputs
in any order, and the same input in a later step, stay accepted. Migration: keep the one value you
mean.

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
   - `with`, `when` and `set` on a transition are a located error (issue
     #1109). They carry data and this expansion has nowhere to put them, so
     accepting and dropping them would lower a model the author never wrote.
     The `requirements` dialect's `process` gives them meaning; `covers` is
     consumed here and stays accepted.
3. `kpi k = count X in S` follows the same typed `ProjectionDef`/
   `Aggregate::Count` path as requirements KPI declarations. Unknown entities
   and stages are rejected before model construction; native `explain` exposes
   the retained metadata. No kernel state/action/invariant is generated.
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
   Both checks are located (issue #1134): the unknown-reference error is
   positioned at the policy or goal that wrote the `satisfies`, and the
   `unused_control` warning at the `control` declaration nobody satisfies.
   The native lowering reports them; before #1134 it dropped
   `BusinessItem::Control` and every `satisfies` with it, so `check` answered
   `ok` for a control that did not exist. The `controls` projection on the
   `requirement` object is still emitted by the Python front end only; the
   native lowering does not carry it yet (declared-but-unimplemented census:
   #782).
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
8. `policy <ID> "<text>" every <Entity> reaching <Stage> [or <Stage> ...]
   must have passed through <Stage> [or <Stage> ...]` (#75, no-bypass
   precedence) synthesizes one more layer of invisible state on top of rule 2
   — a history flag alongside the stage map:
   - For each **distinct** `(process, waypoint-set)` pair across all
     precedence policies (deduped, so two policies over the same waypoints
     share one flag), one `history_var: Map<X, Bool>` is added to the same
     `state { }` block rule 2 emits, named
     `<x_stage>_via_<Waypoint1>[_<Waypoint2>...]` (waypoints ordered by
     declaration order in the process, for a deterministic name regardless of
     the order the policy lists them in — it appears in traces).
   - `init` sets it `true` for every entity if the process's `initial` stage
     is itself in the waypoint set, else `false` — folded into the same
     `init { }` block rule 2 emits.
   - Every transition (rule 2) whose destination stage is in the waypoint
     set gets `history_var[c] = true` appended to its body (before the
     `requires`/`set` transformation runs, so this is only a body-append, not
     a new transition).
   - The policy itself compiles to a kernel invariant (with meta, so the
     REQ-ID propagates like every other policy form):
     `forall c: Entity { _any_stage(Entity, c, targets) => history_var[c] }`.
   - Unknown stage / unknown entity referenced by the policy is a type error
     naming the policy's REQ-ID.
   - Semantics notes: if the initial stage is in `targets` but not in the
     waypoint set, the invariant is violated at init — reported as a genuine
     violation, not special-cased. A waypoint equal to a target is allowed
     (trivially satisfied on entry through it).
   - Limitation: the history flag is synthesized only in the business spec.
     A requirements-layer spec that refines a business spec carrying a
     precedence policy must either map that flag explicitly in its
     refinement mapping or restate the policy at its own layer — refinement
     does not currently propagate synthesized business-layer state.

#### Repeated declarations (#1686)

**Breaking removal (#1686).** Lowering reads these by name and keeps one copy, so a second copy was
dropped without a word. Each is now refused at the repeated declaration, with `kind: "semantics"`
and `loc` there, whether or not the copies are equal. Equal copies lose no value; they are refused
because `const`, `type`, `action` and #1682's equal step values are.

| Declaration | Copy kept on base | What the other copy changed | Refusal on head |
|---|---|---|---|
| `instances`, requirements and business | Last | In requirements, `check` moved between `ok` and an error. In business `check` stayed `ok`, and verdicts were not measured | `duplicate instances bound for 'E' at doc.fsl:13:3` |
| `values`, requirements | Last | `verify` moved between `verified` and `violated`, table below | `duplicate values bound for 'V' at doc.fsl:14:3` |
| `concept`, next edition only | First | `check` moved between `ok` and a decision-table error | `duplicate concept 'SmallClaim' at doc.fsl:24:3` |
| `kpi`, requirements and business | Last on `analyze`, measured in requirements | `analyze` reported only the last stage. `explain`, `document generate` and `html` listed both. `scenarios`, `testgen` and `ledger` were unchanged | `duplicate kpi 'done' at doc.fsl:9:3` |

No row loses nothing, so none stays. A `kpi` changes no verdict, but `analyze` dropped a copy that
differed. A `spec` already refused a repeated bound, `duplicate values bound for 'V' at 12:3` without
`loc`, and that stays as it was; the new wording follows it. Two `verify` blocks were already a parse
error. A document with another earlier error reports that one: a scenario that overflows the last
bound keeps `kind: "parse"` and names the bound it overflows.

Verdicts the removal takes, on one document with an invariant that holds only under `0..1`,
`FSLC_CACHE=off`, base `3901764f0fd07c3e98a5cc78b8ca341a6d1276dd`, head `fc6d86d274ec1de6af8eac9ddb00c9529fdb7de1`:

| Route | Base, `0..3` then `0..1` | Base, `0..1` then `0..3` | Head, either |
|---|---|---|---|
| `check` | exit 0, `ok` | exit 0, `ok` | exit 2 at 14:3 |
| `verify` on bmc | exit 0, `verified` | exit 1, `violated` | exit 2 |
| `verify` on explicit, induction | exit 0, `proved` | exit 1, `violated` | exit 2 |
| `fsl-bfs` | no violation | invariant violation | exit 2, the message on stderr |
| `scenarios`, `testgen`, `mutate`, `html`, `ledger` | exit 0 | exit 1 | exit 2 |
| `refine`, either side, against the `0..3` document | exit 2, a type error | exit 1, `impl_violated` | exit 2 |
| `diff` against the `0..3` document | exit 1, `semantic_diff` | exit 1, `semantic_diff` | exit 2 |
| `lint` | exit 1, `lint` | exit 1, `lint` | exit 2 |
| `fmt --check` | exit 1, `format_check` | exit 1, `format_check` | exit 2 |
| `replay --trace`, an empty trace | exit 0, `conformant` | exit 0, `conformant` | exit 2 |
| `kernel` 1 and 2, `typestate`, `fmt`, `explain`, `analyze`, `conformance`, `document generate`, `document claims` | exit 0 | exit 0 | exit 2 |

`verify --values V=0..3` returned `violated` on both base orders and is refused on head: an override
does not hide the repeat. `lint`, `fmt`, `document generate` and `document claims` omit `loc`.
`verify --values`, `kernel` version 1, `typestate` and `document` omit the file from the message, and
`kernel` version 2 prints a longer path. `compose use` of a requirements document was already
refused, `compose use must reference a spec`, on base and head. An inline `implements` reports
`implements "1_business.fsl" failed to check (duplicate kpi 'paid_claims' at 1_business.fsl:55:3)`, `kind: "type"`, with `loc`
on the `implements`. `fslc-lsp`, the Worker and `replay --from-log` were not run.

**Owed to #1437.** The `concept` row and the bounds and `kpi` of a next-edition document are recorded
only here and in `changelog.d/1686-repeated-declarations.changed.md`: the next edition's header may
not appear in published material until #1434 lifts the gate (`rust/fslc/tests/unstable_gate_leak.rs`).
When #1437 writes the `fsl 5` reference, `docs/manual/LANGUAGE.md`, `docs/manual/LANGUAGE.ja.md` and
`skills/fsl/references/syntax.md` owe the `concept` refusal, regenerating
`docs/intro/language.{en,ja}.html`.

Migration: keep the one declaration you mean. There is no automatic migration.

On-disk transition set (empty). Base `3901764f0fd07c3e98a5cc78b8ca341a6d1276dd`, head `fc6d86d274ec1de6af8eac9ddb00c9529fdb7de1`, each binary built from its own tree in
its own target directory with Rust 1.98.0. Every `.fsl` under `specs/`, `examples/` and
`rust/fslc/tests/fixtures/` (646 forms: 23/195/428) ran with `FSLC_CACHE=off` and its own tree as cwd,
as `fslc check <path>` stable and with the next edition enabled, and as `fslc kernel <path>
--kernel-version 1|2`, twice per side. All 2584 pairs had equal complete stdout, stderr and exit with
no field excluded, and no run differed from its repeat: 0 ok-to-error and 0 error-to-ok transitions.
No required test's inline fixtures were rewritten.

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

## Stage 4: three-layer validation

The return domain across all three layers: fsl-biz (business) ← fsl-req
(requirements; implements) ← fsl (design; adding implementation details such as
persistence and retry, then refine) ← the Adapter implementation in examples.
The executable chain is under `examples/layers/`. Additions of the dialects to
LANGUAGE.md and skills/fsl are also done here.
