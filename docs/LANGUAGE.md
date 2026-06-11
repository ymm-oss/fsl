# FSL 言語リファレンス (v1.1)

FSL は、**生成AIが書き・検証し・修正する**ことを第一の設計目標とした、
アプリケーション開発向けの形式仕様言語です。本書は仕様を書くときに参照する
言語リファレンスです。設計判断の背景は [`DESIGN-v1.md`](DESIGN-v1.md)、
各機能の実装設計は `DESIGN-induction.md` / `DESIGN-scenarios.md` /
`DESIGN-seq.md` を参照。

## 設計原則

| 原則 | 既存言語 (TLA+/Alloy) | FSL |
|---|---|---|
| 構文 | 数学記法 (∀, □, ◇) | TypeScript/Python 風。LLMの学習分布に寄せる |
| 反例 | 人間向けテキスト | **構造化JSON**(状態差分・違反した束縛変数つき) |
| エラー | 人間向けメッセージ | 機械可読(行・列・分類・修復ヒント) |
| 検証 | フル検証が前提 | 有界・高速が既定。**k 帰納法で無限深度証明**も可能 |
| 空虚性 | 専門家の勘で発見 | アクション到達可能性 + **阻害 requires の unsat core 診断** |
| 落とし穴 | 規律で回避 | **構造的に排除**(自動境界チェック、部分操作の暗黙検査) |

## 1. 仕様の構造

```fsl
spec <Name> {
  const <NAME> = <定数式>
  type  <Name> = <lo>..<hi>            // ドメイン型(有界整数)
  enum  <Name> { <Member>, ... }
  struct <Name> { <field>: <スカラ型 | Option<スカラ型>>, ... }

  state { <var>: <型>, ... }
  init  { <文>... }

  [fair] action <name>(<p>: <型名>, ...) {
    requires <式>                       // ガード。複数可(連言)
    let <x> = <式>                      // ローカル束縛
    <文>...                             // 代入 / if-else / forall
    ensures <式>                        // 事後条件。old(式) で旧状態を参照
  }

  invariant <Name> { <式> }             // 全到達状態で成立(安全性)
  reachable <Name> { <式> }             // 到達可能であること(witness が返る)
  leadsTo <Name> { <応答性質> }         // 有界応答性質(§1 参照)
}
```

`fair` は弱公平性注釈: そのアクションインスタンスが連続して enabled
であり続けるなら、いつかは実行される前提。

`leadsTo` ブロック内の応答性質:

```fsl
leadsTo <Name> {
  <式> ~> <式>                          // P が成立したら(同時点含む)いつか Q
  forall x: T { <式> ~> <式> }          // 束縛ごとに独立検査(外側 forall のみネスト可)
}
```

`~>` は **leadsTo ブロック専用** — 一般式では使えない。

## 2. 型

| 型 | 例 | 説明 |
|---|---|---|
| `Int` / `Bool` | `count: Int` | 無界整数 / 真偽値 |
| ドメイン型 | `type Qty = 0..5` | 有界整数。**範囲は自動検査される**(§6) |
| enum | `enum St { Open, Closed }` | メンバは式中で裸の名前で参照 |
| struct | `struct Order { st: St, item: Option<ItemId>, qty: Qty }` | フィールドはスカラまたは `Option<スカラ>` |
| `Option<T>` | `cart: Option<ItemId>` | `none` / `some(e)`。番兵値の代わりに使う |
| `Map<K, V>` | `stock: Map<ItemId, Qty>` | K は有界スカラ(ドメイン型/enum/Bool)推奨 |
| `Set<T>` | `shipped: Set<OrderId>` | T は有界スカラ |
| `Seq<T, N>` | `queue: Seq<JobId, 3>` | 容量 N の列(FIFO)。T はスカラ、N は定数 |

**スカラ** = Int / Bool / ドメイン型 / enum。

**状態変数として合法な型**(これ以外は `check` が型エラーで拒否):
スカラ | `Option<スカラ>` | struct(スカラ / `Option<スカラ>` フィールド)
| `Map<有界スカラ, スカラ | Option<スカラ> | struct>`
| `Set<有界スカラ>` | `Seq<スカラ, N>`

- struct のネスト、struct フィールドへの Set/Map/Seq、
  `Option<Option<...>>` や `Option<Set/Map/Seq/struct>` は不可
  (check 時に hint 付きで拒否される)。optional なスカラフィールドは
  v2.1 から struct 内へ直接書ける。
- `Map<Int, V>` は動くが非推奨警告が出る。ドメイン型キーを使う。

## 3. 式

- 算術: `+ - *`、単項 `-`、`min(a, b)` / `max(a, b)` / `abs(a)`
- 比較: `== != < <= > >=`
- 論理: `and or not =>`
- 量化(有界): `forall x: T { 式 }` / `exists x: T { 式 }`(`where 式` でフィルタ可)、
  v0 形 `forall i in lo..hi: 式` も可
- 集約: `count(x: T where 式)`、`sum(x: T of 式 [where 式])`
- Option: `x == none` / `x != none` / `x is some(v)`(v はその式の中で束縛される)。
  **`x == some(e)` は型エラー** — `x is some(v)` で取り出して比較する
- struct: リテラル `Order { st: Open, qty: 0 }`、フィールド参照 `o.st`、
  `==` はフィールドごとの等価
- Set: `Set {}` / `Set { 1, 2 }`、`.add(e)` `.remove(e)` `.contains(e)` `.size()`
- Seq: `Seq {}` / `Seq { 1, 2 }`、`.push(e)` `.pop()` `.head()` `.at(i)`
  `.contains(e)` `.size()`、`==` は長さと全要素の等価
- ensures 内のみ: `old(式)` で遷移前の状態を読む
- leadsTo ブロック内のみ: `P ~> Q`(応答性質。一般式の演算子階層には含まれない)

## 4. 文(init / action 本体)

- 代入: `x = 式`、`m[k] = 式`、`m[k].field = 式`、`o.field = 式`
- Set/Seq の更新は**再代入イディオム**: `s = s.add(x)`、`q = q.pop()`
- `if 式 { 文... } else { 文... }`(else 内の if でネスト可)
- `forall x: T { 文... }`(一括初期化・一括更新)

## 5. 意味論

- **遷移系**: 1ステップ = いずれか1つのアクションインスタンス
  (アクション名 × パラメータ値)が原子的に実行される。
- **同時代入**: アクション本体の右辺はすべて**旧状態**を読む。
  代入されなかった変数は変化しない(フレーム条件は自動)。
- **二重代入は静的エラー**: 同一実行パス上で同じ変数(またはフィールド)に
  2回代入すると semantics エラー。if の then/else は別パスなので両方で
  代入してよい。if の**後**に同じ変数へ代入するのもエラー
  (分岐内の書き込みが消えるのを防ぐ)。
- **requires**: すべて成立するときのみ enabled。
- **ensures**: 遷移後の状態で検査。違反は `violation_kind: "ensures"`。

## 6. 自動チェック(書かなくても検査されるもの)

| チェック | 内容 | 違反時 |
|---|---|---|
| 型境界 | 有界型の全状態変数(Map値・structフィールド・Seq要素含む)が範囲内 | `violated` / `type_bound` / `_bounds_<var>` |
| 部分操作 | `pop()`/`head()`/`at(i)` 実行時に列が空でない・添字が範囲内 | `violated` / `partial_op` / `_partial_<action>` |
| action coverage | 各アクションが深さ K 以内に一度は enabled | `action_coverage` に阻害 requires の診断 |
| デッドロック | 全アクションが disabled になる状態への到達 | warning(`--deadlock error` で violated) |
| leadsTo | 深さ K までのラッソ / デッドロック停滞で P ~> Q 違反 | `violated` / `leadsTo` / `bindings` + trace |

- 「在庫は 0 以上」のような invariant は**書かない** — `type Qty = 0..N` にすれば
  自動検出される。
- Seq への満杯 `push` も `type_bound` として自動検出される
  (ガードするなら `requires q.size() < N`)。

## 7. 検証器 `fslc`

```
fslc check     <file.fsl>                        # 構文・名前・型のみ(高速)
fslc verify    <file.fsl> [--depth K]            # BMC(既定 K=8、反例は最短)
               [--engine induction] [--k N]      # k帰納法: 無限深度証明
               [--deadlock warn|error|ignore]
fslc scenarios <file.fsl> [--depth K]            # 統合テスト雛形JSONを生成
```

終了コード: `0` = verified / proved / scenarios生成、
`1` = violated / reachable_failed / unknown_cti、`2` = 仕様エラー、`3` = 内部エラー。

### 結果の種類

| result | 意味 | 次の一手 |
|---|---|---|
| `verified` | 深さ K まで違反なし(+ 全 reachable 充足) | 確証を上げるなら `--engine induction` |
| `proved` | **全実行で invariant 成立**(無限深度) | 完了 |
| `violated` | 反例あり。`violation_kind` と最短トレース付き | トレースを読んで仕様を修正 |
| `reachable_failed` | reachable が深さ K で未達 | `action_coverage` の診断を見る / `--depth` を上げる |
| `unknown_cti` | invariant は破られていないが帰納的でない | **CTI を読んで補助 invariant を足す**(§8) |
| `error` | parse / type / semantics / io | `loc` / `expected` / `hint` に従って修正 |

`violation_kind`: `invariant` | `ensures` | `type_bound` | `partial_op` | `deadlock` | `leadsTo`。

`verified` / `proved` で leadsTo を宣言している場合、
`leads_to: { "<Name>": { "checked_to_depth": K } }` が付く
(反例なしは深さ K までの有界保証。invariant の `verified` と同じ位置づけ)。

### coverage 診断(enabled にならないアクション)

```json
"action_coverage": {
  "checkout": {
    "covered": false,
    "blocking_requires": [ {"loc": {"line": 27}, "text": "requires stock[i] > 0"} ],
    "hint": "these requires clauses are unsatisfiable at every step up to depth 8; ..."
  }
}
```

阻んでいる requires 句が unsat core で特定される。弱める/確立するアクションを
足す/深さを上げる、のいずれかが次の一手。

## 8. 推奨ワークフロー: proved を標準とする

1. 仕様を書く → `fslc check`(速い構文・型ループ)
2. `fslc verify --depth 8` → violated ならトレースで修正。
   reachable で意図したシナリオが witness されることを確認する
3. `fslc verify --engine induction` → `proved` なら完了
4. `unknown_cti` なら CTI(k+1 状態のトレース)を読む。CTI の開始状態は
   「全 invariant を満たすが実際には到達不能」な**幽霊状態**。
   それを排除する**補助 invariant**(それ自体がドメインの真実であるもの)を
   足して 3 に戻る

実績として補助 invariant は1ラウンドで収束することが多い
(`DOGFOOD-1.md` / `DOGFOOD-2.md` の実例: 「attempts == 3 ならロック済み」
「返金があるのは Captured のみ」「キューに重複なし」)。

## 9. イディオム集

### 番兵値ではなく Option

```fsl
cart: Map<UserId, Option<ItemId>>      // -1 のような番兵を使わない
struct Reservation { item: Option<ItemId>, qty: Qty }  // optional field も直接書ける
action checkout(u: UserId) {
  requires cart[u] is some(i)          // i がここで束縛される
  requires stock[i] > 0
  stock[i] = stock[i] - 1
  cart[u] = none
}
```

### 手書き境界 invariant ではなくドメイン型

```fsl
type Qty = 0..5
state { stock: Map<ItemId, Qty> }      // NoNegativeStock は書かない(自動)
```

### 部分操作のガード(requires 形 / if 形のどちらでも)

```fsl
action take()  { requires q.size() > 0  x = q.head()  q = q.pop() }
action drain() { if q.size() > 0 { x = q.head()  q = q.pop() } }
```

ガードを忘れると `partial_op` 違反として検出される(黙って壊れない)。

### Seq を invariant で語る: 添字ガード付き forall

```fsl
invariant QueuedAreQueued {
  forall i in 0..2 {                   // 0..容量-1
    i < queue.size() => jobs[queue.at(i)].st == Queued
  }
}
```

`at()` は性質文脈では全域(範囲外は不定値)なので、必ず `i < q.size()` で
ガードする。

### Seq の集約: インデックス・ドメイン型イディオム

```fsl
type Idx = 0..3                        // 容量-1 まで覆うドメイン型
invariant BalanceMatchesLog {
  balance == sum(i: Idx of log.at(i) where i < log.size())
}
```

`sum`/`count` はドメイン型を走るが、`where i < size` で live prefix に
制限すれば **Seq の畳み込み**になる。

### 履歴(過去)を語るにはゴースト変数

```fsl
state { ever_locked: Map<UserId, Bool> }   // 「一度でもロックされた」
// ロックする分岐で ever_locked[u] = true
reachable RecoveredAfterLock {
  exists u: UserId { ever_locked[u] and session[u] }
}
```

reachable / invariant は状態のみを見るため、「X の後に Y」を**状態の事実**
として語るには履歴を状態に落とす(ゴースト変数)。

### 履歴ゴースト変数 vs leadsTo の使い分け

| 書きたいこと | 手段 |
|---|---|
| 「一度でも X だった」(状態の事実) | ゴースト変数 + invariant / reachable |
| 「X になったらいつか Y」(応答性質) | `leadsTo` + 必要なら `fair action` |

例: FIFO mutex で「待ち行列に入ったプロセスはいつかロックを得る」は
`leadsTo WaiterGetsLock { forall p: ProcId { waiters.contains(p) ~> ... } }`。
進行が `release_handoff` など特定アクションに依存するなら `fair` を付ける
(`specs/mutex_queue.fsl` 参照)。

### CTI からの補助 invariant(帰納の強化)

`unknown_cti` の CTI 開始状態を見て「現実には起きない組合せ」を invariant 化する:

```fsl
// CTI: queue = [0, 0, 0](同一ジョブが3重) → 重複なしを明文化
invariant NoDupQueue {
  forall i in 0..2 { forall j in 0..2 {
    (i < j and j < queue.size()) => not (queue.at(i) == queue.at(j))
  } }
}
```

## 10. Refinement(詳細仕様の忠実性)

抽象仕様(abs)を先に `verify` / `prove` したあと、実装に近い詳細仕様(impl)が
abs の振る舞いから外れないことを **`fslc refine`** で検査する
(`DESIGN-refinement.md` 参照)。

マッピングは **独立ファイル** に書く(impl/abs の `.fsl` は汚さない):

```fsl
refinement CartImplRefinesCart {
  impl CartImpl
  abs  ShoppingCart

  map stock[i: ItemId] = impl_stock[i] - reserved[i]
  map cart[u: UserId]  = impl_cart[u]

  action add_to_cart(u, i)   -> add_to_cart(u, i)
  action impl_checkout(u)    -> checkout(u)
  action reserve(i)          -> stutter
}
```

- `map <abs変数> = <impl式>` — スカラ抽象変数。
- `map <abs変数>[<binder>] = <式>` — Map の要素ごと写像(キー型を有界列挙)。
- `action <impl>(<仮引数>) -> <abs>(<式>) | stutter` — 全 impl アクション必須。
  `stutter` は抽象状態が変わらない内部ステップ。

```bash
fslc refine specs/cart_impl.fsl specs/cart_v1.fsl specs/cart_refines.fsl --depth 8
```

成功: `result: "refines"`(exit 0)。違反: `refinement_failed`(exit 1) と
`kind`(`abs_requires_failed` / `abs_state_mismatch` / `stutter_changed_abs` /
`map_out_of_bounds`)、`impl_trace`、写像後の `abs_before` / `abs_after_*`。
静的エラー(map 漏れ・未知アクション等)は `kind: "type"`(exit 2)。

推奨ワークフロー: **abs を人間/LLM がレビュー → impl を LLM が詳細化 →
`refine` が忠実性を担保**。abs の `ensures` / invariant は refine では再検査せず、
abs 側で別途検証済みであることを前提とする。

## 11. 合成 (compose)

複数の検証済みコンポーネント spec を **名前空間付きでマージ**し、1 つの
システム仕様にする。展開後は通常の単一 spec になるため、`verify` / `prove` /
`scenarios` / `Monitor` / `replay` / `testgen` / `refine` はそのまま使える
(設計: `DESIGN-compose.md`)。

```fsl
compose OrderSystem {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  state { orders_linked: Int }
  init  { orders_linked = 0 }

  // 同期アクション: 複数コンポーネントのアクションを同一ステップで実行
  action checkout_and_pay(u: cart.UserId, p: pay.PayId) =
      cart.checkout(u) || pay.capture(p) {
    requires pay.payments[p].st == Authorized
    orders_linked = orders_linked + 1
  }

  // 単独実行から除外(同期経由でのみ発火)
  internal cart.checkout
  internal pay.capture

  invariant LinkedNonNeg { orders_linked >= 0 }
  reachable PaidOrder {
    exists p: pay.PayId { pay.payments[p].st == Captured }
  }
}
```

- `use <SpecName> as <alias> from "<相対パス>"` — パスは compose ファイル基準。
  spec 名はファイル内名と一致必須。alias は compose 内で一意。ネスト compose は不可。
- コンポーネントの型・状態・アクションは `alias.Name` で参照する。
- **同期アクション** `action <name>(...) = <a>.<act>(...) || <b>.<act2>(...) { ... }`:
  各コンポーネントアクションの requires / 本体 / ensures をマージし、追加文は
  合成側状態への代入のみ(同一コンポーネントの 2 アクション同期は不可)。
- `internal <alias>.<action>` — そのアクションをインターリービングから除外。
- 通常の `action`( `=` なし)も書ける(グルーアクション)。
- JSON 表示: 物理名 `alias__x` は `alias.x` として出力される(状態キー・アクション名、
  invariant / reachable 名、トレース、scenarios、Monitor すべて)。

```bash
fslc check  specs/order_system.fsl
fslc verify specs/order_system.fsl --depth 8
fslc scenarios specs/order_system.fsl
```

## 12. 実装への橋

仕様を証明したあと、実装と結線するための3つの入口がある
(`DESIGN-bridge.md` 参照)。

| 手段 | 用途 |
|---|---|
| `fslc.runtime.Monitor` | 仕様の具象インタプリタ(Z3 不要)。実装に組み込んで実行時検査 |
| `fslc replay` | 実システムのイベントログ JSON を仕様に対して検査 |
| `fslc testgen` | pytest 適合性テスト雛形を生成(Adapter に実装を結線) |

推奨ワークフロー: **spec を `verify` / `prove` → `testgen` で雛形生成 →
`Adapter` に実装を結線 → pytest**。`Monitor` は oracle としてランダムウォーク
テストに使われる。

```python
from fslc import Monitor

mon = Monitor("specs/cart_v1.fsl")
mon.reset()
r = mon.step("add_to_cart", {"u": 0, "i": 0})   # ok / kind / state / changes
```

```bash
fslc replay specs/cart_v1.fsl --trace events.json   # conformant / nonconformant
fslc testgen specs/cart_v1.fsl -o test_cart_v1.py   # Adapter 未実装なら全 skip
```

`replay` は有限ログのみを検査するため **`leadsTo` は対象外**(出力 `note` に明記)。
`Monitor` は init が決定的である必要がある(forall 一括代入可)。

## 13. ライブラリ API

```python
from fslc import parse, build_spec, verify, prove, Monitor

spec   = build_spec(parse(src))
result = verify(spec, depth=8)            # BMC
result = prove(spec, k_ind=1, base_depth=8)   # k帰納法
```

CLI と同じ構造の dict を返す(CLI はこれに `"fsl": "1.0"` 封筒を付ける)。
