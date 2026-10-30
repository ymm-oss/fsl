# Cross-cutting dialects and advanced features

Contents: [DB](#database-and-multi-environment-compatibility),
[domain](#functional-ddd-and-async-effects), [AI hard](#ai-hard-contracts),
[agents](#recursive-agents), [external AI evidence](#ai-evidence-projects),
[other features](#other-shared-features).
These are separate dialects, not additional stages in business→requirements→design.
Use their supported native commands and inspect their own assurance boundaries.

The blocks below preserve **grammar templates** from the shared language catalog:
angle-bracket operands/alternatives/omissions must be replaced from a confirmed
source before checking. They are not complete examples. Do not infer that a
kernel/requirements header or glossary is legal in another dialect; check its
actual source with its native command. Edition migration diagnostics and accepted
forms are dialect-specific, not blanket text replacement.

## Database and multi-environment compatibility

```fsl-fragment
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
    requires <namespace>.<capability>, ...;
    provides <namespace>.<capability>, ...;
    calls api.<operation>, ...;
    accepts api.<operation>, ...;
    expects response.<field>, ...;
    responds response.<field>, ...;
    emits_offline api.<operation> ttl <finite_ticks>;
  }
  environment <env> {
    schema <lo>..<hi>;
    flag <flag> { <variants> } default <variant>;
    active <version> when schema <lo>..<hi> when flag <flag>=<variant>;
    supported <version> when schema <lo>..<hi>;
    may_exist <version> when schema <lo>..<hi>;
  }
  check compatibility {
    rule <known_rule>;
  }
}
```

The closed compatibility-rule vocabulary is:
all_active_reads_exist, all_active_writes_exist, removed_only_after_unused,
not_null_after_backfill, destructive_operations_annotated,
preservation_transforms_annotated, api_calls_accepted, api_responses_expected,
offline_payloads_accepted, artifact_capabilities_provided, data_preserved,
rollback_equivalent. Unknown names reject before artifact generation; no arbitrary
rule label adds semantics. Empty-column databases reject too. Environment schema
windows must be reachable in the migration plan.

```text
fslc db check FILE [--depth K] [--engine bmc|induction]
fslc db observe FILE --trace events.json
fslc db import INPUT [--source auto|sql|prisma] [--name NAME] [-o OUT]
fslc compat check FILE [--include-ai]
```

Checks lower finite DB lifecycle/compatibility obligations to kernel and report
stable fsl-db findings (`verified_under_assumptions` on success). Finite flags
report DB-ASSUME-FINITE-FLAG-STATE. Capability profiles can connect mobile/server,
model/prompt/retriever/tool/output-schema artifacts; missing providers are
required_capability_missing. Coexistence/TTL is finite snapshots/ticks, not
probabilistic rollout or wall-clock guarantees. No DB-engine locks/optimizer or
production-data completeness proof. Observe validates observation.v0 records and
event flag snapshots, reports observed_mismatch—not a formal violation.
Production-data/DB-engine evidence carries formal_result:not_run. Importers cover
SQL DDL and minimal Prisma, not arbitrary ORM semantics.

## Functional DDD and async effects

```fsl-fragment
domain <Name> {
  implementation_profile functional_ddd
  enum <Status> { <Pending>, <Approved>, <Cancelled> }
  aggregate <Order> {
    id <OrderId>
    state { <status>: <Status> = <Pending>; }
    command <Approve> {}
    event <ApprovedEvent> {}
    event <RequestedEvent> { <request_id>: <RequestId> }
    event <CompletedEvent> { <request_id>: <RequestId> }
    error <CannotApprove>
    decide <Approve> {
      requires <status> == <Pending>
      emits <ApprovedEvent>
    }
    evolve <ApprovedEvent> { <status> = <Approved> }
    invariant <NoLateApprove> { <status> == <Cancelled> -> not can(<Approve>) }
  }
  effect <Capture> {
    async
    irreversible
    idempotency_key <Order>.id
    correlation_id <RequestedEvent>.<request_id>
    handles <RequestedEvent>
    emits one_of [<Success>, <Failure>, <Timeout>]
    success_event <Success>
    failure_event <Failure>
    timeout_event <Timeout>
    retry { max_attempts 3 }
    timeout after 10m emits <Timeout>
    compensation { emits <Failure> }
  }
  saga <Fulfillment> {
    starts_on <ApprovedEvent>
    outbox <Outbox>
    inbox <Inbox>
    step <RequestPayment> {
      async
      emits <RequestedEvent>
      awaits one_of [<Success>, <Failure>, <Timeout>]
      timeout after 10m emits <Timeout>
    }
  }
}
```

Use enum for variants, range type for bounded numbers, not legacy union types.
Domain has its own expression forms (`X in [A,B]`, `can(Command)`, `->`); do
not mechanically apply kernel operator replacements. Expected field types resolve
bare enum members; unknown/ambiguous symbols/type errors retain source origins.

```text
fslc domain check FILE [--depth K] [--engine bmc|induction]
fslc domain analyze FILE
fslc domain expand FILE [-o OUT]
fslc domain generate FILE --target typescript|python|kotlin|swift|rust [-o DIR]
fslc domain testgen FILE [--target vitest] [--allow-unwired] [-o OUT]
fslc domain replay FILE --logs events.jsonl
```

Check reports domain findings plus nested kernel evidence, not production
exactly-once behavior. Expand exposes generated kernel; analyze/expand are checked
consumers, not raw invalid-AST inspection. Generate/testgen are scaffolds and use
Public Kernel v1 plus validated domain-scaffold-metadata.v1. Replay reports
conformance_checked/nonconformant over actual command/event/effect logs, not proof.
Source snapshot/origin chains keep diagnostics tied to domain declarations rather
than pretending generated names are authored.

The finite effect lifecycle includes status/attempt maps. Explicit success/failure/
timeout roles win over spelling; duplicate explicit roles reject. Unclassified
outcomes use the current timeout→fail→cancel→otherwise-success naming heuristic.
Event flags are one-hot/current-step, not sticky history. A compensation guarded
by different trigger/after event flags is structurally disabled today; report
never-enabled rather than weakening the conjunction. Saga history adds
DOMAIN-ASSUME-SAGA-OBSERVED-HISTORY. Queue delivery, external gateways and real-time
timeouts remain outside the model.

Prefer explicit aggregate initializers. Omitted defaults (false/0/first enum/
range lower bound/none/empty Set/value-object/dense top-level Map) report
implicit_initial_value; safe edits depend on round-tripping. Whole Map defaults
and nested Map state are unsupported. Do not insert a compiler-rendered default
that migration marks unsafe, or assume every warning has an automatic edit.

## AI hard contracts

```fsl-fragment
ai_component <Name> {
  model <model_id>;
  prompt <prompt_id>;
  retriever <retriever_id>;
  temperature <number>;
  input <InputSchema>;
  output <OutputSchema>;
  tools [<ToolName>, ...];
  tool <Tool> [irreversible] {
    schema <ToolSchema>;
    precondition <symbolic_business_precondition>;
    effect <Effect>;
  }
  authority {
    may_suggest <Tools>;
    may_execute <Tools>;
    requires_human_approval <Tools>;
    forbidden <Tools>;
  }
  fallback { when <condition> require <safe_target>; }
  check hard { rule <KnownRule>; }
}
```

Model/prompt/input/output identify profiles. Retriever/temperature and tool schema/
effect are at most once; tool preconditions repeat. Bare tools shorthand supplies
neither schema nor precondition. Fields use identifiers/numbers, not description
string slots; typed annotations are separate metadata.

```text
fslc ai check FILE [--depth K] [--engine bmc|induction]
fslc ai replay FILE --logs events.jsonl
```

Hard checks cover tool_authority, human_approval_required, forbidden_tool_blocked,
tool_schema_declared, tool_precondition_declared. Omit optional check hard to check
all five; an explicit block narrows to exactly listed known rules. Do not narrow
it to hide a violation. Unknown rules/tools, duplicate singleton fields, missing
required fields or malformed clauses reject. Tool authority, symbolic precondition
evidence, approval and fallback routing are checked, not LLM truth/groundedness or
statistical quality. Replay evidence (replay_conformant) is not a universal proof.

## Recursive agents

```fsl-fragment
agent <Parent> {
  model <model_id>;
  prompt <prompt_id>;
  context [<Context>, ...];
  tools [<Tool>, ...];
  tool <Tool> [irreversible] { schema <ToolSchema>; }
  authority { may_execute [<Tool>, ...]; }
  review_gate <Child>;
  agent <Child> {
    trust medium;
    grant authority [<Tool>, ...];
    grant context [<Context>, ...];
    tools [<Tool>, ...];
    authority { may_execute [<Tool>, ...]; }
    contract { hard { rule <Name>; } }
    output <Output> visibility [parent, <Sibling>];
  }
  orchestration { <Child> -> <OtherChild>; }
  failure_policy {
    when <Child>.failed -> retry up_to 2;
    when <Child>.failed_after_retry -> <ParentState>;
  }
}
```

Nested agents are scoped ordinary agents, not sub_agents. Lexical parentage does
not imply runtime collaboration or inherited grants; authority/context stay within
the immediate parent. Orchestration is a separate graph. Explicit review_gate
children mark paths that high-authority descendants must pass; undeclared review
intent is not mechanically inferred. Only literal trust low has a special check;
other free identifiers do not acquire invented trust semantics. Agent contract
hard rules are parsed/echoed metadata, not validated/cross-checked ai_component rules.

`ai check` returns agent_analyzed, agent_ir and graph summaries. Findings include
grant exceedance, low-trust authority reachability, irreversible tools without
approval, review bypass and sibling visibility. guarantee_kind:agent_structural
and formal_result:not_run are structural evidence, not verification of agents'
truth or evaluator quality.

## AI evidence projects

The project scanner is separate from the kernel and recognizes a deliberately
limited set of lines. Preserve this boundary; arbitrary content in recognized
but uninterpreted blocks constrains nothing.

```fsl-fragment
dataset <Name> {
  source "<eval.jsonl>"
}
evaluator <Name> { }
statistical_property <Name> {
  target <Component>
  dataset <Dataset>
  evaluator <Evaluator>
  confidence 0.95
  require ci_lower(metric.<metric>, 0.95) >= <Threshold>
  slice <Slice> {
    require min_samples >= <N>
    require ci_upper(metric.<metric>, 0.95) <= <Threshold>
  }
}
ai_migration <Name> {
  preserve {
    no_regression {
      dataset <Dataset>
      metric <metric> drop <= 0.05
      metric <metric> increase <= 0.02
    }
  }
}
observed_property <Name> {
  target <Component>
  source production_logs
  window last_7_days
  require observed(metric.<metric>) <= <Threshold>
  require drift(metric.<metric>) <= <Threshold> compared_to previous_7_days
}
```

Only dataset source/evaluator name and the recognized threshold/migration/observed
clauses are interpreted. Known threshold grammars are min_samples, ci_lower,
ci_upper, point estimate, observed and drift; a malformed require fails check.
Point-estimate-only eval is inconclusive/exit 1, not a statistical pass.
failure_mode is name-listed metadata, not evidence-checked content. ai_action,
ai_contract, standalone retriever, trust_boundary and named top-level authority
blocks are raw boundaries whose bodies are not validated; never author them as
if they enforce contracts. A declaration-listing ai_project_analyzed is not proof.

```text
fslc ai eval FILE [--records PATH] [--dataset NAME] [--slice NAME] [--property NAME]
fslc ai regress FILE [--migration NAME] --before-records PATH --after-records PATH [--dataset NAME]
fslc ai compare --from PATH --to PATH [--from-label LABEL] [--to-label LABEL] [--dataset NAME]
fslc ai drift FILE --logs PATH [--baseline-logs PATH] [--window N] [--baseline PATH] [--property NAME]
fslc ai compat FILE [--environment NAME]
```

Eval uses Wilson bounds over precomputed records. Status precedence is
dataset_invalid, evaluator_untrusted, insufficient_samples, inconclusive,
statistically_unsupported, statistically_supported. Regress gates aggregate metric
deltas; compare reports deltas without threshold claims. Drift evaluates observed
thresholds/drift, not formal behavior. Compat emits a DB artifact capability profile;
compat check --include-ai folds it into finite coexistence compatibility.
Every external-evidence result has formal_result:not_run. Never display it as
proved/verified or an individual-case guarantee.

## Other shared features

- [NFR](impl.md#11-non-functional-requirements-nfr): guards/ghosts/audit/capacity/
  fault-recovery and discrete tick deadlines, not real-time/probability claims.
- Seq aggregation: `sum(i: Idx of log.at(i) where i < log.size())`; Idx covers
  capacity, partial access must stay guarded.
- [Compose/refinement](syntax.md#compose-and-refinement): synchronization,
  local state, internal actions, explicit maps, enum conversion/abstraction,
  stutters and optional progress obligations.
- [Adapter](impl.md#9-implementation-connection-the-testgen-adapter-contract):
  reset/step/observe at implementation granularity, not generated-code truth.
- [Typestate](commands.md#analysis-and-typestate): derivable/branching/relational
  machines, local query self-loops and partial TypeScript scaffolding.
- [Causal sidecars](impl.md#12-the-causal-profile-review-only): hypotheses,
  evidence lifecycle, expectation checks/observation and portfolio ledger; no
  causal proof or invented verify command.
