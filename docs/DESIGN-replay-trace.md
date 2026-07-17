# Public Replay Trace Contract

Status: accepted and implemented for issues #221, #224, and #225.

## Goal and authority

An external compiler that consumes Public Kernel JSON must be able to emit a
trace that the OSS `fslc replay SPEC --trace TRACE.json` command can judge
without access to FSL's private AST or `KernelModel`. The normative JSON Schema
is `schemas/fslc/kernel/replay-trace.v1.schema.json`; the native Rust CLI is the
authoritative consumer.

Replay is finite runtime evidence. It checks concrete actions, safety
properties, complete observed states, and (since trace schema 1.2) bounded
`leadsTo` deadlines. It does not prove unbounded liveness, interpret wall-clock
time, or replace production-log refinement mappings.

## Versioned v1 envelope

```json
{
  "$schema": "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json",
  "schema_version": "1.2.0",
  "kernel_schema_version": "1.0.0",
  "spec": "ReplayTrace",
  "initial": {"phase": "Idle", "selected": null},
  "events": [
    {
      "tick": 1,
      "action": null,
      "params": {},
      "state": {"phase": "Idle", "selected": null}
    },
    {
      "tick": 2,
      "timestamp": "producer-sequence-42",
      "action": "select",
      "params": {"i": 0},
      "state": {"phase": "Idle", "selected": 0}
    }
  ]
}
```

The root and each event are closed objects. `initial` is the complete logical
state at tick 0. Every event is exactly one logical transition: ticks are the
canonical sequence `1..N`, `action` is either an exact Public Kernel action name
or the v1.1 stutter value `null`, and `state` is the complete resulting logical
state. Action parameters are exact; stutter requires the empty object `{}`.
State values use the ordinary Monitor/Public Kernel representation: enum member
strings, `null`/value for Option, objects for struct and complete Map values,
arrays for Set/Seq, and pair arrays for relations.

`timestamp` is optional, non-empty, opaque producer metadata. Replay ignores it;
it has no ordering, deadline, or formal-time meaning. Consumers use `tick` for
the logical transition order, including stutter steps.

## Observation-point correspondence

Let `S0` be the Monitor initial state and `O0` the reported `initial`; they must
be equal and `Monitor::current_violation` must be empty. For event `i`, replay
applies exactly one of these rules:

- **Action** `a(p)`: `Monitor::attempt(a, p)` must commit without a violation,
  producing `Si`, and the reported complete state `Oi` must equal `Si`.
- **Stutter**: no Monitor action is executed, `Si = S(i-1)`, the current
  violation must be empty, and `Oi` must equal that unchanged state.

Deleting or inserting any number of equal-state stutter observations therefore
preserves the projected action trace and final logical state. A string action
literally named `stutter` remains an ordinary action; only JSON `null` denotes
the stutter rule.

Invariants, bounds, and transition semantics are judged only on these reported
logical states and atomic Monitor action successors. Concrete implementation
states between two observations are absent from the trace and are not checked.
This permits an implementation to pass through a transient state that violates
an FSL invariant while implementing one atomic action, but reporting that same
state as an observation is nonconformant because it cannot equal the required
logical state. Replay does not infer or synthesize unreported intermediates.

Replay-trace v1 accepts `kernel_schema_version` `1.0.0` and `2.0.0`. Kernel v2
adds provenance but does not change action/state execution values. Trace schema
SemVer is independent: changing required fields, tick meaning, or value encoding
requires a trace major; adding support for a Kernel version without changing
trace values is additive.

Trace schema `1.0.0` accepts action events only. `1.1.0` additively permits
`action: null` with empty params for stutter observations. Replay reports the
input trace version in `trace_schema_version` and rejects null actions under
`1.0.0`. Schema `1.2.0` opts into the bounded-liveness semantics below; earlier
minor versions retain safety-only replay rather than changing meaning in place.

## Bounded liveness at observation points

For each `leadsTo L { P ~> within K Q }` and each supported static binder
assignment, replay 1.2 maintains the oldest unsatisfied trigger. At observation
`p`, `P` starts an obligation unless `Q` already holds. `Q` may satisfy that
obligation at any observation through the inclusive deadline `p + K`; if it is
still false there, replay exits 1 with `check:"bounded_liveness"`, `property`,
`bindings`, `pending_since`, `deadline`, `within`, and `tick`. This is the same
deadline rule as native BMC. Tick 0 and every action or stutter event are
observations, so stutters advance the bounded response clock.

Safety is checked before liveness at each observation. A state mismatch or
safety violation at a deadline is therefore reported as safety evidence rather
than being hidden by a liveness result. Successful replay reports separate
`checks.safety` and `checks.bounded_liveness` objects. A finite prefix ending
before a deadline reports liveness `status:"pending"`; this is not a proof or a
failure. Unbounded `leadsTo` declarations are listed in
`unbounded_properties` and remain unchecked.

The runtime monitor is solver-free and consumes the checked Kernel model. It
enumerates typed and static integer-range binders. Collection binders and
`where` filters fail closed instead of silently checking a weaker property.
Requirements `deadline` is already lowered to a safety invariant and stays in
the safety check; only explicit `leadsTo ... within K` uses this monitor.

## Validation and verdicts

The CLI selects the v1 parser when any reserved root marker (`$schema`, either
version, `spec`, or `initial`) is present. A malformed versioned object never
falls through to legacy parsing.

Schema/version/spec/tick/closed-shape errors and incomplete or ill-typed
parameters/state are input errors (exit 2). A well-typed initial observation
that differs from spec init is `initial_state_mismatch`; a well-typed post-state
divergence is `state_mismatch` with deterministic leaf `mismatches`. Rejected
actions and invariant/type/partial-operation failures keep their existing
nonconformant verdict (exit 1). `failed_at_event` remains zero-based; `tick` is
one-based. Conformance exits 0.

All versioned parameters and states are decoded before Monitor execution, so a
malformed later observation is always exit 2 even if an earlier logical step
would be nonconformant. Action execution uses the same `Monitor::attempt`
conformance entry point as the public harness; replay does not reimplement
guard, partial-operation, update, or rollback semantics.

Snapshots are decoded with the same complete typed state loader used by
`--from-state` and mapped log replay, then compared after canonical rendering.
Set/relation ordering and typed scalar conversion therefore do not create raw
JSON false mismatches.

## Compatibility and adjacent trace formats

The pre-v1 bare array and `{ "events": [...] }` action-only shapes remain an
explicit unversioned compatibility adapter. They retain current action/display
name matching and do not claim snapshot evidence. Migration consists of adding
the v1 root, recording complete init, numbering events, and recording every
complete post-state. No heuristic conversion or fallback is performed.

`testgen-trace.v1` is a fixed-seed generated-test oracle with `steps[].expected`;
verifier counterexample trace JSON carries source locations and changes. Neither
is replay-trace input. `--from-log` remains the separate refinement-mapping path
for external names and schemas.

## Fixtures and distribution

The action, state-mismatch, malformed-tick, stuttering, transient-observation,
and bounded-liveness goldens plus their FSL specs live under
`rust/fslc/tests/fixtures/replay_*` and `examples/nfr/bounded_response*`.
Release packaging copies the
schema and all fixtures into both independently checksummed Public Kernel v1
and v2 bundles together with this contract, so an external compiler can
implement and test the backward contract from release artifacts alone.
