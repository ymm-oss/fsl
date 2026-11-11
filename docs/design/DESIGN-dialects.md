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

**Breaking removal (#1256):** a name declared in a `spec`, `requirements`,
`business` or `compose` document may not contain `__`, in every edition; `check`
rejects it at the declaration with exit 2 and `kind: "name"` (a compose
component's declaration at the parent's `use`). `fslc` 4.12.0 accepted such a
declaration. Observed on 2026-11-07 with `FSLC_CACHE=off`: `spec S { state {
foo__bar: Int = 0 } ... }` checks `ok`/0 on the v4.12.0 binary and `error`/`name`/2
on a binary built from `c505d0a6c`; the same spec spelled `foo_bar` stays
`ok`/0. Migration: rename the declaration and its references, including replay
traces and stored outputs that name it.

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

#### One relation per event (#1808)

An event makes at most one relation. The event that makes a relation is done by
the related entity and takes that entity's instance as its first input, named by
the entity (`u` for `User`), so a second relation via the same event would need a
second input of the same name. The alternative the issue proposed, binding every
role the event makes to that one input so that the roles always hold the same
instance, is not adopted.

The second relation was already refused, but as a collision with an input the
document does not write. It now has its own code. The event's relations are
checked in the order written: the actor rule first, then this limit, then the
collision of the generated input with an authored one.

| Form | Base `f1e9ba71` | This change |
|---|---|---|
| `Order belongs to User as buyer via place` and `… as payer via place` (also `所属先 … 役名 … 契機 place`) | exit 2, `parse`, `generated_name_collision`, "event 'place' takes the 'User' that makes the relation 'payer' as its input 'u', which it already has; rename the input", at the event | exit 2, `parse`, `event_already_makes_relation`, "event 'place' already makes the relation 'buyer'; an event can make one relation, so give 'payer' its own event", at the second relation |
| one relation via `place`, `place` declaring `with u: Qty` | exit 2, `generated_name_collision`, "… rename the input", at the event | unchanged |
| `… as buyer via place` and `Order belongs to Box as stock via place` | exit 2, `parse`, the actor message (`the actor of 'place' is 'User'`), at the event's actor | unchanged |

No form changes from `ok` to `error`: `fslc check` of the 655 `.fsl` files under
`specs/`, `examples/` and `rust/fslc/tests/fixtures/`, with a binary built from
`f1e9ba71` in its tree and one built from this change in its tree (the change
edits no `.fsl` file), gives the same result, kind, diagnostic code and exit
code for every file.

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

**Documented (#1437).** The user-facing text of this removal is in `docs/manual/LANGUAGE.md` and
`docs/manual/LANGUAGE.ja.md`, "Repeated inputs, declarations and requirement IDs" (anchor
`fsl5-repeats`), and in `skills/fsl/references/syntax.md`, with `docs/intro/language.{en,ja}.html`
regenerated: the refusal, its `loc`, the forms it covers, the accepted orders, the `implements`
report and the migration (keep the one value you mean).

#### Repeated actors and requirement ID owners (#1705)

**Breaking removal (#1705).** In `fsl 5` requirements, three repeats passed `check`, and for an ID the
ledger kept one row, with one title and one status, for two declarations, or one text for two `covers`.
Each is now refused while the document is desugared, with exit 2 and `kind: "parse"`. The `loc` is the
repeated actor name, the second owner's ID, or the `covers` keyword of the later `covers`:

- A second `actor` of one name, on its own line or in a list: `duplicate actor 'Agent' at 4:9`.
- A second declaration that owns the requirement ID after its keyword (`rule`, `decide`, `scenario`,
  `forbidden`, `goal`, `policy`), at its ID, naming both kinds: `duplicate requirement id
  'REQ-DESK-001': this rule reuses the ID of the rule at 19:8; a requirement ID has one owner at 22:8`.
  This follows the policy that a declaration owns the ID after its keyword (`DESIGN-id-policy.md`), now
  enforced as one owner per ID. A second `scenario`, `forbidden`, `goal` or `policy` of its own kind
  keeps the refusal it already had (`duplicate acceptance id`, `duplicate forbidden id`, `duplicate
  property name`, `kind: "semantics"`). Only another kind's ID is new for them.
- Two `covers` of an ID that no declaration owns, with different texts, anywhere in the document, at
  the later `covers` and naming the earlier one: `requirement ID 'REQ-EV-1' is covered as
  "withdrawing" here and as "closing" at 15:5; every covers of an ID no declaration owns has one text
  at 19:5`. Events' and decision rows' `covers` count alike: two events, two rows of one table, rows of
  two tables, or an event and a row. A `covers` is a link, so one ID with one text stays accepted, and
  the ledger shows it once, as before. An unowned ID has no title of its own, so the `covers` texts are
  its only wording, and two of them left the ledger with one.

What base `56354e477544f9be1f4477ddf036e5855d514bb5` did, measured once each with a debug build on the support-desk and claims documents of
#1705. The routes were `check`, `verify --depth 6`, `ledger --depth 6`, `document claims` and `document generate`:

| Repeat | Base | What was lost |
|---|---|---|
| `actor Agent` twice, or `actor Agent, Agent` | exit 0 on all five | Nothing: the output equals one copy's. Refused because every other declaration of one name is |
| `rule` of one ID twice: `allow`, `always`, `never`, or two guards on one event | exit 0 | One ledger row, with one of the two titles |
| `rule` reusing a `policy`, `forbidden` or `scenario` ID | exit 0 | One ledger row, with the rule's title; the other title is gone |
| `rule` reusing a `decide` ID | exit 0 | The rule's title is in neither the ledger, `document claims` nor `document generate` |
| `decide` of one ID twice, different rows | exit 0 | One ledger row |
| `forbidden` reusing a `scenario` ID | exit 0 | No ledger row for either, both in the documents. Refused by the one-owner rule |
| two rows of one `decide` covering one ID, different texts | exit 0 | One ledger row; the second title is missing |
| two events covering `REQ-EV-1` with "closing" and "withdrawing" | exit 0 on `check` and `ledger` | One ledger row, "closing" |
| two `decide` tables covering `REQ-2` with different texts | exit 0 on `check` | One ledger row, "other"; the first table's text is missing |
| an event and a row covering `REQ-3` on one event, different texts | exit 2, `kind: "semantics"`, `requirement annotation 'REQ-3' has conflicting text at 23:7` | Refused already; now the message above, `kind: "parse"`, at the same location |

Unchanged:

- Two `covers` of one ID with one text.
- A repeated `lifecycle` line, which merges the same way a single line does.
- `policy`, `forbidden`, `measure`, a relation role, an entity field, a decide row that overlaps another, and a
  glossary alias, all already refused.
- `concept` and `kpi`, which #1686 refuses.
- An event's `covers`, or a decide row's `covers`, naming an ID another declaration owns, with any text.
  Its output changed; see the next subsection.

The current edition shares IDs the same way and is unchanged. One run each: two `requirement REQ-1`
blocks checked with one ledger row carrying the last title; an `acceptance` reusing a requirement's
ID, and a `requirement` reusing an acceptance's, checked.

Routes covered by `issue_1705_requirements_repeats`: `check`, `verify` on bmc, explicit and induction,
`ledger`, `document generate`, `document claims`, `document check` and `kernel`. `ledger` and `document
generate` write no file. The other routes load the document through the same desugaring [R]. The
Japanese particle spellings and the LSP were not measured.

On-disk transition set (empty). Base `56354e477544f9be1f4477ddf036e5855d514bb5`, head `2286fc51feaf3b49dac235c15e3367eb8346b919`, each binary built from
its own tree with Rust 1.98.0. Every `.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/`
(651 forms: 23/195/433) ran with `FSLC_CACHE=off` and its own tree as cwd, as `fslc check <path>` stable and with
`FSLC_UNSTABLE=fsl5`, and as `fslc kernel <path> --kernel-version 1|2`, twice per side. All 2604 pairs had
equal complete stdout, stderr and exit with no field excluded, and no run differed from its repeat: 0
ok-to-error and 0 error-to-ok. The same sweep on the earlier heads `779408fa52110cb3ca7b2adf7394d489513c5cb7` and
`c2bd60208cb3102e63b2b3c5ded6ee7fb58a7800` gave the same result. No required test's inline fixtures were rewritten.

#### A `covers` of an owned ID shows the owner's title (#1705)

**Changed ledger and document output (#1705).** A decide row's `covers` or an event's `covers` may name
an ID that a declaration owns (`rule`, `decide`, `scenario`, `forbidden`, `goal`, `policy`, or a
current-edition `requirement` or `acceptance` block), with any text, and two of them may differ. It
stays accepted and stays a link: the covering event's action keeps the ID with the `covers` text in the
kernel, as on base, and the claims that carry the ID are the same. The ID's title is its owner's. The
desugaring records each owner (`NextEditionRequirements::owners`), and the rendering titles the ID with
it: the ledger row takes the owner's title, and `document claims` puts the owner's title, cited at the
owner's keyword, first among the ID's statements, so the document's rule section quotes it and its
requirement text lists it first. The `covers` texts follow as the ID's linked statements, each cited at
its `covers`; they no longer title the ledger row. A first version of this change gave the `covers` the
owner's title in the lowering; it changed the kernel of a migrated process form whose rule took one of two
`covers` texts, so `migrate` refused the flagship example, and it was replaced by the rendering.

Measured once each, base `56354e477544f9be1f4477ddf036e5855d514bb5` against head `2286fc51feaf3b49dac235c15e3367eb8346b919`, on the
claims document of #1705 with `rule REQ-2 "Only a manager rejects a large claim"` added after the table.
`check` exits 0 on both; `ledger` exits 1 on both, because the document violates its own `assuming`;
`document generate`, `document claims` and `document check` exit 0 on both.

| Link to the rule's `REQ-2` | Base | Head |
|---|---|---|
| decide row `covers REQ-2 "A small claim is auto-approved"` | Ledger row titled with the row's text. Claims: the row's text, then the rule's, so the document's rule section quoted the row's text under the rule | Ledger row and rule section show the rule's title. Claims: the rule's title at 24:3, then the row's text at 20:7. Kernel and claim IDs unchanged |
| event `covers REQ-2 "A claim can be rejected"` | Ledger row titled with the event's text. The rule's title was in neither the ledger, `document claims` nor `document generate` | Ledger row and rule section show the rule's title. Claims: the rule's title at 25:3, then the event's text at 18:5. Kernel and claim IDs unchanged |

An event's `covers` of another owner kind's ID, on the support-desk document with the text "different",
measured once each. Base showed "different" as the ledger title; head shows the owner's, and "different"
appears nowhere in the head ledger:

| Owner | Base ledger title | Head ledger title |
|---|---|---|
| `forbidden FB-DESK-001` | different | No ticket is taken once the desk has closed |
| `policy POL-TICKET-001` | different | Every assigned ticket is eventually closed or escalated |
| `goal GOAL-TICKET-001` | different | A ticket can be closed |
| `scenario AC-TICKET-001` | different | An agent takes and closes a ticket |

One case stays refused: a row covering its own table's ID with another text, `requirement annotation
'REQ-2-3' has conflicting text at 22:7` with `kind: "semantics"`, as on base. The row's `covers` lands on
the event the table already annotates with that ID and the table's title, and one action carries one text
per ID. The row contradicts its own table on the one event both describe, so there is no link to keep
apart from the owner. A row covering its own table's ID with the table's title stays accepted.

Tested by `a_covers_of_an_owned_id_shows_the_owners_title`, `a_covers_of_another_owner_kind_shows_its_title`
and `a_row_covering_its_own_table_with_another_text_stays_refused` in `issue_1705_requirements_repeats`.
Faults detected: the ledger ignoring the owner, and the documents ignoring it. Not measured: the Japanese
particle spellings and the LSP.

#### Current-edition blocks own their IDs (#1705)

**Breaking removal (#1705).** A next-edition requirements document still takes the current edition's
`requirement` and `acceptance` blocks, and `DESIGN-id-policy.md` makes each the owner of the ID after its
keyword. The one-owner check now counts them, at the ID, naming both kinds. Measured once each, base
against head:

| Form, next edition | Base | Head |
|---|---|---|
| `requirement REQ-1` twice | exit 0 | `duplicate requirement id 'REQ-1': this requirement reuses the ID of the requirement at 6:15; a requirement ID has one owner at 16:15` |
| `acceptance REQ-1` after `requirement REQ-1` | exit 0 | the same, "this acceptance reuses the ID of the requirement" at 16:14 |
| `requirement AC-1` after `acceptance AC-1` | exit 0 | "this requirement reuses the ID of the acceptance" at 16:15 |
| `requirement REQ-DESK-001` after `rule REQ-DESK-001` | exit 0 | "this requirement reuses the ID of the rule at 19:8" at 54:15 |
| `acceptance AC-TICKET-001` after `scenario AC-TICKET-001` | exit 2, `duplicate acceptance id`, `kind: "semantics"` | "this acceptance reuses the ID of the scenario at 33:12" at 54:14, `kind: "parse"` |
| `acceptance AC-1` twice | exit 2, `duplicate acceptance id 'AC-1' at 16:3`, `kind: "semantics"` | unchanged |

A block's ID is owned, so two `covers` of it with different texts stay accepted, and the ledger titles it
with the block's title. Tested by `a_current_edition_block_is_an_owner`.

`fslc migrate --edition next` used to add the header to a current-edition document with two
`requirement`s of one ID, or an `acceptance` and a `requirement` sharing one, and write a document the
next edition now refuses. It now refuses before rewriting, with exit 1, `result: "migrate_manual_required"`,
nothing written, and a `migrate_manual_required` finding with `reason: "shared_requirement_id"` at the
second owner's ID in the source: `this requirement reuses the ID 'REQ-1' of the requirement at 5:15; the
next edition gives a requirement ID one owner`. It never renames an ID (`DESIGN-id-policy.md`). Two
`acceptance`s of one ID are refused by migrate's existing pre-flight `check`. Tested by
`migrate_refuses_two_requirements_of_one_id`; a fault that skips the search was detected. A process's
`covers` that names a block's ID and becomes a `rule` was not searched for by this refusal; the corpus
pass below found no such document [R].

`fslc migrate --edition 5` over the 109 current-edition `requirements` files under `specs/`, `examples/`
and `rust/fslc/tests/fixtures/`, without `--write`, each in its own tree with the next edition enabled,
twice per side: base `56354e477544f9be1f4477ddf036e5855d514bb5` and head `2286fc51feaf3b49dac235c15e3367eb8346b919` gave equal complete
stdout, stderr and exit for all 109, with no run differing from its repeat (84 `migrated`, 11
`migrate_manual_required`, 6 `migration_refused` and 8 errors on both). `edition_migrator` passes 41 of
41 on head.

The search for a shared ID runs before the process conversion and returns early, so a document that
also has a process migrate cannot rewrite reports only the `shared_requirement_id` findings in that run;
its `migrate_manual_required` findings for the process appear once the IDs are fixed.

A rewrite migrate does make can retitle a ledger row. A process `covers` naming a `requirement` block's ID
with another text migrates with exit 0, and the ID is the block's. On a copy of
`examples/annotations/annotated_claims.fsl` whose `pay` transition writes `covers REQ-READY "different"`,
measured once on head: the current-edition ledger row is `| REQ-READY | different |`, the first linked
text; after `migrate --edition next --write` the next-edition row is `| REQ-READY | the ledger tracks
readiness for external callers |`, the block's title. That is the intended next-edition title. A
digest-bound ledger approval of the current-edition document does not survive the migration either
way: `approval check` reports `drifted` with `spec_changed` and `rendering_changed` both on that copy and
on a control whose `covers` already has the block's title, so the retitling adds no drift a migration
did not already cause.

The current edition is unchanged. Base `9c31458678cf6c70e77987c85c95f4bc0d8df503` against head
`878e03fcd2be79062270c27bbdea88b16d8cb109`, each binary from its own tree, every one of the 631
current-edition `.fsl` files under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` ran through
`ledger --depth 3 -o`, `document generate -o` and `document claims`, without the next edition enabled,
twice per side: complete stdout, stderr, exit and written file were equal for all 1893 pairs, none
unstable (322, 98 and 340 of them exit 0 on base). With the next edition enabled, `kernel
--kernel-version 1` and `2` on the 8 next-edition corpus files that write `covers` were equal on base and
head, so each `covers` keeps its text in the kernel.

**Documented (#1437).** The repeated actor, the one-owner rule including the current-edition
blocks, the `covers` link with its owner's title, the one-text rule for an unowned ID, the row
covering its own table, and `migrate`'s `shared_requirement_id` refusal are in
`docs/manual/LANGUAGE.md` and `docs/manual/LANGUAGE.ja.md` ("Repeated inputs, declarations and
requirement IDs", anchor `fsl5-repeats`) and in `skills/fsl/references/syntax.md`, with the site
regenerated; `skills/fsl-requirements/references/syntax.md` and
`changelog.d/1705-requirements-repeats.changed.md` keep their records. Migration: rename one of the
two IDs, or merge the two declarations; delete the repeated actor.

#### Field and input names in every expression form (#1798)

In `fsl 5` requirements, three rewrites spell names before lowering: concept conditions in rule
guards (`field_names`), decision-table rows and `assuming` (`Scope::fields`), and event `only when`
conditions and `set` expressions (`resolve_event_names`). Each recursed through some `Expr` forms
and passed the rest through unchanged (`other => other`): `abs`/`min`/`max`, method calls, `is`,
quantifiers, `count`/`sum`, `stage`, struct literals and `reachable`. A field name there stayed bare,
and the lowering binds a bare name an event input shares to the input. So `concept Eligible = Claim
where abs(amount) > 0` with `only for Eligible` read the input `amount`, not the stored field, and
`check`, BMC and induction accepted a scenario the concept excludes. All three rewrites now go
through one resolver, `rust/fsl-syntax/src/parser/next_requirements/names.rs`, which names every
`Expr` form with no catch-all and tracks quantifier and aggregate binders: a binder hides a name in
its `where` filter and body or value, not in its domain. An `is some(v)` pattern does not hide a
field (#1552), and a pattern named like a field or an input is refused by the lowering (#1561).

**Breaking removal (#1798).** Measured with `check` on base `995146f45` and head `cb883819d`
(documents in the scratch tests of `rust/fslc/tests/fsl5_rules.rs`, `issue_1798_*`):

| Form | Base | Head, exit 2 |
|---|---|---|
| `only when abs(amount) > 0`, `amount` both an input and a field | exit 0, `ok`, reading the input | `kind: "parse"`, `ambiguous_input_field`, at the rule statement |
| `set limit = abs(amount)`, the same `amount` | exit 0, `ok`, reading the input | `kind: "parse"`, `ambiguous_input_field`, at the set field |
| `concept C = Claim where exists x in amount..amount { x == 1 }`, `amount` also an input | exit 0, `ok`, the bound read the input | `kind: "semantics"`, `binder bound is not a static integer`, at the event; a field is not a static bound |
| `only when exists amount in 0..0 { input amount == amount }`, also `count(amount in 0..0 where …)` and `set limit = sum(amount in 0..0 of input amount)` | exit 2, `semantics`, `public Kernel cannot type identifier 'input amount'` | `kind: "parse"`, new code `captured_by_binder`, at the rule statement or set field |
| `decide … assuming exists cap in 5..5 { Big }`, `Big` reading the resource `cap` | exit 2, `semantics`, `cannot type identifier 'Big'` | `kind: "parse"`, `captured_by_binder` (`concept 'Big' reads 'cap', which the binder 'cap' around it hides`), at the table |

The resolver passes each reference the binders in scope, so an `input name` inside a binder named
`name` is refused rather than spelled as the bare `name` the binder would capture, and a concept
expanded inside a binder that binds a name its predicate reads is refused the same way.
`captured_by_binder` covers both. Under another binder name `input amount` reads the input.

Newly accepted, base `semantics` error to head `ok`: `assuming exists x in 0..0 { Small or Big }`
(a concept named inside a quantifier is now expanded), `only when abs(input amount) > 0`, and
`set limit = abs(Claim.amount)`. Unchanged at `ok`: a binder named like a concept
(`assuming exists Small in 0..0 { Small == 0 }`), which base also read as the binder, since it left
names inside a quantifier unrewritten. A concept condition that read a same-named input through one
of the skipped forms now reads the field, which changes `check`/`verify` verdicts (the issue's
reproducer goes from exit 0 to `acceptance`, exit 2); that is the `fixed` part.

Corpus: each SHA was materialized with `git archive <sha> | tar -x` into its own tree, and
`FSLC_CACHE=off <binary> check <form>` ran in place there, once, with a debug `fslc` built from
that SHA, over every `.fsl` under `specs/` (23), `examples/` (197) and `rust/fslc/tests/fixtures/`
(435). All 655 forms kept their result, kind, diagnostic code and exit code. Only 22 of them are
`fsl 5` documents, so the corpus says little about this surface; the forms above and the
`issue_1798_*` tests carry the evidence. No existing required test was rewritten or removed.
Documented in `docs/manual/LANGUAGE.md` and `docs/manual/LANGUAGE.ja.md`, `skills/fsl-verify/SKILL.md`,
`skills/fsl-requirements/references/syntax.md` (pointed to from `skills/fsl/references/syntax.md`),
and `changelog.d/1798-*.md`.

#### Instance names in role values and checks (#1812)

A scenario names its instances (`o1`, `申請1`), but a relation role's value took the related
entity's raw number (`with buyer 0`) and a `then` expression only the lowered state
(`order_qty[0]`). Both now take the trace's names; the number forms keep working. The rewrite is
part of the step desugaring (`TraceNames` in `rust/fsl-syntax/src/parser/next_requirements.rs`),
so every later stage sees exactly the number form it saw before: the lowering, `check`'s replay,
the Monitor, BMC and BFS are unchanged, and `scenarios`, `testgen`, `explain`, `kernel` and
`document` print the same output for the name form and the number form.

- **Numbering.** One counter per entity, per trace. A name is numbered on its first use, as a
  step's instance or as a role value, from 0; within a step the instance comes before its role
  value (an event makes one relation, #1808, so a step has at most one role value). A name used
  again is the same instance; a name of another entity's instance as a role value is refused
  (`'t1' is an instance of 'Ticket', but the relation 'assignee' of event 'take' takes a
  'Agent'`). The `verify` bound applies to role-introduced names (`scenario_instance_overflow`).
  A number written as a role value does not take a slot: after `with assignee 0`, the first
  Agent name is Agent 0 too. Refusing that mix would need the values of constants as well; it
  is documented instead.
  An instance of an entity with no lifecycle (`entity User`) has no stage: `then u1 is S` and
  `stage(u1)` are refused (`… which has no lifecycle`).
- **Which bare name is an instance.** A role value is any expression; only a single bare name
  is read as an instance name, and only when the document does not declare it as a `const`, a
  `resource`, an `enum` member or a lifecycle stage. Measured on base `0de2dfc8`: a constant
  (`with buyer ALICE`) and an Int resource (`with buyer stock`) are accepted today and keep
  their value; an enum member or stage resolves but fails typing (`acceptance`, "is not a value
  of type 'User'") and keeps doing so. Every other bare name is now an instance name. On base
  it was `public Kernel cannot type identifier`, or, for a name spelled like a lowered
  per-entity state (`with buyer order_qty`), the same `acceptance` error, since such a state is a
  map from instance numbers (`argument {"0":0,"1":0} … is not a value of type 'User'`). The only
  scalar states a requirements document lowers to are its resources (in the 19 corpus
  documents that lower, `stock` and `desk_open`), so no form that checked changes its meaning.
  Lowered names are not excluded: core adds some itself (`*_stage_via_*` history), and the
  parser cannot list them.
- **Checks.** In `then <expr>`, `o1.field` becomes `entity_field[n]` and `stage(o1)` /
  `Order.stage(o1)` becomes `entity_stage[n]`, with `n` the instance's number, through the same
  binder-aware resolver as #1798 (which now also hands `stage(name)` to its callback). A
  quantifier or aggregate binder of the same name keeps the name, as do the declared names
  above. So does an `is some(o1)` pattern anywhere in the check (`names::pattern_names`): the
  resolver does not treat patterns as binders (#1552), and the pattern's scope (its logical
  continuation, `DESIGN-pattern-binding-scope.md`) is the lowering's to decide, so the whole
  check keeps the name. Inside the scope that is the pattern variable, as on base; outside it
  base already failed (`cannot type identifier`, or `stage(o1) cannot be resolved`), and still
  does. A bare name is not rewritten: the kernel's identifiers there include lowered states,
  history fields and ages that the parser does not enumerate, so reading a bare instance name
  could silently change the meaning of a check that reads one of them. The state names come
  from `fsl_syntax::entity_stage_state` / `entity_field_state`, to which
  `fsl_core::naming::process_stage_state` / `process_field_state` delegate.

Measured with `check` on base `0de2dfc8` and this change, on a probe document (`entity User`,
`Order belongs to User as buyer via place`, `instances User = 2`):

| Form | Base | This change |
|---|---|---|
| `User places o1 with buyer u1, qty 1` | exit 2, `semantics`, `cannot type identifier 'u1'` | exit 0, as `with buyer 0` |
| `then o1.qty == 1` / `== 2` | exit 2, `semantics`, `cannot type identifier 'o1'` | exit 0 / exit 2, `acceptance` |
| `then stage(o1) == Placed`, `then Order.stage(o1) == Placed` | exit 2, `semantics`, `stage(o1) cannot be resolved` | exit 0 |
| `with buyer ALICE` (`const ALICE = 1`), `with buyer stock` (`resource stock: Qty = 1`) | exit 0, the value 1 | unchanged |
| `with buyer Web` (enum member), `with buyer Placed` (stage) | exit 2, `acceptance`, not a value of type 'User' | unchanged |
| `then exists o1: Order { o1.qty == 1 }` | exit 2, `semantics`, field access requires struct | unchanged |
| `then some(Note { qty: 1 }) is some(o1) and o1.qty == 1` / `== 2`, in a document without the relation and with `struct Note { qty: Qty }`, after `User places o1 with qty 2` | exit 0 / exit 2, `acceptance`: the pattern's `o1` | unchanged |
| `then o1.buyer == some(u1)` (a bare name) | exit 2, `semantics`, `cannot type identifier 'u1'` | unchanged |
| Japanese: `利用者 が 注文1 を 購入者 利用者1 で 注文数 1 で 発注する`, `結果 注文1.注文数 == 1 かつ stage(注文1) == 発注済み` | exit 2, `semantics` | exit 0; `fmt --lang en --names canonical` keeps the names and lowers to the same kernel |

Corpus: `fslc check` of every `.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/`
(665 forms) with a debug binary built from `0de2dfc8` and one built from this change, both run in
this change's tree (it edits no `.fsl` file): complete stdout and exit code are byte-equal for
all 665. The executable evidence is in `rust/fslc/tests/fsl5_relations.rs`
(`a_role_value_is_an_instance_name_of_the_related_entity`,
`a_declared_name_as_a_role_value_keeps_its_meaning`,
`a_scenario_check_reads_an_instance_by_its_name`,
`japanese_instance_names_in_role_values_and_checks`). Documented in `docs/manual/LANGUAGE.md` and
`docs/manual/LANGUAGE.ja.md` ("Relations, resources, and projections", "Scenarios and
stopping"), `skills/fsl-requirements/references/syntax.md` and `keywords.md`,
`skills/fsl/references/syntax.md`, and `changelog.d/1812-*.md`.

#### Parenthesized event move (#1803)

An `fsl 5` event may write its move in parentheses after the phrase and its inputs:
`event reject = Manager rejects Claim (Submitted -> Rejected)`,
`event submit = Employee submits Claim with amount (Draft -> Submitted)`,
`出来事 reject = 上長 が 申請 を 却下する (提出済み -> 却下済み)`. Written bare, the phrase and the
move run together (`rejects Claim Submitted`, `却下する 提出済み`); the parentheses end the
sentence at its verb. The bare form stays accepted, and both lower to the same kernel. This is an
additive change: no form that checked before changes meaning.

The event parser (`event_declaration` in `rust/fsl-syntax/src/parser/next_requirements.rs`) reads
`( S -> T )` where it read `S -> T`. Inside the parentheses it refuses `in S` / `段階 S` and a lone
`S` with one message that names both forms, located at `(`. A missing `)` is `expected ')'`. A
second `(` is `expected identifier`. The stay stays `in S` / `段階 S`; the issue's `(S)` for a stay
is not adopted. Full-width `（ ）` was already refused when lexing (`fullwidth_symbol`, "write
'('"), and that refusal stays.

Why `(` there cannot mean anything else:

- The lexer reads `(` as a one-character symbol, never as part of an identifier.
- Both phrase loops, plain and with particles, take only identifiers, so they stop at `(`. The
  particle detector `phrase_has_particles` scans the line up to `->`, so it decides the same way
  for both forms.
- Before this change, the token after the phrase and its inputs had to be `with`, `in` / `段階`,
  or the first stage of `S -> T`. A `(` there was `expected identifier`, so no document that parsed
  before has one there.
- An input's type is `qualified_name`, `Ident [. Ident]`. It has no type arguments and no call, so
  `with amount: Amount (Draft -> Submitted)` reads the type `Amount` and then the move.
- `set` and `covers` come after the move and are unchanged.

`fmt` keeps the form written and does not canonicalize. The formatter prints a `(` after an
identifier with no space, as in a call, so the parser records the `(` of each parenthesized move
(`SourceSpellings::parenthesized_moves`). The formatter writes one space before those, so
`Claim(Submitted -> Rejected)` prints as `Claim (Submitted -> Rejected)`. Without that record,
`fmt` printed `Claim(Submitted -> Rejected)`; it parses the same, but it is not the documented
layout. `fmt --lang` and `fmt --names` respell words where they stand; the move keeps its
parentheses. `fslc migrate --edition 5`
still writes the bare form. The LSP index reads stages between the parentheses as uses of the
lifecycle's stages, as it does for a bare move.

Evidence: `rust/fslc/tests/issue_1803_parenthesized_move.rs` and
`the_stages_of_a_parenthesized_move_are_uses_of_the_lifecycle_stages` in
`rust/fsl-lsp/tests/fsl5.rs`. Seven `fsl 5` fixtures were tested, English, Japanese with
particles, all-Japanese, relations, stays and multi-entity. With every move in parentheses, and
with every other one, each gives the same `check` packet and the same `fslc kernel` JSON as the
fixture as written, compared in full. `fmt` prints both forms, and a mix of them, unchanged.
`--lang ja|en` commutes with adding the parentheses. Each refusal above is tested in English and in
Japanese. All of these fail on base `9bcff3eb`, which refuses every parenthesized move with
`expected identifier` at `(`.

Corpus: base `9bcff3eb` and this change were each materialized in their own tree with
`git archive <sha> | tar -x`. `FSLC_CACHE=off <binary> check|kernel|fmt <form>` ran there with a
debug `fslc` built from that SHA, over every `.fsl` under `specs/`, `examples/` and
`rust/fslc/tests/fixtures/` (665 files, 31 of them `fsl 5`). Stdout and exit code are
byte-identical for every file and command. The change edits no `.fsl` file. Of the existing
tests, it changes only the event heading that `rust/fslc/tests/fsl5_keywords.rs` renders into
`skills/fsl-requirements/references/keywords.md`.
Documented in `docs/manual/LANGUAGE.md` and `docs/manual/LANGUAGE.ja.md`, the regenerated
`docs/intro/language.{en,ja}.html`, `skills/fsl-requirements/` (`SKILL.md`,
`references/syntax.md`, `references/glossary.md`, `references/keywords.md`),
`skills/fsl-design/SKILL.md`, and `changelog.d/1803-parenthesized-event-move.added.md`.

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
| `values`, requirements and business | Last in requirements. `lower_business` does not read `values` | In requirements, `verify` moved between `verified` and `violated`, table below. In business nothing changed, and `check` went from exit 0 to exit 2 on `values Nope` twice | `duplicate values bound for 'V' at doc.fsl:14:3` |
| `concept`, next edition only | First | `check` moved between `ok` and a decision-table error | `duplicate concept 'SmallClaim' at doc.fsl:24:3` |
| `kpi`, requirements and business | Last on `analyze`, measured in requirements | `analyze` reported only the last stage. `explain`, `document generate` and `html` listed both. `scenarios`, `testgen` and `ledger` were unchanged | `duplicate kpi 'done' at doc.fsl:9:3` |

No row loses nothing, so none stays. A `kpi` changes no verdict, but `analyze` dropped a copy that
differed. A `spec` with an `entity` or a `number` already refused a repeated bound,
`duplicate values bound for 'V' at 12:3` without `loc`, and that stays as it was; the new wording
follows it. A `spec` with neither returns from `expand_spec_domains` before reading its bounds, so
`instances V = 1` then `= 2`, under any name, stays exit 0 on base and head. Nothing is lost there:
`check`, `kernel` 1 and 2 and `verify` on explicit printed the same with no bound, one, or two in
either order, and `verify` on bmc returned `violated` each time with a counterexample that varied
between runs. Other routes were not compared. Two `verify` blocks were already a parse
error. A document with another earlier error reports that one: a scenario that overflows the last
bound keeps `kind: "parse"` and names the bound it overflows. A repeated `concept` is reported ahead
of a repeated bound or `kpi`, wherever each is written; the others go in source order.

Verdicts the removal takes, on one document with an invariant that holds only under `0..1`,
`FSLC_CACHE=off`, base `3901764f0fd07c3e98a5cc78b8ca341a6d1276dd`, head `fc6d86d274ec1de6af8eac9ddb00c9529fdb7de1`:

| Route | Base, `0..3` then `0..1` | Base, `0..1` then `0..3` | Head, either |
|---|---|---|---|
| `check` | exit 0, `ok` | exit 0, `ok` | exit 2 at 14:3 |
| `verify` on bmc | exit 0, `verified` | exit 1, `violated` | exit 2 |
| `verify` on explicit, induction | exit 0, `proved` | exit 1, `violated` | exit 2 |
| `fsl-bfs` | no violation | invariant violation | exit 2, the message on stderr |
| `scenarios`, `testgen`, `mutate`, `html`, `ledger` | exit 0 | exit 1 | exit 2 |
| `sweep --depth 0..1` | exit 0, `sweep_passed` | exit 1, `sweep_failed` | exit 2 |
| `refine`, either side, against the `0..3` document | exit 2, a type error | exit 1, `impl_violated` | exit 2 |
| `diff` against the `0..3` document | exit 1, `semantic_diff` | exit 1, `semantic_diff` | exit 2 |
| `lint` | exit 1, `lint` | exit 1, `lint` | exit 2 |
| `fmt --check` | exit 1, `format_check` | exit 1, `format_check` | exit 2 |
| `replay --trace`, an empty trace | exit 0, `conformant` | exit 0, `conformant` | exit 2 |
| `kernel` 1 and 2, `typestate`, `fmt`, `explain`, `analyze`, `conformance`, `document generate`, `document claims`, `migrate` | exit 0 | exit 0 | exit 2 |

`verify --values V=0..3` returned `violated` on both base orders and is refused on head: an override
does not hide the repeat. `lint`, `fmt`, `migrate`, `document generate` and `document claims` omit `loc`.
`verify --values`, `kernel` version 1, `typestate` and `document` omit the file from the message, and
`kernel` version 2 prints a longer path. `compose use` of a requirements document was already
refused, `compose use must reference a spec`, on base and head. An inline `implements` reports
`implements "1_business.fsl" failed to check (duplicate kpi 'paid_claims' at 1_business.fsl:55:3)`, `kind: "type"`, with `loc`
on the `implements`. `fslc-lsp`, the Worker and `replay --from-log` were not run.

**Documented (#1437).** The `concept` refusal, its precedence over a repeated bound or `kpi`, and
the bounds and `kpi` refusals of a next-edition document are in `docs/manual/LANGUAGE.md` and
`docs/manual/LANGUAGE.ja.md` ("Repeated inputs, declarations and requirement IDs", anchor
`fsl5-repeats`) and in `skills/fsl/references/syntax.md`, with `docs/intro/language.{en,ja}.html`
regenerated.

Migration: keep the one declaration you mean. There is no automatic migration.

On-disk transition set (empty). Base `9c31458678cf6c70e77987c85c95f4bc0d8df503`, head `b2367d61fd8a410f9e1364301f1fab0d6b7c4cb7` (the rebase onto it), each binary built
from its own tree in its own target directory with Rust 1.98.0. Every `.fsl` under `specs/`,
`examples/` and `rust/fslc/tests/fixtures/` (651 forms: 23/195/433) ran with `FSLC_CACHE=off` and its
own tree as cwd, as `fslc check <path>` stable and with the next edition enabled, and as `fslc kernel
<path> --kernel-version 1|2`, twice per side. All 2604 pairs had equal complete stdout, stderr and exit
with no field excluded, and no run differed from its repeat: 0 ok-to-error and 0 error-to-ok
transitions. Before the rebase, the same sweep on base `3901764f0fd07c3e98a5cc78b8ca341a6d1276dd` and
head `fc6d86d274ec1de6af8eac9ddb00c9529fdb7de1` (646 forms, 2584 pairs) gave the same result.
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
