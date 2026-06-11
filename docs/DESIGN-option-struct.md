# FSL v2.1 — struct フィールドの Option<スカラ> 実装設計

DOGFOOD-1 F3 で実需が確認された機能。`struct Res { item: Option<ItemId> }` を
合法化する。設計方針は既存ロワリングの**合成**であり、新しい意味論は導入しない。

## 1. 言語仕様の変更

- struct フィールドに許す型: スカラ(Int / Bool / ドメイン型 / enum)**+
  `Option<スカラ>`**。Set / Map / Seq / struct / `Option<Option<…>>` は
  引き続き check 時拒否(hint は現行文言から「or use Option<scalar>」を
  反映して更新)。
- 式・文は既存の Option と完全に同じ語彙: `s.v == none` / `!= none` /
  `s.v is some(x)`、`s.v = some(e)` / `= none`、リテラル
  `S { v: none }` / `S { v: some(e) }`。`s.v == some(e)` は従来どおり
  型エラー(BUG10 の規則がそのまま適用)。
- struct 全体の `==` / `!=`: Option フィールドは**論理等価**
  (present 同士が等しい ∧ present ⇒ value 等しい)。`_logical_eq` と同じ規約。

## 2. ロワリング(物理分割の合成)

| 論理 | 物理 |
|---|---|
| スカラ `s: S`(S が `v: Option<K>` を持つ) | `s__v__present: Bool`, `s__v__value: K`(他フィールドは従来どおり `s__f`) |
| `m: Map<K2, S>` | `m__v__present: Map<K2, Bool>`, `m__v__value: Map<K2, K>` |

- `expand_phys_var` / `phys_z3_sort` に「struct フィールドが option」の分岐を追加。
- `eval_expr` の `field` 評価: option 型フィールドは `('option_val', present式, value式)`
  を返す(スカラ Option の既存パスと合流)。
- 代入(`compute_updates`): `s.v = some(e)` → present/value の2物理書き込み。
  `s = S { v: none, ... }` の一括代入も両物理を更新。二重代入検出は
  論理フィールド単位(`s.v` への2回代入はエラー、present/value は内部なので
  ユーザーからは見えない)。

## 3. 自動境界(`_bounds_*`)

option フィールドの境界: `present => lo <= value <= hi`(K が有界のとき)。
スカラ Option の既存 `bounds_invariant_expr` の合成で書けるはず。
induction の step 前提にも自動で入る(invariants 経由)。

## 4. 表示

- struct dict の中で `"v": null` または `"v": 値`(スカラ Option と同一規約)。
- changes パス: `res[0][item]` の from/to に null / 値。
- witness / CTI / scenarios / violating_bindings / Monitor.state すべて追従
  (`logical_state_values` と runtime の struct 復元に option 分岐を足す)。

## 5. 波及先(全部対応すること)

1. **model.py**: check_spec ホワイトリスト緩和、expand_phys_var、bounds 生成。
2. **bmc.py**: eval_expr の field 評価 / struct リテラル / struct 等価
   (`_struct_compare` に option フィールドの論理等価)/ compute_updates の
   field 代入と一括代入 / logical_state_values / `_logical_eq`(§2.3 の
   leadsTo 用論理等価も option フィールドを論理で比較)。
3. **runtime.py**: 具象評価器の struct 復元・field 読み書き・等価。
4. **refine.py**: α の物理レベル構築が新しい物理名(`m__v__present` 等)を
   正しく組むこと(map の要素ごと写像で struct 値に option フィールドが
   ある場合を含む)。
5. **compose.py**: 型参照の書き換えは既存機構で通るはず(回帰のみ確認)。
6. **grammar**: 変更不要(型構文は既にパース可能、check で拒否していただけ)。

## 6. 検証(テスト + 実需仕様の書き直し)

1. `tests/test_option_struct.py`:
   - スカラ struct / Map<_, struct> の両方で: init リテラル、field 代入
     (some/none)、`is some(x)` ガード、`== none` requires、struct 全体 `==`、
     自動境界(present 時のみ)、`s.v == some(e)` が型エラー、JSON 表示
     (null / 値、`__present`/`__value` 非漏出)。
   - induction: option フィールド入り仕様が proved になる(幽霊 CTI が
     出ない = bounds 前提の確認)。
   - runtime Monitor: 同じ仕様で step → state 表示一致(verify witness の
     差分再生テストに新仕様を1本足す)。
2. **`specs/inventory_reservation.fsl` を自然な形に書き直す**(F3 の解消):
   `struct Res { st: RState, item: Option<ItemId>, qty: Qty }`、
   init は `item: none`、hold で `some(i)`、release で `none` に戻す。
   invariant `FreeHasNoItem { res[r].st == Free => res[r].item == none }` を
   追加し、Conservation の `res[r].item == i` 条件は
   `res[r].item is some(j) and j == i` 形に変更。
   **書き直し後も verified(depth 5)+ induction proved を維持**すること
   (proved でなくなったら CTI を報告 — 補助 invariant の要否は私が判断する)。
3. 既存115テストは無修正 green(inventory_reservation を参照する差分テストは
   仕様書き直しに自動追従するはず — 壊れたら原因を報告)。

## 7. ドキュメント

- LANGUAGE.md §2 の表とホワイトリスト、§9 イディオム集の F3 回避策の記述を
  「v2.1 から直接書ける」に更新。
- DESIGN-v1.md §3.4 の「struct のネスト不可」の段落に Option フィールド
  解禁の注記。DOGFOOD-1 F3 に解消済み注記。
