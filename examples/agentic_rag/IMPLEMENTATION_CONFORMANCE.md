# Implementation Conformance

このメモは、Agentic RAGの実装をFSL design層へ接続するためのAdapter契約である。
対象仕様は`agentic_rag_design.fsl`で、生成済みpytestハーネスは
`test_agentic_rag_design_conformance.py`である。

## 位置づけ

3層FSLの検証は、仕様同士の整合性を確認する。

```text
business <- requirements <- design
```

implementation conformance testは、その次の境界を確認する。

```text
design <- implementation
```

つまり、実装がrouter、retriever、reranker/evaluator、drafter、output guard、
approval、tool executorの論理状態を、design層と同じ順序・同じ制約で進めているかを
pytestで確認する。

## 生成コマンド

```bash
fslc testgen examples/agentic_rag/agentic_rag_design.fsl \
  --depth 4 --deadlock ignore \
  -o examples/agentic_rag/test_agentic_rag_design_conformance.py
```

depth 4は初期ハーネス用の浅いシナリオ生成である。深い成功path、たとえば
`DAnswered`や`DActionExecuted`までのcoverシナリオは、この深さでは生成されない。
その代わり、生成ファイルにはMonitorをoracleにしたrandom walkが含まれる。

depth 8やdepth 12の生成はこのモデルでは重い。深いシナリオを増やす場合は、
夜間CIや個別シナリオ化を検討する。

## 実行コマンド

```bash
./.venv/bin/python -m pytest \
  examples/agentic_rag/test_agentic_rag_design_conformance.py -q
```

Adapterが未実装の間はskipされる。これは正常で、生成直後にCIを壊さないための挙動である。

## Adapter契約

生成ファイル内の`Adapter`を実装へ接続する。

- `reset()`: 実装をFSLの`init`と同じ状態へ戻す。
- `step(action, params)`: FSL action 1つを実装上のAPI呼び出し、worker実行、fixture操作へ対応させる。
- `observe()`: 実装状態をFSL design層の論理状態へ投影する。

`observe()`は次の形を返す。

```python
{
    "d": {
        "0": {
            "phase": "DNew",
            "evidence": "Missing",
            "guard": "Unchecked",
            "approval": "NoApproval",
            "role": "Public",
            "retry": 2,
            "needs_tool": False,
            "citation_ok": False,
            "audit": False,
        },
        "1": {
            "phase": "DNew",
            "evidence": "Missing",
            "guard": "Unchecked",
            "approval": "NoApproval",
            "role": "Public",
            "retry": 2,
            "needs_tool": False,
            "citation_ok": False,
            "audit": False,
        },
    }
}
```

重要な点:

- enumは文字列で返す。例: `"DReady"`, `"Adequate"`, `"Passed"`, `"Approved"`。
- Mapのキーは文字列で返す。例: `"0"`, `"1"`。
- 実装が内部に追加フィールドを持っていても、`observe()`にはFSLが要求する論理状態だけを出す。
- 外部評価器の結果は、`evaluate_adequate` / `evaluate_inadequate` actionでfixture化する。
- 出力guardの結果は、`output_guard_pass` / `output_guard_fail` actionでfixture化する。
- tool承認・拒否は、`approve_tool` / `deny_tool` actionで明示的に進める。

## action対応

`step(action, params)`では、少なくとも次のaction名を扱う。

```text
set_operator
accept_request
write_audit
enqueue_router
route_answer
route_tool
vector_search
rerank
vector_search_retry
rerank_retry
evaluate_adequate
evaluate_inadequate
schedule_retry
refuse_low_evidence
review_low_evidence
enqueue_draft
draft_answer
start_output_guard
output_guard_pass
output_guard_fail
publish_answer
refuse_guard_failure
review_guard_failure
plan_tool
request_tool_approval
approve_tool
deny_tool
execute_tool
```

実装APIがこれほど細かく分かれていない場合は、Adapter内で1つの実装操作を複数の観測段階へ
分割する。逆に実装がさらに細かい場合は、FSL action 1つの中に複数の内部操作をまとめる。

## mutateの位置づけ

`mutate`は後で実行する。理由は、mutateは「仕様がどれだけ変異を殺せるか」を見る
仕様強度の監査であり、Adapter未接続の段階で先に回すより、次の順番の方が得られる情報が
読みやすいからである。

```text
1. Adapterを接続する
2. conformance testを実装に対して走らせる
3. negative probeを追加する
4. mutateで仕様の抜けを調べる
5. 必要ならSLA/deadlineを追加する
```
