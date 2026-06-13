# FSL — `fslc explain`(仕様の自然文化と反実仮想)実装設計

動機: issue #7(ロードマップ #1 の類型6)。PM・コンサルが AI の書いた論理式を直接
レビューするのは現実的でない一方、具体トレースの正誤判定は人間が得意。仕様→人間方向の
翻訳を機械が補助する機構がなかった。人間レビューを「論理式の読解」から「具体例の裁定」へ。

## 1. CLI / 出力

`fslc explain <f> [--depth K]` → `result:"explained"`、exit 0。**LLM 不使用**(決定的整形のみ。
散文化はエージェント側スキルに残す)。検証エンジン無改修、#6 mutate と verify を再利用。

```json
{"result":"explained",
 "skeleton":{"state":…, "actions":[{"name","actor?","requires_text","writes","ensures_text","requirement"}],
             "properties":[{"kind","name","body_text","requirement"}], "auto_checks":…},
 "counterfactuals":[{"invariant","weakening":{op,loc,target},"trace","requirement"}],
 "witnesses":[…]}
```

## 2. 3つの生成物

1. **骨格列挙** — 状態スキーマ・各 action の「誰が・いつ(requires)・何を変える」・各性質の
   「何を禁じる/保証する」を requirement タグと暗黙検査(型境界・partial_op)込みで出力。
2. **反実仮想の物語** — 各 user invariant を violated にする最小のモデル弱化の最短反例を
   「このルールが無ければこの手順で REQ-3 が破れる」として提示。`cart_v1_buggy.fsl` の
   手動デモの自動化。
3. **witness の物語化** — reachable / scenarios のトレースを表示名 + requirement 原文で整形。

## 3. 骨格は AST pretty-printer 無しで作る(重要)

**FSL に AST→文字列の整形器は無い**。`_requires_blocking_entry`(bmc.py)がやっているのは
`source_lines[line-1].strip()` ＝ **loc によるソース行の切り出し**で、AST のレンダリングでは
ない。よって:
- 状態スキーマ・action params・**「何を変えるか」は assign 文の左辺を構造的に走査**して算出。
- **requires / 性質本体のテキストは loc 切り出し**で原文を見せる(方言展開済み仕様では loc が
  業務層の原文を指すので "by Manager" 等が出るのはむしろ良い)。
- **compose**: component 由来要素の loc は未ロードのファイルを指すのでソース切り出し不可 →
  名前/構造にフォールバック(クラッシュさせない)。

## 4. 反実仮想は #6 mutate の上に薄く乗る(全オペレータを使う)

実質「#6 mutate の kill を user invariant 別に並べ替えて物語化したもの」。新しい検証
ロジックは書かない。重要な点:
- 反実仮想の探索は mutate の**全オペレータ(ガード除去 ＋ 代入除去 ＋ fair 除去)**から。
  **requires 除去だけでは足りない**: order_workflow の `ShippedWasPaid` を壊すのは
  `requires` 除去でなく **ship の状態代入 `orders[o].status = Shipped` の除去**
  (`shipped.add(o)` だけ残ると「集合に居るのに Shipped でない」で違反)。
- `killed_by` は invariant 以外(ensures=action 名、reachable 名、bounds)も含む → 反実仮想は
  **user invariant に限定**。reachable の kill は別種(「これが無いと X に到達できない」)。
- **「反実仮想なし」は正当かつ頻出**: order_workflow の `NonNegativeRevenue` は depth 内の
  どの弱化でも単独では破れない(他 invariant に含意される冗長か、より深い depth が要る)。
  graceful に明示しエラーにしない。#6 の `empty_formalization` と地続き。

## 5. 波及 / テスト

新規 `src/fslc/explain.py`(~120行、mutate/verify 再利用)+ cli サブコマンド。
表示名のみ(内部名漏出なし)・JSON 直列化可能。tests/test_explain.py: cart_v1 骨格 /
ShippedWasPaid の反実仮想が**代入除去**で出る / NonNegativeRevenue は「反実仮想なし」/
方言 cancel_flow に requirement 原文 / compose 非クラッシュ / exit 0。ロードマップ #1。
