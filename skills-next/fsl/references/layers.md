# Connected workflow and layer boundaries

Contents: [workflow](#connected-workflow), [tags](#declaration-tags-common-to-all-layers),
[readable source](#authoring-specs-as-readable-documentation),
[business](#business-the-consulting-layer),
[requirements](#requirements-the-requirements-layer), [limits](#drawing-the-layer-boundary).

## Connected workflow

Author actual business ⊒ requirements ⊒ design layers through their role skills,
verify each, then check their seams:

1. Requirements → business: `implements Business from "business.fsl" { … }`.
   `check`/`verify`/`scenarios`/`testgen` evaluate this bounded refinement. Only
   `implements.result == "refines"` passes; failures are fail-closed.
2. Design → requirements: explicit mapping, then
   `fslc refine design.fsl requirements.fsl mapping.fsl`.
3. Implementation → design: wired Adapter tests or log replay at the same
   abstraction granularity. Generated scaffolding alone is not evidence.
4. `fslc chain fsl-project.toml` coordinates actual layers. Give a test-running
   `[impl]` a fresh JUnit report, not merely `evidence = "exit_code"`.

An empty inline mapping can synthesize identity only when names/types/parameters
are compatible; new requirements often needs explicit lowered correspondences.
Never weaken an upper contract to make a lower layer pass. Diagnose whether the
lower model, mapping, or human-approved upper interpretation is wrong.

Safety descends through checked refinement; liveness does not automatically.
For an upper response, verify it in the progress-owning layer or explicitly pull
it through `preserve progress { respond AbsLeadsTo by impl_action }`. The action
list does not grant fairness; lower fair actions still matter. Refinement permits
stronger guards, so it is not full LSP/substitutability. For same-named old/new
state/actions, `diff --forbid behavior_removed,unknown` can gate lost enabledness.
See [mapping syntax](syntax.md#compose-and-refinement) and
[review](../../fsl-design-review/SKILL.md).

## Declaration tags (common to all layers)

Use `@requirement("REQ-SCOPE-001", "intent")` before a kernel checked declaration.
Semantic declarations own IDs after their keyword; an event/decision row's
`covers ID "text"` links another source requirement. IDs/text are traceability,
not formulas. A rule present only in prose/comments is unchecked.

Use the active ID policy. Built-in relation prefixes are `REQ|NFR|INV`, case/control
prefixes `AC|FB|POL|GOAL|CTRL`, and explicit verification artifacts `MODEL|ASSUME`,
with scope and numeric suffix. Projects can partially override templates under
`[id_policy.patterns]`; pass `lint --project fsl-project.toml`. Pattern values are
JSON-compatible quoted strings/arrays; model/assumption literal prefixes must
not overlap each other or requirement templates. `--strict-tags` checks presence;
`lint` checks canonical form. `ids.txt` has one source ID per line.

`@undecided("reason")` records owner-accepted deferral, not a requirement or
verification condition. Stack it with requirement tags. Ledger/HTML show affected
IDs and analysis keeps acknowledged findings visible. `undecided` is not an
explicit requirement ID. Multi-relation JSON preserves `requirements`; singular
fields are compatibility projections, not evidence that a declaration has only
one relation.

## Authoring specs as readable documentation

- Put domain vocabulary first, proof scaffolding last.
- Carry verbatim source text on the declaration/ID it grounds. A tag explains
  its formula; it must not become a rival normative rule.
- Keep entity/number verification sizes in `verify`, rather than presenting
  arbitrary small counts as business truths. Kernel specs support these too.
- Prefer member quantifiers for element-wise properties, guarded indices for
  positional/order/uniqueness properties. Expression and statement forall have
  different initialization/read-order restrictions; see [syntax](syntax.md).
- Split long event clauses across lines and name rule properties so diagnostics
  identify source intent. `explain --readable` is a source view, not another spec.
- Encode expressible rules with actual checked declarations using
  [sentence→construct guidance](nl-to-syntax.md); keep unexpressible intent explicit
  outside formal claims.

## business (the consulting layer)

The [business owner](../../fsl-business/SKILL.md) supplies the complete checked
example and syntax. It retains `actor`, identity `entity`, `process/stages/initial/
transition`, `kpi`, `control`, `policy`, `goal`, and finite instance bounds.
Each transition is a fair action with case parameter `c`.

Explicit expression alternatives remain available for policy/goal intent beyond
simple stage phrases:

```fsl-fragment
policy POL-SCOPE-001 "response intent" responds { forall c: Case { P ~> Q } }
goal GOAL-SCOPE-001 "completion intent" { exists c: Case { Q } }
```

`stage(c)` uses the case type to select its generated stage Map. Business
no-bypass `must have passed through` policies add a history Map and stability
invariant; repeated waypoint sets share the generated flag in stage order.
The lower requirements seam must map that history state explicitly.

Controls are metadata: use `policy/goal … satisfies CTRL` to attach checkable
behavior. Unknown controls reject; unused controls warn; satisfied violated
policies retain control attribution. Business derives terminal from process
sink stages; if a process has no sinks, no whole-spec terminal is synthesized.

For cross-business catalogs use standalone governance:

```fsl-fragment
governance <Name> {
  authority <Actor> owns <ControlID>
  control <ControlID> "<text>" owner <Actor> severity <severity> applies_to <Entity>
  delegates <BusinessName> from "<business.fsl>" {
    require <ControlID>
    <ControlID> is satisfied_by policy <PolicyID>, goal <GoalID>
  }
  preservation <Name> {
    before <AsIs> from "<asis.fsl>"
    after <ToBe> from "<tobe.fsl>"
    preserve <ControlID>
    checked_by refinement "<mapping.fsl>"
  }
}
```

Delegation checks files and satisfaction links. Every preservation runs at depth
8, including through scenarios/testgen, regardless of their requested depth.
Read each result and the first failing outer verdict. Failed verification writes
no harness; unknown-budget is not preservation success.

## requirements (the requirements layer)

Use only the [requirements syntax owner](../../fsl-requirements/references/syntax.md),
[English/Japanese keywords](../../fsl-requirements/references/keywords.md), and
[glossary](../../fsl-requirements/references/glossary.md) for this edition:
entity/lifecycle/event, concept/relation/resource, rule/decide, measures,
scenario/forbidden, time/deadline, goal/policy and optional implements.
Do not put process/transition requirements forms under `fsl 5`.

Rules and decision rows lower to guards/properties. Check source→origin placement
with [fsl-verify's origin self-check](../../fsl-verify/SKILL.md#9-origin-self-check-files-headed-fsl-5).
Use canonical lowered state/actions for design mappings; stage enum order is
first appearance in lifecycle paths and must match a business seam.

Every positive scenario step must execute and its final check hold. A forbidden
requires all setup steps to execute and the final call to reject—not merely
violate a property. Unknown action/arity/kind or out-of-scope input is not a
successful rejection. Their diagnostics retain `acceptance`, `forbidden`, and
`forbidden_setup` kinds. They flow into scenarios/testgen for that same layer;
upper positives are not lower implementation tests. Requirements derives no
terminal; declare intended final states explicitly.

Kernel members and explicit requirement blocks remain useful for supported
low-level obligations; they do not authorize kernel-only `spec` files to contain
requirements scenario/forbidden declarations. For timing and clock ownership
see [NFR](impl.md#11-non-functional-requirements-nfr).

```fsl-fragment
requirement <ID> "<verbatim source text>" {
  <supported kernel actions/properties for this requirement>
}
```

Inline `implements` accepts `maps auto`, state maps and explicit action
correspondences, **not preserve progress**. Use a standalone refinement file
for progress preservation. Legacy action-level maps are migration input, not
new-edition guidance; branch-specific maps require manual conversion rather
than one unconditional correspondence. An inline correspondence for a split
action names its generated `name__b<N>` alias, not its unsplit name. Duplicate
explicit/legacy correspondences are errors, not override mechanisms.

## Drawing the layer boundary

Business: stages/actors/controls, not UI/API/data-model/queue implementation.
Requirements: externally observable behavior, not internal storage/transactions.
Design: internal mechanisms, explicit mappings and atomicity decisions.
Implementation: real operations and state projection, not a model-only proof.

Permissions, audit, capacity, reliability and discrete deadlines fit where the
source states them. Probabilities, real elapsed latency, free-text meaning,
screen appearance and external service truth do not become kernel guarantees.
Cross-cutting DB/domain/AI profiles remain supported separately in
[advanced](advanced.md), not invented extra business→requirements stages.
