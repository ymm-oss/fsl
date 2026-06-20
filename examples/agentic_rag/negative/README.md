# Agentic RAG negative probes

このディレクトリは、正常仕様ではなく「失敗するべき設計」を置く場所である。
目的は、3層FSLの境界が実際に効いているかを、期待失敗で確認すること。

## Probes

| Probe | Command | Expected |
|---|---|---|
| guard迂回 | `fslc refine examples/agentic_rag/negative/guard_bypass_design.fsl examples/agentic_rag/agentic_rag_requirements.fsl examples/agentic_rag/negative/guard_bypass_refines_requirements.fsl --depth 6` | `refinement_failed`, `abs_requires_failed` |
| tool承認迂回 | `fslc refine examples/agentic_rag/negative/tool_approval_bypass_design.fsl examples/agentic_rag/agentic_rag_requirements.fsl examples/agentic_rag/negative/tool_approval_bypass_refines_requirements.fsl --depth 6` | `refinement_failed`, `abs_requires_failed` |
| liveness喪失 | `fslc refine examples/agentic_rag/negative/liveness_drop_design.fsl examples/agentic_rag/negative/liveness_requirements.fsl examples/agentic_rag/negative/liveness_drop_refines_requirements.fsl --depth 4` | `refinement_failed`, `progress_lost`, `leadsTo` |

## Reading Notes

- guard迂回は、`Drafted + guard Unchecked` のまま `answer` に対応させる。
  requirements層の `answer` requires が破れるので、refinementが拒否する。
- tool承認迂回は、`ToolApproval + approval Requested` のまま `execute_tool` に
  対応させる。requirements層の `execute_tool` requires が破れる。
- liveness喪失は、回答準備済み境界では安全性のaction対応を守るが、
  `heartbeat` がstutterし続ける。`liveness_requirements.fsl` の
  `RequestEventuallyHandled` をdesign実行へ引き戻すことで、progress喪失として検出する。
