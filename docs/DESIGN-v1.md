# FSL v1 言語設計書

本書は FSL(AI-Native Formal Specification Language)の v1 設計を定める。
v0 プロトタイプ([`LANGUAGE.md`](LANGUAGE.md))のコンセプトを引き継ぎ、
v0 で明示された制限(型の貧弱さ・安全性のみ・有界のみ・実装との橋なし)に
答える。**v1 は v0 の完全上位互換**であり、既存の `.fsl` はそのまま検証できる。

---

## 1. コンセプトの確認と評価基準

FSL の第一目標は「**生成AIが書き・検証し・修正する**」こと。
v1 のすべての機能追加・却下は、次の 5 基準で判断した。

| # | 基準 | 意味 |
|---|---|---|
| G1 | **生成確率** | LLM が一発で正しく書ける確率を最大化する。学習分布に近い構文(TS/Python/Rust 風)を選び、同じことを書く方法は原則 1 つにする |
| G2 | **修正可能性** | 失敗したとき、出力 JSON だけから「どこを・なぜ・どうすれば」が機械的に決まる。すべての診断に位置情報と修正ヒントを付ける |
| G3 | **検証の即応性** | 数秒で返る。有界・小スコープがデフォルト。write→verify→repair ループのレイテンシが第一 |
| G4 | **意味の単純さ** | 「1ステップ=1アクションの原子実行」「同時代入」以外の意味論を増やさない。すべての新構文は既存意味論への糖衣か有界展開で説明できること |
| G5 | **落とし穴の構造的排除** | 番兵値(-1)・非有界量化・空虚な仕様・暗黙の範囲逸脱といった「LLM がやりがちな仕様バグ」を、型と自動チェックで言語から消す |

v0 自身が G5 違反の見本を含んでいる:サンプルの `cart: Map<Int, Int>` は
「-1 = 空」という番兵値を使い、`NoNegativeStock` は本来「在庫数は 0 以上の量」
という**型の事実**を invariant として手書きしている。v1 の型システムは
この 2 つを言語側で吸収する。

---

## 2. v1 で何が変わるか(総覧)

| 領域 | v0 | v1 |
|---|---|---|
| ドメイン | `const MAXU = 1` + `u in 0..MAXU` を毎回書く | `type UserId = 0..1` を宣言し `u: UserId` で参照 |
| 値の不在 | `-1` などの番兵値 | `Option<T>`(`none` / `some(e)` / `is some(x)`) |
| 状態の語彙 | Int の魔法数 | `enum Status { Draft, Placed, ... }` |
| エンティティ | 平行するマップ群を手で管理 | `struct Order { status: Status, qty: Qty }` |
| 集まり | `Map<Int, Bool>` を手で特性関数化 | `Set<T>`(`contains` / `add` / `remove` / `size`) |
| 範囲逸脱 | invariant を手書き | 有界型の**自動境界チェック**(暗黙 invariant) |
| アクション本体 | 代入と forall のみ | `let` / `if-else` / `ensures`(事後条件) |
| 集約 | 書けない | `count(...)` / `sum(...)`(有界展開) |
| 性質 | invariant(安全性)のみ | + `reachable`(到達可能性=シナリオ検査) |
| 自動チェック | action coverage、init 充足性 | + 型境界、デッドロック検査 |
| 出力 JSON | 全状態のみのトレース | + ステップ間 **state diff**、全診断に `loc`、スキーマバージョン |
| CLI | `verify` のみ | + `check`(構文・型のみの高速ループ用) |
| 証明 | 有界のみ | k 帰納法エンジンと CTI 修復プロトコルを規定(実装は v1.1) |

**却下した案**(理由は各節):文字列型、非有界量化、フル LTL、修飾付き enum
参照(`Status.Paid`)、モジュール/インポート、ユーザー定義関数。

---

## 3. 型システム

### 3.1 ドメイン型(有界部分範囲)

```fsl
type UserId = 0..2        // 0,1,2 の 3 値
type Qty    = 0..5
```

- 整数の有界部分範囲に名前を付ける。範囲の両端はコンパイル時整数
  (リテラル、`const`、その四則)。
- **量化のドメインになる**: `forall u: UserId { ... }`。v0 の
  `forall u in 0..MAXU:` の繰り返し記述を消す(G1: 範囲の不一致という
  典型的な生成ミスを構造的に防ぐ)。
- **自動境界チェック**: ドメイン型を持つ状態変数(マップの値・struct の
  フィールドを含む、再帰的)には、暗黙の invariant
  `_bounds_<変数名>` が生成され、ユーザー invariant と同様に検査される。
  init を含む全到達状態で範囲逸脱があれば `violated` になる(§7.4)。
  - 設計判断: 境界は **assume(前提)ではなく check(検査)** する。
    assume にすると範囲逸脱バグが「その状態は存在しない」ことにされて
    隠蔽される(G5)。
  - 帰結: v0 サンプルの `NoNegativeStock` は `type Qty = 0..N` を使えば
    **書かなくても自動検出される**。

### 3.2 enum

```fsl
enum Status { Draft, Placed, Paid, Shipped, Cancelled }
```

- メンバ名は spec 全体でグローバルに一意でなければならない(重複は
  `name` エラー)。参照は非修飾(`Placed`)のみ。
  - 却下: `Status.Paid` 修飾形の併存。書き方が 2 つになると LLM の出力が
    揺れ、diff も汚れる(G1)。一意性チェックで衝突は検出できる。
- トレース・反例 JSON には**メンバ名がそのまま**現れる(`"status": "Paid"`)。
  数値エンコードを LLM に見せない(G2)。

### 3.3 Option

```fsl
state { cart: Map<UserId, Option<ItemId>> }
```

- リテラル `none` / `some(式)`。
- 判定と取り出しは `is` パターンで行う:
  - `cart[u] is none` — 空である
  - `cart[u] is some(i)` — 値があり、**`i` を束縛する**。`requires` に
    書いた場合、束縛はそのアクション本体の残り全体(後続の requires・
    代入・ensures)で使える
  - 単純比較 `cart[u] == none` / `!= none` も可(束縛が不要なとき)
  - Option の `==` / `!=` が許されるのは **`none` との比較のみ**。
    `x == some(e)` や Option 同士の比較は `type` エラーとし、
    `is some(v)` への書き換えヒントを返す(G1「同じことを書く方法は
    1 つ」、G5「サイレントな誤りの排除」)
- 部分関数(`value(x)` / `x!` のような unwrap)は**提供しない**。
  ガードなし unwrap という未定義動作の入り口を作らないため(G5)。
  取り出しは必ず `is some(x)` を通る=全域。
- JSON 上は `null` または値で表示される。

### 3.4 struct

```fsl
struct Order { status: Status, qty: Qty }
state  { orders: Map<OrderId, Order> }
```

- フィールドアクセス `orders[o].status`、フィールド単位の代入
  `orders[o].status = Shipped`、リテラル
  `orders[o] = Order { status: Draft, qty: 0 }`。
- 等価 `==` はフィールドごとの等価。struct のネスト(struct を含む
  struct)は v1 では不可(ロワリングと表示の単純さを優先。必要になった
  実例が出てから検討)。

### 3.5 Set

```fsl
state { shipped: Set<OrderId> }
```

- 要素型は有界型(ドメイン型・enum)に限る。
- 操作はメソッド風(G1: LLM の手癖に一致):
  - `s.contains(e)` : Bool
  - `s.add(e)` / `s.remove(e)` : 新しい集合(式。代入の右辺で使う)
  - `s.size()` : Int
- リテラル: `Set {}`(空)、`Set { 0, 2 }`。
- JSON 上は**ソート済み配列**で表示(`"shipped": [0, 2]`)。

### 3.6 Seq(v1.1)

```fsl
const CAP = 3
state {
  queue: Seq<JobId, CAP>,
  log:   Seq<Qty, 5>
}
```

- 容量 `N` 付きの FIFO 列。要素型 `T` はスカラ型のみ。`N` は正の定数式
  (整数リテラルまたは `const` 名)。
- **状態変数の型としてのみ**使用可能(struct フィールド・Map の値・
  Set の要素・Seq の要素には不可 — `check` で `kind: "type"` + hint)。
- 操作(純粋・再代入イディオム): `size()` / `push(e)` / `pop()` /
  `head()` / `at(i)` / `contains(e)` / `==` / `!=`。
- リテラル: `Seq {}` / `Seq { 1, 2 }`(要素数 ≤ N)。
- `pop()` / `head()` / `at(i)` は部分関数。アクション本体・`requires`・
  `ensures` 内では **暗黙の well-definedness 検査**(`partial_op`)が付く。
  `requires q.size() > 0` 等のガードイディオムと併用する(G5)。
- 満杯時の `push` は暗黙境界 invariant `_bounds_<変数>` 違反
  (`violation_kind: "type_bound"`)。
- JSON 上は長さプレフィックスの配列(`"queue": [1, 2]`、空は `[]`)。
  diff は列全体の `{from, to}`。

### 3.7 Map

- `Map<K, V>`: **K は有界型(ドメイン型・enum)でなければならない**。
  これによりトレース表示が全域になり、量化・集約が常に有界になる。
- v0 互換: `Map<Int, ·>` は引き続き受理するが、`fslc check` /
  `verify` の `warnings` に非推奨警告と機械的な書き換えヒント
  (「`type K = 0..N` を宣言して置換せよ」)を載せる。

### 3.8 Int / Bool

- そのまま残す。`Int` は非有界(Z3 整数)。集計値(売上合計など)に使う。
  非有界変数には自動境界チェックは付かない。

### 3.9 文字列は提供しない(設計判断)

仕様レベルで文字列の中身に意味があることはまれで、ほぼ常に「有限個の
区別される値」で十分。`enum` か不透明なドメイン型で表す。
Z3 の文字列理論は遅く G3 に反し、LLM は文字列比較の表記揺れで
ミスしやすい(G5)。エラーメッセージでこの方針へ誘導する。

---

## 4. 構文

### 4.1 文法(EBNF)

```ebnf
spec          ::= "spec" NAME "{" item* "}"
item          ::= const_def | type_def | enum_def | struct_def
                | state_def | init_def | action_def
                | invariant_def | reachable_def

const_def     ::= "const" NAME "=" const_expr
type_def      ::= "type" NAME "=" const_expr ".." const_expr
enum_def      ::= "enum" NAME "{" NAME ("," NAME)* ","? "}"
struct_def    ::= "struct" NAME "{" field ("," field)* ","? "}"
field         ::= NAME ":" type

state_def     ::= "state" "{" var_decl ("," var_decl)* ","? "}"
var_decl      ::= NAME ":" type
type          ::= "Int" | "Bool" | NAME            // NAME = ドメイン/enum/struct
                | "Map" "<" type "," type ">"
                | "Set" "<" type ">"
                | "Seq" "<" type "," const_expr ">"
                | "Option" "<" type ">"

init_def      ::= "init" "{" stmt* "}"

action_def    ::= "action" NAME "(" (param ("," param)*)? ")" "{" action_item* "}"
param         ::= NAME ":" NAME                     // 有界型
                | NAME "in" const_expr ".." const_expr   // v0 互換
action_item   ::= "requires" expr
                | "ensures" expr
                | "let" NAME "=" expr
                | stmt

stmt          ::= lvalue "=" expr
                | "if" expr "{" stmt* "}" ("else" "{" stmt* "}")?
                | "forall" binder ":"? "{" stmt* "}"
lvalue        ::= NAME ("[" expr "]")? ("." NAME)?
binder        ::= NAME ":" NAME ("where" expr)?
                | NAME "in" const_expr ".." const_expr   // v0 互換

invariant_def ::= "invariant" NAME "{" expr "}"
reachable_def ::= "reachable" NAME "{" expr "}"
```

式:

```ebnf
expr        ::= quant | imp
quant       ::= ("forall" | "exists") binder ("{" expr "}" | ":" expr)
imp         ::= or_e ("=>" imp)?                    // 右結合
or_e        ::= and_e ("or" and_e)*
and_e       ::= not_e ("and" not_e)*
not_e       ::= "not" not_e | is_e
is_e        ::= cmp ("is" pattern)?
pattern     ::= "none" | "some" "(" NAME ")"
cmp         ::= sum (("==" | "!=" | "<" | "<=" | ">" | ">=") sum)?
sum         ::= product (("+" | "-") product)*
product     ::= unary ("*" unary)*
unary       ::= "-" unary | postfix
postfix     ::= atom ("[" expr "]" | "." NAME
                     | "." ("contains" | "add" | "remove" | "push" | "pop"
                            | "head" | "at" | "size")
                       "(" (expr ("," expr)*)? ")")*
atom        ::= INT | "true" | "false" | "none"
              | "some" "(" expr ")"
              | "Set" "{" (expr ("," expr)*)? "}"
              | "Seq" "{" (expr ("," expr)*)? "}"
              | NAME "{" NAME ":" expr ("," NAME ":" expr)* "}"  // struct リテラル
              | "count" "(" NAME ":" NAME "where" expr ")"
              | "sum" "(" NAME ":" NAME "of" expr ("where" expr)? ")"
              | "min" "(" expr "," expr ")"
              | "max" "(" expr "," expr ")"
              | "abs" "(" expr ")"
              | "old" "(" expr ")"                  // ensures 内のみ
              | NAME
              | "(" expr ")"
```

予約語: `spec state init action requires ensures invariant reachable
const type enum struct let if else forall exists in where is and or not
true false none some old count sum min max abs Int Bool Map Set Seq Option`

### 4.2 量化と集約

- `forall u: UserId { 式 }` / `exists i: ItemId { 式 }` — ドメイン型・enum
  上の有界量化。v0 形式 `forall i in 0..MAXI: 式` も引き続き有効。
- `where` 付きは糖衣: `forall x: T where p { q }` ≡ `forall x: T { p => q }`、
  `exists x: T where p { q }` ≡ `exists x: T { p and q }`。
- `count(o: OrderId where 述語)` — 述語を満たす個体数。
- `sum(o: OrderId of 式 [where 述語])` — 式の総和(述語を満たす個体のみ)。
- すべて宣言済みドメイン上の**有界展開**であり、非有界量化は書けない
  (G3/G5: 構文レベルで排除)。

### 4.3 スタイル規約(生成 LLM 向け正準形)

仕様の正準的な書き方を言語仕様の一部として定める。生成のたびに表記が
揺れると diff ベースの修復が壊れるため(G2)。

- インデント 2 スペース、1 行 1 文。
- 命名: アクション = `snake_case` の動詞句、invariant / reachable /
  type / enum / struct = `PascalCase`、const = `UPPER_SNAKE`。
- `requires` はアクション本体の先頭にまとめる(`let`・`is some` 束縛が
  必要な場合のみ間に挟む)。`ensures` は末尾。
- v1 の新規仕様ではドメイン型を使い、`const` + `in lo..hi` は使わない。

---

## 5. アクションの意味論

仕様は遷移系 (S, I, →) を定める。S は状態変数の付値全体、I は `init` を
満たす状態の集合、→ は以下で定まる遷移関係。v0 の意味論を一切変えずに
拡張する(G4)。

1. **アクションインスタンス** = アクション名 × パラメータ値(宣言された
   有界ドメインの全組み合わせを列挙)。
2. **enabled**: すべての `requires` が現状態 σ で真(`is some(x)` は
   「値が存在する」が真、かつ x をその値に束縛)。
3. **1 ステップ** = enabled なインスタンスのうち**いずれか 1 つ**が
   原子的に実行される(インターリービング)。非決定性はこの選択のみ。
   enabled なインスタンスの更新は決定的。
4. **同時代入**: 本体の右辺・`if` 条件・`let` の値はすべて**旧状態** σ を
   読む。代入されなかった変数は変化しない(フレーム条件は自動)。
5. **`let x = 式`**: 旧状態で評価した値を束縛。宣言以降の requires・文・
   ensures で使える。
6. **`if c { ... } else { ... }`**: c は旧状態で評価。実行されなかった
   分岐の変数はフレーム条件に従い不変。
7. **書き込み衝突**:
   - 同一実行経路上で同じ**スカラ変数**(または同じ struct フィールド)へ
     2 度代入 → `semantics` エラー(ほぼ確実にバグ)。
   - **マップ**は同一セルへの書き込みをテキスト順で合成(後勝ち)。
     `forall` ループで別セルに書く通常用途は無干渉。構文上同一の添字へ
     2 度書いている場合は `check` が警告。
8. **`ensures p`**: 事後条件。enabled で遷移 σ → σ' が起きたとき、p が
   σ' で真であることを検査する。p の中の `old(式)` は σ で評価される。
   違反は invariant 違反と同形式の反例(§7.2)で報告する。
   - 採用理由: Dafny / 契約プログラミングの形で LLM の学習分布に強く
     存在し(G1)、「更新を書き間違えた」ことをそのアクションの行に
     局所化して報告できる(G2)。

---

## 6. 性質と自動チェック

### 6.1 invariant(安全性)

v0 と同じ。初期状態を含む全到達状態で成立を要求。

v0 では invariant が 1 つもないとエラーだったが、v1 では型境界の暗黙
invariant が常に存在するため**エラーにしない**(警告は出す)。

### 6.2 reachable(到達可能性=シナリオ検査)

```fsl
reachable FullLifecycle {
  exists o: OrderId { orders[o].status == Shipped }
}
```

「この状態に**到達できる経路が存在する**」ことの表明。アプリ仕様の
「ユーザーは購入を完了できる」というハッピーパスの検査に当たる。

- BMC では深さ K 以内の充足判定そのもので、追加コストはほぼない(G3)。
- 成功時は**witness トレース**(その状態に至る実行列)を JSON で返す。
  これは v2 の実装橋(統合テスト雛形の生成)の入力になる。
- 失敗は「ガードが強すぎる/init が間違っている」ことの兆候であり、
  action coverage(空虚性検査)の一般化に当たる。
- 設計判断: フル LTL や `eventually` は採用しない。公平性のない
  インターリービング+有界検査では「いつか必ず」は意味を持たず、
  LLM にも人間にも誤解を招く(G4)。活性は v2 で公平性注釈とともに
  導入する(§10)。

### 6.3 自動チェック(仕様に書かなくても常に走る)

| チェック | 内容 | 報告 |
|---|---|---|
| init 充足性 | 初期状態が存在するか | `error` / `kind: "vacuous"`(v0 と同じ) |
| 型境界 | 有界型の全状態変数が全到達状態で範囲内か | `violated` / `invariant: "_bounds_<var>"` |
| action coverage | 各アクションが深さ K 以内に一度でも enabled になるか | `verified` 内 `action_coverage` + 警告(v0 と同じ) |
| デッドロック | enabled なインスタンスが 1 つもない到達状態の有無 | 既定は警告+到達トレース。`--deadlock=error` で `violated` に昇格、`--deadlock=ignore` で抑止(意図的な終端状態を持つ仕様向け) |

---

## 7. 検証器インターフェース

### 7.1 CLI

```
fslc check  <file.fsl>                          # 構文・名前・型検査のみ(高速ループ用)
fslc verify <file.fsl> [--depth K]              # BMC(既定 K=8)
                       [--engine bmc|induction] # induction: §9
                       [--k N]                  # 最大帰納深さ(既定 1、induction のみ)
                       [--deadlock warn|error|ignore]
```

- 出力は常に **stdout への単一 JSON オブジェクト**(v0 と同じ)。
- 終了コード: `0` = verified / proved(全 reachable 充足を含む)、
  `1` = violated / reachable_failed、`2` = 仕様エラー(parse/type/…)、
  `3` = 検証器内部エラー。
- BMC は深さ 0 から順に検査するため、返る反例は**最短**である(保証として
  明文化。LLM に渡すトレースは短いほど修復精度が上がる)。

### 7.2 出力 JSON スキーマ v1

全出力は共通エンベロープを持つ:

```json
{ "fsl": "1.0", "result": "...", "spec": "OrderWorkflow", ... }
```

**verified(有界検証成功):**

```json
{
  "fsl": "1.0",
  "result": "verified",
  "spec": "OrderWorkflow",
  "depth": 8,
  "invariants_checked": ["ShippedWasPaid", "RevenueConsistent", "_bounds_orders"],
  "reachables": {
    "FullLifecycle": { "witnessed_at_step": 3, "witness": [ /* トレース */ ] }
  },
  "action_coverage": { "place": true, "pay": true, "ship": true, "cancel": true },
  "deadlock": { "found": false },
  "warnings": [],
  "note": "bounded verification: no violation within depth 8"
}
```

**violated(invariant / ensures / 型境界の違反):**

```json
{
  "fsl": "1.0",
  "result": "violated",
  "spec": "ShoppingCart",
  "violation_kind": "invariant",        // "invariant" | "ensures" | "type_bound" | "partial_op" | "deadlock"
  "invariant": "_bounds_stock",
  "loc": { "line": 8, "column": 5 },    // 違反した性質(または ensures)の位置
  "violated_at_step": 4,
  "violating_bindings": [ { "i": 0 } ],
  "last_action": { "name": "checkout", "params": { "u": 1 },
                   "loc": { "line": 24, "column": 3 } },
  "trace": [
    { "step": 0,
      "state": { "stock": { "0": 1, "1": 1 }, "cart": { "0": null, "1": null } } },
    { "step": 1,
      "action": { "name": "add_to_cart", "params": { "u": 0, "i": 0 } },
      "changes": { "cart[0]": { "from": null, "to": 0 } },
      "state": { "stock": { "0": 1, "1": 1 }, "cart": { "0": 0, "1": null } } },
    ...
  ]
}
```

v0 からの差分:

- `changes`: 各ステップの**状態差分**。キーは射影パスの平坦文字列
  (`"stock[0]"`, `"orders[2].status"`)、値は `{from, to}`。LLM は全状態を
  読まずに「どのアクションが何を壊したか」を追える(G2)。
- `last_action`: 違反直前に実行されたアクションと**その定義位置**。
  修復の第一候補(requires の追加先)を直接指す。
- 値の表示: enum はメンバ名、Option は `null`/値、Set はソート済み配列。
- `violating_bindings` は入れ子 forall に一般化(`[{"u":1,"i":0}]` の形)。

**reachable_failed(シナリオ到達不能):**

```json
{
  "fsl": "1.0",
  "result": "reachable_failed",
  "spec": "OrderWorkflow",
  "unreached": [ { "name": "FullLifecycle", "loc": { "line": 40, "column": 3 } } ],
  "depth": 8,
  "action_coverage": { "place": true, "pay": false, "ship": false, "cancel": true },
  "hint": "within depth 8 no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth"
}
```

**error(構文・名前・型・意味エラー):**

```json
{
  "fsl": "1.0",
  "result": "error",
  "kind": "type",            // "parse" | "name" | "type" | "semantics" | "io" | "internal"
  "loc": { "line": 12, "column": 18 },
  "message": "map key type must be a bounded type (domain or enum), got Int",
  "expected": "a declared domain type, e.g. `type ItemId = 0..N`",
  "hint": "declare `type K = 0..<max>` and use `Map<K, ...>`"
}
```

エラー分類は固定の閉集合とし、各分類が持つフィールドをスキーマで保証する
(`parse`/`name`/`type`/`semantics` は必ず `loc` を持つ)。

### 7.3 値の表示とロワリングの不可視性

内部エンコーディング(enum の整数化、Option の存在ビット、struct の
フィールド分割)は **JSON に一切漏らさない**。LLM が見る語彙は仕様の
語彙と一致させる(G2)。これはスキーマ上の保証であり、テスト対象とする。

---

## 8. 修復プロトコル(LLM の行動指針)

各 `result` に対する推奨の機械的修復手順。fslc のドキュメントおよび
システムプロンプト素材として提供する。

| result / kind | 読むべきフィールド | 推奨アクション |
|---|---|---|
| `error` / `parse` | `loc`, `expected` | 当該行の構文を `expected` に従い修正。再 `check` |
| `error` / `name`・`type` | `loc`, `hint` | 宣言の追加・型の変更。再 `check` |
| `violated` / `invariant`・`type_bound` | `last_action`, `changes`, `violating_bindings` | まず `last_action` の `requires` 不足を疑う(最頻のバグ)。トレース上の `changes` が意図通りなら invariant 側の誤りを疑う |
| `violated` / `ensures` | `last_action`, `changes` | 当該アクションの更新式と ensures のどちらが仕様意図か判断して片方を直す |
| `violated` / `partial_op` | `last_action`, `hint`, `trace` | `requires q.size() > 0` 等でガードを追加するか、空状態で発火しないよう requires を強化する |
| `violated` / `deadlock` | `trace` | 終端状態が意図的なら `--deadlock=ignore`、そうでなければガードを弱めるか脱出アクションを追加 |
| `reachable_failed` | `action_coverage`, `hint` | coverage が false のアクションの `requires` と `init` を疑う。次に `--depth` を増やす |
| `verified` だが `action_coverage` に false | `warnings` | 空虚性。requires と init の矛盾を修正 |
| `unknown_cti`(v1.1, §9) | `cti` | CTI 状態を排除する補助 invariant を追加して再実行 |

設計上の含意: **このテーブルが閉じている**(どの出力にも次の一手がある)
ことが v1 の出力スキーマの設計要件である。新しい診断を追加するときは、
推奨アクションを同時に定義しなければならない。

---

## 9. 帰納的証明エンジン(v1.1)

詳細アルゴリズム・健全性条件・テスト計画は `docs/DESIGN-induction.md` を参照。

`--engine induction` は k 帰納法で invariant の**無限深度証明**を試みる。

- **base**: 通常 BMC(深さ `--depth`)。違反すれば通常の `violated`。
- **step**: 自由状態列 σ₀..σₖ(init なし)で「全 invariant 成立かつ連続遷移」を
  仮定し、対象 invariant が σₖ で破れるかを Z3 で判定。全 invariant が
  unsat → `result: "proved"`。sat → `unknown_cti`(到達可能とは限らない CTI)。
- induction 出力に `deadlock` フィールドは含めない。

**proved:**

```json
{
  "fsl": "1.0",
  "result": "proved",
  "spec": "OrderWorkflow",
  "engine": "induction",
  "k_used": { "ShippedWasPaid": 1, "RevenueConsistent": 2 },
  "base_depth": 8,
  "invariants_checked": ["ShippedWasPaid", "RevenueConsistent", "_bounds_orders"],
  "action_coverage": { "place": true, "pay": true },
  "reachables": { "FullLifecycle": { "witnessed_at_step": 3, "witness": [ ... ] } },
  "warnings": []
}
```

**unknown_cti:**

```json
{
  "fsl": "1.0",
  "result": "unknown_cti",
  "spec": "OrderWorkflow",
  "invariant": "RevenueConsistent",
  "k": 2,
  "cti": {
    "states": [
      { "step": 0, "state": { ... } },
      { "step": 1, "state": { ... }, "action": { ... }, "changes": { ... } },
      { "step": 2, "state": { ... }, "action": { ... }, "changes": { ... } }
    ],
    "violated_at": 2
  },
  "hint": "this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run"
}
```

CTI →「補助 invariant の提案」は LLM が得意な帰納的一般化であり、
人間の専門家が行う invariant 強化ループをそのまま write→verify→repair
ループに乗せる。これが FSL の AI ネイティブ設計の v1 における中心的な賭けである。

---

## 10. ロードマップ

- **v1.0(本書のコア)**: 型システム(ドメイン/enum/Option/struct/Set)、
  `let`/`if`/`is`、`ensures`、`reachable`、`count`/`sum`、自動境界チェック、
  デッドロック検査、JSON v1(diff・loc・スキーマ版数)、`fslc check`。
- **v1.1**: k 帰納法エンジン(§9、実装済み)、`fslc scenarios`(reachable witness と
  coverage トレースから統合テスト雛形 JSON を生成=実装との橋の第一歩)、
  `Seq<T, N>`(容量付き列。配列+長さでエンコード、**実装済み**)、unsat core による
  「どの requires が enabled を阻んでいるか」ヒント。
- **v2.0**: 公平性注釈(`fair action ...`)と有界 `leadsTo`(**実装済み lite**:
  `DESIGN-temporal.md` 参照)、実装橋の本体(**実装済み**:
  `DESIGN-bridge.md` — `fslc.runtime.Monitor` / `fslc replay` / `fslc testgen`)、
  複数 spec の合成(**実装済み**: `DESIGN-compose.md` — `compose` / 同期アクション /
  `internal` / `specs/order_system.fsl`)と refinement(**実装済み**:
  `DESIGN-refinement.md` — `fslc refine`)。**v2.0 ロードマップの項目はすべて実装済み。**

---

## 11. v0 からの移行

v0 仕様は無修正で v1 検証器を通る(完全上位互換)。ただし以下の非推奨
警告が `warnings` に載り、それぞれ機械的書き換えヒントを伴う:

| v0 の書き方 | v1 の推奨 |
|---|---|
| `const MAXI = 1` + `i in 0..MAXI` | `type ItemId = 0..1` + `i: ItemId` |
| `Map<Int, V>` | `Map<ItemId, V>`(有界キー) |
| 番兵値(`-1` = 空) | `Option<T>` |
| 「値は 0 以上」の手書き invariant | ドメイン型の自動境界チェック |

---

## 12. 実装方針(現行コードベースへのロワリング)

すべての新機能は既存の `grammar.py` / `model.py` / `bmc.py` の構造
(タプル AST → spec dict → Z3 有界展開)の上に、**意味論を変えずに**
ロワリングできる:

| 構文 | ロワリング |
|---|---|
| ドメイン型 | Int + 範囲メタデータ。量化・パラメータ展開は既存の `eval_const` 機構をそのまま使用。境界は暗黙 invariant として `spec["invariants"]` に追加 |
| enum | `0..n-1` の Int。表示時にメンバ名へ逆引き |
| `Option<T>` | (present: Bool, value: T) のペア。`Map<K, Option<V>>` はマップ 2 本。`is some(x)` → present 制約+束縛 |
| struct | フィールドごとに変数/マップを分割(`orders__status` など)。`==` はフィールド等価の連言 |
| `Set<T>` | `Map<T, Bool>`(特性関数)。`size()` は有界和 Σ ite(m[i],1,0) |
| `count` / `sum` | 有界展開(既存の forall 展開と同型) |
| `if` | 分岐ごとに pend を計算し、変数ごとに `ite(c, then, else)` で合成 |
| `let` / `is some` 束縛 | `binds` 辞書の拡張(現在の量化束縛と同機構。Int 値に加え Z3 式の束縛を許す) |
| `ensures` | 各遷移の事後状態に対する追加チェック(BMC の invariant 検査と同じ push/pop) |
| `reachable` | 各深さでの充足判定(invariant 検査と極性が逆なだけ) |
| デッドロック | 「全インスタンスの requires の連言の否定」の充足判定 |
| `changes` diff | トレース構築時に隣接状態の表示値を比較するだけ(検証コストゼロ) |
| 位置情報 `loc` | Lark の `propagate_positions=True` で AST にメタデータを付与 |

---

## 付録 A: v1 によるショッピングカート

v0 の `cart_buggy.fsl` / `cart_fixed.fsl` と同じモデルの v1 版。
番兵値が消え、`NoNegativeStock` は型に吸収される。

```fsl
spec ShoppingCart {
  type UserId = 0..1
  type ItemId = 0..1
  type Qty    = 0..3        // 在庫は 0..3 — 負になれば自動的に違反

  state {
    stock: Map<ItemId, Qty>,
    cart:  Map<UserId, Option<ItemId>>
  }

  init {
    forall i: ItemId { stock[i] = 1 }
    forall u: UserId { cart[u] = none }
  }

  action add_to_cart(u: UserId, i: ItemId) {
    requires cart[u] == none
    cart[u] = some(i)
  }

  action remove_from_cart(u: UserId) {
    requires cart[u] != none
    cart[u] = none
  }

  action checkout(u: UserId) {
    requires cart[u] is some(i)
    requires stock[i] > 0          // ← この行を消すと _bounds_stock 違反が最短 4 手で返る
    stock[i] = stock[i] - 1
    cart[u] = none
    ensures stock[i] == old(stock[i]) - 1
  }

  reachable SoldOut {
    forall i: ItemId { stock[i] == 0 }   // 全在庫を売り切る経路が存在する(ガード過剰の検出)
  }
}
```

invariant を 1 行も書いていないのに、バグ版(在庫ガードなし)は
`_bounds_stock` 違反として検出される。これが §1 G5「落とし穴の構造的排除」
の具体例である。

## 付録 B: 注文ワークフロー(enum / struct / Set / 集約のショーケース)

```fsl
spec OrderWorkflow {
  type OrderId = 0..2
  type Qty     = 0..5

  enum Status { Draft, Placed, Paid, Shipped, Cancelled }

  struct Order { status: Status, qty: Qty }

  state {
    orders:  Map<OrderId, Order>,
    shipped: Set<OrderId>,
    revenue: Int
  }

  init {
    forall o: OrderId { orders[o] = Order { status: Draft, qty: 0 } }
    shipped = Set {}
    revenue = 0
  }

  action place(o: OrderId, q: Qty) {
    requires orders[o].status == Draft
    requires q > 0
    orders[o].status = Placed
    orders[o].qty = q
  }

  action pay(o: OrderId) {
    requires orders[o].status == Placed
    orders[o].status = Paid
    revenue = revenue + orders[o].qty
  }

  action ship(o: OrderId) {
    requires orders[o].status == Paid
    orders[o].status = Shipped
    shipped = shipped.add(o)
    ensures shipped.contains(o)
  }

  action cancel(o: OrderId) {
    requires orders[o].status == Placed or orders[o].status == Paid
    if orders[o].status == Paid {
      revenue = revenue - orders[o].qty
    }
    orders[o].status = Cancelled
  }

  invariant ShippedWasPaid {
    forall o: OrderId { shipped.contains(o) => orders[o].status == Shipped }
  }

  invariant RevenueConsistent {
    revenue == sum(o: OrderId of orders[o].qty
                   where orders[o].status == Paid or orders[o].status == Shipped)
  }

  invariant NonNegativeRevenue { revenue >= 0 }

  reachable FullLifecycle {
    exists o: OrderId { orders[o].status == Shipped }
  }
}
```

`RevenueConsistent` のような**集計の整合性**は、アプリ開発で最も多い
仕様バグ(返金漏れ・二重計上)に対応し、v0 では表現できなかったクラスの
性質である。
