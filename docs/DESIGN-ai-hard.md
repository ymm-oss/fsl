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
| Statistical | accuracy, recall, hallucination rate, slice metrics, confidence intervals | out of MVP | `statistically_supported` / `statistically_unsupported`, never `proved` |
| Observed | undeclared tool observed, schema drift in logs, production mismatch | `fslc ai replay` evidence only | `replay_conformant`, `replay_nonconformant`, `observed_contract_violation` |

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
kernel spec. Use `fslc ai check` for AI-specific assumptions and findings:

```bash
fslc ai check examples/ai/refund_agent_tool_safety.fsl
fslc ai check examples/ai/refund_agent_tool_safety.fsl --engine induction
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_conformant.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_human_approval_bypass.jsonl
```

Successful `ai check` returns `verified_under_assumptions`; successful replay
returns `replay_conformant` with `formal_result: "not_run"`.

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

- `fsl`: `fsl-ai-hard-mvp.v0`
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
`syntactic_hard` for hard-contract violations and `runtime_observed` for log
mismatches. Future evaluator/statistical findings must use
`evaluator_supported` or `statistically_supported` and must not be reported as
formal proof.

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

- datasets, slices, metrics, confidence intervals, and statistical properties
- evaluator calibration and evaluator-backed contract support
- prompt/model/retriever/tool-schema migrations and no-regression checks
- production drift aggregation beyond event replay
- multi-environment AI artifact compatibility with server/mobile/DB artifacts
