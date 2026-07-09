# fsl-ai examples

This directory exercises the `ai_component` hard-contract dialect and the
recursive `agent` structural analyzer, plus the external stochastic,
migration-regression, drift, and compatibility-profile evidence commands.

Run:

```bash
fslc ai check examples/ai/refund_agent_tool_safety.fsl
fslc ai check examples/ai/recursive_support_agent.fsl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_conformant.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_human_approval_bypass.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_forbidden_tool.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_observed_mismatch.jsonl
fslc ai eval examples/ai/support_answer_quality.fsl --property LooseQuality
fslc ai regress examples/ai/support_answer_quality.fsl --migration PromptV7ToV8 --before-records examples/ai/support_eval_v7.jsonl --after-records examples/ai/support_eval_v8_regressed.jsonl
fslc ai compare --from examples/ai/support_eval_v7.jsonl --to examples/ai/support_eval_v8_regressed.jsonl --dataset SupportEvalV3
fslc ai drift examples/ai/support_answer_quality.fsl --logs examples/ai/runtime_drift_current.jsonl --baseline-logs examples/ai/runtime_drift_baseline.jsonl
fslc ai compat examples/ai/support_answer_quality.fsl --environment prod
```

The first replay is `replay_conformant`. The others return AI-readable findings:

- `ai_hard_contract_violation` for human approval bypass and forbidden tool execution.
- `observed_contract_violation` when runtime logs show a tool outside the declared component boundary.

Replay evidence is runtime observation, not formal proof. `fslc ai check` lowers
the hard-contract authority model to the existing kernel and returns
`verified_under_assumptions` when the finite hard-contract expansion verifies.

`recursive_support_agent.fsl` shows nested agents as ordinary scoped agents
(`SupportOrchestrator.RetrievalAgent`, etc.). `fslc ai check` returns
`agent_analyzed`, deterministic `agent_ir`, and separate graph summaries for
lexical scope, orchestration/delegation, authority/context grants, output
visibility, tool reachability, and failure policy. Recursive-agent analysis is
structural evidence with `formal_result: "not_run"`, not formal proof of LLM
semantic correctness.

Statistical evidence examples are external to the kernel and run through
`fslc ai eval`:

- `support_answer_quality.fsl` declares `dataset`, `evaluator`,
  `statistical_property`, `ai_migration`, and `observed_property` blocks.
- `support_eval_v3.jsonl` shows the precomputed eval JSONL record shape.
- `statistical_result_supported.json` shows a Wilson-bound result with
  `formal_result: "not_run"`.

AI environment compatibility examples live under `examples/db/` because
model/prompt/retriever/tool-schema/output-schema coexistence is checked through
the shared `dbsystem` artifact capability model.
