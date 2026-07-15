<!-- SPDX-License-Identifier: Apache-2.0 -->

# Shared finite Binder and Aggregate IR

## Decision

Quantifiers and aggregate operators use one finite Binder representation:

- `x: Type`
- `x in lo..hi`
- `x in collection`
- an optional `where predicate` on every form

`count`, `sum`, `unique`, and `exactlyOne` are represented by one
`Aggregate { kind, binder, value }` node. Only `sum` carries `value`; the other
kinds derive their result from the number of matching bindings. The former
typed-only `Count`/`Sum` nodes and named-binder cardinality node are removed.
This makes scope, type checking, traversal, substitution, mutation, analysis,
concrete evaluation, and symbolic evaluation share the same Binder path.

## Source forms

The canonical quantifier spelling uses braces:

```fsl
forall x in queue where x > 0 { ready(x) }
```

The loss-aware syntax tree records both brace use and the legacy colon marker.
The 2.x `forall x in queue: ready(x)` spelling is accepted as non-canonical
input and canonical rendering uses braces.

Aggregate forms are:

```fsl
count(x: Item where selected[x])
count(x in queue where x > 0)
sum(x in queue of x.amount where x.valid)
unique(x in queue where x.id == wanted)
exactlyOne(x in 0..CAP-1 where occupied[x])
```

Map, unbounded domains, and general folds are outside this design. `.size()`
remains available and is not rewritten into an aggregate.

## Finite semantics

A typed Binder enumerates its finite declared domain. A range Binder enumerates
both endpoints inclusively. A Set Binder enumerates distinct present members.
A Seq Binder enumerates the live prefix by position; equal values in different
positions are distinct bindings for cardinality and summation.

The filter is evaluated after inserting the bound value. Empty matching sets
produce `count = 0` and `sum = 0`; `unique` is true and `exactlyOne` is false.
Concrete Monitor/BFS and symbolic verification call the same Binder-shaped
enumeration/filter logic. Symbolic Seq candidates retain the per-slot
`index < length` membership guard.

## Public Kernel boundary

Public Kernel v1/v2 keeps its existing typed `count` and `sum` JSON shapes.
Collection and range aggregates are normalized at export:

- Set candidates use the finite element domain plus `.contains(candidate)`.
- Seq candidates expand to the fixed capacity with `index < .size()` and
  `.at(index)` inside an `ite` branch.
- Range candidates expand to their statically finite integer values.

The result uses only already-published conditional, arithmetic, membership, and
collection-method nodes. This preserves schema/version compatibility and keeps
partial Seq access guarded. A future direct aggregate-binder JSON shape would
require a Public Kernel major version.

## KPI projections

`kpi name = count Entity in Stage` in business and requirements documents is
validated against the resolved process and stored as `ProjectionDef` metadata
containing the same typed `Aggregate::Count` IR. It creates no state, action, or
invariant. Native explain output reads the retained name/entity/stage metadata;
unknown or ambiguous entities and unknown stages fail during lowering.

## Required evidence

Tests cover typed/range/Set/Seq filters, empty collections, duplicate Seq
multiplicity, nested aggregate evaluation, non-Bool filter rejection,
Monitor/symbolic/explicit agreement, canonical rendering, legacy source-form
retention, KPI metadata without ghost invariants, and schema-compatible Public
Kernel normalization.
