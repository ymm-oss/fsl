# fsl-ai hard-contract MVP examples

This directory exercises the Phase 1 `ai_component` hard-contract dialect.

Run:

```bash
fslc ai check examples/ai/refund_agent_tool_safety.fsl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_conformant.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_human_approval_bypass.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_forbidden_tool.jsonl
fslc ai replay examples/ai/refund_agent_tool_safety.fsl --logs examples/ai/runtime_observed_mismatch.jsonl
```

The first replay is `replay_conformant`. The others return AI-readable findings:

- `ai_hard_contract_violation` for human approval bypass and forbidden tool execution.
- `observed_contract_violation` when runtime logs show a tool outside the declared component boundary.

Replay evidence is runtime observation, not formal proof. `fslc ai check` lowers
the hard-contract authority model to the existing kernel and returns
`verified_under_assumptions` when the finite hard-contract expansion verifies.
