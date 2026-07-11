# Predictive BMC from a State Snapshot

Status: implemented for issue #175.

## 1. Goal and command

`fslc verify SPEC --from-state state.json --depth K` asks whether the complete
logical state in `state.json` can reach a violation within `K` steps. It replaces
the spec's `init` constraints for this BMC run; it does not add the snapshot on
top of `init`.

The feature is intended for incident what-if analysis, pre-operation risk
checks, and warnings from a current production snapshot. It is not a claim that
the snapshot itself was produced by the spec or reached from `init`.

## 2. Snapshot contract

The input is the same complete logical-state JSON shape emitted by
`Monitor.state`, replay traces, and generated conformance adapters:

- every state variable is present and no unknown variable is accepted;
- enums use member names, Option uses JSON `null` or its contained value;
- Map keys use the displayed strings (`"0"`, enum member, `"true"`/`"false"`);
- struct and Map-of-struct objects contain every field;
- Set/Seq are arrays; relation is an array of two-element arrays;
- domain bounds, Seq capacity, duplicate Set elements, and duplicate relation
  pairs are checked before solving.

`model.validate_state_snapshot` owns this validation and converts the public
logical form into typed internal values. `bmc.state_snapshot_constraints` then
pins the symbolic initial state. Maps cover every bounded key. Set/relation
arrays are equated to complete constant-store arrays so values outside the
logical domain cannot manufacture a false type-bound counterexample. Unused Seq
slots receive a valid default because they are physically present but absent
from the logical JSON shape.

Partial snapshots are deliberately rejected. Treating an absent variable as a
free symbolic value would answer a different existential question and could
make an unsafe production state look safe.

## 3. BMC-only semantics

`--from-state` is accepted only by the BMC engine. K-induction proves a contract
from all states satisfying its induction hypothesis and uses the spec's base
case; replacing that base with one production point would change the meaning of
`proved`. Combining `--engine induction --from-state` is therefore a semantics
error, not a downgraded proof.

The snapshot names concrete entity identities, so symmetry reduction is
disabled for the run. Otherwise a valid labeled production state could be
discarded merely because it is not the canonical representative of its
symmetry class.

All ordinary bounded checks still run: invariants, transition invariants,
reachables, action coverage, deadlocks, and bounded liveness. Property filters,
scope overrides, deadlock mode, and vacuity mode keep their existing meanings.
Acceptance/forbidden self-checks and an `implements` seam remain spec-level
checks; they are not rewritten as production-snapshot procedures.

## 4. Faithfulness and cache contract

Every successful BMC invocation (whether its result is `verified` or
`violated`) adds:

```json
{
  "initial_state": {
    "source": "snapshot",
    "path": "state.json",
    "complete": true,
    "replaces_spec_init": true
  },
  "faithfulness": {
    "scope": "bounded_from_snapshot",
    "spec_init": "not_used",
    "induction": "not_applicable"
  }
}
```

This prevents the ordinary `verified` label from being read as “verified from
the declared init.” The existing bounded `completeness` and `checked_to_depth`
fields still describe the search horizon.

Snapshot runs bypass the persistent verdict cache. The current cache key is
defined over the spec and verify flags, not arbitrary external state content;
reusing a normal-init or different-snapshot verdict would be unsound. A future
cache may opt in only after hashing the canonical typed snapshot into the key.

## 5. Failure and rollout boundaries

- Invalid JSON is an `io` error with line/column.
- Missing/extra variables, wrong types, or out-of-range values are `type`
  errors before Z3 runs.
- A snapshot that already violates an invariant returns a shortest trace with
  `violated_at_step: 0`.
- No schema migration, daemon, production connection, or snapshot persistence
  is introduced.

The implementation touches `model.py`, `bmc.py`, and `cli.py`. Regression tests
in `tests/test_from_state.py` cover step-zero violation, safe bounded results,
faithfulness metadata, induction rejection, CLI behavior, and Monitor-state
round trips across scalar, enum, Option, struct, Map, Set, Seq, and relation.
