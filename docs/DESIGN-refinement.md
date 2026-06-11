# FSL v2.0 — refinement 検査 実装設計

DESIGN-v1.md §10 v2.0「複数 spec の合成と refinement」の refinement 側。
「詳細仕様(impl)が抽象仕様(abs)の振る舞いを外れない」ことを
**refinement mapping による有界シミュレーション検査**で確かめる。

ユースケース: 抽象仕様を先に proved にし、実装に近い詳細仕様(キャッシュ、
中間状態、最適化を含む)が抽象仕様に**忠実**であることを機械検査する。
LLM ワークフローでは「abs を人間/LLM がレビュー → impl は LLM が自由に
詳細化 → refine 検査が忠実性を担保」という分業になる。

## 1. マッピングファイル

第3のファイル(impl/abs どちらの spec も汚さない):

```fsl
refinement CartImplRefinesCart {
  impl CartImpl                       // 詳細仕様の spec 名(ファイルは CLI で渡す)
  abs  ShoppingCart                   // 抽象仕様の spec 名

  // 抽象状態変数ごとに、impl 状態からの写像式を与える(全変数必須)
  map stock[i: ItemId] = impl_stock[i] - reserved[i]
  map cart[u: UserId]  = impl_cart[u]
  map revenue          = ledger

  // impl アクション → abs アクションの対応(全 impl アクション必須)
  action impl_checkout(u) -> checkout(u)     // パラメータは式でもよい
  action rebalance(i)     -> stutter          // 内部アクション(absでは何も起きない)
}
```

- `map <abs_var> = <式>` — スカラ抽象変数。式は impl の状態変数・const を参照。
- `map <abs_var>[<binder>] = <式>` — Map/Seq 等の要素ごと写像。binder は
  abs 側のキー型を走る(Seq は `map q = <impl の Seq 式>` の全体写像のみ —
  v2.0 では Seq は同型写像(impl 側にも Seq があり式で渡す)に限る)。
- `action <impl_action>(<仮引数列>) -> <abs_action>(<式列>) | stutter`
  仮引数は impl アクションのパラメータ名(順序一致)。abs 側引数は
  それらと impl 状態を使う式。
- 文法は既存 .fsl と同居させない(`refinement` をトップレベルに持つ
  **独立ファイル**。parse は同じ Lark 文法に `refinement_def` を足す)。

## 2. 検査の意味論(有界前方シミュレーション)

α(s) := マッピングが定める impl 状態 → abs 状態の写像。

1. **init 対応**: impl の初期状態 s₀ について、α(s₀) が abs の init 制約を
   満たすこと。反例: `refinement_failed` / `at: "init"`。
2. **遷移対応**: impl の到達可能な遷移 s →[a, params] s' について:
   - `a -> stutter` の場合: **α(s') == α(s)**(論理等価 §は leadsTo の
     `_logical_eq` を流用)。
   - `a(p…) -> b(e…)` の場合: abs アクション b のインスタンス(引数 = e の
     評価値)が α(s) で **enabled**(requires 成立)であり、かつ b の更新を
     α(s) に適用した結果が **α(s') と論理等価**であること。
3. 検査は impl の BMC 展開(深さ K)上で行う: 各ステップ t、各 impl
   インスタンスについて「choice が当該インスタンス ∧ 対応条件の否定」が
   sat なら違反。impl のトレース + α 前後の abs 状態を反例として返す。

abs 側の invariant は検査しない(abs を別途 verify/prove するのが前提。
ただし α(s₀..s_K) が abs invariant を破る場合は通常それ自体が遷移対応違反に
現れる)。abs 側の自動境界(_bounds_*)も同様にスコープ外 — ただし
α の値が abs の型範囲を外れることは遷移対応違反として自然に検出される
(b の更新結果と一致し得ないため)とは限らないので、**α(s_t) の型境界
検査だけは追加で行う**(`map_out_of_bounds` 違反。写像式のバグの典型を
直接指摘できる)。

**検査順序**: 各ステップ t について、遷移対応の検査(s_{t-1}→s_t、t=0 は
init 対応のみ)を、α(s_t) の型境界検査より**先**に行う。境界検査は t=0 の
初期状態と、直前ステップの遷移対応が成立した後の α(s_t) に対してのみ適用する。
ガード緩和などで遷移対応と境界違反が同時に起きる場合、根本原因である
`abs_requires_failed` を優先して報告する。写像式のバグで対応は成立するが
範囲だけ逸脱する場合は `map_out_of_bounds` のまま。

## 3. CLI / JSON

```
fslc refine <impl.fsl> <abs.fsl> <mapping.fsl> [--depth K]
```

成功:

```json
{ "fsl": "1.0", "result": "refines", "impl": "CartImpl", "abs": "ShoppingCart",
  "checked_to_depth": 8,
  "action_map": { "impl_checkout": "checkout", "rebalance": "stutter" } }
```

違反:

```json
{ "fsl": "1.0", "result": "refinement_failed",
  "impl": "CartImpl", "abs": "ShoppingCart",
  "at": "init" | "step",
  "violated_at_step": 3,
  "impl_action": { "name": "rebalance", "params": {...}, "loc": ... },
  "kind": "abs_requires_failed" | "abs_state_mismatch" | "stutter_changed_abs"
        | "map_out_of_bounds",
  "impl_trace": [ ...既存トレース形式... ],
  "abs_before": { ...α(s) の論理状態... },
  "abs_after_expected": { ...b 適用後... } | null,
  "abs_after_actual": { ...α(s') ... },
  "mismatch": ["stock[1]", ...],            // 等価が破れた論理パス(分かる範囲)
  "hint": "the impl step does not correspond to the mapped abs action; fix the map expressions, the action correspondence, or guard the impl action" }
```

exit: refines = 0、refinement_failed = 1、エラー = 2/3。

静的検査(`kind: "type"` エラー、exit 2):
- map されていない abs 状態変数 / 存在しない変数・アクション名
- 対応の無い impl アクション
- 写像式・引数式の型不一致(abs 側の期待型と照合)
- abs に ensures がある場合: 対応検査は「requires + 本体更新」で行うため
  ensures は **abs 側で別途検証済みであること**を前提とする(note に明記)

## 4. 実装ノート

- 2つの spec を同一 Z3 コンテキストで扱う。abs 側の状態は具象変数を
  作らず、**α(s_t) を式として構築**する(map 式を impl 状態変数上で
  評価したものを abs の論理変数に対応付ける dict)。abs アクションの
  requires / 更新は既存 `eval_expr` / `compute_updates` に
  「状態 dict = α の式 dict」を渡せばそのまま動く(物理変数名が一致する
  ように α を**物理レベル**で組み立てる: Option は present/value、
  struct はフィールド分割、Seq は data/len)。
- Map の要素ごと写像 `map stock[i] = 式` は、abs の物理 Map 変数に対する
  Lambda/Store 構築ではなく、**読み出し側で代替**する: abs 式評価中の
  `Select(stock, k)` を `式[i := k]` に置換できるよう、α を「物理変数名 →
  (Z3 式 または キー付き式テンプレート)」として持ち、eval_expr の var 解決
  にフックを足す…のは侵襲的なので、**キーを有界列挙して Z3 の
  K(ArraySort) + Store の連鎖で具象 Array 式を組む**(キーは有界なので
  Store の列で正確に書ける)。こちらが既存コードに手を入れずに済む。
- stutter / 対応の検査式は step t ごと・インスタンスごとに push/pop。
  PERF1 の共有展開・式キャッシュの上で動かす。
- 反例の `abs_before/after` 表示は `logical_state_values` を α の式 dict を
  モデルで評価した値に適用。

## 5. テスト計画(tests/test_refine.py)+ サンプル

サンプル: `specs/cart_impl.fsl`(ShoppingCart の詳細化。例: 予約済み在庫
`reserved` を持ち、`reserve`(stutter 相当の内部状態変更だが abs の stock を
変えないよう map が `impl_stock - reserved` で吸収)→ `impl_checkout` が
reserve 済みを消費)+ `specs/cart_refines.fsl`(マッピング)。

1. **正例**: cart_impl が ShoppingCart を refine する(refines / exit 0)。
2. **stutter 違反**: 内部アクションが map 後の abs 状態を変えてしまう改変
   → stutter_changed_abs、mismatch に変数パス。
3. **requires 違反**: impl がガードを緩めた改変(abs の checkout の
   `stock[i] > 0` に対応する状況を impl が許す)→ abs_requires_failed。
4. **更新不一致**: map 式のバグ(符号ミス等)→ abs_state_mismatch。
5. **init 不一致** → at: "init"。
6. **静的検査**: map 漏れ / 未知アクション / 対応漏れ → kind: type、exit 2。
7. **境界**: 写像値が abs の型範囲外 → map_out_of_bounds。
8. 既存機能の回帰なし(refine は完全に独立した CLI パス)。

## 6. ドキュメント反映

- LANGUAGE.md に「refinement」節(マッピング構文・検査内容・ワークフロー)。
- DESIGN-v1.md §10 に注記。README にコマンド追記。
