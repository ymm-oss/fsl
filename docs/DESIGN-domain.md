# FSL Domain / Effect Dialect Design

Status: adopted MVP.

## Decision

`domain` is a frontend dialect for Functional DDD boundaries and async effect
lifecycles. It is not a new verifier kernel and not a programming language. The
MVP parses domain declarations into typed IR, lowers the checkable part to the
existing kernel, and emits stable fsl-domain findings for structural risks.

Implemented top-level shape:

```fsl
domain OrderDomain {
  implementation_profile functional_ddd

  type OrderStatus = Pending | Approved | Cancelled

  aggregate Order {
    id OrderId
    state { status: OrderStatus = Pending; }
    command ApproveOrder {}
    event OrderApproved {}
    error CannotApprove
    decide ApproveOrder {
      requires status == Pending
      emits OrderApproved
    }
    evolve OrderApproved {
      status = Approved
    }
    invariant noLateCancel { status == Cancelled -> not can(ApproveOrder) }
  }
}
```

The same dialect also accepts process-manager style coordination:

```fsl
saga OrderFulfillment {
  starts_on OrderApproved
  outbox OrderOutbox
  inbox FulfillmentInbox

  step ReserveInventory {
    async
    emits InventoryReservationRequested
    awaits one_of [InventoryReserved, InventoryReservationFailed]
    timeout after 5m emits InventoryReservationFailed
  }

  compensation {
    when PaymentFailed after InventoryReserved {
      emits InventoryReleaseRequested
    }
  }
}
```

## Lowering

Each aggregate becomes kernel state and actions:

- aggregate state field `Order.status` -> kernel state `order_status`
- command + decide + emitted event/evolve -> one kernel `action`
- aggregate invariant -> kernel `invariant`
- event occurrence -> per-step `event_<Event>` Bool flags
- saga step -> kernel action guarded by `starts_on`, `requires`, or awaited
  event flags
- saga compensation -> kernel action guarded by trigger/after event flags

Domain enum members are namespaced during lowering (`OrderStatus_Pending`) so
two domain enums can both contain `Pending`. Domain expressions stay in the
short source vocabulary; the expander rewrites comparisons and `in [A, B]`
membership according to the field type.

`can(Command)` is a domain-only expression helper. It lowers to that command's
decide preconditions: all `requires` clauses and the negation of every
`rejects ... when ...` condition.

## Effects

An async `effect` declares the request event, completion events, correlation id,
retry bound, timeout event, idempotency key, and optional reliable outbox/inbox
boundary. The MVP lowers the lifecycle to finite maps:

- `<effect>_status: Map<CorrelationId, EffectStatus>`
- `<effect>_attempts: Map<CorrelationId, Attempt>`
- completion actions require the request to be pending
- retry actions respect `max_attempts`
- successful effect status is sticky

The checker reports hard structural errors before running the kernel when an
async effect has no `correlation_id`, or an irreversible effect lacks an
`idempotency_key`.

Irreversible effects that lack compensation are reported as warnings. Reliable
effects without an outbox boundary are also warnings because they overstate
runtime delivery evidence.

## Commands

`fslc check` and `fslc verify` accept `domain` files because they lower to the
kernel. Domain-specific commands expose the dialect boundary:

```bash
fslc domain check examples/domain/order_async_effect.fsl
fslc domain analyze examples/domain/order_async_effect.fsl
fslc domain expand examples/domain/order_async_effect.fsl
fslc domain generate examples/domain/order_functional_ddd.fsl --target typescript -o src/domain
fslc domain generate examples/domain/order_functional_ddd.fsl --target python
fslc domain testgen examples/domain/order_functional_ddd.fsl --target vitest -o order.domain.test.ts
fslc domain replay examples/domain/order_async_effect.fsl --logs examples/domain/order_async_effect_replay.jsonl
```

Successful `domain check` returns `verified_under_assumptions` with the kernel
result nested under `kernel`. Hard structural findings return `violated` with
`formal_result:"not_run"`.

## Findings

Findings use `schemas/fslc/domain/finding.v0.schema.json` and
`fsl:"fsl-domain-effect-mvp.v0"`. Implemented finding kinds include:

- `missing_decide_for_command`
- `missing_evolve_for_event`
- `aggregate_boundary_violation`
- `uncorrelated_async_completion`
- `irreversible_effect_without_idempotency_key`
- `pending_effect_without_timeout_or_fallback`
- `late_completion_without_stale_policy`
- `missing_compensation_for_irreversible_effect`
- `reliable_effect_without_outbox_boundary`
- `saga_dead_end`
- `process_wait_cycle`
- runtime replay findings such as `command_rejected_by_model`,
  `uncorrelated_async_completion`, and
  `effect_completion_rejected_by_model`

`warning` findings are design review findings. They do not block the formal
kernel run. `error` findings block the run because the generated model would
otherwise overstate the guarantee.

## Generation

`fslc domain generate --target typescript` emits Functional DDD scaffolds:

- `types.ts`
- `<aggregate>/decide.ts`
- `<aggregate>/evolve.ts`
- `<aggregate>/adapter.ts`
- `effects.ts` when effects are declared
- `process-manager.ts` when sagas are declared

The command also supports `--target python`, `--target kotlin`, `--target
swift`, and `--target rust` as simple pure-domain scaffolds. TypeScript remains
the richest target in this release.

The generated code is a scaffold, not production architecture proof. It keeps
`decide` and `evolve` pure and gives the adapter boundary that existing
`testgen` conformance tests can be wired to.

## Runtime Replay

`fslc domain replay` accepts JSON arrays, `{"events":[...]}`, or JSONL. Runtime
events use these kinds:

- `command`
- `domain_event`
- `effect_request`
- `effect_completion`

Replay returns `conformance_checked` when the finite log matches the model and
`nonconformant` with fsl-domain findings when it observes a rejected command,
completion without request, duplicate irreversible completion, or lifecycle
ordering mismatch. This is runtime observation evidence, not a formal proof.

Saga `await` and compensation `after` clauses use per-step event observations in
the kernel model and add `DOMAIN-ASSUME-SAGA-HISTORY-MVP`. Durable process
history is checked through replay evidence rather than treated as an unbounded
kernel proof.

## Guarantee Boundary

The kernel can prove bounded aggregate invariants, rejected-command no-op shape
by construction, completion-requires-request in the modeled lifecycle, retry
bounds, and sticky success status. It does not prove external API correctness,
queue delivery, wall-clock time, production idempotency across unbounded keys, or
that generated code is production optimal. Those require runtime replay, adapter
tests, or external evidence.

## Future Work

Remaining work is production hardening rather than dialect absence: richer
history-aware saga state, stronger cross-aggregate routing proofs, production
outbox/inbox adapters, and fuller non-TypeScript generators should build on this
IR rather than adding a second semantics.
