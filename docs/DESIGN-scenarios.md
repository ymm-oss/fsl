# FSL v1.1 — unsat core ヒントと `fslc scenarios` 実装設計

DESIGN-v1.md §10 の v1.1 項目のうち、coverage 診断と実装橋の2機能を規定する。

## 1. unsat core による coverage 診断

### 1.1 問題

`action_coverage` が `false` のアクションについて、現状は「深さ K 以内で一度も
enabled にならない」という事実しか返らない。LLM の次の一手は「どの requires が
阻んでいるか」が分かって初めて決まる(修復プロトコル §8 の原則)。

### 1.2 仕様

coverage が false のアクション A について、追加診断を行う:

1. 最終深さ K の状態 σ_K(coverage 検査で使った展開の最終状態)に対し、
   A の各インスタンス(パラメータ束縛ごと)の requires 連言を assumption リテラル
   `p_i ⇔ requires_i` 化して `solver.check(p_1, ..., p_n)` を実行。
2. unsat なら `solver.unsat_core()` から、enabled を阻む requires 句の
   **極小に近い集合**を得る(Z3 の core は極小保証なしでよい。v1.1 は精度より
   応答性を優先し、core をそのまま使う)。
3. インスタンスが複数ある場合(パラメータ付きアクション)、**全インスタンスの
   core の共通部分**を代表として報告する。共通部分が空なら最初のインスタンスの
   core を報告し、`bindings` を添える。
4. 深さは K だけでなく t = 0..K の各ステップで「どこかの t で sat になる句の
   組合せ」があるため、検査は coverage 本体と同じ「step-by-step、t ごと」の
   ループに相乗りする。最後まで false だったアクションについてのみ、
   **t = K の時点の core** を報告する(全 t の core を返すと冗長。K 時点が
   「最も状態が発展した時点でもなお阻んでいる句」として代表性が高い)。

### 1.3 JSON

`action_coverage` を従来の `{name: bool}` から後方互換に拡張する:
true のアクションは従来どおり `true`、false のアクションのみオブジェクトにする。

```json
"action_coverage": {
  "add_to_cart": true,
  "checkout": {
    "covered": false,
    "blocking_requires": [
      { "loc": {"line": 27, "column": 3}, "text": "stock[i] > 0" }
    ],
    "bindings": {"u": 0, "i": 1},      // core が束縛依存のときのみ
    "hint": "these requires clauses are unsatisfiable at every step up to depth K; weaken one of them, add an action that establishes them, or increase --depth"
  }
}
```

- `text` は AST からの逆整形(pretty-print)。v1 実装に pretty-printer が
  なければ `loc` のみ必須、`text` はベストエフォート(ソース行の切り出しで可)。
- 既存テストの `coverage[name] is True` 断言はそのまま通る(true 側は bool 維持)。
- coverage 警告(warnings 内)の hint からこの構造を参照する。

### 1.4 実装ノート

- `_action_coverage` のループで、各未カバーアクションの requires 句リストを
  `z3.Bool(f"__cov_{action}_{i}_{j}")` の含意で積み、`check(assumptions)` を使う。
  通常の `s.add` と違い push/pop 不要で core が取れる。
- requires の AST と loc は instance dict に既にある(`inst["requires"]`)。
  句単位(requires 文ごと)で十分。連言内のさらに細かい分解はしない。

## 2. `fslc scenarios` — 統合テスト雛形の生成

### 2.1 目的

仕様から「実装に対する統合テストの雛形」を機械可読 JSON で吐く。
これが仕様→実装の橋の第一歩(DESIGN-v1.md §10)。LLM はこの JSON を読んで
実装言語のテストコードに変換する。

### 2.2 CLI

```
fslc scenarios <file.fsl> [--depth K]
```

- verify と同じ BMC 機構で以下を収集して stdout に単一 JSON を出力:
  1. **reachable ごとの witness トレース**(= ハッピーパスのシナリオ)
  2. **アクションごとの enabled 最短トレース**(coverage 検査の副産物。
     そのアクションを最後に1回実行して終わるトレース)
  3. **デッドロックトレース**(found のとき。「これ以上何もできない状態」は
     実装では終端状態テストになる)
- 仕様が violated の場合は verify と同じ violated JSON を返して exit 1
  (壊れた仕様からシナリオは作らない)。

### 2.3 出力 JSON

```json
{
  "fsl": "1.0",
  "result": "scenarios",
  "spec": "ShoppingCart",
  "depth": 8,
  "scenarios": [
    {
      "name": "reach_SoldOut",
      "kind": "reachable",
      "property": "SoldOut",
      "steps": [
        { "action": "add_to_cart", "params": {"u": 0, "i": 0} },
        { "action": "checkout",    "params": {"u": 0} }
      ],
      "initial_state": { ... },
      "expected_states": [ {...}, {...} ],   // 各 step 後の論理状態(witness と同形)
      "final_check": "SoldOut"
    },
    {
      "name": "cover_remove_from_cart",
      "kind": "action_coverage",
      "action": "remove_from_cart",
      "steps": [ ... ],
      "initial_state": { ... },
      "expected_states": [ ... ]
    },
    {
      "name": "deadlock_terminal",
      "kind": "deadlock",
      "steps": [ ... ],
      "note": "after these steps no action is enabled"
    }
  ]
}
```

- `steps[].params` は表示形式(enum 名など)で verify の `last_action.params` と同形。
- `expected_states` は witness の `state` と同形(内部名なし)。
- LLM への含意: 「`initial_state` をセットアップ → 各 step を実装 API 呼び出しに
  変換 → 各ステップ後に `expected_states[i]` の **言及されたフィールドのみ**
  assert」という変換規約を JSON 内 `"convention"` フィールドで一文説明する。

### 2.4 実装ノート

- witness / coverage トレースの抽出は verify が既に内部で持っている
  (reachables の witness、coverage の step-by-step 探索)。scenarios は
  coverage 探索で「enabled になった時点のモデル」から具体トレースを構築する
  点だけが新規(現状は sat/unsat の bool しか取っていない)。
- `run_verify` と同様の `run_scenarios(path, depth)` を cli.py に追加し、
  exit code は 0(生成成功)/ 1(violated)/ 2,3(エラー)。

## 3. テスト計画

1. cart_v1 の scenarios: `reach_SoldOut` が存在し、steps を順に「手で実行」した
   ときの状態遷移が `expected_states` と一致する(Python でシミュレートして検証
   するテストを書く — bmc の遷移と同じ結果になることの整合性検査を兼ねる)。
2. coverage シナリオ: 全アクションに `cover_<name>` が生成される。
3. 阻まれたアクションのある仕様(requires が恒偽)で:
   - verify: `action_coverage.<name>.blocking_requires` に当該 requires の loc
   - scenarios: 当該アクションのシナリオは生成されず、`warnings` に説明
4. 既存スキーマ互換: coverage true のアクションが bool のままであること
   (既存テストが無修正で通ること)。
5. deadlock シナリオ: test_warnings_format_and_deadlock_trace の DeadEnd 仕様で
   `deadlock_terminal` シナリオが出ること。
