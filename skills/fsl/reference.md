# FSL 言語リファレンスカード(完全・凝縮版)

仕様を書く前にこのファイル全体を読むこと。これは v2.x 時点の全構文・全規則。

## 1. トップレベル構造

```fsl
spec <Name> {
  const <NAME> = <定数式>                 // 整数定数(式可: CAP - 1 等)
  type  <Name> = <lo>..<hi>               // ドメイン型(有界整数)
  enum  <Name> { <Member>, ... }
  struct <Name> { <field>: <型>, ... }    // フィールド: スカラ | Option<スカラ>

  state { <var>: <型>, ... }
  init  { <文>... }                       // 全変数にちょうど1回代入(決定的)

  [fair] action <name>(<p>: <型名>, ...) {
    requires <式>                          // 0個以上。連言。enabled 条件
    let <x> = <式>                         // ローカル束縛
    <文>...
    ensures <式>                           // 0個以上。old(式) で旧状態
  }

  invariant <Name> { <式> }
  reachable <Name> { <式> }
  leadsTo   <Name> { <式> ~> <式> }        // 外側に forall x: T { … } ネスト可
}
```

合成仕様(別形式のトップレベル):

```fsl
compose <Name> {
  use <SpecName> as <alias> from "<相対パス>"   // 複数可。ネスト compose 不可
  state { ... }  init { ... }                    // 合成側の追加状態(任意)
  action <n>(<p>: <alias>.<Type>, ...) =
      <a>.<act>(<式>...) [ || <b>.<act2>(<式>...) ] {  // 同期(原子的に同時実行)
    [requires <式>]... [<文>...]                 // 追加ガード・合成側状態への代入
  }
  internal <alias>.<action>                      // 単独発火を禁止(同期経由のみ)
  invariant/reachable/leadsTo ...                // alias.var で横断参照
}
```

refinement 写像(第3のファイル。`fslc refine impl.fsl abs.fsl this.fsl`):

```fsl
refinement <Name> {
  impl <ImplSpecName>
  abs  <AbsSpecName>
  map <abs_var> = <impl状態の式>                  // スカラ抽象変数
  map <abs_var>[<x>: <KeyType>] = <式>            // Map の要素ごと写像
  // 写像式・引数式の中でのみ条件式可: if <c> then <a> else <b>(else 必須)
  action <impl_act>(<仮引数>...) -> <abs_act>(<式>...) | stutter
}
```

## 2. 型

| 型 | 書き方 | 注意 |
|---|---|---|
| Int / Bool | `n: Int` | Int は無界 |
| ドメイン型 | `type Qty = 0..5` | **自動境界検査**(violated/type_bound) |
| enum | `enum St { A, B }` | メンバは裸の名前で参照・表示 |
| struct | `struct S { f: Qty, o: Option<K> }` | フィールド = スカラ or Option<スカラ>のみ |
| Option<T> | `c: Option<ItemId>` | T はスカラ。`none` / `some(e)` |
| Map<K, V> | `m: Map<ItemId, Qty>` | K は有界スカラ(Int キーは非推奨警告) |
| Set<T> | `s: Set<OrderId>` | T は有界スカラ |
| Seq<T, N> | `q: Seq<JobId, CAP>` | T スカラ、N は正定数。FIFO |

スカラ = Int / Bool / ドメイン型 / enum。
**state 変数のホワイトリスト**: スカラ | Option<スカラ> | struct |
Map<有界スカラ, スカラ|Option|struct> | Set<有界スカラ> | Seq<スカラ, N>。
これ以外(struct ネスト、Map の値に Set/Map/Seq 等)は check が型エラーで拒否。

## 3. 式カタログ

- 算術: `+ - * / %`、単項 `-`、`min(a,b)` `max(a,b)` `abs(a)`
  (`a//b` は `//` 以降がコメントになるため、除算は `a / b` と空白を置く)
- 比較: `== != < <= > >=` / 論理: `and or not =>`
- 量化: `forall x: T { 式 }`、`exists x: T { 式 }`(`where 式` 可)、
  v0形 `forall i in lo..hi: 式` も可(範囲は定数式: `0..CAP-1` 推奨)
- 集約: `count(x: T where 式)`、`sum(x: T of 式 [where 式])`
- Option: `x == none` `x != none` `x is some(v)`(v は以降その論理式内で使える)。
  **`x == some(e)` と Option の算術・大小比較は型エラー**
- struct: リテラル `S { f: 0, o: none }`、`s.f`、`==`(フィールドごと等価。
  Option フィールドは present 一致∧present⇒値一致)
- Set: `Set {}` `Set { 1, 2 }`、`.add(e) .remove(e) .contains(e) .size()`
- Seq: `Seq {}` `Seq { 1, 2 }`(要素数 ≤ N)、`.push(e) .pop() .head() .at(i)
  .contains(e) .size()`、`==`(長さ+全要素)
- ensures 限定: `old(式)` / leadsTo 限定: `P ~> Q` / 写像式限定: `if c then a else b`

## 4. 文(init / action 本体)

- 代入: `x = e`、`m[k] = e`、`m[k].f = e`、`o.f = e`、`o.f = some(e)`
- Set/Seq は再代入: `s = s.add(x)`、`q = q.pop().push(y)`(チェーン可)
- `if 式 { 文... } [else { 文... }]`(else 内 if でネスト可)
- `forall x: T { 文... }`(一括代入)

## 5. 意味論の規則

1. 1ステップ = 1アクションインスタンス(名前×パラメータ)が原子実行。
2. **同時代入**: 本体の全右辺は旧状態を読む。未代入変数は不変(フレーム自動)。
3. **二重代入 = semantics エラー**: 同一パスで同じ変数/フィールドに2回代入。
   then/else は別パス(両方で代入可)。**if の後**に分岐内と同じ変数への代入もエラー。
4. requires 全成立で enabled。ensures は遷移後に検査。
5. Seq の `pop/head/at` と `/` `%` の除数非ゼロはアクション文脈で
   **well-definedness が自動検査**される(partial_op)。
   requires ガードでも if ガードでも可(パス条件は考慮される)。
   invariant/reachable 内の範囲外 at() は不定値 — 必ず `i < q.size() =>` でガード。
6. `fair` = 弱公平: ループ中ずっと enabled な fair インスタンスが一度も実行
   されない無限実行は leadsTo の反例から除外される。

## 6. 自動検査(書かなくても検査される)

型境界(`_bounds_<var>`、Map値・structフィールド・Seq live prefix 含む)/
部分操作(`_partial_<action>`、Seq pop/head/at・除数非ゼロ)/ action coverage(+ unsat core 診断)/
デッドロック(warning、`--deadlock error` で violated)/ leadsTo(ラッソ+停滞)。

## 7. CLI と JSON の要点

```
fslc check <f>                                  # 構文・名前・型のみ
fslc verify <f> [--depth K=8] [--engine bmc|induction] [--k N=1]
               [--deadlock warn|error|ignore]
fslc scenarios <f> [--depth K]                  # reach_* / cover_* / respond_* / deadlock_terminal
fslc replay <f> --trace <events.json>           # conformant | nonconformant
fslc testgen <f> [--depth K] [-o out.py]        # Adapter 雛形 + 適合 pytest
fslc refine <impl> <abs> <mapping> [--depth K]  # refines | refinement_failed
```

- 反例 trace: `[{step, state, action{name,params,loc}, changes{path:{from,to}}}]`。
  最短保証。状態は論理表示(enum 名 / Option は null|値 / Seq は配列 /
  合成は `alias.var` キー)。内部名(`__`)は出ない。
- `unknown_cti`: `cti.states`(k+1状態)+ `violated_at`。開始状態は到達不能な
  幽霊 — 排除する補助 invariant を足す。
- `proved`: `k_used`(invariant→使った k)、reachables/coverage は base case 由来。
- coverage 診断: `{covered: false, blocking_requires: [{loc, text}], hint}`。
- leadsTo 違反: `pending_since` + `loop_start`(ラッソ)or `stutter: true`。

## 8. イディオム(そのまま流用してよい)

```fsl
// 在庫減算のガード(type_bound を防ぐ)
requires stock[i] > 0
// Option の取り出しと比較
requires cart[u] is some(i)
requires stock[i] > 0
// キュー処理(partial_op を防ぐ2形)
action take()  { requires q.size() > 0  x = q.head()  q = q.pop() }
action drain() { if q.size() > 0 { x = q.head()  q = q.pop() } }
// Seq を語る invariant(添字ガード、const 由来の範囲)
invariant I { forall i in 0..CAP-1 { i < q.size() => jobs[q.at(i)].st == Queued } }
// Seq の畳み込み(インデックス・ドメイン型)
type Idx = 0..3
invariant B { balance == sum(i: Idx of log.at(i) where i < log.size()) }
// 2次元データ: Map のネスト不可 → 積ドメイン1本に平坦化し / % で軸復元
const SLOTS = 4
type Cell = 0..11                          // ROOMS*SLOTS - 1
state { holder: Map<Cell, Option<UserId>> }
reachable Room1Full { forall c: Cell { c / SLOTS == 1 => holder[c] != none } }
// 履歴(「一度でも X した」)はゴースト変数
state { ever_locked: Map<UserId, Bool> }   // ロック時に true をセット
// 重複なしキュー(帰納証明の定番補助 invariant)
invariant NoDup { forall i in 0..CAP-1 { forall j in 0..CAP-1 {
  (i < j and j < q.size()) => not (q.at(i) == q.at(j)) } } }
// 状態タグ依存の refinement 写像(写像ファイル内のみ)
map seats[s: SeatId] = if slots[s].st == Sold then slots[s].holder else none
```

## 9. 実装接続(testgen の Adapter 規約)

生成ファイルの `Adapter` を実装に結線する:
- `reset()`: 実装を init と同じ初期状態に
- `step(action, params)`: 1アクション分実行(合成では `"alias.action"` 名も来る)
- `observe() -> dict`: 実装状態を仕様の論理状態形に射影
  (キーは状態変数名/合成は `alias.var`、enum=名前文字列、Option=None|値、
  Seq=list、Map=キー文字列の dict、struct=dict)

ランダムウォークテストは Monitor(仕様の具象インタプリタ)をオラクルに
実装と1ステップずつ突き合わせる。失敗 = 実装と仕様の乖離(どちらが正かは
trace を見て判断)。

## 10. 3層方言(コンサル / 要件 / 設計)

層間は refinement で連鎖: 業務 ⊒ 要件 ⊒ 設計 ⊒ 実装(testgen/replay)。
全方言はカーネルへの AST 展開なので、§7 の全コマンドがそのまま使える。

### 宣言タグ(全層共通)

invariant / reachable / leadsTo / action の `{` 直前に `"ID: 原文"`:
`invariant PaidLedger "REQ-3: 台帳整合" { ... }` →
violated / unknown_cti / coverage 診断 / scenarios に `requirement: {id, text}`。

### business(コンサル層)

```fsl
business ReturnHandling {
  actor Customer, Manager            // 名簿(by の検証)
  case Return = 0..2                 // → ドメイン型
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager   // → fair action approve(c: Return)
    transition reject  Requested -> Rejected by Manager
    transition refund  Approved  -> Refunded by Manager
  }
  kpi refunded counts Return in Refunded      // → ゴースト+自動整合 invariant(流入遷移で+1。流出遷移があれば type エラー)
  policy PAY-2 "申請は必ず裁定される" responds {
    forall c: Return { stage(c) == Requested ~> not (stage(c) == Requested) }
  }                                           // invariant { 式 } 形も可
  goal AllSettled "全件完了しうる" { forall c: Return { ... } }   // → reachable
}
```

`stage(c)` は束縛 c の型からプロセスの状態 Map(`return_stage[c]`)に展開される。

### requirements(要件層)

```fsl
requirements ReturnSystemReq {
  implements ReturnHandling from "return_policy.fsl" {   // 上位層への写像(省略可)
    map return_stage[c: CaseId] = if sys[c].st == New then Requested else ...
    map refunded = paid_count
  }
  // 型・state・init はカーネル構文そのまま
  requirement REQ-1 "閾値以下は自動承認" {
    fair action submit(c: CaseId, a: Amount) {
      requires sys[c].st == New
      requires a > 0
      branches {                                          // データ依存の分岐対応・自動分割
        when a <= AUTO_LIMIT { sys[c] = ... } maps approve(c)
        when a > AUTO_LIMIT  { sys[c] = ... } maps stutter
      }
    }
  }
  requirement REQ-3 "支払いは承認後のみ" {
    fair action pay(c: CaseId) maps refund(c) { ... }     // maps = 上位層 action 対応
    invariant PaidLedger { ... }
  }
  acceptance AC-1 "小額は支払われる" { submit(0, 1)  pay(0)  expect sys[0].st == Paid }
}
```

- `implements` があると `fslc verify` が上位層 refine を同時実行 →
  結果 JSON に `implements: {abs, result}`。
- `acceptance` は check 時に具象 Monitor で再生検証(失敗は `kind: "acceptance"`)。
  scenarios に `acceptance_<ID>` として出力され testgen に流れる。
- branches の分割アクションの表示は `submit[a <= AUTO_LIMIT]`、**下流の
  refinement 写像から参照するときは内部名 `submit__b1`/`submit__b2`**(現状の制約)。
- requirement 内の要素には自動で {id, text} メタが付く。

### 層の線引き

NFR の過半は扱える(§11)。FSL の外に残るのは確率・パーセンタイル・
実時間(壁時計 ms)・ユーザビリティ・散文の根拠(各層の文書に書く)。

## 11. 非機能要件(NFR)

| NFR | 書き方 |
|---|---|
| 権限 | requires でロール検査 + ゴースト invariant |
| 監査完全性 | 横断 invariant(bank_system パターン) |
| 容量 | 有界型・Seq 容量・count invariant |
| 信頼性の挙動 | 故障注入 action + モード状態 + fair recover + 復旧 leadsTo |
| SLA/タイムアウト | requirements の `time { urgent ...  age m[x: T] while P }` + `deadline m <= K` |
| 確率・%・実時間 | 対象外(文書へ) |

SLA の要点: tick は自動生成。`urgent` に「先延ばしされないアクション」を列挙
しないと飢餓トレースで violated になる(それが正しい診断)。BMC は即動く。
帰納証明には `age + 残り作業 <= K` 型の時間予算補助 invariant を CTI から
導出する(実例: examples/nfr/)。
