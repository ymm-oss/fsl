# Agentic RAG mutation survivor review

2026-06-20時点の`mutation_slices/`に対するsurvivor分類。

このメモは、`fslc mutate`のsurvivorをそのまま「失敗」と扱わず、本体requirementsへ戻すべき制約だけを選ぶためのレビュー台帳である。

## 判定基準

- **本体へ反映**: 状態名の意味、境界条件、遷移元/遷移先の契約としてrequirements層でも読めるもの。
- **slice内の観測補助**: fault actionや1件固定モデルの都合で出たもので、本体仕様の契約ではないもの。
- **同値または低優先**: 初期値を後続actionが必ず上書きする、ID範囲を広げても性質が変わらない、denial pathの詳細をまだ要求していない、など。

## 本体へ反映したもの

`agentic_rag_requirements.fsl`へ戻した制約:

| Source slice | Survivor / gap | 本体に戻した契約 |
|---|---|---|
| answer safety | `retrieve_success`相当のassignment欠落が後続action閉塞としてだけ見えやすい | `EvidenceReadyHasEvidenceAndCitation` |
| answer safety | 下書き中の証拠・引用前提がanswer直前にしか見えない | `DraftedHasEvidenceAndCitation` |
| answer safety / retry | 証拠不足状態と`evidence/citation_ok`の同期 | `EvidenceBadMeansNoCitation` |
| answer safety | `GuardFailed` stageと`guard == Failed`の同期 | `GuardFailedMeansFailedGuard` |
| tool approval | `ToolApproval` stageと`approval == Requested`の同期 | `ToolApprovalMeansRequested` |
| tool approval | `ToolApproved` stageと`approval == Approved`の同期 | `ToolApprovedMeansApproved` |
| tool approval | `ActionExecuted`へ承認待ちstageから直接入るshortcut | `ExecutionComesFromApprovedStage` |
| retry liveness | `retry_retrieve`がretryを減らさない/違う値へ変えるshortcut | `RetryConsumesOneBudget` |
| retry liveness | retryが残る間の早期拒否/早期レビュー | `FB-6`, `FB-7` |
| retry liveness | 低証拠を拒否で閉じる正規pathが`CanRefuse`では見えにくい | `AC-3` |

## 本体へ反映しなかったもの

### type bound / Req範囲

`type Req lo/hi`のsurvivorは、sliceを1件固定にしていることの副作用である。

本体requirementsは`symmetric type Req = 0..1`で複数件を扱っているため、slice側のID範囲survivorを本体制約へ戻す必要はない。

### 初期値のenum swap

`Missing->Adequate`、`Unchecked->Passed`、`NoApproval->Approved`などの初期値swapは、slice内では後続の準備actionが値を上書きするため生き残るものがある。

本体では初期状態の意味はコメントと`init`で十分に明示されており、追加の不変条件で「Newなら必ずMissing/Unchecked/NoApproval」と縛ると、将来の入口前プリセットや認証済み入力を扱いにくくなる。現時点では戻さない。

### fault action / setup action

`prepare_good_draft`、`corrupt_evidence`、`corrupt_citation`、`prepare_tool_ready`、`downgrade_approval`、`revoke_operator`はmutation観測用の前状態生成である。

これらのrequires削除やassignment変更のsurvivorは、実運用のrequirements actionではなく、sliceの観測補助に由来する。本体へ戻さない。

### fair_remove

`output_guard_pass`、`output_guard_fail`、`request_tool_approval`、`retry_retrieve`などの`fair_remove` survivorは、slice単体のliveness範囲では観測しきれないものがある。

本体requirementsでは`RequestEventuallyHandled`があり、progressをdesign refinementでもpreserveしている。fairnessの有無は本体livenessとrefinement側で読む。

### denial pathの詳細

`deny_tool`のrequiresやassignment変異は多くsurviveした。

現時点のrequirementsは「承認拒否はRefusedで閉じる」ことだけを要求しており、拒否理由や監査イベントの詳細状態は持っていない。`Refused`は低証拠、guard失敗、tool拒否で共有されるため、`Refused => approval == Denied`のような制約は誤りになる。

denial reasonを区別したくなったら、`reason` enumを追加する別設計判断として扱う。

## 反映後の確認

- `fslc check examples/agentic_rag/agentic_rag_requirements.fsl`: `ok`
- `fslc verify examples/agentic_rag/agentic_rag_requirements.fsl --depth 8 --deadlock ignore --exclude-property RequestEventuallyHandled`: `verified`
- `fslc verify examples/agentic_rag/agentic_rag_requirements.fsl --depth 8 --deadlock ignore --property RequestEventuallyHandled`: `verified`
- `fslc refine examples/agentic_rag/agentic_rag_requirements.fsl examples/agentic_rag/agentic_rag_business.fsl examples/agentic_rag/agentic_rag_requirements_refines_business.fsl --depth 7`: `refines`
- `fslc refine examples/agentic_rag/agentic_rag_design.fsl examples/agentic_rag/agentic_rag_requirements.fsl examples/agentic_rag/agentic_rag_design_refines_requirements.fsl --depth 6`: `refines`
- `fslc check examples/agentic_rag/mutation_slices/backported_constraints_slice.fsl`: `ok`
- `fslc verify examples/agentic_rag/mutation_slices/backported_constraints_slice.fsl --depth 7 --deadlock ignore`: `verified`
- `fslc mutate examples/agentic_rag/mutation_slices/backported_constraints_slice.fsl --depth 7 --by-requirement --max-mutants 180`: `180 total / 160 killed / 20 survived`

`backported_constraints_slice.fsl`で直接killした本体反映済み制約:

| Property | Kills |
|---|---:|
| `EvidenceReadyHasEvidenceAndCitation` | 8 |
| `DraftedHasEvidenceAndCitation` | 8 |
| `EvidenceBadMeansNoCitation` | 6 |
| `GuardFailedMeansFailedGuard` | 6 |
| `ToolApprovalMeansRequested` | 6 |
| `ToolApprovedMeansApproved` | 6 |
| `ExecutionComesFromApprovedStage` | 5 |
| `RetryConsumesOneBudget` | 4 |

## 次に見る候補

- 本体requirementsの直接mutationはまだ重い。必要なら、夜間/CI向けの重い検査として別扱いにする。
- `Refused` / `HumanReview`の理由を区別する必要が出たら、`TerminalReason`のような状態を足す。ただしこれは要件追加であり、mutation survivorだけを理由に足さない。
