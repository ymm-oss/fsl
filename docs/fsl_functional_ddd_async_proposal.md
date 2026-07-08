# FSL Functional DDD / Async Effect Proposal

**Status:** Proposal  
**Target:** FSL dialect expansion / implementation projection / test adapter generation  
**Created:** 2026-07-08  
**Audience:** FSL maintainers, AI agents that generate FSL, application engineers using FSL-generated implementation scaffolds

---

## 1. Executive Summary

This proposal adds a domain implementation projection layer to FSL for **Functional DDD** and **asynchronous effect modeling**.

The goal is not to turn FSL into a programming language. The goal is to let FSL describe the domain responsibility boundary and asynchronous lifecycle strongly enough that AI agents can generate implementation scaffolds, test adapters, conformance tests, and structural review findings without guessing the essential design decisions.

The proposed extension consists of two related dialects:

```text
fsl-domain
  Functional DDD / domain transition dialect
  aggregate, command, event, error, decide, evolve, projection, consistency boundary

fsl-effect
  Async/effect lifecycle dialect
  effect, effect_handler, await, timeout, retry, cancellation, compensation,
  idempotency, correlation, outbox/inbox, eventual consistency
```

They should lower into the existing FSL shared kernel where possible:

```text
fsl-domain / fsl-effect
  ↓ dialect expansion
FSL kernel
  state / action / invariant / trans / leadsTo / scenario / refinement / compose
  ↓
fslc verify / fslc scenarios / Monitor / replay / testgen
  ↓
Generated Functional DDD scaffold + generated Adapter + conformance tests
```

The design intentionally separates:

```text
Domain decision
  pure decide/evolve logic

External side effect
  effect request / effect handler / async completion

Implementation projection
  TypeScript/Kotlin/Swift/Python functional DDD scaffold

Conformance boundary
  reset / step / observe adapter contract
```

The key design principle is:

> FSL should not prescribe object-oriented DDD or functional DDD directly. It should describe a domain transition model that can be projected into multiple implementation styles. Functional DDD should be a first-class implementation profile because it maps especially cleanly to FSL's state/action/invariant/transition model.

---

## 2. Current FSL Baseline

This proposal assumes the following FSL capabilities as the baseline:

1. FSL is designed as an AI-native formal specification language whose verifier returns machine-readable JSON for AI write → verify → repair loops.
2. FSL supports a shared kernel around bounded transition systems, invariants, reachability, `leadsTo`, refinement, composition, scenarios, Monitor, replay, and test generation.
3. FSL's test adapter model uses the `reset` / `step(action, params)` / `observe()` contract to connect a concrete implementation to a spec.
4. FSL has an architectural precedent for dialect frontends that expand into the shared kernel rather than modifying the kernel for every domain vocabulary.
5. Current docs already show adjacent dialect directions such as DB/multi-environment compatibility and AI hard-contract / recursive agent structure.

Design reference links:

- FSL README: https://github.com/ymm-oss/fsl
- Shared kernel / layered dialect design: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-layers.md
- Bridge / Monitor / replay / testgen design: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-bridge.md
- Transition invariant `trans` design: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-trans.md
- AI hard-contract / recursive agent design: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-ai-hard.md
- DB / multi-environment compatibility design: https://github.com/ymm-oss/fsl/blob/main/docs/DESIGN-db.md

---

## 3. Problem Statement

FSL can already generate conformance-test scaffolds from specifications. That is useful, but it does not by itself determine a good DDD implementation.

Given a spec such as:

```text
action cancelOrder
invariant noCancelAfterShipping
invariant refundAtMostOnce
```

there are many valid implementations:

```text
OO DDD:
  Order.cancel()
  RefundService.issueRefund()
  OrderRepository.save(order)

Functional DDD:
  decide(orderState, CancelOrder) -> [OrderCancelled]
  evolve(orderState, OrderCancelled) -> nextOrderState

Event-sourced Functional DDD:
  command handler emits events
  event store persists events
  state is rebuilt by replay

Saga / Process Manager:
  OrderCancelled triggers async RefundRequested
  RefundSucceeded / RefundFailed are handled later
```

FSL plus a test adapter can check whether an implementation conforms to the observable spec. It cannot, without additional design information, decide:

```text
which state belongs to which aggregate
which invariant is owned by which aggregate
which operation is a command
which domain fact is an event
which side effect is allowed inside the domain boundary
which transition is synchronous
which transition is eventual
which effect requires idempotency or compensation
```

This is precisely the gap this proposal addresses.

---

## 4. Design Goals

### 4.1 First-class Functional DDD projection

FSL should support generating functional DDD scaffolds based on:

```text
State
Command
Event
Error
Decide
Evolve
Projection
```

The generated implementation should be idiomatic for languages such as TypeScript, Kotlin, Swift, Python, or Rust.

### 4.2 Preserve implementation-style neutrality

FSL should not collapse DDD into object-oriented class design. The FSL domain layer should be neutral enough to project into:

```text
functional DDD
object-oriented DDD
event-sourced DDD
CQRS
actor/message-handler style
application-service + repository style
```

Functional DDD is a first-class profile, not the only possible profile.

### 4.3 Separate pure domain logic from effects

A generated functional DDD implementation should keep the domain core pure:

```text
decide : State × Command -> Result<Event list, DomainError>
evolve : State × Event -> State
```

External calls should be represented as effects:

```text
PaymentCaptureRequested
  -> handled by PaymentGatewayEffectHandler
  -> PaymentCaptured | PaymentFailed | PaymentTimedOut
```

### 4.4 Model asynchronous lifecycles explicitly

FSL needs a vocabulary for:

```text
requested
pending
completed
failed
timed_out
cancelled
compensated
retried
```

rather than hiding these behind a programming-language-level `await`.

### 4.5 Support AI implementation generation

The dialect should produce AI-readable artifacts:

```text
implementation plan
aggregate ownership map
command/event/error schema
code scaffold
test adapter scaffold
conformance tests
review findings
repair candidates
```

### 4.6 Keep formal guarantees honest

Pure domain transitions can lower into FSL kernel properties. External effects, runtime queues, webhooks, distributed delivery, and third-party APIs require assumptions or replay evidence. The result schema must distinguish:

```text
proved
verified_under_assumptions
runtime_observed
conformance_checked
not_formally_proved
```

---

## 5. Core Concepts

## 5.1 `domain`

A `domain` groups aggregates, commands, events, effects, policies, projections, and implementation profiles.

```fsl
domain OrderDomain {
  implementation_profile functional_ddd

  aggregate Order { ... }
  effect CapturePayment { ... }
  saga OrderFulfillment { ... }
}
```

A `domain` is not necessarily a bounded context, but it can represent one when the user wants that granularity.

---

## 5.2 `implementation_profile`

The implementation profile tells code generation how to project the specification.

```fsl
implementation_profile FunctionalDddProfile {
  architecture ddd
  style functional
  command_model decide_evolve
  persistence event_sourced
  adapter_target vitest
}
```

Possible styles:

```text
functional
object_oriented
event_sourced_functional
cqrs
actor_model
repository_service
```

The proposal focuses on:

```text
style functional
command_model decide_evolve
```

---

## 5.3 `aggregate`

An aggregate is a consistency boundary, not necessarily a class.

```fsl
aggregate Order {
  id OrderId

  state {
    status: OrderStatus
    payment_status: PaymentStatus
    refund_issued: Bool
    total: Money
  }

  command ApproveOrder { input approved_by: UserId }
  command CancelOrder { input reason: CancelReason }
  command ShipOrder {}

  event OrderApproved { approved_by: UserId }
  event OrderCancelled { reason: CancelReason }
  event OrderShipped {}

  error CannotCancelShippedOrder
  error CannotShipUnapprovedOrder

  decide ApproveOrder { ... }
  decide CancelOrder { ... }
  decide ShipOrder { ... }

  evolve OrderApproved { ... }
  evolve OrderCancelled { ... }
  evolve OrderShipped { ... }

  invariant noCancelAfterShipping { ... }
}
```

The aggregate owns:

```text
state
commands
events
errors
invariants
transition rules
```

---

## 5.4 `command`

A command represents intent.

```fsl
command CancelOrder {
  input order_id: OrderId
  input reason: CancelReason
}
```

A command is not a domain fact. It may be rejected.

Generated functional projection:

```ts
type OrderCommand =
  | { type: "CancelOrder"; orderId: OrderId; reason: CancelReason }
```

---

## 5.5 `event`

An event represents a domain fact accepted by the aggregate.

```fsl
event OrderCancelled {
  order_id: OrderId
  reason: CancelReason
}
```

Generated functional projection:

```ts
type OrderEvent =
  | { type: "OrderCancelled"; orderId: OrderId; reason: CancelReason }
```

---

## 5.6 `error`

A domain error represents rejected intent.

```fsl
error CannotCancelShippedOrder
error CannotShipUnapprovedOrder
```

Generated functional projection:

```ts
type OrderError =
  | { type: "CannotCancelShippedOrder" }
  | { type: "CannotShipUnapprovedOrder" }
```

---

## 5.7 `decide`

`decide` maps current state and command to either events or errors.

```fsl
decide CancelOrder {
  rejects CannotCancelShippedOrder when status == Shipped
  requires status in [Pending, Approved]
  emits OrderCancelled
}
```

Functional projection:

```text
decideOrder : OrderState -> OrderCommand -> Result<OrderEvent list, OrderError>
```

Important rule:

```text
decide must be pure.
```

It cannot directly call payment gateways, send emails, mutate databases, call AI tools, or perform network I/O. It can emit an event or an effect request event.

---

## 5.8 `evolve`

`evolve` maps current state and event to the next state.

```fsl
evolve OrderCancelled {
  status = Cancelled
}
```

Functional projection:

```text
evolveOrder : OrderState -> OrderEvent -> OrderState
```

Important rule:

```text
evolve must be pure.
```

---

## 5.9 `projection`

A projection describes a read model or observable state derived from domain state or events.

```fsl
projection OrderSummary {
  from Order
  fields [status, payment_status, total]
}
```

This supports:

```text
CQRS read models
FSL observe() mapping
runtime conformance state projection
UI/API response schema generation
```

---

## 5.10 `effect`

An effect models an external side effect or asynchronous interaction.

```fsl
effect CapturePayment {
  async
  irreversible
  idempotency_key order_id
  correlation_id payment_request_id

  input order_id: OrderId
  input amount: Money

  request_event PaymentCaptureRequested
  success_event PaymentCaptured
  failure_event PaymentFailed
  timeout_event PaymentCaptureTimedOut

  retry {
    max_attempts 3
    backoff exponential
  }

  compensation {
    emits PaymentVoidRequested
  }
}
```

Effects include:

```text
external API calls
queue messages
webhook waits
email sending
AI tool calls
file generation
DB migrations/backfills
mobile offline sync
```

---

## 5.11 `effect_handler`

An effect handler maps an effect request to a set of possible observed outcomes.

```fsl
effect_handler PaymentGatewayHandler {
  handles PaymentCaptureRequested

  emits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]

  assumptions {
    eventual_response under weak_fairness
    duplicate_delivery_allowed
    out_of_order_delivery_allowed
  }
}
```

The handler is not necessarily formally proved. It may be:

```text
modeled symbolically
checked by runtime replay
tested with generated adapters
mocked in generated tests
connected to real implementation logs
```

---

## 5.12 `await`

`await` describes a workflow dependency on effect completion. It is not a programming-language `await`; it is a domain/process waiting condition.

```fsl
await PaymentResult {
  waits_for one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]

  on PaymentCaptured -> PaymentConfirmed
  on PaymentFailed -> PaymentRejected
  on PaymentCaptureTimedOut -> PaymentReviewRequired
}
```

Supported forms:

```text
await all [A, B]
await any [A, B]
await one_of [A, B, C]
await until condition
await with timeout
eventual await
continue_without_waiting
```

---

## 5.13 `saga` / `process_manager`

A saga coordinates asynchronous domain steps across aggregate boundaries.

```fsl
saga OrderFulfillment {
  starts_on OrderApproved

  step ReserveInventory {
    async emits InventoryReservationRequested
    awaits one_of [InventoryReserved, InventoryReservationFailed]
  }

  step CapturePayment {
    async emits PaymentCaptureRequested
    awaits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
  }

  step ShipOrder {
    requires InventoryReserved
    requires PaymentCaptured
    emits ShipmentRequested
  }

  compensation {
    when PaymentFailed after InventoryReserved {
      emits InventoryReleaseRequested
    }
  }

  invariant noShipmentWithoutPaymentAndInventory {
    ShipmentRequested -> PaymentCaptured && InventoryReserved
  }
}
```

A saga is not an aggregate. It coordinates eventual consistency and compensation across boundaries.

---

## 6. Proposed Syntax Sketch

```fsl
domain OrderDomain {
  implementation_profile functional_ddd

  type OrderStatus = Pending | Approved | Cancelled | Shipped
  type PaymentStatus = NotRequested | Pending | Captured | Failed | TimedOut

  value_object Money {
    amount: Int
    currency: Currency
    invariant nonNegative { amount >= 0 }
  }

  aggregate Order {
    id OrderId

    state {
      status: OrderStatus
      payment_status: PaymentStatus
      refund_issued: Bool
      total: Money
    }

    command ApproveOrder {
      input approved_by: UserId
    }

    command CancelOrder {
      input reason: CancelReason
    }

    command RequestPaymentCapture {
      input payment_request_id: PaymentRequestId
    }

    event OrderApproved {
      approved_by: UserId
    }

    event OrderCancelled {
      reason: CancelReason
    }

    event PaymentCaptureRequested {
      payment_request_id: PaymentRequestId
      amount: Money
    }

    event PaymentCaptured {
      payment_request_id: PaymentRequestId
    }

    event PaymentFailed {
      payment_request_id: PaymentRequestId
    }

    event PaymentCaptureTimedOut {
      payment_request_id: PaymentRequestId
    }

    error CannotCancelShippedOrder
    error CannotCaptureUnapprovedOrder
    error DuplicatePaymentCapture

    decide ApproveOrder {
      requires status == Pending
      emits OrderApproved
    }

    decide CancelOrder {
      rejects CannotCancelShippedOrder when status == Shipped
      requires status in [Pending, Approved]
      emits OrderCancelled
    }

    decide RequestPaymentCapture {
      rejects CannotCaptureUnapprovedOrder when status != Approved
      rejects DuplicatePaymentCapture when payment_status in [Pending, Captured]
      emits PaymentCaptureRequested
    }

    evolve OrderApproved {
      status = Approved
    }

    evolve OrderCancelled {
      status = Cancelled
    }

    evolve PaymentCaptureRequested {
      payment_status = Pending
    }

    evolve PaymentCaptured {
      payment_status = Captured
    }

    evolve PaymentFailed {
      payment_status = Failed
    }

    evolve PaymentCaptureTimedOut {
      payment_status = TimedOut
    }

    invariant noCancelAfterShipping {
      status == Shipped -> not can(CancelOrder)
    }

    invariant noDuplicateCapture {
      payment_status == Captured -> not can(RequestPaymentCapture)
    }
  }

  effect CapturePayment {
    async
    irreversible
    idempotency_key Order.id
    correlation_id PaymentCaptureRequested.payment_request_id

    handles PaymentCaptureRequested
    emits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]

    retry {
      max_attempts 3
      backoff exponential
    }

    timeout after 10m emits PaymentCaptureTimedOut
  }

  saga OrderFulfillment {
    starts_on OrderApproved

    step CapturePaymentStep {
      emits PaymentCaptureRequested
      awaits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
    }

    step ShipOrder {
      requires PaymentCaptured
      emits ShipmentRequested
    }

    compensation {
      when PaymentFailed after OrderApproved {
        emits OrderPaymentFailedNotificationRequested
      }
    }
  }

  projection OrderObservedState {
    from Order
    fields [status, payment_status, refund_issued, total]
  }
}
```

---

## 7. Semantics

## 7.1 Functional DDD semantics

For each aggregate:

```text
Aggregate A =
  State_A
  Command_A
  Event_A
  Error_A
  decide_A
  evolve_A
  invariants_A
```

The intended denotation is:

```text
decide_A : State_A × Command_A -> Result<List<Event_A>, Error_A>
evolve_A : State_A × Event_A -> State_A
apply_A  : State_A × List<Event_A> -> State_A
```

Generated rule:

```text
For every command C:
  if decide(state, C) emits events E1..En,
  then state' = evolve(...evolve(evolve(state, E1), E2)..., En)
  and every aggregate invariant must hold in state'.
```

Invalid command behavior:

```text
If a command is rejected, no domain event is emitted and aggregate state is unchanged.
```

This maps naturally to FSL `action` and `ensures`.

---

## 7.2 Effect semantics

An effect creates a request and later receives an outcome.

```text
NotStarted
  -> Requested
  -> Pending
  -> Completed | Failed | TimedOut | Cancelled
  -> Compensated, if applicable
```

Each effect instance is identified by:

```text
correlation_id
idempotency_key
attempt_no
status
requested_at logical time
completed_at logical time, if completed
```

Effect outcomes may be:

```text
delayed
duplicated
out-of-order
missing until timeout
```

The FSL model should not assume exactly-once delivery unless explicitly declared and justified.

---

## 7.3 Await semantics

`await` is a process-level constraint over event occurrence.

```text
await all [A, B]
  The process cannot enter the dependent step until both A and B have occurred.

await any [A, B]
  The process can continue when at least one has occurred.

await one_of [A, B, C]
  Exactly one terminal branch is selected, unless duplicate/out-of-order delivery is modeled.

await until P timeout T -> Q
  Wait until P, but if P does not hold by T, emit or transition to Q.
```

Potential kernel lowering:

```text
state awaiting_X: Bool
state received_A: Bool
state received_B: Bool
action receive_A
action receive_B
action continue_after_X requires received_A && received_B
invariant no_continue_before_await_condition
leadsTo awaiting_X -> completed_X under fairness assumption
```

---

## 7.4 Idempotency semantics

Idempotency is represented as a transition safety property:

```fsl
trans PaymentCaptureAtMostOnce {
  old(payment_status) == Captured => payment_status == Captured
}
```

For effect instances:

```text
For the same idempotency_key, at most one irreversible success effect may be committed.
```

Generated finding when violated:

```text
non_idempotent_irreversible_effect
```

---

## 7.5 Correlation semantics

Every asynchronous completion must match an existing request.

```text
PaymentCaptured(payment_request_id = X)
requires exists PaymentCaptureRequested(payment_request_id = X)
```

Generated finding when violated:

```text
uncorrelated_async_completion
```

---

## 7.6 Stale completion semantics

An asynchronous result may arrive after the aggregate has moved to a state in which the result should no longer be applied.

Example:

```text
Payment request issued
Order cancelled
PaymentCaptured arrives late
```

FSL should allow guards such as:

```fsl
evolve PaymentCaptured {
  requires status != Cancelled
  payment_status = Captured
}
```

or explicit stale handling:

```fsl
on_stale PaymentCaptured when status == Cancelled {
  emits PaymentVoidRequested
}
```

Generated finding when no stale handling exists:

```text
late_completion_without_stale_policy
```

---

## 8. Kernel Expansion Strategy

## 8.1 Aggregate lowering

Each aggregate lowers to FSL kernel state and actions.

Conceptual lowering:

```text
aggregate Order.state.status
  -> state order_status: OrderStatus

command CancelOrder + decide + evolve
  -> action order_cancel
     requires decide preconditions
     simultaneous assignments from evolve events
     ensures emitted event metadata if modeled

invariant noCancelAfterShipping
  -> invariant noCancelAfterShipping
```

If event sourcing is enabled:

```text
event log modeled as Seq<OrderEvent> or bounded Set/Map
state can be either materialized or derived by replay abstraction
```

MVP should use materialized state plus generated event flags, not full unbounded event replay.

---

## 8.2 Event lowering

For each event:

```text
event flag for occurrence in current step
optional event log append if bounded log modeling is enabled
```

Example:

```text
state event_OrderCancelled: Bool
state event_PaymentCaptured: Bool
```

or:

```text
state events: Seq<DomainEvent>
```

MVP recommendation:

```text
Use per-step event flags for verification.
Generate real event union types for code.
```

---

## 8.3 Effect lowering

For each async effect:

```text
state effect_status: Map<CorrelationId, EffectStatus>
state effect_attempts: Map<CorrelationId, Int>
state effect_committed: Map<IdempotencyKey, Bool>
```

Generated actions:

```text
request_<Effect>
complete_<Effect>_success
complete_<Effect>_failure
timeout_<Effect>
cancel_<Effect>
retry_<Effect>
compensate_<Effect>
```

Generated properties:

```text
completion_requires_request
idempotency_at_most_once
irreversible_requires_guard_or_approval
no_continue_before_await
retry_bound_not_exceeded
compensation_available_for_failed_irreversible_path
```

---

## 8.4 Await lowering

For each await block:

```text
state await_X_status: AwaitStatus
```

Possible values:

```text
NotWaiting | Waiting | Satisfied | TimedOut | Cancelled
```

Generated transitions:

```text
start_await_X
mark_await_X_satisfied
continue_after_await_X
timeout_await_X
```

Generated invariant:

```text
continue_after_await_X -> await_X_status == Satisfied
```

Generated liveness, optional:

```text
leadsTo await_X_status == Waiting -> await_X_status in [Satisfied, TimedOut]
```

This should carry an explicit fairness/timeout assumption.

---

## 9. Generated Functional DDD Scaffold

Given the `Order` aggregate above, a TypeScript projection could generate:

```ts
type OrderState = {
  status: OrderStatus
  paymentStatus: PaymentStatus
  refundIssued: boolean
  total: Money
}

type OrderCommand =
  | { type: "ApproveOrder"; approvedBy: UserId }
  | { type: "CancelOrder"; reason: CancelReason }
  | { type: "RequestPaymentCapture"; paymentRequestId: PaymentRequestId }

type OrderEvent =
  | { type: "OrderApproved"; approvedBy: UserId }
  | { type: "OrderCancelled"; reason: CancelReason }
  | { type: "PaymentCaptureRequested"; paymentRequestId: PaymentRequestId; amount: Money }
  | { type: "PaymentCaptured"; paymentRequestId: PaymentRequestId }
  | { type: "PaymentFailed"; paymentRequestId: PaymentRequestId }
  | { type: "PaymentCaptureTimedOut"; paymentRequestId: PaymentRequestId }

type OrderError =
  | { type: "CannotCancelShippedOrder" }
  | { type: "CannotCaptureUnapprovedOrder" }
  | { type: "DuplicatePaymentCapture" }

function decideOrder(
  state: OrderState,
  command: OrderCommand
): Result<OrderEvent[], OrderError> {
  switch (command.type) {
    case "CancelOrder":
      if (state.status === "Shipped") {
        return err({ type: "CannotCancelShippedOrder" })
      }
      if (!(state.status === "Pending" || state.status === "Approved")) {
        return err({ type: "CannotCancelShippedOrder" })
      }
      return ok([{ type: "OrderCancelled", reason: command.reason }])
  }
}

function evolveOrder(state: OrderState, event: OrderEvent): OrderState {
  switch (event.type) {
    case "OrderCancelled":
      return { ...state, status: "Cancelled" }
  }
}
```

Effect handler interface:

```ts
interface CapturePaymentHandler {
  handle(event: PaymentCaptureRequested): Promise<
    | PaymentCaptured
    | PaymentFailed
    | PaymentCaptureTimedOut
  >
}
```

Process manager interface:

```ts
interface OrderFulfillmentProcessManager {
  on(event: OrderEvent): EffectRequest[]
  onEffectResult(event: OrderEvent): OrderCommand[]
}
```

---

## 10. Generated Test Adapter

For functional DDD, the adapter is straightforward:

```ts
class OrderFslAdapter implements Adapter {
  private state: OrderState

  reset() {
    this.state = initialOrderState()
  }

  step(action: string, params: Record<string, unknown>) {
    const command = mapFslActionToCommand(action, params)
    const result = decideOrder(this.state, command)

    if (result.ok) {
      this.state = result.value.reduce(evolveOrder, this.state)
    }

    return result
  }

  observe() {
    return mapOrderStateToFslObservedState(this.state)
  }
}
```

For async effects, the adapter needs an event/effect harness:

```ts
class AsyncOrderFslAdapter implements Adapter {
  private state: OrderState
  private pendingEffects: EffectRequest[] = []

  reset() { ... }

  step(action: string, params: Record<string, unknown>) {
    if (isCommandAction(action)) {
      const command = mapFslActionToCommand(action, params)
      const result = decideOrder(this.state, command)
      if (result.ok) {
        for (const event of result.value) {
          this.state = evolveOrder(this.state, event)
          this.pendingEffects.push(...processManager.on(event))
        }
      }
      return result
    }

    if (isEffectCompletionAction(action)) {
      const event = mapFslActionToEffectResult(action, params)
      this.state = evolveOrder(this.state, event)
      return ok(event)
    }
  }

  observe() { ... }
}
```

The generated adapter should distinguish:

```text
command action
effect request
effect completion
timeout
compensation
```

---

## 11. Verification Rules

## 11.1 Functional DDD rules

### `decide_is_pure`

`decide` cannot call effects, tools, repositories, network, clocks, random, or mutable external state.

Finding:

```text
impure_decide_logic
```

### `evolve_is_pure`

`evolve` cannot reject, perform I/O, call effects, or branch on external state.

Finding:

```text
impure_evolve_logic
```

### `event_preserves_invariants`

Every emitted event sequence must leave the aggregate satisfying its invariants.

Finding:

```text
event_breaks_aggregate_invariant
```

### `command_rejection_does_not_mutate_state`

Rejected commands must not mutate aggregate state.

Finding:

```text
rejected_command_mutates_state
```

### `invariant_owned_by_consistency_boundary`

An invariant must be owned by an aggregate, saga, effect, or environment boundary. Unowned invariants lead to ambiguous implementation placement.

Finding:

```text
unowned_domain_invariant
```

### `aggregate_does_not_modify_foreign_state`

A command in aggregate A cannot directly mutate aggregate B.

Finding:

```text
aggregate_boundary_violation
```

### `cross_aggregate_change_requires_event_or_saga`

If a transition needs another aggregate to change, it must emit an event or use a saga/process manager.

Finding:

```text
cross_aggregate_update_without_event
```

---

## 11.2 Async/effect rules

### `completion_requires_request`

A success/failure/timeout completion cannot occur unless the corresponding request exists.

Finding:

```text
uncorrelated_async_completion
```

### `idempotency_for_irreversible_effect`

Irreversible effects must declare an idempotency key.

Finding:

```text
irreversible_effect_without_idempotency_key
```

### `at_most_once_commit`

For each idempotency key, at most one irreversible success is committed.

Finding:

```text
non_idempotent_irreversible_effect
```

### `await_guard_before_dependent_step`

A dependent step cannot proceed before the await condition is satisfied.

Finding:

```text
async_step_before_await_satisfied
```

### `timeout_or_fallback_for_pending_effect`

Async effects should have either an explicit timeout, retry bound, fallback, or liveness assumption.

Finding:

```text
pending_effect_without_timeout_or_fallback
```

### `retry_bound_respected`

Retry attempts cannot exceed the declared max attempts.

Finding:

```text
retry_bound_exceeded
```

### `late_completion_policy`

An effect completion that can arrive after cancellation, rollback, or state transition must have a stale-result policy.

Finding:

```text
late_completion_without_stale_policy
```

### `compensation_for_failed_irreversible_path`

If a saga performs an irreversible step and later fails, a compensation or explicit irreversible acceptance must be declared.

Finding:

```text
missing_compensation_for_irreversible_effect
```

### `outbox_for_effect_request`

If an aggregate emits an effect request that must be delivered reliably, the projection should declare an outbox or equivalent delivery mechanism.

Finding:

```text
reliable_effect_without_outbox_boundary
```

---

## 12. Finding JSON Schema Sketch

```json
{
  "fsl": "fsl-domain-effect-proposal.v0",
  "result": "violated",
  "kind": "late_completion_without_stale_policy",
  "severity": "error",
  "domain": "OrderDomain",
  "aggregate": "Order",
  "effect": "CapturePayment",
  "correlation_id": "PaymentCaptureRequested.payment_request_id",
  "failed_rule": "late_completion_policy",
  "guarantee_kind": "kernel_safety",
  "witness": [
    "OrderApproved",
    "PaymentCaptureRequested(payment_request_id=p1)",
    "OrderCancelled",
    "PaymentCaptured(payment_request_id=p1)",
    "payment_status becomes Captured while order status is Cancelled"
  ],
  "minimal_conflict_set": [
    "evolve PaymentCaptured",
    "decide CancelOrder",
    "effect CapturePayment"
  ],
  "repair_candidates": [
    "add stale policy for PaymentCaptured when status == Cancelled",
    "emit PaymentVoidRequested on late PaymentCaptured",
    "forbid CancelOrder while payment_status == Pending",
    "add saga compensation path after late capture"
  ],
  "assumptions": [
    "ASYNC-ASSUME-OUT_OF_ORDER_COMPLETION_ALLOWED",
    "ASYNC-ASSUME-EFFECT_HANDLER_CAN_COMPLETE_AFTER_CANCEL"
  ]
}
```

---

## 13. CLI Proposal

```bash
# Check domain dialect syntax and structural rules
fslc domain check order_domain.fsl

# Expand fsl-domain / fsl-effect to kernel FSL
fslc domain expand order_domain.fsl --out order_domain.expanded.fsl

# Verify generated transition model
fslc domain verify order_domain.fsl

# Generate functional DDD scaffold
fslc domain generate order_domain.fsl --profile functional-ddd --target typescript --out src/domain

# Generate adapter scaffold and conformance tests
fslc domain testgen order_domain.fsl --target vitest --out test/order.conformance.test.ts

# Replay runtime events / command logs / effect completions
fslc domain replay order_domain.fsl --logs events.jsonl

# Analyze domain boundaries and async risks
fslc domain analyze order_domain.fsl --findings json
```

Alternative integration with existing commands:

```bash
fslc check order_domain.fsl
fslc verify order_domain.fsl
fslc scenarios order_domain.fsl
fslc testgen order_domain.fsl --profile functional-ddd
```

---

## 14. Relationship to Existing FSL Features

## 14.1 `state` / `action`

Functional DDD `command + decide + evolve` lowers to kernel `action`.

```text
Command
  -> action parameters

Decide preconditions
  -> requires / forbidden / rejects

Evolve assignments
  -> action body / ensures
```

---

## 14.2 `invariant`

Aggregate invariants lower directly to kernel invariants.

```text
aggregate Order invariant noCancelAfterShipping
  -> invariant Order_noCancelAfterShipping
```

---

## 14.3 `trans`

Transition invariants are useful for sticky states and two-state async rules.

Examples:

```fsl
trans CapturedIsSticky {
  old(payment_status) == Captured => payment_status == Captured
}

trans NoStateMutationOnRejectedCommand {
  old(last_command_rejected) => order_state == old(order_state)
}
```

---

## 14.4 `leadsTo`

`leadsTo` can express liveness under explicit assumptions.

```fsl
leadsTo PaymentRequestEventuallyTerminates {
  payment_status == Pending leadsTo payment_status in [Captured, Failed, TimedOut]
}
```

This must carry fairness/timeout assumptions. It should not be shown as a real-world guarantee unless the effect handler and infrastructure provide evidence.

---

## 14.5 `scenarios`

Scenarios become domain command/effect traces.

```text
ApproveOrder
RequestPaymentCapture
PaymentCaptured
ShipOrder
```

Generated tests replay those steps through the functional DDD adapter.

---

## 14.6 Monitor / replay

Runtime replay should accept event streams such as:

```json
{"event":"command","aggregate":"Order","command":"ApproveOrder","params":{"approved_by":"u1"}}
{"event":"domain_event","aggregate":"Order","event":"OrderApproved"}
{"event":"effect_request","effect":"CapturePayment","correlation_id":"p1"}
{"event":"effect_completion","effect":"CapturePayment","event":"PaymentCaptured","correlation_id":"p1"}
```

Replay can detect:

```text
command accepted when spec rejects it
domain event not allowed by decide/evolve
effect completion without request
duplicate irreversible effect commit
missing compensation
async step before await condition
```

---

## 15. Relationship to AI Agent and DB/Multi-Environment Proposals

## 15.1 AI agent effects

An AI tool call is an effect.

```fsl
effect CreatePullRequest {
  async
  irreversible false
  idempotency_key branch_name
  handles PullRequestRequested
  emits one_of [PullRequestCreated, PullRequestFailed]
}
```

Nested agents can delegate work asynchronously:

```fsl
agent ImplementationOrchestrator {
  agent ResearchAgent { ... }
  agent CodeAgent { ... }
  agent ReviewAgent { ... }

  orchestration {
    async ResearchAgent.run(TaskContext) -> ResearchResult
    async CodeAgent.run(TaskContext) -> CodePatch

    await all [ResearchResult, CodePatch]
      then ReviewAgent.review(CodePatch, ResearchResult)
  }
}
```

This proposal gives `fsl-ai` a consistent lower-level async effect model.

## 15.2 DB migration effects

A DB backfill is an async effect.

```fsl
effect BackfillNormalizedEmail {
  async
  idempotency_key migration_id
  handles BackfillRequested
  emits one_of [BackfillCompleted, BackfillFailed, BackfillTimedOut]
}
```

Then:

```fsl
migration RequireNormalizedEmail {
  requires completed BackfillNormalizedEmail
  alter column users.email_normalized set not_null
}
```

This proposal gives `fsl-db` a general async lifecycle model for backfills, online index creation, dual-write windows, and delayed mobile sync.

---

## 16. AI Generation Workflow

Recommended workflow:

```text
1. AI writes business / requirements FSL
2. AI refines into design FSL
3. AI adds fsl-domain aggregate/command/event boundaries
4. fslc domain check/analyze returns boundary findings
5. AI repairs aggregate ownership, command/event semantics, and effect policies
6. fslc domain verify expands to kernel and proves safety properties where possible
7. AI generates functional DDD scaffold
8. AI generates adapter and conformance tests
9. Human reviews design-boundary questions
10. Runtime replay checks implementation/event logs against FSL
```

Important: AI should not silently pick aggregate boundaries when multiple plausible designs exist. It should emit design candidates:

```json
{
  "kind": "ddd_design_candidate",
  "aggregate": "Order",
  "owned_state": ["status", "payment_status", "refund_issued", "total"],
  "owned_invariants": ["noCancelAfterShipping", "noDuplicateCapture"],
  "commands": ["ApproveOrder", "CancelOrder", "RequestPaymentCapture"],
  "confidence": 0.82,
  "alternatives": [
    {
      "aggregate": "Payment",
      "reason": "payment_status may belong to a separate Payment aggregate if capture lifecycle is independently owned"
    }
  ],
  "human_review_required": [
    "Should payment_status be inside Order or Payment?",
    "Is payment capture synchronous from the business perspective or eventual?"
  ]
}
```

---

## 17. MVP Scope

## Phase 1: Functional DDD core

Implement:

```text
domain
implementation_profile functional_ddd
aggregate
state
command
event
error
decide
evolve
invariant ownership
projection for observe()
TypeScript scaffold generation
Adapter scaffold generation
kernel expansion for command/evolve/invariants
```

Static findings:

```text
unowned_domain_invariant
aggregate_boundary_violation
missing_decide_for_command
missing_evolve_for_event
event_breaks_aggregate_invariant
rejected_command_mutates_state
```

## Phase 2: Async effect core

Implement:

```text
effect
async effect status lifecycle
correlation_id
idempotency_key
success/failure/timeout events
retry bound
await one_of / all / any
basic compensation declaration
```

Findings:

```text
uncorrelated_async_completion
irreversible_effect_without_idempotency_key
non_idempotent_irreversible_effect
async_step_before_await_satisfied
pending_effect_without_timeout_or_fallback
late_completion_without_stale_policy
```

## Phase 3: Saga/process manager

Implement:

```text
saga
step
starts_on
await with timeout
compensation block
cross-aggregate event routing
outbox/inbox annotations
```

Findings:

```text
cross_aggregate_update_without_event
missing_compensation_for_irreversible_effect
reliable_effect_without_outbox_boundary
saga_dead_end
process_wait_cycle
```

## Phase 4: Multi-target implementation generation

Implement generators for:

```text
TypeScript
Kotlin
Swift
Python
Rust, optional later
```

Profiles:

```text
functional_ddd
event_sourced_functional_ddd
cqrs_functional
```

## Phase 5: Runtime replay and evidence boundary

Implement:

```text
command/event/effect JSONL replay
correlation checking
idempotency checking
stale completion detection
conformance reports
AI-readable findings
```

---

## 18. Out of Scope for MVP

The MVP should not try to fully solve:

```text
distributed transaction proof
real queue delivery guarantees
real payment gateway correctness
full event-sourcing log compaction semantics
unbounded event replay verification
arbitrary temporal logic over distributed traces
automatic perfect aggregate boundary discovery
full code generation for production infrastructure
```

These should be handled as assumptions, runtime evidence, or later extensions.

---

## 19. Guarantee Boundary

| Area | Can be proved in kernel? | Guarantee kind |
|---|---:|---|
| Aggregate invariant after command/evolve | Yes, if bounded model is adequate | `kernel_safety` |
| Rejected command does not mutate modeled state | Yes | `kernel_safety` |
| No cross-aggregate direct mutation | Static / structural | `structural` |
| Completion requires request | Yes, in modeled lifecycle | `kernel_safety` |
| Idempotency at most once | Yes, in modeled lifecycle | `kernel_safety` |
| External API actually returns | No | `assumption` / `runtime_observed` |
| Queue exactly-once delivery | No, unless modeled as assumption | `assumption` |
| Runtime implementation conforms | Replay/test evidence | `conformance_checked` |
| Generated code is production-optimal | No | `not_formally_proved` |

Result vocabulary should avoid overstating guarantees.

---

## 20. Design Principles

1. **FSL describes domain transitions, not classes.**  
   Classes, functions, repositories, handlers, and modules are projections.

2. **Aggregate means consistency boundary.**  
   It does not imply object-oriented mutable entity design.

3. **Command is intent; event is fact.**  
   Commands can be rejected. Events are accepted history or transition facts.

4. **`decide` and `evolve` are pure.**  
   External effects must be explicit.

5. **Async is an effect lifecycle, not syntax sugar.**  
   The model must expose pending, completion, failure, timeout, retry, cancellation, and compensation.

6. **Idempotency and correlation are mandatory for irreversible async effects.**  
   Duplicate and out-of-order delivery are normal, not exceptional.

7. **Await is a process constraint.**  
   It should lower to explicit state and transition guards.

8. **Generated code is a scaffold, not a final architecture proof.**  
   FSL can strongly constrain implementation, but human review remains necessary for aggregate boundaries and infrastructure choices.

---

## 21. Example End-to-End Flow

### Input FSL domain spec

```fsl
domain OrderDomain {
  implementation_profile functional_ddd

  aggregate Order {
    state { status: OrderStatus; payment_status: PaymentStatus }
    command ApproveOrder {}
    command RequestPaymentCapture { input payment_request_id: PaymentRequestId }
    event OrderApproved {}
    event PaymentCaptureRequested { payment_request_id: PaymentRequestId }
    event PaymentCaptured { payment_request_id: PaymentRequestId }
    error CannotCaptureUnapprovedOrder

    decide ApproveOrder {
      requires status == Pending
      emits OrderApproved
    }

    decide RequestPaymentCapture {
      rejects CannotCaptureUnapprovedOrder when status != Approved
      emits PaymentCaptureRequested
    }

    evolve OrderApproved { status = Approved }
    evolve PaymentCaptureRequested { payment_status = Pending }
    evolve PaymentCaptured { payment_status = Captured }
  }

  effect CapturePayment {
    async
    irreversible
    idempotency_key Order.id
    correlation_id PaymentCaptureRequested.payment_request_id
    handles PaymentCaptureRequested
    emits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
    timeout after 10m emits PaymentCaptureTimedOut
  }
}
```

### Generated artifacts

```text
src/domain/order/types.ts
src/domain/order/decide.ts
src/domain/order/evolve.ts
src/domain/order/effects.ts
src/domain/order/process-manager.ts
test/order.fsl-adapter.ts
test/order.conformance.test.ts
fsl-expanded/order_domain.expanded.fsl
```

### Generated checks

```text
Order cannot capture payment before approval.
Payment completion cannot occur without payment request.
Duplicate payment capture is blocked by idempotency key.
Dependent shipment step cannot occur before payment is captured.
Rejected command does not mutate state.
```

---

## 22. Suggested File Names in FSL Repository

```text
docs/DESIGN-domain.md
docs/DESIGN-effect.md
docs/intro/domain.ja.md
docs/intro/domain.en.md
examples/domain/order_functional_ddd.fsl
examples/domain/order_async_effect.fsl
schemas/fslc/domain/finding.v0.schema.json
src/fslc/domain_parser.py
src/fslc/domain_expand.py
src/fslc/domain_analyze.py
src/fslc/domain_codegen/typescript.py
src/fslc/domain_testgen.py
```

---

## 23. Conclusion

Functional DDD and async/effect modeling should be added to FSL as dialects rather than kernel-specific ad hoc syntax.

The essential addition is:

```text
fsl-domain:
  aggregate / command / event / error / decide / evolve / projection

fsl-effect:
  effect / effect_handler / await / retry / timeout / cancellation / compensation /
  idempotency / correlation / saga
```

This gives FSL a clean path from formal/domain specification to implementation scaffold:

```text
FSL domain spec
  -> kernel verification
  -> functional DDD scaffold
  -> async effect harness
  -> test adapter
  -> scenario/random-walk/replay conformance
  -> AI-readable repair findings
```

The main value is not just code generation. The main value is that FSL can make the following design decisions explicit and checkable:

```text
which aggregate owns which invariant
which command emits which event
which state transition is pure
which operation is an external effect
which async result is awaited
which effect requires idempotency
which late result requires stale handling
which failure path needs compensation
```

That is exactly the level of structure AI agents need in order to generate implementations that are not merely test-passing, but architecturally coherent.

