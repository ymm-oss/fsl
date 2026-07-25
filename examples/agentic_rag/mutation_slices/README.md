# Agentic RAG mutation slices

このディレクトリは、`agentic_rag_requirements.fsl`全体をそのまま`mutate`すると重い問題を避けるための、用途別の小さなrequirements sliceである。

## Files

| File | Focus |
|---|---|
| `answer_safety_slice.fsl` | 最終回答の evidence / citation / guard 契約 |
| `tool_approval_slice.fsl` | 副作用toolの operator / approval / ToolApproved stage 契約 |
| `retry_liveness_slice.fsl` | 証拠不足時の retry budget と終端liveness。`within`/`until`の新構文を使用 |
| `backported_constraints_slice.fsl` | 本体requirementsへ戻した状態同期/trans/forbidden制約のkill寄与確認 |
| `SURVIVOR_REVIEW.md` | survivor分類と本体requirementsへ戻した制約の台帳 |

## Commands

```bash
fslc check examples/agentic_rag/mutation_slices/answer_safety_slice.fsl
fslc check examples/agentic_rag/mutation_slices/tool_approval_slice.fsl
fslc check examples/agentic_rag/mutation_slices/retry_liveness_slice.fsl
fslc check examples/agentic_rag/mutation_slices/backported_constraints_slice.fsl

fslc verify examples/agentic_rag/mutation_slices/answer_safety_slice.fsl \
  --depth 6 --deadlock ignore

fslc verify examples/agentic_rag/mutation_slices/tool_approval_slice.fsl \
  --depth 7 --deadlock ignore

fslc verify examples/agentic_rag/mutation_slices/retry_liveness_slice.fsl \
  --depth 8 --deadlock ignore

fslc verify examples/agentic_rag/mutation_slices/backported_constraints_slice.fsl \
  --depth 7 --deadlock ignore

fslc mutate examples/agentic_rag/mutation_slices/answer_safety_slice.fsl \
  --depth 6 --by-requirement --max-mutants 80

fslc mutate examples/agentic_rag/mutation_slices/tool_approval_slice.fsl \
  --depth 7 --by-requirement --max-mutants 100

fslc mutate examples/agentic_rag/mutation_slices/retry_liveness_slice.fsl \
  --depth 8 --by-requirement --max-mutants 100

fslc mutate examples/agentic_rag/mutation_slices/backported_constraints_slice.fsl \
  --depth 7 --by-requirement --max-mutants 180
```

## Observed Results

2026-07-25時点、権威実装（native `fslc`、`rust/target/debug/fslc`）での実測値
（issue #485。それ以前の版は frozen Python `python -m fslc` の実測値で、
`backported_constraints_slice.fsl`はnativeでは`check`が
requirement注釈のtext衝突でexit 2になり、この表自体が再現不能だった。同不具合は
`REQ-3`/`REQ-4`/`REQ-5`ブロック内の重複legacy注釈テキストを削除して修正済み）。
mutation engineの列挙順・演算子集合がPython参照実装とnativeで異なるため、
killed/survivedの絶対数はPython実測値と一致しない。

| Slice | Verify | Mutate summary |
|---|---|---|
| `answer_safety_slice.fsl` | `verified` at depth 6 | `80 total / 64 killed / 16 survived` |
| `tool_approval_slice.fsl` | `verified` at depth 7 | `100 total / 67 killed / 33 survived` |
| `retry_liveness_slice.fsl` | `verified` at depth 8 | `95 total / 73 killed / 22 survived` |
| `backported_constraints_slice.fsl` | `verified` at depth 7 | `180 total / 161 killed / 19 survived` |

`backported_constraints_slice.fsl`では、以下の本体へ戻した制約が直接killしている。

| Property | Kills |
|---|---:|
| `EvidenceReadyHasEvidenceAndCitation` | 8 |
| `DraftedHasEvidenceAndCitation` | 8 |
| `EvidenceBadMeansNoCitation` | 6 |
| `GuardFailedMeansFailedGuard` | 6 |
| `ToolApprovalMeansRequested` | 5 |
| `ToolApprovedMeansApproved` | 5 |
| `ExecutionComesFromApprovedStage` | 4 |
| `RetryConsumesOneBudget` | 4 |

## Reading Notes

- survivorは失敗ではなく、レビューキューである。
- `retry_liveness_slice.fsl`では、`within 6`が証拠不足pathのstep上限を、
  `until`が終端まで処理中状態を保つ制約と進捗義務を表す。
- sliceで使っていないフィールドや、意図的に同値になる変異はsurviveし得る。
- `by_requirement`のkill数は、そのmutation集合とdepthにおける下限として読む。
  invariant、acceptance、forbidden、reachableが先にkillした変異は、requirement別killへ
  直接集計されないことがある。
- 本体requirementsへ反映すべき候補は、複数sliceで同じ穴として見えるもの、または本体のnegative probeでも再現するものに絞る。
- survivor分類の判断は`SURVIVOR_REVIEW.md`へ残す。
