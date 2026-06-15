# FSL v1.1 — `Seq<T, N>`(容量付き列)実装設計

DESIGN-v1.md §10 の最後の v1.1 項目。FIFO キュー・追記ログなど「順序のある
有界コレクション」を表現する。設計原則は既存と同じ: G1(生成しやすい)、
G5(落とし穴の構造的排除 = 部分関数の暗黙チェック)、Set と同じ代入イディオム。

## 1. 構文と型

```fsl
const CAP = 3
state {
  queue: Seq<JobId, CAP>,    // 容量は const か整数リテラル
  log:   Seq<Qty, 5>
}
```

- `Seq<T, N>`: T は**スカラ型のみ**(ドメイン型 / enum / Bool / Int)。
  N は正の定数式(整数リテラルまたは const 名)。
- 使える位置: **状態変数の型のみ**。struct フィールド(BUG11 検証に追加)、
  Map の値、Set の要素、Seq の要素には使えない — `check` 段階で
  `kind: "type"` + hint で拒否。
- リテラル: `Seq {}` / `Seq { 1, 2 }`(要素数 ≤ N。超過は check 時 type エラー)。
  Set と同様、代入の右辺にのみ書ける。

## 2. 操作(すべて純粋・Set と同じ「再代入」イディオム)

| 式 | 意味 | 部分性 |
|---|---|---|
| `q.size()` | 現在長 | なし |
| `q.push(e)` | 末尾に追加した新しい列 | 満杯時は **type_bound**(§4) |
| `q.pop()` | 先頭を除いた新しい列(FIFO dequeue) | 空のとき **partial_op**(§5) |
| `q.head()` | 先頭要素 | 空のとき **partial_op**(§5) |
| `q.at(i)` | i 番目(0 始まり) | 範囲外で **partial_op**(§5) |
| `q.contains(e)` | ∃i < size: at(i) == e | なし |
| `q == q2` / `!=` | 長さが等しく全要素が等しい(prefix 比較) | なし |

- v1.1 は FIFO に必要な最小セット。スタック用の `pop_last` 等は実例が
  出てから(DESIGN-v1.md の方針)。
- `q = q.push(x)`、`q = q.pop()` のように再代入で使う(Set の `add`/`remove` と同形)。
- 同一アクション内での `q = q.push(a).push(b)` はメソッドチェーンとして合法。

## 3. ロワリング

物理変数(既存の `__` 分割方式):

- `q__data`: `Map<0..N-1, T>`(Z3 Array)
- `q__len`: Int

各操作:

- `size()` → `q__len`
- `push(e)` → data' = Store(data, len, e)、len' = len + 1(無条件。§4 参照)
- `pop()` → data' = ∀i < N-1: data'[i] = data[i+1](シフト)、len' = len - 1
- `head()` → `Select(data, 0)`
- `at(i)` → `Select(data, i)`(i は式。範囲クランプはしない — §5 のチェックが守る)
- `contains(e)` → `Or(And(0 < len, data[0] == e), ..., And(N-1 < len, data[N-1] == e))`
  (N で有界展開)
- `==` → `len1 == len2 ∧ ∀i ∈ [0, N-1]: i < len1 => data1[i] == data2[i]`
- リテラル `Seq { a, b }` → data[0]=a, data[1]=b, len=2(残りは don't care)

len を超える tail の値は **don't care**(制約しない・読まれない・表示しない)。

## 4. 自動境界チェック(`_bounds_q`)

```
0 <= q__len <= N
∧ ∀i ∈ [0, N-1]: i < q__len => (lo <= q__data[i] <= hi)   // T が有界型のとき
```

- 満杯時の `push` は len = N+1 になり `_bounds_q` 違反 → 通常の
  `violated` / `violation_kind: "type_bound"`。修復ヒントはバインディングと
  last_action から自明(requires `q.size() < N` を足す)。
  これは「境界は assume せず check する」の既存設計(§3 設計判断)と同一。
- induction の step 前提にも他の `_bounds_*` と同様に入る(幽霊 CTI 防止)。

## 5. 部分操作の暗黙チェック(`partial_op` — 新 violation_kind)

`pop()` / `head()` / `at(i)` は部分関数。**アクション本体・requires・ensures 内**
に現れた場合、その操作の well-definedness を暗黙の検査として遷移に付ける:

- 検査内容: アクションが発火する遷移において
  `pop`/`head` → `q.size() > 0`、`at(i)` → `0 <= i < q.size()`。
- 違反時は `violated`、`violation_kind: "partial_op"`、`invariant` フィールドは
  `"_partial_<action名>"`、loc は当該式、hint:
  `"guard the action with requires q.size() > 0 (or bound the index)"`。
  トレース付き(ensures 違反と同じ機構に相乗りできる)。
- **requires 内の扱い**(評価順序): requires 連言の評価は短絡しないため、
  `requires q.size() > 0` と `requires q.head() == x` が並ぶ場合、
  partial_op チェックは「**全 requires が成立する遷移**」に対してのみ行う
  (= ガードが落ちる枝では head() の garbage は読まれない扱い)。
  これにより標準イディオム(ガード requires を先に書く)が正しく通る。
- **invariant / reachable 内**: 暗黙チェックは付けない(状態性質に「発火」は
  ないため)。範囲外読みの値は**未規定**(don't care)。ガード付きイディオム
  `forall k in 0..CAP-1 { k < q.size() => P(q.at(k)) }` を LANGUAGE 文書で
  標準形として示す。ガードなしで garbage を読む invariant は
  spurious violation になり得るが、トレースを見れば分かる(v1.1 の割り切り。
  警告は出さない)。

  **エンジン間の差異(既知)**: 無ガードの部分 Seq 演算(`head`/`pop`/`at`)を含む
  invariant は、`verify`/`prove`(BMC)では don't care 値を**記号的**に読む(任意の値で
  反例を探す)一方、runtime の `Monitor`(具象インタプリタ・適合テスト)では具体的に
  範囲外読みとなり `partial_op` を返す。don't care は本質的に「記号的=任意 vs 具象=単一」
  であり、無ガード invariant の両エンジン一致は原理的に保証できない。**ガード付き
  イディオムで書けば両エンジンの結果は一致する**(検証済み)。したがって invariant /
  reachable で部分 Seq 演算を使う場合はサイズガードを付けることを強く推奨する。

## 6. JSON 表示

- 状態表示: `"queue": [1, 2]`(len プレフィックスを論理値の配列で。空は `[]`)
- diff(`changes`): 列全体の from/to(`"queue": {"from": [1], "to": [1, 2]}`)。
  要素単位 diff はしない(シフトで全要素が動くため)。
- `violating_bindings` / CTI / scenarios も同表示(既存の
  `logical_state_values` に seq 分岐を追加するだけで全出力が揃う)。

## 7. check 段階の検証強化(BUG11 の一般化)

state 変数の型を**ホワイトリスト**で検証する(現状 `Map<K, Set<K>>` が
check を素通りして verify で誤メッセージになる問題の修正を含む):

```
scalar   := Int | Bool | domain | enum
state 変数として合法:
  scalar | Option<scalar> | struct(全フィールドが scalar)
  | Map<bounded-scalar, scalar | Option<scalar> | struct>
  | Set<bounded-scalar>
  | Seq<scalar, N>
```

- 上記以外(Map の値に Set/Map/Seq、Set の要素に struct/Option など)は
  check で `kind: "type"` + 「v1 で合法な型の組合せ」を示す hint で拒否。
- 回帰テスト: `Map<K, Set<K>>` が check 段階で type エラーになること。

## 8. テスト計画

1. **FIFO 基本**: push×2 → head が最初の要素、pop → head が2番目、size 整合。
   reachable で witness を確認。
2. **満杯 push**: 容量2に3回 push → `violated` / `type_bound` / `_bounds_*`。
3. **空 pop / 空 head**: ガードなしアクション → `violated` / `partial_op` /
   loc とヒント。ガード付き(requires q.size() > 0)なら verified。
4. **requires 内 head のガードイディオム**: `requires q.size() > 0` +
   `requires q.head() == 0` の2句 → 空状態でアクションが disabled になるだけで
   partial_op 違反にはならない。
5. **at + forall ガードイディオム invariant**: 追記ログの
   `forall k in 0..N-1 { k < log.size() => log.at(k) <= k }` 型が verified。
6. **Seq ==**: `q.push(1) == q2` 形の requires が正しく働く(struct == の
   regression と同型)。
7. **contains**。
8. **JSON 表示**: 状態が配列、内部名(`__data`/`__len`)が出ない。
9. **check 拒否**: struct フィールドの Seq、`Map<K, Seq<...>>`、`Map<K, Set<K>>`、
   要素数 > N のリテラル。
10. **induction**: FIFO 仕様が proved になる(_bounds_q が前提に入ることの確認)。
11. **scenarios**: FIFO 仕様で cover_* と reach_* が生成される。

## 9. ドキュメント反映

- DESIGN-v1.md §3 に Seq 小節と §10 ロードマップの v1.1 完了マークを追記。
- 文法 EBNF(§4)に `Seq` 型とリテラルを追加。
- `violation_kind` の列挙(§7.2)に `"partial_op"` を追加。
