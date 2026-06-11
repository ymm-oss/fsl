# ドッグフーディング第1回 — 所見 (2026-06-11)

実ドメイン仕様4本(`specs/auth_lockout.fsl`, `specs/inventory_reservation.fsl`,
`specs/payment.fsl`, `specs/rate_limiter.fsl`)+ エッジプローブ7本で v1.0 を実地評価した。

## 結果サマリ

| 仕様 | 結果 |
|---|---|
| auth_lockout (depth 8) | verified。witness: LockedOut@3, RecoveredAfterLock@5。coverage 全 true |
| inventory_reservation (depth 5) | verified(48秒)。AllHeld@3。**depth 8 は推定30分超で打ち切り**(PERF1) |
| payment (depth 6) | verified(4.3秒)。FullyRefunded@3。coverage 全 true |
| rate_limiter (depth 6) | verified(0.2秒)。Exhausted@4。coverage 全 true |

## 新規バグ

### BUG11: struct フィールドの複合型を check が素通しし、verify で内部エラー

- `struct S { v: Option<K> }` → check ok、verify で `kind: "internal", message: "'s__v'"`(生 KeyError)
- `struct Outer { i: Inner }`(struct ネスト、設計 §3.4 で「v1 不可」と明記)→ 同様に `"'o__i'"`
- `struct S { members: Set<K> }` → verify で見当違いな semantics エラー
- **期待動作**: `check_spec` が struct フィールド型を検証し、domain / enum / Bool / Int 以外
  (Option / Set / Map / struct)は `kind: "type"` + hint
  (例: "struct fields must be scalar in v1; model optional fields with an enum state or a separate Map")で拒否。
  修復プロトコル(§8)の「全ての失敗に次の一手」原則に従う。

### BUG12: ネストした if/else の排他分岐を「double assignment」と誤検出

```fsl
action step() {
  if x == 0 { x = 1 }
  else { if x == 1 { x = 2 } else { x = 0 } }
}
```

- → `semantics: double assignment to 'x' on the same execution path`(誤り。3つの代入は全て排他パス)
- 原因: `bmc.py` の `run_into_if`(ネスト if 用)が then/else の評価間で `scalar_writes` を
  save/restore しない。外側 if の `run_branch`(L572-576)は正しくリセットしている。
- **期待動作**: `run_into_if` も分岐ごとに `scalar_writes` を退避・復元し、
  排他パス間の同一変数代入を許す(真の同一パス二重代入は引き続き検出)。

### BUG13: `is some(x)` を含む invariant の違反時に JSON 直列化でクラッシュ

```fsl
invariant Match { c is some(j) => j == target }   // 違反させると…
```

- → `TypeError: Object of type ArithRef is not JSON serializable` の生トレースバック。
  違反検出はできているのに出力で死ぬ(LLM 修復ループにとって最悪の失敗形態)。
- 原因: `eval_expr` の is-pattern が `binds` に Z3 式(ArithRef)を残し、
  `violating_bindings`(bmc.py:935-937)が `_public_bindings(dict(binds))` で
  生の Z3 AST を結果 dict に流す。
- **期待動作**: bindings の値は `model.eval(...)` で具体値化し、enum は表示名へ逆引きして出力。
  あるいは pattern 束縛変数は bindings から除外。違反 JSON は常に直列化可能であること。

### BUG14: if の後の代入が分岐内の書き込みを黙って上書き(検出の非対称)

```fsl
action go() {
  if flag { x = 1 }
  x = 2          // ← エラーにならず、x = 1 が黙って消える
}
```

- `x = 2` を if の**前**に置くと正しく double assignment エラーになるが、**後**に置くと
  素通りし、flag が真でも x は 2 になる(仕様作者の意図と無言で乖離 = 健全性問題)。
- 原因: `compute_updates` の if 処理が分岐評価後に `scalar_writes` を if 以前の状態へ
  復元するため、分岐内の書き込み記録が後続文から見えない。トップレベル
  (`run` の if)とネスト(`run_into_if`)の両方に存在。
- **期待動作**: if の処理後、then/else 各分岐で書かれたスカラキーの**和集合**を
  `scalar_writes` に記録し、後続の同一変数代入をエラーにする。

### PERF1: BMC が深さ方向に指数的(1ステップ約4倍)

- `inventory_reservation.fsl`: 状態 = Map×2(struct 値含む)、アクション3(インスタンス約36/step)、
  sum() 入り invariant×1。実測: depth 2 = 0.46s、depth 4 = 7.8s、depth 5 = 48s
  (約4倍/step)。depth 8 は推定30分超で打ち切り。
- 構造的な要因(bmc.py):
  1. `reachable` ごとに**新しい Solver で全展開をやり直す**(verify 本体 + R 本 × 全展開)
  2. ensures 検査が instance × ensures ごとに `_eval_requires` を再評価して push/pop
  3. 全 struct 代入が ite ツリーを生成し、深さ方向に式が複合的に膨張する可能性
- v1.1 で対応方針を検討(インクリメンタルソルバー共有、式キャッシュ、代入の中間変数化など)。

## 表現力の所見(言語設計へのフィードバック)

- **F1: 「過去」を語る到達性にはゴースト変数が必要。** auth_lockout の
  「ロックされた後に復帰できる」は `ever_locked` ゴースト変数で表現した。
  ワークアラウンドとしては素直だが、v2.0 の `leadsTo` の実例として記録。
- **F2: `is some(j)` の束縛スコープは `=>` の右辺まで届く**(probe2 で確認、verified も正しい)。
  ただし設計書 §3.3 にスコープ規定が無い — 「`is` を含む論理式の中で、`is` の評価が
  true となる文脈でのみ束縛が有効」と明文化すべき。
- **F3: struct に Option フィールドが書けないと不便な実例が出た。**
  inventory_reservation は本来 `Res { item: Option<ItemId> }` と書きたかったが、
  enum 状態(Free のとき item は無意味な 0)で回避した。BUG11 の修正(明確な拒否)を
  v1 の回答とし、Option-in-struct の合成ロワリングは v1.1 候補に積む。
- **F4: 機能の直交組合せは概ね健全。** let 経由の lvalue 添字、struct 一括代入、
  sum の算術式本体+複合 where、count/min/max/abs、Set<Enum>、ゴースト変数、
  const + min によるクランプ — いずれも期待どおり動作(probe2/4/5、auth_lockout で確認)。

## プローブ一覧(回帰テスト化候補)

| プローブ | 内容 | 結果 |
|---|---|---|
| probe1 | Option を struct フィールドに | BUG11(internal error) |
| probe2 | `is some(j)` 束縛が `=>` 右辺に届くか | OK(verified) |
| probe2n | probe2 の負例(violated になるべき) | BUG13(JSON クラッシュ) |
| probe3 | `else { if … else … }` ネスト | BUG12(誤 double assignment) |
| probe4 / 4n | Set<Enum> の正例/負例 | OK / OK(violated@1) |
| probe5 | count/max/abs + reachable で count | OK(witness@2) |
| probe6 | struct ネスト(設計上不可) | BUG11 同類(internal error) |
| probe7 | Set を struct フィールドに | BUG11 同類(誤メッセージ) |
| probe8 / 8r | if の後/前の同一変数代入 | BUG14(後: 素通り / 前: 検出) |

## 対応状況

- BUG11 / BUG12 / BUG13: **修正済み**(codex ラウンド、回帰テスト6件追加、計32テスト green)
- BUG14 / PERF1: 次ラウンドで対応予定
