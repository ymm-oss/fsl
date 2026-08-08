# Initialization forms and migration

FSL keeps three initialization forms at different abstraction levels. Kernel
`init` describes system-wide relational initialization, requirements carried
fields initialize one value per process entity, and domain aggregate fields
initialize aggregate-owned state. They share checked expressions where their
contracts overlap, but they are not one interchangeable syntax.

## Kernel inline initializers

A Kernel state field may carry a deterministic value expression:

```fsl
state {
  status: Status = Pending,
  count: Count = 0,
  current: Option<ItemId> = none,
  queue: Seq<JobId, 3> = Seq {},
}
```

The surface tree retains the field span, initializer span, and inline source
form. Before checked-model construction, each initializer is normalized to an
ordinary assignment at the beginning of the spec's logical `init` sequence.
The Monitor, explicit engine, BMC, induction, mutation, and Public Kernel v1
therefore consume the same statements as the equivalent explicit `init` form.
Public Kernel v1 does not publish a second initializer representation.

An inline initializer is deliberately narrower than an `init` block:

- its expression must not read any state root, including another inline field;
- ordinary constants, enum members, constructors, and deterministic collection
  literals are allowed and use the shared expression/type checking path;
- statement `if`, `forall`, indexed/field targets, and relational or bulk
  initialization remain in `init`;
- a state root initialized inline cannot also be assigned by any explicit or
  generated init statement; root/path overlap is a semantic error.

The overlap check is a core model-construction invariant, not a backend check.
Its diagnostic uses the inline field as the primary source and the conflicting
init assignment as a secondary source so every execution engine fails closed in
the same way.

## Implicit-value migration

The current edition preserves already-existing implicit values but reports the
stable warning code `implicit_initial_value`. Each finding contains the field
source span, selected value, selection reason, and current/next severity.
Where a machine-applicable insertion is safe to offer (see below), a finding
additionally carries a canonical replacement and a byte insertion edit; a
finding without one is still reported, just not auto-fixable yet.

Domain aggregate fields warn whenever an omitted initializer selects an
implicit default -- every shape `fsl_core::domain_type_default` (the same
total dispatch `fslc domain expand`'s renderer uses) can select one for, not
a separately maintained scalar subset (issue #731):

- `false` for `Bool`, `0` for `Int`;
- the first declared enum member (rendered as domain source itself would
  accept, e.g. `Pending`, never the generated kernel's mangled
  `Status_Pending` -- including when the enum is nested inside a
  `value_object`'s struct literal or a `Map`'s per-key value);
- the lower bound of a range, or `0` for an external placeholder without a
  declared lower bound;
- `none` for `Option<T>`;
- `Set {}` for `Set<T>`;
- a `value_object`'s own default struct literal, built from this same
  selection recursively over its fields;
- for a top-level `Map<K, V>` field, the dense per-key
  `forall k: K { field[k] = <V's default> }` init `fslc domain expand`
  renders directly -- `<V's default>` recurses through this same selection,
  and `field: Map<K, V> = expr;` is always rejected
  ("whole-Map domain defaults are not supported"), so this is the *only*
  supported `Map` default.

Two of those shapes do not currently carry a machine-applicable insertion,
and consequently keep next-edition severity at `warning` rather than `error`
(`check --edition next`/`migrate --edition next` cannot yet demand an
initializer they have no safe way to insert): a top-level `Map<K, V>` field,
which has no whole-field initializer syntax at all, and a `Set<T>` or
`value_object`-typed field, whose brace-literal default (`Set { ... }`,
`Name { ... }`) is valid FSL that `check` accepts but that the lossless
formatter cannot yet round-trip through a reformat-and-reparse pass
(issue #770, a pre-existing formatter defect discovered while adding
container-type coverage, independent of this migration contract). Reporting
the finding without the insertion is deliberate: `migrate --write` is
fail-closed and would not write a corrupted file, but attempting the edit
would trip #770's reformat failure and fail migration for the whole file,
dropping every other, otherwise-safe edit in it too.

Requirements process fields warn only when a `number` field omits its initializer
and therefore selects the declared `verify.values` lower bound. Requirements
`Bool` and enum fields continue to require explicit initializers; omission remains
a check-time error. This preserves the accepted requirements contract instead of
inventing a new implicit value.

Where an edit exists, it inserts ` = <selected value>` immediately after the
field type. Applying it preserves surrounding comments/trivia and yields the
value already selected by the current semantics. The lossless formatter and
edition migrator consume this diagnostic contract. `fslc migrate --edition next`
inserts the selected value only after comparing the checked before/after Public
Kernel; `--write` applies it with the other validated file edits.

The next edition severity is `error` for every insertable finding;
`check --edition next` requires that initializer to be explicit. The current
edition continues to parse every omitted form and report its warning,
including the two shapes above with no insertion yet.
