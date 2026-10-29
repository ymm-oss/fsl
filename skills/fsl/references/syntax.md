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
attachment. They desugar to the same relation as `covers`/`requirement` block
relations and the legacy `"ID: text"` string slot (which `fslc lint` reports as
`legacy_string_metadata`) — see "Declaration tags" in
[layers](layers.md#declaration-tags-common-to-all-layers).
An annotation with nothing supported to attach to reports
`FSL-ANNOTATION-TARGET`. `domain`/`dbsystem`/`ai_component` nested
declarations also accept `@...`: `domain` aggregate `command`,
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
[@requirement("<ID>", "<intent>")]       // optional document annotation (line before `spec`); never verified
spec <Name> {                             // the older `spec <Name> "<kind>: <intent>"` slot is legacy_string_metadata
  const <NAME> = <const expr>             // integer constant (expressions allowed: CAP - 1, etc.)
  type  <Name> = <lo>..<hi>               // domain type (bounded integer)
  symmetric type <Name> = <lo>..<hi>      // interchangeable entity identities
  enum  <Name> { <Member>, ... }
  symmetric enum <Name> { <Member>, ... }
  struct <Name> { <field>: <type>, ... }  // field: scalar | Option<scalar>
  def <name>(<p>: <type name>, ...) = <expr> // non-recursive predicate, frontend-inlined

  state { <var>: <type> [= <deterministic expr>], ... }
  [@undecided("<reason>")]                 // optional, line before init/action: an owner-accepted open choice
  init  { <stmt>... }                     // assign exactly once per root/key/field (deterministic)

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
capture-changing substitution are errors, subject to the remaining property-binder
exception below. `def` is frontend sugar only, so
verify/prove/scenarios/Monitor behave exactly as for the hand-expanded
expression. Put the human-facing requirement tag on the surrounding invariant
or action; no compiler-generated predicate name appears in diagnostics.

Breaking removal (#1450): caller action parameters, lets, expression
quantifier/aggregate binders and statement `forall` may not capture a free name
in the expanded def body (excluding the def's parameters). These calls now fail
`check` with exit 2, `kind: "semantics"`, and `would capture variable` at the
call; rename the caller binding. Existing global/pattern shadowing refusals
remain unchanged. Direct and compose-source/component defs share the check.
Non-global `is some(v)` bindings exported by `requires` or a binary expression's
left operand also count as caller bindings, so those captures now reject.
**Exception / remaining false-green risk (Refs #1528):** binders declared
directly by a `leadsTo` property are not supplied to this check in before/after,
`helpful` arguments, `decreases`, or `within`. Rename such a
colliding binder before relying on verification; the fix covers expression
quantifiers/aggregates inside `leadsTo`, not that declaration-level path.
The binder's own `where` expression is checked; capture can reject before the
existing unsupported-property-wrapper-where diagnostic.
The final implementation's full 597-form `check` sweep over `specs/`, `examples/`, and
`rust/fslc/tests/fixtures/` in base
`11f01225e244dabde0f8dfd9aaa665bc47dccfd4` and built implementation
`dd7b47885680076412ab2f0b46ebe8854ff6758b` materialized trees has zero
ok-to-error transitions; inline captures still constitute a breaking removal.

Business/requirements dialects also have type-kinds whose finite bounds live in
a sibling top-level `verify` block instead of inline ranges:

```fsl
business <Name> {
  actor <Actor>, ...                       // roster; every transition `by` must name one
  entity <Entity>                          // identity sort; size set by verify.instances
  process <Entity> { ... }                 // needs the matching `entity`; no `with` fields, no `number`
  control <ID> "<text>"                    // optional governance/control metadata
  policy <ID> "<text>" satisfies <ID> ...  // optional control traceability
                                           // `satisfies` of an undeclared control is a
                                           // located error; a control nothing satisfies
                                           // is an `unused_control` warning
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

Breaking removal (#1552): for requirements `rule ... only when` conditions,
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
DB-engine evidence use versioned JSON schemas with
`formal_result: "not_run"`, not `verified`/`proved`. An unrecognized `check
compatibility` rule name and an environment schema window the migration plan
never reaches both fail validation with exit 2. `fslc db observe` validates its
event envelope against the `observation.v0` schema
(exit 2 on a malformed record) and honors each event's `flags` snapshot the same
way `fslc db check` does.

The twelve compatibility-rule names above are a closed vocabulary shared by the
core lowering and DB tools. Unknown names are rejected before model/artifact
generation (exit 2), including `kernel`, `scenarios`, `explain`, `testgen`, and
file-based `fmt`/`lint`/`migrate`. The authored lowering error is `semantics` with
the rule position: ordinary lowering envelopes carry `loc`, while formatter and
migration wrappers keep the position in `message`. `check`/Worker keep their
existing unlocated specialized-validator errors. This gate does not move DB-only
findings or reachable-schema validation into the core; valid rules/defaults and
intentional repair-tool carve-outs are unchanged.
Direct HTML/ledger reports keep verification's unlocated validator refusal before
presentation lowering. The #1473 comparison of 595 file-backed forms had no
`check` `ok`→`error` transitions or output/exit drift; exact commits, binary hashes,
and commands are recorded in `docs/design/DESIGN-db.md`.

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
Other covered consumers use the source header's edition; no new options are added.

Already-invalid expression `||` / `->` in spec, requirements, compose, business
policy, and refinement maps get `diagnostic_code: unsupported_in_edition` when
checking for the next edition, retaining `kind: parse` and exit 2. Use `or` / `=>`.
This does not ban structural composition/arrows or change domain expressions.

Current-edition `check`, `verify`, and Worker JSON warn on
applicable legacy forms: declaration string metadata,
colon quantifiers, and inline action `maps`, using `legacy_string_metadata`,
`legacy_quantifier_colon`, and `inline_action_maps`. Prefer typed annotations,
braced quantifiers, and action correspondences in `implements`. Warnings retain
the planner's exact spans and safety flags; refused edits are not safe fixes.
Domain checking rejects quantified domain expressions rather than producing this
quantifier warning; legacy metadata slots and requirement mappings are not domain forms.

Captured-root admission also covers db/compat check, scenarios, generic testgen,
kernel/conformance/testplan, typestate, explain, single-FSL-source analyze, trace
replay and all six causal commands (#1449).
The single-root extension completes 21 more modes: six AI readers, domain
analyze/expand/generate/replay/testgen, document check/claims/generate, db observe,
log replay, counterexample export ordering, mutate success warnings, sweep capture
and direct HTML/ledger raw-warning delivery. Causal diff admits both operands and
observation its explicit mapping. Classification and consumption share captured
bytes; each input slot is distinct, including two slots naming the same file.
Envelope edition metadata belongs only to the first slot; other slots add warnings.
All-root preparation makes a later causal root/mapping read error precede an
earlier root's parse/model error. Project-mode intake remains deferred.
Log replay similarly reports mapping IO before an ordinary root parse/model
error. Document check admits before artifact IO/frontmatter, then projects the
captured source under the recorded display label without reading that label's path.
Existing header/gate/conflict rules and read-error kinds are retained. Next-edition
migration findings refuse before consumption (exit 2); this can reject previously
ungated next-edition inputs, not current-edition syntax.

Measured boundary: BASE `5637daf2ce8ce3e1a2c3b1266a8442c3df7887e4`, built
implementation `33e3fcc5606249a097814d91cb09b506add8e97d`; `fslc check FORM`,
cache off, all 597 `.fsl` files in specs/,
examples/ and rust/fslc/tests/fixtures/ plus 309 inline occurrences, both binaries
in both materialized trees: 3,624 checks, empty ok→error set, no excluded fields.
Only warnings change in 57 corpus forms and 22 inline occurrences. Against parent
`25f68db7fe214ed2ed66034701ed1bf06b5fbe7c`, consumer removals are the four
next-edition migration forms (enum union, metadata, colon quantifier, inline maps)
in analyze/conformance/explain/html/kernel/ledger/mutate/scenarios/sweep/testgen/
typestate, plus enum union in testplan: 45 command/form observations, already
refused by check. Those raw-warning/mutate-success/sweep-capture modes were still
open at that measured revision; the single-root extension now completes them.

Open results finish warnings once; successful closed schemas and implemented raw
TypeScript/readable/graph outputs use stderr `warning: <finding-json>` instead of
changing stdout. Verification finalizes after governance; domain/database checks,
scenarios and generic testgen own the outer finish. Worker refusal precedes model/
solver use; editor refusal precedes model/analysis and retains UTF-16 ranges.
Repair projections, safety and exits are unchanged.
Direct raw HTML/ledger, document generate/claims and domain expand/testgen use migration
stderr delivery too. HTML/ledger file-output envelopes stay unchanged, with one
warning in the distinct baseline; approval-only rendering self-admits its baseline
without new stderr delivery. Mutate finishes its authored input once; sweep cells
borrow the one captured root's token.

Latest extension sweep: BASE `1844fa74a717ba9229e0b292d9421c0939016e5e`, Rust
revision `e04df80f35cf3a7a3b8119dc7b6513ac7272ce76`, each built in its own clean
archive. Cache-disabled `fslc check FORM`, 597 corpus files +29 required input
occurrences per tree, both binaries in both trees: **2,504 checks**, empty ok→error
and full stdout/stderr/exit difference sets, no excluded fields. The 29 are the
original eight (two domain snapshot includes, two scenario literals, two
empty-membership inputs, fixed approval source and command-matrix approval
baseline), twenty executed success-table sources and one replay-order root.
Includes and repeated source bytes are retained as occurrences.

Historical named sweeps: BASE `a42d4cede3d8d688daff3173de7bd839a6992172` /HEAD
`e2669212315f4db9bec2c0c525ba8d44f77bacfd`, 597+29 per tree /2,504 checks;
initial BASE `c79016efeb630c717292e9392901cfe65ab40915` /implementation
`ceb03268e567487f6ec080afcf3ab034d4a5ea0c`, 597+8 per tree /2,420 checks. Both
had empty ok→error sets; neither replaces the latest result.
The latest BASE/Rust-revision consumer pair repeats twice, 772 observations,
17 distinct breaking command/source removals (19 control occurrences per repeat,
including repeated requirements metadata): domain analyze/expand/generate/replay/
testgen × enum union (5); document claims × metadata in spec and requirements,
colon quantifier and inline mapping (4); document generate × metadata in spec and
requirements (2); document check × requirements metadata (1); replay --from-log ×
enum union, metadata in spec and requirements, colon quantifier and inline mapping
(5). These were non-error consumer inputs, not grammar removals or proof claims.
A mapping-only control already errored at BASE and is not counted as a removal.

Coverage remains partial: eight modes across seven leaves remain—analyze batch
and project, approval check/create/diff, chain admission, live diff and refine.
The approval-check scope includes ledger `--approval` overlay root rereads.
Deferred controls still execute and must reject; this does not close #1449.
Existing baselines and ledger migration rows are retained;
migration-only rows use 🟡 移行候補 / risk —, mixed/non-migration rows 🔴 要確認 / 要確認.
Dependencies are not recursively admitted. Git-revision roots (#1513), example
warning cleanup (#608), lint dialect support (#609) and other ledger warning
classifications (#1497) are separate; do not infer full reader coverage.

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
`event_*` flags are one-hot/current-step; do not make global event flags sticky
or treat one effect's status map as a general saga history.
A saga `compensation { when Trigger after After { ... } }` block is guarded by
BOTH event flags; because flags are one-hot, a compensation whose
trigger and after events differ is structurally disabled today and surfaces
as a `fslc verify` never-enabled action warning — do not suppress that
warning or weaken the guard to make it disappear.
Native domain generation is grounded in Public Kernel v1. A closed
`domain-scaffold-metadata.v1` companion retains source grouping/spelling that
lowering cannot publish. Versions, dialect, duplicate Kernel members, and
missing lowered type/state/action counterparts fail closed; source expressions
and effect/saga topology are authoritative in the companion because v1 has no
equivalent nodes. Emitters never receive `DomainSpec`, never reparse source
text.

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
byte-compatible and does not expose the internal chain.

Omitted domain aggregate initializers retain the current Bool `false`, Int
`0`, enum first-member (rendered bare, as domain source itself accepts, no
matter how deeply the enum is nested inside a `value_object` or a `Map`
value -- never `domain_kernel_source`'s kernel-mangled identifier), range
lower-bound, external-placeholder `0`, `value_object` struct-literal,
`Option<T>` -> `none`, `Set<T>` -> `Set {}`, or top-level `Map<K, V>` ->
dense per-key `forall k: K { field[k] = <value default> }` behavior, and
every one of those shapes emits `implicit_initial_value` -- the
warning's dispatch matches the renderer's total dispatch
(`fsl_core::domain_type_default`, the single owner of both the selected
value and how any enum member within it is spelled). The warning carries the
selected value, reason, edition severities, source span, and -- where a safe
insertion exists -- a machine-applicable edit. A top-level `Map<K, V>` field
(no whole-field default exists at all) and a `Set<T>`/`value_object` field
whose brace-literal default cannot yet round-trip through `fslc fmt`'s
reformat-and-reparse pass omit the insertion and keep
`edition_severity.next` at `warning`: `migrate --write` is fail-closed and
would not write a corrupted file, but offering the insertion would trip
that reformat failure and fail migration for the whole file, dropping
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
convention ([layers](layers.md#declaration-tags-common-to-all-layers)), every field here is a bare identifier or number.

Declare at least one tool (`tool` or `tools`) in every standalone or project-embedded `ai_component`. A tool-less
component, including one with only `authority { }`, is a breaking removal
(#1546): `check`, `ai check`, BMC, induction, scenarios, testgen and Worker checking reject it with
exit 2 / kind type, exact `ai_component '<Name>' declares no tools` and component
declaration loc. One declared tool with empty authority remains valid; the error
names the authored component, not the generated `Tool` enum.
Existing hard-rule/unknown-authority errors retain priority and their historical
per-command envelopes. #1545 extends no-tools rejection to `ai compat`,
`ai replay`, project `ai check`, and native/Worker project checking: a breaking
removal of tool-less components, including empty authority blocks. Standalone
#1546 envelopes are unchanged. Native `check` and shared Worker stages now also
reject undeclared authority tools and unknown hard rules in every project
component, matching project `ai check` (semantics / exit 2). Generic checking no
longer accepts `error_envelope_ai_project_unknown_tool.fsl` or
`error_envelope_ai_project_invalid_rule.fsl`. Project semantic positions remain
block-relative (#1574); LSP project-component diagnostics remain separate (#1575).

Measured #1545 breaking-removal sweep: independently built B
`173e5f970e571f46ef84f750287761919df2582e` and H
`c35fdf94d5d5e0c32d207c95c7fd1c2c79b47bc9`, each in its own git-archive tree.
Commands `<B-binary> check <relative-path>` in B and
`<H-binary> check <relative-path>` in H cover all 597 `.fsl` files and 30 Markdown
files under `specs/`, `examples/`, `rust/fslc/tests/fixtures/`, plus twelve new
inline occurrences per tree: **1,278 checks**, cache/unstable overrides unset,
no output exclusions. Four Markdown inputs have compiler-recognized FSL fences;
all 30 were checked, including bash-only `examples/ai/README.md` as a negative control.
The **three corpus ok/exit0 → error/exit2 removals** are
`error_envelope_ai_project_unknown_tool.fsl` and
`error_envelope_ai_project_invalid_rule.fsl` and `error_envelope_literate_ai_project.md`.
The Markdown project returns type/`ai_component 'EnvelopeParityLiterateProject' declares no tools`/
block loc 1:1/exit2; standalone literate rejection and both multi-block toggle
acceptances are unchanged. The **four new-inline removals**
are tool-less-component projects in CLI and Worker-shaped tests (two), an
empty-authority-only component in a project (one), and an invalid later component
after a valid first one (one). Three return type at block loc 1:1; the later
unknown tool returns unlocated semantics. Rewritten existing inline FSL: zero.
Other check-only exit/result/stdout/stderr comparisons agree. Declare a tool or remove
an unneeded component, and repair undeclared tool/unknown rule references.

Lowering-only validation was measured with the same B/H: `analyze <relative-path>`
on 18 AI-mentioning corpus inputs plus four explicit shapes; changed shapes and
compose control through `scenarios <relative-path> --depth 2 --deadlock ignore`
and `explain <relative-path>` (60 observations). Analyze22/scenarios4/explain4
each show **one ok→error, zero error→ok, two changed error envelopes**. Selecting
only `tool_authority` no longer hides undeclared `forbidden X`: success becomes
semantics/`unknown tool 'X' in authority block`/no loc/exit2. With no tools,
duplicate fallback x changes type/`declares no tools`/loc1:1 to semantics/
`duplicate fallback reason 'x'`/no loc; with a tool, unlocated semantics/
`duplicate action 'fallback_x'` becomes the duplicate reason diagnostic (all exit2).
Compose use of an AI component stays rejected: `compose use must reference a spec at 2:3`,
loc2:3. No other lowering-only acceptance removal was observed.

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
the full `statistical-result.v0` field set
(`schema_version`/`status`/`slice`/`metric`/`n`/`estimate`/`threshold`/
`evaluator`/`assumptions` included, not just `result`/`interval`/`checks`).
`fslc ai regress` checks the selected `ai_migration`'s declared aggregate
`no_regression` metric clauses, `fslc ai compare` reports metric deltas,
`fslc ai drift` checks the selected `observed_property`'s declared
`observed`/`drift` requirements over runtime telemetry
(`observed_supported` / `observed_mismatch`), and `fslc ai compat` emits DB
artifact capability profiles for one `ai_component` or every `ai_component` a
project declares -- rejecting non-AI input and a project with no
`ai_component` at all (exit 2) rather than an empty profile. `fslc ai check`,
`fslc ai compat`, and `fslc ai replay` share the same `ai_component` semantic
gate (undeclared authority tools and unknown `check hard` rule names, exit 2)
before any success verdict; project-level `fslc ai replay` uses the checked
project parser rather than a line scanner. These results
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
lenient line scanner separate from the kernel parser; they may sit alongside
`ai_component` in one file. When every component passes validation,
`fslc ai check` returns `ai_project_analyzed` (`fslc check` returns `ok` with
`ai_analysis_result:"ai_project_analyzed"`) — a declaration listing, not
verification. The example shows only the lines the scanner reads; any other
line in these bodies is skipped without validation, so it constrains nothing:

```fsl
dataset <Name> {
  source "<path/to/eval.jsonl>"          // the only line read
}

evaluator <Name> { }                       // only the name is read and listed

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

ai_migration <Name> {                      // only preserve { no_regression { .. } } is read
  preserve {
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
**`ai_action`, `ai_contract`, `retriever` (as a standalone block),
`trust_boundary`, and a top-level named `authority { target ... }` are recognized only as block
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
time (`inconclusive`, exit 1), not warned past. Eval statuses, in the
order a result is decided (the first that applies wins), are
`dataset_invalid`, `evaluator_untrusted`, `insufficient_samples`,
`inconclusive`, `statistically_unsupported`, `statistically_supported`.

Composite spec (a separate top-level form):

```fsl
compose <Name> {
  use <SpecName> as <alias> from "<relative path>"   // multiple allowed. nested compose not allowed
  state { ... }  init { ... }                    // optional local state; init may also initialize component state
  action <n>(<p>: <alias>.<Type>, ...) =
      <a>.<act>(<expr>...) [ || <b>.<act2>(<expr>...) ] {  // synchronize (run atomically together)
    [requires <expr>]... [<stmt>...]             // extra guards / assignments to composite-side state
  }
  internal <alias>.<action>                      // forbid standalone firing (only via synchronization)
  invariant/trans/reachable/leadsTo ...          // cross-reference via alias.var
}
```

Sync additional statements may assign only composition-side state: component
targets are rejected with a located error, including physical `alias__x` names
that previously passed `check` (breaking removal, #1334).

Reference arguments may read only header parameters, consts, and state, not additional-item `let` bindings (breaking removal, #1442).

Component free value reads and update roots resolve against the original component declarations before merging; composition cannot rescue an undeclared reference (breaking removal, #1442), while public names remain legal and protected enum reads display their source member spelling.

A glue action reads and writes local and component state directly via `alias.var`.
Compose `init` may also initialize component state (including physical names), but
overlap with a component's own init is rejected as duplicate initialization.

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
  // unmatched abs param is a type error, never a positional guess
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

For auto pairs only, both sides use registered source actors independently of
`covers` (even text beginning `by `), process/event/business form or fsl 4/5
edition. Requirement actions, inside a block or standalone, register their own
metadata's `by ` actor (trimmed and nonempty), including every generated branch.
Missing/non-`by`/empty own metadata registers absence, never an actor from block
prose or typed `@requirement` text. Only unregistered raw spec/compose actions retain
the legacy metadata rule; two available actors must match. A mismatch is a located
`maps auto actor mismatch` type error (exit 2) in check/unscoped verify/refine
and Worker. Explicit action entries and stutter are not actor-checked;
selected verify still skips the seam. Mapping-free diff instead reports
`unknown`/`automatic_mapping_failed` within `semantic_diff`/exit 0.
This is a v5.0.0 breaking removal of hidden actor mismatches (#1441), without a
Kernel/JSON schema change. The four-cell sweep of all `.fsl` in specs/examples/
rust/fslc/tests/fixtures at base `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9`
and head `9ca8630c7f45f1def54c056bf53a852853c7a60c`, running both binaries'
`fslc check` in each own tree (579 base / 591 head forms), measured **0
ok→error and 0 error→ok corpus transitions**. Separately, the expansion class
is registered transition metadata whose requirement text begins with `by `
(covers text and next-edition rule/decide text). With matching System actors,
impl/business covers text `by Manager` (B3/B4) and abstract event rule text
`by Manager only` without covers (F1/p5) expand error/type/2→ok/0 and bounded
verify→verified/0 with implements refines; F1 standalone refine expands
error/type/2→refines/0. Commands and manual migration: repository path
`docs/design/DESIGN-refinement.md`, section “#1441: source-actor auto mapping
and breaking-source migration”.

The #1456 v5.0.0 breaking change rejects own-actor mismatches hidden by requirement
block text, including standalone auto maps of generated branches. Matching or
absent own actors falsely rejected by prose are intentionally accepted instead.
Explicit action entries, inline branch maps and stutter remain exempt. Metadata
priority, Kernel v1/v2, RCIR/digest and attribution are unchanged. Both binaries'
`fslc check` swept every `.fsl` in the same three roots in each archived tree,
base `e8cf78f0162530e264128f588981d49790f2767f` / head
`b8daaa8245f374f74448fd983755689f832484cc`, twice, 591 base / 592 head forms
per binary: **0 corpus ok→error and 0 error→ok transitions**. Acceptance controls
are a separate set. See `docs/design/DESIGN-refinement.md`, section
“#1456: requirement-action own actors and breaking-source migration”. Confirm
policy and correct the action itself, not enclosing prose. No general automatic
migration, bypass or grace period; if problems occur, return to a 4.x binary.

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
| entity kind | `entity Claim` / `process Claim ...` | Finite identity sort for business/requirements (also kernel `spec`); bound by `verify { instances Claim = N }` |
| number kind | `number Amount` | Finite numeric sort for requirements (also kernel `spec`); bound by `verify { values Amount = lo..hi }`. **`business` does not accept `number`** (parse error `expected business declaration`) |
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

Spec/Kernel enums (also `symmetric enum`) must have at least one member, even when unused.
`enum E { }` is a model-build type error: `enum 'E' has no members`, declaration
loc, CLI exit 2. This breaking removal (#1546) applies to `check` and every
model-consuming command/engine and Worker checking, including the former
`EmptyImpl` / `EmptyAbs` refinement inputs. Declare the intended members or
remove an unused empty declaration; do not exploit an empty domain for vacuity.
Singleton enums remain valid. Domain dialect empty enums keep their earlier
semantics diagnostic. A child compose enum uses the #567 wrapper: parent-use
loc and message with child path/declaration position plus parent path. This
empty-enum wrapper resolves the child path against the parent's directory,
unlike older #567 wrappers that preserve the literal `use` path. A
symmetric declaration starts at the symmetric keyword. A column-less
`dbsystem`, including `database app { schema 0 }` or only an empty table, is
also a breaking removal: type / exit2, exact `database 'app' declares no columns`
and database-declaration loc on check, BMC, induction, explicit, scenarios,
testgen and kernel. `db check` reports this only after static compatibility
findings, when lowering is reached: an active artifact reading an absent column
can instead yield violated/exit1, as on base. DB fmt/lint keep semantics/exit2,
a location suffix in the message and no loc field. One column remains valid. See
[the boundary decision](../../../docs/design/DESIGN-enum-member-identity.md#nonempty-enum-declarations-1546).

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
misreading would be silent (`invariant AlwaysHolds { true }` reads the literal,
never a state variable named `true`), so the declaration is rejected. No other word is reserved — `count`, `sum`, `stage`, `in`,
`is`, `where`, `old`, `abs`, `and`, `or` are contextual and stay valid names.

- Arithmetic: `+ - * / %`, unary `-`, `min(a,b)` `max(a,b)` `abs(a)`
  (in `a//b` everything after `//` becomes a comment, so write division with a
  space: `a / b`)
- Comparison: `== != < <= > >=` / logic: `and or not =>`
- Finite binders: `x: T`, `x in lo..hi`, or `x in set_or_seq`, each with an
  optional `where BoolExpr`. Maps and unbounded domains are rejected.
  Position matters: `check` rejects collection domains and all `where` filters
  on outer `leadsTo` wrapper binders. Supported unfiltered typed/range wrappers
  and collection quantifiers inside P/Q remain distinct. This breaking removal
  (#1463) closes the previous check-ok/engine-error gap; it does not implement
  collection/filter response semantics.
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
  `v` must be fresh: `check` rejects a `v` that names a state variable,
  constant, enum member, action parameter, binder, `let`, or an earlier pattern
  binding in scope.
  Arithmetic and ordering on Option are type errors.
- struct: literal `S { f: 0, o: none }`, `s.f`, `==` (field-wise equality; for an
  Option field, presence matches ∧ present ⇒ values match)
- Set: `Set {}` `Set { 1, 2 }`, `.add(e) .remove(e) .contains(e) .size()`
- Seq: `Seq {}` `Seq { 1, 2 }` (element count ≤ N), `.push(e) .pop() .head() .at(i)
  .contains(e) .size()`, `==` (length + all elements)
- Relation: `.contains(a,b) .add(a,b) .remove(a,b)`,
  `reachable(r,a,b) acyclic(r) functional(r) injective(r) domain(r) range(r)`.
  `reachable`/`acyclic` require a self-relation (`relation T -> T`).
  For `r: relation A -> B`, `contains`/`add`/`remove`/`reachable` require
  source `a` assignable to `A` and target `b` assignable to `B` independently.
  A mismatch is `kind: "semantics"`, exit 2, with the existing assignability
  message and `hint: use endpoints matching the relation's declared source and
  target types`. Enums are nominal; numeric ranges retain integer compatibility.
  Breaking removal (#1458): previously accepted mismatched endpoints (such as
  enum `Red` in a numeric relation) are rejected before evaluation. Do not repair
  this by coercing enum ordinals or weakening a property; use the declared types.
  `domain(r)` infers `Set<A>` and `range(r)` infers `Set<B>` (not always
  `Set<Int>`); binders over the projections and their public Kernel projection/
  binder annotations inherit the endpoint type. Enum-typed projection uses such as
  `not (exists x in domain(r): reachable(r, x, x))` are accepted; their runtime
  meaning is unchanged and false invariants still violate. Numeric projections
  remain integer-compatible. The restacked boundary sweep used BASE
  `9af1fa5f36f91e415b7ba249888f4ada4cf7a0db` and HEAD
  `3a8a6019a5bed3ae62f960afedf2bdcf6d129341`: **zero corpus `ok` → `error`
  transitions** across all 597 forms. All 134 inline occurrences were checked;
  #1458 removes 24 occurrences (22 distinct sources) in both modes, 48 mode-form
  transitions. The former 46-transition subset is unchanged; the expanded
  inventory additionally counts numeric Set assignment below. Three counted
  forms (`count`, enum set-literal equality and matching enum Set assignment)
  add six `error` → `ok` pairs. Each mode/tree ran twice: 5,848 checks, full
  outputs without exclusions. On this base, the source-only revert calibration
  produces 28 intended failures / 24 preservation passes; exact restoration is
  followed by 52 passing fixed tests. Workspace Clippy is warning-free. The
  workspace suite passed 3,468 tests on `fc77124b31744f1c1728295db6f09508265b8285`;
  only a proved whitespace-only commit follows it to the measured HEAD (the whole
  source file is equal after removing whitespace). Builds, CLI, fixed tests twice
  and fmt pass on that HEAD. Since the earlier `8e54e862` measurement, main's
  #1463 rejects `explicit_init_state_collection.fsl` on both trees and locates
  the BASE count diagnostic; #1458 removal/addition membership is unchanged.
  Commands and identities are in
  `docs/design/DESIGN-nested-option-support.md`, section #1458.
  The endpoint removals for numeric `N -> N` are
  `not reachable(r, Red, Blue)`, `not reachable(r, Red, 1)`,
  `not reachable(r, 1, Blue)`, `not r.contains(Red, Blue)`,
  `not r.contains(Red, 1)`, `not r.contains(1, Blue)`,
  `r.add(Red, Blue).contains(Red, Blue)`, `r.add(Red, 1) != r`,
  `r.add(1, Blue) != r`, `r.remove(Red, Blue) == r`,
  `r.remove(Red, 1) == r`, `r.remove(1, Blue) == r`;
  for `N -> Color`: `not r.contains(1, 2)`, `r.add(1, 2) != r`,
  `r.remove(1, 2) == r`. `Red`/`Blue` belong to enum `Color`.
  LSP repeats the numeric target-only contains form; Worker repeats target-only
  reachable. Projection typing additionally removes, for `s: relation Color -> Color`
  and `r: relation N -> N`, `forall x in domain(s): not r.contains(x, x)`,
  `forall x in range(s): not r.contains(x, x)`, `not domain(s).contains(0)`,
  `not range(s).contains(0)`, `forall x in domain(s): x != 0`, and
  `forall x in range(s): x != 0`. The expanded inventory also rejects
  `xs = domain(s)` for `s: relation Color -> Color`, `xs: Set<N>`, at assignment
  origin 8:19. Do not coerce enum elements to numeric ordinals.
  General breaking rule: projection elements/sets take their endpoint type, so
  enum or Bool elements reject anywhere `Int` is expected; their whole sets reject
  where a numeric Set is expected. This includes comparison, ordering,
  arithmetic, `min`/`max`, membership, set-literal equality, Option and pattern
  payloads, numeric Set state assignments and refinement maps. The measured
  fixture list is not exhaustive. Newly accepted families: `count` over finite
  endpoint-typed domain/range projections; `sum` over numeric projections; enum
  set-literal equality, enum-typed `where` filters and conditional projection
  membership; `let`-bound projections consumed as matching typed sets; matching
  enum Set assignments and refinement maps. `unique` over enum domain/range was
  already accepted on released main and remains accepted. Do not confuse acceptance
  with a verified invariant. The preserved enum-binder/cycle idioms are
  `not (exists x in domain(r): reachable(r, x, x))`,
  `forall x in domain(r): not r.contains(x, Blue)`, and
  `forall y in range(r): not r.contains(1, y)`. They already checked on released
  main and are not new user-facing additions.
  `reachable(r,a,a)` is **not reflexive**: true only via a real path of ≥1
  edges back to `a` (empty/acyclic `r` gives `false`, never a free 0-hop
  `a==a`). Use `acyclic(r)` for "no self-loops or cycles anywhere".
  Relations are not Maps: `r[a]` and `r[a] = b` are rejected by `check`, even
  for a functional relation. Keep using the relation methods. This is a
  breaking removal (#1463) of accepted but unexecutable reads/writes; Map
  indexing is preserved.
- conditional expression: `if c then a else b` in any expression position;
  `c` is Bool, both branches have one logical type and are checked statically,
  while only the selected branch is evaluated
- ensures/trans only: `old(expr)` / leadsTo only: `P ~> Q`,
  `P ~> within K Q`, plus optional `decreases <int expr>` for induction ranking

Static operand/signature boundary (#1459, breaking): check both conditional
branches and all action/trace expression positions. `is none`/`is some(x)` need
Option; `abs` needs an integer-compatible operand, including projection elements
(no enum ordinal or Bool coercion). All six relation functions require a relation;
non-relation domain/range no longer infer `Set<Int>`. `reachable`/`acyclic` require
identical declared endpoint types (`T -> T`): mixed enum/numeric relations and
`N -> M` reject, even when numeric N and M have identical bounds. Numeric
compatibility is not self-relation identity. Heterogeneous functional/injective/domain/range and
relation endpoint methods remain valid. Supported method signatures are exactly
Set contains/add/remove with one item and size with none; Seq contains/push/at
with one argument and pop/head/size with none; relation contains/add/remove with
two endpoints. Map has no methods. Thus `n.contains(0)`, `s.push(0)`, `q.add(0)`,
`r.size()`, `s.contains()`, `q.pop(1)` and action assignment `n = q.at()` reject.
Validate original count/sum binders, Bool filters and integer values even if the
domain is empty: `count(i in 1..0 where 1)` and `sum(i in 1..0 of true)` reject;
well-typed empty values remain zero. Failures are located semantics errors,
exit 2, before execution/replay. Preserve Option binding scope, endpoint hints
and non-reflexive reachability. See the #1459 design amendment for sweep evidence.

#1459 audit: BASE `e8cf78f0`, HEAD `33d7faa7`; 591 corpus + 384 inline occurrences
= 975 forms, stable/fsl5 twice in three independently built trees (baseline,
immediate parent, head), 11,700 checks. Corpus ok→error set is empty and no other
corpus output drifts; no field/form exclusions. Parent→head attributes 248
mode-form removals (124 occurrences/123 distinct sources) and 60 diagnostic
reclassifications to the six categories above, including numeric N→M. All 60
remain error/semantics/2, now authored static errors rather than unlocated
runtime errors. The expanded inventory separately retains inherited #1439
(86 removals/11 reclassifications) and #1458 (56 removals/6 additions); do not
attribute these to #1459. Full accounting and calibrated controls are in the
design amendment.

Review round 1: self-relation identity follows both conditional branches
(nested/unselected included), `old`, relation add/remove receivers and action
let aliases. A self-typed then branch cannot hide a selectable N→M branch.
Projection equality retains the real relation error over context-needed empty
literals and keeps the authored/precise inner origin; range bounds and terminal
also avoid generated 1:1 origins. Numeric/Set compatibility and valid verdicts
are unchanged. This describes visited expression trees. Helpful argument typing
was outside the historical round-1 baseline; main's #1527 now checks parameter
types and is preserved. Relation-valued temporary collections remain unsupported
separate upstream work.

R1 delta BASE `33d7faa7` → HEAD `8caf5c2e`: 42 additional mode-form removals
(21 source occurrences) and 24 diagnostic reclassifications (12 occurrences:
projection inference 16, range origins 6, terminal origins 2), all latter still
error/semantics/2. Expanded inventory: 591 corpus + 447 inline = 1,038 forms;
both modes twice in four own-built trees, 16,608 checks including the separate
R0 baseline. All 8,304 full repeat pairs agree, no field/form exclusions or
corpus drift. Keep inherited/R0 changes distinct from this R1 delta.

Current audit: final BASE `11f01225e244dabde0f8dfd9aaa665bc47dccfd4`, measured
implementation `2b7d2e98349505af7dccb7843e41ab659066f021`, measured on
`b1557a31b77f549d2c48823c3435217f9df4fc53` and carried to `11f01225`, whose
tree differs only by the two generated language pages. `fslc check FORM`, cache
off, stable/fsl5 twice: all 597 corpus + 601 inline inputs = 1,198 inputs in three
states (historical `9af1fa5f`, final base, implementation), 14,376 checks + 24
identity controls. No corpus output drift or excluded forms/fields.
Final-base→head: **294 removals** (147 occurrences/142 distinct
sources), **62 diagnostic reclassifications**, no additions. Main #1463 already
owns 48 former removal mode-forms and four former reclassifications; the added
let/Kernel/CLI/shared-row occurrences contribute 26 removals. Historical #1458
remains 48 removals/six additions on its 134-entry subset, 56/eight when expanded.

Breaking removal: shared row `X1/operand-method-types`, Bool `done` with
`invariant Capture { acyclic(done) }`, now rejects check at 8:3. Its outcomes
change `[Ok, Error, Error, Error]`→`[Error, Error, Error, Error]`; baseline stays
recorded. The remaining 136 shared-row sources + one resolver dependency
(137 inputs) have **zero further changes** under the same command at the named
base/head, both modes twice. Keep 292 inherited embedded-path differences and 16
observations from two mis-extracted scratch-label inputs explicit: current
sources are the real #1458 test specs, accepted on both baseline binaries, so
attribution is unchanged. Optional replay not run; sweep outputs not normalized.

## 4. Statements (init / action body)

Init assignment values/keys, if conditions and forall filters/bodies require
full definite coverage of a may-assigned root before reading it (#1451), not
just the selected key/field or one branch. Forall source/domain checks are
unchanged. Early
reads return `init references state variable '<name>' before it is assigned`,
semantics/exit 2, at the containing statement under check and ordinary verify.
This conservatively includes dead RHS reads, already-written components, and
free components/paths of a may-assigned root (`struct-other-partial`,
`if-one-branch-read`, #1481 N2), even though those values otherwise follow #493's
free-value semantics. Existing ownership/field/forall diagnostics keep priority.
Never-assigned roots remain free: check accepts `init { g = on }` or an
unassigned `choose` condition; explicit still rejects their reads. Permitted
same-forall current-key reads are exempt only for unresolved binder values,
including #1520 filtered/collection X1/X2. Resolved partial domains require
full-root coverage; typed full-domain loops remain accepted. Binder-keyed
filtered/collection writes, also when nested, do not establish full coverage
for a later read outside the loop. Use a resolved full-domain binder, e.g.
`forall k: K { m[k] = true } g = m[0]`, instead of its filtered/collection
equivalent before reading. Unread omitted/legacy partial init remain accepted.

Breaking-removal sweep: `fslc check <form>` in each own materialized tree, base
`c79016efeb630c717292e9392901cfe65ab40915` versus head
`2d05dd63f05ca68c4bd7d06c3b24ba147ce6eda4`. All 597 files in `specs/`,
`examples/`, `rust/fslc/tests/fixtures/` have identical full outputs/exits
(400 exit 0, 191 exit 2, 6 exit 1), corpus `ok`→`error` set empty. Separately,
42 inline sources × direct/compose = 84 forms have 38 `ok`→`error` transitions:
original 13 cases × 2, N2 × 2, two static-subset cases × 2 and three post-loop
coverage cases × 2, and no other output changes. The
[removal/migration table](../../../docs/design/DESIGN-bridge.md#assigned-root-read-order-1451)
also covers the added subset/post-loop cases; completeness refers only to the
measured inline fixture set, not all removed programs. Assign before reading or initialize the
whole root; choosing formerly free values is a model-strengthening decision.

- Assignment: `x = e`, `m[k] = e`, `m[k].f = e`, `o.f = e`, `o.f = some(e)`
- In `init`, disjoint struct-root or map-entry fields may be assigned separately
  (also in finite binder-keyed `forall`); overlapping writes by different
  statements on a path are rejected, including whole-root/entry versus field.
  Field types remain scalar or nested `Option<scalar>`. One `forall` statement
  may write the same location across iterations with agreeing values; conflicting
  values make init unsatisfiable, not an ownership error. Unresolved keys
  (dynamic, `where`-filtered, collection, or non-constant range binder) own the
  whole root and conflict with any other statement writing it.
  Shapes requiring the new field granularity need complete definite coverage
  of every root init may assign on all paths, including whole-value writes and
  all Map keys/fields; never-assigned roots remain free. Incomplete assigned
  roots retain the legacy ownership error. All legacy-probe-accepted partial
  forms keep symbolic free components, including single-field/branch-split
  writes, key subsets and one-path whole-root writes. Refine refuses incomplete
  assigned roots on either side instead of defaulting missing components (#1521);
  bmc and induction retain free-value verdicts. Partial-component enumeration is
  future work (#1579). Explicit/Monitor require all fields of every entry
  to be covered before reading the root or completing init. Resolved entry-field
  writes now count toward concrete coverage even in legacy-accepted shapes:
  complete single-field-entry maps become constructible; retained incomplete
  coverage adds the partial-component suffix to missing-assignment errors.
  Deterministic refine on either side, auto selection, conformance, testgen and
  requirement-trace gates inherit these construction changes. Auto can switch
  bounded BMC fallback to unbounded explicit closure; requirement trace execution
  can change check/proof/testgen results. Kernel replay/conformance alone does not
  validate declared acceptance traces. Full coverage may expose existing
  out-of-domain or repeated-write vacuity errors rather than missing assignment;
  the relational repeated-write constraint itself is unchanged.
  New admission proofs treat outer-binder-dependent sources/filters/key bounds
  as unproved, never as same-named global consts; no outer valuation product is
  enumerated and #1515's symbolic shadow-resolution behavior is not repaired.
  For shapes newly admitted by per-field ownership, `check` also requires the
  entire may-assigned root (all Map keys/fields) to be covered before a read,
  including roots written only as whole values; early reads of non-field-written
  roots retain the legacy ownership error/location unless the field-read gate
  refuses first. When field-written roots are complete at the end, that gate
  runs before nonfield reads, even if the field read appears later in init;
  incomplete field-written roots instead retain the saved ownership refusal.
  The field-read gate also
  covers an already-written field or a whole-entry/root read of a field-written root
  (diagnostic markers `<whole entry>` / `<whole root>`).
  `s.a = 1; s.b = (s.a == 1)` is rejected, as is a
  current-key field read before sibling fields are written in a `forall`.
  Reads of written fields at the current binder key after all binder-keyed fields
  are written are allowed; a whole-entry copy in that body still fails #1518's
  read-order rule. A whole-entry copy outside the loop after full-root init is
  allowed. The semantics error names root/field and locates the containing statement; legacy
  root-granularity-accepted init shapes retain their ownership-check admission.
- Set/Seq/relation are re-assigned: `s = s.add(x)`, `q = q.pop().push(y)`,
  `r = r.add(a,b)` (chaining allowed)
- A relation-typed field's **literal** initializer must be the empty `Set {}` — a non-empty
  Set literal is rejected in both `init` and an inline initializer — and pairs are added with
  the reassignment idiom `r = r.add(a, b)`, which is also allowed inside `init`.
- `if expr { stmt... } [else { stmt... }]` is allowed in both `init` and action
  bodies (may nest with an if inside else)
- `forall x: T { stmt... }` (bulk assignment)

An init statement's collection `forall` source cannot contain free state
references, even after that state was assigned, for empty domains/bodies, or
in unreachable branches. Rejecting these previously check-ok forms is a
breaking removal (#1463). State-independent literals remain allowed; ordinary
assigned-state reads in init expressions/filters/bodies must also satisfy the
same-forall root-read boundary below (#1518). Binder source/bounds
use the outer scope; its filter/body/value uses the local scope. A local that
shadows a state name is not a state read; fixing that distinction removes the
old symbolic/explicit false refusal without narrowing check acceptance.

An init statement forall cannot read a root that the same forall writes,
including in its filter, assignment keys/RHS, or if conditions (#1518).
Exception: the current binder-key location already written in this iteration.
After `m[k] = value`, read `m[k]` or `m[k].flag`; after `m[k].flag = true`,
read only that field, not the entry or another field. Existing ownership rules
still apply. `forall k: K { m[k] = true seen[k] = m[k] }` is allowed;
substituting `m[1]` or reading before writing is rejected. Earlier-initialized
different roots remain readable. This is a **breaking removal** of previously
check-ok scalar-map and whole-entry forms. The semantics message identifies
the root, not the individual read expression; check/verify locate the containing
statement, legacy replay need not.

The removal deliberately includes conservative cases, without a per-key
dependency proof: `forall k in 0..0 { m[k] = true }` followed by
`forall k in 1..1 { m[k] = m[0] }` is rejected even though the preceding
forall initialized the other key, because the second forall writes the same
root. Likewise,
`forall outer in 0..0 { forall k: K { m[k] = true seen[k] = m[k] } }`
is rejected even after the inner write: the outer forall also writes `m`,
and the read is not at its binder key. Explicit and Monitor already refused
these forms on the base with `before it is assigned` and
`nested forall in init is not supported`, respectively. #1518 rejects them
at check too; the rule is not refined to admit them.

This root-based rule also removes correct forms whose base-engine results
agreed: with `m` indexed by `0..1`,
`m[0] = true forall k in 1..1 { m[k] = true x = m[0] }` reads an already
initialized other key, but is rejected because the loop writes the same root.

For concrete deterministic init, empty or not statically provable nonempty
forall domains add no definite coverage for fixed targets; ownership is still
checked. Static binder/constant filters can prove nonemptiness (for example
`where k == 0` over `0..1`), state-dependent filters cannot. Explicit/Monitor
reject missing assignments with exit 2 rather than using defaults. Symbolic
init leaves components free: type-bound/invariant violations exit 1, but all
admissible values satisfying a property can verify. Auto falls back to BMC for
this explicit limitation. Breaking-removal sweep (#1518), base
`28c4eab710f8d83e766bb0e7c7e006d09d7777d8` versus head
`9d94c5cb5447834f329002200afa0f3404d9a9a0` plus the production runtime change
(`explicit.rs` SHA-256
`b20b92e9945fd7a05eb8462c4a7ad3816fcd06cd2c34a871910b7b652090c17b`):
the coordinator built each binary in its own git-archive materialized tree and
ran `fslc check <form>` over all 597 `.fsl` forms under `specs/`, `examples/`,
and `rust/fslc/tests/fixtures/`. Both trees had 397 `ok`, **0 `ok`→`error`,
0 `error`→`ok`, 0 other changes**. The corpus removal set is empty; A1–A5 pin
five regression forms within this removal, which also includes the
conservative examples above.

A nested statement `forall` in `init` is supported by check and the symbolic
engines subject to the read-order and ownership rules above, but the explicit
engine rejects nested init forall. Use `--engine auto`
to route this documented explicit limitation to symbolic BMC; explicit support
is not extended and forbidden state-dependent collection sources stay rejected.

Breaking-removal sweep (#1463): base `5738cc2c711b691a9d453b0190ead1a2ff77fc50`
versus the #1463 change (exact measured head recorded in the PR), each
in its own materialized tree, ran `FSLC_CACHE=off fslc check <form>` over 597 corpus
forms + 1016 inline IDs = 1613, twice. Both runs exited 0 and the full reports
matched with no exclusions. There are 110 new input errors
(1 corpus, 109 inline); the complete fresh-base corpus includes both unchanged
legacy-return additions. Result other than `error` is acceptance, not necessarily
a satisfied refinement/trace verdict. Complete removal IDs, using Cartesian
brace expansion, are:

| Removed form ID set | Count |
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

These are 22 wrapper, 31 init-source, 51 relation-index (18 aggregate), and
6 empty-aggregate Bool-index removals. The #1463 source-typing decision checks
written aggregate binder/filter/value before normalization, even for empty literal
ranges and zero-capacity Seq: existing type errors cannot disappear.
Empty Count/Sum now reports every existing type error in the written expressions,
such as the non-Bool filter in `count(x in 1..0 where x + 1)`; the 6 Bool-index
removals are examples in the measured corpus/inline inputs, not an exhaustive
list of type-error kinds.
The Bool-index closure is an intentional #1459-class consequence; no new type
rules are added, and valid empty/nonempty Map aggregates retain their verdicts.
Raw validation does not expand inner Count/Sum before outer substitution or
prematurely reject a pattern name awaiting substitution. All 66 nested-aggregate/
pattern check controls preserve acceptance and complete base/head envelopes.
The 15 nested/sync inventory removals are only four quantifier relation-index bounds and
eleven sync-expanded/root relation-index forms, not those preservation controls.

The other 18 of 128 changed IDs remain error/semantics, exit 2:
`inline/1457/boundary/nonfinite` gains authored message/loc 1:161;
`inline/R2/seq-write-remains-rejected` keeps 7:17 but requires Map rather than
Map or Relation; `inline/aggregate-and-component-locations/case-{13,17,37,41,49,53,61,65}` changes synthetic
1:1 to authored 6:3 and Map/Relation/Seq repair text to Map/Seq;
`inline/aggregate-and-component-locations/case-{21,45,57,69}` changes only that text, keeping 6:3.
`inline/nested-aggregates-and-sync-ownership/case-{72,74}` already failed static aggregate bounds; the relation
index now reports property 6:3 instead of no location. For
`inline/nested-aggregates-and-sync-ownership/case-{73,75}`, the same existing failure anchors at use 2:3
and keeps the part.Capture cause at piece.fsl:6:3.
All other envelope fields are unchanged. Imported refusals anchor at their
owning root use, preserve component cause locations, and show public names
such as `part.xs` rather than `part__xs`; these are intended output changes.
Model-expression type errors now point to their owning declaration, including
errors whose base check envelope already had a loc; component type errors,
including pre-existing ones, anchor at the owning root use and retain their
component cause.
Inputs retain actual #1457, aggregate/component and nested/sync emissions, fixed acceptance and rewritten
guard/constructor forms. Remaining domain/range limits are #1480,
init/zero-capacity issues #1481, other typing gaps #1459, literal shapes #1467;
do not infer additional #1463 rejections for them.

## 5. Semantic rules

The shared model-expression checker requires `Bool` for the operand of `not`
and an `Int`-compatible type (including named numeric ranges) for unary `-`.
This applies in traces and every model expression, including unselected branches
and short-circuited `and` / `or`. `not 1` and enum `-Red` are check-time
`kind: "semantics"` errors, exit 2; they are not conversions. Well-typed
`not (c == Red)`, `-(-1)` and numeric `- n` remain valid.

Requirements `acceptance ... { ... expect <expr> }` and FSL 5 scenario
`then <expr>` require a recursively well-typed `Bool` expression before replay,
using the invariant checker for all operands and conditional branches. For enum
state `c: Color`, `c == Blue` is valid if `Blue` belongs to `Color`; `c != 1`
and `c == 1` are assignability errors (`kind: "semantics"`, exit 2), located at
the authored `expect`/`then` keyword. This is a post-state predicate, not an
invariant; a well-typed false expectation still reports `kind: "acceptance"`.
The readable `expect E id in Stage` and forbidden's `expect rejected` are unchanged.

Historical compatibility measurement (#1439): against base `5a539bbe`, all 579
forms per tree under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` were checked in
both gate modes, with no ok-to-error transition. See the
[design note](../../../docs/design/DESIGN-requirements-stage.md#measured-compatibility)
for the command and measured production identity.

Every acceptance/forbidden step argument is recursively checked against the
lowered model before replay, without an outer expected parameter type. This
includes setup/final calls, FSL 5 scenario/forbidden phrase `with` inputs,
particle inputs, marked/with hybrids and relation-role values. Unselected
conditional branches are checked. Internal mismatches use the existing
assignability message, `kind: "semantics"`, exit 2, at the first authored token of the step
(action name for calls, actor for actor-first phrases, or the first token of a
reordered particle phrase, for example its instance). Internally well-typed `paint(1)` and
`paint(if true then 1 else 2)` retain the forbidden `wrong_kind_argument`
diagnostic; values-scope-only skips are unchanged.

Extension measurement against base `5a539bbe`: 579 corpus forms plus all 21
inline fixtures of the lint-rewritten trace-step test per tree, both modes
twice (4,800 checks). Corpus transitions are empty. Inline forms T01–T16 below are
**breaking removals**, `ok` / 0 → `error` / `semantics` / 2. T01–T03, T13 and
T16 transition in both modes, the others only in FSL 5: 21 mode/form transitions,
no other full-output/exit or repeat differences, no exclusions. IDs identify
the exact test sources in the
[extension design note](../../../docs/design/DESIGN-requirements-stage.md#extension-compatibility-measurement-and-breaking-removals).

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

Round 1 additionally checks all sources of the two rewritten diagnostic tests
and their changed-oracle consumers, plus scope/reordered-particle controls:
20 forms per tree, both modes twice, 160 checks against the same base and
unchanged production code. T17–T20 are additional breaking removals in both
modes; T21 only in FSL 5 (five fixture occurrences / nine mode-form transitions,
no other differences or exclusions). T17/T18 are identical FSL in two tests.
T19/T20 report `public Kernel cannot type identifier 'z'`: an `is some(z)`
binding is limited to its true logical continuation (`=>` / `and`), not a
conditional's `then` arm, even if unselected. Invariants already reject that
scope escape; move the use into a supported continuation or avoid the escaped
name. T21's diagnostic starts at `claim1` (8:5), not the later actor. See the
design note for exact identities and inventory.

Unary operand measurement (#1439): base `5a539bbe` and the final implementation,
each in its own tree, both modes twice, 579 corpus forms plus 51 inline fixture
occurrences per tree: 5,040 checks. The corpus ok-to-error set is empty. T01–T21
remain the earlier removals. U01–U07 below are additional **breaking removals**,
each `ok` / 0 → `error` / `semantics` / 2 in both modes (14 new mode/form
transitions, 44 total). Full stdout, stderr and exits have no other differences,
repeat differences or exclusions. See
[unary operand typing](../../../docs/design/DESIGN-requirements-stage.md#unary-operand-typing-1439)
for commands and exact identities.

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
skipping evaluation cannot legalize an ill-typed operand.

Current rebase measurement (#1439): BASE
`23eb5e41326ef4cf81a100ad0e011e81ed9fd507`, HEAD
`1e27d896b69b395ad8e48c570b4ddbca50e442a0`, each built in its own tree.
`BINARY check FORM` covers 594 corpus forms plus 65 inline occurrences per tree,
both modes twice (5,272 checks), with an empty corpus ok-to-error set. Earlier
counts above are historical. The historical 51-inline subset retains its 44
mode/form removals. Complete coverage of all five fixed test files additionally
records acceptance `expect` / FSL 5 `then` predicates `c != 1`,
`true and c != 1`, `not (c == 1)`, `if c != 1 then true else false` and
`forall color: Color { color != 1 }` as **breaking removals** for enum `c: Color`.
Acceptance forms transition in both modes, scenarios only in FSL 5: 15 additional
mode/form removals, 59 total. Use a same-enum member instead of the integer.
The corresponding `c == 1` fixtures remain errors but change their diagnostic from
`acceptance` to `semantics` at the expectation keyword (three mode/form changes,
exit 2 unchanged). These account for all 62 full-output differences; there are no others,
repeat differences or exclusions. See the
[current design measurement](../../../docs/design/DESIGN-requirements-stage.md#rebase-compatibility-measurement-on-23eb5e41)
for inventory commands and the twice-calibrated S1 detector.

Repair the comparisons using members of the same enum, including unselected
branches. A guard refusal does not legitimize an ill-typed argument expression.

1. One step = one action instance (name × parameters) executes atomically.
2. **Simultaneous assignment**: every RHS in the body reads the old state.
   Unassigned variables are unchanged (automatic framing).
3. **Double assignment = semantics error**: assigning twice to the same
   variable/field on the same path. then/else are separate paths (assigning in both
   is allowed). Assigning to the same variable **after an if** as inside a branch is
   also an error.
   **Proven duplicate writes** (for example `m[0]` twice or `forall c { m[0] = ... }`)
   keep the legacy duplicate-write message. **Conservative rejections** for unproved
   `forall` index distinctness (for example `forall c { m[BASE + c] = ... }`) report
   `cannot prove write-index distinctness across forall iterations` with
   `FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED`, `loc`, and a safe `hint` when one
   exists. Acceptance is not widened: the checked Kernel model still rejects before
   any verifier backend runs.
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
   `init` uses the action-context rules: a reached `/`/`%` by zero, `Seq` partial
   operation, `Map` key outside its domain, or i64 overflow in init is a semantics
   error (exit 2) under every engine, e.g. `division by zero in init at 9:5` from
   `bmc`/`induction` — not a `partial_op` violation. Guard it with an init `if`.
   Exception: if init also assigns a value outside a state's type, `bmc`/`induction`
   report the step-0 `type_bound` violation (exit 1) instead.
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
   Helpful argument expressions are checked against action parameter types in
   the property's binder scope; previously unchecked type/name/pattern errors
   now reject at `check`. Normalized Count/Sum retain standalone acceptance
   (including valid empty domains), and compose preserves original-global
   refusals without adding foreign-name refusals.
   Induction additionally requires state-independent helpful arguments: leadsTo
   binders, constants, enum members, integer/Boolean literals, and integer `+`, `-`, `*` (including unary `-`).
   `check` and BMC accept well-typed state reads; induction refuses them.
   - **Placement**: `decreases` is a sibling of the forall wrapper, *outside*
     its braces — `leadsTo L { forall c: Case { P ~> Q } decreases M }`.
     Nesting it inside the forall body is a **parse error** — `fslc`
     reports `expected '}'` at the `decreases` token, with no placement
     hint — not "ranking doesn't work under forall".
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
does not change assurance. Generated-only sentinels remain in raw coverage
(`covered:false` included), but not in public warnings, ledger coverage findings,
HTML badges/coverage denominators, or mutate dead-action notes. The current mutate
CLI cannot reach a generated-only dead-action case; its notes nevertheless use
the same eligibility classifier. Eligibility uses the authored primary
origin's positive line/column, then the kernel action's positive source span,
never a name/prefix test. HTML Raw Data retains the original map; zero eligible
actions render `0/0`. Corrected ledger/HTML renderings invalidate old artifact
approval digests (`drifted`, exit 0); review and re-approve them.

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
// element-wise invariant over a Seq (member quantification; no index arithmetic)
invariant I { forall j in q { jobs[j].st == Queued } }
// position, order, or no-duplicates need the index (guarded, range from the const): see NoDup below
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
