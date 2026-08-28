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
`formal_result: "not_run"`, not `verified`/`proved`. An unrecognized `check
compatibility` rule name and an environment schema window the migration plan
never reaches both fail validation with exit 2. `fslc db observe` validates its
event envelope against `schemas/fslc/db/observation.v0.schema.json`
(exit 2 on a malformed record) and honors each event's `flags` snapshot the same
way `fslc db check` does.

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
At command entry, `fslc domain analyze` and `fslc domain expand` both read one
authored-source `String`, parse their `DomainSpec` with
`parse_domain_document_from_source`, and pass that same string to
`load_kernel_model_from_source` to construct the checked Kernel. Atomic path
replacement therefore cannot make either command validate a different source
version before returning output. They reject unresolved identifiers with the
original source location; neither command is a best-effort/raw-AST inspection
path for semantically invalid domain input. `domain generate`, `domain replay`,
`domain testgen`, and `domain check` extend the same single-snapshot contract
through checked-kernel scaffolding, Monitor replay, generic and adapter test
generation, and edition postprocessing.
The accepted #662 design keeps `event_*` flags one-hot/current-step and will add
a dedicated `Map<Correlation,SagaPhase>` in a follow-up; do not make global
event flags sticky or treat one effect's status map as a general saga history.
A saga `compensation { when Trigger after After { ... } }` block is guarded by
BOTH event flags (#713); because flags are one-hot, a compensation whose
trigger and after events differ is structurally disabled today and surfaces
as a `fslc verify` never-enabled action warning — do not suppress that
warning or weaken the guard to make it disappear.
Native domain generation is grounded in Public Kernel v1. A closed
`domain-scaffold-metadata.v1` companion retains source grouping/spelling that
lowering cannot publish. Versions, dialect, duplicate Kernel members, and
missing lowered type/state/action counterparts fail closed; source expressions
and effect/saga topology are authoritative in the companion because v1 has no
equivalent nodes. Emitters never receive `DomainSpec`, never reparse source
text, and the five targets preserve their pre-migration bytes.

Effect completion classification first honors explicit roles:
`success_event` -> `Succeeded`, `failure_event` -> `Failed`, and `timeout_event`
-> `TimedOut`, regardless of event spelling. The same event in multiple explicit
roles is a parse error. Outcomes without an explicit role retain the v0 name
heuristic in priority order (`timeout`/`timedout`, `fail`, `cancel`, otherwise
success). Completion status, retry eligibility, and success stickiness therefore
share the one lowered status classification.

The Rust frontend keeps an internal origin chain across direct domain lowering,
checked-model construction, verification, counterexamples, and `explain`.
Diagnostics prefer the original domain declaration/expression, expose generated
Kernel names only as machine detail, preserve primary/secondary origins and
expansion steps (`can()`, membership, legacy operators), and represent
source-less nodes as generated-only. Requirement tags are a separate
traceability relation, not origin identities. Public Kernel v1 remains
byte-compatible and does not expose the internal chain; publication belongs to
v2 (#256).

Omitted domain aggregate initializers retain the current Bool `false`, Int
`0`, enum first-member (rendered bare, as domain source itself accepts, no
matter how deeply the enum is nested inside a `value_object` or a `Map`
value -- never `domain_kernel_source`'s kernel-mangled identifier), range
lower-bound, external-placeholder `0`, `value_object` struct-literal,
`Option<T>` -> `none`, `Set<T>` -> `Set {}`, or top-level `Map<K, V>` ->
dense per-key `forall k: K { field[k] = <value default> }` behavior, and
every one of those shapes emits `implicit_initial_value` (#731) -- the
warning's dispatch matches the renderer's total dispatch
(`fsl_core::domain_type_default`, the single owner of both the selected
value and how any enum member within it is spelled). The warning carries the
selected value, reason, edition severities, source span, and -- where a safe
insertion exists -- a machine-applicable edit. A top-level `Map<K, V>` field
(no whole-field default exists at all) and a `Set<T>`/`value_object` field
whose brace-literal default cannot yet round-trip through `fslc fmt`'s
reformat-and-reparse pass (issue #770) omit the insertion and keep
`edition_severity.next` at `warning`: `migrate --write` is fail-closed and
would not write a corrupted file, but offering the insertion would trip
#770's reformat failure and fail migration for the whole file, dropping
every other edit in it too. An explicit whole-`Map` default and a `Map`
nested as another `Map`'s value are both rejected ("whole-Map domain
defaults are not supported" / "Map state requires explicit initialization
through supported semantics").

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
stochastic checker: `fslc ai eval` checks the selected `statistical_property`'s
declared `slice`/`min_samples`/`ci_lower`/`ci_upper` requirements against
precomputed eval JSONL (`--records`, or the declared `dataset`'s `source`
file), Wilson intervals, and `formal_result:"not_run"`. An unknown
`--property`/`--migration` selection is a check-time error (exit 2); a
non-`statistically_supported` gate status (`dataset_invalid`,
`evaluator_untrusted`, `slice_missing`, `insufficient_samples`,
`inconclusive`, `statistically_unsupported`) exits 1, and the result carries
the full `schemas/fslc/ai/statistical-result.v0.schema.json` field set
(`schema_version`/`status`/`slice`/`metric`/`n`/`estimate`/`threshold`/
`evaluator`/`assumptions` included, not just `result`/`interval`/`checks`).
`fslc ai regress` checks the selected `ai_migration`'s declared aggregate
`no_regression` metric clauses, `fslc ai compare` reports metric deltas,
`fslc ai drift` checks the selected `observed_property`'s declared
`observed`/`drift` requirements over runtime telemetry
(`observed_supported` / `observed_mismatch`), and `fslc ai compat` emits DB
artifact capability profiles for one `ai_component` or every `ai_component` a
project declares -- rejecting non-AI input and a project with no
`ai_component` at all (exit 2) rather than an empty profile. These results
are never formal proof.

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
kernel formulas — they add no probability semantics to `fslc verify`. They are
still parsed at `check` time: a `require` clause matching none of the known
grammars (`min_samples`, `ci_lower`, `ci_upper`, a point estimate, `observed`,
`drift`) is a spec error (exit 2) from both `fslc check` and `fslc ai check`,
because `check` must not accept a project `eval`/`drift` cannot execute.
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
  enum conversion <name> <ImplEnum> -> <AbsEnum> {
    <ImplMember> -> <AbsMember>                  // exhaustive bijection; every member exactly once
  }
  enum abstraction <name> <ImplEnum> -> <AbsEnum> {
    <ImplMember> -> <AbsMember>                  // source-total; repeated/unused targets are allowed
  }
  map <abs_var> = <expr over impl state>          // scalar abstract variable
  map <abs_var>[<x>: <KeyType>] = <expr>          // per-element mapping of a Map
  // use convert(<name>, <expr>) in a map or action argument
  // use abstract(<name>, <expr>) for an enum abstraction
  // map and action arguments otherwise use the same expressions as specs, including if <c> then <a> else <b>
  action <impl_act>(<formal params>...) -> <abs_act>(<expr>...) | stutter
  // formal params may be bare names or name: Type annotations matching the impl action
  // explicit map/action entries override maps auto; auto matches action params BY NAME
  // only (never position) — a different arity, a surplus/renamed impl param, or an
  // unmatched abs param is a type error, never a positional guess (#494)
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

Distinct nominal enums stay incompatible without an explicit named enum
mapping. Use `enum conversion` plus `convert(name, expr)` for a bijection. Both
endpoints must be enums;
unknown, duplicate, or missing source/target members fail as a located type
error. Conversion is member-wise and never inferred from ordinal position.
For a requirements `process` stage target, use the checked Kernel enum name
reported by `fslc kernel` (for example `CommandStage`). Raw production-log and
causal replay mappings have no typed impl model and therefore reject enum
conversion declarations/calls; use a typed refinement mapping instead.
Use `enum abstraction` plus `abstract(name, expr)` for a source-total
many-to-one boundary. Both endpoint enums are non-empty and every source
appears exactly once; repeated targets and
targets unused by all rows are intentional and accepted. It shares the local
name namespace with conversions, but its call form cannot be interchanged with
`convert`. Raw replay rejects abstractions for the same missing-type reason.
For a generated abstract Map key such as DB `Column`, the conversion direction
is abstract-to-implementation: convert the abstract binder before indexing the
implementation Map (for example `Column -> DesignColumn`).

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
| struct | `struct S { f: Qty, o: Option<Option<K>> }` | field = scalar or nested Option<scalar> only |
| Option<T> | `c: Option<Option<ItemId>>` | T is a scalar or nested Option around a scalar. `none` / `some(e)` |
| Map<K, V> | `m: Map<ItemId, Qty>` | K must be a bounded scalar; `Map<Int, V>` is rejected by `check` |
| Set<T> | `s: Set<OrderId>` | T is a bounded scalar |
| Seq<T, N> | `q: Seq<JobId, CAP>` | T is a scalar, N is a positive constant. FIFO |
| relation A -> B | `r: relation User -> Role` | Binary relation over bounded scalar endpoints |

Scalar = Int / Bool / domain type / enum. In a `state` declaration,
`x: lo..hi` is an anonymous domain type and is equivalent to declaring
`type X = lo..hi` and writing `x: X`.
**State-variable whitelist**: scalar | nested Option<scalar> | struct |
Map<bounded scalar, scalar|nested Option|struct> | Set<bounded scalar> | Seq<scalar, N> |
relation bounded-scalar -> bounded-scalar.
Anything else (nested structs, Set/Map/Seq as a Map value, etc.) is rejected by
check as a type error.

`Map<Id, Map<K, Bool>>` is rejected: the former check-only acceptance had no
end-to-end explicit-state initialization path. Map values remain scalars, nested
Options around scalars, or structs with those fields; Map/Set/Seq/relation values
are not state types.

Kernel `state` fields may carry deterministic inline initializers. They normalize
to ordinary root assignments before checking and therefore share Monitor/BMC/
induction/explicit/Public-Kernel semantics with `init`. Constants, enum members,
constructors, `none`, and deterministic collection literals are allowed. State
reads, references to another initializer, indexed/field targets, statement `if`,
`forall`, and bulk/relational initialization remain invalid inline. The same root
cannot be assigned both inline and in `init`.

## 3. Expression catalog

**`true`, `false`, and `none` are reserved and cannot name anything** —
specification, const, `def` or its parameters, type, enum or its members, struct
or its fields, state variable, action or its parameters, property, quantifier or
aggregate binder, or an `is some(x)` pattern binding. They always resolve to the
literal, so such a declaration is unreadable from every expression and the
misreading is silent: `state { true: Bool }` with `invariant AlwaysHolds { true }`
used to return `proved` while `init { true = false }` wrote a variable nothing
could read (#570). No other word is reserved — `count`, `sum`, `stage`, `in`,
`is`, `where`, `old`, `abs`, `and`, `or` are contextual and stay valid names.

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
  `reachable(r,a,a)` is **not reflexive**: true only via a real path of ≥1
  edges back to `a` (empty/acyclic `r` gives `false`, never a free 0-hop
  `a==a`). Use `acyclic(r)` for "no self-loops or cycles anywhere".
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
5. For Seq `pop/head/at/index`, **well-definedness is checked automatically** in
   both action and state-property context (`partial_op`). A requires, if, or
   implication guard works because path conditions and short-circuiting are
   considered. An out-of-range read inside a property is
   `_partial_property_<property>` (or `_partial_property_terminal`) — always
   guard with `i < q.size() =>`.
   `/`/`%` are the one exception: division by zero is
   *totally defined* as `0` (Euclidean for `b != 0`: `-7 / 2 == -4`, `-7 % 2 == 1`),
   so `a / 0` inside an invariant/trans/reachable/leadsTo/mapping expression always
   evaluates to `0` rather than being undefined — only the unguarded-in-action-context
   check is skipped there.
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
ignores all stops). `verify`/`sweep` vacuity selection is a warning only on the
verified/proved path: an action
with no enabled instance through the checked depth (`never_enabled_action`; bounded
evidence that can disappear at a larger depth), an unreached antecedent of an implication invariant (`vacuous_implication`), an
unreached leadsTo trigger (`vacuous_leadsto`), a requires clause always true under
the context of the preceding requires (`always_true_requires` — actions with
coverage false and compose synchronized actions are excluded; a synchronized
action's clauses are inherited copies from its components and are checked by
verifying the component spec on its own), and **an invariant that depends only on a
frozen state variable no action ever assigns to and is dynamically always true**
(`tautology_over_frozen` — a dead ghost; make it `const`, or suspect a missing
action that should change it), and a generated deadline `tick` proven dead because
urgency freezes time (`urgency_freeze`). The last three are decided over the
declared type space rather than over the states reached within `--depth`, so
their verdict never moves with the bound: `requires visits < 100` on
`visits: 0..100` is a real guard and stays unreported even at a depth that
never reaches 100. `--vacuity error` gives
`result:"error"`; `--vacuity ignore` disables it. For the two reachability
kinds (`vacuous_implication`/`vacuous_leadsto`, next paragraph) and their
`vacuity_probe_truncated` sibling, `ignore` additionally skips *computing*
the shared reachability probe rather than computing it and filtering the
result; the other four kinds (`always_true_requires`, `tautology_over_frozen`,
`urgency_freeze`, `vacuous_deadline`) are solver-decided lanes that are
still computed and then filtered. `never_enabled_action` is emitted from the
existing action-coverage exploration, then selected or removed by the same mode;
the structured `action_coverage` evidence remains a separate ledger projection and
does not change assurance.

`fslc scenarios` has no `--vacuity` mode and never escalates this finding to
exit 2. It independently retains the same typed `never_enabled_action`
coverage diagnostic so a generated scaffold cannot call a blocked action covered.

The `vacuous_implication`/`vacuous_leadsto` reachability probe shares one
budgeted concrete BFS across every antecedent/trigger in the spec. A
candidate that neither becomes true nor finishes exhausting its reachable
state space before the internal state-count budget is hit (or whose
expression fails to evaluate) reports `kind:"vacuity_probe_truncated"`
instead — vacuity was never established either way, so it selects under
`--vacuity` exactly like the other seven kinds (`error` fails closed on it
too). Budget exhaustion is rare; a corpus-conservation test keeps the
budget generous enough that no maintained spec hits it.

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
