# FSL Domain / Effect Dialect Design

Status: adopted v0.

## Decision

`domain` is a frontend dialect for Functional DDD boundaries and async effect
lifecycles. It is not a new verifier kernel and not a programming language. The
v0 implementation parses domain declarations into typed IR, lowers the
checkable part to the existing kernel, and emits stable fsl-domain findings for
structural risks.

Implemented top-level shape:

```fsl
domain OrderDomain {
  implementation_profile functional_ddd

  enum OrderStatus { Pending, Approved, Cancelled }

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
short source vocabulary. The resolver selects bare enum members by expected
logical type and lowers `in [A, B]` to a finite equality disjunction; empty
membership lowers to `false`. A bare member with multiple candidates and no
expected type is rejected as ambiguous.

Canonical enum declarations use `enum Name { Member, ... }`; bounded numeric
types continue to use `type Name = lo..hi`. The 2.x parser retains
`type Name = A | B` as a loss-aware legacy source form so migration tooling can
replace the complete original declaration without changing the checked domain
model. Current-edition checks emit `deprecated_domain_enum_union`; checks with
`--edition next` reject that form. Empty and duplicate-member enums fail before
lowering, with the duplicate diagnostic attached to the repeated member.
Canonical and legacy non-empty declarations lower to the same public Kernel
enum contract.

`can(Command)` is a domain-only expression helper. It lowers to that command's
decide preconditions: all `requires` clauses and the negation of every
`rejects ... when ...` condition.

### Parse IR boundary

The Rust frontend parses every expression-bearing domain declaration directly
from the document's shared token stream into unresolved, loss-aware syntax
nodes. `SyntaxExpr`, `SyntaxIdent`, `SyntaxTypeExpr`, and `SyntaxLValue` retain
exact source spans for nodes and components; field declarations, invariants,
and assignments also retain their complete declaration span. Defaults, bounded
ranges, guards, rejection conditions, assignments,
invariants, stale policies, effect keys/correlation paths, and saga guards do
not cross the parse boundary as strings. This parse IR deliberately does not
perform domain name or type resolution. Checked and lowered expressions remain
the responsibility of `fsl-core` and the public Kernel contract.

Domain-only finite membership is represented structurally. Accepted legacy
spellings retain their source spelling while recording the canonical operator:
`||` is `or`, and logical `->` is `=>`. Structural `->` in declarations such
as await routing is consumed by the declaration grammar, not the expression
parser. `&&` remains outside the language and is rejected by the lexer.
Effect idempotency and correlation references remain restricted to the existing
dotted-identifier path grammar; routing those paths through the expression
parser does not broaden the public syntax to calls, indexing, or arithmetic.

`fsl-core` builds a symbol table for domain types, aggregate state, command and
event fields, commands, events, enum members, and lexical binders. Command/event
fields and inner binders shadow aggregate-state reads; assignment roots always
resolve to writable aggregate state. Resolution attaches a logical type and a
stable generated Kernel name to each selected symbol, then recursively lowers
`SyntaxExpr` and `SyntaxLValue` into `Expr` and `LValue`. `can(Command)` is
resolved only against the current aggregate. Unknown or cross-aggregate
commands, ambiguous enum members, type mismatches, invalid lvalues, and
unsupported calls fail at the original typed node span.

The executable path constructs `SurfaceSpec` directly and passes it to the
checked Kernel lowering gate. It never renders domain source as Kernel FSL and
parses it again. `fslc domain expand` may still render generated source as a
debug/interop view, but that text is not semantic input. This separation also
lets public Kernel diagnostics and origins use domain declaration/expression
coordinates rather than generated-source coordinates.

The Rust path records those coordinates in the non-serialized origin graph described
by [`DESIGN-origin-chain.md`](DESIGN-origin-chain.md). Checked state, action,
guard, statement, and property targets retain source identity, full span,
declaration path, and lowering steps. `can()` and membership expansions share
the source expression's stable identity across generated targets; merged
actions retain the decision as primary and the command as secondary. Synthetic
event flags and terminal nodes are explicitly generated-only. Requirement tags
remain a separate traceability relation.

Aggregate state fields retain their typed explicit initializer when present. In
the current edition, an omitted initializer keeps the established lowering
choice for Bool (`false`), enum (first declared member), range (lower bound), or
external placeholder (`0`) and emits `implicit_initial_value`. The warning names
the chosen value and reason and carries a byte insertion edit. This makes the
existing behavior migratable without treating an arbitrary default as newly
inferred intent; the edition migrator consumes the edit contract described in
[`DESIGN-initialization.md`](DESIGN-initialization.md).

## Effects

An async `effect` declares the request event, completion events, correlation id,
retry bound, timeout event, idempotency key, and optional reliable outbox/inbox
boundary. The v0 implementation lowers the lifecycle to finite maps:

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
`fsl:"fsl-domain-effect.v0"`. Implemented finding kinds include:

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

All five target emitters consume Public Kernel v1 JSON rather than
`DomainSpec` or another private Rust AST. Public Kernel remains the authority
for the checked spec identity, dialect, and lowered member names. A
small closed companion,
`schemas/fslc/domain/scaffold-metadata.v1.schema.json`, carries only the
source-level grouping and spelling that lowering intentionally erases (including
unused commands/events/errors and effect/saga topology). The adapter validates
both schema versions and confirms that companion declarations with lowered
type, state, and action counterparts are present before an emitter runs; it
never reparses the source and has no fallback to the private model.

The companion is a versioned public migration contract for information that
Public Kernel v1 does not encode. In particular, source expressions, unused
declarations, effect request routing, and saga start topology are
authoritative in the companion and cannot be cross-validated against v1.
Malformed versions, duplicate Kernel members, and missing lowered counterparts
fail closed. The full valid domain corpus is generated for every target to
guard the accepted language surface. The v1 bridge is supported for at least
two minor releases. It
may be removed only in a following major after
target generators have moved to the external compiler boundary or a negotiated
public contract can represent the missing domain topology. The former direct
`DomainSpec` emitter path was retired only after TypeScript, Python, Kotlin,
Swift, and Rust output matched the pre-migration goldens. `domain testgen` now
reuses the same TypeScript adapter/effect emitter instead of maintaining a
second implementation.

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
the kernel model and add `DOMAIN-ASSUME-SAGA-OBSERVED-HISTORY`. Durable process
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
outbox/inbox adapters, and fuller non-TypeScript generators should consume the
public Kernel boundary rather than adding a second semantics.
