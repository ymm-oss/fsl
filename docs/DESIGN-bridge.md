# FSL v2.0 — 実装橋(ランタイムモニタ / replay / testgen)実装設計

DESIGN-v1.md §10 v2.0「実装橋の本体」。仕様と実装の間に3つの橋を架ける:

1. **`fslc.runtime.Monitor`** — 仕様の具象インタプリタ(Z3 不要、純 Python)。
   実装に組み込んで実行時適合性検査(runtime monitoring)に使う。
2. **`fslc replay`** — 実システムのイベントログを仕様に対して検査する CLI。
   コードを書かずに使える最初の入口。
3. **`fslc testgen`** — 実装への適合性テスト(pytest)雛形を生成。
   scenarios(既存)の再生テスト+ランダムウォーク性質テスト。

設計原則: **検証器(Z3 評価器)と同じ意味論の第二実装**になるため、
witness トレースの再生による**差分テスト**で両者を相互検証する(§6)。

## 1. `fslc.runtime` — 具象インタプリタ

新モジュール `src/fslc/runtime.py`。Z3 を import しない(`parse`/`build_spec`
は使ってよい — model.py が z3 を import しているなら、sort 構築部分を遅延
import にする等で「runtime 利用時に z3 不要」までは**要求しない**。v2.0 では
同一パッケージ内の利用で十分)。

### 1.1 値の表現(論理値 — verify の JSON 表示と同一規約)

| FSL 型 | Python 表現 |
|---|---|
| Int / ドメイン型 | int |
| Bool | bool |
| enum | str(メンバ名) |
| Option | None または 中身の値 |
| struct | dict(フィールド名 → 値) |
| Map | dict(キーの論理値 → 値)。キーは全域(有界キーを列挙して初期化) |
| Set | Python set |
| Seq | Python list |

### 1.2 API

```python
from fslc.runtime import Monitor

mon = Monitor(spec_source_or_path)      # parse + build_spec + 静的検査
mon.reset()                             # -> state dict(初期状態)
r = mon.step("checkout", {"u": 0})      # -> 結果 dict(下記)
mon.state                               # 現在の論理状態(dict、JSON 互換)
mon.enabled()                           # -> [{"action": str, "params": dict}, ...]
```

`step` の結果(verify の JSON と同じ語彙):

```json
{ "ok": true, "state": { ... }, "changes": { "stock[0]": {"from": 1, "to": 0} } }
{ "ok": false, "kind": "requires_failed", "action": "checkout",
  "params": {"u": 0}, "requires": {"loc": ..., "text": "..."}, "state": { ... } }
{ "ok": false, "kind": "ensures" | "type_bound" | "partial_op" | "invariant",
  "name": "<invariant名 or _bounds_* or _partial_*>", "loc": ...,
  "state": { ... }, "hint": "..." }
```

- 違反時は**状態を変更しない**(requires_failed / partial_op / type_bound /
  ensures / invariant いずれも遷移前にロールバック)。例外は投げない —
  常に結果 dict を返す(モニタとして組み込みやすく、LLM にも読みやすい)。
- `step` の意味論は BMC と同一: requires 全評価(短絡なし)→ 本体を
  同時代入で適用(全 RHS は旧状態を読む)→ partial_op 検査(パス条件考慮)
  → ensures(old = 旧状態)→ 新状態の invariant + 自動境界検査。
- 未知のアクション名・パラメータ欠落/型範囲外は `kind: "bad_call"`。

### 1.3 init の決定性

具象実行には決定的な初期状態が必要。`Monitor` 構築時に静的検査:

- init は全状態変数に**ちょうど1回**代入していること(forall 一括代入可)。
- init の RHS が参照できるのは const と**既に代入済みの**状態変数のみ
  (上から順に評価)。違反は `FslError(kind="semantics")` +
  hint「runtime monitor requires a deterministic init」。
- 既存仕様は全て満たす(満たさない仕様は verify は通るが Monitor では
  エラーになる — 仕様側の修正を促す)。

### 1.4 式評価

`runtime.py` 内に具象評価器 `eval_concrete(expr, state, binds, spec, old_state)`
を実装。bmc.py の `eval_expr` と同じ AST を入力に取り、§1.1 の Python 値で
評価する。以下に注意:

- 量化・集約(forall/exists/count/sum)は有界列挙で素直にループ。
- `is some(x)` の束縛、`min/max/abs`、struct/Seq/Set の等価は §1.1 の表現上で。
- Seq の `at`/`head`/`pop` の範囲外は**呼び出し時点で partial_op を報告**
  (パス条件は if 評価で自然に守られる — 具象実行では実際に通った分岐しか
  評価しないため、BMC の「パス条件含意」と一致する)。
- 0 除算等はないが、想定外の評価エラーは `kind: "internal"` に包む。

## 2. `fslc replay` — イベントログの適合性検査

```
fslc replay <file.fsl> --trace <events.json>
```

入力(実システムが吐くログの想定形 — scenarios の steps と同形):

```json
{ "events": [ { "action": "add_to_cart", "params": {"u": 0, "i": 1} }, ... ] }
```

トップレベルが配列のみの JSON(`[ {...}, ... ]`)も受け付ける。

出力:

```json
{ "fsl": "1.0", "result": "conformant", "spec": "ShoppingCart",
  "steps_checked": 12, "final_state": { ... } }
{ "fsl": "1.0", "result": "nonconformant", "spec": "ShoppingCart",
  "failed_at_event": 4, "violation": { ...step の ok:false 結果... },
  "state_before": { ... },
  "hint": "the implementation performed an action the spec forbids at this state (or reached a state violating an invariant)" }
```

exit code: conformant = 0、nonconformant = 1、入力/仕様エラー = 2。

## 3. `fslc testgen` — 適合性テスト雛形の生成

```
fslc testgen <file.fsl> [--depth K] [-o <out.py>]    # 既定: test_<spec名小文字>.py を stdout
```

生成物は**自己完結の pytest ファイル**(import は fslc.runtime と pytest のみ):

1. **Adapter スタブ**: ユーザーが実装を結線するクラス。
   ```python
   class Adapter:
       """Connect your implementation to the spec actions/state."""
       def reset(self): raise NotImplementedError
       def step(self, action: str, params: dict): ...   # 実装を1アクション分駆動
       def observe(self) -> dict: ...                   # 実装の状態を仕様の state 形に射影
   ```
2. **シナリオ再生テスト**(scenarios 機構を testgen 時に実行して埋め込む):
   各シナリオの steps を Adapter に流し、各ステップ後の `observe()` が
   `expected_states[i]` の**言及フィールドのみ**一致することを assert。
3. **ランダムウォーク適合性テスト**: `Monitor` を oracle に、
   `mon.enabled()` から疑似乱数(固定シード、`random.Random(0)`)で
   アクションを選び N=100 ステップ、毎ステップ
   `adapter.step(...)` → `observe() == mon.state` を assert。
   Monitor 側で違反(invariant 等)が出たらそれは**仕様自体のバグ**として
   fail メッセージで区別する。
4. Adapter 未実装(NotImplementedError)の間は全テストが `pytest.skip` に
   なるようにし、生成直後でも pytest がエラーにならないこと。

## 4. CLI / 公開 API

- cli.py: `replay` / `testgen` サブコマンド追加。既存コマンドは不変。
- `fslc/__init__.py`: `from .runtime import Monitor` を公開。

## 5. 制約・スコープ外

- leadsTo は runtime monitor では検査しない(有限ログ上の応答性質は
  「未達」と「違反」を区別できない。replay 出力の `note` に明記)。
- fair 注釈は runtime には影響しない。
- 並行実行下のモニタ(スレッド安全性)はスコープ外(ドキュメントに明記)。

## 6. テスト計画(tests/test_runtime.py)

1. **差分テスト(最重要)**: 全サンプル仕様について、`fslc verify` の
   reachable witness トレース(と scenarios の steps)を `Monitor` で再生し、
   **各ステップの状態が witness の状態と完全一致**すること。
   (Z3 評価器と具象評価器の意味論一致の機械的検証。既存の
   test_scenarios §3.1 の一般化)
2. requires_failed: ガードの落ちた step が ok: false / 状態不変。
3. partial_op / type_bound / ensures / invariant の各違反が Monitor でも
   同じ kind で検出される(verify で violated になる仕様の反例トレースを
   1ステップずつ再生し、最後のステップで同じ kind が返る)。
4. 非決定 init(代入漏れ)が Monitor 構築時に semantics エラー。
5. replay: conformant なログ / nonconformant なログ(failed_at_event と
   violation.kind を確認)/ 配列形入力。
6. testgen: 生成ファイルが import 可能で、Adapter 未実装なら skip、
   Monitor を Adapter の代わりに結線した「自己適合」では全テスト pass
   (= 生成された雛形がそのまま動くことの検証)。
7. enabled(): 既知の状態で期待されるインスタンス列挙と一致。

## 7. ドキュメント反映

- LANGUAGE.md に「§10 実装への橋」節: Monitor / replay / testgen の使い方と
  ワークフロー(spec proved → testgen → Adapter 実装 → pytest)。
- README の使い方に replay / testgen を追記。
- DESIGN-v1.md §10 の該当項目に実装済み注記。
