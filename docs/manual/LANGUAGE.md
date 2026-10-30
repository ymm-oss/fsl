# FSL Language Reference

FSL is a formal specification language for application development whose primary
design goal is to be **written, verified, and repaired by generative AI**. This
document is the language reference you consult when writing specifications (it
always tracks the latest implementation). The background to design decisions and
the implementation design of each feature can be reached from
[`README.md`](../README.md) (the docs map).

## Design principles

| Principle | Existing languages (TLA+/Alloy) | FSL |
|---|---|---|
| Syntax | Mathematical notation (∀, □, ◇) | TypeScript/Python-like. Stays close to the LLM training distribution |
| Counterexamples | Human-oriented text | **Structured JSON** (with state diffs and the violated bound variables) |
| Errors | Human-oriented messages | Machine-readable (line, column, classification, repair hint) |
| Verification | Full verification assumed | Bounded and fast by default. **Unbounded-depth proofs via k-induction** are also possible |
| Vacuity | Found by an expert's intuition | Action reachability + **unsat-core diagnosis of blocking requires** |
| Pitfalls | Avoided by discipline | **Structurally eliminated** (automatic bounds checks, implicit checking of partial operations) |

## 1. Structure of a specification

```fsl
spec <Name> ["<kind>: <intent>"] {   // optional spec-level tag → metadata badge (explain/html); never verified
  const <NAME> = <constant expr>
  type  <Name> = <lo>..<hi>            // domain type (bounded integer)
  symmetric type <Name> = <lo>..<hi>   // domain whose values are interchangeable identities
  enum  <Name> { <Member>, ... }
  symmetric enum <Name> { <Member>, ... }
  struct <Name> { <field>: <scalar type | nested Option<scalar type>>, ... }

  def <name>(<p>: <type name>, ...) = <expr> // non-recursive named predicate; frontend-inlined

  state { <var>: <type> [= <deterministic expr>], ... }
  init  ["undecided: reason"] { <stmt>... }

  [fair] action <name>(<p>: <type name>, ...) {
    requires <expr>                     // guard. multiple allowed (conjunction)
    let <x> = <expr>                    // local binding
    <stmt>...                           // assignment / if-else / forall
    ensures <expr>                      // postcondition. reference the old state with old(expr)
  }

  invariant <Name> { <expr> }           // holds in all reachable states (safety)
  trans <Name> { <expr> }               // holds across all reachable transitions (two-state safety)
  reachable <Name> { <expr> }           // is reachable (returns a witness)
  leadsTo <Name> { <response property> }// bounded response, or ranked induction with decreases (see §1)
  terminal { <expr> }                   // intended terminal states (excluded from deadlock checking)
}
```

Property names (`invariant`, `trans`, `unless`, `reachable`, `leadsTo`, and
`until`) share one namespace within a spec: results, `--property` selection, and
the Public Kernel are keyed by that name, so reusing it — within one kind or
across kinds — is a `kind: "semantics"` check-time error located at the later
declaration, naming the earlier one's location. An `until <Name>` also claims
`<Name>_until_safety`, the name of the `trans` it lowers to (#1192).

The optional string after the spec name is a **spec-level tag**
(`"<kind>: <intent>"`, e.g. `spec ReturnUI "ui: screen flow" { … }`). Like the
per-declaration tags, it is **metadata only** — never verified — and is surfaced by
`fslc explain` / `fslc html` as a classification badge next to the spec name (JSON:
`skeleton.spec_kind = {id, text}`). Use it to record what kind of thing the whole
spec is (e.g. `ui` for a screen-flow spec that models only the behavioral slice);
it carries no kernel semantics and desugars to nothing. See `docs/design/DESIGN-ui.md`.
Internally this legacy badge is adapted to the shared typed `Kind` annotation;
the source grammar remains unchanged. See `DESIGN-annotations.md`.

Any layer — including a kernel `spec` — may declare identity/number sorts whose
finite sizes come from a sibling top-level `verify` block instead of an inline
`type X = lo..hi` range. This keeps the domain declaration (what exists) separate
from the verification world size (how much is checked):

```fsl
spec <Name> {
  entity <Entity>          // identity sort; size from verify { instances <Entity> = N }
  number <Number>          // numeric sort; range from verify { values <Number> = lo..hi }
}
business <Name> {
  entity <Entity>
}
requirements <Name> {
  entity <Entity>          // optional explicit identity sort
  number <Number>
  process <Entity> with f: <Number>, g: Bool = <bool>, h: <Enum> = <Member> { ... }
                            // process also declares the entity kind; Bool/enum
                            // carried fields require an explicit `= ...` initializer
}
verify {
  instances <Entity> = <N>
  values <Number> = <lo>..<hi>
}
```

`entity`/`number` desugar to `type <Name> = lo..hi` before verification, so they
are exactly equivalent to writing the bounded type directly — the difference is
only readability (a design spec reads as documentation instead of asserting a
domain size that is really a model bound). See `docs/design/DESIGN-spec-domains.md`.

The database compatibility dialect is another frontend over the same kernel:

```fsl
dbsystem <Name> {
  database <db> {
    schema <initial_version>
    table <table> {
      column <column>: <db_type> present backfilled not_null;
      column <future_column>: <db_type> absent;
    }
  }

  migration <name> from <v0> to <v1> [rollbackable] {
    add <table>.<column> nullable;
    backfill <table>.<column>;
    set_not_null <table>.<column>;
    rename <table>.<old> to <table>.<new>;
    split <table>.<source> into <table>.<a>, <table>.<b> lossless|lossy|irreversible;
    merge <table>.<a>, <table>.<b> into <table>.<target> lossless|lossy|irreversible;
    drop <table>.<column> destructive|irreversible;
  }

  artifact <version> {
    reads <table>.<column>, ...;
    writes <table>.<column>, ...;
    requires <capability_namespace>.<capability>, ...;
    provides <capability_namespace>.<capability>, ...;
    calls api.<operation>, ...;
    accepts api.<operation>, ...;
    expects response.<field>, ...;
    responds response.<field>, ...;
    emits_offline api.<operation> ttl <finite_ticks>;
  }

  environment <env> {
    schema <lo>..<hi>;
    flag <flag_name> { <variant>, ... } default <variant>;
    active <version> when schema <lo>..<hi> when flag <flag_name>=<variant>;
    supported <version> when schema <lo>..<hi>;
    may_exist <version> when schema <lo>..<hi>;
  }

  check compatibility {
    rule all_active_reads_exist;
    rule all_active_writes_exist;
    rule removed_only_after_unused;
    rule not_null_after_backfill;
    rule destructive_operations_annotated;
    rule preservation_transforms_annotated;
    rule api_calls_accepted;
    rule api_responses_expected;
    rule offline_payloads_accepted;
    rule artifact_capabilities_provided;
    rule data_preserved;
    rule rollback_equivalent;
  }
}
```

`dbsystem` expands to a kernel spec with scalar state and `Map<Column, Bool>`
column lifecycle maps. It never uses nested `Map<_, Set<_>>` state. `fslc check`
and `fslc verify` work on it after expansion; `fslc db check` additionally emits
stable fsl-db findings and returns `verified_under_assumptions` for successful
formal checks. Environment schema ranges are finite reachable snapshots in the
declared migration order; rollout percentages and offline TTLs must be modeled as
finite coexistence windows/ticks. API/offline and bounded preservation/rollback
checks are dialect-level compatibility checks. Feature flags are finite declared
variants inside an environment and add `DB-ASSUME-FINITE-FLAG-STATE`; they do
not prove rollout percentages. Generic `requires` / `provides` capabilities
cover AI model/prompt/retriever, tool schema, output schema, mobile/server, and
other artifact profiles in the same snapshot model. See `docs/design/DESIGN-db.md`.

The twelve `check compatibility` rule names form a closed vocabulary shared by
lowering and the DB tools. Unknown names fail before any Kernel/model or artifact
is produced, including by `kernel`, `scenarios`, `explain`, `testgen`, and file-based
`fmt`/`lint`/`migrate` (exit 2). Lowering reports an authored `semantics` error;
commands render it like other lowering errors, with `loc` where their envelope
supports it and the source position in the message for `fmt`/`lint`/`migrate`.
Existing specialized-validator errors from `check` and the Worker remain
unlocated. DB-only findings and reachable-schema-window validation remain in the
DB tools; they are not additional Kernel-export gates. Valid rule/default
selection and repair-tool carve-outs (including formatter stdin) are unchanged.
Direct HTML/ledger reports keep verification's unlocated validator refusal before
presentation lowering. The #1473 base/head comparison found no `check` `ok`→`error`
transitions or output/exit drift across 595 file-backed forms; the exact commits,
binary hashes, and commands are recorded in `docs/design/DESIGN-db.md`.

Functional DDD / async effect dialect (v0; expands to the same kernel and
reports stable fsl-domain findings):

```fsl
domain <Name> {
  implementation_profile functional_ddd

  enum OrderStatus { Pending, Approved, Cancelled }

  aggregate Order {
    id OrderId
    state { status: OrderStatus = Pending; }

    command ApproveOrder {}
    event OrderApproved {}
    event PaymentCaptureRequested { payment_request_id: PaymentRequestId }
    event PaymentCaptured { payment_request_id: PaymentRequestId }
    event PaymentFailed { payment_request_id: PaymentRequestId }
    event PaymentCaptureTimedOut { payment_request_id: PaymentRequestId }
    error CannotApprove

    decide ApproveOrder {
      requires status == Pending
      emits OrderApproved
    }

    evolve OrderApproved {
      status = Approved
    }
    evolve PaymentCaptureRequested { }
    evolve PaymentCaptured { }
    evolve PaymentFailed { }
    evolve PaymentCaptureTimedOut { }

    invariant noLateApprove {
      status == Cancelled -> not can(ApproveOrder)
    }
  }

  effect CapturePayment {
    async
    irreversible
    idempotency_key Order.id
    correlation_id PaymentCaptureRequested.payment_request_id
    handles PaymentCaptureRequested
    emits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
    success_event PaymentCaptured
    failure_event PaymentFailed
    timeout_event PaymentCaptureTimedOut
    retry { max_attempts 3 }
    timeout after 10m emits PaymentCaptureTimedOut
    compensation { emits PaymentFailed }
  }

  saga OrderFulfillment {
    starts_on OrderApproved
    outbox OrderOutbox
    inbox FulfillmentInbox

    step RequestPayment {
      async
      emits PaymentCaptureRequested
      awaits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
      timeout after 10m emits PaymentCaptureTimedOut
    }
  }
}
```

`domain` models aggregate consistency boundaries, command intent, accepted
events, domain errors, pure `decide`/`evolve`, async effect lifecycles, and
saga/process-manager coordination. It lowers each command+decide+evolve path to
a kernel `action`, aggregate state to prefixed kernel state, saga steps to
event-flag guarded actions, and effect lifecycle state to finite
`Map<CorrelationId, EffectStatus>` / `Map<CorrelationId, Attempt>` maps. Every
action that raises an event's occurrence flag applies that event's declared
`evolve` in the same action, whichever construct generated the action
(command, effect completion, saga observe, or saga step/timeout/compensation).
Domain enum members are namespaced during lowering, so two domain enums may
both contain `Pending`. Domain expressions may use `X in [A, B]` and
`can(Command)`; these are
resolved from the typed domain tree and lowered structurally to kernel
expressions. Bare enum members use the expected logical type; an untyped member
shared by multiple enums is an error. Finite membership becomes an equality
disjunction (`X in []` is `false`). `can(Command)` resolves within the current
aggregate and becomes the conjunction of the command's `requires` clauses and
the negation of each rejection condition. Unknown symbols, cross-aggregate
commands, type mismatches, and unsupported calls are reported at the original
domain expression.

A saga `compensation { when Trigger after After { emits ... } }` block lowers
to a kernel action guarded by BOTH the trigger event flag and the `after`
event flag, so the compensation only fires once both events have been
observed. Because generated event flags are a one-hot, one-step observation,
this dual guard is satisfiable only when a single `decide` emits both events
in the same transition; the common case where the trigger and after events
differ (for example, a compensation triggered by a later failure after an
earlier success) is structurally disabled under the current flag scheme, and
`fslc verify` reports the disabled compensation action as a never-enabled
action warning rather than silently accepting a trace that never observed the
`after` event.

An effect can assign completion events explicit outcome roles with
`success_event`, `failure_event`, and `timeout_event`. These declarations are the
authoritative classification: they lower to `Succeeded`, `Failed`, and `TimedOut`
respectively, even when an event name contains `fail`, `cancel`, or `timeout`.
Assigning one event to more than one explicit role is a parse error. An outcome
without an explicit role retains the v0 name heuristic, in priority order:
`timeout`/`timedout` -> `TimedOut`, `fail` -> `Failed`, `cancel` -> `Cancelled`,
and otherwise `Succeeded`. Completion actions, the `Failed | TimedOut` retry
guard, and the success-sticky transition property all consume that one lowered
status.

Domain declarations use `enum Name { Member, ... }` for finite variants and
`type Name = lo..hi` for bounded numeric ranges. The legacy spelling
`type Name = A | B` remains accepted in the current 2.x edition and produces the
stable `deprecated_domain_enum_union` warning over the complete declaration,
including a canonical replacement. `fslc check`, `fslc verify`, and
`fslc domain check` accept `--edition current|next`; `next` rejects the legacy
union spelling. Other covered consumers use the source header's edition; this
does not add an `--edition` option to those commands. `fslc lint` reports the
edition finding without mutation, and
`fslc migrate --edition next` supplies the checked canonical edit. Empty enums
and duplicate members are errors, reported at the declaration and duplicate
member respectively.

The native Rust frontend also retains a private origin chain for domain state,
actions, guards, statements, and properties. Verification, counterexample, and
`explain` diagnostics use the original domain declaration as their primary
display name and keep generated Kernel names as machine detail. The chain
records source file/full span, declaration path, lowering steps,
primary/secondary sources, and explicit generated-only nodes. Requirement IDs
remain an independent traceability relation. Public Kernel v1 output is
unchanged. Opt-in Public Kernel v2 publishes the chain as a top-level provenance
graph with portable source identities, exact byte/line coordinates,
target bindings, source-node reverse lookup, and explicit assurance/completeness.

When a domain aggregate state field omits its initializer, the current edition
preserves the established Bool `false`, Int `0`, enum first-member (rendered
bare, as domain source itself would accept — never `domain_kernel_source`'s
kernel-scoped mangled identifier, no matter how deeply the enum is nested
inside a `value_object` or a `Map`'s value), range lower-bound,
external-placeholder `0`, `value_object` struct-literal, `Option<T>` `none`,
`Set<T>` `Set {}`, or top-level `Map<K, V>` dense per-key
(`forall k: K { field[k] = <V's default> }`, where `<V's default>` recurses
through this same selection) choice, and emits `implicit_initial_value` for
every one of these shapes (#731) — the warning's coverage matches the
renderer's total dispatch (`fsl_core::domain_type_default`, the single owner
both the warning and `domain_kernel_source` read the selected value, and how
an enum member within it is spelled, from), not just a scalar subset. The
warning contains the selected value, reason, current/next severity, field
span, and — where a machine-applicable insertion exists — a
`suggestion`/`canonical_replacement`. Two field shapes omit the insertion, and
consequently keep `edition_severity.next` at `warning` rather than `error`
(the next edition cannot yet demand an initializer it has no safe way to
insert): a top-level `Map<K, V>` field has no whole-field default at all
(`field: Map<K, V> = expr;` is always rejected, "whole-Map domain defaults are
not supported", because the per-key form is the only supported `Map`
default), and a `Set<T>` or `value_object`-typed field's brace-literal default
(`Set { ... }`, `Name { ... }`) cannot currently be round-tripped by `fslc
fmt`'s reformat-and-reparse pass (issue #770) even though `fslc check` accepts
it when written explicitly; `migrate --write` is fail-closed and would not
write a corrupted file, but offering the insertion would trip #770's reformat
failure and fail migration for the whole file, dropping every other,
otherwise-safe edit in it too. A `Map` nested as another `Map`'s value is
separately rejected ("Map state requires explicit initialization through
supported semantics") because the per-key init has no default to select for a
`Map` value type.

Use `fslc domain check` for stable fsl-domain findings and the nested kernel
result (`verified_under_assumptions` on success), `fslc domain analyze` for the
aggregate/effect summary, `fslc domain expand` to inspect a generated kernel FSL
debug view. At command entry, both `domain analyze` and `domain expand` read
one authored-source `String`, parse their `DomainSpec` with
`parse_domain_document_from_source`, and pass that same string to
`load_kernel_model_from_source` to construct the checked Kernel. An atomic path
replacement cannot make either command validate a different source version
before returning output. They reject unresolved identifiers with the original
source location instead of emitting a partial analysis or unusable Kernel text.
`domain generate`, `domain replay`, `domain testgen`, and `domain check` use the
same captured source for checked-kernel scaffolding, Monitor replay,
generic/adapter test generation, and edition post-processing respectively (#808).
`fslc domain generate --target typescript|python|kotlin|swift|rust` for
Functional DDD scaffolds, `fslc domain testgen` for adapter/conformance
scaffolds, and `fslc domain replay --logs events.jsonl` for runtime command /
event / effect evidence. The v0 implementation proves the finite modeled
lifecycle; replay is observation evidence and saga history adds
`DOMAIN-ASSUME-SAGA-OBSERVED-HISTORY`.
The native scaffold emitters consume checked Public Kernel v1 JSON plus the
versioned `domain-scaffold-metadata.v1` compatibility bridge; they do not
receive the private domain AST or reparse source expressions. Kernel/metadata
version and dialect mismatches, duplicate Kernel members, and missing lowered
type/state/action counterparts fail closed. Source expressions and effect/saga
topology remain authoritative in the companion because Public Kernel v1 does
not encode them. All five target outputs are locked to pre-migration goldens,
and the valid domain corpus is generated for every target.
It does not prove real gateway behavior, wall-clock timeouts, queue delivery, or
production exactly-once semantics. See `docs/design/DESIGN-domain.md` and
`docs/design/DESIGN-effect.md`.

`fslc verify` can override a `verify` block's `instances`/`values` bounds from
the command line, without editing the spec:

```bash
fslc verify spec.fsl --instances Case=1 --property EventuallyLeavesInProgress
fslc verify spec.fsl --values Amount=0..3
```

Both flags are repeatable (`--instances A=1 --instances B=2`) and replace the
bound of the matching `entity`/`number` name — handy for shrinking to a
1-entity model for liveness/induction runs (see §7) without touching the file.
A `NAME` with no matching `entity`/`number` declaration, or a spec whose bounds
are not `entity`/`number`-backed (a kernel `type X = lo..hi` literal), is a spec
error (exit code 2), as is a malformed value (`Case=abc`, `N=5..1`). The
effective overridden bounds are echoed back in the JSON envelope's
`bounds_overrides` field.

When the spec has an inline `implements`, the override also propagates into the
abstract spec — restricted to the entity/number names the abstract itself
declares (in a `requirements` abstract a `process` name counts as an entity,
since lowering declares one for it) — so the refinement check runs at the same world size on both sides
(refinement is a same-size forward simulation; without this, a shrunken impl
and a full-size abstract would fail with `map_out_of_bounds`). An impl-only
carried number (e.g. `Amount`, absent from a business abstract) applies to the
impl only.

`fslc verify` still evaluates inline `implements` when only `--instances` /
`--values` scope overrides are present. `--property`, `--exclude-property`, and
`--from-state` do not evaluate it; the envelope then keeps the key as
`implements: {"abs": ..., "result": "not_evaluated", "reason": ..., "reasons": [...]}`
instead of omitting it (see "Checks a selected run does not evaluate" in §7).

`acceptance`/`forbidden` scenarios often hardcode ids/numbers from the spec's
original world (`accept(2)`), which can fall outside a shrunken override
(`--instances Case=1`). When overrides are active, a scenario whose replay
fails *purely* because it references a value outside the overridden bounds
(an out-of-range action argument, or an out-of-range index inside its
`expect`) is downgraded per-scenario from a hard error to a skip, reported in
the envelope's `warnings` (`{"kind": "acceptance_skipped"/"forbidden_skipped",
"id": ..., "reference": ..., "message": ...}`, where `reference` names the
out-of-range argument or index); the rest of the scenarios still run. Without
overrides — or for any other failure (a false `expect`, an unmet `requires`)
— behavior is unchanged: hard error (exit 2, `trace_type: "acceptance"` /
`"forbidden"`), including when the override equals the declared range. Every
`fslc sweep` cell runs with overrides, so the same rule applies per cell: a
failing in-range scenario makes the sweep return that error rather than
`sweep_passed`. A reference counts as out of range only when the override
removed it — it is inside the spec's declared bounds but outside the overridden
ones — and an `expect` error is excused only when the same `expect` evaluates
in the declared world; a reference outside the declared bounds (so a no-op or
unrelated override) or an `expect` error the declared world shares (a division
by zero) stays the unscoped hard error. A forbidden scenario whose *final* step
is outside the overridden scope only because the override removed its argument
tested no guard, so it is reported as `forbidden_skipped` rather than counted as
satisfied; a final step outside the declared scope is the unscoped
`kind:"forbidden"` error (#1229), unless the override widens the scope to
include it, in which case the guard is evaluated. These
three conditions are stricter than the frozen Python reference, which skips any
out-of-range reference, excuses any `expect` error that mentions one, and counts
an out-of-range final step as a rejection. This is what makes `--instances Case=1
--property <Liveness>` usable even when the spec's acceptance scenarios were
written against the original `verify { instances Case = N }` bound.

**Envelope (#1008, #1218).** When a scoped run skips any scenario this way,
the envelope also keeps `requirement_traces: {"result": "not_evaluated",
"reason": "bounds_override", "reasons": [...], "skipped": [{"kind", "id",
"reference"}, ...]}` (§7, "Checks a selected run does not evaluate"); the key
is absent when every scenario was replayed. The native CLI implements the rule
above as stated; the frozen Python reference
(`tests/test_acceptance_override_skip.py`) agrees on its six cases and differs
only where noted above. In `fslc sweep`, `sweep.not_evaluated.skipped` lists
the `{kind, id}` of every scenario any cell skipped.

`fair` is a weak-fairness annotation: if that action instance remains
continuously enabled, the assumption is that it will eventually be executed.
Fairness applies to a whole action instance. To model fairness only under a
condition, split that condition into a separately guarded `fair action`.

**Action parameter types** (`<p>: <type name>`): a domain type, enum, or the
builtin `Bool` — anything BMC can enumerate. `Bool` behaves exactly like a
`Bool` state variable: use it bare as a boolean guard (`requires b`,
`requires not b`) or assign it into `Bool`-typed state
(`flag[i] = b`). The builtin `Int` is rejected (an unbounded parameter can't
be enumerated); use a range parameter instead: `p in <lo>..<hi>` (an inline
alternative to `<p>: <type name>` that doesn't require declaring a named
domain type).

The hierarchy of properties: `invariant` is one-state safety, `trans` is
two-state safety (the pre-transition state can be referenced with `old()`), and
`leadsTo` is response liveness. Without a ranking function, `leadsTo` is checked
boundedly. With `--engine induction` and `decreases <int expr>`, a `leadsTo` can
be proved unbounded by a well-founded ranking argument.

FSL verifies finite transition systems; it does not declare higher-order axioms
over the refinement relation itself. Laws such as refinement reflexivity or
transitivity are checked through concrete refinement chains or finite state-machine
models, not quantified as algebraic axioms.

Response properties inside a `leadsTo` block:

```fsl
leadsTo <Name> {
  <expr> ~> <expr>                      // once P holds (including the same instant), Q eventually holds
  <expr> ~> within K <expr>             // Q must hold within K steps after P
  forall x: T { <expr> ~> <expr> }      // checked independently per binding (only an outer forall may nest)
  helpful <action>(<binding expr>, ...)  // optional; per-binding progress action for ranked induction
  decreases <int expr>                  // optional; induction-only ranking measure
}
```

`~>` is **exclusive to leadsTo blocks** — it cannot be used in general expressions.
`check` checks each `helpful action(args...)` argument against its action parameter
type in the property's binder scope. This closes a previously unchecked position:
ill-typed arguments, unknown names and forbidden pattern shadowing now fail `check`.
Normalized `count`/`sum` patterns retain their standalone acceptance, including
valid empty aggregates; composition cannot hide an original global-name refusal
by prefixing, nor introduce one from a foreign declaration.
Induction additionally requires state-independent arguments: leadsTo binders, constants, enum members,
integer/Boolean literals, and integer `+`, `-`, `*` combinations (including unary `-`).
`check` and BMC accept well-typed arguments that read state; induction refuses them.
`within K` is a bounded deadline on a response; `K` must be a non-negative
constant expression. It is checked by BMC, not by the ranked induction proof.
`decreases` is optional and must be an integer-valued expression. Under
`fslc verify --engine induction`, the verifier proves the ranked response by
checking, under the proved invariants, that whenever `P` is pending and `Q` is
false: the measure is non-negative; progress is possible; and the enabled-action
discipline is satisfied. Without `helpful`, every enabled action must either make
`Q` true or keep `P` true while strictly decreasing the measure. With one or more
`helpful action(args...)` lines, only the matching helpful action instance must
strictly decrease the measure when it fires; unrelated action instances must
keep the pending obligation true (unless they make `Q` true) and must not
increase the measure -- an unbounded increase between helpful firings could
outpace the guaranteed decrease and prevent `Q` from ever being reached, even
though the helpful action keeps firing under fairness. The matching
helpful action must be a lower-layer `fair action` and must be enabled whenever
the obligation is pending. `helpful` is metadata for the ranked proof: it does
not create a fairness assumption. Ranked proof success is independent of
`--depth`; `--depth` is still used for the base BMC check and
reachable/coverage evidence.

**With two or more distinct `helpful` action names**, each instance's
enabledness must not flicker: once a helpful instance becomes enabled while
the obligation is pending, it must stay enabled until it fires (or `Q` holds).
"Some helpful match is enabled at every pending state" is not enough on its
own -- if *which* instance is enabled keeps changing (e.g. two helpful
actions enabled on alternating conditions), no single instance is ever
*continuously* enabled, so its `fair` declaration never actually obligates it
to run, and the leadsTo can be genuinely false even though the disjunctive
enabledness check passes. This is reported as
`rank_failure:"helpful_action_enabledness_not_sticky"`. The common single
per-binding `helpful step(c)` idiom above is unaffected: with one helpful
action, "always enabled while pending" already implies it is continuously
enabled.

**Placement.** `decreases` is a sibling of the response body inside the
`leadsTo` block, *outside* any `forall` wrapper — never nested inside the
forall's braces. Nesting it inside `forall` is a parse error, not a
limitation of ranking under `forall`:

```fsl
// valid: decreases after the forall's closing }
leadsTo Responds {
  forall c: Case { level[c] > 0 ~> level[c] == 0 }
  decreases level[0] + level[1]
}

// parse error: decreases nested inside the forall body
leadsTo Responds {
  forall c: Case { level[c] > 0 ~> level[c] == 0 decreases level[0] + level[1] }
}
```

**Per-entity measures under interleaving need `helpful`.** A measure that
mentions only the bound entity, e.g. `decreases level[c]` inside
`forall c: Case { level[c] > 0 ~> level[c] == 0 }`, is not enough by itself:
an action that advances a different entity can leave `level[c]` unchanged. Add
the per-binding progress action:

```fsl
leadsTo Responds {
  forall c: Case { level[c] > 0 ~> level[c] == 0 }
  helpful step(c)
  decreases level[c]
}
```

With this form, `step(c)` must be declared `fair action`, must be enabled in
every pending state for that binding, and must strictly decrease `level[c]`
when it fires. Other interleavings may preserve the pending obligation
without decreasing this per-entity measure, but must not increase it.
Diagnostics distinguish `progress_action_not_fair`,
`helpful_action_not_enabled`, `non_decreasing_helpful_action`,
`non_helpful_action_increases_measure`, `pending_not_preserved`, and (with
two or more helpful actions) `helpful_action_enabledness_not_sticky`.

**Working idiom: a global sum measure.** Sum the tracked quantity across the
domain with the built-in `sum()` aggregate (§3): `decreases sum(k: Case of
level[k])`. `sum()` enumerates the bounded `Case` domain itself, so the
measure is instances-count independent — the same `decreases` clause works
unchanged whether `Case` is sized via `verify { instances Case = N }` or
shrunk with a CLI `--instances` override. Because every fair `step` decrements
exactly one `level[k]`, every enabled action strictly decreases the total, so
induction returns `"proved"` with `"completeness": "unbounded"`. This idiom
only covers designs where *every* enabled action decreases the total. For
per-entity progress under interleaving, prefer the `helpful` form above.
(A conditional measure can select between integer expressions, but it does not
by itself prove the per-entity decrease required by the `helpful` form.)

## 2. Types

| Type | Example | Description |
|---|---|---|
| `Int` / `Bool` | `count: Int` | Unbounded integer / boolean |
| Domain type | `type Qty = 0..5` | Bounded integer. **The range is checked automatically** (§6) |
| Inline state domain | `state { qty: 0..5 }` | Shorthand for a named domain type in a state-variable declaration |
| Inline state initializer | `state { qty: Qty = 0 }` | Deterministic sugar for the equivalent root assignment in `init` |
| Entity kind | `entity Claim` / `process Claim ...` | Finite identity sort. Allowed in any layer incl. kernel `spec`; size set by `verify { instances Claim = N }`; desugars to `type Claim = 0..N-1` |
| Number kind | `number Amount` | Finite numeric sort. Allowed in any layer incl. kernel `spec`; range set by `verify { values Amount = lo..hi }`; desugars to `type` |
| enum | `enum St { Open, Closed }` | Members are referenced by their bare name in expressions |
| struct | `struct Order { st: St, item: Option<Option<ItemId>>, qty: Qty }` | Fields are scalars or nested `Option<scalar>` |
| `Option<T>` | `cart: Option<Option<ItemId>>` | `none` / `some(e)`. Used instead of a sentinel value; nesting is supported around a scalar payload |
| `Map<K, V>` | `stock: Map<ItemId, Qty>` | K must be a bounded scalar (domain type / enum / Bool) |
| `Set<T>` | `shipped: Set<OrderId>` | T is a bounded scalar |
| `Seq<T, N>` | `queue: Seq<JobId, 3>` | A sequence (FIFO) of capacity N. T is a scalar, N is a constant |
| `relation A -> B` | `delegates: relation User -> User` | A bounded binary relation over bounded scalar endpoints |

**Scalar** = Int / Bool / domain type / enum. In a `state` declaration,
`x: lo..hi` is accepted as an anonymous domain type and is equivalent to
declaring `type X = lo..hi` and writing `x: X`.

Every spec/Kernel enum declaration, including `symmetric enum`, must contain at least one
member, even if the type is unused. `enum E { }` is rejected while building the
checked model with `kind: "type"`, message `enum 'E' has no members`, and `loc`
at the enum declaration (CLI exit 2). `check` and all model-consuming commands,
including every verification engine, scenarios, testgen and refine, share this
gate; Worker checking uses the same rule. This is a **breaking removal** (#1546)
of earlier acceptance, including the former `EmptyImpl` / `EmptyAbs` refinement
inputs. Declare the intended members or remove an unused empty declaration;
do not rely on an empty domain to make verification vacuous. Singleton enums
remain valid. The domain dialect keeps its existing earlier `semantics` error
for empty enums; this change does not reclassify that diagnostic. A composed
child's empty enum follows #567: public loc points at the parent's `use`, and
the message retains the child's declaration path/position and the parent path.
This empty-enum wrapper resolves the child path against the parent's directory,
unlike older #567 wrappers that preserve the literal `use` path.
For `symmetric enum`, the declaration starts at `symmetric`. See
[the boundary decision](../design/DESIGN-enum-member-identity.md#nonempty-enum-declarations-1546).

A `dbsystem` database must declare at least one column. Both
`database app { schema 0 }` and a database containing only an empty table are
**breaking removals** (#1546): database lowering reports exact
`database 'app' declares no columns`, kind type, database-declaration loc and
CLI exit 2, before generating the `Column` enum, on `check`, BMC, induction,
explicit, scenarios, testgen and kernel. `db check` reports that error only when
it reaches database lowering; static compatibility findings are still reported
first. An active artifact reading an absent column can therefore yield
`violated` / exit 1, as on base. DB fmt/lint keep their existing rendering:
kind semantics, a location suffix in the message and no loc field (exit 2).
A database with one column remains valid.

A Kernel state field may use `name: Type = expr` for a simple deterministic
initial value. It is normalized to an ordinary root assignment before checking,
so Monitor, explicit exploration, BMC, induction, and Public Kernel v1 observe
the same semantics as an equivalent `init` block. The expression may use
constants, enum members, constructors, `none`, and deterministic collection
literals, but it must not read any state root or another initializer. Indexed,
field, conditional-statement, quantified, relational, and bulk initialization
remain in `init`. Assigning the same root both inline and in `init` is a semantic
error that reports both source locations. See
[`DESIGN-initialization.md`](../design/DESIGN-initialization.md).

**Types legal as state variables** (anything else is rejected by `check` as a type error):
scalar | nested `Option<scalar>` | struct (scalar / nested `Option<scalar>` fields)
| `Map<bounded scalar, scalar | nested Option<scalar> | struct>`
| `Set<bounded scalar>` | `Seq<scalar, N>` | `relation bounded-scalar -> bounded-scalar`

- Nesting structs, Set/Map/Seq inside a struct field,
  and `Option<Set/Map/Seq/struct>` are not allowed
  (rejected at check time with a hint). Optional scalar fields can be written
  directly inside a struct as of v2.1.
- A Map value may be a scalar, nested `Option<scalar>`, or a struct with those
  fields; nested Map/Set/Seq/relation values are rejected. In particular,
  `Map<Id, Map<K, Bool>>` is rejected because explicit-state execution cannot
  construct its required initial Map value. This is a breaking removal of a
  shape that earlier `check` accepted without an end-to-end execution path.
- The shared CLI/LSP state and struct-field type hints name this recursive
  `Option<scalar>` boundary, including the rejected relation and collection
  forms; this replaces the earlier one-level `Option<scalar>` wording.
- `Map<Int, V>` is rejected by `check`. Declare a bounded key type, for example
  `type ItemId = 0..<max>`, and use `Map<ItemId, V>`.
- `symmetric type` and `symmetric enum` mark values as interchangeable entity
  identities for liveness symmetry reduction. During `leadsTo` lasso/stall
  search, fslc uses one canonical representative for per-entity rows built from
  `Map<SymmetricType, V>` and `Set<SymmetricType>` state; `V` is used only when
  it contains no symmetric identity type. This is intended for models such as
  `Map<TaskId, Status>` where no task identity is special.

## 3. Expressions

`true`, `false`, and `none` are **reserved**: they always resolve to the literal,
so they cannot name a declaration anywhere — specification, const, `def` and its
parameters, type, enum and its members, struct and its fields, state variable,
action and its parameters, property, or a quantifier/aggregate binder or `is
some(x)` pattern binding. A declaration with one of those names would be
unreadable from every expression, and the misreading is silent: before this was
checked, `state { true: Bool }` with `invariant AlwaysHolds { true }` returned
`proved` while `init { true = false }` assigned a variable nothing could read.
No other word is reserved. `count`, `sum`, `stage`, `in`, `is`, `where`, `old`,
`abs`, `and`, and `or` are contextual and remain valid names; `some`, `Set`,
`Seq`, `unique`, `exactlyOne`, `forall`, and `exists` are only recognized before
further syntax, so misusing one as a name is a parse error rather than a silent
literal.

Named predicates factor repeated or business-significant expressions:

```fsl
def eligible(c: Claim) = submitted[c] and amount[c] <= AUTO_LIMIT
invariant OnlyEligible { forall c: Claim { approved[c] => eligible(c) } }
```

Calls must name a `def` in the same source file and match its arity. Definitions
may call earlier or later definitions but cannot be directly or mutually
recursive. They are expanded before semantic checking; the verifier and
runtime see the same kernel expression as hand expansion. Expansion rejects
variable capture instead of inventing internal binder names. See
[`DESIGN-def.md`](../design/DESIGN-def.md).

**Breaking removal (#1450):** a call is rejected when an action parameter,
`let`, expression quantifier/aggregate binder, or statement `forall` at the call
site would capture a free name in the definition's expanded body. The def's own
parameters are excluded. For example, if `on` is a state variable read by
`def p(z: Bool) = (not on) or z`, a call `p(false)` under `let on = ...` now
fails with exit 2, `kind: "semantics"`, and a `would capture variable` error at
the call. Rename the caller's binding and recheck; no synthetic rename or
compatibility bypass is supplied. Existing `is some(x)` pattern-shadowing
rejections are unchanged. This applies to direct specs and source-local defs
in compose components and synchronized actions.
A non-global `is some(v)` binding exported by a `requires` guard or the left
operand of a binary expression is also a caller binding for this check.

The final implementation's `check` sweep of all 597 `.fsl` files under
`specs/`, `examples/`, and `rust/fslc/tests/fixtures/`, using base
`11f01225e244dabde0f8dfd9aaa665bc47dccfd4` and built implementation
`dd7b47885680076412ab2f0b46ebe8854ff6758b` in their respective materialized
trees, has an empty ok-to-error transition set. The newly rejected inline
forms are not negated by that corpus result; the `leadsTo` declaration-binder
path was closed later by #1528.

**Breaking removal (#1528):** binders declared directly by a `leadsTo` property
also participate in this capture check for before/after expressions, `helpful`
arguments, `decreases`, and `within`. A call capturing a def-body free name now
rejects with exit 2, `kind: "semantics"`, and `would capture variable` at the
call, in direct specs, compose components and root properties, and dialect
properties lowered to `leadsTo` (such as requirements). Rename the declaration binder.
Binder ranges and the binder's own `where` expansion are unchanged; capture in
`where` can still precede the unsupported-property-wrapper-where rejection.
An already-invalid state-reading `within` call can now report `would capture`
at the call instead of `unknown constant`, still with exit 2 / semantics.
When a `leadsTo` also contains an already-invalid pattern-scope escape (#1547),
`would capture` can likewise precede that rejection; exit 2 / semantics is unchanged.

The #1528 `check` sweep used separately built `git archive` trees for base
`08d9656c5548a7ae5cbd2156880fdf52f9d0b8fc` and implementation
`c1a7d126071d8ac0600ffcd13d3ea9b3f0b7f286`. All 601 `.fsl` files under
`specs/`, `examples/`, and `rust/fslc/tests/fixtures/` agree in full stdout/stderr
and exits: zero ok-to-error transitions and zero error-to-error diagnostic
changes. The one migrated inline test source (602 forms total) is the only
ok-to-error transition: a declaration binder captures a def-body free state
name in a `decreases` call. There are no unrelated breaking removals.

- Arithmetic: `+ - * / %`, unary `-`, `min(a, b)` / `max(a, b)` / `abs(a)`
  (since `a//b` would turn everything after `//` into a comment, write division
  as `a / b` with whitespace). `/` and `%` are Euclidean for `b != 0`
  (`a == b * (a / b) + (a % b)` and `0 <= a % b < |b|`, so `-7 / 2 == -4` and
  `-7 % 2 == 1`). Division by zero is **totally defined**: `a / 0 == 0` and
  `a % 0 == 0` always evaluate, so an invariant/trans/reachable/leadsTo/refinement
  state-map expression that reads `a / 0` never fails to evaluate. Inside an
  action's `requires`/body/`ensures`, an unguarded `/`/`%` whose divisor can
  reach zero is still reported as `partial_op` (see §6) even though the value
  is defined — guard it with `y != 0 => P(x / y)` or `requires y != 0`. A
  refinement action-correspondence *argument* expression (§10,
  `impl_action(a) -> abs_action(a / c)`) is action context, not a property-context
  mapping expression, since it constructs an abstract action call: it gets the
  same `partial_op` treatment, surfaced as `kind: "map_partial_op"`. The
  corresponded abstract action's own body is action context too: a partial
  operation it hits at the mapped state is `kind: "abs_partial_op"`; one in
  its `ensures`, or a partial operation in an abstract post-state check
  (`Seq` operations; `/` `%` are total there), is `kind: "abs_step_violation"`.
- Comparison: `== != < <= > >=`
- Logical: `and or not =>`
  The shared model-expression checker requires a `Bool` operand for `not` and
  an `Int`-compatible operand for unary `-` (including named numeric ranges).
  This applies in traces and every model expression, even in unselected
  branches and short-circuited `and` / `or`. `not 1` and `-Red` for enum member
  `Red` are check-time `kind: "semantics"` errors (exit 2), not truthiness or
  enum-to-integer conversions. Well-typed `not (c == Red)`, `-(-1)` and numeric
  `- n` remain valid.
- Conditional: `if condition then when_true else when_false`. The condition is
  `Bool`; both branches are checked and must have compatible types. Scalars
  retain integer/range compatibility; whole finite containers require equal
  carriers (and equal Seq capacity), as specified below. It is
  available anywhere an expression is accepted, including guards, assignments,
  properties, function arguments, ranking expressions, constants, and
  refinement mappings. Conditional expressions associate to the right and each
  branch extends to the enclosing delimiter; use parentheses when a following
  operator should apply to the whole conditional. Concrete evaluation executes
  only the selected branch, while name and type checking always visits both.
- Finite binders: `x: T`, `x in lo..hi`, or `x in set_or_seq`, each optionally
  followed by `where predicate`. The predicate is `Bool` and is scoped after
  `x` is bound. Maps and unbounded collections are not binder domains. `x`'s
  scope is exactly that predicate and the quantifier/aggregate body: naming it
  outside is a type error, and when it shadows an enclosing name (an action
  parameter, say) the shadowing is confined to that scope — a satisfied
  `exists` never exports its witness to the enclosing bindings.
  **Constant-name collisions are rejected (#1515).** This breaking removal
  applies to every const type, not just integer constants: quantifier and
  aggregate binders, statement `forall` in init/actions, `leadsTo` binders,
  action parameters and `let` bindings, and `def` parameters may not have the
  same name as a const. Requirements action/transition inputs and indexed age
  binders follow the same rule. Components are checked before constant prefixing;
  defs are checked before inlining. `check` and all verification engines return
  the same `semantics` error (exit 2), naming the binding. The location is the
  existing AST owner's span, **not the binding-name token**: the enclosing
  expression/declaration for expression binders, the `forall` statement for
  statement binders, the `leadsTo` declaration for wrapper binders, the action
  for parameters/lets, the def for its parameters, the source transition for
  lowered inputs, and the age keyword for an indexed age. Component errors
  locate the parent's `use` and retain the component owner's coordinate in the
  message. Rename the local and its uses: with `const N = 2`, replace
  `forall N in 0..1 { N <= 1 }` with `forall k in 0..1 { k <= 1 }`.
  State-name shadowing remains valid; bare local `N` does not collide with
  qualified component const `part.N` in glue/sync. Refinement indexed-map and
  action-correspondence declarations have no additional const-shadow gate;
  their source specs still obey the ordinary checks. Existing pattern-binding
  freshness restrictions remain unchanged. This refusal prevents static-const
  and lexical-bound resolution from yielding opposite engine verdicts; it does
  not add general dependent range bounds (#1480).
  Source-visible implicit instance names obey the same rule: legacy process
  `c`, a next-edition entity's lowercased first letter (`r` for Request), and
  a relation event's implicit actor-instance parameter (`a` for Agent).
  A matching const is rejected even if the source never spells that local
  declaration. Rename the conflicting const and its constant references;
  keep `stage(c)` when it means the process instance. For example, in
  `requirements R { const c = 2 process Case { ... } }`, the generated init
  rejection loc is the **process declaration**, not the const token or an
  unwritten binder token. Relation-parameter refusal uses the source event.
  Hidden KPI projection binders are different: on a const collision only,
  their internal binder is freshened; a const `c` need not be renamed merely
  because a KPI's implementation previously used `c`.
  Def-call argument binders are checked before argument expansion, so an
  ignored, retained or duplicated parameter cannot change the refusal's
  message/owner. With `const N = 2`, `ignore(forall N in 0..1 { true })`
  is rejected even if `def ignore(p: Bool) = true` discards the argument.
  The retained original inline fixture inventory (shared consumers counted once)
  contains **126**
  compose-capture forms (#1442: 63 standalone/composed cells), e.g.
  `const x = false` with `action go(x: Bool) { requires x }`, including nested
  binders; **24** nested-aggregate forms (#1463, shared CLI/LSP/Worker source),
  e.g. `const x = false` with `sum(x in 0..1 of count(y in 0..x)) == 3`;
  **2** per-binding helpful-argument forms (#1333: standalone/composed), e.g.
  `const c = 0` with `forall c: Case`, action parameter `c`, `helpful step(c)`.
  Total: **152** inline removal forms; the 126 fresh compose controls remain
  `ok`. Rename the local declaration and its uses, not the constant merely to
  hide the collision. Five partial-init sources (#1481) are **error→error**,
  not acceptance removals: outer `flag`/`limit` binders over same-named consts
  now report the located const-shadow error before the former ownership error.
  This changes diagnostic priority; it does not make partial init valid.
  Additional removals include the implicit-name cases above and nested init
  `const i = 1` with `forall i in 0..0 { forall j in 0..i { m[j] = true } }`.
  The nested init fixture checked on base `46031415` but now reports the outer
  forall owner at **5:10**. Rename the local and its bound uses; dependent-range
  execution limits remain separate. Native `check` at base `46031415` versus
  measured ancestor `65bf2876`, each in its own archive, found **0** removals
  and **0** other output differences in **603** corpus forms, including
  `issue_1425/interleaved.fsl` and three added `issue_1547_guard_reservations/`
  fixtures and two added `ai_project_*_invalid_rule.fsl` fixtures. The original
  inline counts remain 152
  removals, 126 fresh accepted and 5 diagnostic changes. Another 70 removal
  witnesses cover implicit/nested sources, raw arguments and unused definitions;
  70 renamed controls and both hidden-KPI cases remain accepted. The measurement
  ancestor has identical Rust implementation and corpus sources to this docs
  update. The design note records the full comparison; it does not enumerate
  every removed program.
  This expression rule does not promise executable `leadsTo` wrapper binders:
  `check` rejects a collection domain or any `where` filter on an outer
  `leadsTo` binder. Unfiltered supported typed/range wrappers remain available,
  and collection quantifiers inside P or Q are not wrapper binders. This is a
  breaking removal (#1463) of forms that previously checked but no verification
  engine could execute, aligning the reference with the accepted temporal and
  induction designs' fail-closed boundary; it does not implement those forms.
- Quantification (bounded): the canonical forms are `forall binder { expr }`
  and `exists binder { expr }`. The 2.x legacy colon/no-braces spelling such as
  `forall i in lo..hi: expr` remains accepted but is non-canonical. A Seq binder
  visits its live prefix in position order and preserves duplicate values.
- Aggregation: `count(binder)` and `sum(binder of value)`. Examples include
  `count(x: T where p)`, `count(x in queue where p)`,
  `sum(x in queue of x.amount where p)`, and range equivalents. Empty domains
  produce `0`; Seq duplicates contribute once per live position, while Set
  membership contributes once per distinct member.
  Public Kernel v1/v2 export preserves these actual candidates through `add`,
  relation projections and collection `let` aliases, using existing scalar
  captures rather than enumerating only the declared carrier. Later shadowing
  or state updates do not change a let's captured candidates. `min`/`max` around
  a sum remain binary arithmetic; no new aggregate JSON shape is introduced.
- Cardinality predicates: `unique(binder)` / `exactlyOne(binder)`. `unique`
  means at most one matching binding, while `exactlyOne` means exactly one.
- Option: `x == none` / `x != none` / `x == some(e)` / `x != some(e)`.
  Equality is structural: `none` equals only `none`, while two `some` values are
  equal exactly when their payloads are equal. `x is some(v)` remains the form
  that binds `v`; equality never introduces a binding. The binding is available
  only in the logical continuation where the match is true, such as the
  right-hand side of `(x is some(v)) => ...` or `(x is some(v)) and ...`; it is
  not a global binding. Only `and` exports bindings from a true whole expression;
  consequently only conjunctive `requires` matches export bindings to later
  guards and the action body. Breaking removal (#1547): `check` now rejects
  previously accepted uses on the right of `or`, after an `or` or implication
  on the left of `=>`, across comparison operands or out of comparison results,
  and disjunctive flow into quantifier/aggregate bodies and filters. Outer escapes
  from negation, quantifiers and aggregates, and condition-to-`if`-arm bindings,
  were already rejected on base; those are preservation controls, not removals.
  Collision reservation is separate from visibility: a name may forbid
  redeclaration without being readable. Breaking removal (#1573): matches inside
  `not` and expression `if` reserve their names for later operands, guards,
  action statements and ensures. An expression-if condition reserves names in
  both arms, and its **then arm also reserves names in the else arm** because
  the symbolic evaluator shares their binding map (#1577). A quantifier/aggregate
  `where` reserves names in its body or `of` expression, when present; count,
  unique and exactlyOne have only a filter, not an independent body/of.
  A statement-if condition reserves names in both arms and later statements;
  bindings inside either arm also reserve names in later statements. Opposite
  statement arms remain independent, and candidate-local names do not escape
  a quantifier/aggregate. These statement rules apply in init and action bodies;
  direct-spec init validation shares the reservation walk across all init blocks
  and inline initializers, preserving its initializer/const/map-key diagnostic
  owners. Compose has a known check gap across same-source init fragments and
  inline→init, and common init can inherit an unrelated source's Fresh decision.
  These programs remain illegal; their false verified results belong to #1622,
  while compose init-fragment reservations will be tracked in a separate issue
  filed after merge. This is not a general composed-init checking guarantee.
  Rename the later binding, for example change
  `(p is some(v)) and v == 2` to `(p is some(w)) and w == 2`.
  None of these reservations grants additional read visibility. This includes
  four breaking-removal routes from the earlier #1442 nested-if examples
  (standalone/compose): retain the original rejection, and rename only the inner
  `u` binding to `w` for the accepted preservation control.
  For sync actions and non-init paths, compose separates such bindings across
  independent sources; rendered guards
  may now use `__v_substitution_0/1` instead of the authored `v`. The explain
  result and ordinary verification verdicts of the guard-name regression cases
  are unchanged. For two sources whose statement payloads were incorrectly
  shared, check remains accepted but the previously false `violated` verdict
  becomes holds after separation; a genuinely false payload condition remains
  violated. These are checker/hygiene compensations, not an evaluator repair
  (the remaining binding-lifetime work is tracked in #1622). Independent-source
  init pattern separation is not repaired here and remains part of that work.
  Out-of-scope `v` is a `kind: "semantics"` error (exit 2), with message
  `invalid model expression: public Kernel cannot type identifier 'v' at <line>:<column>`.
  The location identifies the clause or `if` arm containing the use. Guards and
  postconditions retain their action-level fallback, but a more specific arm
  span can take precedence. An earlier newly illegal reference can replace a
  later collision message while retaining exit 2 and kind semantics.
  Expression-if arm inference can report an unbound arm reference before a
  condition's freshness error; the source remains rejected, not reinterpreted.
  A free name in a `def` body that only an out-of-continuation pattern would
  capture is now reported as an unbound reference at the containing clause,
  instead of a predicate-capture error. This diagnostic class changes messages
  and possibly locations, but retains exit 2 and kind semantics.
  Repair the undefined `(o is some(v)) or v == 1` according to the intended
  treatment of `none`: if absence is allowed and a present payload must equal 1,
  write `(o is some(v)) => v == 1`, equivalently
  `o == none or (o is some(v) and v == 1)`. Using `and` alone instead rejects
  `none`; no equivalence with the old undefined expression is implied.
  Measured removals and diagnostic examples are recorded in the
  [scope design](../design/DESIGN-pattern-binding-scope.md#3-true-continuation-enforcement-1547).
  The [reservation amendment](../design/DESIGN-pattern-binding-scope.md#5-retained-pattern-reservations-1573)
  records the additional removals and source-separated output contract.
  `v` must be a fresh name: `check` rejects a `v` that is
  already a state variable, constant, enum member, action parameter, quantifier
  binder, `let`, or an earlier pattern binding in scope, and asks you to rename
  it. Ordering is not defined.
- struct: literal `Order { st: Open, qty: 0 }`, field reference `o.st`,
  `==` is field-by-field equality
- Set/Seq expression materialization (Refs #1467): admitted scalar/enum-element
  literals use the checked model's inferred/expected type in symbolic methods,
  comparisons, binders and `let`. Context reaches both conditional arms before
  construction; equality also obtains it from the opposite operand, including
  empty-left forms. Inferred `Int` temporaries retain members, slots and length;
  finite-carrier, Seq-capacity and storage-bound rules below are unchanged.
  The state/storage whitelist is not a blanket rejection of composite expression
  literals: some pass `check` while symbolic implementation remains incomplete.
  Existing deciding composite Seq paths are preserved; remaining composite and
  relation-valued temporary gaps stay tracked by #1467, not permanent engine
  exceptions. See the [materialization decision](../design/DESIGN-typed-collection-materialization.md).
  Historical `FSLC_CACHE=off <binary-for-SHA> check <form>` in each own tree checked
  606 corpus forms and 3 unique rewritten-test inline forms at each of
  `a52bef4931adc67ea2b1fdca827e2318b4ae25ab`,
  `e201c0d66834ed8c860be84e96876690487b11af` and
  `cc7a538c3b647c1c3bed5bf2088551806442c794`: both base->cc7 ok->error sets were
  empty. This is not later b8 integration validation or whole-corpus engine agreement.
- Set: `Set {}` / `Set { 1, 2 }`, `.add(e)` `.remove(e)` `.contains(e)` `.size()`
  An accepted `add` retains a computed member outside the declared carrier:
  `contains`, `size`, removal and equality observe it in the temporary value.
  Assigning the result back to that bounded receiver reports `type_bound` /
  `_bounds_<var>` instead of dropping the member or transition. Scalar argument
  typing is unchanged; this does not enable unbounded Set storage.
  **Breaking removal (#1472, #1627):** whole Set, Seq, Map and relation values
  are compatible only with equal corresponding carriers. Numeric aliases with
  identical lower/upper bounds remain compatible; Seq capacity must also match,
  and Map requires both key and value carriers to match. Assignment, comparison,
  conditional arms, projections and whole refinement maps share this rule.
  Both wider-to-narrower and narrower-to-wider whole Set assignments now fail
  check with the existing located assignability error (`semantics`, exit 2),
  shared by every verify engine, rather than failing later in symbolic coercion.
  Scalar/Option numeric compatibility and add/contains/push arguments remain
  unchanged. `Int` has no finite carrier, including an inferred literal element:
  Set/Seq/Map uses involving it retain prior recursive compatibility (and inferred
  Seq capacity). An uncontextualized Set/Seq conditional retains an already
  known finite arm's carrier/capacity through a `let`, in either arm order;
  that inference alone does not materialize literals. Refinement resolves numeric types in each
  model independently. #1599's shared Type-kind gate rejects same-name declarations
  before map validation. Other map errors use authored recovery (#1642),
  retaining unknown 1:1 only without an authored span. The historical
  map anchors from #1472 are runtime 1:40/1:45
  (`map`), standalone 4:3 (`map`) and inline 2:3 (`implements`), formerly 1:1.
  Measured base `79687655` vs `3c99c93b`, each built in its own archive, twice
  checked 606 corpus +16 original inline +18 literal-boundary +18 bridge forms:
  no corpus removals, zero error→error diagnostic changes, and nine ok→error forms.
  Three original inline removals are:
  `FALSE_VACUITY` / `CondMap` (Map keys 0..0 vs 0..2, 20:41), #1522 `MAP`
  (Map keys 0..2 vs 0..1, 7:35), and `SET_WIDENING` (Set 0..2 to 0..4, 6:48).
  Also removed are admitted whole Seq maps in runtime `SeqWide`→`SeqAbs`,
  runtime `SeqWider`→`SeqGuarded`, CLI standalone `SEQ_WIDE_IMPL`→`SEQ_ABS`
  and inline `seq_req(-1)`: capacity 2, model-local elements -1..2 or 0..3 vs
  0..2; former runtime refinement failures now reject statically at the authored
  locations above. The six additional breaking removals are conditional/let
  copies in both arm orders: Set0..2→0..4, Seq0..2→0..4 at capacity2, and
  Seq capacity2→3 with equal element carrier (all 8:3). Standalone/same-carrier
  controls still check; the 18 original literal-boundary outputs are unchanged.
  See the [single carrier decision](../design/DESIGN-finite-container-carriers.md)
  for commands and the full removal inventory. Remaining literal gaps stay tracked by #1467.
  In mutate, a statically ill-typed implements map is `invalid`, using the shared
  check-invalid classification, not an oracle error. Thus whole-container map
  mutants that change a numeric range move from `errored` to `invalid` in the
  summary; they are excluded from `kill_rate` and do not add `oracle_errors` to
  the gate. Solver/evaluation failures remain errored and budget cutoffs remain
  inconclusive; the same typed map validator serves auto/explicit maps,
  standalone refinement, governance preflight and composed models.
  Symbolic init preserves introduced members through copies, copy chains and
  conditions in the checked dependency order, without replacing relational init
  with sequential execution. In BMC, re-framing a root whose bound is omitted
  requires proof that no live key escapes the new frame's finite support; the
  re-framing check does not assume that omitted bound. Genuinely growing support
  continues to be retained.
- Seq: `Seq {}` / `Seq { 1, 2 }`, `.push(e)` `.pop()` `.head()` `.at(i)`
  `.contains(e)` `.size()`, `==` is equality of length and all elements
- Relation: `r.contains(a, b)`, `r.add(a, b)`, `r.remove(a, b)`,
  `reachable(r, a, b)`, `acyclic(r)`, `functional(r)`, `injective(r)`,
  `domain(r)`, `range(r)`. `reachable` and `acyclic` require a self-relation
  (`relation T -> T`); endpoint type/arity errors include repair hints.
  Accepted relation adds likewise retain pairs outside either endpoint carrier;
  domain/range and graph operations see those pairs. Storing the add result back
  to that bounded relation reports its `_bounds_<var>` violation.
  Breaking removal (#1522, related #1524): relation values are compatible only
  when both corresponding endpoint carriers agree. Numeric aliases compare their
  resolved lower and upper bounds, not their names: `N = 0..2` and `M = 0..2` remain
  compatible, but `A = 0..1` and `B = 0..2` do not. Conditional arms, Set/Seq
  items (including nested items), and assignments mixing these relation carriers
  now fail `check` with a located assignability error (`semantics`, exit 2),
  shared by `verify` and `replay`. Ordinary scalar Int/Range compatibility,
  nominal enums, and the declared self-relation gate are
  unchanged. Equal-bound relation literals still check, but their remaining
  symbolic engine support belongs to #1467; this is not key-union support.
  The shared gate also rejects action assignment `r = q`, init
  `q = Set {} r = q`, `let x = q` followed by `r = x`, and mixed relation
  conditionals in a let. A plain alias without an incompatible use stays valid.
  Narrow-to-wide relation copies are also refused in action assignment, init
  and let uses: previously BMC/induction could not coerce these forms while
  explicit decided action/let and Monitor agreed; init also failed explicit.
  #1472 extends this rule to whole Set, Seq and Map values, including
  narrow-to-wide Set copies (#1627); see the carrier decision above.
  See [the compatibility decision](../design/DESIGN-relation-endpoint-compatibility.md)
  for the breaking-removal evidence.
  For `r: relation A -> B`, `contains`, `add`, `remove`, and `reachable`
  check `a` against `A` and `b` against `B` independently. Incompatible
  endpoints are check-time `kind: "semantics"` errors (exit 2), with the
  assignability message followed by `hint: use endpoints matching the relation's
  declared source and target types`. Named enums remain nominal; numeric ranges
  retain their existing integer compatibility. For example, enum `Red` cannot
  be an endpoint of `relation N -> N` when `N` is a numeric range. This breaking
  removal (#1458) rejects previously accepted ill-typed expressions rather than
  assigning them a runtime meaning. `domain(r)` now has type `Set<A>` and
  `range(r)` has type `Set<B>`, rather than `Set<Int>` for every relation.
  Collection binders over these projections and their public Kernel projection
  and binder annotations inherit those endpoint types. Enum-typed uses of
  `domain` and `range` are accepted, including
  `not (exists x in domain(r): reachable(r, x, x))`; this does not change their
  runtime meaning or guarantee that their invariants hold. Numeric projections
  retain integer compatibility. The restacked boundary sweep used BASE
  `9af1fa5f36f91e415b7ba249888f4ada4cf7a0db` and HEAD
  `3a8a6019a5bed3ae62f960afedf2bdcf6d129341`: all 597 corpus forms have a
  **measured empty** `ok` → `error` transition set. All 134 inline fixture
  occurrences were also checked; #1458 removes 24 occurrences (22 distinct
  sources) in both modes, 48 mode-form transitions. The previous 46-transition
  subset is unchanged; the expanded inventory counts numeric Set assignment
  below. Three counted forms (`count`, enum set-literal equality and matching
  enum Set assignment) add six `error` → `ok` pairs. Both modes ran twice in
  each tree: 5,848 checks, full outputs and no exclusions. On this base, the
  source-only revert calibration produces 28 intended failures and 24 preservation
  passes, followed by exact restoration and 52 passing fixed tests; workspace
  Clippy is warning-free. The workspace suite passed 3,468 tests on
  `fc77124b31744f1c1728295db6f09508265b8285`, carried across only the
  whitespace-only commit to the measured HEAD, with whole-file equality after
  removing whitespace. Builds, CLI conformance, fixed tests twice and formatting
  pass on that HEAD. Compared with the earlier `8e54e862` measurement, main's
  #1463 now rejects `explicit_init_state_collection.fsl` on both BASE and HEAD
  and adds the authored location to the BASE count diagnostic; neither changes
  the #1458 removal/addition sets. See
  [the design note](../design/DESIGN-nested-option-support.md#relation-endpoint-expression-boundary-1458)
  for commands, identities and measurements. The endpoint breaking removals are:
  for numeric `N -> N`, `not reachable(r, Red, Blue)`,
  `not reachable(r, Red, 1)`, `not reachable(r, 1, Blue)`,
  `not r.contains(Red, Blue)`, `not r.contains(Red, 1)`,
  `not r.contains(1, Blue)`, `r.add(Red, Blue).contains(Red, Blue)`,
  `r.add(Red, 1) != r`, `r.add(1, Blue) != r`,
  `r.remove(Red, Blue) == r`, `r.remove(Red, 1) == r`,
  `r.remove(1, Blue) == r`; for `N -> Color`, `not r.contains(1, 2)`,
  `r.add(1, 2) != r`, `r.remove(1, 2) == r`.
  `Red`/`Blue` are `Color` enum members. The additional LSP and Worker
  occurrences repeat the numeric target-only `contains` and `reachable` forms.
  Projection typing also removes these previously accepted enum/Int forms for
  `s: relation Color -> Color` and `r: relation N -> N`:
  `forall x in domain(s): not r.contains(x, x)`,
  `forall x in range(s): not r.contains(x, x)`,
  `not domain(s).contains(0)`, `not range(s).contains(0)`,
  `forall x in domain(s): x != 0`, `forall x in range(s): x != 0`.
  The expanded inventory also rejects `xs = domain(s)` when
  `s: relation Color -> Color` and `xs: Set<N>` (assignment origin 8:19).
  Enum projection elements cannot be used as numeric endpoints or compared with
  integer values; use the declared endpoint type, not an enum ordinal.
  More generally, projection elements and whole projection sets take the declared
  endpoint type: enum or Bool elements cannot be used wherever `Int` is expected;
  their sets cannot be used wherever a numeric Set is expected. This breaking rule applies
  to comparison, ordering, arithmetic, `min`/`max`, membership, set-literal
  equality, Option comparisons and pattern payloads, assignment to numeric Set
  state, and refinement maps; the listed measured removals are not exhaustive.
  Newly accepted families include `count` over finite endpoint-typed domain/range
  projections, `sum` over numeric projections, enum set-literal equality,
  enum-typed `where` filters and conditional projection membership, `let`-bound
  projections consumed as matching typed sets, assignment to a matching enum Set,
  and refinement maps into a matching enum Set. `unique` over enum domain/range
  was already accepted on released main and remains accepted. Acceptance
  does not imply that an invariant holds. The preserved enum-binder/cycle idioms
  are `not (exists x in domain(r): reachable(r, x, x))`,
  `forall x in domain(r): not r.contains(x, Blue)`, and
  `forall y in range(r): not r.contains(1, y)`. They already checked on released
  main; they are not new additions.
  `reachable(r, a, a)` is **not** reflexive by construction: it is true only
  if a real path of one or more edges leads from `a` back to `a` (so an empty
  or acyclic relation gives `reachable(r, a, a) == false` for every `a`). A
  reflexive reading would make `not reachable(r, a, a)` unsatisfiable by
  construction for any inhabited domain, draining the predicate of meaning;
  use `acyclic(r)` to state "no self-loops or cycles anywhere in `r`".
  A relation is not a Map: `r[a]` and indexed assignment `r[a] = b` are rejected
  by `check`, even when `functional(r)` holds. Use the relation methods instead.
  This is a breaking removal (#1463) of previously accepted reads and writes
  without executable index semantics; Map indexing is unchanged.
- Inside ensures / trans only: read the pre-transition state with `old(expr)`
- Inside a leadsTo block only: `P ~> Q` (response property. not part of the operator hierarchy of general expressions);
  optional `within K` before Q for a bounded deadline, and optional
  `decreases <int expr>` after the response body for induction ranking

Static operand/signature checking (#1459, **breaking**) also visits unselected
branches, action assignments, `requires`/`ensures`, and trace expressions:

- `is none` / `is some(x)` require an Option operand; fresh payload binding and
  continuation scope are unchanged.
- `abs` requires an integer-compatible operand, including when its argument is
  a domain/range element. Enum ordinals and Bool values are not integers.
- `acyclic`, `functional`, `injective`, `domain`, `range`, and `reachable` require
  a relation. Non-relations no longer acquire an invented `Set<Int>` projection.
  `reachable` and `acyclic` require identical declared endpoint types (`T -> T`):
  mixed enum/numeric relations and `N -> M` are rejected, even if numeric `N`
  and `M` have identical bounds. Numeric compatibility does not establish a self-relation;
  heterogeneous `functional`/`injective`/`domain`/`range` remain supported.
- Method receivers, names and argument counts must match the runtime signatures:
  Set `contains(x)`, `add(x)`, `remove(x)`, `size()`;
  Seq `contains(x)`, `push(x)`, `pop()`, `head()`, `at(i)`, `size()`;
  relation `contains(a,b)`, `add(a,b)`, `remove(a,b)`. Map has no methods.
  This rejects, for example, `n.contains(0)`, `s.push(0)`, `q.add(0)`,
  `r.size()`, `s.contains()`, `q.pop(1)`, and `n = q.at()` in an action body.
- `count`/`sum` validate the original binder, Bool filter and integer sum value
  before expansion, including empty ranges and zero-capacity sequences.
  `count(i in 1..0 where 1)` and `sum(i in 1..0 of true)` are now errors;
  well-typed empty aggregates still return `0`.

These are located loader errors (`kind: "semantics"`, exit 2), not deferred
runtime or acceptance-replay errors. Existing endpoint-type hints, valid method
behavior and non-reflexive reachability are preserved. See the
[design amendment](../design/DESIGN-nested-option-support.md#static-operand-and-signature-boundary-1459).

The #1459 boundary audit compares BASE `e8cf78f0` with HEAD `33d7faa7` using
591 corpus files and 384 inline occurrences (975 forms), in stable and fsl5 modes
twice in each of three independently built trees: baseline, immediate parent
and head, for 11,700 checks. **No corpus form stops checking, and no other corpus
output changes; no fields or forms are excluded.** The immediate-parent
comparison attributes **248 mode-form removals (124 occurrences, 123 distinct
sources)** and **60 diagnostic reclassifications** to the six #1459 categories
above, including numeric `N -> M`. The latter remain `error/semantics/2` but now
have authored static locations rather than unlocated runtime errors. Inherited
#1439 contributes 86 removals and 11 reclassifications; inherited #1458 contributes
56 removals and six additions in this expanded inventory. These are not new
#1459 changes; the design amendment records the full attribution and controls.

Review round 1 closes a further self-relation selection gap: both branches of a
conditional relation operand are checked, even when unselected, including nested
conditionals, `old`, relation add/remove receivers and action-local let aliases.
Inference cannot hide an `N -> M` branch behind a self-relation then branch.
Projection-inference errors retain the authored enclosing or more precise inner
origin; empty literals do not replace the substantive relation diagnostic.
Range-bound errors and terminal errors also use their authored origins, not a
generated 1:1 location. Numeric/Set compatibility and valid dynamic verdicts
are unchanged. These claims concern the expression trees visited by the shared
validator. The historical round-1 baseline checked helpful action existence and
arity only; main's #1527 now also checks parameter types, and that behavior is
preserved. Unsupported relation-valued temporary collections remain separate
upstream work, not newly supported by this fix.

The round-1 delta compares BASE `33d7faa7` with HEAD `8caf5c2e`: **42 additional
mode-form removals (21 source occurrences)** for non-self selections, and **24
diagnostic reclassifications (12 occurrences)**—projection inference 16,
range-bound origins six, terminal origins two. The latter stay
`error/semantics/2`. The expanded audit retains 591 corpus files and 447 inline
occurrences (1,038 forms); both modes twice across four own-built trees give
16,608 checks, including the separate previous-fix baseline. All 8,304 repeat
pairs agree in full; no fields/forms are excluded and no corpus output changes.
Inherited/R0 changes are separated from R1 in the design amendment.

The current integration audit uses final base
`11f01225e244dabde0f8dfd9aaa665bc47dccfd4` and measured implementation
`2b7d2e98349505af7dccb7843e41ab659066f021`. Checks were measured on
`b1557a31b77f549d2c48823c3435217f9df4fc53` and carried to `11f01225`, whose
tree differs only by the two generated language pages. `fslc check FORM`, with
cache off and stable/fsl5 each repeated twice, covers all **597 corpus forms**
under `specs/`, `examples/`, and `rust/fslc/tests/fixtures/`, plus **601 inline
inputs**: **1,198 inputs**, **14,376 checks** across the three measured states,
and 24 binary-identity controls. The third state is historical pre-#1458 baseline
`9af1fa5f`, used for inherited attribution. No corpus output changes; no form or
output field is excluded. Relative to final base, #1459 removes **294 mode-forms
(147 occurrences, 142 distinct sources)** and reclassifies **62 diagnostics**.
Main #1463 already rejects 24 original empty-aggregate occurrences and locates
two range-bound occurrences, so their former 48 removals/four reclassifications
are no longer attributed to #1459. New let/Kernel/CLI/shared-row occurrences add
26 removals. Historical #1458 attribution remains 48 removals/six additions on
its 134-entry inventory, or 56/eight in this expanded inventory.

This includes the breaking removal `X1/operand-method-types` from the shared
#1463 inventory: `done: Bool; invariant Capture { acyclic(done) }` changes from
`[Ok, Error, Error, Error]` to `[Error, Error, Error, Error]`, with a located
check rejection at **8:3**. It is the only changed shared row. For the remaining
136 row sources and their one resolver dependency (**137 inputs**), the same
`fslc check FORM` command at the named base and head produces no transition or
other output change in either mode/repetition. The inherited-output comparison
retains 292 embedded-input-path differences and 16 observations from two prior
#1458 entries mis-extracted as scratch labels rather than specs; this audit uses
their real test sources, accepted on both baseline binaries, without changing
attribution. The optional replay was not run; no sweep output was normalized.

Top-level temporal sugar:

```fsl
unless Name { P unless Q }   // while P holds and Q is false, the next state must keep P or make Q true
until  Name { P until Q }    // unless safety plus a leadsTo P ~> Q progress obligation
```

Use this sugar for persistent workflow states such as "held until released" or
"pending until completed". For arbitrary history facts, use an explicit ghost
variable.

## 4. Statements (init / action bodies)

Init assignment values/keys, if conditions and forall filters/bodies may read
a root assigned anywhere in init only after complete definite assignment
(#1451). Forall sources keep their separate existing source/domain rules.
Check rejects an early read with
`init references state variable '<name>' before it is assigned`, kind
`semantics`, exit 2, locating the containing statement; ordinary verify entries
return the same refusal before selecting an engine. Coverage is root-wide:
every Map key/struct field and both branches must be covered. Even a
short-circuited RHS, an already-written key/field, or a free component/path of
a partially assigned root is rejected. In particular, `struct-other-partial`,
`if-one-branch-read`, and #1481 N2 reject reads of values that would otherwise
be free under #493. Existing ownership/field/forall refusals keep priority.

A root init never assigns remains a symbolic free value, including the issue's
`init { g = on }` and `if choose { x = 1 } else { x = 0 }` with unassigned
`on`/`choose` (#493). Check still accepts them; BMC/induction retain their
symbolic free values, and refinement's free-root enumeration is unchanged
(#1521). Explicit and Monitor still refuse these reads, but now distinguish
unsupported nondeterministic init from assigned-root early reads with
`the explicit engine does not support a free initial value of state variable '<name>' (nondeterministic init); use --engine bmc`
(kind `semantics`, exit 2; compose prefixes are retained). Auto still falls
back to BMC. For non-error BMC results, `engine_fallback.reason` carries
that diagnostic; BMC error results still omit fallback metadata.
This is a message-only change, not new explicit enumeration or check rejection.
Only when binder values are unresolved, including filtered/collection
forms (#1520, X1/X2), does the #1518 current-key rule exempt an already-written
current key from this root-wide check. A resolved partial domain still requires
full root coverage; a typed full-domain forall remains accepted.
Binder-keyed writes under filtered/collection binders, including a nested such
binder, do not prove full coverage for a subsequent read outside the loop.
For example, replace `forall k: K where true { m[k] = true }` followed by
`g = m[0]` with `forall k: K { m[k] = true } g = m[0]` for a finite `K`.
Unread omitted roots and legacy-accepted partial init remain
accepted; check does not require a deterministic initial state.

Breaking-removal sweep (#1451): `fslc check <form>` in each revision's own
materialized tree, base `c79016efeb630c717292e9392901cfe65ab40915` versus
head `2d05dd63f05ca68c4bd7d06c3b24ba147ce6eda4`: all **597** `.fsl` files under
`specs/`, `examples/`, and `rust/fslc/tests/fixtures/` have identical full outputs
and exits (400 exit 0, 191 exit 2, 6 exit 1); corpus `ok`→`error` set **empty**.
Separately, 42 inline sources, each direct and composed (84 forms), have **38**
`ok`→`error` transitions: the original 13 cases × 2, #1481 N2 × 2, two static
subset cases × 2 and three post-loop coverage cases × 2; no other output changes. The
[migration table](../design/DESIGN-bridge.md#assigned-root-read-order-1451)
also covers the added static-subset and post-loop coverage families. Its list is
scoped to the measured inline fixtures, not every possible removed program.
Move a read after complete assignment or explicitly initialize the whole root;
choosing a formerly free value narrows the model and requires a design decision.

- Assignment: `x = expr`, `m[k] = expr`, `m[k].field = expr`, `o.field = expr`
- Updating a Set/Seq/relation uses the **reassignment idiom**:
  `s = s.add(x)`, `q = q.pop()`, `r = r.add(a, b)`
- A relation-typed field's **literal** initializer must be the empty `Set {}` — a non-empty
  Set literal is rejected in both `init` and an inline initializer — and pairs are added with
  the reassignment idiom `r = r.add(a, b)`, which is also allowed inside `init`.
- `if expr { stmt... } else { stmt... }` is allowed in both `init` and action bodies
  (can be nested with an if inside the else)
- `forall x: T { stmt... }` (bulk initialization / bulk update)

An `init` statement's collection `forall` source must not contain a free state
reference. `check` rejects `forall x in xs { ... }` when `xs` is state, even
after an assignment to `xs`, for an empty domain/body, or in an unreachable
branch. This breaking removal (#1463) does not itself prohibit assigned-state
reads in init expressions, filters, or bodies; those reads must also satisfy
the same-forall root-read boundary below (#1518). A local binder
that shadows a state name is not a state reference: its source/bounds are read
in the enclosing scope, and only its filter/body/value uses the new local scope.
State-independent collection literals remain allowed. Correcting this lexical
distinction also removes the old symbolic/explicit false rejection of valid
shadowed names; that fix does not narrow `check` acceptance.

Within an init statement `forall`'s filter or body, reading a state root that
the **same forall writes** is rejected (#1518), including assignment keys,
right-hand sides, and `if` conditions. The exception is a location at the
current binder key that the body has already written in this iteration:
`m[k] = value` permits later `m[k]` and `m[k].flag` reads; `m[k].flag = true`
permits a later read of that field, not the whole entry or another field.
Existing ownership restrictions still apply. For example,
`forall k: K { m[k] = true seen[k] = m[k] }` is allowed, but using `m[1]`
instead of `m[k]`, or reading before the write, is rejected. Reads of a
different root initialized before the forall remain allowed. This is a
**breaking removal** of previously check-ok scalar-map and whole-entry forms.
The semantics message identifies the root, not the individual read expression;
check and verification locate the containing statement, while legacy replay
does not require a location.

This removal deliberately includes conservative cases, without a per-key
dependency proof: `forall k in 0..0 { m[k] = true }` followed by
`forall k in 1..1 { m[k] = m[0] }` is rejected even though the preceding
forall initialized the other key, because the second forall writes the same
root it reads. Likewise,
`forall outer in 0..0 { forall k: K { m[k] = true seen[k] = m[k] } }`
is rejected even immediately after the inner write, since the outer forall
also writes `m` and the read is not at its binder key. Explicit and Monitor
already refused these forms on the base with `before it is assigned` and
`nested forall in init is not supported`, respectively. #1518 also rejects
them at check; it does not refine the rule to admit them.

The rule is root-based, not limited to iteration-order disagreements: with
`m` indexed by `0..1`, `m[0] = true forall k in 1..1 { m[k] = true x = m[0] }`
is also rejected even though all engines agreed on this correct init on the
base, because the loop reads a previously initialized other key of `m`.

For deterministic concrete init, a forall with an empty or not statically
provable nonempty domain contributes no definite coverage for its fixed
targets. A static filter such as `where k == 0` over `0..1` can establish
nonemptiness; a state-dependent filter cannot. Write ownership is still
checked. Missing deterministic assignments are rejected by explicit and
Monitor (exit 2), not filled with defaults. Symbolic init keeps unwritten
components free: its verdict follows those free values (type-bound/invariant
violations exit 1, or verification if every admissible value satisfies the
property). Auto may fall back to BMC for this unsupported explicit init.
This is also a conservative concrete-support breaking removal: after
`flag = true`, a fixed-target write under `where flag and k == 0` is refused
by explicit and Monitor with `init does not assign` even if the runtime domain
is nonempty, rather than producing an incorrect verdict.

Explicit, Monitor and refinement resolve state-independent filtered type/range
and collection binder domains for init key coverage (#1520). A current-key read
after its write is supported when the selected keys establish full-root coverage;
`--engine auto` then uses explicit without fallback. Empty domains contribute no
coverage, partial domains cover only selected keys, and state-dependent filters
remain unresolved. The check-time read-order rule is unchanged.
Refinement now refuses nested fixed-target initialization depending on an outer binder
that shadows a same-named constant, previously incorrectly accepted as `refines`.

Breaking-removal sweep (#1518): base
`28c4eab710f8d83e766bb0e7c7e006d09d7777d8` versus head
`9d94c5cb5447834f329002200afa0f3404d9a9a0` plus the production runtime change
(`explicit.rs` SHA-256
`b20b92e9945fd7a05eb8462c4a7ad3816fcd06cd2c34a871910b7b652090c17b`).
The coordinator built each binary in its own git-archive materialized tree
and ran `fslc check <form>` on all 597 `.fsl` forms in `specs/`, `examples/`,
and `rust/fslc/tests/fixtures/`: 397 `ok` in each tree, **0 `ok`→`error`,
0 `error`→`ok`, 0 other changes**. The corpus removal set is empty; A1–A5 pin
five regression forms within this removal, which also includes the
conservative examples above.

A nested statement `forall` in `init` remains supported by `check` and the
symbolic engines subject to the read-order and ownership rules above, but
`--engine explicit` rejects nested init `forall`.
`--engine auto` routes this documented explicit limitation to symbolic BMC;
it does not extend explicit execution or make state-dependent collection
sources acceptable.

Breaking-removal evidence (#1463): base `5738cc2c711b691a9d453b0190ead1a2ff77fc50`
and the #1463 change (exact measured head recorded in the PR), each in its
own materialized tree, ran `FSLC_CACHE=off fslc check <form>` over 597 corpus forms
and 1016 inline IDs (1613 total) twice. Both runs exited 0; their full reports
were identical without field exclusions: 110 new input errors, 1 corpus and
109 inline. Both added legacy-return fixtures were included and unchanged;
the full measurement was retaken on this base. Acceptance means result is not `error`;
refinement/trace verdicts with exit 1 are not syntax/semantic input rejection.
The complete breaking list follows; braces mean every Cartesian choice.

| Removed check acceptance: form ID set | Count |
|---|---:|
| `corpus/rust/fslc/tests/fixtures/explicit_init_state_collection.fsl` | 1 |
| `inline/1457/init-stmt/{empty-body,empty-domain}/{Set,Seq}/state/{positive,negative}` | 8 |
| `inline/L1/{Set,Seq}/{state,literal}/{false,true}` | 8 |
| `inline/L1/{utf16-prefix,resolved-component}` | 2 |
| `inline/L2/{z: K,z in 0..1}/{z == 1,not done,true,false}` | 8 |
| `inline/L3/z in Set { 1 }` | 1 |
| `inline/L3/z: K where z == 1` | 1 |
| `inline/I1/{Set,Seq}/{false,true}/{plain,conditional,unreachable,nested}` | 16 |
| `inline/S4/{own-source,sibling-restored,aggregate-source}` | 3 |
| `inline/R1/read/{init,action,invariant,trans,leadsTo,reachable}/{false,true}` | 12 |
| `inline/R1/write/{init,action}` | 2 |
| `inline/R1/receiver/{old,conditional,domain-binder}` | 3 |
| `inline/guard/{StateCollection,ranked-where}` | 2 |
| `inline/aggregate-and-component-locations/case-{00,04,08,12,16,20,24,28,32,36,40,44,48,52,56,60,64,68}` | 18 |
| `inline/aggregate-and-component-locations/case-{01,05,09,25,29,33}` | 6 |
| `inline/aggregate-and-component-locations/case-{72,73,74}` | 3 |
| `inline/aggregate-and-component-locations/unchecked-init/state-source` | 1 |
| `inline/nested-aggregates-and-sync-ownership/case-{68,69,70,71}` | 4 |
| `inline/nested-aggregates-and-sync-ownership/case-{76,77,78,79,80,81,82,83,84,85,86}` | 11 |

The 110 removals are 22 wrapper forms, 31 init-source forms, 51 relation-index
forms (18 in aggregates), and 6 Bool-index forms inside empty literal-range or
zero-capacity Seq aggregates. Written aggregate binder/filter/value validation
now precedes normalization even for an empty domain under the #1463 source-typing
decision: existing type errors cannot disappear, and source diagnostics retain
the property span.
Empty Count/Sum now reports every existing type error in the written expressions,
such as the non-Bool filter in `count(x in 1..0 where x + 1)`; the 6 Bool-index
removals are examples in the measured corpus/inline inputs, not an exhaustive
list of type-error kinds.
The Bool-index closure is an intentional #1459-class consequence; no new type
rules are added, and valid empty/nonempty Map aggregates retain their verdicts.
Raw checking does not enumerate inner Count/Sum before outer-binder substitution
or prematurely reject a pattern name awaiting substitution. All 66 nested-aggregate
and pattern check controls remain accepted with identical base/head envelopes.
The 15 added removal IDs are only four quantifier relation-index bounds and
eleven sync-expanded/root relation-index forms, not those preservation controls.

Of 128 full-output differences, the other 18 remain error/semantics, exit 2:
`inline/1457/boundary/nonfinite` gains authored message/loc 1:161;
`inline/R2/seq-write-remains-rejected` keeps loc 7:17 but requires Map, not Map or
Relation; `inline/aggregate-and-component-locations/case-{13,17,37,41,49,53,61,65}` replaces synthetic
1:1 with authored 6:3 and the Map/Relation/Seq repair with Map/Seq;
`inline/aggregate-and-component-locations/case-{21,45,57,69}` changes only that repair text, retaining 6:3.
`inline/nested-aggregates-and-sync-ownership/case-{72,74}` already failed static aggregate bounds; the relation
index now reports authored 6:3 instead of no location. For
`inline/nested-aggregates-and-sync-ownership/case-{73,75}`, the existing error now anchors at root use 2:3
with the part.Capture cause at piece.fsl:6:3.
All other envelope fields are unchanged. Component refusals anchor at the owning
root use, preserve the component cause location, and use public names such as
`part.xs`, not lowered `part__xs`. These are intentional output changes.
Model-expression type errors now point to their owning declaration, including
errors whose base check envelope already had a loc; component type errors,
including pre-existing ones, anchor at the owning root use and retain their
component cause.
The inline IDs include actual #1457, aggregate/component and nested/sync emissions, fixed acceptance,
and rewritten guard/constructor inputs. Remaining range/domain limitations
are #1480; #1481 cell 1 covers Map Struct per-field init and cell 2 fixes
`head()`, `at(i)`, and index reads of empty zero-capacity Seqs. Reached reads
report `partial_op` in ordinary action/property contexts. Ranked decreases
narrowly refuses reached zero-slot reads with the base semantics error, before
their fresh value can enter rank evaluation. Unreached conditional or
short-circuited reads can now prove by ranking where the base reported a
semantics error. After that narrow preflight, #1542 also probes reached partial
operations in initial, successor, and deadlock-side measures under the ranking
pending assumption. A possible undefined measure, including one with physical
slots, is refused with exit 2, kind `semantics`, and
`leadsTo 'L' decreases measure may be undefined: OP`, not used as a rank value.
This is a rank-proof-phase refusal: if base BMC has already found a real
`leadsTo` counterexample, a refused optional rank diagnosis preserves that
`violated` verdict (exit 1).
Short-circuit guards remain effective; measures without partial operations add
no solver checks. BMC discharges nothing on a ranking error and retains its
lasso search (see
[Seq partial operations](../design/DESIGN-seq-partial-operations.md)).
Other typing gaps are #1459, literal shape relatives #1467. They are not
additional removals in #1463.

## 5. Semantics

- **Transition system**: one step = exactly one action instance
  (action name × parameter values) is executed atomically.
- **Simultaneous assignment**: all right-hand sides in an action body read the
  **old state**. Variables that are not assigned do not change (the frame
  condition is automatic).
- **Double assignment is a semantics error**: assigning to the same variable (or
  field) twice on the same execution path is a semantics error. The then/else
  of an if are separate paths, so you may assign in both. Assigning to the same
  variable **after** an if is also an error (to prevent the writes inside the
  branches from being lost).
- **Conservative write-alias rejection**: when a `forall` body writes an indexed
  location whose indices are not provably distinct across iterations, native
  `check`/`verify` and the browser Worker reject the spec before any verifier
  backend runs. This is distinct from a **proven duplicate write** (for example
  `m[0]` twice, or `forall c { m[0] = ... }`), which keeps the legacy
  `an action may not assign the same state location more than once` message.
  Unproved injectivity is reported as
  `cannot prove write-index distinctness across forall iterations` with
  `diagnostic_code: FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED`, the offending
  assignment `loc`, and—when a safe repair exists—a `hint` such as
  `forall k: Cell { if k >= BASE and k < BASE + 4 { m[k] = ... } }`.
- For `Map<K, Struct>` values, field writes are tracked per field. Updating two
  different fields of the same element in one action, such as `m[k].f1 = 1`
  followed by `m[k].f2 = 2`, is allowed. Repeating the same field on the same
  path is a semantics error. Indexed writes are rejected unless their indices
  are provably distinct constants; guards such as `requires k != j` and local
  constant bindings do not establish distinctness. The checked Kernel model rejects it before any
  verifier backend runs, so native `check`/`verify` and the browser Worker
  return the same `kind:"semantics"` classification.

  ```fsl
  type K = 0..1
  type V = 0..3
  struct Pair { f1: V, f2: V }
  state { m: Map<K, Pair> }
  action update(k: K) {
    m[k].f1 = 1
    m[k].f2 = 2
  }
  ```

  Observed result: `fslc check struct_fields_ok.fsl` returned `result:"ok"`,
  and `fslc verify struct_fields_ok.fsl --depth 1` returned
  `result:"verified"`. Changing the action to assign `m[k].f1` twice returned
  `result:"error"`, `kind:"semantics"` from both `fslc check` and
  `fslc verify`.
- **requires**: enabled only when all hold.
- **ensures**: checked in the post-transition state. A violation is
  `violation_kind: "ensures"`.
- **trans**: checked in the post-transition state of each execution step, with
  `old(expr)` evaluated in the pre-transition state. A violation is
  `violation_kind: "trans"`.

## 6. Automatic checks (things checked even without being written)

| Check | Content | On violation |
|---|---|---|
| Type bounds | Every bounded-type state variable (including Map values, struct fields, Seq elements, Set members and relation endpoints) is within range | `violated` / `type_bound` / `_bounds_<var>` |
| Partial Seq operations (action and property contexts) | At the time of `pop()`/`head()`/`at(i)`/index, the sequence is non-empty and the index is in range | `violated` / `partial_op` / `_partial_<action>`, `_partial_property_<property>`, or `_partial_property_terminal` |
| Partial arithmetic (action context: `requires`/body/`ensures` only, see §3) | The divisor of `/` `%` is non-zero | `violated` / `partial_op` / `_partial_<action>` |
| action coverage | Each action is enabled at least once within depth K | diagnosis of the blocking requires in `action_coverage` |
| Deadlock | Reaching a state where all actions become disabled | warning (`violated` with `--deadlock error`) |
| trans | Whether the two-state predicate holds across all reachable transitions | `violated` / `trans` / `trans` + trace |
| leadsTo | A P ~> Q violation via a missed `within` deadline, a lasso up to depth K, or deadlock stagnation (deadline misses and stagnation are detected as soon as `--depth` reaches the deadline/stalling step, and at every larger depth after that, not only when it lands exactly on that step) | `violated` / `leadsTo` / `bindings` + trace |

- `init` is evaluated with the action-context rules. A `/`/`%` by zero, a
  partial `Seq` operation, a finite `Map` key outside its domain (read or
  assignment target), or checked i64 overflow that init evaluation reaches
  (an operation on the unreached side of an init `if`, conditional, or
  `and`/`or`/`=>` is not reached) is a semantics error, exit 2, under every
  engine. It is not a `partial_op` violation, because there is no step before
  the initial state. All engines name the failure and the init statement with
  the same ` in init at L:C` suffix, e.g. `division by zero in init at 9:5`
  (#1543). `bmc` and `induction` (whose base case is `bmc`) also do so when init
  is nondeterministic (#1258). Even when init assigns an out-of-type value,
  `bmc` and `induction` report undefined init before assigned-root bounds,
  preserving the same located message as the other engines (#1597).
  If a symbolic init failure is found only beyond a stored `Seq`'s represented
  capacity, for example when init stores an over-capacity sequence and reads
  beyond its slots, these engines instead refuse without a location:
  `model sequence length exceeds capacity`.
  For quantified and aggregate expressions, all engines name the first reached
  concrete failure, not a generic failure of the whole binder (#1607). Evaluation
  visits the source first, then each present candidate's `where`, then its body
  or summand if the filter is true. `forall` stops at the first false body;
  `exists` stops at the first true body. `count`, `unique`, and `exactlyOne` do
  not stop early when their result is determined. `sum` checks each running
  partial sum for i64 overflow (`integer overflow in sum`), even if its final
  mathematical sum would fit; an undefined summand reports its own failure
  before accumulation. The location remains the containing init statement.
- Dividing the minimum i64 value by `-1` (`MIN / -1`) or taking its remainder
  (`MIN % -1`) is integer overflow, including in property contexts where only
  zero divisors are totalized to `0`.
- A deadlock warning includes which state you got stuck in (e.g. `deadlock reachable at
  step 1 (state: status=ToolFault, ...)`). The full trace is also in the JSON `deadlock.trace`.
- **Intended terminal states** (states where stopping is correct, such as
  processing complete or a final result) are declared in a
  `terminal { <predicate> }` block. Stopping states satisfying the predicate are
  excluded from deadlock checking, while other unexpected deadlocks continue to
  be detected. Whereas `--deadlock ignore` uniformly ignores **all stopping
  states**, `terminal` lets you select **which stops are intentional**.
  Example: `terminal { status == Done or status == Failed }`.
  - **requirements**: `terminal { }` is a `requirements_item` and passes
    through to the kernel spec (§13.2). Inside a spec that uses `process E {
    ... }`, use the source-level accessor, for example `terminal { forall c:
    Claim { stage(c) == Approved or stage(c) == Rejected } }`. It resolves from
    `c`'s entity type and lowers to the generated stage map; the generated
    `claim_stage` name is not part of requirements source syntax.
  - **business**: no `terminal` syntax exists at all — it is derived
    automatically from each process's sink stages (stages with no outgoing
    `transition`); see §13.3.
- **Do not write** an invariant like "inventory is at least 0" — make it
  `type Qty = 0..N` and it is detected automatically.
- A full `push` into a Seq is also detected automatically as `type_bound`
  (to guard it, write `requires q.size() < N`).

## 7. The verifier `fslc`

```
fslc check     <file.fsl|file.md>                  # syntax / names / types, and an inline implements block (depth-bounded, no Z3); .md = literate FSL
fslc lint      <path>... [--edition current|next] [--project fsl-project.toml] # edition + ID-policy diagnostics; never mutates
fslc migrate   <path>... --edition next [--write] # dry-run edits; --write applies validated set
fslc fmt       <file.fsl|-> [--edition current|next] # canonical FSL to stdout; never mutates input
fslc fmt       <path>... --check                 # JSON format_check; exit 0 clean, 1 changed, 2 error
fslc kernel    <file.fsl> [--kernel-version 1|2] # normalized typed Kernel JSON (default v1)
fslc conformance <file.fsl> [--depth K] [--kernel-version 1|2] # matching vectors (default v1)
fslc verify    <file.fsl|file.md> [--depth K]     # BMC (default K=8, counterexample is shortest)
               [--engine induction] [--k N]      # k-induction: unbounded-depth proof
               [--engine explicit]               # concrete-state BFS (native fslc): closure ⇒ proved
               [--explicit-budget N]             #   max visited states (default 1000000); over ⇒ unknown_budget
               [--engine auto]                   # explicit first, transparent fallback to bmc when
                                                 #   explicit can't decide (leadsTo, nondeterministic init,
                                                 #   unknown_budget); "engine"/"engine_fallback" in the result
               [--lemma "<expr>"]...             # independently prove auxiliary candidates,
                                                 # then retry CTIs with proved lemmas only
               [--from-state state.json]         # replace init with a complete logical snapshot (BMC only)
               [--deadlock warn|error|ignore]
               [--vacuity warn|error|ignore]     # vacuity check (§15)
               [--property <Name>]               # check one named property obligation —
                                                 #   invariant / trans / leadsTo / reachable;
                                                 #   induction keeps all invariants for a selected trans
                                                 #   induction still checks every implicit type bound in its base case
               [--exclude-property <Name>]...    # skip named invariant/trans/leadsTo/reachable
               [--strict-tags] [--requirements ids.txt]  # tag matching (§15)
fslc counterexample export <file.fsl> [--depth K] [--engine bmc|explicit|auto] [-o reproducer.json]
                                               # slice 1: export a safety-invariant counterexample to reproducer.v1
fslc sweep     <file.fsl> --instances E=lo..hi --depth lo..hi [--property Name]
                                                 # opt-in scope sweep over bounded verification
fslc scenarios <file.fsl|file.md> [--depth K]     # generate integration-test scaffold JSON
fslc replay    <file.fsl> --trace <events.json>  # spec-action trace conformance (§12)
fslc replay    <file.fsl> --from-log <events.jsonl> --mapping <mapping.fsl>
                                                 # production log mapping + conformance (§12)
fslc testgen   <file.fsl> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [--allow-unwired] [-o out]  # implementation-conformance test scaffold (§12)
fslc testplan  <file.fsl> [--depth K]             # closed test-plan.v1 vector selection (§12)
fslc refine    <impl> <abs> <mapping> [--depth K]# fidelity check of a detailed spec (§10)
fslc diff      <old> <new> [--depth K] [--mapping map.fsl]
               [--forbid behavior_added,invariant_weakened,forbidden_relaxed]
                                                 # bounded semantic change analysis
fslc diff      --git BASE..HEAD [spec.fsl] [--depth K]
                                                 # revision-consistent tree materialization; omit spec for all changed .fsl
fslc chain     [fsl-project.toml] [--keep-going] [--jobs N]
                                                 # manifest-driven cross-layer report (§10)
fslc mutate    <file.fsl> [--by-requirement] [--oracle-attribution] [--max-mutants N]
               [--from mutants.jsonl] [--fail-on-survivors] [--min-kill-rate R]
                                                 # built-in + external spec mutation (§15)
fslc explain   <file.fsl> [--depth K] [--readable] # JSON by default; readable text review view (§15)
fslc analyze   <file-or-dir>... [--projection tsg|action_state_graph|action_dependency_graph|code_audit|impact_graph|requirement_property_graph|property_state_graph|refinement_graph|traceability_graph] [--code FILE_OR_DIR] [--focus NODE] [--profile ai-review] [--export tag-review] [--format json|dot|mermaid]  # structural/tag/code review (§15)
fslc html      <file.fsl> [--depth K] [-o report.html] [--no-timing] # self-contained review report (§15)
fslc ledger    <file.fsl> [--depth K] [--impl-log run.json] [--approval record.json] [--trust-key public.pem] [-o ledger.md] # business audit ledger by requirement id (§15)
fslc document generate <file.fsl> [--view requirements] [--lang ja|en] [--strict] [--strict-rendering]
               [--glossary glossary.json] [--evidence evidence.json]... [--approval record.json]... [--trust-key public.pem]... [-o requirements.md]
                                                  # deterministic ja/en requirements document from the Requirement Claim IR (§13);
                                                  # --evidence overlays a per-requirement assurance class from saved
                                                  # external evidence (repeatable; same envelope shape as `fslc ledger --evidence`);
                                                  # --approval displays a verified requirements_document approval record
                                                  # (repeatable; fails closed if it does not match the current rendering)
fslc document claims <file.fsl> [--view requirements] [-o requirements.claims.json]
                                                  # emit the Requirement Claim IR (RCIR) claim set as JSON (§13)
fslc document check <file.fsl> <document.md> [--glossary glossary.json] [--evidence evidence.json]... [--approval record.json]...
                                                  # structural drift check: generated claim blocks vs a
                                                  # fresh re-render; never interprets prose (§13)
fslc approval create <file.fsl> --kind ledger|html|scenarios|requirements_document --artifact <reviewed> --approver <name>
               [--signing-key private.pem] [--glossary glossary.json] [--evidence evidence.json]... [-o record.json]
                                                  # requirements_document records a v3/v4 revision with a claim_set_digest (§13)
fslc approval check  <file.fsl> --record <record.json> [--trust-key public.pem]
                                                  # approved (exit 0) | drifted (exit 0, analysis output)
                                                  # | signature-invalid (exit 1); see DESIGN-approval.md
fslc approval diff   <file.fsl> --record <record.json> [--depth K] [--trust-key public.pem]
fslc typestate <file.fsl> [--ts]                 # decide applicability of state machine → ghost type (§16)
fslc domain check <file.fsl> [--depth K] [--engine bmc|induction] # Functional DDD / effect findings
fslc domain analyze <file.fsl>                                  # aggregate/effect ownership summary
fslc domain expand <file.fsl> [-o out.fsl]                      # generated kernel FSL
fslc domain generate <file.fsl> --target typescript|python|kotlin|swift|rust [-o dir] # Functional DDD scaffold
fslc domain testgen <file.fsl> [--target vitest] [--allow-unwired] [-o out]  # domain adapter/conformance scaffold
fslc domain replay <file.fsl> --logs events.jsonl              # domain runtime replay evidence
fslc db check  <file.fsl> [--depth K] [--engine bmc|induction] # dbsystem compatibility findings (§13.5)
fslc db observe <file.fsl> --trace events.json                  # runtime observation evidence
fslc db import <file.sql> [--name Name] [-o out.fsl]            # minimal SQL DDL -> dbsystem
fslc ai check <file.fsl> [--depth K] [--engine bmc|induction]   # ai_component hard-contract findings (§13.6)
fslc ai replay <file.fsl> --logs events.jsonl                   # AI runtime event replay evidence
```

Edition lint findings use the stable taxonomies `deprecated`,
`non_canonical`, `ambiguous_intent`, and `unsupported_in_edition`. Migration
handles legacy domain enums/operators, declaration string metadata, colon
quantifiers, unambiguous local inline action mappings, and implicit defaults.
It refuses source/comment movement that cannot be proven safe. `&&` remains an
invalid token and is therefore reported with an `and` suggestion but is never
machine-applied. See `docs/design/DESIGN-migration.md` for atomicity and bulk-update
steps.

For already-invalid expression `||` and `->` in spec, requirements, compose,
business policy, or refinement mappings, checking for the next edition reports
`kind: parse` with `diagnostic_code: unsupported_in_edition`. The diagnostic
points to the operator and suggests repairing it manually to `or` / `=>`, then
using `fslc migrate <file> --edition next` for remaining supported edits. These
invalid expression spellings are not automatically rewritten; lint/migrate
retain a parse failure (exit 2) until they are repaired. This is a diagnostic
improvement, not a new removal: current-edition
rejection and exit 2 are unchanged. Structural composition `||`, correspondence
and transition arrows (including anonymous range endpoints in relation types),
and domain expressions retain their existing behavior.
Range type endpoints, including parenthesized subexpressions within them,
retain generic `FSL-PARSE` diagnostics for these invalid spellings. They remain
rejected; this exception does not change structural relation arrows.

In the current edition, `check`, `verify`, and Worker JSON
append migration warnings for declaration string metadata
(`legacy_string_metadata`), colon quantifiers (`legacy_quantifier_colon`), and
inline action `maps` (`inline_action_maps`). These reuse lint's exact locations,
replacements, edits, and migration-safety information, without changing verdicts
or exit codes. A refused rewrite remains `machine_applicable: false`; a warning
does not mean automatic migration is safe. Prefer typed annotations, braced
quantifiers, and explicit action correspondences in `implements`. Lint retains
exit 0 for no findings, 1 for findings, and 2 for parse/check failure.
Domain checking rejects quantified domain expressions instead of emitting a
colon-quantifier warning; legacy string metadata and requirement action mappings
are not accepted domain forms.

Captured-root admission also covers `db check`/`compat check`, `scenarios`,
generic `testgen`, `kernel`, `conformance`, `testplan`, `typestate`, `explain`,
single-FSL-source `analyze`, trace replay and the six causal commands (#1449).
The single-root extension completes 21 further modes: the six AI readers,
domain analyze/expand/generate/replay/testgen, document check/claims/generate,
database observation, log replay, counterexample export ordering, mutate success
warnings, sweep capture and direct HTML/ledger raw-warning delivery.
They classify the same captured bytes they consume, before model/solver work or
output effects. Causal diff includes both operands; observation includes its
explicit FSL mapping. Each slot is distinct, so repeating a file in two slots
permits one warning per slot. Headers, existing edition options and read-error
kinds keep their rules; no new dialect/header combination is accepted.
Project analysis captures its TOML manifest once and admits the referenced direct
FSL layers and active mappings before projection; TOML itself contributes no
edition metadata. Envelope `fsl_edition` and `edition` describe only the primary
FSL root, while warnings retain each slot.
Preparing all causal roots means a later operand/mapping read error intentionally
precedes an earlier root's parse/model error.
Log replay now prepares its root and mapping before model work, so mapping IO
precedes an ordinary root parse/model error. Document check admits its captured
root before artifact IO/frontmatter, then projects the same bytes under the
artifact's recorded display label without rereading that label as a path.

Implemented open envelopes use `warnings` once. Successful closed schemas
(`kernel`, `conformance`, `testplan`) and implemented raw TypeScript/readable/graph
outputs use stderr `warning: <finding-json>` without changing stdout. Admitted
verification finalizes after governance; domain/database checks, scenarios and
generic testgen finish only their outer result. Domain check no longer duplicates
migration warnings, including non-definitive errors. Worker admission precedes
model/solver use; editor refusals retain UTF-16 ranges without model/analysis work.
Current-edition warnings preserve verdicts, assurance and exits; next-edition
migration findings refuse before consumption with the existing edition error/exit 2.
Direct raw HTML/ledger, document generate/claims and domain expansion/testgen deliver
migration warnings on stderr. HTML/ledger file-output envelopes stay unchanged;
their distinct baselines retain one warning. Approval-only rendering borrows the
captured source/token and preserves standalone artifact/baseline diagnostics
without new stderr delivery. Requirements-document rendering does not read or
speculatively scan an `implements` child. Mutate owns the authored
source's finish, and each sweep cell borrows one captured root's token.
Previously ungated consumers can therefore reject next-edition input they accepted.

The measured comparison uses BASE `5637daf2ce8ce3e1a2c3b1266a8442c3df7887e4`
and built implementation `33e3fcc5606249a097814d91cb09b506add8e97d` in their
materialized trees. `fslc check FORM`, with cache off, covers all 597 `.fsl`
files in `specs/`, `examples/`, and
`rust/fslc/tests/fixtures/` plus 309 inline occurrences, with both binaries in
both trees (3,624 checks). The ok-to-error transition set is empty; 57 corpus
forms and 22 inline occurrences change only warnings. No fields are excluded.
The consumer comparison against parent `25f68db7fe214ed2ed66034701ed1bf06b5fbe7c`
records a breaking refusal for four next-edition migration forms (enum union,
metadata, colon quantifier, inline maps) in analyze/conformance/explain/html/
kernel/ledger/mutate/scenarios/sweep/testgen/typestate, and the enum-union form
in testplan: 45 command/form observations that check already rejects. This does
not complete raw-warning, mutate-success-warning or sweep-capture modes at that
revision; the single-root extension now completes them.

The latest extension sweep uses BASE `1844fa74a717ba9229e0b292d9421c0939016e5e`
and implementation Rust revision `e04df80f35cf3a7a3b8119dc7b6513ac7272ce76`, each
built from its own clean archive. Cache-disabled `fslc check FORM` covers 597
tracked corpus files plus 29 required-test input occurrences in each tree, both
binaries in both trees: **2,504 checks**, empty ok-to-error and full
stdout/stderr/exit difference sets, no excluded fields. The 29 comprise eight
original occurrences (two domain snapshot includes, two scenario literals, two
empty-membership inputs, fixed approval source and command-matrix helper approval
baseline), twenty executed success-table sources and one replay-order root.
Included files and repeated source bytes remain counted as occurrences.

Earlier named sweeps remain valid historical records: BASE
`a42d4cede3d8d688daff3173de7bd839a6992172` /HEAD
`e2669212315f4db9bec2c0c525ba8d44f77bacfd` used 597+29 per tree /2,504 checks;
BASE `c79016efeb630c717292e9392901cfe65ab40915` /implementation
`ceb03268e567487f6ec080afcf3ab034d4a5ea0c` used 597+8 per tree /2,420 checks.
Both had empty ok-to-error sets; neither is substituted for the latest result.
The latest BASE/Rust-revision consumer comparison runs twice, 772 observations:
17 distinct breaking command/source removals (19 control occurrences per repeat,
including repeated requirements-metadata probes). Domain analyze/expand/generate/
replay/testgen refuse enum union (5 pairs); document claims refuses metadata in
spec and requirements, colon quantifier and inline mapping (4); document generate
refuses metadata in spec and requirements (2); document check refuses requirements
metadata (1); replay --from-log refuses enum union, metadata in spec and
requirements, colon quantifier and inline mapping (5). These are formerly
non-error consumer inputs, not grammar removals or proof claims. A mapping-only
control already errored at BASE and is not counted as a removal.

Live multi-root coverage also includes analyze batch/project, approval
check/create/diff and ledger `--approval`, chain, live diff and direct/multi-link
refine. The deferred set is empty; complete-adapter controls require every
nonhistorical observation to agree and reject a restored deferral. The historical
Git old/new and approval-baseline controls still execute separately (#1513).
Diff's old root and refinement's first implementation supply primary metadata;
approval diff's sole live root is primary. Repeated explicit occurrences remain
distinct, but a multi-link intermediate spec is not counted again for each link.
Live Git mappings are secondary slots; materialized mappings remain historical,
with the existing absolute/relative selection unchanged.

A single live Git mapping is captured once at its first legitimate use and lent
to all selected comparisons, including stream-backed paths already readable by
the filesystem API. Comparison-local warning/IO results and empty-range/missing-
operand precedence are unchanged. Project mapping preparation requires canonical
business/requirements/design endpoints with files; a skipped external target's
mapping is not read or classified, and no stricter TOML validation is introduced.

All batch roots are prepared before any file consumer, after the existing sort
and adjacent deduplication, while dependency scope and diagnostics remain per
file. A refused child cannot discard another file or replace the batch envelope.
Project results receive direct-root warnings without a fabricated TOML edition;
empty project/batch inputs retain their existing success. Chain groups captured
bytes and carries immutable source/tokens through its existing scheduler; each
actual step thread finishes its detail before wrapping it. Ordinary chain IO
retains the established layer/cutoff rather than becoming a top-level refusal.

These are intentional consumer-output changes: outside chain, later direct-input
IO now precedes earlier parse/model errors (including project and mixed-batch
preparation). Newly owned dependency reads in diff/refine/approval and chain's
refine detail return the complete child refusal/findings/edition with exit 2,
instead of flattened errors, and deliver accepted child warnings once. Approval
diff retains approval metadata on success; a child refusal replaces it. Distinct
approval artifacts and baselines keep standalone diagnostics without new stderr.
They can reject previously ungated consumer input, not add grammar or assurance.
Neither HTML nor ledger has independent baseline pinning in this completion;
that strengthening and three internal seam controls are tracked in #1641.
Report baselines and ledger migration rendering are retained; migration-only rows
use 🟡 移行候補 / risk —, not vacuity/mutate advice. Mixed/non-migration rows remain
🔴 要確認 / 要確認. Dependency admission is described below; historical Git roots
remain #1513. Repair projections/exits stay unchanged, including lint's existing
dialect limits (#609); this does not remove public-example warnings (#608) or fix
other ledger warning classifications (#1497).

The multi-root type-boundary audit compares base
`796876558b83f2dedd7dafdf04ef77fced3de033` with Rust/source-test revision
`8796f19d9646a4edffbac0719c3693a8f88c1405`, built in separate materialized trees.
`fslc check FORM` covers 606 corpus files and 35,611 captured inline occurrences,
both parser configurations and observed edition requests (72,859 pairs).
The additional `e16f65bfeae773218d23f425376ea7c23b895bff` comparison covers its
603 corpus files and the same inline inputs: 218,571 commands overall, empty
ok-to-error sets and empty full base/head JSON/stderr/exit differences, with no
excluded fields. Repeated occurrences and rewritten dependency-test inputs remain.

The active-target/single-mapping repair repeats that comparison with independent
Rust/source-test head `e47dce898889e891e9fa82a5fdd8698749e4db80` and the same
named main base: 606 corpus files, 35,663 inline occurrences including new repair
inputs, 72,963 configuration pairs / 145,926 commands. Reused fixture bytes were
verified and every CLI check rerun. Both the ok-to-error and complete
JSON/stderr/exit difference sets are empty; no fields are excluded.

Actually read dependencies use **their own** header or the current default
(#1452), not a parent's requested edition. Canonical mixed editions remain valid.
Compose `use`, `implements from`, causal `uses` and governance delegates,
before/after documents and refinement mappings all pass the same admission rule,
including nested reads and rereads, before their bytes enter model/solver or output work. Unused
virtual files and declared references a consumer does not read are not scanned.
Owned commands return the child's complete edition refusal/exit 2; owner-free
strict loaders retain their read-error envelope with the child path/codes in its
cause and still fail closed. This dependency rule is separate from the explicit
root-slot completion above.

Dependency migration warnings finish once per provider-resource/complete finding
within each outer native owner, or each explicit local log, without replacing
primary-root edition metadata or merging explicit root slots. Native keys use
real canonical paths; child `loc.file` keeps the reference spelling. Virtual lookup keeps its
provider keys. Existing closed/raw stdout and stderr-warning rules remain.
Worker/editor records are local to their call. The editor makes its own key and
related link absolute and lexically normalized, without changing Store lookup or
resolving symlinks; labels without file paths are not directories. It relates the child's own
text/severity to the first parent use in read order. A child refusal prevents model/analysis
and parent quick-fix delivery. Repair tools report child findings without consumer
rejection or child edits, preserving their root planner/formatter exits. Parse
reports are findings, not nested command envelopes. This is not a global preflight:
native lemma-mode implements and governance reads may follow root solver work;
refused child bytes still never enter that solver/cache. Ordinary Worker governance
errors retain native kernel-error precedence; only dependency refusal returns early.

**Breaking removal:** a headerless compose root importing a next-edition child
with a legacy colon quantifier formerly returned `ok`/0 with no message. It now
returns the complete child `unsupported_in_edition` refusal/2, whose finding
message is `legacy colon quantifier syntax is deprecated`. A braced canonical
twin still succeeds with the complete `fsl:1.0,result:ok,spec:C,warnings:[]`
check envelope (plus validated runtime versions). The inline shape is retained by
`edition_dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice`
and the `edition_dispatch_inventory` contexts
`dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice` and
`all_nonhistorical_cells_agree_across_the_complete_native_adapters`.

The materialized-tree `fslc check <path>` comparison from `bc14d885` to corrected
Rust source/test tree `a65eddd5` checked all **603 corpus forms in both parser
configurations (1206 checks per tree)** and **368 required-test inline/included
occurrences per tree**, for **3148 checks**. It found **6 `ok`→`error`
occurrences**, only the legacy-compose cell above in three contexts twice each;
**0 other transitions**, including **0 corpus transitions**. All canonical twins
remain accepted. See the dependency amendment in
[`DESIGN-migration.md`](../design/DESIGN-migration.md) for the exact source-tree identity and scope.

Each `fslc lint` path may be a file or directory. Directories recursively expand
to regular `*.fsl` files; symlink entries and other extensions are skipped while
walking them, and the combined file set is deduplicated and sorted
deterministically. Explicit file paths retain their existing extension-agnostic
behavior.

The native Rust-only `kernel` command runs after dialect lowering and semantic
checking. Its versioned JSON contains structural types for every expression,
source spans, requirement/lowering origin, simultaneous-update semantics, and
explicit partial-operation failure conditions; an external compiler never needs
the Python AST or an FSL expression parser. `conformance` emits bounded concrete
success, disabled, and rollback-failure vectors under the companion schema.
Nested options use tagged `none`/`some` objects so no reachable states collapse.
Public Kernel v1 explicitly rejects `compose` export because current lowering
does not retain truthful per-component filenames; direct and other lowered
dialects remain supported.
Compatibility rules, schemas, fixtures, and Rust API entry points are specified
in [`DESIGN-kernel-contract.md`](../design/DESIGN-kernel-contract.md). Public Kernel v2 is
opt-in and specified by
[`DESIGN-kernel-origin-v2.md`](../design/DESIGN-kernel-origin-v2.md); it replaces the lossy
v1 origin object with target references into a queryable provenance graph. Its
`completeness` field must be checked before relying on source coverage.

`verify --from-state state.json` replaces the declared `init` for one bounded
run and asks what can happen from that complete current state. The JSON shape is
exactly `Monitor.state` / replay logical state: all variables and Map keys are
required, enums are member names, Option is value or `null`, Set/Seq are arrays,
and relation is an array of pairs. Missing/extra/type-invalid values are rejected
before solving. Snapshot runs bypass the verdict cache, disable symmetry
reduction (the identities are concrete), and reject `--engine induction`.
Results add `initial_state.source:"snapshot"` and
`faithfulness:{scope:"bounded_from_snapshot",spec_init:"not_used",induction:"not_applicable"}`
so bounded `verified` cannot be mistaken for verification from the spec init.
See `DESIGN-from-state.md`.

In addition to `reachable` and action coverage, `scenarios` outputs, for each
`leadsTo P ~> Q`, a `respond_<Name>[_<binding>]` scenario. Each scenario has
`kind: "leadsTo"`, `pending_at`, `satisfied_at`, `bindings`, `steps`,
`initial_state`, and `expected_states`, representing the shortest trace from P
holding to Q holding within depth K. A quantified property is tracked
per binding, not per property name: one binding's response scenario never
suppresses the completeness warning for a sibling binding with none. A
binding for which P never holds within depth K gets its own warning worded
"antecedent never holds within depth K"; a binding for which P held but no
response ever closed gets "has no response scenario within depth K" — the
two are never conflated.

`verify --property Name` resolves across invariant, `trans`, `leadsTo`, and
`reachable` declarations and checks only the named property kind in isolation.
Induction also checks every implicit `_bounds_*` in its base case because its
step assumes them. Thus `--engine induction --property P` can report a violated
`_bounds_*` (`violation_kind: "type_bound"`, exit 1), even when P is selected;
the **`proved` envelope's** `invariants_checked` includes those bounds, but
**`violated` envelopes omit `invariants_checked`**. The intentional #1598
transitions are exit 0 false `proved` / `unbounded` → exit 1 `violated` /
`type_bound`, exit 2 `error` / `kind: "semantics"` → exit 1 `violated` /
`type_bound` (for an overflowing Seq frame).
The shared base also applies to `sweep --engine induction --property P`
(exit 0 `sweep_passed` → exit 1 `sweep_failed`) and `verify --lemma` targets
(exit 0 `proved` → exit 1 `violated` / `type_bound`).
In addition, `unknown_cti` or `violated P` at the same or a later step becomes
`violated _bounds_*` at the first failing base step.
Same-step examples: `n: 0..1`, init 0, increment, `P { n <= 1 }` at depth 2
changes `P`@2 to `_bounds_n`@2; a Set initial copy with `K = 0..1`,
`init { x = 2 t = Set { x } s = t }`, `P { not s.contains(2) }` changes `P`@0 to
`_bounds_s`@0 (both remain exit 1 `violated`, now `type_bound`).
For example, a deterministic overflowing Seq init now reports the bound at
step 0; `n: 0..1`, init 0, increment, `P { n <= 2 }` reports `_bounds_n` at
step 2 when the base depth is at least 2. Standalone BMC and explicit property
selection are unchanged. In addition, with
`--engine induction --property <trans>`, the named `trans` is the only transition
obligation, but all user invariants and implicit type bounds remain in the base
case and the induction hypothesis. This preserves proofs whose two-state safety
argument depends on established state invariants and makes the selected
transition's verdict match the same transition in an all-properties induction
run. `--property <invariant>` still restricts user invariants to the selected
one rather than retaining sibling invariants. The
induction selector continues to reject selected `leadsTo` and `reachable`
properties with a usage error.
`--exclude-property Name` is repeatable and acts as the cross-kind inverse:
it removes named invariants, `trans`, `leadsTo`, and `reachable` properties
from the run and from checked-property outputs (`invariants_checked`,
`transitions_checked`, `leads_to`, and `reachables`). When `--property` and
`--exclude-property` name the same property, exclusion wins.
Implicit bounds are also selectable with `--property _bounds_<var>`, including
relation bounds. They appear in `invariants_checked` and, when proved by
induction, `k_used`. The current exclusion limitation is unchanged:
`--exclude-property _bounds_<var>` is rejected as `no such property` (exit 2);
exclusion applies to the declared property kinds above, not implicit bounds.

**Checks a selected run does not evaluate (#1008).** Not executing a check is
never evidence that it holds. When a `verify` option makes the run skip a
declared check, the envelope keeps that check's key with
`result: "not_evaluated"` and the cause instead of omitting it. The top-level
`result` and the exit code are unchanged and speak only for what was checked.
The complete list:

| Section | Present when the spec declares | `reason` (option) |
|---|---|---|
| `implements` | an inline `implements` | `property_selection` (`--property`), `property_exclusion` (`--exclude-property`), `from_state` (`--from-state`) |
| `requirement_traces` | `acceptance` / `forbidden` scenarios, one of which `--instances` / `--values` put out of scope | `bounds_override` (`--instances` / `--values`) |

`reasons` lists every cause that applies, in the table's order, and `reason`
repeats the first. `implements` also carries `abs`; `requirement_traces` carries
`skipped`, one `{kind, id, reference}` entry per skipped scenario (the scenarios it
does not list were replayed and passed). A spec that declares no such
check keeps the key absent, so a consumer can tell "nothing declared" from
"declared but not evaluated". `not_evaluated` is never a pass: a consumer that
accepts only `implements.result == "refines"` rejects it, and the seam is gated
on an unfiltered `check`/`verify` or on `fslc chain`. No other option suppresses
either check; an option that starts to must be added to this table. Warnings are
not a selected check: a selected run reports the same compose-lowering warnings
(`fair_not_inherited`) and the same `no_user_invariants` suppression as a full
run.

`verify --engine induction --lemma "EXPR"` accepts repeatable auxiliary
invariant candidates for an `unknown_cti` repair loop. All candidates are parsed
and recursively type-checked as `Bool` by the invariant checker before any
candidate proof or evaluation. Each valid expression is then
proved independently against the original init/actions and implicit type
bounds, without assuming the original user invariants. A false candidate is
`rejected` with its reachable counterexample; a non-inductive candidate is
rejected with its own CTI; invalid candidates are `rejected`, `used:false`, with
a parse/type error. Type errors carry `proof.result:"error"`, `proof.kind:"semantics"`
and the checker message, without an argv `loc`; parse errors keep their parser
location. Rejection does not abort later candidates; the target result owns the
exit code. Only candidates whose independent result is `proved` may enter the
original proof.
The verifier evaluates those candidates on each target CTI, adds the first one
that is actually false on that CTI, and retries. JSON fields `lemmas` and
`lemma_cti_exclusions` record adjudication and the exact CTI/violated steps.
When the target reaches `proved`, `auxiliary_invariant_recommendation` emits the
declarations to persist in the source; the command never rewrites the file.
There is no unverified-assumption mode. `--lemma` with the BMC engine is a usage
error. Lemma-guided runs bypass persistent verification cache lookup and storage.
For #1448, the 579-form, two-mode base/head `check` sweep removed no accepted
`.fsl` forms; commands and revision identities are recorded in
`docs/design/DESIGN-induction-lemmas.md`, "Type-validation compatibility evidence".

The sweep envelope is a function of its inputs apart from `cache` and timing
execution metadata. For stable comparisons, exclude `cost` and `cache` only at
the root of each verification envelope (`sweep.results[].verification` and
`sweep.minimal_counterexample.verification` when present), as approval's
`stable_json` does; preserve same-named fields in domain state. Cache provenance
is not verdict evidence. Sweep has no `--no-cache` option; use `FSLC_CACHE=off`
to disable cache reads and writes.

`sweep` is an opt-in wrapper around `verify`; it does not change normal
verification. It evaluates a deterministic grid of `--instances NAME=lo..hi`,
`--values NAME=lo..hi`, and `--depth lo..hi` overrides, records each underlying
verification result under `sweep.results`, and returns the first true failing
scope under `sweep.minimal_counterexample`. For `--values`, the sweep fixes the
lower bound and expands the upper bound (`lo..lo`, `lo..lo+1`, ..., `lo..hi`).
A passing sweep means "no counterexample in this grid", not an unbounded proof.
A spec `error` (parse / type / semantics / io / vacuous / …) from any scope in
the grid is not a counterexample: `sweep` returns that underlying error
envelope verbatim (`result`, `kind`, `message`, `loc`, exit code unchanged)
instead of folding it into `sweep_passed`, `sweep_inconclusive`, or
`sweep_failed`.

A `reachable_failed` cell is inconclusive only when its nonempty `unreached`
array contains only `classification:"insufficient_depth"`. Such a cell remains
in `sweep.results` but is not a counterexample. Inconclusive cells are settled
per scope: a scope is one `--instances`/`--values` combination across the whole
`--depth` range, and it is settled when any of its cells is a determinate
success. A larger depth answers what a smaller depth left open; a different
instance count or value range does not, so a success in one scope never settles
another. With no true failure, a grid whose every scope is settled returns
`sweep_passed`/exit 0; otherwise it returns `sweep_inconclusive`/exit 1 with
`minimal_counterexample:null`. `sweep.inconclusive_scopes` lists every scope
(`{instances, values}`) whose cells are all inconclusive, under any sweep
result, and is `[]` when there is none. Missing, unknown, or mixed
classifications, including `over_constrained`, are true failures and return
`sweep_failed`/exit 1.

Every sweep cell runs with `--instances`/`--values`, so a cell can report a
declared check as `not_evaluated` (§7, "Checks a selected run does not
evaluate"): `requirement_traces` when the cell's scope puts an
`acceptance`/`forbidden` scenario out of range (§4; an in-range failing scenario
is an error the sweep returns), and `implements` under `--property`. The grid verdict does not
change, but the skip is not hidden behind it: each such cell's `summary` row
carries the section name with the value `"not_evaluated"`, and
`sweep.not_evaluated: {"sections": [...], "reasons": [...]}` is the union over
all cells (sections sorted, reasons in first-seen order). The key is absent when
no cell skipped anything. A `sweep_passed` grid with `sweep.not_evaluated`
therefore says nothing about those sections; check them on an unscoped
`check`/`verify`.

`diff` compares state-machine meaning instead of source text. It runs bounded
refinement in both directions: NEW→OLD failure is `behavior_added`, while
OLD→NEW failure is `behavior_removed`. It separately checks implication between
the conjunctions of user invariants (`invariant_weakened` /
`invariant_strengthened`) and replays OLD `forbidden` scenarios against NEW
(`forbidden_relaxed`). A NEW final step that its guard disables preserves the
OLD rejection, and so does a final step both sides reject as `bad_call` outside
a range or enum parameter type. Each side is classified with its own `entity` /
`number` types: since #1229 an OLD final step outside its verify scope is
`unknown` / `forbidden_replay_failed` (no OLD guard rejected it), and an OLD
`bad_call` that only NEW's scope excludes is `unknown` /
`forbidden_step_unrelatable` (before #1229 both were
`forbidden_step_unrelatable`). A compose NEW, or a NEW of another dialect whose
`entity` / `number` types are not in its source, never preserves an OLD
`bad_call`; before #1229 a compose NEW that rejected the step as `bad_call`
did, so such a forbidden now fails `--forbid unknown`. An OLD final step naming no action or arity is
`unknown` / `forbidden_replay_failed`. Since #1213 a NEW final step that is
enabled and then stops with a runtime violation is `forbidden_relaxed`, and an
OLD final step or a NEW setup step that violates is `unknown` /
`forbidden_replay_failed`. Since #1239 (breaking) so is a NEW setup step that
its guard disables: NEW never runs the final step, so the forbidden is not
preserved (it used to be), and a gate that must catch it lists `unknown`. An
OLD final step whose argument is not of its parameter's kind (#1241) is
`unknown` / `forbidden_replay_failed` too (see
[`DESIGN-semantic-diff.md`](../design/DESIGN-semantic-diff.md), "Forbidden
replay"). Directional failures include counterexample witnesses.
Same-named compatible state/actions are mapped automatically; name mismatches
are `unknown` unless `--mapping` supplies that direction. An arbitrary mapping
is never inverted automatically.

The JSON result is `semantic_diff` with `bounded`, `scope`, `directions`,
`summary`, `findings`, and `gate`. A changed `verify { instances/values }`
scope is reported as `scope_changed`, and shared OLD bounds are rebuilt under
the NEW scope recorded by `scope.comparison:"new"` and
`scope.applied_to_old`. With no findings, `summary` is
`["no_semantic_change"]`. Findings are analysis output and therefore exit 0 by
default. Only findings explicitly listed by comma-separated `--forbid` make
the gate fail and exit 1. All comparisons remain bounded to `--depth`; clean
output is not an unbounded equivalence proof.

`diff --git BASE..HEAD [spec.fsl]` is the VCS/CI adapter. It resolves and
records both full commit hashes, materializes both complete tracked trees, and
then invokes the same two-path comparison. This makes relative imports resolve
from their own revision. Omitting the spec compares every changed `.fsl` path
and returns `semantic_diff_batch`; supplying one spec preserves
`semantic_diff`. Both forms include `vcs.materialization:
"git_archive_full_tree"`. The ordinary two-path form never invokes Git and
works outside a repository.

`approval create` binds a reviewed `ledger`, `html`, or `scenarios` artifact to
the fully lowered, location-insensitive kernel digest and the current clean Git
commit. The versioned JSON sidecar also records the normalized artifact digest,
generator/version/options, approved requirement IDs, approver, and UTC time.
Creation regenerates the artifact and rejects a stale review file. `approval
check` returns `approved` only while the spec, rendering, and renderer bindings
all match; otherwise it returns `drifted` with `spec_changed`,
`rendering_changed`, and/or `renderer_changed`. `ledger --approval` adds the
same status per requirement and includes the full baseline digest. `approval
diff` materializes the approved commit and invokes the ordinary bounded semantic
diff against the current working file. See `docs/design/DESIGN-approval.md`.

Without `--signing-key`, `approval create` emits the unchanged unsigned
`fslc.approval.v1` record. With an Ed25519 PKCS#8 PEM signing key it emits
`fslc.approval.v2`; every check, diff, or ledger use of that record requires the
matching SPKI PEM `--trust-key`. The v2 detached signature covers the full
canonical record, and a cryptographic mismatch is `signature-invalid` rather
than an approval. Trust-key possession authenticates a signer but authorization
remains an organizational policy. The exact canonicalization and key formats
are specified in `docs/design/DESIGN-approval.md`.

Exit codes: `0` = verified / proved / scenarios/testgen generated / conformant / refines /
mutated (unless its explicit gate fails) / explained / analyzed /
semantic_diff (unless its explicit gate fails) /
typestate / sweep_passed / observed_conformant /
imported / imported_with_warnings,
`1` = violated / reachable_failed / unknown_cti / unknown_budget / nonconformant /
refinement_failed / impl_violated / sweep_failed / sweep_inconclusive / observed_mismatch /
indeterminate (`chain` when its `[impl]` layer has no evidence that a test executed),
`2` = input error (usage for invalid argv / parse / type / semantics / io / vacuous / acceptance / forbidden /
`--vacuity error`), `3` = internal error. `observed_*` is `fslc db observe`'s
result; `imported`/`imported_with_warnings` is `fslc db import`'s. `impl_violated` is listed
because inline `implements` propagates that seam verdict to the top-level `result`; a failed
governance `preservation` folds into `check`/`verify`/`scenarios`/`testgen` through the same mapping (§13). The fold
happens where the verification envelope is produced, so it is not confined to `check` and
`verify`: on a spec whose seam fails, `mutate` re-emits the baseline verdict (generating no
mutants, because its baseline is no longer `verified`), `ledger` and `fslc html` inherit the
same exit from the folded verification envelope, and `sweep` reports `sweep_failed` for a true
failure or `sweep_inconclusive` when some scope has only depth-limited cells. `scenarios`
folds the seam and every preservation into its own envelope the same way (the folded
`result`, exit 1, with the `implements` / `governance` section attached), whether it
answered from its own bounded run or fell back to the `verify` envelope.
For inline `implements`, the ledger body also shows the folded verdict and
`abs` / `result` in `## inline implements`, before governance and the risk lists;
only `refines` is green, including passing seams. The section addition alone
does not change ledgers without `implements`; sentinel coverage removal and
verdict findings separately change affected ledgers, including governance
documents without `implements`. Previously approved inline-implements ledgers now report
`drifted` in `approval check` (exit 0): review the section and re-approve them.
The ledger risk table also records each non-`refines` preservation as a `GOV`
finding and a non-`refines` inline seam as a spec-level finding. Unrelated
requirement rows keep their own status. `unknown_cti` is an unestablished
induction finding, attributed by the same declaration-owner rule as ordinary
violations (see [the ledger amendment](../design/DESIGN-ledger.md)),
not a reachable counterexample; raw CTI, hint, and rank-failure evidence is
preserved. Added findings and corrected attribution also change affected ledger
approval digests.
Ledger warning kinds select their trace type, summary, translation and advice
from one presentation table (#1497). Six vacuity and five migration families
retain their rendered rows; `deadlock` is a deadlock finding,
`never_enabled_action` is coverage beside the existing coverage projection, and
`vacuity_probe_truncated` is `verification_incomplete` without mutate advice.
Unknown, missing or non-string kinds remain visible generic warnings.
`no_user_invariants` describes absent user invariants and transition constraints;
reachable and progress checks may still run. Raw warnings and other evidence,
assurance, generated-artifact envelopes and command exits remain unchanged.
Changed rows make affected approvals `drifted` for `rendering_changed` (exit 0);
review the ledger and regenerate the approval. Newly approved ledgers stay approved.

Direct `ledger` and `html` return verification `result:"error"` as the original
JSON error envelope and exit 2/3, before rendering or writing an artifact;
direct `ledger` stops before ancillary replay, evidence loading, and witness preparation
(`html` also stops before explain). With `-o`, no artifact is created. Other
failed/unestablished verdicts still generate reports with their nonzero exit.
Approval-only ledger rerendering for digest comparison is separate from this gate.
All native full-verification callers share one stage owner; baseline adapters
disable caching and retain default vacuity warnings.
Direct reports verify and check for errors before loading the presentation model or
extracting its trace contract. Thus malformed causal syntax retains `FSL-PARSE`
and its parser location instead of `FSL-DIALECT-UNKNOWN`; Agent input retains the
parse refusal instead of a missing-state semantics error; an unknown DB column
retains the specialized error without a generated-name location; and an unknown
AI authority tool returns a semantics error (exit 2), not a panic (exit 101).
Passing reports, warnings, metadata and approval artifact digests are unchanged.
These error corrections apply only to direct `ledger` and `html` reports.
`approval create --kind ledger|html`, `approval check`, `approval diff`, and
`counterexample export` still lower the spec before verification or rerendering,
retaining their existing invalid-input precedence. After #1464, unknown AI authority
tools return a semantics error with exit 2 rather than panicking. Approval create's
diagnostic precedence remains a separate issue.
`testgen` refuses a failed seam or preservation with the same verify failure
envelope and exit (1 for failed verification, 2 for a contract error, 3 for an
internal error), on both kernel and fallback paths,
and writes no harness. The same
`2` mapping is fail-closed for `chain`'s project-manifest reader (unrecognized
section, zero recognized sections, or an unparseable `depth`/`refine_depth` —
`docs/design/DESIGN-layers.md` §7) and for `ledger --impl-log`'s replay input (a
replay error is not implementation-log evidence and must not be rendered as
an empty ledger row — `docs/design/DESIGN-ledger.md`).

`approval_check` and `approval_diff` are absent from this table because their
exit code is not a function of `result`. `fslc approval check` reports the same
`approval_check` for every outcome and carries the verdict in `status`: `0` for
`approved` and for `drifted`, `1` for `signature-invalid`. Drift exits 0 because
it is analysis output, as `diff` findings are (§12); `docs/design/DESIGN-approval.md`
records why, and why `fslc document check`'s `document_drifted` differs. Gate on
`status`, not on the exit code.

### Kinds of result

| result | Meaning | Next move |
|---|---|---|
| `verified` | No violation up to depth K (+ all reachable satisfied); `completeness:"bounded"` | To raise confidence, use `--engine induction` |
| `proved` | **The invariant holds in all executions** (unbounded depth); `completeness:"unbounded"`. From `--engine induction`, or from `--engine explicit` when exploration closes (`closure:true`) | Done |
| `violated` | A counterexample exists. Comes with `violation_kind` and the shortest trace | Read the trace and fix the spec |
| `reachable_failed` | reachable not reached within depth K | Read each `unreached[].classification`: raise `--depth` for `insufficient_depth`, or fix the blocking constraint for `over_constrained` |
| `sweep_passed` | A sweep grid has no true failure and every `--instances`/`--values` scope has a determinate success at some depth; depth-limited observations remain in `sweep.results` | Inspect the recorded grid; this is not an unbounded proof |
| `sweep_inconclusive` | No true failure, but at least one `--instances`/`--values` scope has only `reachable_failed` cells with only `insufficient_depth` classifications; those scopes are listed in `sweep.inconclusive_scopes` and `minimal_counterexample` is null | Raise `--depth` for the listed scopes or adjust the sweep bounds, then rerun |
| `sweep_failed` | A sweep cell has a counterexample or a fail-closed reachability classification | Inspect `sweep.minimal_counterexample` and fix the reported problem |
| `unknown_cti` | The invariant is not violated but is not inductive | **Read the CTI and add an auxiliary invariant** (§8), or try `--engine explicit` (closure proves without lemmas) |
| `proved` + `not_covered: ["int_overflow"]` | `--engine induction` proved the properties over unbounded integers; i64 overflow is checked only by the base case to `checked_to_depth`. Present when a proof site uses `+ - * / %`, unary `-`, `abs` or `sum`; absent otherwise. The verdict and exit code are unchanged | Bound the value (range type or a guard) or run `--engine bmc`/`explicit` at a depth that reaches the overflow if it matters |
| `unknown_cti` / `partial_op` | A state satisfying every proved invariant reaches a partial operation (§6) in a guard, body, property, or reached `ensures` | Guard the operation with a short-circuit `and`/`=>`/`or`/`if`, or add an auxiliary invariant that excludes the state if it is unreachable |
| `unknown_cti` / `key_domain` | A state satisfying every proved invariant indexes a finite-key `Map` outside its key domain (read or indexed assignment) in a guard, body, property, or reached `ensures`; BMC and explicit report it as an error once it is within depth | Guard the index with a short-circuit `and`/`=>`/`or`/`if` or a `requires`, or add an auxiliary invariant that excludes the state if it is unreachable |
| `unknown_cti` / `ensures` | An action's reached `ensures` is false on a step between states that satisfy every proved invariant | Fix the body or the `ensures`; if the start state is unreachable, add an auxiliary invariant that excludes it |
| `unknown_budget` | Either: `--engine explicit` exceeded `--explicit-budget` before closing; or a refinement correspondence search exceeded its fixed internal state budget (no CLI flag) — an inline `implements Abs from "file" { }` seam (`check`/`verify`), `fslc refine` (single or chain), a `fslc chain` refine layer, or a governance `preservation` (§10, §13) | For the explicit engine: raise the budget, or use `--engine bmc`/`induction` for this spec. For a refinement: lower the depth or narrow the `verify {}` domains of both layers; an inline `implements` seam can also be split into `fslc verify` of each layer, but `fslc refine` shares the same budget. `fslc diff` reports a cut-off direction as `unknown_budget` and fails its gate unconditionally (an `unknown_budget` finding, exit 1); `fslc mutate` reports the mutant as `inconclusive`, outside `kill_rate`, and fails a requested gate |
| `error` | usage (invalid argv) / parse / type / semantics / io | Fix argv for `usage`; otherwise follow `loc` / `expected` / `hint` |

`--engine auto` composes explicit and bmc: it tries explicit first (faster,
and can reach `proved`+`closure:true`) and falls back transparently to bmc
when explicit cannot decide the spec on its own — an unsupported feature
(`leadsTo`, nondeterministic/partial init) or `unknown_budget`. Every result
carries `engine: "explicit"` or `engine: "bmc"` naming whichever engine
actually decided it; a fallback additionally carries
`engine_fallback: {from: "explicit", reason: "...", kind: "unsupported"|"budget"}`
so a caller can tell a bounded bmc verdict from an unbounded explicit one
without re-deriving it. `auto` never changes the default engine (still
`bmc`) and is Rust-only, like `explicit` itself. See
`docs/design/DESIGN-explicit-engine.md` §6a.

`violation_kind`: `invariant` | `trans` | `ensures` | `type_bound` | `partial_op` | `deadlock` | `leadsTo`.

A `violated` result with `violation_kind` `invariant`/`type_bound` additionally
carries **blame assignment** (issue #170), localizing the counterexample
instead of just showing it: top-level `blame.conjuncts[]` (`{index, text,
holds, violating_bindings?}`) names which AND-conjunct of the invariant is
false when it is built from more than one (a 1-element list otherwise); each
action-bearing `trace[k]` (k≥1) gets its own `blame: {guards[], effects[]}`
naming the `requires` clauses and state-writing statements that fed the
blamed conjunct(s) at that step (a backward slice over the concrete
counterexample, not a new solver query). `fslc explain`'s counterfactuals
inherit both automatically. Vacuity findings (`vacuous_implication` /
`vacuous_leadsto`) gain `classification` (`insufficient_depth` |
`over_constrained`) and `blocking` (the other invariants making the
antecedent/trigger impossible, empty when it's merely unreached within
depth) — same shape as `reachable_failed`'s `unreached[].blocking_requires`.
`vacuity_probe_truncated` (§15) is a distinct claim — the reachability
probe was cut off by its state budget, not by depth — and carries its own
`classification: "probe_truncated"` rather than either of the above.
Blame identifies; it never proposes a repair (weakening a guard, dropping a
conjunct) — that would cut against the anti-hollowing principle. All of this
is strictly additive to the JSON contract.

Diagnostics that identify a faithfulness/intent gap may also carry
`faithfulness_class` plus `recommended_action`. Current classes are:
`partial_op_unguarded`, `frozen_only_invariant`, `intent_unexercised`,
`liveness_not_refined`, and `reachability_unknown` (`vacuity_probe_truncated`
only — the reachability probe was budget-cut, not merely intent-unexercised).
The tag is derived from existing `result` / `kind` / `violation_kind` fields
and is additive; consumers should keep reading the original classification
fields for detail.

Progress-preserving refinement failures are reported as `refinement_failed` with
`kind:"progress_lost"`, `violation_kind:"leadsTo"`, `impl_trace`,
`progress:{leadsTo, actions}`, and `faithfulness_class:"liveness_not_refined"`.

Every `verify` result includes `checked_to_depth` and one fixed `cost` object:
total `elapsed_s`, `solver` check count/time plus nullable common Z3 statistics,
and deterministic `properties` rows with per-property check counts and time.
Native and browser Workers use the same keys and nullability. Z3 counters are
the maximum snapshot observed across constituent checks rather than a sum of
possibly cumulative snapshots; timing values are nondeterministic. The explicit
engine uses the same shape with zero solver checks and null Z3 statistics. See
[`DESIGN-verification-cost.md`](../design/DESIGN-verification-cost.md). BMC `verified` is
explicitly bounded; when the final
depth first witnesses a reachable/vacuity/coverage fact during normal
exploration, `verified` also includes a `hint` that the state space is not
obviously saturated at that depth and suggests a larger `--depth` or induction.

Native and browser BMC set Z3 `random_seed` and `smt.random_seed` to `0`.
This makes each backend deterministic under a fixed Z3 build, but concrete
counterexamples and reachable/deadlock witnesses remain non-unique across
native and WebAssembly builds. Consumers must use the verdict and replayable
witness contract, not a particular satisfying assignment.

When a leadsTo is declared and the result is `verified` / `proved`,
`leads_to: { "<Name>": { "checked_to_depth": K } }` is attached
(no counterexample is a bounded guarantee up to depth K, the same standing as a
`verified` invariant). If induction discharges a ranked `leadsTo`, that entry is
upgraded with `proved: true`, `completeness: "unbounded"`, `proof: "ranking"`,
and `decreases`. When a `trans` is declared, the success output carries
`transitions_checked: ["Name", ...]`.

### Coverage diagnosis (actions that never become enabled)

```json
"action_coverage": {
  "checkout": {
    "covered": false,
    "name": "checkout",
    "blocking_requires": [ {"loc": {"line": 27}, "text": "requires stock[i] > 0"} ],
    "hint": "never enabled within depth 8; blocking requires: requires stock[i] > 0; ...",
    "faithfulness_class": "intent_unexercised",
    "recommended_action": "add a single-shot reachable for the action / raise --depth"
  }
}
```

The blocking requires clause is identified by a minimized unsat core when that is
cheap. For requirements `branches`, a false coverage diagnostic keeps the
internal split-action `name` and adds `display_name`.
Raw `action_coverage` retains generated-only sentinel entries, including
`covered:false`; verify, induction, and Worker JSON are unchanged. Public coverage
findings require a positive line/column on the authored primary origin, or else
the kernel action declaration. This shared source-location rule, not a name or
prefix, excludes scaffolding from runtime warnings, ledger coverage findings,
mutate dead-action notes, and HTML badges and coverage denominators. The current
CLI mutant-enumeration path cannot encounter generated-only dead actions.
With no eligible action the HTML
metric is `0/0`; HTML Raw Data still contains the original coverage map.
Affected ledger/HTML approvals become `drifted` (exit 0) because the rendering
changes; review the corrected artifact and re-approve it.

### Explicit action execution profile

When `--engine explicit` decides a run, its envelope additionally carries an
`action_profile` alongside `cost` and `action_coverage`:

```json
"action_profile": {
  "checkout": {"enabled": 5, "fired": 4, "no_op": 1}
}
```

For each declared action, `enabled` is the number of unique explored states
where at least one action instance was enabled; `fired` is the number of unique
successful `(state, action instance)` edges actually explored; and `no_op` is
the subset of fired edges whose successor equals the source state. This is
diagnostic coverage evidence, not a verdict or a solver metric. A bounded run
includes enabled states at its final depth but cannot fire their outgoing edges,
so `enabled` and `fired` need not be equal. The profile is deterministic because
explicit BFS visits canonical states and action instances once; it remains
present for `--engine auto` only when `engine:"explicit"` decided the result.
Symbolic BMC and induction results do not carry `action_profile`.

For `reachable_failed`, each `unreached` entry carries:

```json
{
  "name": "SoldOut",
  "classification": "insufficient_depth",
  "hint": "not witnessed within depth 3; try a larger --depth"
}
```

or, when the target predicate is unsatisfiable under type bounds/invariants:

```json
{
  "name": "TooHigh",
  "classification": "over_constrained",
  "blocking_requires": [{"kind": "type_bound", "name": "_bounds_x"}],
  "hint": "target predicate is unsatisfiable under type bounds/invariants (_bounds_x); ..."
}
```

### Literate Markdown FSL

Markdown files (`.md`) containing ` ```fsl ` fenced code blocks are accepted
directly by `fslc check`, `fslc verify`, and `fslc scenarios` — no flags or
extraction step needed. Lines outside fsl blocks are blanked (replaced with
empty lines) so that all diagnostic positions (line numbers, columns in error
messages and counterexamples) point to the original Markdown document.
Multiple fsl blocks in one document are treated as one compilation unit, so
definitions can be split across sections:

    # Cart invariant

    ```fsl
    spec Cart {
      state { count: 0..3 }
      init  { count = 0 }
    ```

    ## Actions (continued in a second fsl block)

    ```fsl
      action inc() { requires count < 3  count = count + 1 }
      invariant Bounded { count >= 0 and count <= 3 }
    }
    ```

A `.md` file with no ` ```fsl ` fences is rejected with a clear diagnostic.
Non-fsl fenced blocks (` ```python `, etc.) are ignored. `use`/compose paths
resolve relative to the Markdown file's directory (same as for `.fsl` files);
a literate `.md` may `use`/compose `.fsl` files this way, but using another
`.md` file as a compose target is not supported.

`check`, `verify`, and `scenarios` are the only commands that extract fences
this way. Most other commands that read a spec path (`lint`, `migrate`,
`fmt`, `kernel`, `conformance`, `explain`, `mutate`, `typestate`, `testgen`,
`testplan`, `html`, `ledger`, `analyze`, `diff`, `refine`, `replay`, `sweep`, `counterexample export`,
`db check`/`observe`, `compat check`, `domain check`/`analyze`/`expand`/`generate`/`replay`/`testgen`,
`ai check`/`replay`/`compat`,
`causal check`/`analyze`/`diff`/`ledger`/`observe-expectations`/`verify-expectations`,
`document generate`/`claims`/`check`, and `approval create`/`check`/`diff`) reject a `.md` input as an input-kind
error instead: `result: "error"`, `kind: "usage"`,
`diagnostic_code: "FSL-INPUT-LITERATE-UNSUPPORTED"`, a message naming the
commands that do support literate input, and a `loc` that names the input
file rather than a spec position. This keeps a Markdown document passed to an
unsupported command from being misreported as a spec syntax error at the
position of the Markdown's own first non-fsl character. `chain` (its
positional is a project manifest, not a spec) and `db import` (its positional is
a SQL/Prisma schema artifact) are not spec-path commands in this sense.
`approval create`/`check`/`diff`'s positional is always parsed as an FSL spec
regardless of `--kind` -- `--kind requirements_document`'s legitimately
`.md`-shaped input is `--artifact`, never the positional -- so the guard
above applies to their positional the same way it does for every other
registered command, checked immediately after the positional is resolved and
before `check`/`diff` read `--record` (#980). `ai eval`/`regress`/`drift`
already parse `.md` input through their own `load_ai_project` frontend
(success on a valid literate AI project, a clean semantic error otherwise)
and are unaffected by this change. See
`rust/fslc/src/literate_access.rs`'s `LITERATE_EXCLUDED` for the measured
reason each is excluded.

## 8. Recommended workflow: make proved the standard

1. Write the spec → `fslc check` (the fast syntax/type loop)
2. `fslc verify --depth 8` → if violated, fix using the trace.
   Confirm that the intended scenarios are witnessed by reachable
3. `fslc verify --engine induction` → if `proved`, done
4. If `unknown_cti`, read the CTI (the k+1-state trace). The starting state of
   the CTI is a **ghost state** that "satisfies all invariants but is actually
   unreachable." Add an **auxiliary invariant** that excludes it (one that is
   itself a truth of the domain) and return to step 3

Useful auxiliary invariants are domain truths such as "if attempts == 3, locked,"
"only Captured has a refund," and "no duplicates in the queue," rather than
proof-only artifacts.

## 9. Idiom collection

### Option instead of a sentinel value

```fsl
cart: Map<UserId, Option<ItemId>>      // do not use a sentinel like -1
struct Reservation { item: Option<ItemId>, qty: Qty }  // optional fields can be written directly too
action checkout(u: UserId) {
  requires cart[u] is some(i)          // i is bound here
  requires stock[i] > 0
  stock[i] = stock[i] - 1
  cart[u] = none
}
```

### A domain type instead of a hand-written bounds invariant

```fsl
type Qty = 0..5
state { stock: Map<ItemId, Qty> }      // do not write NoNegativeStock (automatic)
```

### Guarding partial operations (either the requires form or the if form)

```fsl
action take()  { requires q.size() > 0  x = q.head()  q = q.pop() }
action drain() { if q.size() > 0 { x = q.head()  q = q.pop() } }
```

Forgetting the guard is detected as a `partial_op` violation (it does not break silently).

### Talking about a Seq in an invariant: forall with an index guard

```fsl
invariant QueuedAreQueued {
  forall i in 0..2 {                   // 0..capacity-1
    i < queue.size() => jobs[queue.at(i)].st == Queued
  }
}
```

An out-of-range `at()`/index (or `head()`/`pop()` on an empty sequence) is a
`partial_op` violation in property context. Guarding with `i < q.size() => ...`
short-circuits the read and keeps the property defined. Unlike `/` and `%`, Seq
reads never totalize to an unspecified symbolic value.

### Aggregating over a Seq: the index / domain-type idiom

```fsl
type Idx = 0..3                        // a domain type covering up to capacity-1
invariant BalanceMatchesLog {
  balance == sum(i: Idx of log.at(i) where i < log.size())
}
```

`sum`/`count` range over a domain type, but restricting to the live prefix with
`where i < size` turns it into a **fold over the Seq**.

### Two-dimensional data (rooms × slots, etc.): flatten to a single key

**Nesting Maps** like `Map<RoomId, Map<SlotId, …>>` is not allowed (§2).
Flatten the two axes into a single product domain type, and recover the axes
with `/` and `%`:

```fsl
const SLOTS = 4
type RoomId = 0..2
type Cell   = 0..11                       // ROOMS*SLOTS - 1
state { holder: Map<Cell, Option<UserId>> }
// c / SLOTS = room, c % SLOTS = slot
reachable Room1Full {
  forall c: Cell { c / SLOTS == 1 => holder[c] != none }
}
```

When the axes are few and have names (e.g. a fixed 5-period weekday), there is
also the option of decomposing into struct fields, but if quantification is
needed, flattening is the default.

### Use a ghost variable to talk about history (the past)

```fsl
state { ever_locked: Map<UserId, Bool> }   // "was locked at least once"
// in the locking branch, ever_locked[u] = true
reachable RecoveredAfterLock {
  exists u: UserId { ever_locked[u] and session[u] }
}
```

Since reachable / invariant look only at state, to talk about "Y after X" as a
**fact of the state**, push the history into the state (a ghost variable).

### When to use a history ghost variable vs. leadsTo

| What you want to write | Means |
|---|---|
| "Was X at least once" (a fact of the state) | Ghost variable + invariant / reachable |
| "Keep X true until Y, and Y may or may not happen" | `unless Name { X unless Y }` |
| "Once it becomes X, eventually Y" (a response property) | `leadsTo` + `fair action` if needed |
| "Keep X true until Y, and Y must eventually happen" | `until Name { X until Y }` |

Example: in a FIFO mutex, "a process that has entered the wait queue eventually
obtains the lock" is
`leadsTo WaiterGetsLock { forall p: ProcId { waiters.contains(p) ~> ... } }`.
If progress depends on a specific action such as `release_handoff`, add `fair`
(see `specs/mutex_queue.fsl`).

### Auxiliary invariants from a CTI (strengthening induction)

Look at the starting state of the `unknown_cti` CTI and turn "a combination that
does not occur in reality" into an invariant:

```fsl
// CTI: queue = [0, 0, 0] (the same job tripled) → state that there are no duplicates
invariant NoDupQueue {
  forall i in 0..2 { forall j in 0..2 {
    (i < j and j < queue.size()) => not (queue.at(i) == queue.at(j))
  } }
}
```

For the common "monotone counter" idiom — an `Int` or `Map<K, Int>` state
variable that only ever moves in one direction — `unknown_cti` results carry
an additive `suggested_invariants: [<expr>, ...]` field (and the matching
sentence appended to `hint`) whenever the CTI's counter starts on the
unreachable side of its concrete initial value (e.g. a huge or negative
"ghost" start a real execution could never produce). This is a heuristic
computed by diffing the CTI trace against the concrete init (not a proof of
global monotonicity), so treat it as a starting point:

```fsl
// CTI: audit = -101 (only increases in this trace, but starts below its
// init value 0) → suggested_invariants: ["audit >= 0"]
invariant AuditNonNeg { audit >= 0 }
```

A `Map<K, Int>` counter suggests the `forall`-quantified form
(`forall k: K { audit[k] >= 0 }`) when every key shares the same initial
value. If no monotone counter is detected, or the CTI start does not violate
the would-be bound, no suggestion is added and `hint` is unchanged.

## 10. Refinement (fidelity of a detailed spec)

After first `verify`-ing / `prove`-ing the abstract spec (abs), check with
**`fslc refine`** that a detailed spec (impl) close to the implementation does
not depart from the behavior of abs (see `DESIGN-refinement.md`).

Write the mapping in a **separate file** (do not pollute the impl/abs `.fsl`):

```fsl
refinement CartImplRefinesCart {
  impl CartImpl
  abs  ShoppingCart

  maps auto
  map stock[i: ItemId] = impl_stock[i] - reserved[i]
  map cart[u: UserId]  = impl_cart[u]

  action add_to_cart(u: UserId, i: ItemId) -> add_to_cart(u, i)
  action impl_checkout(u: UserId)          -> checkout(u)
  action reserve(i: ItemId)                -> stutter
}
```

- `map <abs var> = <impl expr>` — a scalar abstract variable.
- `map <abs var>[<binder>] = <expr>` — element-wise mapping of a Map (enumerate the key type, which is bounded).
- `maps auto` — optional identity defaults. For same-named compatible state
  variables with no explicit `map`, it synthesizes `map x = x`; for same-named
  compatible actions with no explicit correspondence, it synthesizes
  `action f(params...) -> f(params...)`. Explicit entries override the defaults.
  Action parameters are matched **by name, never by position**: each abstract
  parameter binds to the impl parameter sharing its exact name (so a pure
  reorder still auto-maps), in the abstract action's parameter order. Auto
  never guesses — a same-name action pair with a different arity, a surplus
  impl parameter, a renamed parameter, or any abstract parameter with no
  same-named impl counterpart is a located `kind: "type"` error; write an
  explicit `action ... -> ...` correspondence for that pair instead.
  Incompatible same-name state candidates are likewise reported as
  `kind: "type"` errors.
  Auto actor compatibility, including `covers`, mixed editions and compose,
  follows §13.2; see [DESIGN-refinement](../design/DESIGN-refinement.md),
  “#1441: source-actor auto mapping and breaking-source migration” and
  “#1456: requirement-action own actors and breaking-source migration”.
- `action <impl>(<formal params>) -> <abs>(<expr>) | stutter` — required for every impl action.
  Formal params may be bare names or `name: Type` annotations matching the impl action declaration.
  `stutter` is an internal step in which the abstract state does not change.

All four authoring routes—standalone `action`, inline `implements` action,
requirement-action `maps`, and auto/identity synthesis—resolve to the same typed
action-correspondence IR. The common validation checks impl identity and typed
parameters, target identity/arity, argument expressions, and auto-mapped actor
compatibility. A duplicate diagnostic names both origin kinds and source
locations; explicit entries still take precedence over auto synthesis.

Refinement maps and action arguments use the same expression grammar and type
rules as ordinary specs, including `if <condition> then <expr> else <expr>`.
Both branches are name- and type-checked even when the condition is constant.

Distinct nominal enums require an explicit, named enum mapping. For a
member-wise bijection, declare a conversion in the refinement and call it from
a state map or action argument:

```fsl
enum conversion use_case_stage UnitOfWorkStage -> CommandStage {
  Received -> Received
  Validated -> Validated
  Executing -> Executing
  Completed -> Completed
}
map command_stage[c: Command] = convert(use_case_stage, stage[c])
```

Both endpoints must be enum types. Every source and target member must appear
exactly once; unknown, duplicate, or missing members are `kind: "type"`
(exit 2) errors at the conversion site. Conversion is never inferred from
declaration order or equal member spelling, so direct assignment between the
two enums remains a type error. The target may be a requirements `process`
stage enum synthesized by lowering; use its checked Kernel name (visible in
`fslc kernel`), such as `CommandStage`. Existing raw `replay --from-log` and
causal observation mappings have no typed impl model and reject conversion
declarations/calls with a located type error; migrate those mappings to a typed
refinement before using `convert`.

For a many-to-one boundary, declare a source-total abstraction and use its
distinct call form:

```fsl
enum abstraction lifecycle ImplStage -> AbsStage {
  Received  -> Pending
  Validated -> Pending
  Completed -> Done
}
map status = abstract(lifecycle, stage)
```

Both endpoint enums must be non-empty, and every source member must appear
exactly once. Duplicate or missing source
members and unknown or wrong-nominal members are located type errors. Repeated
targets are allowed; target members omitted from all rows are intentionally
unused by this abstraction. `enum conversion` remains a bijection and still
rejects duplicate or missing targets. Conversion and abstraction names share
one refinement-local namespace, and `convert`/`abstract` cannot be
interchanged. Both forms are independent of enum declaration order. Raw
production-log and causal mappings reject abstractions too because they have
no typed implementation model.

When an abstract generated enum is the binder for a map whose implementation
uses a design enum as its key, convert the binder in the reverse direction,
for example `map column_exists[c: Column] =
design_exists[convert(column_key, c)]` with `column_key Column -> DesignColumn`.

```bash
fslc refine specs/cart_impl.fsl specs/cart_v1.fsl specs/cart_refines.fsl --depth 8
```

Success: `result: "refines"` (exit 0). Violation: `refinement_failed` (exit 1)
with `kind` (`abs_requires_failed` / `abs_state_mismatch` / `stutter_changed_abs` /
`map_out_of_bounds` / `map_partial_op` / `abs_partial_op` / `abs_step_violation`), `impl_trace`, and the post-mapping
`abs_before` / `abs_after_*`. A static error (a missing map, an unknown action,
etc.) is `kind: "type"` (exit 2). `map_partial_op` is an action-correspondence
argument expression (`impl_action(a) -> abs_action(a / c)`) dividing by an impl
state variable that can be zero — action context (§3/§6's `partial_op` rule),
not the "no check" property context a refinement *state* map gets.
`abs_partial_op` is the corresponded abstract action's own body hitting a
partial operation (a zero divisor, `pop`/`head`/`at` on a too-short `Seq`) at
the mapped state of an impl step whose abstract guard holds: the abstraction
has no defined successor there, so the step cannot correspond (there is no
`abs_after_expected`). A false abstract guard is still `abs_requires_failed`.
A leading `let` of the abstract action is evaluated with its guard, so a
partial operation there (`let q = 4 / y` at `y = 0`) is not `abs_partial_op`:
like a failing abstract guard, it still stops with `result: "error"` (exit 2).
`abs_step_violation` is the corresponded abstract step being rejected by the
abstraction itself: its successor violates an abstract type bound, invariant or
`trans`, its `ensures` is false, or one of those checks hits a partial
operation (in an invariant or `trans`, only a `Seq` operation can: `/` `%`
are total there). The step cannot correspond whatever the impl's mapped state is, so
it fails even when the impl leaves the mapped state unchanged; `abs_violation`
gives the abstract `kind` and `name`, `abs_after_attempted` the rejected
successor, and there is no `abs_after_expected`. So abstract `ensures` and
invariants are checked on every corresponded step within `--depth` — but only
there: verify the abstract spec on its own as well (the `refines` note says
so).

Before any correspondence is checked, `refine` first verifies that the impl
spec is internally consistent on its own — does not violate its own type
bounds or invariants — within `--depth`. A refinement mapping cannot make a
broken impl meaningful, so this is checked independently of the abstraction
and the mapping: `result: "impl_violated"` (exit 1) with `violation_kind` and
a `note` explaining this is a property of the refinement *input*, not a
fidelity failure. `impl_violated` is the one word for this outcome wherever it
appears: `refine`, an inline `implements` seam, and a governance
`preservation` (§13). It is never reported `refines`, and never folded into
`refinement_failed` (whose `kind`s above describe a mismatch *between* impl
and abs, not a defect in the impl alone). `fslc diff` surfaces the same
condition as an `impl_violated` finding and fails its gate unconditionally
(not subject to `--forbid`), since a self-violating side makes the
comparison itself untrustworthy.

The correspondence check that follows visits at most a fixed number of
distinct impl states (50,000; no CLI flag — the same internal budget an inline
`implements` seam uses, §13). When it reaches that budget before deciding
within `--depth`, `refine` reports `result: "unknown_budget"` (exit 1) with
`states_explored` and a `hint`, never `refines`: the unvisited part of the
reachable set may still hold a mismatch. Lower `--depth`, or narrow the
`verify {}` domains (`instances`/`values`) of both specs. A chain check stops
at that link (`failed_link.kind` is `null`), a `fslc chain` refine layer fails
with `result: "unknown_budget"`, and a governance `preservation` reports
`unknown_budget` as its `result`. This changed verdicts: a refinement whose
correspondence check reaches the budget used to report `refines` (exit 0).
The self-consistency precondition above walks the same reachable set first and
stops at the same budget, with the same `unknown_budget` report: a
self-violation it did not reach is not searched for, and the correspondence
check does not run. An impl whose reachable set within `--depth` reaches the
budget is therefore `unknown_budget` even when the correspondence check would
have found a mismatch early; before this bound such a run either exhausted
memory in the precondition or, when it fit, reported the decided result.
`fslc diff` reports a direction cut off this way as `result: "unknown_budget"` with
`states_explored`, and adds an `unknown_budget` finding that fails the gate
whatever `--forbid` says (exit 1, never `no_semantic_change`); `fslc diff
--git` and `fslc approval diff` report the same. `fslc mutate` reports such a
mutant as `status: "inconclusive"` with `inconclusive.reason:
"unknown_budget"`: it is excluded from `kill_rate`, counted in
`summary.inconclusive`, and fails a requested `--fail-on-survivors` /
`--min-kill-rate` gate. Both used to report the cutoff as a pass
(`no_semantic_change` / exit 0, and `survived`).

A state variable `init` never assigns on any path (an `init if` reading an
unassigned `Bool`, for example) is a genuinely free initial value across its
full type domain, not a value silently defaulted — `refine` checks it that
way on both the impl and abs side. The self-consistency precondition above
and the init correspondence it feeds both reason over **every** concrete
initial valuation `init` permits, not one arbitrarily chosen default: an
impl-side initial valuation must not violate the impl's own semantics, and
its mapped state must be a member of the abs's own set of valid initial
valuations, for every such valuation on both sides. A same-named state
variable init leaves unassigned on *some* but not all paths (e.g. only
inside an `if` with no `else`) is not enumerated this way and keeps the
prior single-valuation behavior.

This closed two opposite defects and is a **breaking change** to `refine`'s
verdicts for a mapping where either side's `init` leaves a state variable
fully unassigned: a refinement that previously reported `refines` because
only the impl's default initial branch was ever checked may now report
`refinement_failed` (an initial-correspondence violation on another impl
branch was missed before); a refinement that previously reported
`refinement_failed` solely because the abs's default initial branch did not
equal the mapped impl state may now report `refines` (the impl state
matches a different, still-valid, abs initial branch).

Same-named impl/abs types must agree in kind, resolved domain bounds, ordered
enum members, and struct fields/types. `symmetric` is ignored; equal definitions
keep the impl entry. Different definitions, even unused ones, are `kind: "type"`
errors (exit 2), located at the `refinement` or inline `implements` declaration
and naming both specs. Governance retains the outer preservation location.
Give different definitions distinct names. **Breaking removal (#1599):** unequal
bounds may no longer share a type name; the boundary gallery's former exit 1
`refinement_failed/abs_state_mismatch` becomes exit 2 `error/type`.
Different enums may share member names without progress: ambiguous bare uses
still fail, while typed conversions/abstractions remain accepted. A refinement
with `preserve progress` rejects those declarations even if ordinals coincide.
Pulled-model safety violations fail closed as exit 2 `error/semantics`, naming
the property and pulled model without a trace, never as `refines`/`progress_lost`.
With distinct type names, out-of-domain mapping/argument errors retain their
`map_out_of_bounds` behavior and false abstract guards still take precedence.
Entity domains derived from `verify { instances Item = N }` follow the same rule.
A conflict reports each spec's instances and whether its verify block or a CLI
override supplied them. Align the verify blocks; with `verify` and `refine`,
`--instances Item=N` can align both direct specs. **`check` has no `--instances` flag.**
Composed conflicts name the public type (for example `p.Item`) and retain both
component files, instance counts and verify-block sources. Align the component
verify blocks; compose does not support that instances override. Diagnostics use
public names without changing internal type/member identities or member syntax.
Breaking removal: the default check/verify of `inline_implements_bounds/impl_spec_extra.fsl`
(Item=1 versus Item=3) changes from exit 1 `refinement_failed` to exit 2 `error/type`
at its `implements` declaration; an equal shared instances override still refines.

Boundary audit (#1599): base `a52bef4931adc67ea2b1fdca827e2318b4ae25ab`,
measured product/test head `9eaaea7d1c526ede6a13408ea414616ddadccf69`;
`check`610 corpus forms (606 existing,4 added),39 next-edition controls,
60 typed mappings,20 rewritten inline triples,2 expression triples and39
acceptance cells, full JSON/stderr/exit twice per tree. Existing corpus
check ok→error set: **empty**. Inline check removals are the original D-bound
fixtures of issue_1296_abs_partial_op/issue_1302_abs_step_violation and the
inline_collision_is_located_at_implements/governance_check_collision cells;
their verify routes also become type errors. Refine removals: cases1/2a/3a/3b,
domain_bounds/domain_lower/kind_change/struct_named_type, matching-ordinal
member collision, chain collision and the original shared-member progress
expression_agreement form. Renamed companions retain successful agreement.
Boundary/entity refinement_failed→type error is a verdict-class change.
The original2 runtime/2 CLI/3 mutation Seq controls change diagnostic priority,
not rejection or invalid counts/gates: declaration validation runs first.
Authored standalone1:1 denotes the full ten-byte `refinement` keyword span
(end1:11), not an unknown-location fallback. Inline keeps the `implements`
token; distinct-name carrier companions still fail at the authored map token.
Previously reported type errors also gain loaded impl/abs identities.

### Chain checking (composition of mappings)

The end-to-end fidelity of a layer chain (business ⊒ requirements ⊒ design …)
can be checked **directly by composition** when you line up `(spec mapping)` in
sequence:

```bash
fslc refine bot.fsl  mid.fsl bot_refines_mid.fsl  top.fsl mid_refines_top.fsl --depth 6
#            ^impl    ^abs1   ^map(impl→abs1)      ^abs2   ^map(abs1→abs2)
```

It composes the adjacent mappings (state α_AC = α_BC ∘ α_AB, actions a→b→c /
stutter) and checks bottom ⊒ top. On success it returns the composed
`action_map` and the layer ordering `chain`; on failure it returns the first
broken link `failed_link: {from, to, kind}`. At the same depth K, if every
adjacent link refines, their composition refines; the converse does not hold,
because a direct endpoint success does not establish the middle layer's
contract. For example, Low may stop after one step while Mid permits a second
step that Top forbids, so direct Low→Top succeeds while Mid→Top and the chain
fail. The chain requires every adjacent contract to hold
(`docs/design/DESIGN-refinement.md` §7, example `examples/refinement_chain`). For
manifest-driven `fslc chain`, links may use different `refine_depth` values;
the minimum link depth is the conservative end-to-end bound.
Only the case where an argument expression reads the state of an intermediate
layer is unsupported.

Recommended workflow: **a human/LLM reviews abs → the LLM elaborates impl →
`refine` guarantees fidelity**. The `ensures` / invariants of abs are not
re-checked in refine; it is assumed that they have been separately verified on
the abs side.

What `refine` guarantees is **inclusion of safety** (impl does not break the
guards/invariants of abs). **Liveness (`leadsTo`) does not propagate** — since
refine allows stutter, even if impl drops the progress that abs guaranteed with
`fair`, the result can still be `refines` (the mapping does not require fair
annotations). To opt into checking the abstract response on the lower layer,
write `preserve progress` in a standalone refinement mapping file (an inline
`implements { }` block rejects it — see §13.2):

```fsl
refinement DesignRefinesReq {
  impl Design
  abs  Req
  map st = ...
  action enqueue(c) -> stutter
  action answer(c)  -> answer(c)
  action refuse(c)  -> refuse(c)

  preserve progress {
    respond EveryRequestHandled by answer, refuse
  }
}
```

This pulls the named abstract `leadsTo` through the state mapping and checks
`P(α(impl_state)) ~> Q(α(impl_state))` on impl executions. If the lower layer can
spin forever or deadlock while the abstract response remains pending, the result
is `refinement_failed` with `kind:"progress_lost"` and
`violation_kind:"leadsTo"`. `progress_failure` distinguishes
`lasso_blocks_progress` from `deadlock_or_stall_blocks_progress`. The `by`
actions are review metadata and must name impl actions; they do not create
fairness or prove implementation conformance. Fairness still comes from
lower-layer `fair action` declarations.
Abstract Int/Bool constants become literals in P/Q, `decreases`, domains and nested filters.
Impl-constant/read const-state collisions are refused; `check` already rejects const-namesake locals.
Property patterns matching free impl names from lexically used maps are refused outside
readable continuations too. Used-map RHS patterns cannot reuse property binders or sibling pattern names.
Rename the abstract or mapping pattern binder to repair these collisions. Ordinary source scope is unchanged.
Direct refine gives exit 2, `kind:"type"`, `loc` at `respond`. The ambiguous const/state
bound witness gives refine exit 2 versus abstract-only BMC exit 1 `violated`.
Unused maps/legal locals stay admissible; outer capture is refused. Only nested diagnostics may rename.
For a violated abstract constant bound with the impl constant renamed, or a body constant
without an impl namesake, former exit-2 errors now report exit 1 `progress_lost`.
For unbounded proof, keep using a lower-layer `leadsTo ... decreases ...` and
`verify --engine induction`.

## 11. Composition (compose)

**Merge with namespaces** several verified component specs into a single system
specification. After expansion it becomes an ordinary single spec, so `verify` /
`prove` / `scenarios` / `Monitor` / `replay` / `testgen` / `refine` can be used
as-is (design: `DESIGN-compose.md`).

```fsl
compose OrderSystem {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  state { orders_linked: Int }
  init  { orders_linked = 0 }

  // synchronized action: execute the actions of several components in the same step
  action checkout_and_pay(u: cart.UserId, p: pay.PayId) =
      cart.checkout(u) || pay.capture(p) {
    requires pay.payments[p].st == Authorized
    orders_linked = orders_linked + 1
  }

  // excluded from standalone execution (fires only via synchronization)
  internal cart.checkout
  internal pay.capture

  invariant LinkedNonNeg { orders_linked >= 0 }
  reachable PaidOrder {
    exists p: pay.PayId { pay.payments[p].st == Captured }
  }
}
```

- `use <SpecName> as <alias> from "<relative path>"` — the path is relative to
  the compose file. The spec name must match the in-file name. The alias must be
  unique within the compose. Nested compose is not allowed.
- A component's types/state/actions are referenced as `alias.Name`.
- **Synchronized action** `action <name>(...) = <a>.<act>(...) || <b>.<act2>(...) { ... }`:
  merges the requires / body / ensures of each component action, and the
  additional statements may only assign to the composition-side state
  (synchronizing two actions of the same component is not allowed).
  Breaking removal (#1334): additional statements assigning component state via physical names such as `alias__x`, which previously passed `check`, now produce a located error too.
  Reference arguments may read only header parameters, consts, and state, not additional-item `let` bindings (breaking removal, #1442).
  Component free value reads and update roots resolve against the component's original declarations before merging, so composition cannot rescue an otherwise undeclared reference (breaking removal, #1442); public names remain legal, and an enum read that needs its owner protected still displays the source member spelling.
- Fairness is not inherited through synchronization. If a fair component action
  is referenced by a non-fair synchronized action, `check` / `verify` emits a
  `fair_not_inherited` warning in JSON `warnings`; write
  `fair action <name>(...) = ...` when the synchronized action must be fair.
- Synchronized action arguments are structurally compatible by bounded integer
  domain, not by declared type name. Passing a `core.TaskId` value to an action
  parameter declared as `NoteId` is valid when the underlying value range fits
  the target type. This is the intended compose behavior, not an incidental
  naming accident. Repro: with `TaskId = 0..2` and `NoteId = 0..2`,
  `action sync(t: core.TaskId) = core.choose(t) || note.attach(t) { }`
  produced `result:"ok"` from `fslc check` and `result:"verified"` from
  `fslc verify --depth 1`. With `NoteId = 0..1`, the same compose still passed
  `check` but `verify --depth 1` returned `result:"violated"`,
  `violation_kind:"type_bound"`, invariant `"_bounds_note.last"`, for
  `sync(t: 2)`. Recommended idiom: use same-range component-local domain types
  for intentionally shared IDs; if a target domain is narrower, add an explicit
  `requires` guard on the synchronized action or model a conversion in one
  component.
- `internal <alias>.<action>` — excludes that action from interleaving.
- An ordinary `action` (without `=`) can also be written (a glue action).
  A glue action reads and writes composition-side state and component state directly via `alias.var`.
  Compose `init` can also initialize component state via `alias.var`; overlapping a component's own `init` assignment is rejected as duplicate initialization.
- Names and `__`: a name you declare in a `spec`, `requirements`, `business`, or
  `compose` document (a spec, action, parameter, `let`, state variable, property,
  type, const, enum member, struct field, process, stage, actor, or `use` alias)
  must not contain `__`. `fslc check` rejects one at the declaration
  (`kind:"name"`, exit 2); a compose component's declaration is reported at the
  parent's `use`. `__` is reserved for the names lowering generates: the
  physical name `alias__x` of a compose component declaration and the split
  `submit__b1` of a requirements `branches` action. A generated name may still
  be referenced, as a refinement mapping does with `submit__b1`.
  Breaking removal (#1600): in next-edition requirements, a relation role
  (`Claim belongs to User as __owner via approve`), a resource name
  (`resource __r: Amount = 0`), and a measure name
  (`measure __m = count Claim in Approved`) must not contain `__` either.
  These previously passed `check`; `fslc check` now rejects one at the
  declaration (`kind:"name"`, exit 2) with, for example,
  `relation role name '__owner' contains '__', which is reserved for the names FSL generates ...`.
  Rename the role, resource, or measure without `__` and update its references.
  Breaking removal (#1602/#1603): this also covers goal and policy IDs,
  requirement IDs (`rule` in the next edition), scenario IDs (`acceptance` in
  legacy), forbidden IDs, decision-table IDs (`decide`), and glossary aliases,
  in both editions wherever the construct exists. These previously passed
  `check`; they now fail with exit 2, `kind:"name"`, and structured `loc` at the
  ID or alias, subject to the legacy locator limitation below. The existing
  separator message uses the position `goal`, `policy`,
  `requirement`, `scenario`, `forbidden`, `decision`, or `glossary alias`.
  Rename the ID or alias without `__` and update its references.
  Next-edition rule IDs are located only from their header token spans, including
  rules with deadlines or IDs preceded by comments. Explicit `requirement`
  blocks also remain supported and their IDs are checked in both editions,
  with or without deadlines. Only rule-generated requirement
  copies are excluded from the duplicate ID check.
  Known legacy-syntax limitation (also for explicit blocks):
  split-spelled requirement, acceptance and forbidden IDs such as `REQ - __1`
  are still rejected, but the fallback locator may report the declaration keyword
  or a later repeated normalized spelling rather than the ID tokens. This
  syntax-specific location limitation is not fixed here.
  A next-edition rule-property label containing `__` or naming a literal
  (such as `none:`) remains rejected with exit 2 and
  `diagnostic_code:"invalid_rule_property_name"`. Its kind changes from
  `semantics` to `name`, with structured `loc` at the statement's start, not
  the label. Its diagnostic wording is unchanged; when checking a file, the
  message suffix changes from `at L:C` to `at <source-file>:L:C`, like other
  located name errors.
- JSON display: a generated name is published with `.` in place of its `__`,
  in one spelling for every output: the physical name `alias__x` is `alias.x`,
  and a branch split `submit__b1` is `submit.b1` (state keys, action names,
  invariant / reachable names, diagnostics, traces, scenarios, and the Monitor —
  all of them). A component declaration's public name is built from its alias
  and its own name. Every command that names one — verify (including
  `cost.properties[].name`), sweep, scenarios, explain, testgen, conformance
  (including outcome names such as `acct.Inv`), testplan, mutate (targets and the
  dead-action note), refine, diff, html, ledger, analyze (including graph IDs
  such as `action:acct.go` and `action:submit.b1`), `check --strict-tags`
  warnings, and `replay` output including `state_mismatch.action` — uses this
  form, including in names derived from an action such as
  `_requires_failed_acct.go`. Only the Public Kernel (`fslc kernel`) keeps the
  internal name. A v1 `replay` trace may write a generated action in its public
  spelling or as its internal name (`acct.go` or `acct__go`, `submit.b1` or
  `submit__b1`); both name the same action (see
  `docs/design/DESIGN-replay-trace.md`). The native implementation owns all of
  these spellings, and the names the engines build such as `_partial_<action>`
  and `_bounds_<state>`, in one module, `rust/fsl-core/src/naming.rs`.

```bash
fslc check  specs/order_system.fsl
fslc verify specs/order_system.fsl --depth 8
fslc scenarios specs/order_system.fsl
```

## 12. The bridge to implementation

After proving a specification, there are three entry points for wiring it to the
implementation (see `DESIGN-bridge.md`).

| Means | Use |
|---|---|
| `fslc.runtime.Monitor` | A concrete interpreter of the spec (no Z3 needed). Embed it in the implementation for runtime checking |
| `fslc replay` | Check a real system's event-log JSON against the spec |
| `fslc counterexample export` | Export a bounded safety-invariant verifier counterexample to a closed `reproducer.v1` JSON artifact (slice 1 of #885; not `replay-trace` or `testgen-trace`) |
| `fslc testgen` | Generate a conformance-test scaffold — pytest (default), Vitest (`--target vitest`), Swift Testing (`--target swift`), kotlin.test (`--target kotlin`), Dart `package:test` (`--target dart`), or PHPUnit (`--target phpunit`) (wire the implementation into the Adapter) |
| `fslc testplan` | Select the bounded `conformance` vectors — the accepting ones **and** the `requires_failed` ones `testgen` never emitted — into a closed `test-plan.v1` JSON plan. Pass a spec at the implementation's layer granularity |

Recommended workflow: **`verify` / `prove` the spec → generate the scaffold with
`testgen` → wire the implementation into the `Adapter` → run the tests**. `Monitor`
is used as an oracle in random-walk testing.

**Layer selection:** generate conformance tests from the spec at the **same layer
granularity as the implementation** you are checking. From an **upper** layer you
may reuse **`forbidden` (negative) scenarios only** — forbidden traces remain
sound under refinement, but upper-layer **positive** scenarios (`acceptance`,
`cover`, random-walk witnesses) can falsely fail a legitimately refined
implementation because refinement is forward simulation, not backward replay of
every abstract positive trace. **Example:** `examples/refinement_chain/top.fsl`
lets `finish` from initial `TOpen`; `mid.fsl` requires `MOpen → MReview` first —
a `ChainTop` positive `finish` trace must not be used as a `ChainMid`
implementation test. See `docs/design/DESIGN-layers.md` (refinement chain → design-layer
testgen/replay; requirements `acceptance` → that layer's scenarios/testgen).

`testgen` separates a language-independent scenario-collection core (`scenarios`)
from per-target emitters, so the same scenarios render to multiple harnesses. In
the native implementation all six emitters consume one validated adapter built
from Public Kernel v1, scenario JSON, and the versioned fixed-seed
`testgen-trace.v1` conformance trace; they do not read the private model or AST.
Schema/version/spec mismatches, unknown state/action/parameter names, and
malformed input fail closed. Compose uses an explicit checked names/order bridge because
Public Kernel intentionally rejects incomplete multi-file provenance; it enters
the same adapter rather than falling back after an export error.
Before the walk, output-path handling, or generation, `testgen` checks inline
`implements` and governance preservations through the same fold as `scenarios`.
If either fails, it returns the `verify` owner's failure envelope and exit
(1 for a failed seam/preservation, 2 for a contract error, 3 for an internal error), not a scenarios-shaped
envelope, and writes no harness. Passing documents and non-strict partially
reachable scenarios retain their ordinary generation behavior.

The fixed-seed walk is a concrete Monitor run capped at 100 steps, and it stops
early when no action is enabled; it is **not** bounded by `--depth`. It can
therefore reach a violation that the bounded verification `testgen` runs first
proved absent within `depth`. Because the Monitor rolls a violating step back,
recording that step would state that the action is a no-op -- an expectation no
FSL contract makes. `testgen` instead reports the violation with the same
`result:"violated"` envelope, exit code, property, step, and replayable trace
`verify` reports, and writes no harness.

`fslc counterexample export` (reproducer slice 1) runs the same bounded
verification as `verify`, but when the result is a **safety invariant**
violation it also writes a closed `reproducer.v1` JSON file (`-o` required).
The stdout envelope remains the violated `verify` result plus
`reproducer.exported_to`; it is not a replay-trace or testgen-trace input.
v1 rejects `leadsTo`, refinement documents, induction/CTI, nondeterministic
`init`, and non-invariant violations with exit 2. Stage-2
`testgen --reproducer` (slice 2) is not implemented yet.

- `--target pytest` (default): emits Python tests that import `fslc.runtime.Monitor`
  and drive the random walk live as the oracle.
- `--target vitest`: emits a self-contained TypeScript (Vitest) file. Deterministic
  scenarios and forbidden-rejection assertions translate directly; the random walk
  is **baked at generation time** — the concrete Monitor runs the fixed-seed walk and
  the `(action, params, expected_state)` trace is embedded as a static fixture, so the
  generated tests need **no `fslc`/Python at runtime**. The output extension defaults
  to `<spec>.test.ts`. Native generation refuses integers outside ±(2^53−1) before
  emitting an artifact (exit 2, existing semantic-error envelope), naming the value
  and its input-vector path rather than emitting a lossy JavaScript number.
- `--target swift`: emits a self-contained Swift Testing file (`import Testing`,
  `@Test`, `#expect`; **not XCTest**). Same baked-walk design as Vitest. Dynamic
  state is `[String: Any]` with a bundled deep-equality/partial-match helper; an
  Option `None` bakes as the self-contained `FSLNull.instance` sentinel (no
  Foundation). Until `makeAdapter()` is wired every test fails (its
  `FSLNotWired` error says "adapter not wired"); with `--allow-unwired` every test
  is disabled via `@Test(.enabled(if:))` instead. Output defaults to
  `<SpecName>ConformanceTests.swift`.
- `--target kotlin`: emits a self-contained kotlin.test file (multiplatform; the
  JVM delegates to JUnit). Same baked-walk design. Dynamic state is
  `Map<String, Any?>`. The partial-match helper recurses by expected map keys;
  before leaf equality it normalizes Byte/Short/Int/Long to Long recursively
  through List/Map values. List order/length and exact maps inside lists remain
  significant; Float/Double values are not coerced. Int-range literals remain
  unsuffixed, wider i64 values use `L`, and i64 minimum uses `Long.MIN_VALUE`.
  Adapters should read numeric params through `Number`, not cast them to `Int`.
  Until `makeAdapter()` is wired (it returns `null`) each test fails
  with `fail(ADAPTER_NOT_WIRED)`. kotlin.test has no portable runtime skip, so
  `--allow-unwired` is rejected for this target (exit 2) rather than generating
  tests that return early and pass. Output defaults to
  `<SpecName>ConformanceTest.kt`.
- `--target dart`: emits a self-contained `package:test` file (also runs under
  `flutter test`). Same baked-walk design. Dynamic state is `Map<String, dynamic>`;
  Dart's `==` is reference-based on collections, so the bundled `assertPartial`
  recurses by the expected keys and compares leaves/sequences with the `equals`
  matcher (re-exported by `package:test`, so the only dependency is `package:test`).
  Until `makeAdapter()` is wired every test fails with its "adapter not wired"
  `UnimplementedError`; with `--allow-unwired` a top-level probe sets `skip:` on
  each `test()` instead. Output defaults to `<spec_name>_conformance_test.dart` (snake_case, the
  `_test.dart` suffix the runner expects).
- `--target phpunit`: emits a self-contained PHPUnit file (PHP 8.1+ / PHPUnit 10+,
  `declare(strict_types=1)`). Same baked-walk design. Dynamic state is an
  associative `array`; leaves are compared with `assertSame` (`===`), which keeps
  `int`/`float`, `bool` and `null` from coercing (PHP's loose `==` would conflate
  `0 == "0"` etc.). `assertPartial` recurses by the expected keys (maps match
  order-independently; list-shaped values also pin length). Until `makeAdapter()`
  is wired `setUp()` fails every test ("adapter not wired"); with
  `--allow-unwired` it marks them skipped instead. Output defaults to
  `<SpecName>ConformanceTest.php` (PSR-4 class = file name).

**Unwired adapters fail (issue #1200, breaking).** Until the generated adapter
is wired, every generated test of every target **fails** with a message that
starts `adapter not wired`: a suite that never calls the implementation is not
evidence that it conforms, so it must not turn a runner green. To get the
earlier skip-until-wired scaffold, generate with `--allow-unwired`; the output
is then byte-identical to the pre-#1200 scaffold (pytest `pytest.skip`, Vitest
`test.skip`, Swift `.enabled(if:)`, Dart `skip:`, PHPUnit `markTestSkipped`).
The choice is made at generation time so it is visible in the generated file.
Kotlin rejects `--allow-unwired`. A green run of an `--allow-unwired` suite
whose tests were all skipped still proves nothing. `fslc chain`'s `[impl]`
table therefore names its evidence: `report = "<JUnit XML file or directory>"`
written by the command (exit 0 passes only with at least one executed test and
no failing one), or `evidence = "exit_code"` for a command that is not a test
suite (decided by the exit status, flagged `detail.evidence: "exit_code_only"`
and in top-level `warnings[]`). With neither, an exit-0 run is `indeterminate`
(exit 1); an unknown `[impl]` key is a parse error (exit 2). Breaking for
manifests written before #1200: add `report` or `evidence = "exit_code"`
(`docs/design/DESIGN-layers.md` §7).

```python
from fslc import Monitor

mon = Monitor("specs/cart_v1.fsl")
mon.reset()
r = mon.step("add_to_cart", {"u": 0, "i": 0})   # ok / kind / state / changes
```

```bash
fslc replay specs/cart_v1.fsl --trace events.json   # conformant / nonconformant
fslc replay specs/cart_v1.fsl --from-log production.jsonl --mapping log_mapping.fsl
fslc testgen specs/cart_v1.fsl -o test_cart_v1.py            # pytest (default); partial reachability warnings unless --strict; unwired tests fail
fslc testgen specs/cart_v1.fsl --allow-unwired -o test_cart_v1.py  # opt-in: unwired tests skip instead (not for kotlin)
fslc testgen specs/cart_v1.fsl --target vitest -o cart.test.ts  # self-contained Vitest (TypeScript) scaffold
fslc testgen specs/cart_v1.fsl --target swift -o CartConformanceTests.swift  # self-contained Swift Testing scaffold
fslc testgen specs/cart_v1.fsl --target kotlin -o CartConformanceTest.kt  # self-contained kotlin.test scaffold
fslc testgen specs/cart_v1.fsl --target dart -o cart_conformance_test.dart  # self-contained package:test scaffold
fslc testgen specs/cart_v1.fsl --target phpunit -o CartConformanceTest.php  # self-contained PHPUnit scaffold
fslc testplan specs/cart_v1.fsl --depth 4                     # closed test-plan.v1 JSON to stdout
```

`fslc testplan` emits a closed `test-plan.v1` document
(`schemas/fslc/kernel/test-plan.v1.schema.json`) built from the same checked
model as the Kernel and `conformance` JSON, so a plan cannot pair vectors from
two different snapshots. A plan is a *selection*, never a verdict: it always
carries `formal_result: "not_run"`, `assurance_effect: "none"`, and a
`do_not_assume` list recording that it is not proof of implementation
correctness, not exhaustive beyond the declared depth and finite scope, that
selection coverage is not completeness, and that it does not replace `verify`,
induction, `replay`, or refinement. `layer_selection.requirement` repeats what
the CLI help states: pass the spec at the same FSL layer granularity as the
implementation you are checking; from an upper layer reuse forbidden
(negative) scenarios only.

External compilers emit the native replay contract as a closed versioned JSON
object (`schemas/fslc/kernel/replay-trace.v1.schema.json`):

```json
{"$schema":"https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json","schema_version":"1.2.0","kernel_schema_version":"1.0.0","spec":"ShoppingCart","initial":{"stock":{"0":1},"cart":{"0":0}},"events":[{"tick":1,"action":null,"params":{},"state":{"stock":{"0":1},"cart":{"0":0}}},{"tick":2,"action":"add_to_cart","params":{"u":0,"i":0},"state":{"stock":{"0":0},"cart":{"0":1}}}]}
```

`initial` is the complete tick-0 state and every event has the exact Public
Kernel action/parameter names plus its complete post-state. Since trace schema
1.1, `action:null` with `{}` params is an explicit stutter observation whose
complete state must equal the current logical state; 1.0 remains action-only.
Inserting or deleting equal-state stutters preserves the projected action trace
and final state. Ticks are exactly `1..N`, including stutters. Optional non-empty
`timestamp` is ignored opaque producer metadata, not
formal time. Trace v1 accepts Kernel `1.0.0` and `2.0.0`. Malformed contracts
fail as input errors; typed initial/post-state divergence is nonconformant with
leaf mismatch evidence. Invariants apply to observation points and atomic
Monitor successors, not to unreported implementation intermediates. Bare arrays
and `{events:[...]}` remain the explicit
unversioned action-only adapter. Testgen and verifier trace JSON are not replay
input. See `docs/design/DESIGN-replay-trace.md`.

Trace schema 1.2 additionally checks every `leadsTo P ~> within K Q` at the
initial state and each action/stutter observation. The deadline `p + K` is
inclusive: `Q` there succeeds, while absence of `Q` fails with bounded-liveness
evidence. Safety is evaluated first. Successful output separates
`checks.safety` from `checks.bounded_liveness`; an unfinished finite obligation
is `pending`, and unbounded `leadsTo` properties are named but not claimed as
checked. Schemas 1.0/1.1 retain their earlier safety-only meaning.

The `--from-log` form reuses the exact refinement mapping grammar to translate
external JSONL records into spec actions and logical state; it does not add a
second mapping language. Each non-empty line is
`{"action":"external_name","params":{...},"state":{...}}`, where `state`
is the observed post-action state. In the mapping, `impl` labels the external
log schema, `abs` must name the replayed spec, `map` entries cover every spec
state variable, and `action external(params) -> spec_action(exprs)` (or
`stutter`) maps the event. The Monitor executes the mapped action and compares
its result with the mapped observed state on every line. The first divergence
reports zero-based `failed_at_record` / `failed_at_event`, one-based `log_line`,
and either the Monitor violation or a `state_mismatch` with leaf paths.

This first version requires full observation: a missing field or Map key is a
`log_mapping` nonconformance, not an unconstrained value. See
`DESIGN-log-replay.md` for the record schema, mapping example, and the boundary
with `db observe` / `ai replay` / `domain replay`.

Since `replay` checks only finite logs, **`leadsTo` is out of scope** (stated
explicitly in the output `note`). `Monitor` requires init to be deterministic
(forall bulk assignment is allowed). For a Map/index target (`m[K] = ...`),
"assign exactly once" is per concrete key, not per variable: separate flat
`m[K1] = ...` / `m[K2] = ...` statements for two *different* keys are fine;
different assignment statements that write the same key on a path are rejected.
A finite binder-keyed `forall k: K { m[k] = ... }` is allowed, but a later
statement writing one of its covered keys is an ownership conflict.
For struct roots and `Map<K, Struct>` entries, ownership is per
`(root, key, field)` (with no key for a struct root): different fields may be
initialized separately, including in a `forall`, but different statements
writing the same field or overlapping a whole root/entry with one of its fields
on a path are rejected. Field types remain scalar or nested `Option<scalar>`.
Ownership summarizes a `forall` statement once: repeated writes by that same
statement across iterations are allowed when their values agree; conflicting
values make init unsatisfiable rather than causing a duplicate-ownership error.
An unresolved map key (dynamic, `where`-filtered, collection, or non-constant
range binder) retains whole-root ownership and conflicts with any other
statement writing that root.
Init shapes that need the new per-field ownership granularity are admitted only
when every root init assigns on any path is definitely complete on all paths,
including all keys and fields of a Map, even for roots written only as whole
values. Roots init never assigns remain free. A partial branch, empty or not-provably-nonempty
loop, or subset of keys does not suffice; partial forms retain the previous
ownership error. Shapes accepted by the previous root-granularity rule, such
as single-field or mutually exclusive branch-split field writes, whole-entry key
subsets, or whole-root writes on just one path, retain their symbolic free-component behavior.
Refinement refuses incomplete assigned roots on either side instead of defaulting
missing components (#1521); bmc and induction retain their free-value verdicts.
Enumerating partial init components is future work (#1579).
For new admission proofs, a source, filter or key bound depending on an outer
binder is conservatively unresolved, even if a same-named global const exists.
Enclosing binding combinations are not enumerated. Independently, the frontend
rejects executable local bindings that share a const's name (#1515), before
engine-specific bound resolution; this does not add dependent-range support.
For newly admitted shapes, `check` rejects reads of any root init may assign
until the entire root is definitely assigned, including roots written only as
whole values and reads of an already-written field or entry. Early reads of
non-field-written roots retain the previous ownership error and location unless
the field-read gate refuses first. If all field-written roots are complete at
the end, that gate runs before the nonfield-read pass: a later field read can
therefore outrank an earlier nonfield read. Incomplete field-written roots retain
the saved ownership refusal instead.
The same rule covers whole-entry or whole-root reads of a field-written root;
their diagnostics identify the read as `<whole entry>` or `<whole root>`.
Thus `s.a = 1; s.b = (s.a == 1)` is rejected, as is reading `m[k].f` before
writing its sibling fields inside a `forall`; reads of already-written fields
at the current binder key after all binder-keyed fields have been written are
allowed. Writing all fields separately does not permit a whole-entry copy inside
that body under #1518's read-order rule; whole-entry copies outside the loop
after full-root initialization are allowed. The semantics error names the
root and field and locates the containing statement. Shapes accepted by the
previous root-granularity ownership rule retain their ownership-check admission.
The explicit engine and Monitor require every field of every entry to be
covered before the root is read or initialization completes. They now count
resolved map-entry-field writes toward coverage even for legacy-accepted shapes:
complete single-field-entry maps can be constructed, while retained incomplete
coverage adds the partial-component suffix to missing-assignment diagnostics.
The same concrete initialization is used by deterministic refinement on either
side, auto engine selection, conformance vectors, test generation and requirement
trace validation. Their outcomes can therefore change too: auto may replace a
bounded BMC fallback with unbounded explicit closure, and requirement-trace gates
can change check/proof/testgen results. Kernel replay or conformance vectors alone
do not establish that declared acceptance traces pass. Complete coverage can also
expose existing out-of-domain or repeated-write vacuity refusals rather than an
earlier missing-assignment error; relational repeated-write constraints are not
changed by this different concrete diagnostic stage.

## 13. The three-layer dialects (consulting / requirements / design) and traceability

The background of the design is in `DESIGN-layers.md`, and the implementation
spec in `DESIGN-dialects.md`. There is a single kernel (§1–12 of this document),
and the per-layer dialects are a front-end that expands into the AST. The layers
are connected by refinement: **business ⊒ requirements ⊒ design ⊒ implementation
(testgen/replay)**.

`fslc document generate`/`claims` (see `docs/design/DESIGN-document-requirement-claim-ir.md`)
projects only the `spec` and `requirements` dialects into its Requirement Claim
IR today. Every other dialect described in this section — `business`,
`dbsystem`, `ai_component`, and the rest — is rejected fail-closed with a coded
error (`FSL-DOC-DIALECT-UNSUPPORTED`) rather than producing a partial document.
`--view business`/`--view design` are reserved, not yet implemented, flag
values — see `docs/design/DESIGN-document-dialect-adapters.md` for the activation
contract a future cross-layer view must satisfy.

### 13.1 Declaration tags (traceability common to all layers)

The canonical relationship syntax is a typed annotation immediately before the
linked declaration. Violations, CTIs, coverage diagnoses, and scenarios then
carry `requirement: {id, text}`:

```fsl
@requirement("REQ-LEDGER-003", "the ledger matches the number of payments")
invariant PaidLedger { ... }

@requirement("REQ-EXPENSE-001", "amounts at or below the threshold are auto-approved")
action submit(c: Case, a: Amount) { ... }
```

A `requirement`, `acceptance`, `forbidden`, `policy`, `goal`, or `control`
declaration owns the ID after its keyword. `@requirement(...)` links another
declaration to such an ID; process `covers ID "text"` is canonical dialect sugar
for the same typed relationship. The older `"ID: original text"` slot before a
declaration body remains accepted migration input, but `fslc lint` reports it as
`legacy_string_metadata` and `fslc migrate --edition next` converts safe cases
to `@requirement(...)`.

The reserved `"undecided: reason"` tag marks a reviewed, intentionally deferred
decision. It is metadata and is never verified as a property. It may be attached
to `init`, `action`, `invariant`, `trans`, `reachable`, or `leadsTo`; for example:

```fsl
init "undecided: initial operating mode is pending" { mode = Manual }
action route() "undecided: routing policy is pending" { ... }
```

`fslc ledger` and `fslc html` list these declarations and the requirement IDs
whose state dependencies overlap them. `analyze --profile ai-review` keeps an
underspecification finding but marks an exact declaration match as
`acknowledged:true`. The legacy direct declaration syntax still has one string
slot, but native lowering adapts it to a typed annotation carrier that can hold
requirements, undecided markers, kinds, and namespaced custom annotations
together — the same carrier the canonical `@...` syntax below populates.
Requirement blocks therefore merge their outer requirement with an inner
legacy marker instead of overwriting it. Explicit `covers` and requirement-block
annotations keep their own exact source spans, and `undecided` is reserved
rather than accepted as an explicit requirement ID. JSON consumers that expose
multiple relations use a `requirements` array while preserving the existing
singular field as a lexical compatibility projection. See
`DESIGN-undecided.md` and `DESIGN-annotations.md`. This is implemented by the
authoritative native Rust CLI and is not backported to the frozen Python
reference implementation.

A document itself may carry typed annotations before its dialect keyword, and
(as of issue #241) a declaration inside the document may carry typed
annotations immediately before it, in either order relative to legacy
metadata that also targets the same declaration:

```fsl
@requirement("REQ-CHECKOUT-001", "this document owns the checkout contract")
@acme.review(owner.platform, 2, true)
spec Checkout {
  state { paid: Bool }

  @requirement("REQ-CHECKOUT-003", "the ledger matches payments")
  @undecided("late gateway completion policy is pending")
  @kind("safety")
  invariant PaidLedger { paid == paid }
}
```

The shared lexer skips a leading BOM, whitespace, and `//` comments before an
annotation group (top-level or nested); comments and blank lines between
stacked annotations, or between the last annotation and its target, do not
break attachment. Built-ins are `@requirement(id, text?)`, `@undecided(reason)`,
`@kind(id, text?)`, and custom multi-segment namespaces
(`@acme.review.owner(...)`) with string/integer/Boolean/symbol-path arguments;
multiple annotations may stack on one declaration in any order without
changing the checked result. At top level, keywords inside annotation
arguments do not participate in dialect dispatch; empty and unknown documents
report `FSL-DIALECT-EMPTY` / `FSL-DIALECT-UNKNOWN` with exact locations and the
deterministic supported-keyword list (see `DESIGN-dialect-dispatch.md`).

#### 13.1.1 Canonical ID policy

The grammar accepts broad IDs so existing taxonomies remain representable.
`fslc lint`, independently of parsing and verification, applies a built-in
kind-aware policy:

| Kind | Built-in template(s) |
|---|---|
| requirement | `REQ-{scope}-{number:3}`, `NFR-{scope}-{number:3}`, `INV-{scope}-{number:3}` |
| acceptance | `AC-{scope}-{number:3}` |
| forbidden | `FB-{scope}-{number:3}` |
| policy | `POL-{scope}-{number:3}` |
| goal | `GOAL-{scope}-{number:3}` |
| control | `CTRL-{scope}-{number:3}` |
| model | `MODEL-{scope}-{number:3}` |
| assumption | `ASSUME-{scope}-{number:3}` |

`scope` is uppercase ASCII alphanumeric and may contain hyphen-separated
segments; `number:3` is exactly three decimal digits. A mismatch is reported as
the non-machine-applicable `non_canonical_id` finding: ID renaming is never
performed automatically because references can cross source, tests, code,
telemetry, and external evidence.

Pass `--project fsl-project.toml` to replace selected kinds while retaining the
built-in defaults for omitted kinds:

```toml
[id_policy.patterns]
requirement = ["PAY-{number}", "NFR-{scope}-{number:3}"]
acceptance = "TEST-{number}"
```

Values are a string or non-empty string array. Templates support `{scope}`,
`{number}`, and positive-width `{number:N}`. Invalid policy configuration fails
with exit 2. Values use the manifest reader's closed subset: double-quoted
JSON-compatible strings and arrays without trailing commas or inline comments;
single-quoted TOML strings are rejected. Model and assumption templates must
start with distinct literal prefixes that overlap neither each other nor
requirement templates. Lint does not search parent directories: the manifest
is explicit, and JSON output records its
source and the fully resolved patterns. See `DESIGN-id-policy.md`.

Declaration-level annotations attach to `init`, `action` (including sync and
mapped/requirement actions), `invariant`, `trans`, `reachable`, `until`,
`unless`, `leadsTo`, process `transition` (alongside `covers`), and
`requirement`/`acceptance`/`forbidden` blocks (a `requirement` block's own
annotations fan out to every action/property it contains, the same way its
`id`/`text` already does). An annotation with no following declaration in the
same block, or one preceding a declaration kind that does not accept
annotations, reports `FSL-ANNOTATION-TARGET` at the annotation's span; malformed
arguments, namespaces, or syntax report `FSL-ANNOTATION-ARGUMENTS` /
`FSL-ANNOTATION-PATH` / `FSL-ANNOTATION-SYNTAX`. The new syntax and the legacy
string/`covers` forms both desugar into the same typed relation and simply
union when both are present on one declaration (identical `(id, text)` pairs
deduplicate; conflicting text for one requirement ID is a checked-model error).
As of issue #281, the same `@...` grammar also attaches to nested
declarations in the three specialized frontends that do not share this
parser: `domain` (aggregate `command`, `decide`, `evolve`, `invariant`,
`projection`, `effect`, saga `step`), `ai_component` (`tool`, the `tools
[a, b];` shorthand, the `authority` block and each of its rule lines,
`fallback` and each `when` item, `check`), and `dbsystem` (`migration`, and
each `rule` line inside `check compatibility { ... }`). A `command`/`decide`
pair (plus any `evolve` block the decide's emitted events reach) union onto
the one action they together generate; an `effect`'s or saga `step`'s
annotations broadcast to every action it generates (a step's own action and
its `_timeout` variant; an effect's per-outcome `_complete_*` actions, its
`_retry` action, and its success-sticky progress property). Everywhere else
in these three dialects — a migration operation line, a `dbsystem`/
`ai_component` scalar declaration, a domain declaration outside the accepted
list — a stray annotation still reports the coded `FSL-ANNOTATION-TARGET`
diagnostic. See `DESIGN-annotations.md`'s "domain/db/ai nested declaration
syntax" section for the full attachment table, the `AiAuthority` rule
restructuring, and how a `dbsystem` migration/compatibility-rule annotation
reaches the checked model without going through the lossy legacy
`quote_meta` string convention. `@...` on a nested declaration remains a
native-only surface addition — the frozen Python reference does not parse
it, so specs that use it are outside the Python-parity corpus by
construction.

#### 13.1.2 Rationale for tooling and AI consumption

A `//` comment is lexer trivia (`rust/fsl-syntax/src/lexer.rs`): it never
reaches the AST, `KernelModel`, `kernel_ast_v1()`, the JSON result envelope, the
LSP index, or the audit ledger. A fact a downstream tool or an AI agent must
not lose — that an auxiliary invariant exists to close a k-induction CTI, or
that an implementation guard is deliberately stronger than its abstract
counterpart — should therefore ride the annotation carrier instead of living
only in prose:

- Use the built-in `@kind(id, text?)` to classify and explain a declaration in
  one line, e.g. `@kind("aux_invariant", "closes the k-induction CTI for
  attempts_bounded")`. `Kind` never alters guards, actions, property kinds,
  verification, or lowering. It survives in the checked `KernelModel` for
  in-process consumers that explicitly call `annotations_for`; the current
  JSON envelope, LSP index, and audit ledger do not project generic annotations.
- For a short rationale that does not naturally fit a classification, the
  recommended custom namespace is `@doc.rationale("...")` (an ordinary
  `Custom` annotation; see 13.1). It carries the same verification-inert
  checked-model representation as any other custom namespace and needs no
  grammar change. A public JSON, LSP, or audit consumer still needs an explicit
  projection contract before it can query that rationale.
- Multi-sentence narrative — what a spec demonstrates, a walkthrough of why a
  design works, a pedagogical bug marker in an intentionally-broken example —
  stays an ordinary `//` comment. Annotation argument strings have no escape
  syntax and stop at the first `"` or newline (`lex_string`), so they cannot
  hold prose; forcing narrative into an annotation argument makes a spec
  harder to read, not easier.

### 13.2 Requirements layer: `requirements` (the fsl-req dialect)

```fsl
requirements ExpenseRequirements {
  implements ExpenseToBe from "1_business.fsl" { }

  number Amount
  const AUTO_LIMIT = 1

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit       Draft     -> Submitted by Employee with a: Amount when a > 0 set amount = a covers REQ-EXPENSE-001 "The applicant submits an expense claim by entering an amount"
    transition auto_approve Submitted -> Approved  by System  when amount <= AUTO_LIMIT covers REQ-EXPENSE-002 "Claims at or below AUTO_LIMIT are auto-approved by the system"
    transition mgr_approve  Submitted -> Approved  by Manager when amount >  AUTO_LIMIT covers REQ-EXPENSE-003 "Claims above AUTO_LIMIT are approved by a manager"
    transition reject       Submitted -> Rejected  by Manager when amount >  AUTO_LIMIT covers REQ-EXPENSE-003 "Claims above AUTO_LIMIT may be rejected by a manager"
    transition pay          Approved  -> Paid      by Finance covers REQ-EXPENSE-004 "Only approved claims are paid"
  }

  kpi paid_claims = count Claim in Paid

  acceptance AC-EXPENSE-001 "Approval flow: a low-amount claim is auto-approved and paid" {
    submit(0, 1) auto_approve(0) pay(0)
    expect Claim 0 in Paid
  }
  acceptance AC-EXPENSE-002 "Rejection flow: a high-amount claim ends in manager rejection" {
    submit(1, 2) reject(1)
    expect Claim 1 in Rejected
  }
}
verify {
  instances Claim = 3
  values Amount = 0..3
}
```

**Breaking removal (#1552):** for requirements `rule ... only when` conditions,
`check` now rejects previously accepted unsatisfiable guards that reuse an entity
field's name as an `is` pattern inside a quantifier body or aggregate value. The
guard copy now reads that name as the field, just as the event's lowering does,
not as the matched payload. For example,
`amount == 0 and (exists x: Amount where x == 1 { some(x) is some(amount) and amount == 1 })`
was accepted even though its event could never occur: the field must be both 0
and 1. It now fails with exit 2, `kind: "semantics"`, and
`rule_conflict_unsatisfiable_guard`. To test the payload instead, use a non-field
pattern name: replace `some(x) is some(amount) and amount == 1` with
`some(x) is some(v) and v == 1`. Keeping the outer condition and quantifier, this
means the field is 0 and there is an `x` equal to 1 whose matched payload `v`
equals 1; the rewritten example passes `check`. The pattern-name rule itself
remains tracked in #1561; this correction only aligns the conflict check with
the lowered event.

- The process+data profile is the primary requirements form for a single-entity
  lifecycle. `process E with f: T { ... }` creates the entity stage map and
  carried fields; transitions can add an input (`with a: T`), guard (`when`),
  field update (`set f = expr`), and traceability (`covers REQ-n "text"`). A
  carried field's type `T` is a `number`, `Bool`, or an enum declared in the
  same requirements spec:
  - `number` fields default to the domain's `lo` bound; `f: T = <expr>` is an
    optional explicit initializer (a compile-time constant expression). Omission
    is retained in the current edition with the stable
    `implicit_initial_value` warning and an insertion edit for the selected lower
    bound.
  - `Bool` and enum fields have no invented default — `f: Bool = true/false`
    and `f: T = Member` are **required**; omitting the initializer is a
    check-time error (no silently-chosen `false` or first enum member).
- `number Amount` declares a value kind; the finite verifier range lives in
  `verify { values Amount = lo..hi }`. Entity sizes live in
  `verify { instances Entity = N }`.
- `kpi NAME = count ENTITY in STAGE` is a declarative projection in both
  business and requirements. It does not create a ghost counter or an automatic
  `_kpi_*` invariant.
- With `implements`, `fslc check` and `fslc verify` both run the refinement to the upper layer
  as well, and the result carries `implements: {abs, result}`, whose values are `refines`,
  `refinement_failed`, and `impl_violated`. **A failing seam is not confined to this field.**
  `refines` leaves the command's own top-level `result` and exit code untouched; either failure
  value becomes the top-level `result` verbatim — `refinement_failed` stays `refinement_failed`
  and `impl_violated` stays `impl_violated`, so the two remain distinguishable at both levels —
  and the process exits 1, the same class standalone `fslc refine` already uses for
  `refinement_failed`. `implements.violation` keeps the seam-specific evidence, except under
  `--vacuity error`, which replaces the whole envelope with its own `error` result and does not
  carry `implements` over.
  `fslc chain` applies the same gate to the layer and exits 1
  (`docs/design/DESIGN-design-family.md` states the same rule for orchestrators); a caller no longer
  needs a second gate of its own for the inline seam. An empty body
  (`implements X from "..." { }`) auto-generates identity refinement when process/action/stage
  names match. Inline refinement is **not** evaluated when `verify` is scoped with
  `--property`, `--exclude-property`, or `--from-state`; the envelope then reports
  `implements: {"result": "not_evaluated", "reason": ...}` instead of omitting the key
  (§7, "Checks a selected run does not evaluate"). **A selected run therefore cannot gate
  the seam**: `result` and the exit code speak only for the selected properties, so a broken
  seam still passes such a run, but the envelope says the seam was not checked. Gate on an
  unscoped `check`/`verify`, or on `fslc chain`. The decision for these three options is
  suppress-with-reason (`docs/design/DESIGN-refinement.md`,
  [#1008](https://github.com/ymm-oss/fsl/issues/1008)). Inside the `implements { }` block you write
  state `map` entries, `maps auto`, and — since #73 —
  `action <impl_act>(<params>) -> <abs_act>(<args>) | stutter`, the same
  correspondence syntax as a separate refinement file's `refinement_action`
  (`docs/design/DESIGN-refinement.md` §1), including an arity change between the impl
  and abs action. `preserve progress` is not accepted there: the inline seam checks
  safety refinement only, so a `preserve progress` item inside `implements { }` is a
  located `kind: "parse"` error (exit 2) in every command that loads the document
  ([#1275](https://github.com/ymm-oss/fsl/issues/1275)). To check an upper `leadsTo`
  through the mapping, put the mapping and its `preserve progress` in a standalone
  refinement file and run `fslc refine`. Action↔action correspondence can also still be written as the
  `maps <abs_act>(...)` clause on the requirement-level action; `maps auto`
  covers same-name kernel-wrapper state/actions, and explicit maps override it.
  An impl action with both a `maps` clause and a matching inline `action ...`
  item is a duplicate-correspondence, `kind: "type"` check-time error naming
  both origins and locations (same as a mapping file that lists the same action
  twice). Whenever the seam is evaluated, auto-mapped process transitions and
  events compare the declared source actors on both sides, regardless of edition (4/5) or `covers` on
  either side, including prose beginning `by `. A registered actor takes
  precedence over metadata. Requirement actions, standalone or inside a block,
  register only their own metadata's actor (`by` followed by a space and a
  nonempty trimmed actor), including every generated branch. No annotation,
  non-`by` text or an empty actor registers absence: neither enclosing prose nor
  typed `@requirement` text can supply an actor. Only unregistered raw spec/compose
  actions retain the existing metadata rule. When both actors are available,
  a mismatch is `error` / `kind: "type"`, exit 2, with
  `maps auto actor mismatch for action 'approve': impl actor 'Manager' != abstract actor 'System'`
  at the implicit `implements` or explicit `maps auto` correspondence. The same rule
  applies in unscoped `verify`, standalone `refine` and Worker; explicit action
  correspondences and `stutter` are not actor-checked. Selected `verify` still
  does not evaluate the seam. Without a supplied mapping, `diff` reports an
  affected direction as `unknown` / `automatic_mapping_failed` inside its
  `semantic_diff`/exit-0 envelope, not as a top-level type error.
  **v5.0.0 breaking source change (#1441):** mismatches previously hidden by
  `covers`, cross-edition or mixed compose lookup are rejected; Kernel/JSON
  schemas do not change. The full four-cell corpus sweep at base
  `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9` and head
  `9ca8630c7f45f1def54c056bf53a852853c7a60c` ran both binaries' `fslc check`
  on every `.fsl` in `specs/`, `examples/`, `rust/fslc/tests/fixtures/`, in each
  SHA's own tree (579 base / 591 head forms): **0 `ok`→`error` and 0
  `error`→`ok` corpus transitions**. Separately, the acceptance-expansion class
  is registered transition metadata whose requirement text begins with `by `
  (covers text and next-edition rule/decide text). With matching `System` actors,
  impl covers (B3), business covers (B4) and an abstract event's rule text
  `by Manager only` without covers (F1/p5) change `error/type/2`→`ok/0`;
  bounded verify returns `verified/0` with `implements refines`, and F1's
  standalone refine changes `error/type/2`→`refines/0`.
  See [DESIGN-refinement](../design/DESIGN-refinement.md),
  “#1441: source-actor auto mapping and breaking-source migration”,
  for commands, measured evidence, gate policy and manual correction; no general
  automatic semantics-preserving migration exists. LSP delivery is not promised.
  **v5.0.0 breaking source change (#1456):** requirement actions whose own actor
  mismatch was hidden by block text are rejected, including standalone auto maps
  of generated branches. Explicit action entries, inline branch maps and stutter
  remain exempt. Conversely, matching or absent own actors falsely rejected by
  block prose are intentionally accepted; typed requirement text is not an actor
  declaration. Public metadata priority, Kernel v1/v2, RCIR, spec digests and report
  attribution are preserved. Both binaries' `fslc check` swept all three roots in
  their own archived trees at base `e8cf78f0162530e264128f588981d49790f2767f`
  and head `b8daaa8245f374f74448fd983755689f832484cc`, twice (591 base / 592 head
  forms per binary): **0 corpus `ok`→`error` and 0 `error`→`ok` transitions**.
  Acceptance removals/expansions are separate from this empty corpus set.
  See [DESIGN-refinement](../design/DESIGN-refinement.md), section
  “#1456: requirement-action own actors and breaking-source migration”,
  for manual policy correction and checks. No automatic migration, compatibility
  bypass or grace period is provided; if problems occur, return to a 4.x binary.
- In a requirements process-form transition (edition 4), `by X` is a free label,
  not a reference to a declared actor: this form has no `actor` declaration,
  and `check` does not ask whether `X` is declared anywhere. Here, “source actor”
  means the label written on the transition, not a declared actor symbol.
  (A `business` process is the opposite: a transition whose actor no `actor`
  line declares is a check-time error.) The label is compared with a business
  actor only when a refinement maps the transition automatically to the
  business action of the same name — through `maps auto`, or through the inline
  `implements` seam's implicit auto-mapping of every transition that no explicit
  inline `action ... -> ...` item names — and that refinement is evaluated.
  The mismatch and `diff` reporting rules are described above. For example,
  the inline seam is evaluated by an unscoped `check` or `verify` and by
  `fslc scenarios`, but not by a `verify` scoped with `--property` and the like
  (above); a refinement file's `maps auto` is evaluated by `fslc refine` or a
  `fslc chain` refine layer; and `fslc diff` without a mapping builds a
  `maps auto` refinement between the two versions itself, so a changed `by`
  label there surfaces as an automatic-mapping failure. The label is checked
  against nothing for a transition mapped by an explicit `action` item, for one
  the inline seam maps to `stutter` because the business layer has no action of
  that name, and whenever no such refinement is evaluated.
- `acceptance` is replay-verified at check time by the concrete Monitor (a
  failure is `kind: "acceptance"`). It supports `expect <Entity> <id> in
  <Stage>` alongside `expect <expr>` and flows directly into scenarios / testgen
  (= the acceptance criteria become conformance tests for the implementation).
  Action arguments in `acceptance`/`forbidden` steps must match the parameter's
  kind: enum parameters take member names, not numeric ordinals (`paint(1)` is
  not `paint(Blue)`). Const names are also accepted where their values match
  the parameter's kind; an undefined name is a check-time error. An acceptance
  step with a wrong-kind argument fails with `kind: "acceptance"`, a
  `wrong_kind_argument` object (`parameter`, `value`, `type`), a top-level
  `message` explaining which argument does not belong to the parameter's type
  and that no guard was evaluated, a hint, and a `loc` at that step.

  Before replay, `expect <expr>` (and FSL 5 scenario `then <expr>`) is checked
  recursively with expected type `Bool`, just like an invariant, including all
  operands and conditional branches. A wrong-kind comparison such as `c != 1`
  or `c == 1` for enum state `c: Color` is a check-time `kind: "semantics"`
  error (exit 2), with the existing assignability message and a location at the
  authored `expect`/`then` keyword. `c == Blue` is well-typed when `Blue` belongs
  to `Color`. The expression remains a post-state predicate: it need not hold
  initially or between steps. A well-typed false predicate still fails replay
  with `kind: "acceptance"`; the readable stage form is unchanged.
  Compatibility measurement (#1439): against base `5a539bbe`, all 579 forms
  per tree under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` were
  checked in both gate modes, with no ok-to-error transition. See the
  [design note](../design/DESIGN-requirements-stage.md#measured-compatibility)
  for the command and measured production identity.
  Every acceptance and forbidden step argument is also checked recursively
  against the lowered model before replay, including setup/final steps,
  FSL 5 calls, phrase `with` and particle inputs, hybrid inputs and relation-role
  values. All branches are checked even when unselected. This validates the
  expression internally, without forcing its outer type to the parameter type.
  An internal mismatch is `kind: "semantics"`, exit 2, with the existing
  assignability message at the first authored token of the step: the action
  name for calls, the actor for actor-first phrases, or the first token of a
  reordered particle phrase (for example `claim1`, not the later actor).
  Internally consistent `paint(1)` and `paint(if true then 1 else 2)` keep the
  existing forbidden `wrong_kind_argument` diagnostic; values-scope-only skips
  are unchanged.

  Extension measurement against base `5a539bbe`: 579 corpus forms plus all 21
  inline fixtures of the lint-rewritten trace-step test per tree, both modes
  twice (4,800 checks). Corpus transitions are empty; inline forms T01–T16 below
  are **breaking removals** (`ok` / 0 → `error` / `semantics` / 2), with
  21 mode/form transitions and no other full-output/exit or repeat changes.
  T01–T03, T13 and T16 transition in both modes; the others only in FSL 5.
  Each row identifies the corresponding measured removal in the
  [extension design note](../design/DESIGN-requirements-stage.md#extension-compatibility-measurement-and-breaking-removals).

  | Removal | Ill-typed trace form now rejected |
  |---|---|
  | T01 | acceptance Color argument with condition `c != 1` |
  | T02 | forbidden setup Color argument with condition `c != 1` |
  | T03 | forbidden final S1: `paint(if c != 1 then Red else Blue)` |
  | T04 | FSL 5 scenario call with condition `claim_stage[0] != 1` |
  | T05 | FSL 5 scenario with input, same malformed stage condition |
  | T06 | FSL 5 scenario particle input, same condition |
  | T07 | FSL 5 forbidden call setup, same condition |
  | T08 | FSL 5 forbidden call final, same condition |
  | T09 | FSL 5 forbidden with setup input, same condition |
  | T10 | FSL 5 forbidden with final input, same condition |
  | T11 | FSL 5 forbidden particle setup input, same condition |
  | T12 | FSL 5 forbidden particle final input, same condition |
  | T13 | acceptance numeric argument: `setn(if c != 1 then 2 else 0)` |
  | T14 | FSL 5 marked/with hybrid input, malformed ClaimStage comparison |
  | T15 | FSL 5 relation-role value with condition `ticket_stage[0] != 1` |
  | T16 | acceptance unselected branch: `paint(if true then Blue else (if c != 1 then Red else Blue))` |
  | T17 | native/shared diagnostic test's `expect c != 1` row |
  | T18 | rewritten Worker diagnostic test's identical `expect c != 1` row |
  | T19 | step: `paint(if o is some(z) then z else Red)` with `o = none` |
  | T20 | expectation: `expect (if o is some(z) then z else Red) == Red` with `o = none` |
  | T21 | reordered particle input: `claim1 を Employee が amount (if claim_stage[0] != 1 then 1 else 2) で submits` |

  Round 1 supplements the original measurement with all sources of both
  rewritten diagnostic tests and the tests using their changed shared oracle,
  plus the scope/reordered-particle controls: 20 forms per tree, both modes
  twice, 160 checks against the same base and unchanged production code.
  T17–T20 are additional breaking removals in both modes; T21 only in FSL 5
  (five fixture occurrences / nine mode-form transitions, no other differences
  or exclusions). T17/T18 are identical FSL in two rewritten tests. T19/T20
  report `public Kernel cannot type identifier 'z'`: §5's `is some(z)` binding
  is confined to the true logical continuation (`=>` / `and`), not a
  conditional's `then` arm, even if that arm is unselected. Invariants already
  reject the same scope escape. Move the use to a supported continuation or
  avoid the escaped name. T21's location is the first token `claim1` (8:5),
  not `Employee`. See the design note for exact identities and fixture inventory.

  Unary operand measurement (#1439): base `5a539bbe` and the final implementation
  were checked in their own trees, both modes twice: 579 corpus forms plus 51
  inline fixture occurrences per tree, 5,040 checks. The corpus ok-to-error set
  is empty. T01–T21 remain the earlier removals; these seven additional inline
  forms are **breaking removals**, each `ok` / 0 → `error` / `semantics` / 2 in
  both modes (14 new mode/form transitions; 44 total). Full stdout, stderr and
  exits have no other differences, repeat differences or exclusions. See
  [unary operand typing](../design/DESIGN-requirements-stage.md#unary-operand-typing-1439)
  for the command and exact base/head identities.

  | Removal | Ill-typed unary form now rejected |
  |---|---|
  | U01 | forbidden step `paint(if true then Red else (if not 1 then Red else Blue))` |
  | U02 | forbidden step `paint(if true then Red else (if -Red > 0 then Red else Blue))` |
  | U03 | forbidden step `paint(if c == Red or not 1 then Red else Blue)` |
  | U04 | acceptance `expect c == Blue or not 1` after `paint(Blue)` |
  | U05 | invariant `c == Red or c == Blue or not 1` |
  | U06 | invariant `c == Red or c == Blue or -Red > 0` |
  | U07 | acceptance `expect not (false and not 1)` after `paint(Blue)` |

  Repair `not` to use a Bool operand and unary minus to use a numeric operand;
  skipping evaluation does not make an ill-typed operand legal.

  Current rebase measurement (#1439): BASE
  `23eb5e41326ef4cf81a100ad0e011e81ed9fd507`, HEAD
  `1e27d896b69b395ad8e48c570b4ddbca50e442a0`, each built in its own tree.
  `BINARY check FORM` covered all 594 corpus forms and 65 inline occurrences per
  tree in both modes twice (5,272 checks); the corpus ok-to-error set is empty.
  The earlier counts above are historical. All five fixed test files are now
  covered: in addition to the 44 recorded mode/form removals, acceptance `expect`
  and FSL 5 `then` reject `c != 1`, `true and c != 1`, `not (c == 1)`,
  `if c != 1 then true else false` and `forall color: Color { color != 1 }`.
  These are **breaking removals** for enum `c: Color`: acceptance in both modes,
  scenario only in FSL 5, adding 15 transitions (59 total). Use a same-enum member
  instead of the integer. The corresponding `c == 1` fixtures remain errors but
  change their diagnostic from `acceptance` to `semantics` at the expectation
  keyword (three mode/form changes, exit 2 unchanged). All 62 full-output differences are thus
  accounted for; there are no others, repeat differences or exclusions. See the
  [current design measurement](../design/DESIGN-requirements-stage.md#rebase-compatibility-measurement-on-23eb5e41)
  for inventory commands and calibration.

  Repair these comparisons using members of the same enum, including in
  unselected branches; a guard refusal cannot legitimize an ill-typed argument.
- Use the kernel-wrapper form only for hard cases: multi-entity requirements,
  conservation rules, SLA/time, history not expressible as a carried field, or
  behavior that needs explicit kernel state. That fallback still supports
  `struct` / `state` / `init`, `fair action`, `branches`, and explicit `maps`.
  `branches` automatically splits an action by each when condition (displayed as
  `submit[a <= AUTO_LIMIT]` in mutation targets; the split actions are
  `submit.b1`, `submit.b2`, … in every other output), and the `maps` clause
  provides the action correspondence to the upper layer.
- `terminal { <expr> }` is allowed at the top level of a `requirements` spec
  and lowers to the kernel (§6) — there is exactly one `terminal` block per
  spec, same as the kernel. `stage(c)` is available in all requirements
  expression contexts when `c` is a typed entity parameter or binder. The
  entity type selects its process and stage enum, and the checked expression
  lowers to the generated stage-map index. No sink stage becomes terminal
  automatically in requirements.
- A process may use a qualified path when one entity participates in several
  processes: `process claims.Claim { ... }`. Unqualified `stage(c)` is an error
  when several processes correspond to `Claim`; use
  `claims.Claim.stage(c)` (arbitrary-depth `SymbolPath`) to select one. A
  missing process, non-entity argument, unknown stage member, or unresolved
  qualifier is reported as `kind: "semantics"`, with a source position in the
  message and no diagnostic code.

**Breaking removal (#1569):** in legacy requirements with a process field
`amount`, `check` now rejects a transition guard such as
`exists c: Amount { stage(c) == Approved and amount == c }`. The non-entity
binder shadows the transition's implicit entity parameter `c`; a field named
`c` is not required. Previously, capture-avoiding substitution renamed the binder
without renaming the stage operand: `check` accepted the guard and `verify`
reported `verified`, but `stage(c)` silently read the transition's own instance.
Now `check`, `kernel`, and `verify` fail with exit 2, `kind: "semantics"`, and
`stage(c) refers to type 'Amount', which has no process`. The same invalid guard
with the binder and its uses consistently renamed to `d` already failed this way.
`stage(...)` requires an entity parameter or binder. To read the transition's own
stage, use `exists n: Amount { stage(c) == Approved and amount == n }`, keeping
`c` for the entity and a different name for the non-entity binder; this rewritten
guard passes `check`. No checked-in spec or example has the removed shape; the
597-form base `1844fa74` / head `7b9b34b3` sweep found no corpus acceptance
transitions.

The implicit entity parameter `c` is a source-visible local for #1515's const
rule: do not declare a same-named const in a requirements document using that
process interface. Rename the const, not the instance reference `stage(c)`.
Next-edition entity instance names use the entity's lowercased first letter;
relation events similarly take an implicit instance of their target actor.
Those visible names also reject matching consts, at the generated init's source
process/entity declaration or the relation event declaration. Hidden KPI
projection bindings are not source names and instead freshen on collision.

### 13.3 Consulting layer: `business` (the fsl-biz dialect)

Write business processes, policies, and KPIs with zero implementation
vocabulary (syntax details in `DESIGN-dialects.md` §3). A process expands into
enum+Map+transition actions, a policy into invariant / leadsTo, and a kpi into a
declarative count projection recorded as metadata. A contradiction in the regulations =
an invariant violation, a dead process step = a coverage diagnosis, an
unreachable business goal = reachable_failed, and a case left unattended = a
leadsTo counterexample — all can be detected mechanically.

A business process is a pure stage graph, so a `transition` accepts only its
name, source and target stages, actor and `covers` annotations. Writing `with`,
`when` or `set` on one is a located error: those clauses carry data, the
business dialect has nowhere to lower them, and silently dropping them would
verify a model the author never wrote. Declare the process in a `requirements`
spec (§13.4) to give them meaning.

For PM/consulting-facing files, use the readable stage syntax for common
response policies and goals:

```fsl
business ReturnHandling {
  actor Customer, Manager
  entity Return
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Manager
  }

  kpi refunded = count Return in Refunded

  control CTRL-DECISION
    "Every return must preserve an adjudication control"
    owner Manager
    severity high
    applies_to Return

  policy PAY-2 "every request is eventually decided"
    satisfies CTRL-DECISION
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be completed"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

The explicit forms remain available when the rule is not just stage progression:
`policy ... responds { forall c: Return { stage(c) == Requested ~> ... } }` and
`goal ... { exists c: Return { stage(c) == Refunded } }`.

For a **no-bypass** control — a target stage that must never be reached
without first passing through a required waypoint — use the precedence form
(#75; design rationale in `DESIGN-precedence-policy.md`):

```fsl
policy CTRL-APPROVAL "承認を経ずに完了しない"
  every Return reaching Refunded must have passed through Approved
```

This synthesizes an invisible `Map<Return, Bool>` history flag (named
`return_stage_via_Approved`, so it is legible in traces), sets it `true` on
any transition landing on `Approved`, and compiles the policy to
`forall c: Return { stage(c) == Refunded => return_stage_via_Approved[c] }`.
A `Requested -> Refunded` transition that skips `Approved` is then a genuine
invariant violation, with the trace showing exactly which bypass transition
fired. Both sides accept a disjunction —
`every Return reaching Refunded or Closed must have passed through Approved
or Rejected` — and two policies over the same `(process, waypoint-set)` share
one synthesized history flag.

Alongside the history flag, a second invariant, `<PolicyId>_stability`, is
auto-synthesized from the process's stage graph (#85; design rationale in
`DESIGN-precedence-policy.md`) so that a **compliant** precedence policy
proves under `--engine induction` out of the box, with no manual invariant
needed — no ghost counterexample-to-induction from the history flag being
"true but not yet provably so" at an arbitrary induction step.

```json
{"result": "proved", "k_used": {"CTRL-APPROVAL": 1, "CTRL-APPROVAL_stability": 1}}
```

Business has no `terminal` syntax of its own. Instead, each process's **sink
stages** (stages with no outgoing `transition`) are collected automatically:
if every process has at least one sink, a kernel `terminal { }` is generated
as the conjunction, over processes, of `forall c: <Entity> { stage(c) in
{Sink1, Sink2, ...} }` — so a deadlock is "intended" only once every entity of
every process is simultaneously parked at one of its own sinks. `ReturnHandling`
above therefore verifies clean at `Rejected`/`Refunded` without
`--deadlock ignore`. If any process is cyclic (every stage has an outgoing
transition, so it has no sink), no terminal is generated at all and deadlock
checking is unaffected — cyclic processes never deadlock in the first place,
since some transition is always enabled.

Governance/control metadata can be kept inside `business` or lifted into a
standalone catalog. `control ID "text" owner NAME severity NAME applies_to Entity`
does not generate a property by itself; it is a catalog entry. A `policy` or
`goal` can declare `satisfies CTRL-ID`, and violations then carry both the
policy/goal requirement and the satisfied controls in JSON:

```fsl
policy PAY-2 "every request is eventually decided"
  satisfies CTRL-DECISION
  every Return in Requested must eventually be Approved or Rejected or Refunded
```

The catalog is checked. A `satisfies` naming a control the document does not
declare is a located error, reported at the policy or goal that wrote it, and a
`control` that no policy or goal satisfies raises an `unused_control` warning in
JSON `warnings`, located at the declaration. Both exist because the declaration
and the reference used to be parsed and then dropped, so `check` answered `ok`
for a control that was never written.

For controls reused across business specs, use a `governance` catalog:

```fsl
governance EnterpriseReturnControls {
  authority Operations owns CTRL-DECISION
  control CTRL-DECISION "Every return must preserve an adjudication control"

  delegates ReturnHandling from "return_policy.fsl" {
    require CTRL-DECISION
    // optional if the business policy already says `satisfies CTRL-DECISION`
    CTRL-DECISION is satisfied_by policy PAY-2
  }

  preservation ReturnReform {
    before AsIsReturn from "asis_return.fsl"
    after  ToBeReturn from "tobe_return.fsl"
    preserve CTRL-DECISION
    checked_by refinement "tobe_refines_asis.fsl"
  }
}
```

`fslc check governance.fsl` validates all referenced controls, business files,
policies/goals, and preservation files. Preservation blocks also run the declared
refinement at depth 8 and report results under `governance.preservations`:
`refines`, `refinement_failed`, `impl_violated` (the `after` spec breaks its own
semantics, §10), or `unknown_budget` (the correspondence check reached its
state budget, §10). The browser Worker reports the same values.

A preservation that does not `refines` is the command's verdict, fail-closed,
for `fslc check` and `fslc verify` alike (`verify` runs the preservations at
depth 8 too, whatever its `--depth`, and attaches the same `governance`
section). The first non-`refines` preservation in declaration order sets the
top-level `result`; every preservation keeps its own value under
`governance.preservations`:

| preservation `result` | top-level `result` | exit |
|---|---|---|
| `refines` (every preservation) | unchanged (`ok` / `verified` / `proved`) | unchanged (0) |
| `refinement_failed` | `refinement_failed` | 1 |
| `impl_violated` | `impl_violated` (the same word an inline `implements` seam uses for the same condition) | 1 |
| `unknown_budget` | `unknown_budget` | 1 |
| anything else | `error`, `kind: "internal"` | 3 |

A run that already fails for its own reasons (`error`, or a failure-class
verdict of the document's own kernel) keeps its verdict; the section is
evidence only. Any governance contract error that `check` reports (a
delegated business file, a control, a `satisfied_by` mapping, or a
preservation's `before`/`after`/mapping files that are missing or ill-typed)
is the same located `error` (exit 2) under `verify` too; a preservation
refinement that fails internally is `error`/`internal` (exit 3) under both.
Consumers of the `check`/`verify` envelope read the folded verdict: a
`fslc chain` spec layer fails, `fslc ledger`, `fslc html`, and `fslc mutate`
exit 1 from the same baseline (the ledger body also states that verdict and
lists every preservation with its result, failing ones marked), `fslc
scenarios` folds the preservations into its own envelope as `verify` does,
`testgen` returns the verify failure envelope and exit without a harness when
a preservation fails (including on the kernel path), and the browser Worker
folds `check` and `verify` the same way. This changed exit codes: before 4.11 a failed
preservation left `check` at `ok` and `verify` at `verified`, both exit 0, and
`verify` reported no `governance` section at all (and answered `verified` even
for a contract `check` rejected). A CI that gated on the exit
code passed a broken preservation; it now fails it. To keep a preservation
advisory, run it as a separate `fslc refine` step whose exit the CI ignores.

### 13.4 How to write non-functional requirements (NFRs)

The majority of NFRs can be written with the same machinery as functional
requirements (details and demonstration in `DESIGN-nfr.md`):

| NFR | How to write it |
|---|---|
| Permission (only an admin does X) | `requires role[u] == Admin` + an invariant over the ghost `done_by_admin` |
| Audit completeness | A cross-cutting invariant (e.g. `audit.balance == cleared + pending + withdrawn`) |
| Capacity / upper bound | A bounded type / Seq capacity / a `count(...) <= N` invariant |
| Reliability behavior | A fault-injection action (`crash`) + a mode state + `fair recover` + a recovery leadsTo |
| **SLA / timeout** | A `time` block + `deadline` (below) |
| Probability, percentiles, real time in ms | **Out of scope** (write it in prose) |

An SLA is checked as safety in discrete time (inside `requirements`):

```fsl
time {
  urgent start, finish                    // while enabled, time (tick) does not advance
  age waitAge[r: Req] while pending[r]    // +1 per tick, 0 if the condition is false
}
requirement NFR-1 "complete within 4 ticks of acceptance" {
  deadline waitAge <= 4
}
```

- The tick is auto-generated, and the urgency discipline ("the system does not
  procrastinate work when idle") is its guard. If you do not specify urgent,
  most deadlines become violated by a starvation trace — which is a correct
  indication that "there is no scheduling assumption."
- Placement: at most one `time` directly under requirements, and `deadline`
  inside a requirement (the requirement ID is tied to the violation). An age is
  +1 per tick (reset to 0 if while is false) and can be read from guards as an
  ordinary state variable.
- `tick` is generated — do not declare your own `action tick` (it is a check
  error). It advances age counters only and auto-maps to `stutter` under
  refinement; reference it as `tick()` (e.g. in an `acceptance` scenario).
  Tick-side work (service time, etc.) needs the kernel-wrapper form.
- **Across layers, a `deadline` refines only over a shared clock.** A `deadline`
  is a safety property of the clock that declares it, so a design refines a
  *timed* requirements spec only when its `tick` mirrors the generated one; a
  design with a finer clock (a `tick` that also consumes service time) has no
  abstract image for those steps and fails `fslc refine` with
  `abs_requires_failed` — the same non-propagation as liveness. Verify a timed
  property at the clock-owning layer, or keep the upper contract time-less and put
  the clock in the design kernel (`tick → stutter`). See `docs/design/DESIGN-nfr.md` §6
  and `examples/nfr/sla_worker_design.fsl`.
- **⚠ The vacuous-SLA trap**: making an action that can always be enabled urgent
  freezes time, and any K satisfies the deadline vacuously (even `<= 0` is
  green). The correct form is **to make urgent only the guarded action that
  becomes enabled at deadline arrival (the respond_due form with
  `requires age >= K`)**. To confirm non-vacuity, lower it to `K-1` and check
  that it becomes violated. `fslc verify --vacuity` emits
  `kind:"urgency_freeze"` for the provable form of this trap (the urgent
  condition is initial and inductive). It emits `kind:"vacuous_deadline"` when
  the stronger per-deadline check proves that every relevant age value remains
  zero across all transitions even though `tick` may run while the age
  condition is false. Absence of either warning is not a proof of non-vacuity.
- The BMC check works immediately. An inductive proof often needs a time-budget
  auxiliary invariant (the `age + remaining work <= K` form) (derived from the
  CTI; real examples in `examples/nfr/`).

### 13.5 Database compatibility layer: `dbsystem` (fsl-db)

`dbsystem` models migration compatibility across databases, application
artifacts, API/offline payloads, and environments. It is a dialect expansion, not
a DB engine model: optimizer behavior, lock timing, wall-clock TTLs,
probability, and full production-data completeness are outside the formal model.

Core shape:

```fsl
dbsystem <Name> {
  database <db> {
    schema <initial_version>
    table <table> {
      column <column>: <db_type> present backfilled not_null;
      column <future_column>: <db_type> absent;
    }
  }

  migration <name> from <v0> to <v1> [rollbackable] {
    add <table>.<column> nullable;
    backfill <table>.<column>;
    set_not_null <table>.<column>;
    rename <table>.<old> to <table>.<new>;
    split <table>.<source> into <table>.<a>, <table>.<b> lossless|lossy|irreversible;
    merge <table>.<a>, <table>.<b> into <table>.<target> lossless|lossy|irreversible;
    drop <table>.<column> destructive|irreversible;
  }

  artifact <version> {
    reads <table>.<column>, ...;
    writes <table>.<column>, ...;
    requires <capability_namespace>.<capability>, ...;
    provides <capability_namespace>.<capability>, ...;
    calls api.<operation>, ...;
    accepts api.<operation>, ...;
    expects response.<field>, ...;
    responds response.<field>, ...;
    emits_offline api.<operation> ttl <finite_ticks>;
  }

  environment <env> {
    schema <lo>..<hi>;
    flag <flag_name> { <variant>, ... } default <variant>;
    active <version> when schema <lo>..<hi> when flag <flag_name>=<variant>;
    supported <version> when schema <lo>..<hi>;
    may_exist <version> when schema <lo>..<hi>;
  }

  check compatibility {
    rule all_active_reads_exist;
    rule all_active_writes_exist;
    rule removed_only_after_unused;
    rule not_null_after_backfill;
    rule destructive_operations_annotated;
    rule preservation_transforms_annotated;
    rule api_calls_accepted;
    rule api_responses_expected;
    rule offline_payloads_accepted;
    rule artifact_capabilities_provided;
    rule data_preserved;
    rule rollback_equivalent;
  }
}
```

If `check compatibility` is omitted, default rules cover read/write lifecycle,
destructive annotations, preservation-transform annotations, and API/offline
compatibility. `data_preserved` and `rollback_equivalent` are opt-in bounded
checks and report `DB-ASSUME-BOUNDED-ROW-MODEL`. A `rule` name outside the
closed vocabulary above, or an `environment schema lo..hi` that the declared,
strictly sequential migration plan never reaches, fails validation with exit 2
instead of silently checking nothing.

Feature flags are finite environment dimensions. `fslc db check` enumerates the
declared variants with schema snapshots and reports
`DB-ASSUME-FINITE-FLAG-STATE`; this is a rollout/kill-switch compatibility
check, not a percentage or probability proof. Omit `flag` / `when flag` to keep
the existing artifact/window-only model.

Current formal violation kinds include:

- `column_removed_while_still_read`
- `column_removed_while_still_written`
- `not_null_before_backfill`
- `destructive_migration_unannotated`
- `preservation_transform_unannotated`
- `data_preservation_loss`
- `rollback_not_equivalent`
- `api_call_not_accepted`
- `api_response_field_missing`
- `offline_payload_not_accepted`
- `required_capability_missing`

Use `fslc db check` when you want fsl-db vocabulary:

```bash
fslc db check examples/db/safe_add_nullable_column.fsl
fslc db check examples/db/safe_dual_write_backfill_switch_read_drop_old.fsl --engine induction
fslc db observe examples/db/runtime_observation_target.fsl --trace examples/db/runtime_observation_mismatch.json
fslc db import examples/db/minimal_import.sql --name ImportedFromSql -o /tmp/imported.fsl
fslc db import examples/db/minimal_prisma_schema.prisma --name ImportedFromPrisma
```

Successful checks return `verified_under_assumptions` with the finite rollout and
capability-completeness assumptions. Compatibility failures return
`finding_schema_version: "fsl-db-finding.v0"` plus `findings[]` containing the
environment, artifact, migration/schema element, minimal conflict set, and repair
candidates. Runtime observation returns `observed_mismatch` with
`formal_result: "not_run"`; absence from logs is not proof of unused behavior.
A non-passing nested `kernel.result` (`violated`/`reachable_failed`/
`unknown_cti`/`unknown_budget`) folds through to the top-level `result`, and
the nested `kernel` object carries that verdict's replayable evidence
(`loc`/`trace`/`blame`/`cti`/`hint`/`unreached`/... — the same registry
`fslc domain check` uses). Use ordinary `fslc verify` when you want the full
generated-kernel envelope, including diagnostics `db check`/`domain check`
deliberately do not project (coverage-name lists, run statistics, and the
like).

`fslc db observe` validates the observation envelope/events against
`schemas/fslc/db/observation.v0.schema.json` (typed required fields, a closed
`capability` vocabulary, exit 2 on a malformed record) before evaluating them,
and matches each event's optional `flags` snapshot against artifact windows the
same way `fslc db check` does.

Generic `requires` / `provides` capabilities let AI model/prompt/retriever/tool
schema and output schema profiles share the same compatibility check as
DB/API/mobile/server artifacts. A `requires tool.RefundPaymentV2` declaration is
safe only when an active or supported artifact in the same environment snapshot
`provides tool.RefundPaymentV2`. These profiles are finite coexistence facts,
not evaluator or statistical quality claims.

Importer boundary: SQL DDL import is `sql-ddl-minimal.v0`. The first
source-specific ORM importer is `prisma-schema-minimal.v0`, which imports Prisma
`model` scalar fields and reports relation/list/model attributes as
`unsupported_prisma` warnings. Production-data preservation evidence and
DB-engine evidence live in separate JSON schemas under `schemas/fslc/db/` and
use `formal_result: "not_run"`; they never upgrade sampled/audited evidence into
`verified` or `proved`.

### 13.6 AI hard-contract layer: `ai_component` (fsl-ai)

`ai_component` models the deterministic, guard-backed slice of an AI component:
tool declarations, symbolic tool schemas, business precondition evidence,
authority, human approval, forbidden tools, and fallback routing. It is a
dialect expansion, not a stochastic kernel. Probability, evaluator scoring,
groundedness judgments, prompt-injection semantic judgments, and confidence
intervals remain outside this formal kernel model and are handled as external
evidence.

Core shape:

```fsl
ai_component RefundAgentToolSafety {
  model refund_model_v1;
  prompt refund_prompt_v1;
  input RefundRequestV1;
  output RefundDecisionV1;

  tool SearchOrder {
    schema SearchOrderV1;
    precondition order_exists;
  }

  tool RefundPayment irreversible {
    schema RefundPaymentV1;
    precondition order_paid;
    precondition amount_refundable;
  }

  tool DeleteCustomerData irreversible {
    schema DeleteCustomerDataV1;
  }

  authority {
    may_execute SearchOrder;
    requires_human_approval RefundPayment;
    forbidden DeleteCustomerData;
  }

  fallback {
    when low_confidence require human_review;
  }
}
```

`model`/`prompt`/`input`/`output`/`tool`/`authority`/`fallback` are the fields
most specs need. Three more are optional, each at most once: `retriever
<id>;`, `temperature <number>;`, and a `tools [Name, ...]` shorthand that
declares bare tools with no schema/precondition/effect. A `tool` block's
`precondition <name>;` line is repeatable (0 or more) and it may also declare
one `effect <name>;`. None of these fields — nor `authority`, `fallback`, or
the `check hard { }` block below — accept a `"description text"` tag; every
field here is a bare identifier or number, unlike the declaration-tag
convention in §13.1.

Shared validation requires every standalone or project-embedded `ai_component`
to declare at least one tool, using `tool` or `tools`.
Omitting tools is a **breaking removal** (#1546): both `ai_component NoTools { }`
and `ai_component NoTools { authority { } }` are rejected before kernel lowering
with `kind: "type"`, exact message `ai_component 'NoTools' declares no tools`,
and `loc` at the component declaration (CLI exit 2). `check`, `ai check`, BMC
and induction, plus scenarios and testgen, report this authored-component diagnostic, not a generated `Tool`
enum name; Worker checking agrees. An empty `authority { }` remains valid when
at least one tool is declared.
Existing bad hard-rule and unknown-authority-tool errors still take precedence,
with their historical per-command envelopes. #1545 extends the same no-tools
rejection to `ai compat`, `ai replay`, project `ai check`, and native/Worker
project checking: a **breaking removal** of tool-less components, including
empty authority blocks, on those paths. Standalone #1546 envelopes are unchanged
on check and specialized-preflight paths; the lowering-only duplicate-fallback
priority exception is measured below.
Native `check` and the shared Worker stages also reject every project component
with an undeclared authority tool or unknown hard rule, matching project
`ai check` (semantics, exit 2), rather than reporting `ok`. This removes the
former acceptance of the fixtures `error_envelope_ai_project_unknown_tool.fsl`
and `error_envelope_ai_project_invalid_rule.fsl` under generic project checking.
Embedded component locations refer to the original document: lines and columns
are one-based, and columns count Unicode scalar values, not bytes or UTF-16 units.
The project parser translates token spans once before building component IR,
including nested locations and annotation UTF-8 byte offsets. For literate
Markdown, byte offsets refer to the extracted FSL text rather than the Markdown
file; document lines and code-line columns are preserved. Unknown hard-rule
messages point to `check hard`; existing parse errors are not translated twice.
This corrects coordinates only, without changing validity, diagnostic kind,
envelope fields, exit status or assurance. The editor publishes project-component
validation through the shared validator at document positions, with model `None`.

**Measured boundary sweep (#1545):** B
`d37a215e8df6c00222e6aafd94bc9b556c445521` and H
`99810256faca086360c6ca4233f935b9b56b1ec8` were built separately in their own
git-archive trees. `<B-binary> check <relative-path>` in B and
`<H-binary> check <relative-path>` in H checked all 597 corpus `.fsl` files and
30 Markdown files under `specs/`, `examples/`, `rust/fslc/tests/fixtures/`, plus
twelve new inline occurrences per tree (**1,278 checks**). Four Markdown files
contain compiler-recognized FSL fences; all 30, including the bash-only
`examples/ai/README.md` negative control, were checked. Cache/unstable overrides
unset and no excluded output fields. Corpus ok/exit0 → error/exit2 removals
are exactly **three**: `error_envelope_ai_project_unknown_tool.fsl`,
`error_envelope_ai_project_invalid_rule.fsl` and
`error_envelope_literate_ai_project.md`. The Markdown project reported type,
`ai_component 'EnvelopeParityLiterateProject' declares no tools`, block loc 1:1,
exit 2 (historical #1545 frame; #1574 now reports document loc 5:1);
the standalone literate component's rejection and both multi-block
toggles are unchanged. New test-inline removals are
**four**: tool-less-component projects in CLI and Worker-shaped tests (two),
an empty-authority-only component in a project (one), and an invalid later
component after a valid first component (one). Zero existing inline FSL forms
were rewritten. The three tool-less occurrences reported type at block loc 1:1
(historical frame; #1574 now reports document loc 2:3 for the indented CLI project
forms, while the first-character Worker case stays 1:1);
the later undeclared tool reports unlocated semantics. Other exit/result and
complete stdout/stderr observations in this check-only sweep are unchanged. Declare a tool or remove
an unneeded component, and correct unknown tool/rule references.

Lowering-only commands share the full validator too. With the same B/H,
`analyze <relative-path>` covered 18 AI-mentioning corpus inputs and four
explicit forms; changed forms plus a compose control were compared through
`scenarios <relative-path> --depth 2 --deadlock ignore` and `explain <relative-path>`.
Across 30 command-form pairs × 2 binaries = 60 observations,
analyze (22 forms), scenarios (4) and explain (4) each
have **one ok→error**, **zero error→ok**, and **two changed error envelopes**.
The breaking removal is an undeclared authority tool X hidden from generated
references by selecting only `tool_authority`: it now gives semantics,
`unknown tool 'X' in authority block`, no loc, exit 2 instead of success.
Duplicate fallback x without tools changes
type/`ai_component 'DupNoTools' declares no tools`/loc 1:1 to
semantics/`duplicate fallback reason 'x'`/no loc (both exit 2); with a tool it
changes unlocated semantics/`duplicate action 'fallback_x'` to
semantics/`duplicate fallback reason 'x'`/no loc (both exit 2).
A compose use of an AI component remains rejected
with `compose use must reference a spec at 2:3`, loc 2:3. No other lowering-only
acceptance removal was observed; do not generalize check-only agreement to
these intentional lowering diagnostic changes.

An `ai_component` may also declare which hard rules get an explicit,
separately-reported invariant:

```fsl
  check hard {
    rule tool_authority;
    rule human_approval_required;
    rule forbidden_tool_blocked;
    rule tool_schema_declared;
    rule tool_precondition_declared;
  }
```

Omitting `check hard { }` checks all five rules (the default); naming an
unknown rule is a check-time error. Narrowing the set only removes an
explicit, separately-reported invariant for `forbidden_tool_blocked` /
`human_approval_required` — the structural guards themselves (no
execute-action is ever generated for a forbidden tool; an approval-required
tool's execute action always carries a `requires human_approved` guard) are
generated unconditionally either way. `tool_authority`,
`tool_schema_declared`, and `tool_precondition_declared` are checked
unconditionally regardless of this block.

`ai_component` lowers to a kernel spec with finite tool state:

- `Tool` enum
- `human_approved: Map<Tool, Bool>`
- `tool_executed: Map<Tool, Bool>`
- generated `approve_*` / `execute_*` actions
- generated invariants for approval-before-execution and forbidden tools

Use `fslc ai check` when you want fsl-ai vocabulary:

```bash
fslc ai check examples/ai/refund_agent_tool_safety.fsl
fslc ai check examples/ai/recursive_support_agent.fsl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_human_approval_bypass.jsonl
fslc ai eval examples/ai/support_answer_quality.fsl --property LooseQuality
fslc ai regress examples/ai/support_answer_quality.fsl --migration PromptV7ToV8 --before-records examples/ai/support_eval_v7.jsonl --after-records examples/ai/support_eval_v8_regressed.jsonl
fslc ai drift examples/ai/support_answer_quality.fsl --logs examples/ai/runtime_drift_current.jsonl --baseline-logs examples/ai/runtime_drift_baseline.jsonl
fslc ai compat examples/ai/support_answer_quality.fsl --environment prod
```

Successful `ai_component` hard-contract checks return
`verified_under_assumptions`. Recursive `agent` checks return
`agent_analyzed` with `formal_result: "not_run"` because they are structural
graph analysis, not kernel proof. `fslc ai replay` accepts JSONL or
`{ "events": [...] }` and returns
`replay_conformant` / `replay_nonconformant` with
`formal_result: "not_run"` because replay is observation evidence. Findings use
`finding_schema_version: "fsl-ai-finding.v0"` and include
`guarantee_kind`:

- `syntactic_hard`: schema/authority/approval/forbidden/precondition guard facts.
- `agent_structural`: recursive-agent scope, grant, visibility, delegation, and
  tool-reachability findings.
- `runtime_observed`: declared component capability differs from observed events.
- `statistically_supported` / `statistically_unsupported`: precomputed eval JSONL
  and Wilson confidence-bound evidence from `fslc ai eval`; never displayed as
  `proved`.
- `evaluator_supported`: reserved for external evaluator-backed evidence and must
  not be displayed as `proved`.

Project-level fsl-ai evidence declarations can combine `ai_component`,
`dataset`, `evaluator`, `failure_mode`, `statistical_property`,
`ai_migration`, and `observed_property`. `fslc ai check` parses these files with
the same parser the evidence commands run and returns `ai_project_analyzed`;
`fslc ai eval` checks the selected
`statistical_property`'s declared `slice`/`min_samples`/`ci_lower`/`ci_upper`
requirements against precomputed JSONL (`--records`, or the declared
`dataset`'s `source` file) with Wilson intervals; `fslc ai regress` checks the
selected `ai_migration`'s declared aggregate `no_regression` metric
drop/increase clauses; `fslc ai compare` reports metric deltas without a
threshold claim; `fslc ai drift` checks the selected `observed_property`'s
declared `observed`/`drift` requirements over runtime telemetry
(`observed_supported` / `observed_mismatch`); and `fslc ai compat` emits a
finite `dbsystem artifact` capability profile for one `ai_component` or every
`ai_component` a project declares, rejecting non-AI input and an AI project
with no `ai_component` at all (exit 2). Before `fslc ai check`, `fslc ai
compat`, or `fslc ai replay` may emit a success verdict, every `ai_component`
in the input -- standalone or embedded in a project -- is checked for the same
semantic constraints as `fslc verify` on a lone component: undeclared tools
referenced in `authority`, and unknown `check hard { rule ... }` names (exit
2, `kind:"semantics"`). `fslc ai replay` on a project file uses the same
checked project parser as `fslc ai check`, so malformed project syntax cannot
report `replay_conformant`. All of these use
`formal_result:"not_run"`. A `require` clause matching none of the known
evidence-clause grammars (`min_samples`, `ci_lower`, `ci_upper`, a point
estimate, `observed`, `drift`) is a spec error (exit 2) at `check` time, in
both `fslc ai check` and the `fslc check` dispatch: the check stage must not
report an analyzed project that `eval`/`drift` cannot execute. An unknown
`--property`/`--migration` selection is
a check-time error (exit 2); a selected `statistical_property`'s gate status
(`dataset_invalid`, `evaluator_untrusted`, `slice_missing`,
`insufficient_samples`, `inconclusive`, or `statistically_unsupported`) exits
1, matching `statistically_supported`'s exit 0 as the only success case.

Recursive `agent` shape:

```fsl
agent SupportOrchestrator {
  context [CustomerTicket, ApprovedSupportDocs];
  tools [SearchDocs, CheckPolicy, CreateDraft];
  authority {
    may_execute [SearchDocs, CheckPolicy, CreateDraft];
  }
  review_gate PolicyCheckAgent;

  agent RetrievalAgent {
    trust medium;
    grant authority [SearchDocs];
    grant context [ApprovedSupportDocs];
    tools [SearchDocs];
    authority { may_execute [SearchDocs]; }
    output RetrievedSources visibility [parent, PolicyCheckAgent];
  }

  agent PolicyCheckAgent {
    trust high;
    grant authority [CheckPolicy];
    grant context [CustomerTicket, ApprovedSupportDocs];
    tools [CheckPolicy];
    authority { may_execute [CheckPolicy]; }
    contract { hard { rule PolicyMustCiteSource; } }
    output PolicyDecision visibility parent;
  }

  orchestration {
    RetrievalAgent -> PolicyCheckAgent;
  }

  failure_policy {
    when RetrievalAgent.failed -> retry up_to 2;
    when RetrievalAgent.failed_after_retry -> HumanReviewPending;
  }
}
```

Nested agents are ordinary agents scoped by their parent, not a distinct
`sub_agent` type. Nesting creates lexical names such as
`SupportOrchestrator.RetrievalAgent`; runtime collaboration is declared
separately in `orchestration`. Parent authority/context is not inherited
implicitly. A child must receive explicit `grant authority` and `grant context`,
and each grant must stay inside the immediate parent boundary. `model`/
`prompt` are also valid at any agent level (root or child), and a direct
`tool { }` block works inside an agent the same way it does inside
`ai_component`. `review_gate <Child>;` names a **direct child** agent through
which every orchestration path to a high-authority-tool descendant must pass;
a path that skips all declared review gates is flagged
`policy_review_bypass_in_orchestration`. `trust` is a free identifier, not a
validated enum — only the literal `low` currently drives a dedicated check
(a low-trust agent's path to a high-authority tool); other values parse but
have no distinct check yet. `contract { hard { rule <Name>; } }` is parsed
and listed per agent, but — unlike `ai_component`'s `check hard { }` — its
rule names are not validated against a known set and are not yet
cross-checked against anything; treat it as forward-declared metadata.

Design details: `docs/design/DESIGN-ai-hard.md`.

### 13.7 What is not handled (the boundary of the layers)

The majority of non-functional requirements (permissions, audit, capacity,
reliability behavior, discrete-time SLA) can be handled (§13.4). What remains
outside FSL is: **probability, percentiles (99.9% etc.), real time (wall-clock
ms), usability, DB optimizer/lock timing, full production-data proof, evaluator
truth judgments, and prose rationale** (write these in each layer's document).
FSL is responsible for the **checkable skeleton** of each artifact.

## 14. Library API

```python
from fslc import parse, build_spec, verify, prove, Monitor

spec   = build_spec(parse(src))
result = verify(spec, depth=8)            # BMC
result = prove(spec, k_ind=1, base_depth=8)   # k-induction
```

Returns a dict with the same structure as the CLI (the CLI wraps it with a
`"fsl": "1.0"` envelope). Native CLI and browser Worker `check`/`verify`
envelopes also contain `versions.verifier`, `versions.core`, and
`versions.solver` objects. Each object has `name` and `version`; the solver
object additionally has `backend`. Solver versions come from the loaded native
or browser Z3 runtime rather than a CLI constant. The machine-readable contract
is `schemas/fslc/envelope.v1.schema.json`.

## 15. Validation suite (the spec ≠ intent gap)

What `fslc` guarantees is "the internal consistency of the written spec," not
"whether the spec is faithful to the original intent." When you have AI write a
spec, errors concentrate in this validation layer. The following is the set of
checks that **surface those errors as mechanical mismatches** (the overall
picture of the design is roadmap issue #1; each feature has a corresponding
DESIGN-*.md).

- **`forbidden` (negative acceptance criteria)** — a requirements-dialect
  construct. Write an "operation sequence that should be rejected," and at check
  time it is replay-verified that the last step is rejected, i.e. not enabled
  because a guard refuses it or its argument is outside the declared type.
  If it is accepted, `kind:"forbidden"` (detection of
  under-constraint = a missing guard, which a safety invariant stays silent
  about). **Since #1213 (breaking)** a last step that is enabled and then
  stops with a runtime violation (`invariant` / `trans` / `ensures` /
  `type_bound` / `partial_op`) is also `kind:"forbidden"`, with a `violation` field: a
  violation is not a rejection. Earlier versions counted it as satisfied; to
  migrate, add the `requires` that rejects the call. There is no opt-out.
  Because this check runs before BMC and induction, `verify` on such a spec now
  ends with this error (exit 2) at any `--depth` and engine, where earlier
  versions reported whatever the engine reached: with BMC, `violated` (exit 1)
  when the depth reached the violation, `verified` (exit 0) when it did not;
  with `--engine induction`, its own verdict. Every other command that runs this
  check exits 2 too (`scenarios` and `testgen` with no scenario and no test
  file). **Since #1242 / #1243 (breaking)** that includes `explain`, `html`,
  `ledger`, the `document` commands, and `approval create` / `check` of every
  kind: each prints the gate's error and writes no report, ledger, document,
  or record (before, `explain` exited 0 with no witnesses, `html` and `ledger`
  wrote their report, the `document` commands rendered the case as a claim,
  `approval create --kind ledger` / `requirements_document` recorded an
  approval, and `approval check` of a ledger record reported `drifted`,
  exit 0). The language server shows the error as an `FSL-REQUIREMENT-TRACE`
  diagnostic, and `fslc analyze` still graphs the cases, each node marked
  `"trace_gate": "unchecked"`. `fslc diff` reports the violation (below).
  The measured before/after of each command, exit status and output, is in
  [`DESIGN-forbidden.md`](../design/DESIGN-forbidden.md) §2.1.
  The scenario's `rejected_by` (asserted by testgen) is
  `requires_failed` for a guard refusal and `bad_call` for an argument of the
  parameter's kind outside its declared range type (an implementation must
  reject it at its decoding boundary). **Since #1241 (breaking)** an argument
  of another kind — an enum member or a Bool for an `entity`, `number`, or
  range parameter, an integer or another enum's member for an enum parameter
  (`respond(Waiting)` with `c: Case`) — is a malformed step, not a `bad_call`:
  it is a `kind:"forbidden"` error with `wrong_kind_argument` (`{parameter,
  value, type}`), on every command that runs the forbidden gate. To migrate,
  write a value of the parameter's type. **Since #1229 (breaking)** an argument
  outside the verification scope of an `entity` (`verify { instances }`, which
  also bounds a `process`) or `number` (`verify { values }`) parameter is not a
  rejection: under `instances Case = 3`, a last step `accept(7)` evaluates no
  guard, and an implementation, which has no such bound, may accept that call.
  It is a `kind:"forbidden"` error with `out_of_scope_argument` (`{parameter,
  value, type, scope}`). To migrate, widen the scope to include the value, or
  move the argument inside the scope so the guard is tested. Under a
  `--instances` / `--values` override that alone removed the argument, the
  forbidden is reported as `forbidden_skipped` (not evaluated) instead. A last
  step naming no action, or no variant of that arity, is a `kind:"forbidden"`
  error with a `message`, not a rejection. Both are breaking changes: such a
  step used to satisfy the forbidden (one naming no callable action through
  4.8.1, one outside the verification scope as `requires_failed` before #1212
  and `bad_call` since), so `check` passed, and what the other commands
  reported depended on the rest of the spec: `verify`, `sweep`, `chain`,
  `mutate`, `html`, and `ledger` exited 0 when it verified and 1 when it did
  not, while `scenarios`, `testgen`, `counterexample export`, and `approval
  create` could also exit 2. Now `check`, `verify` (every engine), `sweep`,
  `chain`, `scenarios`, `testgen`, the `mutate` baseline, `counterexample
  export`, `explain`, `html`, `ledger`, the `document` commands, and
  `approval create` / `check` of every kind exit 2 and report no verdict,
  counterexample, scenario, test, mutant, explanation, report, ledger,
  document, or record for the rest of the spec. `chain` reports the error as
  the `[requirements]` layer's `detail`; the others print it (`error` /
  `kind: "forbidden"`), and the language server shows it as an
  `FSL-REQUIREMENT-TRACE` diagnostic (`explain`, `html`, `ledger`, the
  `document` commands, `approval create --kind ledger` /
  `requirements_document`, and `approval check` of a ledger record only
  since #1242 / #1243; §2.1 of the design note has their before and after).
  `fslc diff` changes forbidden
  verdicts for both steps, which can change a `--forbid unknown` exit. The
  before/after of every command, measured, is in
  [`DESIGN-forbidden.md`](../design/DESIGN-forbidden.md) §2.1.
  The dual of `acceptance` (must-allow). → [`DESIGN-forbidden.md`](../design/DESIGN-forbidden.md)
- **Vacuity check (`--vacuity`)** — on the verified/proved path, warns about
  `never_enabled_action` (an action has no enabled instance through the checked
  depth; bounded evidence, not a permanent-dead proof), `vacuous_implication`
  (the antecedent of an implication is unreachable),
  `vacuous_leadsto` (the trigger is unreachable), `always_true_requires`
  (a guard that is always true under the context of preceding clauses),
  `tautology_over_frozen` (a dynamically tautological invariant over state no
  action changes), `urgency_freeze` (a generated deadline `tick` proven dead
  because urgency freezes time), `vacuous_deadline` (a generated deadline
  whose age is proven to remain zero across every transition), and
  `vacuity_probe_truncated`†. `error` exits 2. →
  [`DESIGN-vacuity.md`](../design/DESIGN-vacuity.md)

  `never_enabled_action` findings apply only to source-backed actions (authored
  primary origin, then kernel declaration, with positive line/column).
  Generated-only sentinels remain in raw coverage evidence but not in these
  warnings, ledger coverage findings, or HTML badges/coverage denominators.

  † `vacuity_probe_truncated`: the `vacuous_implication`/`vacuous_leadsto`
  reachability probe shares one budgeted concrete BFS across every
  antecedent/trigger in the spec; a candidate that neither becomes true nor
  finishes exhausting its reachable state space before the state-count
  budget is hit reports this kind instead of `vacuous_implication`/
  `vacuous_leadsto` — vacuity was never established either way, so treating
  it as confirmed-vacuous would be a false positive and dropping it would
  let `--vacuity error` pass a spec whose vacuity was never actually
  decided. Selected by `--vacuity` exactly like the other seven kinds.
- **`--strict-tags`** — warns on success results about untagged declarations
  (fabrication candidates) and unreferenced requirements (omission candidates,
  including empty requirement blocks). Existence-level matching. → [`DESIGN-strict-tags.md`](../design/DESIGN-strict-tags.md)
- **`fslc mutate`** — mechanically mutates the spec and measures whether each
  mutant is killed by the existing net of checks. The score is **bounded
  mutant-set sensitivity**: `kill_rate = killed / (killed + survived)` over a
  selected finite mutant set, at the selected `--depth`, under the
  verify/acceptance/forbidden/refinement oracle. It moves with the operator
  mix, the `--max-mutants` cap, the depth, and the oracle — it is not a
  production defect-detection rate, not a probability that the spec is
  correct, and not a completeness measure. Survivors are a review queue, not
  automatic missing-invariant findings: a survivor may be an equivalent
  mutant, behavior dead at baseline, an effect visible only beyond the depth
  bound, or genuine under-constraint.
  The baseline uses auto (default 1,000,000-state budget), accepting `verified`
  or `proved`; a nonaccepted auto result reruns BMC on a fresh thread, continuing
  if verified or returning that baseline envelope otherwise. JSON retains
  `baseline:"verified"`. For each mutant, a boundary-probe finding wins first;
  a completed empty probe then permits explicit exploration (50,000 states) to
  certify clean through K only when every reachable is witnessed; if neither
  decides, the existing BMC path chooses the verdict/killer. Secondary checks
  still run. Stderr reports the accepted baseline and each evaluated mutant,
  with `engine=explicit|bmc|probe|none`
  and elapsed seconds; the engine names the main-oracle route, not later checks.
  Stdout is unchanged except for the encoding-only residual in
  [`DESIGN-mutate` §3](../design/DESIGN-mutate.md#3-kill-oracle-and-baseline-gate):
  explicit-clean can survive where BMC alone reported `build_spec`.
  Baseline refusals/command errors emit no progress.
  `--from mutants.jsonl` additionally adjudicates externally generated
  mutations expressed as a full `mutated_spec` or an exact
  `replace:{target,replacement,occurrence?}` instruction. Valid external
  mutants use the same verify/acceptance/forbidden/refinement oracle. JSON,
  instruction, parse, name, type, and construction errors are `invalid`, never
  killed, and are excluded from combined/per-source kill-rate denominators
  (they measure external generator quality, not spec strength); a built-in
  mutant that does not build is `invalid` the same way. `invalid` means statically
  invalid input, including implements-map static type errors (auto or explicit
  maps). This validation precedes runtime oracles: a main or requirement kill
  cannot turn an invalid map into a judged mutant. Invalid inputs are excluded
  from `gate.judged` and `kill_rate`, regardless of runtime verdict order.
  A mutant the oracle
  could not judge (Z3 unavailable, solver `unknown`/backend failure, or an
  acceptance/forbidden/implements evaluation failure) remains `status:"error"` with
  `error{stage, message}`: never killed, excluded from `kill_rate`, counted in
  `summary.errored` (present only when non-zero, like `summary.inconclusive`),
  and it fails any requested gate (`oracle_errors`). Precedence for one mutant:
  `killed` > `error` > `inconclusive` > `survived`.
  Every entry carries `source:"builtin"|"external"`; `--max-mutants` caps only
  the built-in catalog, so `--max-mutants 0 --from ...` runs external-only.
  `--by-requirement` flags "a requirement that kills no behavior mutant" as an
  `empty_formalization` warning (the semantic-level extension of
  `--strict-tags`); per-requirement kill counts and this warning are observed
  lower bounds within the chosen mutant set and depth. Acceptance and forbidden
  kills use explicit requirement annotations on the failed trace declaration;
  AC/FB case IDs are not implicit requirements. Trace case IDs are unique
  within each declaration kind. `--oracle-attribution` (opt-in) adds per-mutant
  `killers` arrays and `by_obligation` sole/shared counts keyed by oracle display
  names; default output is unchanged and these counts are observed lower bounds,
  not completeness or correctness measures.
  `--fail-on-survivors` and `--min-kill-rate R` (opt-in, `R` in `[0, 1]`) add a
  `gate{fail_on_survivors, min_kill_rate, judged, survived, kill_rate, dropped,
  violations, passed}` object (plus `errored` / `inconclusive` when non-zero); `result` stays `mutated` and
  `gate.passed` decides the exit code (1 when false). Zero judged mutants fails
  either flag (`no_judged_mutants`), and so does any oracle error
  (`oracle_errors`); `--min-kill-rate` compares the published four-decimal
  `summary.kill_rate` with `>=`; built-in mutants dropped by `--max-mutants` are
  recorded in `gate.dropped` and do not fail the gate. Without either flag the
  output and exit code are unchanged.
  → [`DESIGN-mutate.md`](../design/DESIGN-mutate.md)
- **`fslc explain --readable`** — a text view over skeleton enumeration (state,
  action who/when/what-changes, verification bounds, fairness, KPI projections,
  branch lowering, synthesized refinement mappings, automatic checks, tags) +
  counterfactuals ("without this rule, this procedure could break it") + witness
  narration. Moves human review from reading logical formulas to adjudicating
  concrete examples. JSON mode remains available without `--readable`. →
  [`DESIGN-explain.md`](../design/DESIGN-explain.md)
- **`fslc analyze`** — emits structural observation JSON. `--projection tsg`
  returns the Typed Semantic Graph of requirements, actions, state, properties,
  and scenarios. Init carrying requirement relations also gets a node with
  id/kind/name/label `init`, a spec-to-init `declares` edge, and requirement-to-init
  `covers` edges. It is a useful anchor for `disconnected_requirement` and is
  included in `requirement_property_graph`; init without requirement relations
  adds no graph output. Graph projections return connected components, SCCs,
  representative cycles, degree, and structural metrics such as cycle rank and
  fan-in/fan-out hubs. `--projection action_dependency_graph` exposes structural
  action enables/conflict edges; `--projection impact_graph --focus NODE` emits
  the upstream/downstream slice around a TSG node. It accepts multiple files or
  directories in batch mode;
  directories are expanded recursively for `*.fsl` and sorted deterministically —
  an explicitly named file is always kept regardless of extension (a `.toml`
  project manifest routes the same way it does in single-file mode; anything
  else that cannot be analyzed is a real error in `files[]`/`errors[]`, never
  a silent drop).
  Standalone refinement mappings can be viewed with `--projection
  refinement_graph`; this is an unresolved structural view because the command
  has no impl/abs model paths. Project manifests can be viewed with `--projection
  traceability_graph`; their action edges use the checked correspondence IR and
  include synthesized auto mappings. `--projection code_audit --code <path>`
  maps exact executable Kernel requirement targets to language-independent
  `@fsl.trace` source annotations; missing/orphan/mismatched pairs remain review
  findings with `formal_status:not_a_violation`. It is single-spec and JSON-only.
  `--format dot` and `--format mermaid` export graph-shaped
  projections for review diagrams while keeping JSON as the default. `--profile
  ai-review` emits review findings such as `disconnected_requirement`,
  `unanchored_property`, `progressless_cycle`, `unwritten_state`,
  `unread_state`, `unguarded_action`, `conservation_candidate`,
  `divergent_choice`, and `unconstrained_effect`. The last two use a fixed
  depth-4 bounded probe (solver-free explicit-state exploration via the runtime
  Monitor): they include `evidence_basis:"bounded_bmc"` (frozen v0 vocabulary
  for "backed by a bounded reachability witness"), the reachable
  branch witness, and a question-form `spec_question` asking which outcome is
  intended. Exact matches with `undecided:` declarations remain visible with
  `acknowledged:true` and `acknowledged_by`; unmatched semantic findings carry
  no acknowledgement fields. A bounded-witness-backed `unconstrained_effect` suppresses the same state's
  structural `unread_state`; semantic action witnesses similarly suppress a
  duplicate `unguarded_action`. Absence is not proof of determinism beyond the
  bound. See [`DESIGN-underspecification.md`](../design/DESIGN-underspecification.md).
  Exact identifier checks additionally emit `tag_stale_reference` when a
  code-shaped tag token no longer exists and `tag_formula_disjoint` when a tag
  names a current state/constant absent from that declaration's formal
  definition. `--export tag-review` emits tagged declarations one at a time
  with their rendered formal definition under schema `tag-review.v0`; it does
  not judge natural-language meaning or call a model.
  These findings carry `formal_status: "not_a_violation"`; a structural cycle or
  disconnected component is not a proof failure. Versioned schemas for the TSG,
  graph projections, and findings are published under `schemas/fslc/analysis/`. →
  [`DESIGN-analysis.md`](../design/DESIGN-analysis.md)
- **`fslc html`** — a self-contained HTML report over the same explain/verify
  evidence: status summary, state/action/property tables, an action-to-state
  write graph, trace timelines, witness examples, counterfactuals, source, and
  raw JSON. It is meant for PRs, design reviews, and non-specialist project
  review without requiring the reader to run the CLI. By default it retains
  nondeterministic timings. HTML-only `--no-timing` (default false, no alias)
  omits only `verification.cost.elapsed_s`,
  `verification.cost.solver.check_elapsed_s`,
  `verification.cost.properties[].elapsed_s`,
  `explained.counterfactuals[].violation.cost.elapsed_s`,
  `verification.cost.solver.memory_mb`, `verification.cost.solver.conflicts`,
  `verification.cost.solver.decisions`, and
  `verification.cost.solver.propagations` from presentation
  copies before rendering, on stdout and in `-o`/`--output` files alike.
  Source and domain-state fields, `solver.checks`, and property
  `kind`/`name`/`checks` remain intact; ordinary verify JSON and exit codes are
  unchanged. Wall-clock timings and environment-dependent Z3 internal
  statistics are omitted; default HTML retains all timings and solver statistics.
  This supports byte-stable comparison for the same input path, source,
  options, and toolchain, not normalization across paths, backends, or
  environments.
  `approval create --kind html` regenerates the default timing-bearing report and normalizes timing and ambient solver-statistic values without removing their keys, so it rejects a `--no-timing` artifact as not matching a fresh rendering. →
  [`DESIGN-html-report.md`](../design/DESIGN-html-report.md)

The discipline before writing (the formalization memo, the NL→syntax reverse
lookup, recommended practices) is in the AI-agent skills under `skills/`, with
the shared language reference in `skills/fsl/SKILL.md`. A maintained worked example
is under [`examples/validation/`](https://github.com/ymm-oss/fsl/tree/main/examples/validation).

## 16. Promotion judgment to ghost types (typestate)

`fslc typestate <file.fsl> [--ts]` decides how soundly the state machine of a
design spec (enum-valued struct fields, scoped by field name **and** owning
struct type so two structs with a same-named field remain independent
machines / state variables / `Option<_>` slots) can be mapped to the host
language's typestate (ghost types). It classifies each
`(entity, action)` as `derivable` (the from-state is the entity's own local
guard — a compound guard's from-state is what the **whole** formula implies:
`or` is a union of what each disjunct implies, but only when every disjunct
constrains the entity; a disjunct silent about the entity, e.g. an unrelated
flag, drops the guard entirely rather than narrowing it) /
`branching` (data-dependent inside an `if`) / `relational` (no local
guard, and the precondition lives in an external structure — cannot be expressed
in the type and remains as a runtime/verification obligation). An entity's
`applicability` is `full` only when all transitions are derivable/branching.
With `--ts`, it outputs a TypeScript scaffold for the derivable portion.
The native Rust command performs this judgment from the versioned public Kernel
JSON v1 contract, not private parser/model structures, and fails closed on an
unsupported Kernel schema version. Relative to the frozen reference, native
judgment already scopes enum fields by owning struct and drops a disjunctive
guard when any disjunct does not constrain the entity. Since #680, locally
guarded state-preserving actions appear as self-loop report entries with
`state_preserving: true` and generic
TypeScript signatures preserving the exact input state; #1431 adds reserved-word
escaping (including collision-free identifier allocation) and model-derived
parameter types. The JSON `typescript` fields carry all these TypeScript
differences. The command envelope and existing report field shapes remain
compatible (including source-name `params`); report values, entity inclusion,
applicability and summary counts can differ under the earlier judgment rules.
Native entity order is alphabetical (the frozen reference uses declaration order).
Neither JSON nor TypeScript is claimed byte-identical to the frozen reference.
→ [`DESIGN-typestate.md`](../design/DESIGN-typestate.md)

## 17. The causal profile (review-only causal hypothesis graphs)

`causal <Name> { ... }` is a standalone sidecar document (its own `.fsl` file,
never part of a kernel spec) that structures long-horizon causal hypotheses:
interventions, mediators, outcomes, contexts, time lags, persistence, delayed
feedback, measurement bindings, and applicability scope. **FSL never proves
real-world causality**: every causal output carries
`formal_result: "not_run"` and a `do_not_assume` array, no output path
attaches `proved`/`verified` to a causal claim, and there is deliberately no
`fslc causal verify` command.

```bash
fslc causal check model.fsl
fslc causal analyze model.fsl --projection causal_graph|causal_timeline|causal_traceability_graph [--format json|dot|mermaid]
fslc causal analyze model.fsl --profile causal-review
fslc causal diff before.fsl after.fsl
```

`fslc check model.fsl` on a causal file routes to the causal checker
automatically. A model declares one discrete `timebase`
(`tick | hour | day | week`) and a finite `horizon`; `uses <alias> from
"<path>"` imports kernel/business/requirements specs so variables can bind
actions (`binds action alias.name`) and observe KPIs, states, or properties
(`observes kpi alias.name`, etc.). Claims are directed hypotheses
`claim <Id> a -> b { version N status active|retired polarity
positive|negative|unknown lag a..b|unknown persists a..b|unknown|unbounded
basis hypothesis|assumption ... }` with stable IDs and monotonically increasing
content versions; retired claims keep their identity and history. Cycles with
a positive minimum lag sum are allowed but must be acknowledged with a
`feedback` declaration (an unacknowledged cycle is a warning; a zero-lag cycle
is an error). Feedback loops are classified `reinforcing | balancing |
unknown` from the sign product of edge polarities (`unknown` absorbs), and the
timeline reports Minkowski-sum first-response windows (exact minimum; the
maximum is an upper bound when the pair is connected through a feedback SCC).
`--profile causal-review` emits deterministic review findings (all
`formal_status: "not_a_violation"`) such as `single_hypothesis_bottleneck`,
`high_leverage_untested_claim`, `opposing_path_polarity`,
`potential_common_cause`, `feedback_without_damping_story`,
`deadline_before_earliest_effect`, `observation_window_misses_effect`,
`measurement_cadence_too_coarse` (fires exactly when a measurement cadence
exceeds an arriving claim's minimum persistence; unknown persistence is
reported as `not_evaluable`, never guessed), and `unknown_lag_blocks_timeline`.
`fslc causal diff` compares two model files by stable claim ID and content
version. `causal-diff.v0` accepts no evidence inputs, so its
`support_transition` stays `not_available`; evidence aggregation is reported
separately by analyze/ledger projections. It reports
`content_changed_without_version_bump` when
version-relevant fields move under the same version,
`retired_claim_reactivated` when a terminal retired claim returns to active,
and `retired_hypothesis_reproposed` when a new claim repeats a retired claim's
source, target, and polarity.

External evidence enters through versioned artifacts
(`fsl-causal-evidence.v0`: closed `design` vocabulary
`randomized_experiment | quasi_experiment | observational | expert_judgment`,
directed `support` `supports | challenges | inconclusive`, claim-ID **and
content-version** pins, scope tokens, ISO-8601 `period`/`valid_until`, an
`artifact_digest` over the canonical payload) plus independent append-only
lifecycle chains (`fsl-causal-evidence-lifecycle.v0`: digest-linked records;
`retracted`/`superseded` are terminal and never rewrite the payload).
`fslc causal analyze model.fsl --evidence a.json [--lifecycle a.lifecycle.json]
[--as-of YYYY-MM-DD] --projection causal_evidence_graph` (or
`--profile causal-review`) validates artifacts fail-closed — schema/digest/
lifecycle-chain violations stop the analysis — and aggregates a deterministic
per-claim `causal_support`: `untested`, `supported`, `challenged`,
`inconclusive`, `mixed`, or `unsupported_by_current_evidence`. Only artifacts
that pin the current claim version, whose scope `subsumes` the claim scope,
with declared freshness, an `active` lifecycle, and an observation window at
least the claim's minimum lag count; one source lineage collapses to one vote
(contradictions inside a lineage are `inconclusive`). An observation window
that cannot be converted, or a claim whose `lag` is `unknown` so there is no
`lag_min` to compare against, leaves timing eligibility not evaluable: the
edge stays in history and in the graph but is excluded from current support
with `evidence_timing_not_evaluable` and casts no vote. Staleness is judged only
against an explicit `--as-of` date — never the wall clock. A scope dimension
present on only the claim or artifact side is `unassessable`; absence is never
treated as universal. **`causal_support`
and `formal_assurance` are orthogonal axes**: evidence never changes
`formal_assurance: "not_run"`, and no support value is a verdict.

A human can carve an observable contract out of a claim as an `expectation`
and check it with the existing verifier — **the claim itself is never
lowered**: `expectation E { trigger action alias.name` (or `trigger
predicate alias { <kernel expr> }`) `response predicate alias { <kernel
expr> } within N clock <name> derived_from_claim <Id> }`.
`fslc causal verify-expectations model.fsl [--depth K]` compiles each
expectation, fail-closed, into an ordinary `leadsTo ... within ticks`
property on the imported kernel spec (an action trigger becomes a one-step
pulse ghost; `within` must convert to an exact integer number of kernel
ticks under the named clock — nothing is rounded, and a clock is never
applied to a different spec) and reports `pass`/`violated` with
`assurance: "bounded"`. Whatever the verdict, the derived claim's
`formal_assurance` stays `"not_run"` and its `causal_support` is untouched:
`derived_from_claim` is traceability in both directions, never evidence.
The legacy field name `supports` is rejected. Versioned
schemas live under `schemas/fslc/causal/`; the browser Worker does not expose
causal commands. Working models live in
[`examples/causal/`](https://github.com/ymm-oss/fsl/tree/main/examples/causal).
→ [`DESIGN-causal.md`](../design/DESIGN-causal.md)
