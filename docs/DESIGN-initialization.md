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
source span, selected value, selection reason, current/next severity, canonical
replacement, and a machine-applicable byte insertion edit.

Domain aggregate fields warn when omission selects:

- `false` for `Bool`;
- the first declared enum member;
- the lower bound of a range;
- `0` for an external placeholder without a declared lower bound.

Requirements process fields warn only when a `number` field omits its initializer
and therefore selects the declared `verify.values` lower bound. Requirements
`Bool` and enum fields continue to require explicit initializers; omission remains
a check-time error. This preserves the accepted requirements contract instead of
inventing a new implicit value.

The edit inserts ` = <selected value>` immediately after the field type. Applying
it preserves surrounding comments/trivia and yields the value already selected by
the current semantics. The lossless formatter (#248) and edition migrator (#249)
own whole-source rewriting and consume this diagnostic contract; this feature does
not introduce an incomplete formatter or migrator command.

The next edition severity is reported as `error` for migration planning, but this
design does not remove implicit-default parsing. Breaking removal remains gated by
the edition migration policy.
