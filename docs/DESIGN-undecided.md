# Intentional undecided declarations

Issue: #189

## Goal

FSL represents deferred decisions as nondeterminism, but nondeterminism alone
does not say whether the freedom is intentional. The reserved declaration tag
`"undecided: reason"` records that a specification owner has reviewed a choice
and deliberately left it open. The tag is metadata: it does not add a guard,
property, assumption, or verification obligation.

## Syntax and scope

`undecided` uses the existing single declaration-tag slot. The canonical spelling
is lower-case and the parser recognizes it case-insensitively. It is available on
kernel declarations that accept metadata tags, plus `init`:

```fsl
init "undecided: initial operating mode will be selected later" { ... }
action route() "undecided: routing policy is pending" { ... }
trans Routing "undecided: cross-step routing constraint is pending" { ... }
```

The same slot cannot simultaneously carry a requirement tag. Therefore
`undecided` is never treated as a requirement ID. A declaration's affected
requirement IDs are derived from the model dependency graph instead.

The authoritative implementation is the native Rust CLI. The Python
implementation is frozen as a compatibility reference and is intentionally not
changed for this feature; tests and snapshots for this contract live on the
Rust CLI side.

## Affected-requirement calculation

Each declaration receives a deterministic state footprint:

- `init`: assignment targets, right-hand sides, branch conditions, and binders;
- `action`: guards, lets, assignment targets/values, branch conditions, and
  ensures clauses;
- properties: every referenced state root in the property expression (and both
  sides/decreases for `leadsTo`).

A requirement ID is affected when one of its tagged declarations has a
non-empty footprint intersection with the undecided declaration. Output is
deterministic (IDs lexicographically, declarations in model order); the
dependency comparison itself uses sets. An empty result is rendered as `—`,
not guessed from prose.

## Report surfaces

`fslc ledger` adds `## 未決定一覧`; `fslc html` adds
`Intentional Undecided Decisions`. Both list the declaration, reason, and
affected requirement IDs, and both state that the marker is metadata rather
than a verification condition. Reports omit the section when no marker exists.

## Underspecification acknowledgement

`analyze --profile ai-review` continues to emit every `divergent_choice` and
`unconstrained_effect` finding. It does not suppress reviewed freedom. A finding
whose `involved_nodes` contains an `undecided` declaration gains:

```json
{
  "acknowledged": true,
  "acknowledged_by": [
    {"declaration": "action route", "reason": "routing policy is pending"}
  ]
}
```

An unmatched semantic finding carries no acknowledgement fields and remains an
unresolved finding. The fields are additive in `analysis-findings.v0`;
`formal_status` remains
`not_a_violation`. Acknowledgement is exact declaration-node matching, never a
natural-language similarity judgment.

## Non-goals and limits

- Proving that an undecided declaration is safe; ordinary verification still
  checks every allowed resolution.
- Suppressing findings or lowering their evidence strength.
- Inferring product decisions from the tag reason.
- Supporting more than one metadata tag on a declaration.
- Treating requirements-process `covers` as an undecided marker; `covers`
  remains the transition-to-requirement traceability contract.
