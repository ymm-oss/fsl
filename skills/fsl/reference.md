# FSL Language Reference Card (complete, condensed)

Read this entire file before writing a spec. This is the full syntax and full set
of rules as of v2.x.

## 1. Top-level structure

The native parser selects every dialect from one shared lexer/registry. A leading
UTF-8 BOM, whitespace, and `//` comments are trivia. Typed document annotations
may precede the dialect keyword and attach to the document rather than affecting
dispatch:

```fsl
@requirement("REQ-CHECKOUT-001", "document contract")
@acme.review(owner.platform, 2, true)
spec <Name> { ... }
```

Document annotations support `@requirement(id, text?)`, `@undecided(reason)`,
`@kind(id, text?)`, and multi-segment custom namespaces. Annotation argument
keywords are never dialect keywords. Empty/unknown documents use the stable
`FSL-DIALECT-EMPTY` / `FSL-DIALECT-UNKNOWN` diagnostics.

The same `@...` syntax also attaches directly to a nested declaration —
`init`, `action`, `invariant`/`trans`/`reachable`/`until`/`unless`/`leadsTo`, a
process `transition`, or a `requirement`/`acceptance`/`forbidden` block — in
the spec/business/requirements/compose dialects:

```fsl
@requirement("REQ-CHECKOUT-003", "the ledger matches payments")
@undecided("late gateway completion policy is pending")
invariant PaidLedger { ... }
```

Multiple annotations may stack in any order without changing the checked
result; comments/blank lines between them or before the target do not break
attachment. They coexist with (and desugar to the same relation as) the legacy
`"ID: text"` string slot and `covers`/`requirement` block relations — see §13.1.
An annotation with nothing supported to attach to reports
`FSL-ANNOTATION-TARGET`. `domain`/`dbsystem`/`ai_component` nested
declarations also accept `@...` (issue #281): `domain` aggregate `command`,
`decide`, `evolve`, `invariant`, `projection`, `effect`, and saga `step`;
`ai_component` `tool`, the `tools [a, b];` shorthand, `authority` (and each
rule line), `fallback` (and each `when` item), `check`; `dbsystem`
`migration` and each `check compatibility { rule ...; }` line. A
`command`/`decide` pair and any matching `evolve` union onto the one action
they generate together; an `effect` or saga `step` broadcasts to every
action it generates. This is native-only syntax the frozen Python reference
does not parse.

**Rationale for tooling/AI consumption:** comments are trivia and invisible
to any structured output (JSON envelope, LSP, audit ledger). Use
`@kind(id, text?)` to classify + explain a declaration in one line (e.g. an
aux invariant's CTI provenance, or why an impl guard is deliberately stronger
than the abstract one); use the custom namespace `@doc.rationale("...")` for
a short rationale that isn't a classification. Keep multi-sentence narrative
in `//` comments — annotation strings have no escape syntax and stop at the
first `"` or newline.

`@kind` and custom annotations survive in the checked model for in-process
consumers that explicitly query `KernelModel::annotations_for`. The current
JSON envelope, LSP index, and audit ledger do not project generic annotations;
do not assume those public consumers can query a rationale without a separate
projection contract.

```fsl
spec <Name> ["<kind>: <intent>"] {        // optional spec-level tag → metadata badge (explain/html); never verified
  const <NAME> = <const expr>             // integer constant (expressions allowed: CAP - 1, etc.)
  type  <Name> = <lo>..<hi>               // domain type (bounded integer)
  symmetric type <Name> = <lo>..<hi>      // interchangeable entity identities
  enum  <Name> { <Member>, ... }
  symmetric enum <Name> { <Member>, ... }
  struct <Name> { <field>: <type>, ... }  // field: scalar | Option<scalar>
  def <name>(<p>: <type name>, ...) = <expr> // non-recursive predicate, frontend-inlined

  state { <var>: <type> [= <deterministic expr>], ... }
  init  ["undecided: reason"] { <stmt>... } // assign exactly once per variable/Map-key (deterministic)

  [fair] action <name>(<p>: <type name>, ...) {
    requires <expr>                        // 0 or more. conjunction. enabled condition
    let <x> = <expr>                       // local binding
    <stmt>...
    ensures <expr>                         // 0 or more. old(expr) for the old state
  }

  invariant <Name> { <expr> }
  trans     <Name> { <expr> }            // two-state safety. old(expr) for the old state
  reachable <Name> { <expr> }
  leadsTo   <Name> { <expr> ~> [within K] <expr> [helpful act(args)] [decreases <int expr>] } // outer forall x: T { … } may wrap the response
  unless    <Name> { <expr> unless <expr> } // safety: preserve P until Q
  until     <Name> { <expr> until <expr> }  // unless safety + progress P ~> Q
  terminal  { <expr> }                     // intended terminal state (excluded from the deadlock check)
}
```

Action parameter types (`<p>: <type name>`): domain type, enum, or builtin
`Bool` — anything BMC can enumerate. `Bool` params behave like `Bool` state:
usable bare as a guard (`requires b`, `requires not b`) or assigned into
`Bool`-typed state (`flag[i] = b`). Builtin `Int` is rejected (unbounded,
can't be enumerated) — use a range parameter instead:
`action f(p in <lo>..<hi>) { ... }` (inline alternative to `<p>: <type name>`,
no named domain type required).

Use `def` to give a business name to a repeated guard/property expression.
Calls are file-local and arity-checked; direct/mutual recursion and
capture-changing substitution are errors. `def` is frontend sugar only, so
verify/prove/scenarios/Monitor behave exactly as for the hand-expanded
expression. Put the human-facing requirement tag on the surrounding invariant
or action; no compiler-generated predicate name appears in diagnostics.

Business/requirements dialects also have type-kinds whose finite bounds live in
a sibling top-level `verify` block instead of inline ranges:

```fsl
business <Name> {
  entity <Entity>                          // identity sort; size set by verify.instances
  control <ID> "<text>"                    // optional governance/control metadata
  policy <ID> "<text>" satisfies <ID> ...  // optional control traceability
}
requirements <Name> {
  entity <Entity>                          // optional explicit identity sort
  number <Number>                          // numeric sort; range set by verify.values
  process <Entity> with f: <Number>, g: Bool = <bool>, h: <Enum> = <Member> { ... }
                                             // process also declares the entity kind; Bool/enum
                                             // carried fields require an explicit `= ...` initializer
}
verify {
  instances <Entity> = <N>
  values <Number> = <lo>..<hi>
}
```

Optional governance catalog (metadata and cross-business checks; no new kernel
semantics):

```fsl
governance <Name> {
  authority <Actor> owns <ControlID>, ...
  control <ControlID> "<text>" [owner <Actor>] [severity <Name>] [applies_to <Entity>]...
  delegates <BusinessName> from "<business.fsl>" {
    require <ControlID>
    <ControlID> is satisfied_by policy <PolicyID>, goal <GoalID>
  }
  preservation <Name> {
    before <AsIsBusiness> from "<asis.fsl>"
    after  <ToBeBusiness> from "<tobe.fsl>"
    preserve <ControlID>
    checked_by refinement "<mapping.fsl>"
  }
}
```

Database / multi-environment compatibility dialect (expands to the same kernel
for DB lifecycle checks and reports stable fsl-db findings for the dialect layer):

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

`dbsystem` checks migration compatibility across DB schema, artifacts, API/offline
payloads, and environments. Feature flags are finite declared variants inside an
environment and may gate artifact windows with `when flag name=value`; success
then reports `DB-ASSUME-FINITE-FLAG-STATE`. It does not model DB-engine
locks/optimizers, probability, wall-clock TTL, or full production-data
completeness. Schema ranges are finite reachable rollout snapshots; percentages,
flag rollout, and offline TTLs must be modeled as finite coexistence
windows/ticks. Generic `requires` / `provides` capabilities place AI
model/prompt/retriever, tool schema, output schema, mobile/server, and other
artifact profiles into the same snapshot model; missing providers report
`required_capability_missing` under `artifact_capabilities_provided`. Use
`fslc db check` for stable fsl-db findings
(`verified_under_assumptions` on success). Use `fslc db observe` for runtime
evidence only (`observed_mismatch`, not formal violation) and `fslc db import`
for SQL DDL or minimal Prisma schema importers. Production-data preservation and
DB-engine evidence use JSON schemas under `schemas/fslc/db/` with
`formal_result: "not_run"`, not `verified`/`proved`.

Functional DDD / async effect dialect (v0; expands to the same kernel and
reports stable fsl-domain findings):

Use `enum Name { Member, ... }` for finite domain variants and
`type Name = lo..hi` for bounded numeric ranges. The legacy
`type Name = A | B` spelling is accepted by the current 2.x edition with the
stable `deprecated_domain_enum_union` warning and a canonical replacement.
Pass `--edition next` to `check`, `verify`, or `domain check` to reject legacy
enum unions. Use `fslc lint <path>... --edition next` for stable non-mutating
edition diagnostics and `fslc migrate <path>... --edition next` to review
machine edits; add `--write` only after reviewing the complete validated set.

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
    evolve OrderApproved { status = Approved }
    evolve PaymentCaptureRequested { }
    evolve PaymentCaptured { }
    evolve PaymentFailed { }
    evolve PaymentCaptureTimedOut { }
    invariant noLateApprove { status == Cancelled -> not can(ApproveOrder) }
  }

  effect CapturePayment {
    async
    irreversible
    idempotency_key Order.id
    correlation_id PaymentCaptureRequested.payment_request_id
    handles PaymentCaptureRequested
    emits one_of [PaymentCaptured, PaymentFailed, PaymentCaptureTimedOut]
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

`domain` models aggregate ownership, command intent, accepted events, domain
errors, pure `decide`/`evolve`, async effect lifecycles, and saga/process-manager
coordination. It lowers to kernel actions/state/invariants plus finite effect
status/attempt maps. Domain enum members are namespaced during lowering, so
separate enums may reuse words like `Pending`. Domain expressions may use `X in
[A, B]` and `can(Command)`. Rust resolves these constructs structurally: bare
enum members use the expected field type, membership becomes a finite equality
disjunction, and `can()` expands the selected current-aggregate command's
preconditions. Unknown/ambiguous symbols and type mismatches point to the
original domain expression. Use `fslc domain check` for
`verified_under_assumptions` plus fsl-domain
findings, `fslc domain expand` to inspect the generated kernel, and
`fslc domain generate --target typescript|python|kotlin|swift|rust` /
`fslc domain testgen` for Functional DDD and adapter scaffolds. Use
`fslc domain replay --logs` for runtime command/event/effect evidence
(`conformance_checked` / `nonconformant`, not proof). Saga history adds
`DOMAIN-ASSUME-SAGA-OBSERVED-HISTORY`. The v0 implementation does not prove
real gateway behavior, queue delivery, wall-clock timeouts, or production
exactly-once semantics.
Native domain generation is grounded in Public Kernel v1. A closed
`domain-scaffold-metadata.v1` companion retains source grouping/spelling that
lowering cannot publish. Versions, dialect, duplicate Kernel members, and
missing lowered type/state/action counterparts fail closed; source expressions
and effect/saga topology are authoritative in the companion because v1 has no
equivalent nodes. Emitters never receive `DomainSpec`, never reparse source
text, and the five targets preserve their pre-migration bytes.

The Rust frontend keeps an internal origin chain across direct domain lowering,
checked-model construction, verification, counterexamples, and `explain`.
Diagnostics prefer the original domain declaration/expression, expose generated
Kernel names only as machine detail, preserve primary/secondary origins and
expansion steps (`can()`, membership, legacy operators), and represent
source-less nodes as generated-only. Requirement tags are a separate
traceability relation, not origin identities. Public Kernel v1 remains
byte-compatible and does not expose the internal chain; publication belongs to
v2 (#256).

Omitted domain aggregate initializers retain the current Bool `false`, enum
first-member, range lower-bound, or external-placeholder `0` behavior while
emitting `implicit_initial_value`. The warning carries the selected value,
reason, edition severities, source span, and a machine-applicable insertion.

AI hard-contract dialect (expands to the same kernel for deterministic
tool-boundary checks and reports stable fsl-ai findings for runtime replay):

```fsl
ai_component <Name> {
  model <model_id>;
  prompt <prompt_id>;
  retriever <retriever_id>;              // optional, at most once
  temperature <number>;                  // optional, at most once
  input <InputSchema>;
  output <OutputSchema>;

  tools [<BareToolName>, ...];           // shorthand: declares tools with no schema/precondition/effect

  tool <ToolName> [irreversible] {
    schema <ToolSchema>;                 // at most once
    precondition <symbolic_business_precondition>;  // repeatable, 0 or more
    effect <EffectName>;                 // optional, at most once
  }

  authority {                            // an optional NAME after `authority` is accepted and ignored here
    may_suggest <ToolName>, ...;
    may_execute <ToolName>, ...;
    requires_human_approval <ToolName>, ...;
    forbidden <ToolName>, ...;
  }

  fallback {
    when <condition_name> require <safe_target>;
  }

  check hard {                           // optional, at most once; omit for the default (all 5 rules)
    rule <RuleName>;                     // tool_authority | human_approval_required | forbidden_tool_blocked
                                          // | tool_schema_declared | tool_precondition_declared
  }
}
```

`ai_component` checks tool authority, human approval before irreversible or
approval-required execution, forbidden tools, declared tool schemas, symbolic
business precondition evidence, and fallback routing. It does not model LLM
truth, groundedness, evaluator judgment, probability, confidence intervals, or
prompt/model sampling distributions in the kernel. No field on `ai_component`,
`tool`, `authority`, `fallback`, or `check` accepts a `"description text"` /
`"ID: text"` tag — unlike the kernel/business/requirements declaration-tag
convention (§10), every field here is a bare identifier or number.
`check hard { rule <Name>; ... }` selects which of the 5 named rules above get
an explicit, separately-reported invariant/finding in `fslc ai check`; an
unlisted name is a check-time error (`kind:"semantics"`, hint lists the 5).
Omitting the block checks all 5 (the safe default). Verified nuance: today
only `forbidden_tool_blocked`/`human_approval_required` change what the kernel
expansion generates (dropping the block drops one *redundant, explicit*
certifying invariant — the underlying structural guards, no execute-action for
a forbidden tool and the `requires human_approved` clause on an
approval-required tool's execute action, are generated unconditionally
either way); `tool_authority`/`tool_schema_declared`/`tool_precondition_declared`
are checked unconditionally regardless of this block. Use `fslc ai check` for
`verified_under_assumptions` hard-contract findings and `fslc ai replay --logs`
for JSONL runtime evidence (`replay_conformant` / `replay_nonconformant`,
`formal_result:"not_run"`). Statistical quality evidence uses the external
stochastic checker: `fslc ai eval` over precomputed eval JSONL,
Bernoulli/proportion metrics, Wilson intervals, and
`formal_result:"not_run"`. `fslc ai regress` checks aggregate
`ai_migration.no_regression`, `fslc ai compare` reports metric deltas,
`fslc ai drift` checks runtime telemetry thresholds/drift, and
`fslc ai compat` emits DB artifact capability profiles. These results are never
formal proof.

Recursive fsl-ai `agent` composition is checked structurally by
`fslc ai check` and returns `agent_analyzed` on success:

```fsl
agent <Parent> {
  model <model_id>;                      // optional at any agent level (root or child), at most once
  prompt <prompt_id>;                    // optional at any agent level, at most once
  context [<ContextName>, ...];
  tools [<ToolName>, ...];
  tool <ToolName> [irreversible] { schema <ToolSchema>; }  // a detailed `tool` block also works here
  authority { may_execute [<ToolName>, ...]; }
  review_gate <Child>;                   // Child must be a direct child agent (see below)

  agent <Child> {
    trust medium;                        // free NAME; only "low" has a distinct check today
    grant authority [<ToolName>, ...];
    grant context [<ContextName>, ...];
    tools [<ToolName>, ...];
    authority { may_execute [<ToolName>, ...]; }
    contract { hard { rule <Name>; } }   // parsed and echoed in agent_ir; not yet cross-checked
    output <OutputName> visibility [parent, <SiblingAgent>];  // or bare `visibility parent;` for one name
  }

  orchestration {
    <Child> -> <OtherChild>;
  }

  failure_policy {
    when <Child>.failed -> retry up_to 2;
    when <Child>.failed_after_retry -> <ParentState>;
  }
}
```

Nested agents are ordinary scoped agents (`Parent.Child`), not a separate
`sub_agent` type. Nesting defines lexical scope and grant boundaries only;
runtime collaboration is the separate `orchestration` graph. Parent authority
and context are never implicitly inherited: child `grant authority` and
`grant context` must stay inside the immediate parent boundary.
`review_gate <Child>;` declares that any orchestration path reaching a
descendant with high-authority tools must pass through one of the named
review-gate children; a path that skips them all is the "review-gate bypass"
finding below. `trust` is a free identifier, not a validated enum — only the
literal `low` currently triggers a distinct check
(`low_trust_agent_path_to_high_authority_tool`); other values (`medium`,
`high`, or anything else) parse but have no dedicated check yet.
`contract { hard { rule <Name>; } }` is parsed and listed under each agent's
`agent_ir.contracts`, but — unlike `ai_component`'s `check hard { }` — its
rule names are not validated against a known set and are not yet cross-checked
against anything; treat it as forward-declared metadata. As with
`ai_component`, no field here accepts a `"description text"` tag. Structural
findings use `guarantee_kind:"agent_structural"` and cover child grant
exceedance, low-trust paths to high-authority tools, irreversible tools without
human approval, review-gate bypass, and sibling visibility leaks. This is not
formal proof and does not model LLM truth or statistical/evaluator quality.

Stochastic / migration / drift evidence declarations (project-level fsl-ai;
dialect tag `fsl-ai-project.v0`). These blocks are read by a deliberately
lenient separate parser, not the kernel Lark grammar; they may sit alongside
`ai_component` in one file, and `fslc ai check` (or `fslc check`) on such a
file returns `ai_project_analyzed` — a declaration listing, not verification:

```fsl
dataset <Name> {
  source "<path/to/eval.jsonl>"
  population {
    <field> in ["<a>", "<b>"]
  }
  slice <SliceName> {
    <field> == "<a>"
  }
}

evaluator <Name> {
  input <name>: <Type>
  output <name>: <Type>
  calibration {
    dataset <GoldLabelDataset>
    require agreement_with_human >= 0.90
  }
}

statistical_property <Name> {
  target <AiComponentName>
  dataset <DatasetName>
  evaluator <EvaluatorName>
  confidence 0.95
  require ci_lower(metric.<metric>, 0.95) >= <T>   // or ci_upper(metric.<m>, 0.95) <= <T>
  slice <SliceName> {
    require min_samples >= <N>
    require ci_lower(metric.<metric>, 0.95) >= <T>
  }
}

ai_migration <Name> {
  from <Component> {
    model <id>
    prompt <id>
    retriever <id>
  }
  to <Component> {
    model <id>
    prompt <id>
    retriever <id>
  }
  preserve {
    hard_contract <Contract>.hard
    no_regression {
      dataset <DatasetName>
      metric <metric> drop <= 0.05
      metric <metric> increase <= 0.02
    }
  }
}

observed_property <Name> {
  target <AiComponentName>
  source production_logs
  window last_7_days
  require observed(metric.<metric>) <= <T>
  require drift(metric.<metric>) <= <T> compared_to previous_7_days
}
```

`require` clauses here are threshold labels for external evidence jobs, not
kernel formulas — they add no probability semantics to `fslc verify`.
`failure_mode <Name> { condition ...; severity ...; }` is parsed and listed by
name under `ai_project_analyzed`'s `failure_modes`, but no command yet checks
its content against evidence — it is tracked metadata, not a verified claim.
**`ai_action`, `retriever` (as a standalone block), `trust_boundary`, and a
top-level named `authority { target ... }` are recognized only as block
*boundaries*: the parser does not descend into their body at all, so any text
inside — even garbage — passes `check`. They are echoed as bare `{kind, name}`
entries under `raw_blocks`, not validated.** Do not author one expecting it to
constrain anything; the checked surface is exactly `ai_component`/`agent`
(hard contract, kernel-backed) plus `dataset`/`evaluator`/
`statistical_property`/`ai_migration`/`observed_property` (external evidence,
above). Commands: `fslc ai eval`
checks a `statistical_property` by Wilson bound over precomputed eval JSONL
(the `dataset` `source` file, or `--records`); `fslc ai regress` checks
aggregate `ai_migration.no_regression` metric deltas between
`--before-records`/`--after-records`; `fslc ai compare` reports metric deltas
with no threshold claim; `fslc ai drift` checks `observed_property` thresholds
and drift over runtime telemetry (`observed_supported` / `observed_mismatch`);
`fslc ai compat` emits a `dbsystem` `artifact` capability profile, which
`fslc compat check --include-ai` folds into a dbsystem compatibility check.
Hard boundary: every result carries `formal_result:"not_run"` and must never
be displayed as `proved`/`verified`; a point-estimate-only requirement
(`require accuracy >= 0.92` with no `ci_lower`/`ci_upper`) is rejected at eval
time (`inconclusive`, exit 1), not warned past. Eval statuses are
`dataset_invalid`, `evaluator_untrusted`, `insufficient_samples`,
`inconclusive`, `statistically_unsupported`, `statistically_supported`; the
priority order and the eval-record JSONL schema live in
`docs/DESIGN-stochastic.md`.

Composite spec (a separate top-level form):

```fsl
compose <Name> {
  use <SpecName> as <alias> from "<relative path>"   // multiple allowed. nested compose not allowed
  state { ... }  init { ... }                    // additional state on the composite side (optional)
  action <n>(<p>: <alias>.<Type>, ...) =
      <a>.<act>(<expr>...) [ || <b>.<act2>(<expr>...) ] {  // synchronize (run atomically together)
    [requires <expr>]... [<stmt>...]             // extra guards / assignments to composite-side state
  }
  internal <alias>.<action>                      // forbid standalone firing (only via synchronization)
  invariant/trans/reachable/leadsTo ...          // cross-reference via alias.var
}
```

Compose synchronization does **not** inherit `fair` from component actions. If a
fair constituent action is synchronized into a non-fair composite action, the
result JSON `warnings` includes `kind: "fair_not_inherited"` naming the
composite action and fair constituent(s). Use `fair action <name>(...) = ...`
when the synchronized action itself must be fair.

Compose synchronized arguments are **structural by bounded value range**, not
nominal by type name. Passing `core.TaskId` to an action parameter declared
`NoteId` is intended when both domains cover the same values: a repro with
`TaskId = 0..2`, `NoteId = 0..2`, and
`action sync(t: core.TaskId) = core.choose(t) || note.attach(t) { }` returned
`ok` from `fslc check` and `verified` from `fslc verify --depth 1`. If the target
is narrower (`NoteId = 0..1`), `check` still returns `ok`, but verification can
fail with `violated/type_bound` on the target component's `_bounds_...`
invariant (`sync(t: 2)` in the repro). Idiom: use same-range component-local
domain types for shared IDs; add a sync-action `requires` guard when passing to
a narrower domain.

refinement mapping (the third file; `fslc refine impl.fsl abs.fsl this.fsl`):

```fsl
refinement <Name> {
  impl <ImplSpecName>
  abs  <AbsSpecName>
  maps auto                                      // optional identity defaults for same-named compatible state/actions
  map <abs_var> = <expr over impl state>          // scalar abstract variable
  map <abs_var>[<x>: <KeyType>] = <expr>          // per-element mapping of a Map
  // map and action arguments use the same expressions as specs, including if <c> then <a> else <b>
  action <impl_act>(<formal params>...) -> <abs_act>(<expr>...) | stutter
  // formal params may be bare names or name: Type annotations matching the impl action
  // explicit map/action entries override maps auto; incompatible same-name candidates are type errors
  preserve progress {                            // optional, only when upper leadsTo must be preserved
    respond <AbsLeadsTo> by <impl_act>, ...
  }
}
```

Standalone action items, inline `implements` items, requirement-action `maps`,
and auto/identity synthesis share one typed action-correspondence validator.
Typed impl parameters, target arity/argument expressions, and auto actor
compatibility are checked identically. Duplicate diagnostics identify both
origin kinds and line/column sites; auto synthesis never replaces an explicit
entry.

Give impl and abs distinct enum/struct type names. Refinement merges type
metadata by name; a same-named enum/struct with a different member list/field
set on each side is rejected as `kind: "type"` (exit 2) rather than silently
merged (merging would let an impl-only member get reinterpreted as whichever
abs member sits at the same ordinal index). Same-named domain types
(`lo..hi`) with different bounds are fine — an out-of-range value there is
still caught as `map_out_of_bounds`/`abs_state_mismatch`.

## 2. Types

| Type | How to write | Notes |
|---|---|---|
| Int / Bool | `n: Int` | Int is unbounded |
| Domain type | `type Qty = 0..5` | **automatic bound check** (violated/type_bound) |
| Inline state domain | `state { qty: 0..5 }` | Shorthand for a named domain type in a state-variable declaration |
| Inline state initializer | `state { qty: Qty = 0 }` | Deterministic sugar for the equivalent root assignment in `init`; may not read state |
| symmetric domain | `symmetric type TaskId = 0..2` | Same as a domain type, plus liveness symmetry reduction |
| entity kind (dialects) | `entity Claim` / `process Claim ...` | Finite identity sort for business/requirements; bound by `verify { instances Claim = N }` |
| number kind (dialects) | `number Amount` | Finite numeric sort for business/requirements; bound by `verify { values Amount = lo..hi }` |
| enum | `enum St { A, B }` | members are referenced and displayed by bare name |
| symmetric enum | `symmetric enum Worker { A, B }` | Same as enum, plus liveness symmetry reduction |
| struct | `struct S { f: Qty, o: Option<K> }` | field = scalar or Option<scalar> only |
| Option<T> | `c: Option<ItemId>` | T is a scalar. `none` / `some(e)` |
| Map<K, V> | `m: Map<ItemId, Qty>` | K is a bounded scalar (Int keys give a deprecation warning) |
| Set<T> | `s: Set<OrderId>` | T is a bounded scalar |
| Seq<T, N> | `q: Seq<JobId, CAP>` | T is a scalar, N is a positive constant. FIFO |
| relation A -> B | `r: relation User -> Role` | Binary relation over bounded scalar endpoints |

Scalar = Int / Bool / domain type / enum. In a `state` declaration,
`x: lo..hi` is an anonymous domain type and is equivalent to declaring
`type X = lo..hi` and writing `x: X`.
**State-variable whitelist**: scalar | Option<scalar> | struct |
Map<bounded scalar, scalar|Option|struct> | Set<bounded scalar> | Seq<scalar, N> |
relation bounded-scalar -> bounded-scalar.
Anything else (nested structs, Set/Map/Seq as a Map value, etc.) is rejected by
check as a type error.

Kernel `state` fields may carry deterministic inline initializers. They normalize
to ordinary root assignments before checking and therefore share Monitor/BMC/
induction/explicit/Public-Kernel semantics with `init`. Constants, enum members,
constructors, `none`, and deterministic collection literals are allowed. State
reads, references to another initializer, indexed/field targets, statement `if`,
`forall`, and bulk/relational initialization remain invalid inline. The same root
cannot be assigned both inline and in `init`.

## 3. Expression catalog

- Arithmetic: `+ - * / %`, unary `-`, `min(a,b)` `max(a,b)` `abs(a)`
  (in `a//b` everything after `//` becomes a comment, so write division with a
  space: `a / b`)
- Comparison: `== != < <= > >=` / logic: `and or not =>`
- Finite binders: `x: T`, `x in lo..hi`, or `x in set_or_seq`, each with an
  optional `where BoolExpr`. Maps and unbounded domains are rejected.
- Quantification: canonical `forall binder { expr }` / `exists binder { expr }`.
  The 2.x colon/no-braces spelling remains accepted as non-canonical input.
- Aggregation: `count(binder)` and `sum(binder of value)`, including collection
  and range binders. Empty domains yield `0`; Seq duplicates count once per live
  slot and Set members once per distinct value.
- Cardinality predicates: `unique(binder)` / `exactlyOne(binder)`. `unique`
  means at most one match; `exactlyOne` means exactly one.
- Option: `x == none` `x != none` `x == some(e)` `x != some(e)` use structural
  equality (presence first, then payload when present). `x is some(v)` is still
  required when `v` must be bound for the rest of the formula; equality creates
  no binding. The binding is scoped to the logical continuation where the match
  is true, such as the guarded RHS of `=>` or `and`, and is not global.
  Arithmetic and ordering on Option are type errors.
- struct: literal `S { f: 0, o: none }`, `s.f`, `==` (field-wise equality; for an
  Option field, presence matches ∧ present ⇒ values match)
- Set: `Set {}` `Set { 1, 2 }`, `.add(e) .remove(e) .contains(e) .size()`
- Seq: `Seq {}` `Seq { 1, 2 }` (element count ≤ N), `.push(e) .pop() .head() .at(i)
  .contains(e) .size()`, `==` (length + all elements)
- Relation: `.contains(a,b) .add(a,b) .remove(a,b)`,
  `reachable(r,a,b) acyclic(r) functional(r) injective(r) domain(r) range(r)`.
  `reachable`/`acyclic` require a self-relation (`relation T -> T`).
- conditional expression: `if c then a else b` in any expression position;
  `c` is Bool, both branches have one logical type and are checked statically,
  while only the selected branch is evaluated
- ensures/trans only: `old(expr)` / leadsTo only: `P ~> Q`,
  `P ~> within K Q`, plus optional `decreases <int expr>` for induction ranking

## 4. Statements (init / action body)

- Assignment: `x = e`, `m[k] = e`, `m[k].f = e`, `o.f = e`, `o.f = some(e)`
- Set/Seq/relation are re-assigned: `s = s.add(x)`, `q = q.pop().push(y)`,
  `r = r.add(a,b)` (chaining allowed)
- `if expr { stmt... } [else { stmt... }]` is allowed in both `init` and action
  bodies (may nest with an if inside else)
- `forall x: T { stmt... }` (bulk assignment)

## 5. Semantic rules

1. One step = one action instance (name × parameters) executes atomically.
2. **Simultaneous assignment**: every RHS in the body reads the old state.
   Unassigned variables are unchanged (automatic framing).
3. **Double assignment = semantics error**: assigning twice to the same
   variable/field on the same path. then/else are separate paths (assigning in both
   is allowed). Assigning to the same variable **after an if** as inside a branch is
   also an error.
   For `Map<K, Struct>` values, the path includes the field: `m[k].f1 = ...`
   and `m[k].f2 = ...` in one action are allowed independent field writes
   (`check` and `verify --depth 1` succeed in the repro). Repeating the same
   field, e.g. `m[k].f1 = 1; m[k].f1 = 2`, is rejected while building the
   checked Kernel model. Indexed writes are rejected unless their indices are
   provably distinct constants; `requires k != j` and local constant bindings
   do not establish distinctness. Native `check`/`verify` and the browser Worker all
   return `kind:"semantics"` before a verifier backend runs.

   ```fsl
   struct Pair { f1: V, f2: V }
   state { m: Map<K, Pair> }
   action update(k: K) { m[k].f1 = 1  m[k].f2 = 2 }
   ```
4. enabled when all requires hold. ensures is checked after the transition.
5. For Seq `pop/head/at` and a nonzero divisor of `/` `%`, **well-definedness is
   checked automatically** in action context (partial_op). A requires guard or an if
   guard both work (path conditions are considered). An out-of-range at() inside an
   invariant/reachable is an undefined value — always guard with `i < q.size() =>`.
6. `fair` = weak fairness: an infinite execution in which a fair instance that is
   enabled throughout the loop is never executed is excluded from leadsTo
   counterexamples. Fairness applies to whole action instances; model conditional
   fairness by splitting the condition into a separately guarded `fair action`.
   Removing `fair` is not a useful negative probe in a structurally terminating
   machine; the probe must admit a lasso, deadlock, or pending stall.
7. `leadsTo ... decreases M` under `verify --engine induction` proves an
   unbounded response when, under the proved invariants and while P holds and Q is
   false, M is non-negative and the ranked progress discipline holds. Without
   `helpful`, every enabled action must either make Q true or keep P true while
   strictly decreasing M. With `helpful act(args...)`, only the matching helpful
   action instance must strictly decrease M when it fires; unrelated actions only
   need to preserve the pending obligation unless they make Q true. `helpful`
   does **not** create fairness: the matching action must still be declared
   `fair action` and be enabled whenever the obligation is pending. Without
   `decreases`, leadsTo remains bounded to `--depth`.
   - **Placement**: `decreases` is a sibling of the forall wrapper, *outside*
     its braces — `leadsTo L { forall c: Case { P ~> Q } decreases M }`.
     Nesting it inside the forall body is a **parse error**
     (`fslc` reports `unexpected 'decreases' here` with a placement hint),
     not "ranking doesn't work under forall".
   - **Per-entity measure under interleaving**: use
     `helpful step(c) decreases level[c]`. Without `helpful`, an action advancing
     a different entity still reports `rank_failure:"non_decreasing_action"`.
     With `helpful`, diagnostics include `progress_action_not_fair`,
     `helpful_action_not_enabled`, `non_decreasing_helpful_action`,
     `pending_not_preserved`, and (two or more distinct helpful actions)
     `helpful_action_enabledness_not_sticky` — each helpful instance's
     enabledness must not flicker (once enabled while pending, it must stay
     enabled until it fires or Q holds), otherwise none is ever
     *continuously* enabled and weak fairness never obligates it to run even
     though "some" helpful match is always enabled — and
     `non_helpful_action_increases_measure`: a non-helpful action may
     preserve the pending obligation without decreasing the measure, but not
     increase it, or an unbounded pump could outpace the helpful action's
     guaranteed decrease and Q would never be reached.
   - **Global sum idiom**: `decreases sum(k: Case of level[k])` is still the
     simplest instances-count-independent measure when every enabled action
     decreases the total; works with `--instances` overrides too.
8. `symmetric type` / `symmetric enum` means those values are interchangeable
   entity identities. For leadsTo lasso/stall search, fslc symmetry-breaks the
   representative state using canonical rows from `Map<SymmetricType, V>` and
   `Set<SymmetricType>` state (`V` is used only when it contains no symmetric
   identity type); use it only when no identity is semantically special.

## 6. Automatic checks (checked even if not written)

Type bounds (`_bounds_<var>`, including Map values, struct fields, and the Seq live
prefix) / partial operations (`_partial_<action>`, Seq pop/head/at and nonzero
divisor) / action coverage (+ unsat-core diagnostics) / deadlock (warning, with
state, `deadlock reachable at step N (state: …)`, violated under
`--deadlock error`) / leadsTo (lasso + stall).
An **intended terminal state** (processing complete, etc. — a state where stopping
is correct) is declared with `terminal { <predicate> }` — a stop satisfying the
predicate is excluded from the deadlock check, while other unexpected deadlocks
continue to be detected (more precise than `--deadlock ignore`, which uniformly
ignores all stops). vacuity is a warning only on the verified/proved path:
an unreached antecedent of an implication invariant (`vacuous_implication`), an
unreached leadsTo trigger (`vacuous_leadsto`), a requires clause always true under
the context of the preceding requires (`always_true_requires` — actions with
coverage false and compose synchronized actions are excluded; a synchronized
action's clauses are inherited copies from its components and are checked by
verifying the component spec on its own), and **an invariant that depends only on a
frozen state variable no action ever assigns to and is dynamically always true**
(`tautology_over_frozen` — a dead ghost; make it `const`, or suspect a missing
action that should change it), and a generated deadline `tick` proven dead because
urgency freezes time (`urgency_freeze`). `--vacuity error` gives
`result:"error"`; `--vacuity ignore` disables it.

## 7. CLI and JSON essentials

```
fslc check <f>                                  # syntax / names / types only; f = .fsl or .md (literate)
fslc lint <path>... [--edition current|next] [--project fsl-project.toml] # edition + ID-policy findings; never mutates
fslc migrate <path>... --edition next [--write] # dry run by default; atomic validated write set
fslc fmt <f|-> [--edition current|next]         # canonical source on stdout; input is never mutated
fslc fmt <path>... --check                      # JSON; exit 0 clean, 1 changed, 2 error
fslc kernel <f> [--kernel-version 1|2]          # normalized typed Kernel JSON (default v1)
fslc conformance <f> [--depth K=4] [--kernel-version 1|2] # matching vectors (default v1)
fslc verify <f> [--depth K=8] [--engine bmc|induction|explicit|auto] [--k N=1]
               [--explicit-budget N=1000000]        # explicit/auto; max visited states
               [--deadlock warn|error|ignore] [--vacuity warn|error|ignore]
               [--property <Name>]                  # check one named property in isolation
                                                    #   (invariant / trans / leadsTo / reachable)
               [--exclude-property <Name>]...       # skip named invariant/trans/leadsTo/reachable
               [--instances NAME=N]...              # override verify-block `instances NAME = N`
               [--values NAME=LO..HI]...            # override verify-block `values NAME = LO..HI`
               [--from-state state.json]            # complete Monitor/replay state; replaces init (BMC only)
               [--strict-tags] [--requirements ids.txt] [--no-cache]
               [--lemma "<expr>"]...                 # induction only; independently adjudicated
fslc sweep <f> --instances NAME=LO..HI --depth LO..HI [--property Name]
                                                     # grid of verify runs; JSON sweep.results/minimal_counterexample
fslc explain <f> [--depth K=8] [--readable]    # JSON by default; --readable emits a text review view
fslc mutate <f> [--depth K=8] [--by-requirement] [--max-mutants N=200]
              [--from mutants.jsonl]
fslc scenarios <f> [--depth K]                  # reach_* / cover_* / respond_* / deadlock_terminal
fslc replay <f> --trace <events.json>           # conformant | nonconformant
fslc replay <f> --from-log <events.jsonl> --mapping <mapping.fsl>
                                                # production JSONL -> mapped action/state -> Monitor
fslc testgen <f> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [-o out]  # Adapter skeleton + conformance tests (pytest default / Vitest / Swift Testing / kotlin.test / package:test / PHPUnit)
fslc refine <impl> <abs> <mapping> [--depth K]  # refines | refinement_failed
fslc diff <old> <new> [--depth K] [--mapping <mapping>]
          [--forbid behavior_added,invariant_weakened,forbidden_relaxed]
                                                  # bounded semantic change report
fslc diff --git BASE..HEAD [spec.fsl] [--depth K]
                                                  # materialize both full revision trees; omit spec for changed .fsl batch
fslc chain [fsl-project.toml] [--keep-going]     # manifest-driven business -> req -> design -> impl table + JSON
fslc analyze <file-or-dir>... [--projection tsg|action_state_graph|action_dependency_graph|code_audit|impact_graph|requirement_property_graph|property_state_graph|refinement_graph|traceability_graph] [--code FILE_OR_DIR] [--focus NODE] [--profile ai-review] [--export tag-review] [--format json|dot|mermaid]  # structural/tag/code review
fslc typestate <f> [--ts]                       # state machine -> ghost-type applicability + TS skeleton
fslc html <f> [--depth K] [-o report.html] [--engine bmc|induction]  # self-contained HTML review report (dev audience)
fslc ledger <f> [--depth K] [--impl-log run.json] [-o ledger.md] [--engine bmc|induction] [--evidence result.json]... [--approval record.json]...
                                                        # business audit ledger by requirement id (PM/audit)
fslc document generate <f> [--view requirements] [--lang ja|en] [--strict] [--strict-rendering]
               [--glossary glossary.json] [--evidence evidence.json]... [--approval record.json]... [--trust-key public.pem]... [-o requirements.md]
                                                        # deterministic ja/en requirements document from RCIR (Requirement Claim IR);
                                                        # --glossary applies presentation-only display labels (FSL-DOC-LABEL-UNKNOWN/-CONFLICT);
                                                        # --evidence overlays a per-requirement assurance class (proved/bounded/
                                                        # replay-observed/statistical/not_run), same envelope shape as `fslc ledger --evidence`;
                                                        # --approval displays a verified requirements_document approval record, failing
                                                        # closed (FSL-DOC-APPROVAL-DRIFTED) if it does not match the current rendering;
                                                        # only spec/requirements dialects project (others: FSL-DOC-DIALECT-UNSUPPORTED)
fslc document claims <f> [--view requirements] [-o requirements.claims.json]
                                                        # emit the RCIR claim set as JSON; agents/tools consume this instead of re-parsing .fsl
fslc document check <f> <document.md> [--glossary glossary.json] [--evidence evidence.json]... [--approval record.json]...
                                                        # structural drift check: generated claim blocks vs a fresh re-render;
                                                        # document_conformant (0) | document_drifted (1); never interprets prose
fslc approval create <f> --kind ledger|html|scenarios|requirements_document --artifact <reviewed> --approver <name>
               [--requirement ID]... [--glossary glossary.json] [--evidence evidence.json]... [-o record.json]
                                                        # bind the reviewed artifact to normalized spec + Git baseline;
                                                        # requirements_document records schema v3/v4 with a claim_set_digest
fslc approval check <f> --record <record.json>          # approved | drifted with machine reasons
fslc approval diff <f> --record <record.json> [--depth K]
                                                        # semantic diff from approved commit to current working spec
fslc domain check <f> [--depth K] [--engine bmc|induction]  # Functional DDD / effect findings
fslc domain analyze <f>                                      # aggregate/effect ownership summary
fslc domain expand <f> [-o out.fsl]                          # generated kernel FSL
fslc domain generate <f> --target typescript|python|kotlin|swift|rust [-o dir] # Functional DDD scaffold
fslc domain testgen <f> [--target vitest] [-o out]           # adapter/conformance scaffold
fslc domain replay <f> --logs events.jsonl                  # runtime command/event/effect evidence
fslc db check <f> [--depth K] [--engine bmc|induction]  # dbsystem compatibility findings
fslc db observe <f> --trace events.json                 # runtime observation evidence
fslc db import <sql|schema.prisma> [--source auto|sql|prisma] [--name Name] [-o out.fsl]
                                                        # SQL DDL / minimal Prisma -> dbsystem
fslc ai check <f> [--depth K] [--engine bmc|induction]  # ai_component hard-contract findings
fslc ai replay <f> --logs events.jsonl                  # AI runtime replay evidence, not proof
fslc ai eval <f> [--records <path>] [--dataset <Name>] [--slice <Name>] [--property <Name>]
                                                        # Wilson-bound check over precomputed eval JSONL
fslc ai regress <f> [--migration <Name>] --before-records <p> --after-records <p> [--dataset <Name>]
                                                        # ai_migration.no_regression metric drop/increase check
fslc ai compare --from <records> --to <records> [--from-label L] [--to-label L] [--dataset <Name>]
                                                        # metric deltas between two eval JSONL files, no threshold claim
fslc ai drift <f> --logs events.jsonl [--baseline-logs p] [--window N] [--baseline p] [--property <Name>]
                                                        # observed_property threshold/drift check from runtime telemetry
fslc ai compat <f> [--environment <env>]                # emit a dbsystem artifact capability profile for AI compat
fslc compat check <f> [--include-ai]                    # dbsystem compatibility check, optionally folding in AI capability profiles
```

Each `lint` path may be a file or directory. Directories recursively expand to
regular `*.fsl` files; symlink entries and other extensions are skipped while
walking them, and the combined file set is deduplicated and sorted
deterministically. Explicit file paths retain their existing extension-agnostic
behavior.

Native generated-code replay uses `replay-trace.v1`: a closed root carrying
trace and Kernel versions, exact spec identity, complete tick-0 `initial`, and
events with exact Public Kernel `action`/`params`, canonical ticks `1..N`, and
complete post-transition `state`. Trace schema 1.1 adds explicit stutter as
`action:null` plus empty params; its state must equal the unchanged Monitor
state. Equal-state stutters may be inserted/deleted, while unreported concrete
intermediates are outside invariant judgment. Optional `timestamp` is opaque
and ignored. Trace v1 accepts Kernel 1.0.0/2.0.0. Ill-shaped/incomplete input is exit 2; typed
state divergence is exit 1 with leaf mismatches. Bare arrays/`{events}` are the
unversioned action-only compatibility adapter; testgen/verifier traces are not
replay input. See `docs/DESIGN-replay-trace.md`.

Schema 1.2 opts into solver-free bounded-liveness replay. Every
`leadsTo P ~> within K Q` is observed at tick 0 and after each action/stutter;
`Q` at the inclusive deadline succeeds and absence of `Q` fails. Safety is
reported first and separately. A finite unfinished obligation is `pending`, and
unbounded `leadsTo` is listed as unchecked. Schema 1.0/1.1 stays safety-only.

Use native `fslc kernel` as the stable compiler boundary after dialect lowering
and type checking. Do not consume the frozen Python AST JSON or reparse expression
strings: every exported expression has a structural type and span, actions and
properties carry requirement/lowering origin, and partial failures declare
rollback conditions. The default and legacy Rust API remain Public Kernel v1.
Select `--kernel-version 2` only when a consumer needs the queryable provenance
graph; check its `completeness` and per-origin assurance rather than assuming v2
means every dialect is source-complete. Requirement relations remain separate
from origin targets. Use `fslc conformance` with the same major and the matching
`schemas/fslc/kernel/conformance.v{1,2}.schema.json` to test an independent runtime.
The compatibility policy and field contract are in
`docs/DESIGN-kernel-contract.md`; v2 provenance is in
`docs/DESIGN-kernel-origin-v2.md`.

For an induction `unknown_cti`, first try `--engine explicit` — if exploration
closes it returns `proved` with **no lemmas at all** (the invariant being
non-inductive is irrelevant to exhaustive search). Only when explicit is
rejected or returns `unknown_budget`, fall back to lemma hunting: pass
candidate auxiliary invariants with repeatable `--lemma "EXPR"`. fslc proves each candidate independently (original
init/actions + implicit bounds, without original user invariants), rejects
false/non-inductive/invalid candidates with their own evidence, and makes only
`proved` candidates available to the target proof. A candidate is used only
when it is false on the current CTI; `lemma_cti_exclusions` records the target,
CTI, and violated steps. On final `proved`, copy the declarations from
`auxiliary_invariant_recommendation` into the spec and review that source edit.
There is no flag for injecting an unverified assumption, and `--lemma` is an
error with the BMC engine.

`--engine explicit` enumerates the concrete state space (Z3-free BFS). It is
the fastest route on small-state-space specs and, when exploration **closes**
(no new states within `--depth`), returns `result:"proved"` with
`closure:true` — a complete, unbounded proof that needs no lemmas, including
for true-but-not-inductive invariants where induction returns `unknown_cti`.
Depth exhaustion without closure returns bounded `verified` (same strength as
BMC); exceeding `--explicit-budget` returns `unknown_budget` (exit 1) — never
a silent `verified`. Violations return the same shortest-counterexample trace
schema as BMC. Results carry `states_explored`, `max_frontier_width`, and
`depth_reached`. Fail-closed rejections (kind `semantics`, exit 2): `leadsTo`
properties, nondeterministic `init` (every state variable must be definitely
assigned), and `init forall` binder domains that reference state variables
(range bounds and collections must be compile-time constants) — use
`--engine bmc` for those specs. `--from-state`, `--lemma`, and `--k` do not
apply to this engine.

`--engine auto` tries explicit first and falls back to bmc transparently
when explicit can't decide the spec (a fail-closed rejection above, or
`unknown_budget`); everywhere else explicit's own verdict is final. Every
result carries `engine: "explicit"` or `engine: "bmc"` naming whichever
engine decided; a fallback additionally carries `engine_fallback: {from:
"explicit", reason: "...", kind: "unsupported"|"budget"}` — `kind`
distinguishes a permanent gate from one a larger `--explicit-budget` might
clear. `auto` shares its cache entries with plain `--engine explicit`/`bmc`
runs of the same spec (the cache key is always the engine that actually
decided, never `auto` itself), does not change the default engine, and is
Rust-only.

**Literate Markdown FSL.** `fslc check`, `fslc verify`, and `fslc scenarios`
accept `.md` files containing ` ```fsl ` fenced code blocks directly — no
extraction step or flag needed. Non-fsl lines are blanked in place so all
diagnostic positions point to the Markdown document's own line numbers.
Multiple fsl blocks form one compilation unit (split definitions across
sections). Files without fsl fences are rejected; non-fsl fences
(` ```python ` etc.) are ignored. A literate `.md` may `use`/compose `.fsl`
files relative to its own directory; using another `.md` as a compose target
is not supported.

`diff` uses bidirectional bounded refinement for behavior changes, implication
between the OLD/NEW user-invariant conjunctions, and replay of OLD `forbidden`
scenarios against NEW. Its stable finding kinds are `behavior_added`,
`behavior_removed`, `invariant_weakened`, `invariant_strengthened`,
`forbidden_relaxed`, `scope_changed`, and `unknown`; an empty report uses
`no_semantic_change`. A changed `verify` scope is explicit and comparison uses
NEW's shared entity/number bounds. Findings exit 0 because the command is an
analysis; use `--forbid` to turn selected kinds into an exit-1 CI gate. Every
verdict is bounded by `--depth`, and a mapping only resolves the direction
declared in its `impl`/`abs` fields (it is never inverted).

Use `verify --from-state` for bounded prediction from a current concrete state,
not for proof. The input must be the complete logical JSON emitted by
`Monitor.state`/replay (enum names, Option as value/`null`, complete Map keys,
Set/Seq arrays, relation pairs). It replaces `init`, bypasses the verdict cache,
disables symmetry reduction for concrete identities, and is rejected with the
induction engine. Results always stamp
`faithfulness.scope:"bounded_from_snapshot"`, `spec_init:"not_used"`, and
`induction:"not_applicable"`. A step-zero invariant violation is a valid
predictive result. Do not fill missing variables: partial snapshots are a
different, weaker existential query and are rejected.

For production-log replay, each non-empty JSONL line is an object with
`action`, `params`, and the observed post-action `state`. The mapping file is
parsed by the same `parse_refinement` path as `fslc refine`: `impl` names the
external log schema, `abs` names the target spec, `map` covers every target
state variable, and `action external(args) -> target(exprs)` (or `stutter`)
maps events. The Monitor executes the target action and compares its state with
the mapped observed state. This v1 requires complete observed state; missing
fields/keys are `log_mapping` nonconformance. The first divergence includes
`failed_at_record` (0-based), `log_line` (1-based), and the action/state
mismatch. Finite replay does not check `leadsTo`.

`verify` is backed by a persistent verdict cache (issue #169) keyed on every
input that can affect its output (the post-desugaring kernel AST, the raw
entry-file text, and every flag/override) plus an implementation fingerprint,
so an unchanged re-run in the same write→verify→repair loop returns instantly
instead of re-solving. A hit adds one additive field,
`"cache":{"hit":true,"key":...,"source":"exact"|"cross_depth"}`; a miss looks
exactly like today's output. `"source":"cross_depth"` means a prior
`violated` result at a shallower depth was reused, because a counterexample's
earliest step does not depend on the requested search bound. Comment/
whitespace-only edits still miss (diagnostics quote source by line number,
so entry-file text is hashed verbatim) — that is a deliberate hit-rate/
staleness trade-off, never a soundness one. `--no-cache` (or `FSLC_CACHE=off`)
opts a run out entirely. Cache writes are atomic, so running `fslc verify` on
many files as concurrent processes (e.g. `xargs -P`, a CI job matrix) is safe —
concurrent runs at worst duplicate solving, never corrupt the cache. When
verifying a whole project's specs, prefer process-level parallelism over a
sequential per-file loop. See `docs/DESIGN-incremental-verify.md`.

`analyze` is a structural observation layer, not a verifier. `--projection tsg`
emits a stable Typed Semantic Graph over requirements, actions, state variables,
properties, acceptance/forbidden scenarios, and traceability metadata.
`--projection action_state_graph`, `action_dependency_graph`,
`impact_graph --focus NODE`, `requirement_property_graph`, and
`property_state_graph` summarize deterministic components/SCCs/cycles, degree,
and metrics over that graph. It accepts multiple files/directories in batch mode;
directories expand recursively to sorted `*.fsl` files and partial failures stay
visible in the batch JSON. Standalone refinement mappings use `--projection
refinement_graph`, project manifests use `--projection traceability_graph`, and
graph projections can export DOT or Mermaid with `--format dot|mermaid`.
`--projection code_audit --code PATH` is the single-spec, JSON-only bridge from
exact executable Kernel requirement targets to `@fsl.trace` implementation
locations. Treat missing, orphan, and target-mismatch findings as review signals,
not proof. `origin_assurance` describes Public Kernel provenance
(`source_backed|generated_from_source|generated_only|unknown`), never formal
verification strength. See `docs/DESIGN-code-audit.md`.
`--profile ai-review` emits AI-readable review findings such as
`disconnected_requirement`, `unanchored_property`, `progressless_cycle`,
`unwritten_state`, `unread_state`, `unguarded_action`, and
`conservation_candidate`. It also runs a fixed depth-4 bounded semantic probe
for `divergent_choice` (two same-state enabled actions split an
invariant/acceptance outcome) and `unconstrained_effect` (an unread state can
receive different next values from two enabled actions). These add
`evidence_basis:"bounded_bmc"` (frozen v0 vocabulary for a bounded reachability
witness; the native probe is solver-free explicit-state exploration, not
symbolic BMC), a reachable witness, and `spec_question` ending
in `?`. Ask that question; do not invent which branch is intended. Bounded-witness
findings supersede duplicate `unread_state`/`unguarded_action` approximations.
No finding means only “not witnessed within depth 4,” not proof of determinism.
Treat all findings as review signals: they carry
`formal_status:"not_a_violation"` unless a future finding explicitly cites
`verify`/`refine`/`replay` evidence. Versioned schemas live under
`schemas/fslc/analysis/`.

Natural-language interpretation on top of `analyze` is agent-side only. The core
analyzer must not infer semantics from English, Japanese, or other free text.
The deterministic tag checks compare only exact code-shaped identifiers:
`tag_stale_reference` and `tag_formula_disjoint`. For meaning review, run
`fslc analyze file.fsl --export tag-review`, compare each `tag.text` with its
`formal_definition`, cite the declaration tuple, and keep conclusions marked
`formal_status:not_a_violation`; never silently rewrite intent from this export.
If an agent reviews requirement text, comments, or source excerpts together with
the TSG, it must cite the exact text and graph node ids it used, keep
`formal_status:"not_a_violation"`, and never convert that suggestion into an
`fslc` violation, proof result, or CI failure. Non-English text should be handled
by the agent's language capability or a user-approved reviewer, not by hard-coded
keywords in this repository. External model calls are an agent privacy decision:
do not send source text, requirement text, comments, or analysis JSON outside the
local environment unless the user or execution environment has explicitly opted
in.

`ledger` (issue #24) re-organizes `verify`/`scenarios`/`replay` findings **by
requirement id** into a Markdown audit ledger a PM / governance / internal-audit
reader can decide approve/reject/risk-accept from. It is a presentation layer
(no new verification): the `trace_type` discriminator drives a per-finding
business translation, governance columns (risk/decider) come from `control`
metadata when present (fill-in otherwise), and the guarantee limit is stated in
positive form. Raw JSON is demoted to a collapsed appendix. See
`docs/DESIGN-ledger.md`.

Digest-bound approvals (issue #190) are separate from assurance class and from
the ledger's empty human-decision checkbox. `approval create` must be run from a
clean tracked Git baseline and only accepts a reviewed artifact that matches a
fresh rendering under the recorded inputs. The sidecar uses a lowered-kernel
digest that ignores source locations plus a normalized artifact digest for
`ledger`, `html`, or `scenarios`. `approval check` and `ledger --approval`
report `approved` or `drifted`; drift reasons distinguish spec, rendering, and
renderer changes. A drifted row carries the complete baseline digest and an
`approval diff` command, which compares the approved commit to the current
working spec. Treat `approver` as attribution; authenticity comes from the
repository's signed-commit/review/branch-protection policy. See
`docs/DESIGN-approval.md`.

Every requirement id in the ledger (and every property row in `fslc html`)
carries an **assurance class** (issue #171): `proved(induction)` (k-induction,
all depths) / `bounded(BMC depth k)` (BMC, depth k) / `replay-observed`
(concrete log/trace checked, not a universal claim) / `statistical(Wilson c%)`
(precomputed eval JSONL, aggregate not per-case) / `not_run` (no formal
evidence — structural analysis, profiles, comparisons). `--engine induction`
is required for a requirement to ever show `proved`; `--evidence
<result.json>` folds a saved fsl-ai/fsl-db/fsl-domain `formal_result:"not_run"`
producer's output (tagged via a top-level `requirements: [...]` list) into the
per-requirement classification. Class is method coverage, not verdict — a
`violated` BMC run is still `bounded`. See `docs/DESIGN-assurance-classes.md`.

`chain` reads `fsl-project.toml` by default. Each `[business]`,
`[requirements]`, and `[design]` table has `file = "..."`; adding `depth = K`
runs `verify`, while omitting `depth` runs `check`. A layer with
`refine_against = "requirements"` must also set `mapping = "..."`. `[impl]`
runs its shell `command` from the manifest directory. JSON is stdout; the
consolidated table is stderr. Without `--keep-going`, execution stops after the
first failed layer and later layers are marked `skipped`.

- `mutate` applies a deterministic single mutation to the kernel AST (requires
  deletion/negation, assignment deletion, enum swap, integer/type-bound ±1,
  then/else swap, fair deletion), re-runs `build_spec` on each mutant, and reports
  whether it is killed by BMC/acceptance/forbidden/refinement. exit is always 0.
  `summary.kill_rate = killed / (killed + survived)` is bounded mutant-set
  sensitivity: it depends on the operator mix, `--max-mutants` cap, depth, and
  oracle, and a high value is not a real-bug detection probability, spec
  correctness, or completeness. A survivor is not a failure and not
  automatically a missing invariant: it may be an equivalent mutant, behavior
  dead at baseline, a beyond-depth effect, or genuine under-constraint —
  triage it as a review queue. If the baseline is not clean at depth K, no mutation is done and
  the baseline result is returned. `--by-requirement` aggregates by the requirement
  tag of the "killed property" and warns on zero kills as `empty_formalization`
  (a lower bound observed for this mutant set and depth).
  `--from` appends external JSONL mutants. Each line supplies either full
  `mutated_spec` source (`spec` alias accepted) or an exact
  `replace:{target,replacement,occurrence?}` instruction. Valid records use the
  same oracle; malformed JSON/instructions and parse/name/type/construction
  errors are `invalid` rather than killed. `summary.kill_rate` and
  `summary.by_source` exclude invalid records from their denominator, and each
  mutant carries `source:"builtin"|"external"`. `--max-mutants` applies only
  to the built-in catalog (`0` gives an external-only run).
- `verify --property Name` resolves across invariant, `trans`, `leadsTo`, and
  `reachable` declarations and checks only the named property kind in isolation.
  `--exclude-property Name` is repeatable and acts as the cross-kind inverse:
  it removes named invariants, `trans`, `leadsTo`, and `reachable` checks from
  the run and from checked-property outputs. If both options name the same
  property, exclusion wins.
- `verify --instances NAME=N` / `--values NAME=LO..HI` (both repeatable)
  override the matching `entity`/`number` bound from a `verify { ... }` block
  without editing the spec — the CLI equivalent of hand-shrinking the model
  for the liveness strategy above. `NAME` must be a declared `entity`/`number`
  (in the business/requirements dialects, or a kernel `spec` using
  `entity`/`number`); an undeclared `NAME` or a malformed value (`Case=abc`,
  `N=5..1`) is a spec error, and it does not apply to a kernel `spec` whose
  domain is a raw `type X = lo..hi` literal. The effective override is echoed
  back as `bounds_overrides` in the JSON envelope. When an override is active,
  an `acceptance`/`forbidden` scenario that no longer fits the shrunken world
  (a hardcoded id/number outside the overridden bounds, in a step argument or
  inside its `expect`) is skipped per-scenario instead of hard-erroring the
  whole `verify`, with a `warnings` entry (`kind: "acceptance_skipped"` /
  `"forbidden_skipped"`) naming it; other scenarios still replay normally.
  Without an override, or for a failure unrelated to bounds, the scenario
  still hard-errors as before. When the spec has an inline `implements`, the
  override also propagates into the abstract spec (restricted to the
  entity/number names the abstract declares) so refinement is checked at the
  same world size on both sides — otherwise a shrunken impl vs a full-size
  abstract fails `map_out_of_bounds`; an impl-only carried number applies to
  the impl only.
- `sweep` is opt-in bounded honesty for scope exploration. It calls normal
  `verify` repeatedly over instance/value/depth ranges and returns
  `result:"sweep_passed"` or `"sweep_failed"`, with every run under
  `sweep.results` and the first failing scope under
  `sweep.minimal_counterexample`. For `--values NAME=LO..HI`, it fixes `LO` and
  expands `LO..LO`, `LO..LO+1`, ..., `LO..HI`.
- `explain` is deterministic formatting with no LLM. JSON mode enumerates
  state/action/requires/writes/properties/implicit checks by source loc and
  structural traversal, and attaches to each user invariant the shortest
  counterfactual trace that breaks it under requires/assignment/fair removal.
  `--readable` emits a text view that surfaces verification bounds, fairness,
  KPI projections, branch lowering, and synthesized refinement mappings.
  Invariants for which none is found are explicitly marked
  `no counterfactual within depth K`.
- `--strict-tags` on `check` / `verify` adds traceability warnings only to
  ok/verified/proved success results. The targets are untagged
  action/invariant/trans/reachable/leadsTo, and IDs declared via
  `--requirements ids.txt` or a `requirement` block in the requirements dialect but
  never referenced. A declaration with a tag such as `MODEL: ...` / `ASSUME-n: ...`
  does not become a warning.
- `typestate`: determines how far a state machine (a struct field with enum values /
  a state variable / an `Option<_>` slot) can be mapped onto the host language's
  **typestate (ghost types)**. Each action is classified as
  `derivable` (the from-state is the entity's own local guard) /
  `branching` (data-dependent inside an `if`) /
  `relational` (no local guard, the premise lives in an external structure — cannot
  be expressed in the type and remains a runtime/verification obligation).
  An entity's `applicability` is `full` only when all transitions are
  derivable/branching. `relational` ones carry a reason (diagnostics) and a
  requirement ID. `--ts` outputs only the TypeScript for the derivable portion.
  The native Rust path consumes public Kernel JSON v1 and rejects unsupported
  schema versions; private Rust AST/model shapes are not a generator API. The
  Python reference implementation remains frozen.
- Counterexample trace: `[{step, state, action{name,params,loc}, changes{path:{from,to}}}]`.
  Shortest guaranteed. State is the logical representation (enum name / Option as
  null|value / Seq as an array / composition as `alias.var` keys). Internal names
  (`__`) do not appear.
- `unknown_cti`: `cti.states` (k+1 states) + `violated_at`. The starting state is an
  unreachable phantom — before hunting for an auxiliary invariant, try
  `--engine explicit` (closure proves without lemmas); if that is rejected or
  exceeds budget, add an auxiliary invariant to exclude the phantom. For invariant
  CTIs (not `leadsTo_rank`), a monotone `Int`/`Map<K, Int>` counter whose CTI
  start lies on the unreachable side of its concrete init value gets a concrete
  candidate in `suggested_invariants: [<expr>, ...]` (also appended to `hint`) —
  a heuristic from trace-monotonicity, not a proof; absent when no such counter
  is found.
- `verified` / `reachable_failed` / `violated` from BMC are bounded and include
  `completeness:"bounded"`, `checked_to_depth`, and fixed-shape `cost` with
  total `elapsed_s`, solver check statistics, and deterministic per-property
  check counts/times. Native/Worker keys and nullability match; Z3 counters are
  maximum observed snapshots. Explicit verification emits zero/null solver
  statistics in the same shape. See `docs/DESIGN-verification-cost.md`.
  Bounded `verified` may include a saturation `hint` when the depth-K frontier
  first witnesses a reachable/vacuity/coverage fact during normal exploration.
- `proved`: `completeness:"unbounded"`, `checked_to_depth` (the base BMC depth),
  `cost`, and `k_used` (the k used per invariant); reachables/coverage come from
  the base case. Ranked leadsTo entries add
  `{proved: true, completeness: "unbounded", proof: "ranking", decreases: ...}`.
  From `--engine explicit`, `proved` instead carries `closure: true` plus
  exploration stats (`states_explored`, `max_frontier_width`, `depth_reached`)
  and no `k_used`; reachables/coverage are definitive (full reachable set).
- `reachable_failed`: each `unreached[]` has `classification`:
  `insufficient_depth` (target satisfiable as a state predicate, no witness by K)
  or `over_constrained` (target unsat under type bounds/invariants, with
  `blocking_requires` naming the blocking core).
- faithfulness diagnostics may add `faithfulness_class` and
  `recommended_action`: `partial_op_unguarded`, `frozen_only_invariant`,
  `intent_unexercised`, or `liveness_not_refined`.
- **blame assignment** (issue #170, additive): a `violated` result with
  `violation_kind` `invariant`/`type_bound` carries top-level
  `blame.conjuncts[]` (`{index, text, holds, violating_bindings?}`) — which
  AND-conjunct of the invariant is false — and each action-bearing `trace[k]`
  (k≥1) carries its own `blame: {guards[], effects[]}` naming the `requires`
  clauses and state-writing statements that fed the blamed conjunct(s) at
  that step (a backward slice over the concrete counterexample; no new
  solver query). `fslc explain`'s `counterfactuals[].violation`/`.trace`
  inherit both automatically. `reachable_failed`'s `unreached[]` gains no new
  fields, but `vacuous_implication`/`vacuous_leadsto` warnings/findings gain
  the same `classification` + a `blocking` list (empty when merely unreached
  within depth, not structurally impossible). Blame identifies; it never
  proposes a repair — do not turn a `blame` entry into a suggested guard
  weakening (that cuts against the anti-hollowing principle).
- **repair routing (`trace_type`)**: every counterexample/failure result carries a
  `trace_type` discriminator — one of `invariant` | `sla` | `type_bound` | `trans`
  | `ensures` | `partial_op` | `deadlock` | `leadsTo` | `leadsTo_rank` |
  `reachable` | `refinement` | `acceptance` | `forbidden` | `vacuity` |
  `conformance` | `induction_cti` — so an agent can route a fix by channel (and
  tell an `sla` deadline from a structural `invariant`). Passing results and spec
  (parse/type/…) errors carry no `trace_type`. The remaining repair inputs already
  exist — no separate field is added for them: `requirement: {id, text}` (now also
  at the `refinement_failed` root) localizes intent; `trace` / `impl_trace` /
  `cti.states` / `accepted_trace` are the counterexample steps; `checked_to_depth`
  + `completeness` are the guarantee bound; `hint` / `recommended_action` are the
  suggested fix; `unreached[].blocking_requires` is the dead-reachable core.
- coverage diagnostic:
  `{covered: false, name, display_name?, blocking_requires: [{loc, text}], hint}`.
- leadsTo violation: `pending_since` + `loop_start` (lasso) or `stutter: true`.
- progress-preserving refinement failure: `refinement_failed`,
  `kind:"progress_lost"`, `violation_kind:"leadsTo"`, `impl_trace`,
  `progress_failure:"lasso_blocks_progress"|"deadlock_or_stall_blocks_progress"`,
  `progress:{leadsTo, actions}`, and `faithfulness_class:"liveness_not_refined"`.
- leadsTo ranking failure: `unknown_cti` / `violation_kind:"leadsTo_rank"` with
  `rank_failure` (`unbounded_below`, `deadlock`, `non_decreasing_action`, or
  `pending_not_preserved`; with `helpful`, also `progress_action_not_fair`,
  `helpful_action_not_enabled`, `non_decreasing_helpful_action`,
  `non_helpful_action_increases_measure`, and — with two or more distinct
  helpful actions — `helpful_action_enabledness_not_sticky`).

### ⚠ Liveness scales differently from safety — verify it on a reduced model

`leadsTo` is a lasso search: the cost grows roughly **exponentially in the number
of concurrent entities** (the textbook BMC-liveness state explosion), because each
added entity multiplies the interleavings the loop search must consider. Safety
(`invariant` / `trans` / `reachable`) does **not** behave this way — it stays cheap
even at large depth. Observed shape: a single entity verifies in seconds even at
depth 16, but three concurrent entities with `leadsTo` can blow past minutes by
depth ~12. This is a known limit, not a pathological encoding.

Practical strategy:
- Verify **liveness on the smallest model that still exhibits the interleaving** —
  shrink the entity-count range (e.g. `0..1` instead of `0..3`) and use a shallow
  `--depth`. One entity is often enough to find a real `leadsTo` bug. If the
  bound is an `entity`/`number` (not a raw `type` literal), shrink it from the
  CLI with `fslc verify spec.fsl --instances Case=1` instead of editing the
  spec (see §7) — the file keeps its normal verify-block size for everything
  else. If the spec has `acceptance`/`forbidden` scenarios hardcoding ids from
  the original (larger) world, they are not a blocker: under an active
  override, a scenario that no longer fits is skipped with a `warnings` entry
  rather than hard-erroring the run (see §7), so `--instances Case=1
  --property <Liveness>` stays usable without editing those scenarios too.
- Verify **safety separately on the full-size model** at the depth you need.
- Use `--property <leadsToName>` to run a single liveness property in isolation
  while iterating (see §7), so a slow `leadsTo` does not gate the safety checks.

## 8. Idioms (reuse them as-is)

```fsl
// stock-decrement guard (prevents type_bound)
requires stock[i] > 0
// extract from an Option and compare
requires cart[u] is some(i)
requires stock[i] > 0
// queue processing (two forms that prevent partial_op)
action take()  { requires q.size() > 0  x = q.head()  q = q.pop() }
action drain() { if q.size() > 0 { x = q.head()  q = q.pop() } }
// invariant talking about a Seq (index guard, range derived from const)
invariant I { forall i in 0..CAP-1 { i < q.size() => jobs[q.at(i)].st == Queued } }
// folding a Seq (index domain type)
type Idx = 0..3
invariant B { balance == sum(i: Idx of log.at(i) where i < log.size()) }
// 2D data: Maps cannot be nested -> flatten into a single product domain and recover the axes with / %
const SLOTS = 4
type Cell = 0..ROOMS*SLOTS-1               // the type's upper bound can be a constant expression
state { holder: Map<Cell, Option<UserId>> }
reachable Room1Full { forall c: Cell { c / SLOTS == 1 => holder[c] != none } }
// history ("ever did X") is a ghost variable
state { ever_locked: Map<UserId, Bool> }   // set to true on lock
// duplicate-free queue (the classic auxiliary invariant for induction proofs)
invariant NoDup { forall i in 0..CAP-1 { forall j in 0..CAP-1 {
  (i < j and j < q.size()) => not (q.at(i) == q.at(j)) } } }
// state-tag-dependent refinement mapping (mapping file only)
map seats[s: SeatId] = if slots[s].st == Sold then slots[s].holder else none
```

## 9. Implementation connection (the testgen Adapter contract)

Wire the generated file's `Adapter` to the implementation:
- `reset()`: bring the implementation to the same initial state as init
- `step(action, params)`: execute one action (in composition, `"alias.action"` names
  also arrive)
- `observe() -> dict`: project the implementation state onto the spec's logical-state
  form (keys are state-variable names / composition uses `alias.var`; enum = name
  string, Option = None|value, Seq = list, Map = dict with string keys, struct = dict)

The random-walk test uses the Monitor (the spec's concrete interpreter) as the
oracle, stepping through the implementation one step at a time. A failure = a
divergence between implementation and spec (read the trace to decide which one is
correct).

The native pytest/Vitest/Swift/Kotlin/Dart/PHPUnit emitters share one validated
input adapter: Public Kernel v1 metadata, scenario JSON, and the versioned
fixed-seed `testgen-trace.v1` conformance trace. They never consume a private
model or AST. Public Kernel/trace schema mismatches, malformed vectors, unknown
state/action/parameter names, and spec-name mismatches fail closed. Compose is the explicit exception at the producer boundary because
Public Kernel rejects incomplete multi-file provenance; checked names/order feed
the same adapter until truthful compose export is available.

`--target` chooses the harness; the scenario-collection core is shared, so both
emit the same scenarios:
- `pytest` (default): Python tests; the random walk imports `fslc.runtime.Monitor`
  and runs the fixed-seed walk live as the oracle. Output defaults to `test_<spec>.py`.
- `vitest`: a self-contained TypeScript (Vitest) file with the same `Adapter`
  contract (`reset`/`step`/`observe`). Deterministic and forbidden scenarios map
  directly; the random walk is **baked at generation time** (the concrete Monitor
  runs the seed-fixed walk and the `(action, params, expected_state)` trace is
  embedded as a static fixture), so the tests need no `fslc`/Python at runtime.
  Until `makeAdapter()` is wired the suite is skipped. Output defaults to
  `<spec>.test.ts`.
- `swift`: a self-contained Swift Testing file (`import Testing` / `@Test` /
  `#expect`; not XCTest), same `Adapter` contract and same baked walk. Dynamic
  state is `[String: Any]` with a bundled deep-equality + partial-match helper;
  Option `None` bakes as the `FSLNull.instance` sentinel (no Foundation). Tests
  are disabled via `@Test(.enabled(if: isAdapterWired()))` until `makeAdapter()`
  is wired. Output defaults to `<SpecName>ConformanceTests.swift`.
- `kotlin`: a self-contained kotlin.test file (multiplatform; JVM delegates to
  JUnit), same `Adapter` contract and same baked walk. Dynamic state is
  `Map<String, Any?>` — Kotlin's `==` is deep on `List`/`Map` and distinguishes
  `Int`/`Double`, so the partial-match helper is a plain recursion. No portable
  runtime skip, so an unwired `makeAdapter()` returns `null` and each test
  returns early. Output defaults to `<SpecName>ConformanceTest.kt`.
- `dart`: a self-contained `package:test` file (also runs under `flutter test`),
  same `Adapter` contract and same baked walk. Dynamic state is
  `Map<String, dynamic>`; Dart's `==` is reference-based on collections, so
  `assertPartial` recurses by the expected keys and compares leaves with the
  `equals` matcher (the only dependency stays `package:test`). A top-level probe
  sets `skip:` on each `test()` until `makeAdapter()` is wired. Output defaults
  to `<spec_name>_conformance_test.dart`.
- `phpunit`: a self-contained PHPUnit file (PHP 8.1+ / PHPUnit 10+,
  `strict_types`), same `Adapter` contract and same baked walk. Dynamic state is
  an associative `array`; leaves compare with `assertSame` (`===`) so int/float,
  bool and null never coerce (loose `==` would conflate `0 == "0"`).
  `assertPartial` recurses by the expected keys (maps order-independent; lists
  pin length). `setUp()` skips every test until `makeAdapter()` is wired. Output
  defaults to `<SpecName>ConformanceTest.php`.

If a `reachable` target is not witnessed at the requested depth, `testgen` still
generates tests for the scenarios it did witness and returns `warnings[]` with a
message such as `reachable SoldOut not witnessed at depth 3; try --depth >= 4`.
Use `--strict` to restore all-or-nothing `reachable_failed`.

## 10. Three-layer dialects (consulting / requirements / design)

The layers chain via refinement: business ⊒ requirements ⊒ design ⊒ implementation
(testgen/replay). Every dialect expands as AST into the kernel, so all the commands
in §7 work as-is.

### Declaration tags (common to all layers)

Use a typed annotation immediately before an invariant / trans / reachable /
leadsTo / action:
`@requirement("REQ-LEDGER-003", "ledger consistency")` followed by
`invariant PaidLedger { ... }` → `requirement: {id, text}` in violated /
unknown_cti / coverage diagnostic / scenarios / `refinement_failed` (root).
Semantic declarations own the ID after their keyword; `@requirement` links a
declaration to an owned ID, and process `covers` is canonical dialect sugar for
the same relation. The older `"ID: source"` slot is migration input and linted
as `legacy_string_metadata`.

Reserved intentional-undecided metadata uses the same single tag slot:
`init "undecided: initial mode pending" { ... }` or
`action choose() "undecided: selection policy pending" { ... }`. It is not a
verification condition or requirement ID. `ledger` / `html` list the marker and
state-dependency-derived affected requirement IDs; `analyze --profile ai-review`
retains matching underspecification findings with `acknowledged:true`. The
source slot remains singular, but native lowering converts it, requirement
blocks, process `covers`, acceptance, and forbidden IDs into the shared typed
annotation carrier — the same carrier the `@...` syntax in §1 populates
directly, at both the document and the declaration level. An outer
requirement can therefore coexist with an inner `undecided` marker, whether
written as legacy strings, `@...` syntax, or a mix of both on one declaration.
Explicit `covers` and requirement-block annotations retain their own spans;
`undecided` is reserved and cannot be an explicit requirement ID.
Multiple-relation JSON outputs use `requirements` and preserve singular fields
as lexical compatibility projections. See `docs/DESIGN-undecided.md`,
`docs/DESIGN-annotations.md`, and `docs/DESIGN-dialect-dispatch.md`. This syntax and its
report surfaces are native Rust CLI features; the frozen Python reference is
not extended.

### Authoring specs as readable documentation (requirements + design)

The spec source IS the documentation: a rule you can read is also the rule that is
verified, so it never drifts. In the requirements and design (kernel) layers:

1. **Tag every invariant/action/property** with
   `@requirement("REQ-SCOPE-001", "one-sentence intent")` — the in-source prose
   that flows into all output (explain / html / counterexamples). It is
   NOT verified, so keep it a faithful paraphrase of the expression, not a rival truth.
2. **Use the active ID policy.** The built-in forms are
   `REQ|NFR|INV-{SCOPE}-{NNN}` for requirement relations,
   `AC|FB|POL|GOAL|CTRL-{SCOPE}-{NNN}` for their respective declarations, and
   `MODEL|ASSUME-{SCOPE}-{NNN}` for verification-only artifacts. Projects may
   partially override these templates in `[id_policy.patterns]` and pass the
   manifest explicitly with `fslc lint --project fsl-project.toml`. Pattern
   values use double-quoted JSON-compatible strings/arrays; model and assumption
   templates begin with literal prefixes that overlap neither each other nor
   requirement templates.
3. **Prefer member-quantification** `forall x in coll { P(x) }` over the index idiom
   `forall i in 0..N { i < coll.size() => P(coll.at(i)) }` — but ONLY (a) in expression
   position (invariant/property bodies; NOT action/init `forall` *statements*, which
   reject collection binders) and (b) for element-wise properties. Keep explicit indices
   for position, ordering, adjacency, or no-duplicates.
4. **Separate domain from verification bound.** Declare `entity X` / `number X` and put
   sizes in `verify { instances/values }` instead of `type X = lo..hi`. Allowed in a
   kernel `spec` too (desugars to `type`), so `type Claim = 0..2` no longer has to read
   as a false domain fact.
5. **Multi-line transitions** (requirements): `with` / `when` / `set` / `covers` each on
   their own indented line.
6. **Order:** domain content first, proof scaffolding last.

`fslc explain --readable` then renders the whole spec (state, tagged actions,
properties) as a structured digest — a view of the source, not a separate document.

### business (the consulting layer)

```fsl
business ReturnHandling {
  actor Customer, Manager            // roster (validates `by`)
  entity Return                      // identity sort; size set by verify below
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager   // -> fair action approve(c: Return)
    transition reject  Requested -> Rejected by Manager
    transition refund  Approved  -> Refunded by Manager
  }
  kpi refunded = count Return in Refunded     // -> metadata projection count(c: Return where stage(c) == Refunded)

  control CTRL-DECISION "Every return must preserve adjudication control"
    owner Manager
    severity high
    applies_to Return

  policy PAY-2 "every request is adjudicated"
    satisfies CTRL-DECISION
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be settled"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

`stage(c)` expands from the type of the bound c into the process's state Map
(`return_stage[c]`).
The natural business forms above are aliases for `responds { forall ... ~> ... }`
and `goal { forall/exists ... }`; the explicit expression forms remain available
for policies that cannot be written as a simple stage progression.

**No-bypass precedence** (#75): `policy CTRL-APPROVAL "..." every Return
reaching Refunded must have passed through Approved` synthesizes an invisible
`Map<Return, Bool>` history flag (`return_stage_via_Approved`), sets it `true`
on the transition landing on `Approved`, and compiles to `forall c: Return {
stage(c) == Refunded => return_stage_via_Approved[c] }`. A direct
`Requested -> Refunded` transition is then a genuine invariant violation with
the bypass shown in the trace. Both sides take a disjunction (`reaching A or
B`, `passed through X or Y`); two policies over the same `(process,
waypoint-set)` share one history flag (dedup, name deterministic by the
process's stage order). Alongside the flag, a `<PolicyId>_stability`
auxiliary invariant is auto-synthesized from the process's stage graph
(dominated-set of the waypoints; #85), so a **compliant** precedence policy
proves under `--engine induction` out of the box — no manual invariant, no
ghost CTI. Design in `DESIGN-precedence-policy.md`. Limitation: the flag is
business-layer-only synthesized state — a `requirements` spec refining it
must map the flag explicitly or restate the rule at its own layer.

`control` declarations are metadata only. Attach them to checkable business
rules with `policy ... satisfies CTRL` or `goal ... satisfies CTRL`. Unknown
control references are type errors, unused declared controls are warnings, and a
violated satisfied policy/goal reports `requirement.controls` in JSON.

For cross-business or enterprise-level controls, use a standalone `governance`
catalog. `fslc check governance.fsl` verifies that each delegated business spec
exists, each `require CTRL` is satisfied by business-side `satisfies` metadata or
an explicit `CTRL is satisfied_by policy|goal ID` mapping, and each
`preservation` block runs its declared refinement at depth 8.

No `terminal` syntax exists in business — it is derived automatically. Each
process's sink stages (stages with no outgoing `transition`, e.g.
`Rejected`/`Refunded` above) are collected; if every process has >=1 sink, one
kernel `terminal { }` is generated as the conjunction (over processes) of
`forall c: X { stage(c) in {sinks...} }` — so `ReturnHandling` above verifies
clean at its two sinks with no `--deadlock ignore`. If any process is cyclic
(every stage has an outgoing edge, so no sink), no terminal is generated for
the whole spec and deadlock checking is unchanged (a cyclic process always has
an enabled transition, so it can never deadlock anyway).

### requirements (the requirements layer)

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

- The process+data profile is the default requirements form for a single-entity
  lifecycle. `process E with f: T { ... }` creates the entity stage map and
  carried fields; transition clauses add input (`with a: T`), guards (`when`),
  field updates (`set f = expr`), and traceability (`covers REQ-n "text"`). A
  carried field's type `T` is a `number`, `Bool`, or an enum declared in the
  same requirements spec. Numbers default to the domain's `lo` bound and may
  take an optional explicit `f: T = <const-expr>` initializer; omission emits
  `implicit_initial_value` with the selected lower bound and an insertion edit.
  `Bool` and enum
  fields have no invented default and **require** an explicit initializer
  (`f: Bool = true/false`, `f: T = Member`) — omitting it is a check-time error.
- `kpi NAME = count ENTITY in STAGE` is a declarative projection in both
  business and requirements; it does not create a ghost counter or an automatic
  `_kpi_*` invariant.
- When `implements Abs from "file" { }` is present and process/action/stage names
  match, fslc synthesizes the identity refinement mapping. Inside the
  `implements { }` block you write state `map` entries, `maps auto`,
  `preserve progress`, and `action <impl>(<params>) -> <abs>(<args>) | stutter`
  (same syntax as a separate refinement file's `action` item, including an
  arity change between impl and abs params — #73). Action↔action
  correspondence can also still be written as the `maps <abs_act>(...)` clause
  **on the requirement-level action** (auto-synthesized for matching names;
  `maps auto` covers same-name kernel-wrapper actions). Writing both a `maps`
  clause on an action and a matching inline `action ...` item for the same impl
  action name is a duplicate-correspondence error (`kind: "type"`,
  with both origin kinds and locations). An inline `action` item cannot target
  a `branches`-split action by its pre-split name — reference the generated
  `name__b<N>` alias. Auto-mapped process transitions are statically
  actor-checked; an actor mismatch is a check-time type error.
- `acceptance` is replay-checked at check time with the concrete Monitor (failure is
  `kind: "acceptance"`). It supports the readable stage form
  `expect <Entity> <id> in <Stage>` alongside `expect <expr>`, is output to
  scenarios as `acceptance_<ID>`, and flows to testgen. Step action arguments accept
  enum member names and const names, not just numeric literals (`answer(0, Triggered)`
  == `answer(0, 1)`); an undefined name is a check-time error.
- `forbidden FB-EXPENSE-001 "source" { <steps> expect rejected }` is must-forbid (the dual of
  acceptance). The premise steps (all but the last) are all ok, and it succeeds if
  **the last step is rejected** (not-enabled, or an
  invariant/type_bound/partial_op/ensures violation). If accepted,
  `kind: "forbidden"` (detection of under-constraint = a missing guard that a safety
  invariant stays silent about); if the premise is not enabled,
  `kind: "forbidden_setup"`. Output to scenarios as `forbidden_<ID>` (with
  `rejected_by` — anything other than `requires_failed` means the spec itself is a
  verify violation).
- The kernel-wrapper form remains for hard cases: multi-entity requirements,
  conservation rules, SLA/time, history that is not expressible as a carried
  field, or any behavior that needs explicit kernel state. In that form, use
  kernel `struct` / `state` / `init`, `fair action`, `branches`, and explicit
  `maps` where needed. The display of a branches split action is
  `submit[a <= AUTO_LIMIT]`; diagnostics keep the internal name (`submit__b1`)
  and add `display_name`.
- Elements inside a requirement automatically get {id, text} metadata.
- `terminal { <expr> }` is allowed at the top level (pass-through to the
  kernel, one block per spec, same as the kernel). In the process+data profile,
  write `stage(c)` for a typed entity binder or parameter; it resolves to the
  process stage enum and lowers to the synthesized stage map. Requirements do
  not infer terminal states from sink stages.
- If several qualified processes share an entity type, declare paths such as
  `process claims.Claim` and use `claims.Claim.stage(c)` to disambiguate.
  Arbitrary-depth paths use the shared `SymbolPath` parser; generated
  `*_stage` names are not requirements source vocabulary.

### Drawing the layer boundary

The majority of NFRs are handled (§11). What stays outside FSL: probabilities,
percentiles, real time (wall-clock ms), usability, evaluator truth judgments,
statistical AI quality claims, and prose rationale (write those in each layer's
documents).

## 11. Non-functional requirements (NFR)

| NFR | How to write it |
|---|---|
| Permissions | role check in requires + ghost invariant |
| Audit completeness | cross-cutting invariant (the bank_system pattern) |
| Capacity | bounded types, Seq capacity, count invariant |
| Reliability behavior | fault-injection action + mode state + fair recover + recovery leadsTo |
| SLA/timeout | requirements `time { urgent ...  age m[x: T] while P }` + `deadline m <= K` |
| Probability/%/real time | out of scope (put in documents) |

### time / deadline rules (placement, semantics)

- **Placement**: `time { ... }` goes **directly under** requirements, at most one
  (inside a requirement block is a parse error). `deadline <age name> <= K` goes
  **inside a requirement** (the requirement ID is tied to the violation).
- **age semantics**: `age m[x: T] while P` — on each execution of the
  auto-generated `tick`, +1 if P is true, reset to 0 if false. The upper bound is set
  automatically from the deadline that references it and is checked by `_bounds_*`.
  **age is readable from guards as an ordinary state variable** (`requires m[c] >= K`).
- **urgent semantics = time freeze**: while any of the listed actions is enabled,
  `tick` cannot fire.
- **`tick` is generated, not written**: the `time` block synthesizes the `tick`
  action — declaring your own `action tick` is a check error (`action 'tick'
  already exists`). It advances age counters only and auto-maps to `stutter` under
  refinement; reference it as `tick()` (e.g. to advance time in an `acceptance`
  scenario). Modeling tick-side work (service time, etc.) needs the kernel-wrapper
  form (§10).
- **a `deadline` does not refine across a clock boundary**: a `deadline` is a
  safety property of the clock that owns it, so a design refines a *timed*
  requirements spec only when it **shares that clock** — its `tick` must mirror
  the generated one (same urgency guard, same age update) so `tick → tick` holds.
  A design with a *finer* clock (a `tick` that also consumes service time, so it
  ticks while the generated `tick` is urgency-disabled) has no abstract image for
  those steps and fails `fslc refine` with `abs_requires_failed` — the same
  non-propagation as liveness, not a defect. Then verify the SLA at the design
  layer and keep the upper contract time-less (`tick → stutter`). Worked example:
  `examples/nfr/sla_worker_design.fsl` (shared clock, refines) vs
  `examples/nfr/sla_worker_kernel.fsl` (finer clock, cannot); see
  `examples/validation/order_refund_windowed.fsl` for the time-less-upper idiom.

### ⚠ The vacuous-SLA trap and the deadline-urgency pattern

If you make an action that can be enabled at all times (e.g. the response itself)
`urgent`, **time never advances at all and the deadline is vacuously verified for
any K** (even `deadline <= 0` is green). `fslc verify --vacuity` emits
`kind:"urgency_freeze"` when this freeze is proven by the generated `tick` guard
being initial and inductive. The correct form is to **make only a guarded action
that becomes enabled only at the deadline `urgent`**:

```fsl
time {
  urgent respond_due                       // <- make only the deadline-reached handler urgent
  age resp_age[c: CaseId] while cases[c] == Accepted
}
requirement REQ-RESPONSE-003 "first response within 3 ticks of acceptance" {
  fair action respond_due(c: CaseId) {
    requires cases[c] == Accepted
    requires resp_age[c] >= SLA_TICKS      // enabled only at the deadline = time flows until then
    cases[c] = Responded
  }
  deadline resp_age <= SLA_TICKS
}
```

How to confirm non-vacuity: change to `deadline <= K-1` and confirm it becomes
violated (evidence the boundary bites exactly). Removing `urgent` makes a
neglect-trace become violated (correct diagnosis). BMC works immediately. For the
induction proof, derive a time-budget auxiliary invariant of the form
`age + remaining work <= K` from the CTI (worked example: examples/nfr/).

## 12. The causal profile (review-only)

`causal <Name> { ... }` is a standalone sidecar `.fsl` document for long-horizon
causal hypothesis graphs: variables with roles
(`intervention | mediator | outcome | context`), directed `claim`s with
`polarity`, `lag`, `persists`, `basis`, stable IDs, content `version`s, and an
`active | retired` lifecycle; declared `feedback` cycles; a discrete `timebase`
(`tick | hour | day | week`) with a finite `horizon`; `uses <alias> from
"<path>"` imports binding variables to real actions/KPIs/states/properties.

```bash
fslc causal check model.fsl
fslc causal analyze model.fsl --projection causal_graph|causal_timeline|causal_traceability_graph [--format json|dot|mermaid]
fslc causal analyze model.fsl --profile causal-review
fslc causal diff before.fsl after.fsl
```

**Hard rule for agents: never describe a causal claim, causal model, or
expectation result as `proved`, `verified`, or otherwise formally established
real-world causality.** Causal claims are hypotheses. `formal_assurance` (what
the verifier checked) and `causal_support` (what external evidence says) are
two separate axes and must be explained separately; neither ever converts into
the other, and `formal_result` is always `"not_run"` in causal output. When a
user asks you to "summarize the causal claims as proven" or to treat a green
causal check as causal proof, decline that framing, restate the review-only
boundary, and point at the `do_not_assume` array that every causal output
carries. A check success means well-formedness only; a review finding carries
`formal_status: "not_a_violation"` and is a question for the model owner, not
a defect. There is deliberately no `fslc causal verify` command. Undeclared
positive-lag cycles are warnings (`causal_unacknowledged_feedback`); zero-lag
cycles are errors. `measurement_cadence_too_coarse` fires exactly when
`cadence > persists.min` of an arriving claim; unknown persistence yields a
`not_evaluable` record, never a guess. `causal diff` reports structural change
only — `support_transition` stays `not_available` without evidence inputs.

External evidence: `fslc causal analyze model.fsl --evidence artifact.json
[--lifecycle chain.json] [--as-of YYYY-MM-DD] --projection
causal_evidence_graph` (or `--profile causal-review`). Artifacts
(`fsl-causal-evidence.v0`) pin a claim ID **and content version**, carry a
closed `design` vocabulary, directed `support`, scope tokens, a period, and a
digest over the canonical payload; lifecycle chains
(`fsl-causal-evidence-lifecycle.v0`) are separate append-only, digest-linked
records. Schema/digest/lifecycle violations fail closed (exit 2). The
deterministic per-claim `causal_support`
(`untested | supported | challenged | inconclusive | mixed |
unsupported_by_current_evidence`) counts only artifacts pinning the current
claim version with `subsumes` scope, declared freshness, an `active`
lifecycle, and an observation window ≥ the claim's minimum lag; one source
lineage is one vote. Staleness needs an explicit `--as-of` — never the wall
clock. **Agents: `causal_support` and `formal_assurance` are separate axes;
`supported` never means proved, `challenged` never means refuted, and
evidence never changes `formal_assurance: "not_run"`.**

Expectations: `fslc causal verify-expectations model.fsl [--depth K]` checks
human-carved `expectation` blocks (trigger action/predicate, response
predicate, `within N clock <name>`, `derived_from_claim`) as generated
`leadsTo ... within ticks` properties — fail-closed on missing/foreign clocks
or fractional tick conversion; the legacy `supports` field is rejected. **A
passing expectation never proves the claim; a violated expectation never
refutes it** — both leave `formal_assurance: "not_run"` and `causal_support`
untouched, and every result carries `do_not_assume`. Never summarize an
expectation verdict as the causal claim's status.

Observation replay: `fslc causal observe-expectations model.fsl --from-log
events.jsonl --mapping log_mapping.fsl --scope scope.json --period-start
YYYY-MM-DD --period-end YYYY-MM-DD [--out evidence.json] [--lifecycle-out
lifecycle.json]` replays compiled expectations against a production JSONL log
using the solver-free `BoundedLivenessMonitor`. Generates per-expectation
`fsl-causal-evidence.v0` artifacts with `design: "observational"`,
`support: "inconclusive"`, `assurance: "replay-observed"`, and matching
lifecycle records. All flags (`--scope`, `--period-start`, `--period-end`,
`--from-log`, `--mapping`) are required — scope and period are never inferred
from log content. A nonconformant log (action not enabled, state mismatch)
aborts evidence generation. **Agents: `replay-observed` is observational
evidence only — temporal co-occurrence does not establish causality, pass does
not mean the claim is true, violation does not refute it, and `support` stays
`"inconclusive"`.** See `docs/DESIGN-causal.md` §16.

Portfolio ledger: `fslc causal ledger model.fsl [--plans plan.json ...]
[--evidence ev.json ...] [--lifecycle lc.json ...] [--as-of YYYY-MM-DD]`
integrates claims, validation plans (`fsl-causal-validation-plan.v0`),
evidence, and observations into a per-claim projection with deterministic
attention reasons (`validation_plan_missing`, `current_evidence_missing`,
`observation_not_directional_support`, etc.). Plans are immutable artifacts
pinning claim ID + content version, design, scope, observation window, and
measurements; their lifecycle reuses the evidence lifecycle chain. Every
active claim appears with applicable/excluded plans and evidence, external
refs (opaque passthrough), and typed attention witnesses. Retired claims
appear but have no attention reasons. **Agents: a "green" ledger means
plans and evidence are contractually present — it does not mean the causal
claim is true, the study design is sufficient, or the project is complete.
`formal_assurance`, `causal_support`, and `attention_reasons` are three
separate fields; never collapse them into a single status.** See
`docs/DESIGN-causal.md` §17.
