# Intentional undecision metadata

Issue: #189.

## Convention

Intentional decision deferral uses the existing declaration-tag syntax:

```fsl
action approve() "REQ-1: undecided: approval policy awaits owner decision" {
  ...
}
```

Use `"REQ-n: undecided: ..."` when the open decision affects a requirement, or
`"undecided: ..."` for a spec-wide item. The marker is accepted on existing
taggable user declarations (actions and named properties). No grammar or
verification expression is added.

Requirements-dialect expansion preserves the surrounding requirement ID while
carrying an explicit action/property undecided marker into the kernel metadata.
FSL init is deterministic, so an open initial-state choice must be modeled as
an explicit action/decision point rather than hidden in init metadata.

## Semantics

The marker is metadata only. Removing or adding it cannot change `verify`,
induction, scenarios, runtime, or refinement results. It does not weaken an
invariant, disable a branch, or suppress a finding.

`undecided_declarations(spec)` normalizes each item to declaration kind/name,
node id, open-decision text, affected requirement IDs, source location, and
`verification_semantics:"metadata_only"`.

## Presentation

`fslc ledger` emits `## 未決定一覧` with declaration, open decision, affected
requirement IDs, and the metadata-only guarantee. `fslc html` emits the same
information in an “Undecided Items” section. An empty section states that no
intentional undecision has been declared; absence is not proof that the spec is
fully determined.

## Relationship to bounded underspecification

The #179 `divergent_choice` and `unconstrained_effect` findings remain present.
When a bounded witness includes an action carrying an undecided marker, the
finding adds:

```json
{
  "acknowledged": true,
  "acknowledged_by": [{"node_id": "action:approve", "requirements": ["REQ-1"]}]
}
```

Without a matching marker the same fields are `false` and `[]`. This makes the
review queue distinguish known decision debt from newly detected ambiguity
without hiding either. A marker on an unrelated declaration does not
acknowledge the finding.
