# Declaration tag drift and review export

Issue: #188.

## Boundary

FSL declaration tags put human wording next to checked definitions. That
locality improves review, but a rename or formula edit can leave the label
stale. `fslc` can detect exact identifier relationships deterministically; it
cannot decide whether arbitrary natural-language prose means the same thing as
a formula.

The implementation therefore has two layers:

1. `analyze --profile ai-review` emits narrow structural findings.
2. `analyze --export tag-review` emits one declaration tuple at a time for an
   explicitly chosen human or external agent reviewer.

Neither layer is a verifier result. Every finding and export contract says
`formal_status:"not_a_violation"`.

## Deterministic findings

`tag_stale_reference` is emitted when tag text contains a code-shaped token
(backtick-quoted, snake_case, or an uppercase constant-like token) that is not
a current state variable, action, type, enum member, constant, parameter, or
other identifier in that declaration. This intentionally avoids treating
ordinary English/Japanese words as identifiers.

`tag_formula_disjoint` is emitted when the tag exactly names a current state
variable or constant but the tagged action/property formal definition does not
reference it. State and constants are used because they can occur directly in
the checked formula; merely mentioning an action name in prose is not treated
as formula disjointness.

These checks can find a rename omission or a conspicuously missing formula
dependency. They do not establish semantic agreement. Matching identifiers
can still describe the wrong rule, and a prose-only tag can be correct without
containing any identifier.

## `tag-review.v0` export

```bash
fslc analyze spec.fsl --export tag-review
```

The single-file JSON export contains each tagged, non-generated action and
user property, sorted by kind/name:

- declaration kind/name/node id and source location;
- parsed tag `{id,text}`;
- a deterministic `formal_definition` (guards, ensures, effects, or rendered
  property expressions);
- `formal_identifiers` for reviewer grounding.

The stable schema is
`schemas/fslc/analysis/tag-review.v0.schema.json`. The mode cannot be combined
with graph projections, `--profile`, non-JSON format, focus, batch directories,
project manifests, or refinement mappings. This keeps the review unit local
and prevents mixed output contracts.

The export makes no model call. A consuming reviewer must compare `tag.text`
and `formal_definition` declaration by declaration, cite the tuple, retain
`formal_status:not_a_violation`, and ask for human confirmation rather than
silently rewriting the spec.
