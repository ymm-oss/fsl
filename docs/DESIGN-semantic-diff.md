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
