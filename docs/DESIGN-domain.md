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
  event flags. **Evolve pairing invariant** (#779): an `evolve` is 1:1 with
  the *occurrence* of its event, so any generated action that raises
  `event_<Event> := true` — a command/decide action, an effect-completion
  action, a saga observe action, or a saga step/timeout/compensation action —
  must apply that event's declared `evolve` assignments in the same action, in
  emit order, if the domain declares one for it. Before #779, saga
  step/timeout/compensation actions called only the shared `event_assignments`
  helper and never the paired `evolve_items`/`saga_emit_evolve` call, so a step
  could raise its emitted event's flag while leaving the aggregate state that
  event was declared to evolve frozen — an accepted-but-unreachable-transition
  soundness defect in the same class PR #725 (#713) closed for the
  compensation guard. Both lowering paths now call the paired helper for every
  saga action kind; `rust/fsl-core/tests/domain_saga_evolve_pairing.rs` sweeps
  every action in the domain corpus and fails if any future lowering change
  (e.g. #679's saga-history rewrite) drops the pairing again. If the same
  event appears in both a step's `emits` and its (or a later step's) `awaits`,
  the evolve is applied twice — once per occurrence (emit, then observe) —
  which is the correct event-sourcing reading, not a defect: applied count
  equals `event_assignments` execution count equals occurrence count.
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

Parsing a construct into the IR is not a promise that it means anything. Four
constructs reached the parse IR, were type-checked there, and were then dropped
by **both** lowering paths, so an author could write them and the executable
model would not contain them — an accepted construct with hollow semantics,
which `AGENTS.md` classifies as a soundness defect. They are now rejected at
their declaration, with a located `semantics` diagnostic, by the shared
`validate_lowerable_constructs` in `rust/fsl-core/src/domain_lowering.rs`,
called from both `lower_domain_surface` (path A) and `domain_kernel_source`
(path B) so neither path can accept what the other rejects. Any consumer of
the raw, unlowered `DomainSpec` must itself reach one of those two calls (not
walk the AST independently) or it silently regresses to accepting these
constructs — the gap #726 found and closed for `fsl_tools::analyze_domain`,
the structural projection behind `fslc domain analyze`, by having it call
`domain_kernel_source` (path B) before projecting, discarding the rendered
text and keeping only the fail-closed guard:

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
- **effect `retry` `backoff`** (#723). This document (see "Effects" below)
  pins only the `max_attempts` retry bound; no accepted decision assigns a
  backoff strategy an execution meaning in the finite model, and neither
  lowering path nor the frozen Python reference (`src/fslc/domain_parser.py`,
  `src/fslc/domain_ir.py`) ever reads the parsed `backoff` value. `retry {
  max_attempts N }` without a `backoff` clause is unaffected and still lowers
  the existing `attempts < N` guard.

Rejecting is deliberate rather than conservative: implementing any of the four
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
boundary. `retry`'s `max_attempts` is the only lowered field; a `backoff`
clause parses but has no execution meaning in the finite model and is
rejected fail-closed (see "Rejected constructs" above, #723). The v0
implementation lowers the lifecycle to finite maps:

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

The checker reports two structurally different hard failures before a formal
proof can run:

- an irreversible effect that lacks `idempotency_key` is the
  `irreversible_effect_without_idempotency_key` finding (`error` severity,
  `rust/fsl-tools/src/domain.rs`): `domain check` returns
  `result:"violated"`, `formal_result:"not_run"`, and the finding is in
  `findings`, per the Findings section below.
- an async effect with no `correlation_id` is not a finding at all.
  `lower_domain` rejects it during Kernel lowering itself
  (`rust/fsl-core/src/domain_lowering.rs`), before the domain checker's
  finding pass runs, so the top-level envelope is `result:"error"`,
  `kind:"semantics"` — there is no `dialect`, `findings`, or
  `finding_schema_version` key, because this error is outside the finding
  schema entirely.

Irreversible effects that lack compensation are reported as the
`missing_compensation_for_irreversible_effect` warning. Reliable effects
without an outbox boundary are reported as the
`reliable_effect_without_outbox_boundary` warning, because they overstate
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

`domain analyze` rejects a spec containing one of the three constructs listed
above ("Rejected constructs") with the same `kind`/location/exit code
`check` reports for it (#726). Because `analyze` now shares its guard with
`domain expand` (both call `domain_kernel_source`), `analyze`'s accepted/
rejected spec set is identical to `expand`'s, not limited to the three
constructs above: `domain_kernel_source` also rejects a conflicting explicit
effect-outcome role (`validate_effect_outcome_roles`), a duplicate or empty
enum declaration (`validate_domain_enums`), and any failure its own
kernel-text rendering step raises (for example an unsupported Map/container
default shape, or a reference to an unknown domain type) — `analyze` now
rejects all of these too, even though none of them is one of the three named
constructs. Before this fix, `analyze`'s raw-AST projection was the only
`fslc` surface that could still show the shape of such a spec; every other
command (`check`, `domain check`, `domain expand`, `fslc fmt`) already
rejected it before this fix. Closing that last window is a deliberate
product choice, not an oversight: a spec containing one of these
constructs is now opaque to every `fslc` command until the construct is
removed or #723 gives it executable semantics, and no diagnostic-only
"structural inventory of an unlowerable spec" surface is offered as a
replacement. An author debugging why such a spec fails to lower falls back
to reading the source and the `check` diagnostic's location, the same as for
any other rejected construct.

## Findings

Findings use `schemas/fslc/domain/finding.v0.schema.json` and
`fsl:"fsl-domain-effect.v0"`. The native `fslc domain check` implements
exactly 4 finding kinds, all added by `effect_findings` in
`rust/fsl-tools/src/domain.rs`:

- `irreversible_effect_without_idempotency_key` (`error`)
- `pending_effect_without_timeout_or_fallback` (`warning`)
- `missing_compensation_for_irreversible_effect` (`warning`)
- `reliable_effect_without_outbox_boundary` (`warning`)

`warning` findings are design review findings. They do not block the formal
kernel run. `error` findings block the run because the generated model would
otherwise overstate the guarantee.

`reliable_effect_without_outbox_boundary`'s `witness` carries the fixed
`{"effect": <name>}` shape every finding's `witness` uses, plus an
`uncovered_sagas` array (saga names, sorted) when the effect has at least one
owning saga (see `DESIGN-effect.md`) and not every owning saga declares an
outbox. `uncovered_sagas` is absent when the effect has no owning saga at
all, so its presence distinguishes "no saga owns this effect" from "some
owning saga is still missing its outbox".

`fslc domain replay` additionally reports its own runtime-observation finding
kinds against a log, distinct from the static kinds above: `command_rejected_by_model`,
`uncorrelated_async_completion`, `duplicate_irreversible_effect_commit`,
`effect_completion_rejected_by_model`, `unknown_domain_event`, `unknown_effect`,
`effect_completion_event_not_declared`, and `unknown_runtime_event_kind`
(`rust/fslc/src/main.rs`, tested by
`rust/fslc/tests/issue_518_domain_replay_detection.rs`). See the Runtime
Replay section.

`schemas/fslc/domain/finding.v0.schema.json`'s `kind` enum lists 24 values —
a superset of every kind on this page, reserved vocabulary the same way the
generated effect-status enum keeps a reserved `Compensated` member that no
lowering path writes (`docs/DESIGN-effect.md`). The 12 kinds above (4 static
+ 8 replay) are natively reachable; the other 12 are not. Of those 12, 7 have
accepted semantics in the frozen Python reference — `aggregate_boundary_violation`
(see the fail-closed note just below) plus the 6 kinds itemized further down
this section; the remaining 5 — `unowned_domain_invariant`,
`event_breaks_aggregate_invariant`, `rejected_command_mutates_state`,
`non_idempotent_irreversible_effect`, and `async_step_before_await_satisfied`
— have no implementation anywhere, native or frozen Python.

Several structural risks are enforced but are **not** findings — they fail
closed earlier, before the finding pass or the kernel run can start:

- An `evolve` that writes state outside its aggregate never becomes a
  finding; `unknown domain lvalue '<name>'` fails Kernel lowering itself
  (`rust/fsl-core/src/domain_lowering.rs`), a stronger (fail-closed) guarantee
  than a warning/error finding would give. (The frozen Python reference does
  implement this as a finding, `aggregate_boundary_violation`; native chose
  the stronger fail-closed form instead.)
- An async effect with no `correlation_id` never becomes a finding either;
  `lower_domain` rejects it with `effect '<name>' requires correlation_id`
  during Kernel lowering, before the finding pass runs. The static and replay
  forms of `uncorrelated_async_completion` are therefore different
  mechanisms with the same name: the static case is a hard
  `result:"error"`/`kind:"semantics"` failure outside the finding schema, and
  only the replay case (a completion observed with no matching prior request
  in a runtime log) is an actual `fsl-domain-finding.v0` finding.

The following finding kinds are **not implemented natively** but do exist,
with accepted semantics and executable regression coverage, in the frozen
Python compatibility reference (`src/fslc/domain_expand.py`). `src/fslc` is
not a product surface (`AGENTS.md`), so none of these are native contract —
but a future native implementer has real prior art to port, not a blank
page:

- `missing_decide_for_command` (`src/fslc/domain_expand.py:843-855`): a
  command with no `decide` is silently accepted by native `domain check`
  today (`verified_under_assumptions`, empty `findings`) rather than
  reported.
- `missing_evolve_for_event` (`src/fslc/domain_expand.py:856-868`): same gap
  for an event with no `evolve`.
- `cross_aggregate_update_without_event` (`src/fslc/domain_expand.py:886-900`):
  present in the frozen Python reference but was never listed here; recorded
  for completeness alongside the above.
- `late_completion_without_stale_policy` (`src/fslc/domain_expand.py:909`):
  present in the frozen Python reference, but not revivable in native as
  designed: `on_stale` itself has no executable lowering and is rejected
  fail-closed (`on_stale '<name>' has no executable lowering; stale policies
  are not supported`, `rust/fsl-core/src/domain_lowering.rs`, accepted in
  #711). A finding that recommends adding an `on_stale` policy would
  recommend syntax native rejects, so this finding kind must not be revived
  without first reopening #711's rejection.
- `saga_dead_end` (`src/fslc/domain_expand.py:993,1006`) and
  `process_wait_cycle` (`src/fslc/domain_expand.py:1019`): both have a
  currently-passing regression test against the frozen reference
  (`tests/test_domain_dialect.py::test_domain_reports_process_wait_cycle`).
  Whether to port them to native is an open decision tracked in
  [issue #769](https://github.com/ymm-oss/fsl/issues/769); until then, the
  closest native signals for a stuck saga are `fslc verify`'s action-coverage
  warnings in general, and — specifically for the one-hot dual trigger/after
  compensation-guard shape — the never-enabled-action warning described in
  the Runtime Replay section. Neither is a targeted `fsl-domain` finding for
  a dead-end saga or a wait-graph cycle.

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
includes the generated `kernel_source`). The CLI validates source through
`load_kernel_model` before `domain analyze` returns its projection or `domain
expand` returns rendered text; that is the same checked path `domain generate`
uses, so neither command can emit success for a document typed lowering rejects
(#796).
`rust/fsl-core/tests/domain_render_agreement.rs` projects both through
`public_kernel_contract` for the full domain corpus and requires them to match
except on source spans (#664). Building that gate found the two
implementations already disagree: `Context::normalize`/`Context::default`
(`domain.rs`) render text with `str::replace` and no syntax tree, so they
cannot be scope-aware the way `lower_domain`'s typed AST composition is. #690
fixed the known `can(...)` precedence false green by parenthesizing each joined
piece. #798 tracks the remaining scope-aware substitution and generated-name
renderer gaps. #796 validates the CLI source before returning its renderer
output, so the #798 generated-name misuse is rejected at the command boundary
rather than being emitted as a seemingly valid Kernel; it does not make the
two lowerings agree.
`domain_render_agreement.rs`'s `KNOWN_DIVERGENT_DOMAIN_FIXTURES` pins the
currently known instances as regression fixtures; #798 owns the remaining
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
