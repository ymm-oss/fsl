---
name: fsl
description: >-
  FSL (AI-Native Formal Specification Language) で仕様を書き・検証し・修復する。
  .fsl ファイルの作成/編集/検証、fslc コマンド(check/verify/scenarios/replay/
  testgen/refine)の実行、形式仕様・モデル検査・invariant 証明・仕様からのテスト
  生成・refinement 検査・実装の適合性検査を行うときに使う。「仕様を書いて」
  「形式検証して」「この仕様を証明して」「FSL」などが合図。
---

# FSL — 仕様の書き方と write→verify→repair ループ

FSL は学習データに存在しない言語。**記憶で書かず、本書と reference.md に従うこと。**
構文の詳細・全式カタログ・イディオム集は同ディレクトリの `reference.md` を読む
(仕様を書く前に必ず一読)。リポジトリ内なら `docs/LANGUAGE.md` が完全版、
`specs/*.fsl` が動く実例(cart_v1 が基本形、mutex_queue が Seq+leadsTo、
bank_* が refinement+compose の実例)。

## 前提: 検証器 fslc

このスキルは言語知識を供給するだけで、検証は CLI `fslc` が行う。未導入なら
FSL リポジトリ(`pyproject.toml` のあるルート)で `pip install -e .` する
(依存は lark と z3-solver のみ、ネイティブビルド不要)。`fslc` が PATH に
無い環境では `python -m fslc ...` でも同じ。

## 実行方法

```bash
fslc <subcommand> ...            # editable install 済みの場合
python -m fslc <subcommand> ...  # または venv の python で
```

出力は常に stdout への単一 JSON。exit: 0=成功(verified/proved/生成)、
1=性質不成立(violated/reachable_failed/unknown_cti/nonconformant)、
2=仕様エラー(parse/type/semantics/io)、3=内部エラー。

## 標準ワークフロー(proved を標準とする)

1. 仕様を書く → `fslc check file.fsl`(構文・型のみ、速い。エラーの `loc`/`expected`/`hint` に従って修正)
2. `fslc verify file.fsl --depth 8` → 結果ごとの対応は下表
3. verified になったら `fslc verify file.fsl --engine induction` → `proved` で完了
4. 必要に応じて: `fslc scenarios`(統合テスト雛形 JSON)、`fslc testgen -o test_x.py`
   (実装適合 pytest 雛形)、`fslc replay --trace events.json`(ログ適合性)、
   `fslc refine impl.fsl abs.fsl mapping.fsl`(詳細仕様の忠実性検査)

## 修復プロトコル(結果 → 次の一手)

| result / violation_kind | 意味 | 次の一手 |
|---|---|---|
| `violated` / `invariant` | 反例あり(trace は最短) | trace の `changes` と `violating_bindings` を読み、ガード追加か invariant 修正 |
| `violated` / `type_bound` | 有界型が範囲外(自動検査) | `last_action` のガード不足。`requires` で範囲を守る(invariant を手書きしない) |
| `violated` / `partial_op` | 空 Seq の pop/head、添字範囲外 | `requires q.size() > 0` か `if q.size() > 0 { … }` でガード |
| `violated` / `ensures` | 事後条件不成立 | 本体と ensures のどちらが正かを判断して修正 |
| `violated` / `leadsTo` | 応答性質の反例(ラッソ/停滞) | trace の `loop_start` を確認。進行を担うアクションに `fair` を付けるか仕様を修正 |
| `reachable_failed` | 到達したい状態に届かない | `action_coverage` の `blocking_requires`(unsat core)を読む。ガード緩和/アクション追加/`--depth` 増 |
| `unknown_cti` | invariant は真だが帰納的でない | **CTI の開始状態 = 全 invariant を満たす幽霊状態。それを排除する補助 invariant(ドメインの真実であるもの)を追加して再実行。** 実績: 1ラウンド収束(例: 「キューに重複なし」「返金は Captured のみ」) |
| `error` / `parse` | 構文エラー | `loc` と `expected`(候補トークン)に従う |
| `error` / `type` | 型エラー | `hint` に従う(例: `x == some(e)` → `x is some(v)` で束縛して比較) |
| `error` / `semantics` | 二重代入など | 同一パスで同じ変数に2回代入しない(if の then/else は別パスなので可) |
| `error` / `vacuous` | init が充足不能(矛盾した代入など) | init を見直す。1つの状態変数に矛盾する値を与えていないか確認。範囲外の値による違反は別物で `violated`/`type_bound` になる |

coverage が `false` のアクションは `blocking_requires` が「どの requires が
阻んでいるか」を句単位で特定している。silent に無視しないこと。

## 最小構文(詳細・全カタログは reference.md)

下記はそのまま `fslc check` を通る自己完結の雛形(Map/Option/Seq の要素型は
全てドメイン型として宣言してある — **使う型は必ず `type ... = lo..hi` か `enum`
で宣言する**。未宣言だと `unknown type` の型エラーになる):

```fsl
spec Cart {
  const CAP = 3
  type ItemId = 0..1
  type UserId = 0..1
  type JobId  = 0..1
  type Qty    = 0..5                     // ドメイン型 = 有界整数。範囲は自動検査
  enum St { Open, Closed }
  struct Order { st: St, qty: Qty, buyer: Option<UserId> }

  state {
    stock: Map<ItemId, Qty>,
    cart:  Option<ItemId>,
    q:     Seq<JobId, CAP>
  }
  init {
    forall i: ItemId { stock[i] = 1 }
    cart = none
    q = Seq {}
  }

  action add_to_cart(i: ItemId) {
    requires cart == none
    cart = some(i)
  }

  fair action abandon() {                // 常に可能なので Served(下記)が成立する
    requires cart != none
    cart = none
  }

  fair action checkout(u: UserId) {      // fair = 弱公平(leadsTo 用)
    requires cart is some(i)             // i がここで束縛される
    requires stock[i] > 0
    stock[i] = stock[i] - 1              // 右辺は全て旧状態を読む(同時代入)
    cart = none
    ensures stock[i] == old(stock[i]) - 1
  }

  // 「stock[i] >= 0」のような境界 invariant は書かない(Qty=0..5 で自動検査)。
  // 下は非・境界の真の安全性 invariant の例(<式> の位置)。
  invariant QueueStaysEmpty { q.size() == 0 }   // q を触る action が無いので不変
  reachable SoldOut { stock[0] == 0 }           // witness が返る
  leadsTo Served { cart is some(j) ~> cart == none }   // ~> は leadsTo 専用
}
```

## 絶対に守る規則(構造的落とし穴)

- **番兵値(-1 等)禁止 → `Option<T>`**。`x == some(e)` は型エラー —
  `x is some(v)` で取り出す。`== none` / `!= none` は可。
- **「0以上」系 invariant を手書きしない** → `type Qty = 0..N` で自動検査される。
- 同一実行パスでの**二重代入はエラー**。if の後に分岐内と同じ変数へ代入もエラー。
- Set/Seq の更新は**再代入**: `s = s.add(x)`、`q = q.pop()`。
- Seq の `pop/head/at` は**必ずガード**(requires か if)。忘れは partial_op で検出される。
- invariant で Seq を語るときは添字ガード:
  `forall i in 0..CAP-1 { i < q.size() => P(q.at(i)) }`(範囲は `0..CAP-1` と
  const から導出して書く — リテラルをハードコードすると容量変更に追従しない)。
- 「X の後に Y が起きた」という**履歴**は状態で書けない → ゴースト変数
  (`ever_locked` 等)を足すか、応答性質なら `leadsTo`。

## 高度な機能(必要になったら reference.md の該当節)

- **Seq の集約**: `sum(i: Idx of log.at(i) where i < log.size())`(Idx は容量を覆うドメイン型)
- **合成**: `compose X { use A as a from "a.fsl" ... }`、同期アクション
  `action s(..) = a.act(..) || b.act2(..) { .. }`、`internal a.act`
- **refinement**: 写像ファイル(`map abs_var = 式`、`action impl -> abs(..) | stutter`、
  写像式限定の `if c then a else b`)+ `fslc refine`
- **実装接続**: `fslc testgen` 生成ファイルの Adapter(reset/step/observe)を
  実装に結線。observe は仕様の論理状態と同形(enum は名前、Option は None|値、
  Seq は list、合成は `alias.var` キー)
