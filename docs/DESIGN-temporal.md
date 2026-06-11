# FSL v2.0-lite — 有界 `leadsTo` と公平性注釈 実装設計

DESIGN-v1.md §10 v2.0 の最初の2項目。動機は DOGFOOD-1 F1 / DOGFOOD-2 F7:
「X の後にいつか Y」(応答性質)が状態のみでは書けない。

## 1. 構文

```fsl
fair action release_handoff() { ... }       // 公平性注釈(弱公平)

leadsTo WaiterGetsLock {
  forall p: ProcId {
    waiters.contains(p) ~> (holder is some(h) and h == p)
  }
}
```

- `leadsTo <Name> { <lt> }` をトップレベル項目に追加。
  `lt := <expr> "~>" <expr> | "forall" binder "{" lt "}"`(forall は外側にのみ、
  ネスト可。`~>` は leadsTo ブロック内専用の演算子で、一般式には使えない)。
- `fair` はアクション定義の前置修飾子。意味は**弱公平**(そのインスタンスが
  連続して enabled であり続けるなら、いつかは実行される)。

## 2. 意味論

`P ~> Q`: 全実行で「P が成立した時点から、同時点を含むいつか Q が成立する」。
(P と Q が同時成立なら直ちに満たされる。)

反例は無限実行であり、有限状態系では**ラッソ**(prefix + 繰り返しループ)
または**デッドロック停滞**(その状態で永遠に停止)として有限表現できる:

### 2.1 ラッソ反例

位置 i < j ≤ K について、

```
loop(i, j)   := states[j] ==L states[i]                  // 論理状態等価(§2.3)
violation    := ∃ i < j ≤ K, ∃ p < j:
                  loop(i, j)
                ∧ P(states[p])
                ∧ ∀ q ∈ [min(i, p), j-1]: ¬Q(states[q])
                ∧ fairness_ok(i, j)                       // §2.2
```

- `¬Q` の範囲が `[min(i,p), j-1]` なのは、ループ内の全状態は無限に再訪される
  ため、p がループ内にあっても prefix にあっても「p 以降+ループ全体」で
  Q が一度も成立しないことが必要だから。
- K は `--depth` を流用。展開は verify の共有展開(`_bmc_explore`)に相乗りし、
  leadsTo ごとに push/pop で1クエリ。i, j, p は K ≤ 10 程度なので
  **有界展開(Or の列挙)**でよい(整数変数より素直でデバッグしやすい)。

### 2.2 弱公平性によるラッソの除外

`fair` の付いたアクションの各インスタンス a について:

```
fairness_ok(i, j) := ∀ a ∈ FairInstances:
    (∃ q ∈ [i, j-1]: ¬enabled_a(states[q]))     // ループ中に一度 disabled になる
  ∨ (∃ q ∈ [i, j-1]: choices[q] == a)           // またはループ中に実行される
```

連続 enabled なのに一度も実行されないループは「現実には起きない」として
反例から除外する。`fair` のないアクションには制約なし。

### 2.3 論理状態等価 `==L`

ループ検出の状態比較は**物理変数の生比較ではなく論理等価**で行う:

- スカラ / Map / Set: 物理変数の等価
- `Option`: present 同士が等しく、`present => value 等しい`(absent 時の
  value は don't care)
- `Seq`: `len` 等しく、`∀ idx < len: data 等しい`(tail は don't care)

生比較だと don't care 部分の差で同一論理状態のループを見逃す
(= 反例の見逃し。有界検査としても精度を落とさないため必須)。

### 2.4 デッドロック停滞反例

states[j] で全アクションが disabled(デッドロック)の場合、実行はそこで
永遠に停滞する:

```
violation_stutter := ∃ j ≤ K, ∃ p ≤ j:
    deadlock(states[j]) ∧ P(states[p]) ∧ ∀ q ∈ [p, j]: ¬Q(states[q])
```

(公平性はデッドロック状態では適用しない — enabled なものが無いため。)

### 2.5 forall 付き leadsTo

`forall x: T { P(x) ~> Q(x) }` はインスタンスごとに展開し、**各束縛で独立に**
§2.1/§2.4 を検査する(1つでも反例があれば violated。反例 JSON に
`bindings` を含める)。

## 3. 検査の位置づけと結果

- **violated(反例あり)は確定的な違反**(ラッソは実在の無限実行)。
- **反例なしは「深さ K まで反例なし」の有界保証**(K を超える prefix を持つ
  ラッソは見えない)。invariant の `verified` と同じ位置づけで、
  `leads_to` フィールドに `checked_to_depth` を載せる。
- `--engine induction` でも leadsTo は **base case(BMC)側で同じ検査**を行い、
  `proved` は invariant に対する主張のままとする(leadsTo の無限深度証明は
  v2.0 本体のスコープ外)。`proved` 出力の `leads_to` にも
  `checked_to_depth` を載せ、note で区別する。

## 4. JSON

### 4.1 違反

```json
{
  "fsl": "1.0",
  "result": "violated",
  "violation_kind": "leadsTo",
  "invariant": "WaiterGetsLock",
  "loc": { "line": 40, "column": 3 },
  "bindings": { "p": 0 },
  "pending_since": 1,
  "trace": [ { "step": 0, ... }, ... ],
  "loop_start": 2,
  "stutter": false,
  "hint": "P held at step 1 but the loop from step 2 can repeat forever without Q; if progress relies on some action being taken eventually, annotate it with `fair action ...`"
}
```

- `trace` は既存形式(state / action / changes)。末尾状態はループ先頭
  (`loop_start`)と論理等価。デッドロック停滞反例では `stutter: true` で
  `loop_start` の代わりに最終ステップで停滞。
- `pending_since`: P が成立した(以後 Q が来ない)ステップ。
- exit code は他の violated と同じ 1。

### 4.2 成功時(verified / proved への追記)

```json
"leads_to": {
  "WaiterGetsLock": { "checked_to_depth": 8 }
}
```

## 5. 実装ノート

- **grammar.py**: `leadsTo_def`、`~>`(`LEADSTO_OP`)、`fair` 修飾子。
  AST: `("leadsto", name, binders, P, Q, loc)`(binders は外側 forall の列)、
  action に `fair: bool`。
- **model.py**: spec dict に `leadstos`、action/instance に `fair` を伝播。
  ホワイトリスト検証は変更なし。`~>` が一般式に現れたら parse エラーになる
  文法にする(式階層に入れない)。
- **bmc.py**:
  - `_logical_eq(spec, s1, s2)` — §2.3 の論理等価を返すヘルパ
    (phys_vars のメタデータ — option の present/value、seq の data/len — を
    使って組み立てる)。
  - leadsTo 検査は `_bmc_explore` 後(verify の reachable 処理と同様)に、
    共有ソルバー上で leadsTo × 束縛ごとに push/pop:
    `s.add(Or over (i,j,p) of [loop ∧ P ∧ ¬Q列 ∧ fairness_ok])` → sat なら
    モデルから (i, j, p) を特定(各 (i,j,p) 候補に selector Bool を付けて
    モデルで読む)してトレース構築。
  - enabled_a は coverage 検査と同じ `_eval_requires` の連言を再利用
    (expr_cache が効く)。
  - デッドロック停滞(§2.4)は既存 deadlock 検査の enabled 式を再利用。
  - 性能: クエリは leadsTo 束縛ごとに1回。式サイズは O(K² · (|P|+|Q|+|Fair|·K))。
    K=8、束縛数 ≤ 容量程度なら問題ない(PERF1 の共有展開上で動く)。
- **cli.py**: 変更最小(violation_kind が増えるだけ)。
- **scenarios**: leadsTo は対象外(将来: pending→達成のトレースを
  シナリオ化する余地をコメントで残す)。

## 6. テスト計画(tests/test_temporal.py)

1. **stutter 反例**: P になった後デッドロックして Q が来ない仕様 →
   violated / leadsTo / stutter: true。
2. **ラッソ反例(公平性なし)**: noop 自己ループがある mutex で
   `waiters.contains(p) ~> holder == some(p)` → violated、trace に
   loop_start、hint に fair の提案。
3. **公平性で証明**: 2 の release_handoff(と必要なら他)に `fair` を付ける
   → 反例消滅(leads_to.checked_to_depth が返る)。noop ループは
   「release_handoff が enabled なのに実行されない」ため除外されることの確認。
4. **同時成立**: P ∧ Q が同時に立つ遷移 → 違反にならない。
5. **forall leadsTo**: 束縛ごとの検査。violated 時に bindings が返る。
6. **論理等価ループ**: Seq の don't-care tail だけが異なる同一論理状態の
   ループが検出される(生比較だと見逃すケースを再現して回帰テスト化)。
7. **既存互換**: leadsTo を含まない仕様の verify/proved 出力が完全不変。
8. **induction との併用**: leadsTo 付き仕様の `--engine induction` が
   proved + leads_to.checked_to_depth を返す。

## 7. ドキュメント反映

- DESIGN-v1.md §10 の v2.0 項目に「実装済み(lite): fair / leadsTo」を注記。
- LANGUAGE.md: §3 に `~>`(leadsTo ブロック専用)、§1 に `fair` と
  `leadsTo`、§6 表に leadsTo、§9 イディオム集に「履歴ゴースト変数 vs
  leadsTo の使い分け」(状態の事実 → ゴースト、応答性質 → leadsTo)を追記。
- mutex_queue.fsl に `fair` + `WaiterGetsLock` を追加して実例とする
  (DOGFOOD-2 F7 の解消)。
