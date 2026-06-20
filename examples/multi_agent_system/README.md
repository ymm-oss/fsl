# AI multi-agent system example

この例は、AIマルチエージェントシステムをbusiness、requirements、designの3層FSL契約としてモデル化する。
対象はLLM出力の意味品質そのものではなく、制御の契約である。

- supervisorはworkをscopeし、複数workerへ委譲する
- 両workerの成果物がそろうまで統合しない
- critic承認なしに納品または副作用tool実行へ進まない
- 高リスクworkは人間承認後にだけ副作用toolを実行する
- critic修正ループは1回までで、その後は拒否または人間レビューへ進む
- 受け付けたworkは最終的に納品、拒否、人間レビュー、または副作用tool実行へ到達する

## Files

| File | Purpose |
|---|---|
| `multi_agent_business.fsl` | 業務上の統制を表すbusiness層PoC |
| `multi_agent_requirements.fsl` | supervisor / worker / critic / approvalの外部可視制御を表すrequirements層PoC |
| `multi_agent_requirements_refines_business.fsl` | requirements層をbusiness層へ畳み込むrefinement mapping |
| `multi_agent_design.fsl` | planner queue、worker queue、critic queue、tool queueへ分解したdesign層PoC |
| `multi_agent_design_refines_requirements.fsl` | design層をrequirements層へ畳み込むrefinement mapping |
| `IMPLEMENTATION_CONFORMANCE.md` | system prompt / agent prompt / orchestrator / tool gateをFSL design層へ接続するAdapter契約 |
| `test_multi_agent_design_conformance.py` | `fslc testgen`で生成したimplementation conformance pytestハーネス |

## Layer Notes

business層では、業務として守りたい統制だけを扱う。

- scope済みworkは委譲または終端判断へ進む
- 委譲されたworkはレビューへ戻る
- レビュー中workは承認、拒否、人間レビューで解決される
- 承認済みworkは納品または副作用承認待ちへ進む
- 副作用toolは人間承認後にだけ実行される

requirements層では、PM/レビューアが追える制御状態へ具体化する。

- `active_agents: Set<Agent>`でworker A/Bのassignmentを表す
- `exactlyOne(a in active_agents where ...)`でworker割当の一意性を明示する
- `WorkEventuallyHandled`は`within 18`でbounded livenessを表す
- `WorkOpenUntilHandled`は`until`で処理中workが終端まで処理中集合から外れないことを表す
- 初回委譲の`assign_workers`/`aggregate_results`と、修正後の
  `assign_revision_workers`/`aggregate_revision_results`を分けている。
  これにより、business層には初回委譲だけを見せ、修正ループはレビュー内の内部処理として扱う

design層では、実装で分かれやすい内部工程を追加する。

- `planner_q`、`worker_q`、`critic_q`、`tool_q`を`Seq`で表す
- `forall w in planner_q { ... }`のようにSeq上の量化でqueue内容の意味を固定する
- queue投入やworker開始など、requirements層から見て状態を変えない工程はrefinementで`stutter`にする
- `preserve progress`で、requirements層のlivenessをdesign層でも確認する対象actionを明示する

## Suggested Checks

```bash
fslc check examples/multi_agent_system/multi_agent_business.fsl
fslc check examples/multi_agent_system/multi_agent_requirements.fsl
fslc check examples/multi_agent_system/multi_agent_design.fsl

fslc verify examples/multi_agent_system/multi_agent_business.fsl \
  --engine induction --deadlock ignore

# requirements層の安全性、到達性、acceptance、forbiddenを確認する。
# livenessの進捗義務は重いので、普段のループでは除外する。
# `until`由来のsafetyはこの検証でも確認される。
fslc verify examples/multi_agent_system/multi_agent_requirements.fsl \
  --depth 18 --deadlock ignore \
  --exclude-property WorkEventuallyHandled \
  --exclude-property WorkOpenUntilHandled

# design層の浅い安全性を確認する。
# 深い到達性はqueue工程が増えるため別ジョブまたはsliceで扱う。
fslc verify examples/multi_agent_system/multi_agent_design.fsl \
  --depth 12 --deadlock ignore \
  --exclude-property CanDeliverD \
  --exclude-property CanRejectD \
  --exclude-property CanReviewD \
  --exclude-property CanExecuteToolD

fslc refine examples/multi_agent_system/multi_agent_requirements.fsl \
  examples/multi_agent_system/multi_agent_business.fsl \
  examples/multi_agent_system/multi_agent_requirements_refines_business.fsl \
  --depth 8

fslc refine examples/multi_agent_system/multi_agent_design.fsl \
  examples/multi_agent_system/multi_agent_requirements.fsl \
  examples/multi_agent_system/multi_agent_design_refines_requirements.fsl \
  --depth 8

fslc testgen examples/multi_agent_system/multi_agent_design.fsl \
  --depth 6 --deadlock ignore \
  -o examples/multi_agent_system/test_multi_agent_design_conformance.py

# Adapter未実装の間はskipされる。
./.venv/bin/python -m pytest \
  examples/multi_agent_system/test_multi_agent_design_conformance.py -q
```

期待結果:

- `check`: business / requirements / design が`ok`
- business層の`verify --engine induction`: `proved`
- requirements層の安全性・到達性検証: `verified` at depth 18
- design層の浅い安全性検証: `verified` at depth 12。深い後段actionには到達深さ不足のwarningが出る
- requirements→businessの`refine`: `refines` at depth 8、`progress.BP1..BP6`が確認される
- design→requirementsの`refine`: `refines` at depth 8、`progress.WorkEventuallyHandled`と`progress.WorkOpenUntilHandled`が確認される
- conformance pytest: Adapter未実装ならskip。Adapter接続後はscenario replayとrandom walkが実装・prompt harnessを検査する
- `WorkEventuallyHandled`全体の深さ18 liveness verifyは重い。継続運用では、RAG例と同様にliveness専用sliceへ分ける

## Open Decisions

- 高リスク判定を何で決めるか。
- critic修正ループを1回でよいとするか、N回にするか。
- HumanReviewを終端とするか、人間承認・拒否まで要求するか。
- 複数workを同時に扱う場合のqueue capacity、公平性、優先度をどう扱うか。
- worker成果物の意味的品質を外部評価器として扱うか、別FSL sliceに分けるか。
