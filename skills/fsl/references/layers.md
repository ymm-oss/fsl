## Connected workflow (across layers — when alignment is the deliverable)

When the value is cross-layer alignment rather than one spec (the connectivity lens
in [when to use FSL](when-to-use.md)), the workflow changes shape: author each layer,
then verify the **seams** between them. Here the connecting operations are the stars,
not an advanced afterthought. Layer syntax and the three-layer dialects are below;
route authoring through the role skills.

1. **Author each layer** as its own spec — `business` ⊒ `requirements` ⊒ `design`
   (⊒ implementation) — via the role skills (fsl-business / fsl-requirements /
   fsl-design). Verify each on its own first with the core skill's
   `check` → `verify` → `--engine induction` loop.
2. **Stitch the seams downward — each seam is a refinement obligation = a contract:**
   - requirements → business: put `implements BusinessName from "business.fsl" { }`
     in the requirements spec; `verify` then also runs the refine and reports it under
     the `implements` field of the result JSON (an empty body auto-generates identity
     refinement when names match). This seam is the exception to the exit-code
     contract: assert `implements.result == "refines"` yourself — a failed seam
     still exits 0.
   - design → requirements: a mapping file + `fslc refine design.fsl requirements.fsl
     mapping.fsl`.
   - when an upper response must survive the seam, add
     `preserve progress { respond AbsLeadsTo by impl_action, ... }` to the mapping
     (see the soundness note below).
3. **Gate the whole chain at once** with `fslc chain fsl-project.toml`: it runs
   business → requirements → design → impl from a manifest and returns a per-layer
   table (a failed layer stops the chain unless `--keep-going`). This is the connected
   analogue of single-spec `verify`.
4. **Read counterexamples by seam.** A `refinement_failed` / `implements.violation`
   names the seam that broke; repair in line with the contract — never weaken the
   upper layer just to make the lower one pass (that hollows out the very traceability
   the chain exists to prove).

### Two soundness facts about connection (read before trusting a chain)

- **Safety descends, liveness does not.** Refinement propagates safety (`invariant` /
  `trans`) downward for free, but a response property (`leadsTo`) does **not** ride
  down with it: a safety refinement can return `refines` while a lower-layer `leadsTo`
  fails. To keep an upper response across a seam, either re-prove it at the layer that
  owns progress, or pull it through the mapping with `preserve progress` (failure is
  `refinement_failed / progress_lost`; the actual lasso exclusion still comes from
  lower-layer `fair action` declarations). This is a general property of forward
  simulation, not an fslc limitation.
- **A chain is exactly as strong as its refinement soundness.** "Verified
  traceability" is real only if each seam genuinely fails when it should; a link that
  silently passes where it ought to break turns the chain into false confidence. So
  treat a green seam like a green single spec — confirm it is not vacuous (e.g. the
  `mutate` kill-rate per layer) and never relax a mapping just to turn a seam green.

## Three-layer dialects (consulting / requirements / design)

A spec can be written in three layers. Chain **business ⊒ requirements ⊒ design ⊒
implementation** via refinement. Every layer expands
to the kernel, so verify/induction/scenarios/Monitor are used identically. This
section is the **per-layer syntax**; for driving the layers end-to-end with
`implements` / `refine` / `chain`, see the connected workflow in this reference.

Treat the layer boundary as part of the contract — it is the refinement seam the
lower layer must honor. Do not move to the lower layer unless the user asks for it or
the relevant role skill directs it.

- `business Name { process/control/policy/kpi/goal }` — the consulting layer. For
  PM/consulting-facing files, prefer the readable stage syntax for common rules:
  `policy ... every Case in Source must eventually be Target [or Target ...]`,
  `policy ... every Case reaching Target [or Target ...] must have passed
  through Waypoint [or Waypoint ...]` (no-bypass; desugars to an invisible
  history flag + kernel invariant), `goal ... some Case can reach Target`, and
  `goal ... all Case can be Target [or Target ...]`. Use explicit
  `responds { forall ... stage(c) ... ~> ... }` / `{ expr }` only when the rule is
  not simple stage progression. Regulation contradiction = invariant violation,
  dead business step = coverage diagnostic, unreachable business goal =
  reachable_failed. Use `control ID "..."` for governance/catalog metadata and
  `policy/goal ... satisfies ControlID` for the actual checkable rule; violations
  then carry both the broken policy/goal and satisfied controls. A standalone
  `governance Name { ... }` catalog can require controls across business specs
  and run preservation refinements during `fslc check`.
- `requirements Name { process E with f: T {...} / kpi / acceptance /
  forbidden / implements Abs from "file" { } }` — the requirements layer. Use
  the process+data profile first for a single-entity lifecycle: transition
  clauses carry inputs (`with`), guards (`when`), field updates (`set`), and
  traceability (`covers`). Put verifier bounds in `verify { instances E = N
  values T = lo..hi }`. With `implements`, verify simultaneously runs the refine
  to the upper layer (the `implements` field in the result JSON); an empty body
  auto-generates identity refinement when names match, `maps auto` is allowed for
  same-name kernel-wrapper state/actions, and auto-mapped process transitions are
  actor-checked; the inline block also takes action-correspondence items
  (`action impl(..) -> abs(..) | stutter`), including an arity change, the same
  syntax a separate refinement file uses. `acceptance` is replay-checked at
  check time and supports `expect E id in Stage` as well as `expect <expr>`,
  then flows scenarios → testgen; action arguments in `acceptance`/`forbidden`
  accept enum member names as well as numeric ordinals. `forbidden` (must-forbid)
  conversely writes an "operation sequence that should be rejected" and
  verifies at check time that the last step is rejected (not-enabled or a
  violation) — if accepted, `kind: "forbidden"`. Carried fields (`f: T`) accept
  `number` (optional initializer, default `lo`), or `Bool`/enum (initializer
  required). Use kernel-wrapper `struct` / `state` / `init`, `fair action`,
  `branches`, and explicit `maps` only for hard cases such as multi-entity
  behavior, conservation rules, SLA/time, or history that needs kernel state.
  An independent channel for catching under-constraint (missing guards) that a
  safety invariant stays silent about (a receptacle for cross-validation where a
  separate agent writes positive/negative traces from NL)
- The design layer is an ordinary `spec` (the main subject of this guide). Connect
  it to the requirements layer with `fslc refine`
- **Traceability**: a `"ID: source"` tag immediately before a declaration's `{`.
  `requirement: {id, text}` appears in violated / CTI / coverage / scenarios — when
  you read a counterexample, always look at the requirement and repair in line with
  that requirement's intent

## 10. Three-layer dialects (consulting / requirements / design)

The layers chain via refinement: business ⊒ requirements ⊒ design ⊒ implementation
(testgen/replay). Every dialect expands as AST into the kernel, so all the commands
in §7 work as-is.

### Declaration tags (common to all layers)

Use a typed annotation immediately before an invariant / trans / reachable /
leadsTo / action:
`@requirement("REQ-LEDGER-003", "ledger consistency")` followed by
`invariant PaidLedger { ... }` → `requirement: {id, text}` in violated /
unknown_cti / coverage diagnostic / scenarios / `refinement_failed` (root).
Semantic declarations own the ID after their keyword; `@requirement` links a
declaration to an owned ID, and process `covers` is canonical dialect sugar for
the same relation. The older `"ID: source"` slot is migration input and linted
as `legacy_string_metadata`.

Reserved intentional-undecided metadata uses the same single tag slot:
`init "undecided: initial mode pending" { ... }` or
`action choose() "undecided: selection policy pending" { ... }`. It is not a
verification condition or requirement ID. `ledger` / `html` list the marker and
state-dependency-derived affected requirement IDs; `analyze --profile ai-review`
retains matching underspecification findings with `acknowledged:true`. The
source slot remains singular, but native lowering converts it, requirement
blocks, process `covers`, acceptance, and forbidden IDs into the shared typed
annotation carrier — the same carrier the `@...` syntax in §1 populates
directly, at both the document and the declaration level. An outer
requirement can therefore coexist with an inner `undecided` marker, whether
written as legacy strings, `@...` syntax, or a mix of both on one declaration.
Explicit `covers` and requirement-block annotations retain their own spans;
`undecided` is reserved and cannot be an explicit requirement ID.
Multiple-relation JSON outputs use `requirements` and preserve singular fields
as lexical compatibility projections. See `docs/DESIGN-undecided.md`,
`docs/DESIGN-annotations.md`, and `docs/DESIGN-dialect-dispatch.md`. This syntax and its
report surfaces are native Rust CLI features; the frozen Python reference is
not extended.

### Authoring specs as readable documentation (requirements + design)

The spec source IS the documentation: a rule you can read is also the rule that is
verified, so it never drifts. In the requirements and design (kernel) layers:

1. **Tag every invariant/action/property** with
   `@requirement("REQ-SCOPE-001", "one-sentence intent")` — the in-source prose
   that flows into all output (explain / html / counterexamples). It is
   NOT verified, so keep it a faithful paraphrase of the expression, not a rival truth.
2. **Use the active ID policy.** The built-in forms are
   `REQ|NFR|INV-{SCOPE}-{NNN}` for requirement relations,
   `AC|FB|POL|GOAL|CTRL-{SCOPE}-{NNN}` for their respective declarations, and
   `MODEL|ASSUME-{SCOPE}-{NNN}` for verification-only artifacts. Projects may
   partially override these templates in `[id_policy.patterns]` and pass the
   manifest explicitly with `fslc lint --project fsl-project.toml`. Pattern
   values use double-quoted JSON-compatible strings/arrays; model and assumption
   templates begin with literal prefixes that overlap neither each other nor
   requirement templates.
3. **Prefer member-quantification** `forall x in coll { P(x) }` over the index idiom
   `forall i in 0..N { i < coll.size() => P(coll.at(i)) }` — but ONLY (a) in expression
   position (invariant/property bodies; NOT action/init `forall` *statements*, which
   reject collection binders) and (b) for element-wise properties. Keep explicit indices
   for position, ordering, adjacency, or no-duplicates.
4. **Separate domain from verification bound.** Declare `entity X` / `number X` and put
   sizes in `verify { instances/values }` instead of `type X = lo..hi`. Allowed in a
   kernel `spec` too (desugars to `type`), so `type Claim = 0..2` no longer has to read
   as a false domain fact.
5. **Multi-line transitions** (requirements): `with` / `when` / `set` / `covers` each on
   their own indented line.
6. **Order:** domain content first, proof scaffolding last.

`fslc explain --readable` then renders the whole spec (state, tagged actions,
properties) as a structured digest — a view of the source, not a separate document.

### business (the consulting layer)

```fsl
business ReturnHandling {
  actor Customer, Manager            // roster (validates `by`)
  entity Return                      // identity sort; size set by verify below
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager   // -> fair action approve(c: Return)
    transition reject  Requested -> Rejected by Manager
    transition refund  Approved  -> Refunded by Manager
  }
  kpi refunded = count Return in Refunded     // -> metadata projection count(c: Return where stage(c) == Refunded)

  control CTRL-DECISION "Every return must preserve adjudication control"
    owner Manager
    severity high
    applies_to Return

  policy PAY-2 "every request is adjudicated"
    satisfies CTRL-DECISION
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be settled"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

`stage(c)` expands from the type of the bound c into the process's state Map
(`return_stage[c]`).
The natural business forms above are aliases for `responds { forall ... ~> ... }`
and `goal { forall/exists ... }`; the explicit expression forms remain available
for policies that cannot be written as a simple stage progression.

**No-bypass precedence** (#75): `policy CTRL-APPROVAL "..." every Return
reaching Refunded must have passed through Approved` synthesizes an invisible
`Map<Return, Bool>` history flag (`return_stage_via_Approved`), sets it `true`
on the transition landing on `Approved`, and compiles to `forall c: Return {
stage(c) == Refunded => return_stage_via_Approved[c] }`. A direct
`Requested -> Refunded` transition is then a genuine invariant violation with
the bypass shown in the trace. Both sides take a disjunction (`reaching A or
B`, `passed through X or Y`); two policies over the same `(process,
waypoint-set)` share one history flag (dedup, name deterministic by the
process's stage order). Alongside the flag, a `<PolicyId>_stability`
auxiliary invariant is auto-synthesized from the process's stage graph
(dominated-set of the waypoints; #85), so a **compliant** precedence policy
proves under `--engine induction` out of the box — no manual invariant, no
ghost CTI. Design in `DESIGN-precedence-policy.md`. Limitation: the flag is
business-layer-only synthesized state — a `requirements` spec refining it
must map the flag explicitly or restate the rule at its own layer.

`control` declarations are metadata only. Attach them to checkable business
rules with `policy ... satisfies CTRL` or `goal ... satisfies CTRL`. Unknown
control references are type errors, unused declared controls are warnings, and a
violated satisfied policy/goal reports `requirement.controls` in JSON.

For cross-business or enterprise-level controls, use a standalone `governance`
catalog. `fslc check governance.fsl` verifies that each delegated business spec
exists, each `require CTRL` is satisfied by business-side `satisfies` metadata or
an explicit `CTRL is satisfied_by policy|goal ID` mapping, and each
`preservation` block runs its declared refinement at depth 8.

No `terminal` syntax exists in business — it is derived automatically. Each
process's sink stages (stages with no outgoing `transition`, e.g.
`Rejected`/`Refunded` above) are collected; if every process has >=1 sink, one
kernel `terminal { }` is generated as the conjunction (over processes) of
`forall c: X { stage(c) in {sinks...} }` — so `ReturnHandling` above verifies
clean at its two sinks with no `--deadlock ignore`. If any process is cyclic
(every stage has an outgoing edge, so no sink), no terminal is generated for
the whole spec and deadlock checking is unchanged (a cyclic process always has
an enabled transition, so it can never deadlock anyway).

### requirements (the requirements layer)

```fsl
requirements ExpenseRequirements {
  implements ExpenseToBe from "1_business.fsl" { }

  number Amount
  const AUTO_LIMIT = 1

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit       Draft     -> Submitted by Employee with a: Amount when a > 0 set amount = a covers REQ-EXPENSE-001 "The applicant submits an expense claim by entering an amount"
    transition auto_approve Submitted -> Approved  by System  when amount <= AUTO_LIMIT covers REQ-EXPENSE-002 "Claims at or below AUTO_LIMIT are auto-approved by the system"
    transition mgr_approve  Submitted -> Approved  by Manager when amount >  AUTO_LIMIT covers REQ-EXPENSE-003 "Claims above AUTO_LIMIT are approved by a manager"
    transition reject       Submitted -> Rejected  by Manager when amount >  AUTO_LIMIT covers REQ-EXPENSE-003 "Claims above AUTO_LIMIT may be rejected by a manager"
    transition pay          Approved  -> Paid      by Finance covers REQ-EXPENSE-004 "Only approved claims are paid"
  }

  kpi paid_claims = count Claim in Paid

  acceptance AC-EXPENSE-001 "Approval flow: a low-amount claim is auto-approved and paid" {
    submit(0, 1) auto_approve(0) pay(0)
    expect Claim 0 in Paid
  }
  acceptance AC-EXPENSE-002 "Rejection flow: a high-amount claim ends in manager rejection" {
    submit(1, 2) reject(1)
    expect Claim 1 in Rejected
  }
}
verify {
  instances Claim = 3
  values Amount = 0..3
}
```

- The process+data profile is the default requirements form for a single-entity
  lifecycle. `process E with f: T { ... }` creates the entity stage map and
  carried fields; transition clauses add input (`with a: T`), guards (`when`),
  field updates (`set f = expr`), and traceability (`covers REQ-n "text"`). A
  carried field's type `T` is a `number`, `Bool`, or an enum declared in the
  same requirements spec. Numbers default to the domain's `lo` bound and may
  take an optional explicit `f: T = <const-expr>` initializer; omission emits
  `implicit_initial_value` with the selected lower bound and an insertion edit.
  `Bool` and enum
  fields have no invented default and **require** an explicit initializer
  (`f: Bool = true/false`, `f: T = Member`) — omitting it is a check-time error.
- `kpi NAME = count ENTITY in STAGE` is a declarative projection in both
  business and requirements; it does not create a ghost counter or an automatic
  `_kpi_*` invariant.
- When `implements Abs from "file" { }` is present and process/action/stage names
  match, fslc synthesizes the identity refinement mapping. Inside the
  `implements { }` block you write state `map` entries, `maps auto`,
  `preserve progress`, and `action <impl>(<params>) -> <abs>(<args>) | stutter`
  (same syntax as a separate refinement file's `action` item, including an
  arity change between impl and abs params — #73). Action↔action
  correspondence can also still be written as the `maps <abs_act>(...)` clause
  **on the requirement-level action** (auto-synthesized for matching names;
  `maps auto` covers same-name kernel-wrapper actions). Writing both a `maps`
  clause on an action and a matching inline `action ...` item for the same impl
  action name is a duplicate-correspondence error (`kind: "type"`,
  with both origin kinds and locations). An inline `action` item cannot target
  a `branches`-split action by its pre-split name — reference the generated
  `name__b<N>` alias. Auto-mapped process transitions are statically
  actor-checked; an actor mismatch is a check-time type error. The seam's
  verdict is reported as `implements: {abs, result}` with `result` one of
  `refines` / `refinement_failed` / `impl_violated` (the last one meaning the
  requirements spec breaks its own bounds/invariants, so no refinement verdict
  was reached), plus `violation` on the two failing values. It is **not** folded
  into the top-level `result` or the exit code — unlike standalone `fslc refine`,
  where `refinement_failed` is exit 1 — so a broken seam returns
  `result: "ok"` / `"verified"` and exit 0. Gate on
  `implements.result == "refines"`, or use `fslc chain`, which applies exactly
  that gate to the layer and exits 1.
- `acceptance` is replay-checked at check time with the concrete Monitor (failure is
  `kind: "acceptance"`). It supports the readable stage form
  `expect <Entity> <id> in <Stage>` alongside `expect <expr>`, is output to
  scenarios as `acceptance_<ID>`, and flows to testgen. Step action arguments accept
  enum member names and const names, not just numeric literals (`answer(0, Triggered)`
  == `answer(0, 1)`); an undefined name is a check-time error.
- `forbidden FB-EXPENSE-001 "source" { <steps> expect rejected }` is must-forbid (the dual of
  acceptance). The premise steps (all but the last) are all ok, and it succeeds if
  **the last step is rejected** (not-enabled, or an
  invariant/type_bound/partial_op/ensures violation). If accepted,
  `kind: "forbidden"` (detection of under-constraint = a missing guard that a safety
  invariant stays silent about); if the premise is not enabled,
  `kind: "forbidden_setup"`. Output to scenarios as `forbidden_<ID>` (with
  `rejected_by` — anything other than `requires_failed` means the spec itself is a
  verify violation).
- The kernel-wrapper form remains for hard cases: multi-entity requirements,
  conservation rules, SLA/time, history that is not expressible as a carried
  field, or any behavior that needs explicit kernel state. In that form, use
  kernel `struct` / `state` / `init`, `fair action`, `branches`, and explicit
  `maps` where needed. The display of a branches split action is
  `submit[a <= AUTO_LIMIT]`; diagnostics keep the internal name (`submit__b1`)
  and add `display_name`.
- Elements inside a requirement automatically get {id, text} metadata.
- `terminal { <expr> }` is allowed at the top level (pass-through to the
  kernel, one block per spec, same as the kernel). In the process+data profile,
  write `stage(c)` for a typed entity binder or parameter; it resolves to the
  process stage enum and lowers to the synthesized stage map. Requirements do
  not infer terminal states from sink stages.
- If several qualified processes share an entity type, declare paths such as
  `process claims.Claim` and use `claims.Claim.stage(c)` to disambiguate.
  Arbitrary-depth paths use the shared `SymbolPath` parser; generated
  `*_stage` names are not requirements source vocabulary.

### Drawing the layer boundary

The majority of NFRs are handled (§11). What stays outside FSL: probabilities,
percentiles, real time (wall-clock ms), usability, evaluator truth judgments,
statistical AI quality claims, and prose rationale (write those in each layer's
documents).
