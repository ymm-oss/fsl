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
  struct <Name> { <field>: <スカラ型>, ... }

  state { <var>: <型>, ... }
  init  { <文>... }

  action <name>(<p>: <型名>, ...) {
    requires <式>                       // ガード。複数可(連言)
    let <x> = <式>                      // ローカル束縛
    <文>...                             // 代入 / if-else / forall
    ensures <式>                        // 事後条件。old(式) で旧状態を参照
  }

  invariant <Name> { <式> }             // 全到達状態で成立(安全性)
  reachable <Name> { <式> }             // 到達可能であること(witness が返る)
}
```

## 2. 型

| 型 | 例 | 説明 |
|---|---|---|
| `Int` / `Bool` | `count: Int` | 無界整数 / 真偽値 |
| ドメイン型 | `type Qty = 0..5` | 有界整数。**範囲は自動検査される**(§6) |
| enum | `enum St { Open, Closed }` | メンバは式中で裸の名前で参照 |
| struct | `struct Order { st: St, qty: Qty }` | フィールドは**スカラのみ**(下記) |
| `Option<T>` | `cart: Option<ItemId>` | `none` / `some(e)`。番兵値の代わりに使う |
| `Map<K, V>` | `stock: Map<ItemId, Qty>` | K は有界スカラ(ドメイン型/enum/Bool)推奨 |
| `Set<T>` | `shipped: Set<OrderId>` | T は有界スカラ |
| `Seq<T, N>` | `queue: Seq<JobId, 3>` | 容量 N の列(FIFO)。T はスカラ、N は定数 |

**スカラ** = Int / Bool / ドメイン型 / enum。

**状態変数として合法な型**(これ以外は `check` が型エラーで拒否):
スカラ | `Option<スカラ>` | struct | `Map<有界スカラ, スカラ | Option | struct>`
| `Set<有界スカラ>` | `Seq<スカラ, N>`

- struct のネスト、struct フィールドへの Option/Set/Map/Seq、Map の値への
  Set/Map/Seq は **v1 では不可**(check 時に hint 付きで拒否される)。
  optional なフィールドは enum 状態か別 Map で表現する。
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

`violation_kind`: `invariant` | `ensures` | `type_bound` | `partial_op` | `deadlock`。

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

reachable / invariant は状態のみを見るため、「X の後に Y」は履歴を状態に
落とす。(時相演算子 `leadsTo` は v2.0 で導入予定)

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

## 10. ライブラリ API

```python
from fslc import parse, build_spec, verify, prove

spec   = build_spec(parse(src))
result = verify(spec, depth=8)            # BMC
result = prove(spec, k_ind=1, base_depth=8)   # k帰納法
```

CLI と同じ構造の dict を返す(CLI はこれに `"fsl": "1.0"` 封筒を付ける)。
