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
Expression temporaries, including full-sequence `push` results and literals,
retain every live element even beyond the declared state capacity; the declared
N bounds state, not temporary storage. Checked state bounds enforce `len <= N`;
when a sequence's bound is not checked, a possibly over-capacity frame instead
fails closed before any property or action can read its missing suffix.
For a checked overflow caused by an action assignment, trace projection
reconstructs the actual attempted value from that action without displaying
padding or truncating the sequence. This reconstruction does not cover init:
symbolic projection of an initial frame with unrepresented elements fails closed
rather than inventing a witness, while the default concrete pre-pass can instead
report that overflow as `type_bound`. Non-bound invariant, `trans`, and ranked
`leadsTo` induction CTIs with missing slots also fail closed; bound-property CTIs
retain action-based reconstruction without changing the induction premises.

Capacity-zero Seqs with no physical slots follow the same live-prefix
definedness rule, with a fresh, unconstrained symbolic read slot outside the
declared capacity, materialized for each read-term construction rather than
stored as a state slot or a live temporary element. Empty live-prefix
temporaries that retain physical slots use those existing slots instead.
For a zero-slot frame whose bound is unchecked (`len > N`), the fail-closed
guards above from #1519, not definedness, keep the slot unobservable.

The `decreases` measure of a ranked `leadsTo` is a narrow exception to the
`partial_op` classification: ranking does not generally probe measure
definedness. Before evaluating its initial, successor, or deadlock measure,
a rank-only, path-sensitive preflight refuses only reached reads with no
physical slot whose fresh value would otherwise enter the rank evaluation.
It retains the pre-#1481 error (`head() on zero-capacity sequence` for `head`,
`cannot select from an empty finite domain` for `at`/index). Unreached reads in
untaken conditional branches or short-circuited operands can now yield a ranking
proof where the base reported a semantics error. Physical-slot measures retain
their existing behavior; this does not close the capacity-zero case or add
general measure-definedness semantics. In particular, capacity-zero
`xs.push(0).pop().head()` and the head of
`if n > 0 then xs else xs.push(1)` can still read a stale or default physical
slot and prove on both base and head; these remain #1542. The BMC rank-discharge
fast path uses the same preflight and discharges nothing on its error; it does
not report the ranking error itself.

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
