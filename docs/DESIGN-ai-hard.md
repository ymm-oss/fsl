# FSL AI Hard-Contract Dialect Design

Status: adopted for Phase 1 MVP. This document cuts the deterministic
hard-contract slice out of `docs/fsl_ai_stochastic_proposal.md`.

## Decision

`ai_component` is a frontend dialect, not a new verifier kernel. Phase 1 models
AI components only at the observable tool-boundary:

- declared model/prompt/input/output metadata
- declared tools and their schema names
- symbolic business preconditions for tool execution
- authority (`may_suggest`, `may_execute`, `requires_human_approval`, `forbidden`)
- fallback declarations
- runtime event replay for hard-contract findings

The dialect lowers the hard-contract authority model to the existing kernel:

- `Tool` enum
- `human_approved: Map<Tool, Bool>`
- `tool_executed: Map<Tool, Bool>`
- `tool_suggested: Map<Tool, Bool>`
- `fallback_required: Bool`
- generated `suggest_*`, `approve_*`, `execute_*`, and `fallback_*` actions
- generated invariants for forbidden tools and approval-before-execution

No probability, percentile, evaluator scoring, model-distribution, or stochastic
semantics are added to the kernel. Runtime replay is observation evidence, not
formal proof.

`fallback` declarations are structural only in Phase 1: each `when <reason>
require <target>` lowers to a `fallback_<reason>` action that sets the ghost
`fallback_required: Bool`, but — unlike forbidden tools and approval-before-
execution — **no invariant is generated over it**, because `target` is a free
label with no corresponding tool/action in the grammar, so "the target was
actually taken" is not expressible in the kernel yet. `fallback_required` is
kept as observable state for `fslc explain`/`fslc html` and as a hook for a
future phase that ties `target` to a real action; it is not proof that any
fallback routing happens. Each `reason` must be unique per component
(`validate_ai_component`) since two entries sharing a reason would collide on
the same generated action name.

## Guarantee Boundary

| Contract class | Examples | Phase 1 handling | Result vocabulary |
|---|---|---|---|
| Syntactic / structural hard | declared tool schema, enum-like authority, forbidden tool, human approval token, symbolic precondition evidence | static check, kernel expansion, runtime guard/replay finding | `verified_under_assumptions`, `violated`, `ai_hard_contract_violation` |
| Evaluator-backed hard-like | groundedness, source support, prompt-injection-following judgment, instruction hierarchy compliance | out of MVP; must be explicit external evidence | `evaluator_supported`, never `proved` |
| Statistical | accuracy, recall, hallucination rate, slice metrics, confidence intervals | external stochastic evidence layer | `statistically_supported` / `statistically_unsupported`, never `proved` |
| Observed | undeclared tool observed, schema drift in logs, production mismatch | `fslc ai replay` evidence only | `replay_conformant`, `replay_nonconformant`, `observed_contract_violation` |
| Environment compatibility | model/prompt/retriever/tool-schema/output-schema coexistence with server/mobile/DB artifacts | shared `dbsystem` artifact capabilities | `verified_under_assumptions` / `required_capability_missing` |

Prompt injection and RAG groundedness may be called `hard` only when the checked
predicate is structural and guard-backed, for example "no tool call executes when
the event marks `schema_valid=false`" or "a citation ID is in a declared finite
allow-list." A semantic claim such as "the answer is supported by the source"
requires an evaluator and cannot be displayed as a formal proof.

## Syntax

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

  tool CreateDraft {
    schema CreateDraftV1;
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
    may_suggest CreateDraft;
    may_execute SearchOrder;
    requires_human_approval RefundPayment;
    forbidden DeleteCustomerData;
  }

  fallback {
    when low_confidence require human_review;
  }

  check hard {
    rule tool_authority;
    rule human_approval_required;
    rule forbidden_tool_blocked;
    rule tool_schema_declared;
    rule tool_precondition_declared;
  }
}
```

If `check hard` is omitted, the default rule set is the five rules shown above.
Every authority reference must name a declared `tool`.

## Static Rules

- `tool_authority`: a forbidden tool cannot also be executable.
- `human_approval_required`: an `irreversible` executable tool must be in
  `requires_human_approval`.
- `forbidden_tool_blocked`: generated kernel state has no executable transition
  that can make a forbidden tool's `tool_executed` entry true.
- `tool_schema_declared`: executable tools must declare a schema name.
- `tool_precondition_declared`: declared symbolic preconditions are checked
  during replay as business precondition evidence, distinct from tool schema
  conformance.

## CLI

`fslc check` and `fslc verify` accept `ai_component` because it expands to a
kernel spec. `fslc check` also accepts recursive `agent` files for parse and
structural validation, but `agent` does not lower to the kernel. Use
`fslc ai check` for AI-specific assumptions and findings:

```bash
fslc ai check examples/ai/refund_agent_tool_safety.fsl
fslc ai check examples/ai/refund_agent_tool_safety.fsl --engine induction
fslc ai check examples/ai/recursive_support_agent.fsl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_conformant.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_human_approval_bypass.jsonl
```

Successful `ai_component` checks return `verified_under_assumptions`;
successful recursive `agent` checks return `agent_analyzed` with
`formal_result: "not_run"`; successful replay returns `replay_conformant` with
`formal_result: "not_run"`.

## Recursive Agent Composition

`agent` is the recursively composable fsl-ai structure. A nested agent is not a
separate `sub_agent` entity type; it is a normal agent inside its parent's
lexical namespace, for example `SupportOrchestrator.RetrievalAgent`.

Implemented structural syntax:

```fsl
agent SupportOrchestrator {
  context [CustomerTicket, ApprovedSupportDocs];
  tools [SearchDocs, CheckPolicy, CreateDraft];
  authority {
    may_execute [SearchDocs, CheckPolicy, CreateDraft];
  }

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

The analyzer separates these graphs:

- lexical scope tree (`agent Parent { agent Child { ... } }`)
- delegation graph (`orchestration { A -> B; }`)
- authority/context grant graph (`grant authority`, `grant context`)
- information-flow graph (`output X visibility ...`)
- tool-reachability graph (`tools`, `tool ...`, `authority`)
- failure propagation entries (`failure_policy`)

Rules enforced as stable semantics:

- top-level agents declare authority/context directly; nested agents receive
  them through explicit grants.
- child `grant authority` and `grant context` must be subsets of the immediate
  parent boundary; exceeding grants are semantics errors.
- child tool/authority/context use outside its grants reports
  `child_authority_exceeds_parent_authority` or
  `child_context_exceeds_parent_context`.
- sibling visibility without a delegation path reports
  `visibility_leak_across_sibling_agents`.
- low-trust paths to high-authority tools report
  `low_trust_agent_path_to_high_authority_tool`.
- irreversible tools without `requires_human_approval` report
  `irreversible_operation_without_human_approval_path`.
- declared `review_gate` bypass on a path to high-authority tooling reports
  `policy_review_bypass_in_orchestration`.

Recursive `agent` analysis is structural evidence, not formal proof. It does
not prove LLM semantic correctness, evaluator judgments, or statistical quality.

## Event Replay

The MVP event stream is JSONL, or JSON `{ "events": [...] }`.

```json
{"event":"human_approval","component":"RefundAgentToolSafety","tool":"RefundPayment","approval_id":"redacted"}
{"event":"tool_call","component":"RefundAgentToolSafety","tool":"RefundPayment","mode":"execute","tool_schema":"RefundPaymentV1","schema_valid":true,"preconditions":{"order_paid":true,"amount_refundable":true},"args":{"order_id":"redacted","amount":"redacted"}}
```

Replay detects:

- forbidden tool execution
- execution outside authority
- irreversible/approval-required execution before a `human_approval` event
- schema invalidity (`schema_valid: false`)
- declared schema vs observed schema mismatch
- business precondition mismatch
- undeclared tool or component mismatch observed in logs

## Finding Contract

The stable finding schema is `fsl-ai-finding.v0` (see
`schemas/fslc/ai/finding.v0.schema.json`). Required fields:

- `fsl`: `fsl-ai-hard-mvp.v0` or `fsl-ai-agent-mvp.v0`
- `result`
- `kind`
- `severity`
- `component`
- `contract`
- `tool`
- `failed_rule`
- `violation`
- `guarantee_kind`
- `evidence`
- `witness`
- `minimal_conflict_set`
- `repair_candidates`
- `assumptions`

`guarantee_kind` is the key boundary marker. Phase 1 emits
`syntactic_hard` for hard-contract violations, `runtime_observed` for log
mismatches, and `agent_structural` for recursive-agent graph findings. Future
evaluator/statistical findings must use
`evaluator_supported` or `statistically_supported` and must not be reported as
formal proof.

## Relationship To Compatibility And Statistical Evidence

`ai_component` is the hard-contract checker for tool authority, approval, and
runtime replay. It is not the environment compatibility checker. AI model,
prompt, retriever, tool schema, and output schema compatibility use the shared
`dbsystem` artifact/environment model:

```fsl
artifact support_agent_v8 {
  requires tool.RefundPaymentV2, retriever.SupportDocsV14;
  provides output.AnswerSchemaV2;
}
```

`fslc db check` evaluates those finite capability profiles in the same
environment/schema/flag snapshots as DB/API/mobile/server artifacts and reports
`required_capability_missing` for provider gaps.

Statistical quality is also outside this hard-contract dialect. The MVP
stochastic evidence layer reads precomputed eval JSONL, supports only
Bernoulli/proportion metrics with Wilson intervals, and returns
`formal_result: "not_run"`; see `docs/DESIGN-stochastic.md`.

## Assumptions

Results carry explicit assumptions:

- `AI-ASSUME-CAPABILITY-DECLARATIONS`: declared tools and authority are complete.
- `AI-ASSUME-RUNTIME-GUARD`: hard contracts are enforced before external side
  effects occur.
- `AI-ASSUME-NO-PROBABILITY-IN-KERNEL`: Phase 1 adds no probability, percentile,
  or evaluator semantics to the kernel.
- `AI-ASSUME-OBSERVABILITY-COVERAGE`: replay logs are evidence only; absence
  from logs is not proof of unused behavior.

## Out of MVP

The following remain in later issues/phases:

- eval runner implementation beyond the external statistical result schema
- evaluator calibration and evaluator-backed contract support
- prompt/model/retriever/tool-schema migrations and no-regression checks
- production drift aggregation beyond event replay
- full agent contract-expression semantics beyond the structural
  `rule <Name>` contract metadata accepted by the recursive-agent parser
