# Agentic RAG example

この例は、Agentic RAGのサポートフローをbusiness、requirements、designの3層FSL契約として
モデル化する。対象は自然言語回答の品質そのものではなく、制御の契約である。

- 最終回答には、十分な証拠、許可された引用、通過済み出力ガードレールが必要
- 副作用toolには、operatorロールと明示承認が必要
- 証拠不足の経路はretry budgetを消費し、最終的に拒否または人間レビューへ進む
- 受け付けたリクエストは最終的に処理済み状態へ到達する
- forbidden traceで、ガードレール通過前の回答や承認前tool実行のような
  missing guardリスクを検出する

この仕様は、検索品質、意味的関連性、回答faithfulnessを証明しない。
それらは外部評価器の出力として扱い、FSL上では`evidence`、`citation_ok`、
`guard`で表現する。

## Files

| File | Purpose |
|---|---|
| `agentic_rag_business.fsl` | 業務上の統制を表すbusiness層PoC |
| `agentic_rag_requirements.fsl` | 検索証拠、出力guard、承認、retryを具体化したrequirements層PoC |
| `agentic_rag_requirements_refines_business.fsl` | requirements層の状態/actionをbusiness層へ畳み込むrefinement mapping |
| `agentic_rag_design.fsl` | router、retriever、reranker/evaluator、drafter、output guard、approval、tool executorへ分解したdesign層PoC |
| `agentic_rag_design_refines_requirements.fsl` | design層の内部状態/actionをrequirements層へ畳み込むrefinement mapping |
| `negative/` | 失敗するべきdesignを置いたネガティブプローブ集 |
| `mutation_slices/` | `fslc mutate`を軽く回すための回答安全性、tool承認、retry/liveness sliceとsurvivorレビュー台帳 |
| `test_agentic_rag_design_conformance.py` | `fslc testgen`で生成したimplementation conformance pytestハーネス |
| `IMPLEMENTATION_CONFORMANCE.md` | 実装Adapterの`reset`/`step`/`observe`契約 |

## Layer Notes

business層では、業務上の統制だけを扱う。

- 受け付けた依頼は放置されず、回答準備・拒否・人間レビュー・承認待ちへ進む
- 回答可能になった依頼は顧客へ回答される
- 副作用toolの承認待ちは承認または拒否で解決される
- 承認済みの副作用toolは実行される

requirements層では、PM/レビューアが追いやすい外部可視な制御状態へ具体化する。

- 検索証拠、引用許可、出力guard、operator承認、retry budgetを明示する
- mutation sliceのsurvivorレビューから、EvidenceReady/EvidenceBad/ToolApproval/ToolApprovedの
  状態同期、retry decrement、ActionExecutedの直前stageを本体requirementsにも戻している
- guard評価は`Unchecked`の時だけ実行できる。これにより、guard pass/failの
  無意味な繰り返しがbusiness層の「回答可能化」を壊さないようにする
- `Drafted + guard == Passed`だけをbusiness層の`BReadyForCustomer`へ写す

design層では、実装で分かれやすい内部工程を追加する。

- `accept_request`と`write_audit`を分け、監査ログが書かれた時点だけを`receive`に対応させる
- `vector_search`、`rerank`、`evaluate_*`を分け、評価結果が出るまで抽象状態は`Classified`または`Retrieving`のままにする
- `enqueue_draft`、`start_output_guard`のようなworker準備処理はrequirements層では`stutter`にする
- `plan_tool`を内部準備にし、明示承認待ちへ出す`request_tool_approval`だけをrequirements層に見せる
- `preserve progress`で、requirements層のlivenessをdesign層でも確認する対象actionを明示する

この構造により、下位層の詳細化が「上位層で許可されていない近道」を作っていないかを
`fslc refine`で確認できる。

## Language Notes

2026-06-20に`origin/main`を取り込み、手元の`fslc`もeditable installで更新した。
これにより、`within`付き`leadsTo`、`unless`/`until`、Set/Seq上の量化、
`unique`/`exactlyOne`を検査できる。

このPoCでは、まず`mutation_slices/retry_liveness_slice.fsl`で新構文を使っている。

- `within`付き`leadsTo`で、retry budget=2の証拠不足pathが6 step以内に
  `Refused`または`HumanReview`へ閉じることを明示する
- `until`で、「処理中の証拠不足pathは終端まで処理中集合から外れない」と
  「最終的に終端へ到達する」を1つの読みやすい契約にまとめる
- 3層本体の`RequestEventuallyHandled`には、まだ`within`を入れていない。
  2リクエストのinterleavingや準備操作までSLAに含めるかは設計判断になるためである
- citation候補、tool候補、worker queueを`Set`/`Seq`でモデル化する場合、
  `forall x in candidates { ... }`や`exactlyOne(...)`で重複・一意性を読みやすく書ける

## Suggested Checks

```bash
fslc check examples/agentic_rag/agentic_rag_business.fsl
fslc check examples/agentic_rag/agentic_rag_requirements.fsl
fslc check examples/agentic_rag/agentic_rag_design.fsl

# business層の業務統制と到達可能性を確認する。
fslc verify examples/agentic_rag/agentic_rag_business.fsl \
  --engine induction --deadlock ignore

# safety、到達可能性、acceptance、forbiddenを確認する。
# livenessは重いので普段の短いループでは除外する。
fslc verify examples/agentic_rag/agentic_rag_requirements.fsl \
  --depth 8 --deadlock ignore --exclude-property RequestEventuallyHandled

# livenessだけを確認する。leadsToのlasso探索は高コストなので分けて実行する。
fslc verify examples/agentic_rag/agentic_rag_requirements.fsl \
  --depth 8 --deadlock ignore --property RequestEventuallyHandled

# design層の浅い安全性スモークを確認する。
# 内部工程が増えたぶん成功pathが長いので、短い普段使いでは到達性を除外する。
# depth 4では深いaction未到達のvacuity warningが出るが、基本不変条件の浅い破壊は拾える。
fslc verify examples/agentic_rag/agentic_rag_design.fsl \
  --depth 4 --deadlock ignore \
  --exclude-property CanAnswerD \
  --exclude-property CanReviewD \
  --exclude-property CanExecuteToolD

# requirements層がbusiness層から逸脱していないことを確認する。
fslc refine examples/agentic_rag/agentic_rag_requirements.fsl \
  examples/agentic_rag/agentic_rag_business.fsl \
  examples/agentic_rag/agentic_rag_requirements_refines_business.fsl \
  --depth 7

# design層がrequirements層から逸脱していないことを確認する。
fslc refine examples/agentic_rag/agentic_rag_design.fsl \
  examples/agentic_rag/agentic_rag_requirements.fsl \
  examples/agentic_rag/agentic_rag_design_refines_requirements.fsl \
  --depth 6

# 実装conformance testの雛形を再生成する。
# depth 4は浅い初期ハーネス用。深いcoverシナリオは重いので別途扱う。
fslc testgen examples/agentic_rag/agentic_rag_design.fsl \
  --depth 4 --deadlock ignore \
  -o examples/agentic_rag/test_agentic_rag_design_conformance.py

# Adapter未実装の間はskipされる。
./.venv/bin/python -m pytest \
  examples/agentic_rag/test_agentic_rag_design_conformance.py -q

# ネガティブプローブ: 出力guardを迂回した回答公開はrefinementで拒否される。
fslc refine examples/agentic_rag/negative/guard_bypass_design.fsl \
  examples/agentic_rag/agentic_rag_requirements.fsl \
  examples/agentic_rag/negative/guard_bypass_refines_requirements.fsl \
  --depth 6

# ネガティブプローブ: 承認済みでない副作用tool実行はrefinementで拒否される。
fslc refine examples/agentic_rag/negative/tool_approval_bypass_design.fsl \
  examples/agentic_rag/agentic_rag_requirements.fsl \
  examples/agentic_rag/negative/tool_approval_bypass_refines_requirements.fsl \
  --depth 6

# ネガティブプローブ: 内部stutter loopでlivenessを落とすdesignは
# preserve progressによりprogress_lostとして拒否される。
fslc refine examples/agentic_rag/negative/liveness_drop_design.fsl \
  examples/agentic_rag/negative/liveness_requirements.fsl \
  examples/agentic_rag/negative/liveness_drop_refines_requirements.fsl \
  --depth 4

# Mutation slices: requirements本体を直接mutateする代わりに、用途別sliceを軽く検査する。
fslc mutate examples/agentic_rag/mutation_slices/answer_safety_slice.fsl \
  --depth 6 --by-requirement --max-mutants 80

fslc mutate examples/agentic_rag/mutation_slices/tool_approval_slice.fsl \
  --depth 7 --by-requirement --max-mutants 100

fslc mutate examples/agentic_rag/mutation_slices/retry_liveness_slice.fsl \
  --depth 8 --by-requirement --max-mutants 100
```

期待結果:

- `check`: `ok`
- business層の`verify --engine induction`: `proved`
- requirements層の`verify`: `verified`
- design層の浅い`verify`: `verified`、ただし深いaction未到達のvacuity warningは許容
- requirements→businessの`refine`: `refines`、かつ`progress.BP1..BP4`が確認される
- design→requirementsの`refine`: `refines`、かつ`progress.RequestEventuallyHandled`が確認される
- conformance pytest: Adapter未実装ならskip。Adapter接続後はscenario replayとrandom walkが実装を検査する
- ネガティブプローブ:
  - guard迂回とtool承認迂回は`refinement_failed / abs_requires_failed`
  - liveness喪失は`refinement_failed / progress_lost / leadsTo`
- mutation slices:
  - baselineは`verified`
  - survivorは失敗ではなく、追加すべき制約や同値変異を確認するレビューキュー
  - survivor分類で本体requirementsへ戻した制約は`mutation_slices/SURVIVOR_REVIEW.md`に記録する

## Open Decisions

- 証拠不足時の既定終端を`Refused`にするか`HumanReview`にするか。
- `HumanReview`を終端扱いにするか、最終的にapprove/rejectまで要求するか。
- retrieval不要の雑談回答を対象に含めるか。
- 実装conformance testにつなぐ具体的な副作用toolを何にするか。
- `time`と`deadline`でSLAを追加するか。
