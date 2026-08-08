# FSL Domain — correlated saga history

Status: accepted design for issue #662; implementation is a follow-up change.

## Problem and invariant

The current generated event flags are a one-step, one-hot observation. They
must remain useful as the event emitted by the current transition, but they
cannot also mean "this event occurred earlier for correlation k". A saga await
must never require two one-hot events to be true simultaneously, and history
for one correlation must not discharge another correlation's await.

## Measured candidates

The executable measurement
`issue_662_saga_history_measurement::candidate_state_costs_are_anchored_to_the_current_fixture`
uses the maintained effect+saga fixture: five event flags, two
`PaymentRequestId` correlations, seven existing effect phases, and currently
zero parameters on the generated saga action.

| Candidate | Added state projection for the fixture | Dispatch instances | Expressiveness / failure mode |
|---|---:|---:|---|
| 1. Add a correlation parameter and read the effect status | ×1 (reuses the existing status map) | 2 instead of 1 | Correct for a saga step owned by one effect; couples saga progress to effect-specific phases and does not compose multi-effect/compensation history |
| 2. Make five event flags sticky | 32 valuations versus the current one-hot 6 (5.33× event projection) | 1 | Still global: 10 correlation-labelled single-event histories collapse into 5 states, so concurrent sagas alias |
| 3. Add `Map<Correlation,SagaPhase>` | `6^2 = 36` saga-progress valuations | 2 | Represents independent concurrent progress, multi-step waits, terminal outcomes, and compensation |

The six accepted saga phases are `NotStarted`, `Awaiting`, `Succeeded`,
`Failed`, `TimedOut`, and `Compensating`. The measurement is a projection-size
comparison, not a claim that all Cartesian states are reachable; it deliberately
states the upper-bound cost that symbolic and explicit engines must carry.

## Decision

Choose candidate 3: explicit correlation-indexed saga state. Candidate 1 is an
attractive local minimum because it adds no state to the current single-effect
fixture, but it makes the saga abstraction depend on one effect's lifecycle and
cannot represent a multi-effect join or compensation phase without another
special case. Candidate 2 spends comparable state while remaining correlation
unsound.

Generated saga step/timeout/compensation actions will take the correlation key,
guard the relevant saga phase, and update that key's phase. Effect completion
remains the only writer of effect outcome state. One-hot `event_*` flags retain
their current-transition meaning; they are not repurposed as history.

## Implementation boundary

Issue #662 closes on this measured design decision. A follow-up issue owns the
language/lowering implementation, including:

- correlation source resolution and action parameters;
- the generated saga phase enum/map and initialization;
- step, timeout, outcome, and compensation transitions;
- concurrent-correlation positive controls and a cross-correlation negative
  control;
- concrete/symbolic agreement, dialect registry, LSP indexing where syntax
  changes, English/Japanese language documentation, skill reference, examples,
  and a `changelog.d/<id>-<slug>.<category>.md` fragment (see `changelog.d/README.md`)
  rather than a direct `CHANGELOG.md` edit.

Until that follow-up lands, `domain check` warnings for structurally disabled
saga actions remain valid evidence. They must not be suppressed or replaced by
sticky global flags.
