# FSL v2.0 — spec 合成(compose)実装設計

DESIGN-v1.md §10 v2.0「複数 spec の合成」。複数の検証済みコンポーネント仕様を
**名前空間付きインターリービング合成**で1つのシステム仕様にする。

設計の核: 合成は**フロントエンドの AST 変換**(プレフィックス付きマージ)として
定義する。lowering 後は通常の単一 spec になるため、BMC / k帰納法 / scenarios /
coverage 診断 / leadsTo / runtime Monitor / replay / testgen / refine が
**一切の変更なしに**合成仕様に対して動く。

## 1. 構文

```fsl
compose OrderSystem {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  // 合成側の追加状態(任意)
  state { orders_linked: Int }
  init  { orders_linked = 0 }

  // 同期アクション: 複数コンポーネントのアクションを同時に1ステップで実行
  action checkout_and_pay(u: cart.UserId, p: pay.PayId) =
      cart.checkout(u) || pay.capture(p) {
    requires pay.payments[p].amount > 0
    orders_linked = orders_linked + 1
  }

  // コンポーネントのアクションを単独実行から外す(同期経由でのみ発火)
  internal cart.checkout
  internal pay.capture

  // 横断 invariant / reachable / leadsTo(alias.var で参照)
  invariant LinkedNonNeg { orders_linked >= 0 }
  reachable PaidOrder {
    exists p: pay.PayId { pay.payments[p].st == Captured }
  }
}
```

- `use <SpecName> as <alias> from "<相対パス>"`: パスは compose ファイルの
  ディレクトリ基準。spec 名はファイル内の spec 名と一致必須。alias は
  compose 内で一意。compose の中で compose は **不可**(v2.0 は1段)。
- コンポーネントの型・状態・アクションは `alias.Name` で参照する。
- **同期アクション** `action <name>(<params>) = <a>.<act>(<引数式>...) [ || <b>.<act2>(...) ]* { 追加items }`:
  - requires = 各コンポーネントアクションの requires の連言 + 追加 requires
  - 本体 = 各アクションの本体の和 + 追加文(同時代入。書き込み先は
    コンポーネントごとに素である上、追加文は合成側状態のみ書けるため
    衝突しない — 同一コンポーネントのアクションを2つ同期するのは不可)
  - ensures も各アクションのものを引き継ぐ
  - 引数式は合成側パラメータ・const を使える
- `internal <alias>.<action>`: そのコンポーネントアクションを単独の
  インターリービングから除外(同期アクション経由でのみ実行される)。
- 通常の `action`(`=` なし)も書ける(グルーアクション。合成側状態と
  コンポーネント状態を `alias.var` で直接読み書き)。

## 2. 意味論(= 変換の定義)

`build_spec` の前段で compose を**単一 spec の AST に展開**する:

1. 各 use について対象ファイルを parse し、全宣言名(型・enum メンバ含む
  型名前空間・状態変数・アクション・invariant/reachable/leadsTo 名)に
  `<alias>__` プレフィックスを付け、本体内の参照を書き換える。
  enum **メンバ名**はプレフィックスしない(型が名前空間を持つため衝突しない
  — ただし2コンポーネントが同名メンバを持つ enum を export しても、
  メンバ解決は型文脈で行われるので曖昧になる場合のみ check エラー)。
2. compose 本体の `alias.x` 参照を `alias__x` に書き換える。
3. 同期アクションを §1 の規則でフラットな action に展開する
  (パラメータ → 引数式の代入は let 束縛として注入)。
4. `internal` 指定のアクションを spec の action リストから除去し、
  同期アクションの展開で本体を**コピー**して使う。
5. コンポーネントの invariant / 自動 _bounds_ / reachable / leadsTo / fair は
  プレフィックス名でそのまま生き残る(全部検査される)。
6. 表示メタデータ: 物理変数 `alias__x` は JSON 上 `alias.x` と表示する
  (logical_state_values の表示名マップ。トレース・witness・CTI・scenarios・
  Monitor の state すべてが追従)。

静的検査(check 段階、`kind: "type"`):
- use のファイル不在 / spec 名不一致 / alias 重複
- `alias.x` の解決失敗(未知の alias・変数・アクション)
- 同期アクションで同一コンポーネントの複数アクション参照
- 同期引数の型不一致、internal 対象の不在
- compose 内の compose

## 3. CLI

新コマンドなし。`fslc check / verify / scenarios / replay / testgen` が
compose ファイルをそのまま受け付ける(parse 結果が compose なら展開してから
通常処理)。`fslc refine` の impl 側に compose を渡すことも可能(展開後は
単一 spec のため)。

## 4. 実装ノート

- grammar.py: `compose_def`(use / internal / sync action)。`alias.x` は
  既存の `field` 構文と衝突する(`o.st` と同形)— compose 展開時は
  「alias 集合に入っている名前への field アクセス」を名前空間参照として
  書き換え、それ以外は従来どおり struct フィールドとして扱う
  (展開は compose の場合のみ走るので既存仕様への影響なし)。
- 新モジュール `src/fslc/compose.py`: `expand_compose(ast, base_dir) -> ast`。
  parse 済み AST のタプルを書き換える純粋変換。`parser.parse` に
  `base_dir` を渡せるよう CLI から配線(ライブラリ API は
  `parse(src, base_dir=...)` 互換追加)。
- 表示名マップ: spec dict に `display_names: {phys/logical: "cart.stock"}` を
  持たせ、`logical_state_values` 他の表示箇所で引く。runtime.py も同じ
  マップを使う。
- ファイル読み込みは check/verify の io エラー処理に乗せる
  (`kind: "io"`、どの use かを loc で指す)。

## 5. テスト計画(tests/test_compose.py)+ サンプル

サンプル: `specs/order_system.fsl`(§1 とほぼ同じ: cart_v1 + payment、
checkout と capture の同期、横断 reachable)。

1. **正例**: order_system が verified(coverage 全 true、PaidOrder witness)。
   `--engine induction` で proved(コンポーネントの補助 invariant が
   そのまま効くこと)。
2. **同期の意味論**: checkout_and_pay 後、cart 側の在庫減と pay 側の
   Captured が**同一ステップ**で起きている(witness の changes で確認)。
3. **internal**: cart.checkout 単独が coverage に現れない(アクション一覧に
   ない)こと。internal を外すと単独でも発火し得ること。
4. **横断 invariant 違反**: わざと壊した合成(グルーアクションで
   orders_linked を負に)→ violated、トレースの状態キーが `cart.stock` 形。
5. **静的検査**: alias 重複 / 未知 alias / spec 名不一致 / 同一コンポーネント
   2アクション同期 → kind: type。ファイル不在 → kind: io。
6. **表示**: JSON 全出力(witness / scenarios / Monitor.state)で
   `cart.stock` 表示・`__` 非漏出。
7. **runtime**: Monitor が compose ファイルでそのまま動く(replay 含む)。
8. 既存仕様の回帰なし(compose を含まないファイルは従来パス)。

## 6. ドキュメント反映

- LANGUAGE.md「合成」節(構文・同期アクションの意味論・internal)。
- README / DESIGN-v1.md §10 注記。
