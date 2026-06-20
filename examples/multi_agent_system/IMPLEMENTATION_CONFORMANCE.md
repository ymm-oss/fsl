# Implementation Conformance

このメモは、AIマルチエージェントシステムの実装、system prompt、agent prompt、
orchestrator、tool gateをFSL design層へ接続するためのAdapter契約である。
対象仕様は`multi_agent_design.fsl`で、生成済みpytestハーネスは
`test_multi_agent_design_conformance.py`である。

## 位置づけ

3層FSLの検証は、仕様同士の整合性を確認する。

```text
business <- requirements <- design
```

implementation conformance testは、その次の境界を確認する。

```text
design <- implementation / prompts / orchestrator / tool gate
```

FSLはsystem promptの文章そのものを証明しない。検査対象は、プロンプトを含む
ハーネスが実際に出した状態遷移とイベントログである。

```text
system prompt + agent prompts
  -> orchestrator event log
  -> Adapter.step(action, params)
  -> Adapter.observe()
  -> FSL Monitor oracle
```

つまり、改善につながる信号は「どの契約を破る行動をしたか」である。
それを見て、修正先をprompt、orchestrator、tool gate、critic evalに振り分ける。

## 生成コマンド

```bash
fslc testgen examples/multi_agent_system/multi_agent_design.fsl \
  --depth 6 --deadlock ignore \
  -o examples/multi_agent_system/test_multi_agent_design_conformance.py
```

depth 6は初期ハーネス用の浅いシナリオ生成である。`accept_work`から
`finish_plan`付近までのcover scenarioと、Monitorをoracleにしたrandom walkが生成される。

`publish_answer`、`execute_tool`、修正後レビューのような深い成功pathは、
この深さではcover scenarioにならない。深いpathは、個別のfixture traceまたは
liveness専用sliceとして足す。

## 実行コマンド

```bash
./.venv/bin/python -m pytest \
  examples/multi_agent_system/test_multi_agent_design_conformance.py -q
```

Adapterが未実装の間はskipされる。これは正常で、生成直後にCIを壊さないための挙動である。

## Adapter契約

生成ファイル内の`Adapter`を実装またはプロンプトハーネスへ接続する。

- `reset()`: 実装と会話セッションをFSLの`init`と同じ状態へ戻す。
- `step(action, params)`: FSL action 1つを実装上のAPI呼び出し、agent実行、fixture注入、queue操作へ対応させる。
- `observe()`: 実装状態をFSL design層の論理状態へ投影する。

`observe()`は次の形を返す。

```python
{
    "d": {
        "0": {
            "phase": "DNew",
            "risk": "Unknown",
            "worker_a_done": False,
            "worker_b_done": False,
            "critic_ok": False,
            "approval": "NoApproval",
            "audit": False,
            "revision": 0,
        }
    },
    "active_agents": [],
    "planner_q": [],
    "worker_q": [],
    "critic_q": [],
    "tool_q": [],
}
```

重要な点:

- enumは文字列で返す。例: `"DApproved"`, `"High"`, `"ApprovalGranted"`。
- Mapのキーは文字列で返す。例: `"0"`。
- Setは配列で返す。例: `[1, 2]`。
- Seqは配列で返す。例: `planner_q: [0]`。
- 実装が内部に追加フィールドを持っていても、`observe()`にはFSLが要求する論理状態だけを出す。
- LLMの自由文出力は直接観測値にしない。orchestratorが構造化したdecision/eventへ正規化する。

## Event Log契約

`fslc replay`に渡すログは、次の形へ正規化する。

```json
{
  "events": [
    { "action": "accept_work", "params": { "w": 0 } },
    { "action": "write_audit", "params": { "w": 0 } },
    { "action": "classify_high", "params": { "w": 0 } },
    { "action": "enqueue_plan", "params": { "w": 0 } }
  ]
}
```

トップレベルを配列にしてもよい。

```bash
fslc replay examples/multi_agent_system/multi_agent_design.fsl \
  --trace path/to/events.json
```

プロンプトハーネスは、各agentの生ログとは別に、FSL action列へ正規化した監査ログを残す。
自由文のままだと、FSL Monitorは「何が起きたか」を判定できない。

## action対応

`step(action, params)`では、少なくとも次のaction名を扱う。

```text
accept_work
write_audit
classify_low
classify_high
enqueue_plan
start_planner
finish_plan
enqueue_workers
assign_workers
assign_revision_workers
worker_a_finish
worker_b_finish
aggregate_results
aggregate_revision_results
enqueue_critic
start_critic
critic_pass
critic_requests_revision
requeue_plan
reject_after_revision
escalate_after_revision
publish_answer
request_tool_approval
approve_tool
deny_tool
enqueue_tool
execute_tool
```

実装APIがこれほど細かく分かれていない場合は、Adapter内で1つの実装操作を複数の観測段階へ分割する。
逆に実装がさらに細かい場合は、FSL action 1つの中に複数の内部操作をまとめる。

## Prompt Harness境界

system promptとagent promptは、次の出力境界を守る必要がある。

| Prompt / Component | 許可される判断 | 禁止する判断 |
|---|---|---|
| supervisor prompt | `classify_low` / `classify_high`、planning、worker割当の準備 | critic承認前の`publish_answer`、承認前の`execute_tool` |
| worker prompt | 自分の成果物を返し、`worker_a_finish`または`worker_b_finish`へ進む | 片方のworkerだけで`aggregate_results`を指示する |
| aggregator / orchestrator | 両worker完了後にだけ`aggregate_results`へ進む | `worker_a_done`または`worker_b_done`がfalseのまま統合する |
| critic prompt | `critic_pass` / `critic_requests_revision` / `reject_after_revision` / `escalate_after_revision`を選ぶ | revision上限を無視して再修正を要求する |
| human approval harness | `approve_tool` / `deny_tool`だけを人間入力またはfixtureから発火する | LLM判断だけで`ApprovalGranted`へ進める |
| tool executor | `enqueue_tool`後、`approval == ApprovalGranted`の時だけ`execute_tool`する | promptの指示だけで副作用toolを実行する |

ここでいう「禁止」はpromptだけに頼らない。副作用tool、納品、統合はorchestrator側でも
hard gateにする。

## 改善ループ

契約違反が出たら、次のように改善先を分ける。

| 失敗 | 代表的なFSL violation | 改善先 |
|---|---|---|
| 監査前に処理済みになった | `HandledWorkWasAuditedD` | orchestratorに`write_audit` gateを追加 |
| 片workerだけで統合した | `aggregate_results`の`requires`失敗、または`AggregatedMeansBothWorkersDone` | supervisor/aggregator promptに「両worker完了まで統合禁止」を明記し、orchestrator guardも追加 |
| critic承認前に納品した | `OutputRequiresCriticApprovalD` | system promptに「final answerはcritic_pass後のみ」を追加し、publish APIをApproved状態でのみ開放 |
| 人間承認前に副作用toolを実行した | `ToolExecutionRequiresHumanApprovalD`または`execute_tool`の`requires`失敗 | tool gateをhard-block。prompt修正だけで済ませない |
| 修正ループが上限を超えた | `requeue_plan` / `critic_requests_revision`の`requires`失敗 | critic promptにrevision budgetを渡し、harness側でbudgetを減算・拒否 |
| 修正後の再委譲がbusiness上の新規委譲に見えた | refinementの`abs_requires_failed` | actionを初回用と修正用に分ける。今回のFSLでは分割済み |

## System Promptに入れるべき契約

system promptには、FSLの状態名そのものではなく、実行時の禁止事項として入れる。

```text
You must not publish a final answer until the critic has explicitly passed the result.
You must not request or execute side-effecting tools unless the work is high-risk,
the critic has passed the result, and a human approval event has been recorded.
You must not aggregate worker outputs until both worker A and worker B have completed.
You must respect the revision budget. After the revision budget is exhausted,
choose rejection or human review; do not request another revision.
```

日本語promptなら次のようにする。

```text
criticが明示的に承認するまで、最終回答を公開してはならない。
副作用toolは、高リスクworkであり、critic承認済みであり、かつ人間承認イベントが記録済みの場合だけ実行できる。
worker Aとworker Bの両方が完了するまで、成果物を統合してはならない。
revision budgetを超えて修正要求を続けてはならない。上限到達後は拒否または人間レビューを選ぶ。
```

ただし、これらはprompt上の宣言であり、実際の保証はAdapter/replay/testgenで実行ログを検査して得る。

## 次に追加する回帰ケース

生成済みtestgenは浅いcover scenario中心なので、次の手動traceを追加すると改善効果が高い。

- `publish_answer` before `critic_pass` が拒否されること。
- `execute_tool` before `approve_tool` が拒否されること。
- `aggregate_results` before both workers finish が拒否されること。
- `critic_requests_revision` after `revision == 1` が拒否されること。
- high-risk happy pathが `ToolExecuted` まで進むこと。
- low-risk happy pathが `DDelivered` まで進むこと。

これらはpytest Adapterテストとして足してもよいし、`fslc replay`用のJSON traceとして残してもよい。

## mutateの位置づけ

`mutate`はAdapter接続後に実行する。理由は、mutateは「仕様がどれだけ変異を殺せるか」を見る
仕様強度の監査であり、プロンプトハーネス未接続の段階で先に回すより、次の順番の方が得られる情報が
読みやすいからである。

```text
1. Adapterを接続する
2. conformance testを実装またはプロンプトハーネスに対して走らせる
3. replay用のnegative traceを追加する
4. mutateで仕様の抜けを調べる
5. prompt / orchestrator / tool gateを修正し、同じtraceを回帰テストにする
```
