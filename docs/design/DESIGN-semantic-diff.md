# Semantic specification diff

Issue: #176

## Goal and boundary

`fslc diff OLD.fsl NEW.fsl --depth K` compares two specifications as state
machines rather than comparing their source text. It answers three bounded
review questions:

1. Does NEW admit a behavior OLD did not (`behavior_added`)?
2. Does NEW remove a behavior OLD admitted (`behavior_removed`)?
3. Did the declared safety contract become weaker or stronger
   (`invariant_weakened` / `invariant_strengthened`), or did an OLD
   `forbidden` scenario become executable (`forbidden_relaxed`)?

This is analysis, not an unbounded proof. Every result includes
`bounded:{depth, completeness:"bounded"}`. A clean diff means no difference was
found within that contract and scope; it does not prove semantic equivalence at
all depths.

## Comparison algorithm

### Bidirectional refinement

With identity-compatible state and action names, the command synthesizes
`maps auto` and checks both directions:

- NEW refines OLD: failure is `behavior_added`.
- OLD refines NEW: failure is `behavior_removed`.

A directional failure carries the refinement counterexample as
`witness:{trace_type:"counterexample", trace, violation}`. This reuses the
refinement engine's shortest bounded implementation trace and keeps its action,
step, and mismatch evidence.

Different state or action names are not guessed. That direction is `unknown`
with the exact `only_impl` / `only_abs` names. `--mapping FILE` is the escape
hatch: the file may map NEW→OLD or OLD→NEW and is used for that direction. The
opposite direction still uses identity auto-mapping when possible; a mapping is
not mechanically inverted because arbitrary state expressions and stutter do
not have a sound general inverse.

### Invariant implication

When the logical and physical state schemas match, the command asks Z3 whether
the conjunctions of user invariants imply each other, under the implicit type
bounds:

- OLD ⇒ NEW and not NEW ⇒ OLD: `invariant_weakened`.
- NEW ⇒ OLD and not OLD ⇒ NEW: `invariant_strengthened`.
- both implications: equivalent invariant contracts.
- neither implication: `unknown` (`invariant_sets_are_incomparable`).

A failed implication carries a concrete state witness. Implicit type-bound
invariants constrain the query but are not themselves classified as authored
invariant changes.

### Forbidden replay

Each OLD `forbidden` scenario is replayed against NEW. If NEW accepts the step
OLD required to be rejected, the result contains `forbidden_relaxed` and the
accepted trace. A scenario that cannot be related because its action or
arguments no longer exist becomes `unknown`, not a false relaxation finding.
Scenario arguments are evaluated against the OLD typed model and OLD replay
state before their values are matched to the NEW action parameter domains; NEW
must not reinterpret a same-spelled nominal enum member as its own type.
The finding carries `subject:"forbidden"`, the OLD case `id`, a stable
`reason` (`forbidden_step_unrelatable` or `forbidden_replay_failed`), and the
zero-based failing `step` plus `action` when available. A matching action that
is disabled by its guard is a related rejection and therefore preserves the
forbidden scenario; it is not reported as `unknown`. Known gap (issue #1239):
the implementation applies this to a setup step as well, so a NEW setup step
disabled by its guard is reported as preserved although NEW never runs the
final step; only a disabled final step is evidence of a preserved rejection.
A final step that both OLD and NEW reject as `bad_call` outside a declared type
is preserved too, even though NEW cannot relate its arguments to an action: on
both sides, every same-named action of that arity has an argument outside a
range or enum parameter type. Each side is classified with its own `entity` /
`number` types (DESIGN-forbidden.md §2), so a type that either side declares as
an `instances` / `values` scope never yields a preserved `bad_call`. NEW's are
known only for a spec, business, or requirements NEW, so a compose or
other-dialect NEW never preserves a forbidden this way. An OLD final step
outside an `entity` / `number` verify scope (including the NEW scope OLD is
replayed under) was rejected by no guard (#1229), so it is `unknown` with
`forbidden_replay_failed`; one only NEW's scope excludes is `unknown` /
`forbidden_step_unrelatable`. Before #1229 both were `unknown` /
`forbidden_step_unrelatable`. An OLD final step that names no action,
or no variant of that arity, is `unknown` / `forbidden_replay_failed`. A NEW
final step that is enabled and then stops with a runtime violation is not a
rejection either (DESIGN-forbidden.md §2, #1213): it is `forbidden_relaxed`, and
its witness carries `violation` (`{kind, name}`). An OLD final step that is
enabled and then violates was never a rejection to preserve, so it is `unknown`
with `forbidden_replay_failed`.
A NEW setup step that is enabled and then violates is `unknown` with
`forbidden_replay_failed` as well: NEW never reaches the final step, so it
neither preserves nor relaxes the OLD rejection. The same reasoning applies to
a NEW setup step that its guard disables, but that step is still reported as
preserved (the known gap above, issue #1239).

## Scope changes

Source-level `verify { instances ...; values ... }` bounds are recorded under
`scope.old` and `scope.new`. A difference is a first-class `scope_changed`
finding. Comparisons use the NEW side as the declared comparison scope: shared
OLD entity/number bounds are overridden with NEW's values before OLD is
desugared and built. `scope.applied_to_old` records the exact overrides, so a
consumer never has to infer which finite universe was compared.

Inline kernel `type X = lo..hi` declarations remain type contracts, not
`verify` scope metadata. Refinement's normal domain-bound handling applies to
them.

## JSON and exit contract

The stable top-level shape is:

```json
{
  "result": "semantic_diff",
  "bounded": {"depth": 8, "completeness": "bounded"},
  "scope": {"old": {}, "new": {}, "comparison": "new", "applied_to_old": {}},
  "directions": {"new_to_old": {}, "old_to_new": {}},
  "summary": ["behavior_added"],
  "findings": [],
  "gate": {"forbidden": [], "violations": [], "passed": true}
}
```

Finding kinds are `behavior_added`, `behavior_removed`,
`invariant_weakened`, `invariant_strengthened`, `forbidden_relaxed`,
`scope_changed`, and `unknown`. With no findings, `summary` is exactly
`["no_semantic_change"]`.

Analysis completion exits 0 even when findings exist. CI policy is explicit:

```bash
fslc diff old.fsl new.fsl --depth 8 \
  --forbid behavior_added,invariant_weakened,forbidden_relaxed
```

Only a finding named by `--forbid` makes `gate.passed:false` and exits 1.
Parse/type/IO errors remain exit 2 and internal failures exit 3. This separates
an informative change report from a repository-specific compatibility gate.

## Non-goals

- Source/AST edit descriptions; use the VCS diff for those.
- Unbounded language equivalence.
- Automatic inversion of arbitrary refinement mappings.
- Comparing non-adjacent project-chain revisions; a future project-aware layer
  can compose mappings and call this directional core.

Git revision selection is provided by the separate thin adapter documented in
[`DESIGN-diff-git.md`](DESIGN-diff-git.md). It materializes inputs and calls
this unchanged two-path core.
