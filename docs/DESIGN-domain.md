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
  (#713: both lowering paths, `lower_saga_actions` in `domain_lowering.rs` and
  `render_saga_actions` in `domain.rs`, emit a `requires` for the trigger
  event flag AND a separate `requires` for the `after_event` flag, in that
  order; before #713 only the trigger flag was required, so a compensation
  could fire on a trace that never observed its `after_event`)

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

### Rejected constructs: parsed, never lowered, so rejected fail-closed

Parsing a construct into the IR is not a promise that it means anything. Three
constructs reached the parse IR, were type-checked there, and were then dropped
by **both** lowering paths, so an author could write them and the executable
model would not contain them — an accepted construct with hollow semantics,
which `AGENTS.md` classifies as a soundness defect. They are now rejected at
their declaration, with a located `semantics` diagnostic, by the shared
`validate_lowerable_constructs` in `rust/fsl-core/src/domain_lowering.rs`,
called from both `lower_domain_surface` (path A) and `domain_kernel_source`
(path B) so neither path can accept what the other rejects:

- **top-level `await` routing** (#712). No accepted decision assigns routing a
  meaning; this document accepts only its grammar (see the Parse IR boundary
  above), and cross-aggregate routing proofs are Future Work. A saga step's
  `awaits` is a different construct and remains fully lowered.
- **aggregate `on_stale`** (#711). Nothing pins the stale-completion semantics.
  For this to become implementable, an accepted decision must define the stale
  predicate's evaluation point, the generated action and guard, and the
  interaction with the effect-completion single-writer rule below.
- **`value_object` invariants** (#710). What is undecided here is the *instance
  set*, not the predicate: whether the constraint attaches to direct aggregate
  state fields only, or also to occurrences inside `Option<T>`/`Set<T>`/
  `Map<_, T>`, to command parameters and event fields, to nested value objects,
  and whether a value object's own field defaults must satisfy it. The frozen
  Python reference emits only the direct-state-field instances, which is why it
  is not adopted as the contract: an author would believe every occurrence is
  checked while most are unconstrained. Value objects *without* invariants are
  unaffected and still lower as structs.

Rejecting is deliberate rather than conservative: implementing any of the three
would require inventing semantics inside a verifier, which is worse than
refusing the construct. Each becomes implementable when an accepted amendment
to this document settles the questions named above. Because the constructs never
had executable meaning, removing such a block from an existing spec changes no
verification outcome — see `CHANGELOG.md` for the migration note, and #702/#703
for the general strictness-migration mechanism.

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
choice -- Bool (`false`), Int (`0`), enum (first declared member, rendered
bare as domain source itself would accept), range (lower bound), external
placeholder (`0`), `Option<T>` (`none`), `Set<T>` (`Set {}`), `value_object`
(its own default struct literal), or a top-level `Map<K, V>` (the dense
per-key `forall` init) -- and emits `implicit_initial_value` for every one of
these shapes (issue #731), not only the four scalar ones. The warning names
the chosen value and reason and, where the value's shape allows a safe
insertion, carries a byte insertion edit; a top-level `Map<K, V>` field (no
whole-field initializer syntax exists) and a `Set<T>`/`value_object` field
(blocked by a pre-existing formatter round-trip defect, issue #770) warn
without one and keep next-edition severity at `warning`. This makes the
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

`success_event`, `failure_event`, and `timeout_event` are authoritative outcome
roles and lower to `Succeeded`, `Failed`, and `TimedOut` before any event-name
heuristic is considered. A single event assigned to multiple explicit roles is
rejected before Kernel lowering. Outcomes without an explicit role keep the v0
heuristic (`timeout`/`timedout`, then `fail`, then `cancel`, otherwise success).
The completion assignment is the single classification point; retry guards and
the success-sticky transition consume its resulting status rather than
reclassifying the event.

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
kernel proof. Outcome events owned by an effect are observed only through that
effect's correlation-guarded completion action, so the modeled lifecycle retains
completion-requires-request without a weaker saga observation writer.

Because generated `event_<Event>` flags are a one-hot, one-step observation
(see `docs/DESIGN-saga-history.md`), a compensation's dual trigger/after guard
is only satisfiable by a single transition that emits both events. When the
trigger and after events differ — the typical shape, e.g.
`examples/domain/order_fulfillment_saga.fsl`'s
`when PaymentFailed after InventoryReserved` — the compensation action is
structurally disabled under the current one-hot flag scheme, and `fslc
verify` surfaces it as a "never enabled" action warning rather than silently
passing. This is the accepted interim boundary
(`docs/DESIGN-saga-history.md`'s "Implementation boundary" section): the
warning is valid evidence of the known gap and must not be suppressed or
replaced by sticky global flags until the correlation-indexed saga history
follow-up (issue #662) lands.

## Guarantee Boundary

The kernel can prove bounded aggregate invariants, rejected-command no-op shape
by construction, completion-requires-request in the modeled lifecycle, retry
bounds, and sticky success status. It does not prove external API correctness,
queue delivery, wall-clock time, production idempotency across unbounded keys, or
that generated code is production optimal. Those require runtime replay, adapter
tests, or external evidence.

`domain` semantics has two independent lowerings that must stay in agreement:
`lower_domain` (typed `KernelSpec`, used by `check`/`verify`) and
`domain_kernel_source` (rendered `.fsl` text, used by `domain expand` and
by `check_domain` to validate renderability; only its hard-finding envelope
includes the generated `kernel_source`).
`rust/fsl-core/tests/domain_render_agreement.rs` projects both through
`public_kernel_contract` for the full domain corpus and requires them to match
except on source spans (#664). Building that gate found the two
implementations already disagree: `Context::normalize`/`Context::default`
(`domain.rs`) render text with `str::replace` and no syntax tree, so they
cannot be scope-aware the way `lower_domain`'s typed AST composition is. #690
fixed the known `can(...)` precedence false green by parenthesizing each joined
piece; its scope-aware substitution and generated-name symptoms remain open.
`domain_render_agreement.rs`'s `KNOWN_DIVERGENT_DOMAIN_FIXTURES` pins the
currently known instances as regression fixtures; #690 owns the remaining
`Context::normalize` scope and generated-name gaps. #691 (a separate root cause --
`Context::default` had a catch-all arm reachable for every container type,
not `Context::normalize`'s substitution-order problem) is fixed:
`Context::default`/`Context::default_for_type` are now total over the
field's `SyntaxTypeExprKind` with no catch-all, `Option<T>`/`Set<T>` render
`none`/`Set {}`, and a top-level `Map<K, V>` state field renders the same
dense per-key `forall` init `lower_domain` already generated. Fixtures for
all three affected shapes (`Option`, `Set`, `Map`) are registered in
`VALID_DOMAIN_FIXTURES`. Enum declaration validation is shared ahead of both
paths, so a container cannot hide an empty or duplicate enum until rendered
kernel parsing; rejection is anchored at the enum declaration.
The CLI keeps the renderer's typed location and name-resolution classification
when it constructs that rejection envelope; it does not recover either from the
formatted message.

## Future Work

Remaining work is production hardening rather than dialect absence: the
accepted correlation-indexed saga-history design in
[`DESIGN-saga-history.md`](DESIGN-saga-history.md), stronger cross-aggregate routing proofs, production
outbox/inbox adapters, and fuller non-TypeScript generators should consume the
public Kernel boundary rather than adding a second semantics.
