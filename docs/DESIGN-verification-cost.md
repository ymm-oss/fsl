# Verification cost contract

Issue: #271

## Goal

Every native and browser `verify` result exposes one machine-aggregatable cost
shape. The contract reports total wall time, the checks issued to the selected
solver backend, a stable common subset of Z3 statistics, and check time grouped
by semantic property. It is diagnostic evidence, not part of the verdict.

## JSON contract

`cost` follows the `verificationCost` definition in
[`schemas/fslc/envelope.v1.schema.json`](../schemas/fslc/envelope.v1.schema.json):

```json
{
  "elapsed_s": 0.012,
  "solver": {
    "checks": 14,
    "check_elapsed_s": 0.004,
    "conflicts": 3,
    "decisions": 8,
    "propagations": 21,
    "memory_mb": 18.2
  },
  "properties": [
    {
      "kind": "invariant",
      "name": "BalanceNeverNegative",
      "checks": 4,
      "elapsed_s": 0.001
    }
  ]
}
```

All keys are always present. `conflicts`, `decisions`, `propagations`, and
`memory_mb` are `null` when the backend does not report them. The other fields
are non-negative numbers. `properties` is sorted by `(kind, name)`, and its
check counts sum to `solver.checks`.

`elapsed_s` covers the complete command verification path. `check_elapsed_s`
and each property `elapsed_s` cover only backend satisfiability calls. Timings
are nondeterministic and native/browser values are not expected to be equal.

## Z3 aggregation

Z3 exposes statistics for its last check rather than a lifetime counter. For
`conflicts`, `decisions`, `propagations`, and `memory_mb`, FSL therefore reports
the maximum value observed after any constituent check. It does not sum
snapshots that may already be cumulative. `checks` and `check_elapsed_s` are
measured by the FSL adapters and are accumulated exactly.

Native Z3 and `z3-solver` may spell raw keys differently. Each adapter maps
`conflicts`/`sat conflicts`, `decisions`/`sat decisions`, direct propagations or
the sum of SAT binary/n-ary propagations, and `memory`/`max memory` into the
fixed fields. Raw backend-specific keys never enter the public envelope.

Induction merges its bounded base and proof solver sessions: check counts and
times are summed, maximum-observation statistics use `max`, and equal property
identities are combined. The explicit engine performs no SMT checks and emits
the same shape with zero checks, null Z3 statistics, and an empty property
array alongside its existing state-exploration fields.

## Property attribution

The current kinds are `init`, `type_bound`, `invariant`, `trans`, `ensures`,
`reachable`, `action_coverage`, `deadlock`, `leadsTo`, and `leadsTo_rank`.
Names are stable Kernel property or action identifiers; `initial_state` and
`deadlock` name their structural checks. Witness reconstruction remains charged
to the property whose satisfiability query produced it.

The JSON Lines event stream is outside this contract. Cached verdicts retain
the cost recorded by the original solve; a cache hit does not rewrite it as a
new solver measurement.
