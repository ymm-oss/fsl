# FSL — AI-Native Formal Specification Language (v0 プロトタイプ)

FSL は、**生成AIが書き・検証し・修正する**ことを第一の設計目標とした、
アプリケーション開発向けの形式仕様言語です。

## 設計原則

| 原則 | 既存言語 (TLA+/Alloy) | FSL |
|---|---|---|
| 構文 | 数学記法 (∀, □, ◇) | TypeScript/Python 風。LLMの学習分布に寄せる |
| 反例 | 人間向けテキスト | **構造化JSON**(状態差分・違反した束縛変数つき) |
| エラー | 人間向けメッセージ | 機械可読(行・列・分類)— LLMの修正ループ用 |
| 検証 | フル検証が前提 | **デフォルトで有界・高速**(小スコープ仮説) |
| 空虚性 | 専門家の勘で発見 | アクション到達可能性を自動チェック |
| ドメイン | 汎用集合論 | アプリ開発の語彙(状態・アクション・不変条件) |

## 構文 (v0)

```
spec <Name> {
  const <NAME> = <int式>            // スコープ定数

  state {
    <var>: Int | Bool | Map<Int, Int> | Map<Int, Bool>
  }

  init {
    <代入>...                        // 初期状態の定義
    forall i in lo..hi: { <代入>... }
  }

  action <name>(p in lo..hi, ...) {  // パラメータは有界範囲
    requires <式>                    // ガード(事前条件)。複数可
    <var> = <式>                     // 次状態への代入(同時代入意味論)
    <map>[<式>] = <式>
    forall i in lo..hi: { ... }
  }

  invariant <Name> { <式> }          // 全到達状態で成立すべき性質
}
```

### 式

- 算術: `+ - *`、単項 `-`
- 比較: `== != < <= > >=`
- 論理: `and or not =>`
- 量化(有界): `forall i in lo..hi: <式>`、`exists i in lo..hi: <式>`
- マップ参照: `m[idx]`(idx は任意の式でよい)
- コメント: `//`

### 意味論

- **遷移系**: 1ステップ = いずれか1つのアクションインスタンス
  (アクション名 × パラメータ値)が原子的に実行される(インターリービング)。
- **同時代入**: アクション本体の右辺はすべて**旧状態**を読む。
  代入されなかった変数は変化しない(フレーム条件は自動)。
- **requires**: すべて成立するときのみアクションは実行可能(enabled)。
- **invariant**: 初期状態を含む全到達状態で成立を要求。

## 検証器 `fslc`

```
python3 fslc.py verify <file.fsl> [--depth K]
```

Z3 による**有界モデル検査(BMC)**: 初期状態から深さ K までの
全実行を記号的に展開し、各ステップで全 invariant を検査します。

### 出力(常にJSON、stdout)

**検証成功:**
```json
{
  "result": "verified",
  "spec": "ShoppingCart",
  "depth": 8,
  "invariants_checked": ["NoNegativeStock"],
  "action_coverage": { "add_to_cart": true, "checkout": true },
  "warnings": [],
  "note": "bounded verification: no violation within depth 8"
}
```

`action_coverage` が空虚性検査です。深さ K 以内で一度も実行可能に
ならないアクションは `false` になり、「仕様が何も検査していない」
事故(ガードが常に偽など)を検出します。

**違反(反例):**
```json
{
  "result": "violated",
  "invariant": "NoNegativeStock",
  "violated_at_step": 4,
  "violating_bindings": [ { "i": 0 } ],
  "trace": [
    { "step": 0, "state": { "stock": {"0": 1, "1": 1}, "cart": {"0": -1, "1": -1} } },
    { "step": 1, "action": { "name": "add_to_cart", "params": {"u": 0, "i": 0} },
      "state": { ... } },
    ...
  ]
}
```

トレースは「初期状態 → アクション → 状態 → …」の列。
`violating_bindings` は forall 不変条件のうち**どの個体で壊れたか**を特定し、
LLM が修正箇所を絞り込めるようにします。

**エラー(構文・意味):**
```json
{ "result": "error", "kind": "parse", "line": 12, "column": 5, "message": "..." }
```

## v0 の制限と今後

- 型は Int / Bool / Map<Int,·> のみ。文字列・集合・リストは未対応
- 安全性(invariant)のみ。時相性質(`leadsTo` 等)は未実装
- 有界検査のみ。帰納的不変条件による無限深度証明(k-induction)は次段階
- 実装との橋(プロパティテスト生成・実行時モニタ生成)は未実装
- 仕様→自然言語の逆翻訳は LLM 側(Claude)が JSON を読んで行う設計

これらに答える次版の設計は [`DESIGN-v1.md`](DESIGN-v1.md) を参照。
