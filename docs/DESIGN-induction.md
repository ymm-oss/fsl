# FSL k 帰納法エンジン — 実装設計(v1.1、DESIGN-v1.md §9 の詳細化)

本書は `--engine induction` の実装可能レベルの仕様。§9 のプロトコル(`proved` /
`unknown_cti` / JSON 形)は確定済みであり、ここでは意味論・アルゴリズム・
既存コードへの組み込み・エッジケースを規定する。

## 1. 目的と非目的

- **目的**: invariant(ユーザー定義+自動 `_bounds_*`)の**無限深度証明**。
  成功時 `result: "proved"`。BMC の「深さ K まで違反なし」を「全到達状態で成立」に格上げする。
- **非目的(v1.1 では扱わない)**:
  - `reachable` の証明(`reachable` は有界 witness 探索のままでよい。
    induction 実行時も BMC と同じ方法で witness を探す — 深さは `--depth` を流用)
  - `ensures` の帰納的証明(ensures は1遷移の性質なので帰納不要 — §5 参照)
  - IC3/PDR(CTI からの自動補強はやらない。CTI を LLM に返すのが v1.1 の賭け)

## 2. アルゴリズム

入力: spec、最大帰納深さ `K_ind`(CLI `--k`、既定 1、上限 4 程度)、
BMC 深さ `K_bmc`(`--depth`、base case と reachable witness に使用)。

Inv(s) := 全 invariant(ユーザー + `_bounds_*`)の連言。
T(s, s') := 既存 `transition()` と同一の遷移関係(choice 変数込み)。
Init(s) := 既存 `init_constraints()`。

### 2.1 Base case(基底)

既存 BMC をそのまま深さ `K_bmc` で実行する(コード再利用)。violated なら
通常の violated JSON を返して終了(反例は実トレースであり、ここで返すのが最善)。

注: 教科書的 k-induction の base は「深さ k-1 まで」だが、FSL では
**base = 通常 BMC(深さ K_bmc ≥ k)** とする。base を深めに走らせるほど
偽 CTI(実は到達可能な違反)を実トレース付き violated として先に検出でき、
LLM への応答品質が上がる。

### 2.2 Step case(帰納段)

k = 1, 2, ..., K_ind の順に試す。各 k について、**invariant ごとに**判定する
(連言まるごとではなく個別に。理由: どの invariant が帰納的でないかを
特定して JSON で返すため):

```
変数: 自由状態列 σ_0 .. σ_k(init 制約は付けない)
制約:
  ∀ t ∈ [0, k-1]:  Inv(σ_t)            // 過去 k 状態で全 invariant 成立
  ∀ t ∈ [0, k-1]:  T(σ_t, σ_{t+1})     // 連続遷移
  ¬ inv_i(σ_k)                          // 対象 invariant が k 状態目で破れる
```

- **unsat** → inv_i は k-帰納的。次の invariant へ。
- **sat** → モデルから CTI を抽出(§3)。k < K_ind なら k+1 で再試行。
  k = K_ind でも sat なら `unknown_cti` を返す。

全 invariant が(それぞれ何らかの k ≤ K_ind で)unsat になれば `proved`。

重要: 個別判定の前提 Inv(σ_t) は**全 invariant の連言**を仮定してよい
(相互帰納。標準的かつ健全 — 全部の同時帰納より強い前提で各々を示す)。

### 2.3 健全性メモ(実装者向け)

- 前提に Init を**入れない**こと(入れると BMC と同じになり証明にならない)。
- `_bounds_*` も Inv に含める。bounded 型の変数は step case では自由変数に
  なるため、bounds を仮定しないと「範囲外の幽霊状態」由来の偽 CTI が大量に出る。
  (bounds の **check** は base case が担っており、step の前提に入れても
  隠蔽は起きない — base で全到達状態の bounds 違反は検出済みのため。)
- enum / Option の物理エンコーディング制約(例: enum 値 ∈ [0, n-1]、
  `present == false` のとき value は don't care)のうち、型として常に成り立つ
  べきものは step の前提に追加する。さもないと物理エンコーディング上
  ありえない CTI が出る。具体的には:
  - enum フィールド/変数 v: `0 <= v < len(members)` (これは `_bounds_*` が
    enum を含むなら不要。含まれていなければ明示追加)
  - Option: 追加制約不要(present/value とも任意の組合せが意味を持つ)
- deadlock 検査は induction では行わない(deadlock は到達可能性の性質)。
  `deadlock` フィールドは `--engine induction` の出力に含めない。
- action coverage も base case(BMC)側の結果をそのまま使う。

## 3. CTI(counterexample to induction)の抽出

step case が sat のとき、モデルから k+1 状態のトレースを構築する。
JSON(§9 で確定済みの形を多状態に一般化):

```json
{
  "fsl": "1.0",
  "result": "unknown_cti",
  "spec": "...",
  "invariant": "RevenueConsistent",
  "k": 2,
  "cti": {
    "states": [ {"step": 0, "state": {...}},
                {"step": 1, "state": {...}, "action": {...}, "changes": {...}},
                {"step": 2, "state": {...}, "action": {...}, "changes": {...}} ],
    "violated_at": 2
  },
  "hint": "this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run"
}
```

- `states` の表示は既存 `_build_trace` の論理値復元(`logical_state_values`)を
  そのまま使う(enum 名逆引き、Option null/値、struct dict、`__` 内部名なし)。
- §9 の `cti: {state, action, next_state}` 形(k=1 用)は、この一般形の
  別名とせず**一般形に統一**する(k=1 でも `states` 配列、長さ2)。
  DESIGN-v1.md §9 の JSON 例は本書の形に追従して更新すること。
- 終了コードは **2 でも 1 でもなく 0 でもなく**、新設はせず `1` を使う
  (「性質は未確立」のカテゴリ。修復ループは result 文字列で分岐するので
  終了コードの粒度は不要)。

## 4. CLI / JSON の変更点

```
fslc verify <file.fsl> --engine induction [--k N] [--depth K]
```

- `--engine bmc`(既定)は完全に従来動作。コードパスも共有部以外触らない。
- `--k N`: 最大帰納深さ K_ind。既定 1。
- `--depth K`: base case の BMC 深さ + reachable witness 探索深さ。既定 8。
- 成功時の出力:

```json
{
  "fsl": "1.0",
  "result": "proved",
  "spec": "...",
  "engine": "induction",
  "k_used": { "ShippedWasPaid": 1, "RevenueConsistent": 2, "_bounds_orders": 1 },
  "base_depth": 8,
  "invariants_checked": [...],
  "action_coverage": {...},        // base case の BMC から
  "reachables": {...},             // base case 側で witness 探索した結果
  "warnings": [...]
}
```

- `proved` の終了コードは 0。
- reachable が見つからない場合は従来どおり `reachable_failed`(exit 1)が
  proved より**優先**される(性質が全部成立して初めて 0)。
- 既存スキーマとの整合: `violated` 時の形は BMC と完全同一(base case が
  返すため自動的にそうなる)。

## 5. 既存コードへの組み込み(bmc.py)

新関数 `prove(spec, k_ind, base_depth, deadlock_mode)`:

1. `verify(spec, base_depth, ...)` を呼ぶ(= base case + reachables + coverage)。
   - `violated` / `reachable_failed` / `error` ならそのまま返す。
2. step case 用に状態列 σ_0..σ_k を `make_state(spec, t)`(名前衝突回避の
   ため `@ind{t}` などの suffix)で作り、共通ソルバーに
   Inv(σ_0..σ_{k-1}) と T を積む。
3. invariant ごとに push / ¬inv_i(σ_k) / check / pop。
4. 全部 unsat → verify の結果 dict を `result: "proved"` に組み替えて返す。
   どれか sat → CTI 抽出して `unknown_cti`。
5. k をインクリメントするとき、σ_{k+1} と Inv(σ_k)・T(σ_k, σ_{k+1}) を
   **追加するだけ**で再利用できる(ソルバーを作り直さない)。
   ただし「Inv(σ_k) を前提に追加」は k での ¬inv_i 検査と矛盾しないよう
   pop 後に行うこと。

実装ノート:
- `eval_expr(inv, σ_t, {}, spec)` は既存のまま使える(状態 dict を差し替える
  だけ)。`transition(spec, instances, σ_t, σ_{t+1}, ch_t)` も同様。
- choice 変数は step 用に別名(`__ind_choice@t`)。
- PERF1 の解決(展開共有)が前提。完了後のコードベースに乗せること。

## 6. テスト計画(回帰スイートに追加するもの)

1. **proved になる仕様**: `specs/cart_v1.fsl` は SoldOut witness があるので
   そのまま proved + reachables を確認(k=1 で全 invariant が帰納的のはず。
   もし CTI が出るなら、それ自体が「bounds を前提に入れ忘れ」等の実装バグの兆候)。
2. **counter ラッチ**(確実に k=1 で proved):
   `state { x: Int }  init { x = 0 }  action inc() { requires x < 5  x = x + 1 }
   invariant XRange { x >= 0 and x <= 5 }`
3. **unknown_cti になる仕様**(帰納的でない真の invariant):
   `state { x: Int, y: Int }  init { x = 0  y = 0 }
   action step() { requires x < 4  x = x + 1  y = y + 1 }
   invariant Sync { y <= 4 }` — Sync は真(y は x と同期して 4 で止まる)だが
   x との結び付き(補助 invariant `x == y`)なしでは帰納的でない。
   CTI が返り、`states` が JSON 直列化可能で、hint があること。
   さらに `invariant Aux { x == y }` を足すと **proved** に変わること
   (= LLM 補強ループの end-to-end 検証)。
4. **base で violated**: cart_v1_buggy が induction でも従来と同一の
   violated JSON(最短反例)を返すこと。
5. **CLI**: `--engine induction` の exit code(proved=0, unknown_cti=1)、
   `engine`/`k_used` フィールドの存在。
6. **k=2 が必要な仕様**(k_used に 2 が現れる例):
   `state { a: Bool, b: Bool }  init { a = false  b = false }
   action flip() { a = not a  b = a }` のような 1 手遅れ追従で
   `invariant Lag { b => a }` … 実装後に実例で調整してよい(コメント参照)。
   作りにくければ k=2 ケースは「Aux を抜いた Sync で k=2..4 を試して全部 sat」
  (= k 反復が回ること)の検証で代替可。

## 7. DESIGN-v1.md への反映

実装完了時に §9 を本書へのポインタ+確定 JSON 形(states 配列形)に更新し、
§7.1 の `--engine induction` から「v1.1」の注記を外す。
