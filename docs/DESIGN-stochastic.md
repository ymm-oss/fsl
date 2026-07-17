# FSL Stochastic Evidence Layer Design

Status: adopted and implemented. This document fixes issue #139 and defines
the implemented `fslc ai eval` precomputed-evidence checker.

## Decision

`fsl-stochastic` is an external statistical evidence layer, not a kernel
dialect. It does not change `fslc verify`, `proved`, or `verified`. The
implemented layer runs a deterministic checker over precomputed eval JSONL via
`fslc ai eval` and produces JSON results whose `formal_result` is always
`"not_run"`.

The implemented statistical checker supports Bernoulli/proportion metrics:

- accuracy
- recall
- hallucination_rate
- wrong_tool_call_rate
- equivalent 0/1 pass/fail metrics

The confidence interval method is Wilson. A statistical result may be
`statistically_supported`, but it is never a formal proof and must not be
displayed as `proved` or `verified`.

## Guarantee Boundary

| Result class | Meaning | May be shown as `proved`? |
|---|---|---|
| `proved` / `verified` | Kernel safety/liveness facts over finite FSL state | yes, when returned by `fslc verify` / induction |
| `verified_under_assumptions` | Dialect hard-contract compatibility under explicit finite assumptions | no bare `proved`, but formal kernel evidence may be included |
| `replay_conformant` / `observed_mismatch` | Runtime log evidence | no |
| `statistically_supported` | Dataset/slice/evaluator evidence supports a threshold by Wilson bound | no |

Successful statistical results include assumptions such as fixed dataset,
sample-independence not proved by fslc, and evaluator calibration supplied by
separate evidence.

## Input: Precomputed Eval JSONL

The checker reads precomputed JSONL. It does not run an AI model, call an
evaluator, sample providers, or mutate prompts. Each JSONL line is one
Bernoulli observation for one metric/slice/case. The schema is
`schemas/fslc/ai/eval-record.v0.schema.json`.

Minimal line shape:

```json
{"schema_version":"fsl-ai-eval-record.v0","case_id":"support-001","component":"SupportAnswerAgent","dataset":"SupportEvalV3","slice":"all","metric":"accuracy","outcome":true,"evaluator":{"id":"gold_labels_v3","calibration_status":"trusted"}}
```

Dataset validity rules:

- `case_id`, `slice`, `metric`, `outcome`, and `evaluator.id` are required.
- A missing required slice field is `dataset_invalid`.
- Duplicate `(case_id, slice, metric)` records are `dataset_invalid`.
- `outcome` is boolean. Continuous scores and free-form evaluator rationales are
  intentionally outside this checker.
- Raw prompts, answers, documents, secrets, and production payloads are not
  required result fields; evidence artifacts should keep identifiers and
  aggregate counts only.

## Result Status Priority

An implementation must decide status in this order:

1. `dataset_invalid`: schema invalid, duplicate case id for a slice/metric, or a
   required slice field is missing.
2. `evaluator_untrusted`: evaluator calibration evidence is missing or below the
   declared trust threshold.
3. `insufficient_samples`: any required slice has `n < min_samples`.
4. `inconclusive`: the declared metric or interval method cannot be computed.
5. `statistically_unsupported`: there are enough samples, but the Wilson bound
   does not support the threshold.
6. `statistically_supported`: every required gate and bound check passes.

`result` and `status` use the same vocabulary in the result schema
(`schemas/fslc/ai/statistical-result.v0.schema.json`) so consumers can route
without interpreting a separate proof status.

## Wilson Threshold Rules

For `n` Bernoulli observations and `k` successes, `estimate = k / n`. The Wilson
interval at confidence `c` uses the standard normal quantile `z` for `c`:

```text
center = (estimate + z^2 / (2n)) / (1 + z^2 / n)
margin = z / (1 + z^2 / n) * sqrt(estimate * (1 - estimate) / n + z^2 / (4n^2))
lower = center - margin
upper = center + margin
```

Properties must use one of these bound forms:

- `ci_lower(metric, 0.95) >= T`
- `ci_upper(metric, 0.95) <= T`

Examples:

- `ci_lower(accuracy, 0.95) >= 0.92` is supported only when the Wilson lower
  bound is at least `0.92`.
- `ci_upper(hallucination_rate, 0.95) <= 0.03` is supported only when the Wilson
  upper bound is at most `0.03`.

A point-estimate-only property such as `accuracy >= 0.92` is a check error, not
a warning. The checker must reject it before producing statistical support.

## Multiple Slices

Each declared slice is an independent gate. A result is
`statistically_supported` only when every required slice passes its own
`min_samples` and bound check.

The checker does not provide a family-wise guarantee and does not automatically
apply multiple-testing correction. When a job declares more than one slice,
results should include an assumption or warning stating that family-wise error
control is not claimed by this layer.

## Required Result Fields

The result schema requires:

- `result`
- `status`
- `formal_result`
- `dataset`
- `slice`
- `metric`
- `n`
- `estimate`
- `interval.method`
- `interval.confidence`
- `interval.lower`
- `interval.upper`
- `threshold`
- `evaluator`
- `assumptions`
- `findings`

`formal_result` is always `"not_run"` for this layer.

## Relationship To AI Compatibility

Statistical quality evidence is separate from multi-environment compatibility.
AI model, prompt, retriever, tool schema, and output schema compatibility belong
to the `dbsystem` artifact/environment model as finite capability profiles
(`requires` / `provides`). See `docs/DESIGN-db.md`.

`ai_migration.no_regression` is implemented as aggregate precomputed-metric
comparison via `fslc ai regress`; paired case-level comparison can be supplied
as stricter external evidence but is not required by this layer.

Runtime telemetry drift is implemented separately via `fslc ai drift` over
JSON/JSONL records. It emits observed evidence, not statistical proof.

## Outside This Layer

- bootstrap or arbitrary-metric intervals
- mandatory paired migration regression semantics
- automatic multiple-testing correction
- LLM evaluator execution
- provider sampling distribution estimation
- stochastic semantics inside `fslc verify`
- automatic prompt/model/retriever migration execution
