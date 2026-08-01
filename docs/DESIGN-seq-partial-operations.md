# FSL — Seq partial-operation semantics

Status: accepted for issue #650.

## Decision

FSL does not totalize `Seq` `pop()`/`head()`/`at(i)`/index reads. If evaluation
reaches one while the sequence is empty or the index is outside its live
prefix, every engine reports `partial_op`. Action context names
`_partial_<action>`; invariant, transition, reachable, and `leadsTo` property
context names `_partial_property_<property>`. An undefined `terminal`
predicate is `_partial_property_terminal` when it is consulted for a deadlock
state.

This is distinct from the existing `/` and `%` contract: division and
remainder by zero remain totalized to zero in property context and remain
`partial_op` only in action context.

## Symbolic contract

The symbolic evaluator may retain bounded slots beyond a Seq's live prefix as
an encoding detail, but no property value or witness may observe such a slot.
Before testing an invariant, transition, pending reachable predicate, or
`leadsTo` side, BMC probes the path-sensitive definedness condition. Terminal
definedness is probed under the deadlock condition, matching concrete
evaluation. A reached Seq failure wins over the ordinary property truth value
and produces a replayable violation. The condition is then asserted before the
property query, preventing a phantom slot from satisfying either polarity.

## Concrete and output contract

Monitor state checks, explicit/BFS reachable evaluation, and explicit terminal
evaluation translate the same concrete runtime failures into `partial_op`
rather than returning a raw runtime error. CLI/Worker rendering locates the
property declaration, carries its requirement metadata, and recommends an
implication/index guard. Short-circuit guards such as
`i < q.size() => q.at(i) == expected` remain defined.

## Controls

- The typed-agreement R6 head fixture requires Monitor BFS, explicit, and BMC
  to agree on kind, property carrier, and step.
- CLI regressions cover an invariant head read and a reachable index read, so
  both false-red and phantom-witness polarities are observed.
- Terminal regressions require symbolic and explicit verification to agree,
  while a `leadsTo` trigger regression prevents an undefined head from becoming
  a vacuously false antecedent and a false green.
- A property division-by-zero fixture remains verified, preventing accidental
  removal of the `/`/`%` totalization exception.
