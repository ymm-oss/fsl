# DOGFOOD-9: 妥当性確認ワークフローの実走 (2026-06-12)

issue #2(AI形式化の妥当性確認ロードマップ)で skills/fsl に追加した
**形式化メモ → NL→構文対応 → 仕様 → 正例ペア → 修復**のワークフローを、
新ドメイン(注文の支払い・キャンセル・返金フロー、在庫付き)で最初から実走した。
成果物は `examples/validation/order_refund.fsl`。

検証器(fslc)は v1.0.3 から無変更。本回が検証するのは**コードではなくワークフロー**
— 「内部整合は通るが意図からズレた仕様」を、新しい規律が書く前/書いた後に
捕まえられるか。

## 元の自然言語要件(PM想定インプット)

1. 注文は支払い後にキャンセルでき、キャンセルすると全額返金する
2. 出荷後はキャンセルできない
3. 返金は支払い済みの注文のみ。二重返金は禁止
4. 支払い時に在庫を1引き当て、キャンセル時に在庫を戻す
5. 返金は支払いから一定期間内のみ

## 形式化メモ(チャットに出した。ファイルにしない)

要件を トリガ/制約/例外/**境界の含意** に正規化した時点で、地雷が2つ見えた:

- **R2 の境界**: 「出荷後はキャンセル不可」の「後」は Shipped を**含む**。
  → `cancel requires order[o] == Paid`(Shipped を除外)。ASSUME-2 に明記。
- **R5 が未定義**: 「一定期間」の値・起点・境界(以内=含む?)がどれも原文にない。
  人間への質問に積んだうえで、「離散時刻SLAなら設計層ではなく requirements 層の
  time+deadline 案件では」という疑いをメモした。

仮定は ASSUME-1〜4 として控え、仕様に畳むことにした(後述)。

## 実走ログ

| 版 | 操作 | 結果 |
|---|---|---|
| v1 | R5 を `window_open: Map<OrderId,Bool>` フラグで素朴にモデル化 | `check` ok |
| v1 | `verify --depth 8` | **reachable_failed**。`FullyRefunded` 不到達、`action_coverage.refund = false` |
| v1 | refund の coverage 診断 | hint: 「これらの requires は depth 8 のどのステップでも充足不能。**それを成立させるアクションを追加せよ**」 |
| → 修復 | window フラグを除去、refund は `requires order[o] == Cancelled` のみ。R5 は ASSUME-5 で上位層に委譲 | |
| v2 | `verify --depth 8` | **verified**。coverage 全 true(refund も)。`FullyRefunded` を step 3 で witness: `pay(0) → cancel(0) → refund(0)` |
| v2 | `verify --engine induction` | **proved (k=1)**、CTI 0 ラウンド。`_bounds_stock` も `StockConserved` の下で帰納的 |

## 知見

- **F13: 正例ペア(P4)が「沈黙して verified」を可視化した(本回の主眼)。**
  v1 の安全性 invariant(StockConserved / RefundLedger)は**両方とも成立する** —
  返金経路が丸ごと死んでいても、安全性だけ見れば破れないからだ。`reachable
  FullyRefunded` を1本添えていたことで、`verify` が verified ではなく
  reachable_failed を返し、coverage が `refund` を名指しした。invariant だけの
  仕様なら、この「返金機能が動かない」は CI もレビューも素通りしていた。
  P4 を推奨に留めた判断(重い手順は義務化しない)とは別に、**境界に関わる
  アクションには1本添える価値が高い**ことを実地で確認。

- **F14: 形式化メモの「境界の含意」欄が R5 を書く前に地雷指定した。**
  R5 の曖昧さ(期間の値・起点・含意)はメモ段階で人間質問に上がった。だが
  「とりあえず window フラグで」と素朴に設計層へ持ち込むと、開く手段を書き忘れ、
  返金が不能になった。**メモで疑い、正例ペアで実証、ASSUME で決着**という3点が
  噛み合った。曖昧な NFR を設計層の状態に安易に展開しない、という線引き
  (DESIGN-nfr の SLA は requirements 層)がワークフロー上でも再現した。

- **F15: 修復が仕様を弱めたが、ASSUME タグで「なぜ」が残った。**
  修復は refund のガードを1本削る(弱化)だった。骨抜きと正当な修復の区別は
  ASSUME-5(「期間検査は上位層に委ねる」+ 経緯)が引き受けている。修復ログを
  仮定台帳に追記する規律(SKILL.md 修復プロトコル)が、まさにこの弱化で効いた。

- **F16: 保存則 invariant を書くと自動境界が一発で帰納的になった。**
  `_bounds_stock`(stock ≤ CAP)は単独では非帰納的(stock=CAP かつ Paid 在りの
  幽霊状態が CTI になりうる)だが、ドメインの真実である `StockConserved`
  (stock + 保有数 == CAP)を書いた時点で k=1 proved。CTI ラウンドは 0。
  「補助 invariant はそれ自体ドメインの真実」(DOGFOOD-2)の再確認。

## ワークフロー評価

- **書く前(メモ)**: R2/R5 の境界の含意を、論理式に落とす前に日本語で人間確認
  できる形にした。論理式レビューを人間に課さない狙いは成立。
- **書いた後(正例ペア)**: 過小制約ではなく**過剰制約/死に経路**を捕まえた。
  これは verify(安全性)単独では原理的に見えない種類の誤りで、P4 の存在意義を
  実証する最短の例になった。
- **限界**: 本回は「R5 を設計層に持ち込む」という形式化者の判断ミスを、
  形式化者自身が書いた正例ペアが捕まえた。もし正例ペアの方も同じ誤解
  (返金は本来不要)で書いていたら捕まらない。独立チャネル(別エージェントが
  NL から正負トレースを書く = issue #3 forbidden / D4)が次の防御層になる。

## 残課題への接続

- v1 の「安全性は通るが経路が死ぬ」は、issue #4(空虚性検査)が verify 段で
  warning として出すべき類のもの(`always_true_requires` / 到達不能)。本回は
  正例ペアが代替したが、ペアを書き忘れた仕様では検出器側が要る。
- ASSUME タグの存在は issue #5(`--strict-tags`)が、その意味的な拘束力は
  issue #6(`fslc mutate`)が引き受ける守備範囲。

## 追補(2026-06-13): ASSUME-5 の機械検証 — 設計レビューの実走

fsl-design-review スキルの手続きで、本回の先送り判断そのものを検査した。
ASSUME-5 の前提は「期間制限は、凍結した設計契約を壊さずに後から足せる」だった:

| 検査 | 結果 |
|---|---|
| 窓付き変種(`order_refund_windowed.fsl`: age マップ + tick + refund への時間ガード) | 単体 **proved**。FullyRefunded@3(窓内返金は可能)+ WindowExpired@4(期限切れも実際に起きる) |
| 窓付き変種 ⊑ 契約(`fslc refine`、tick → stutter) | **refines** — 抽象契約を1行も編集せずに期間制限が入る。**ASSUME-5 は健全** |
| 負例プローブ「即時返金」(cancel を飛ばして Paid → Refunded) | 単体では **verified**(保存則・台帳とも無傷)だが refine は **abs_requires_failed**: 最短2手 `pay(0) → instant_refund(0)` が「refund は Cancelled のみ」を迂回 |

- **F17: 「単体 verify は通るが契約は破る」変種が refinement で最短反例化される。**
  本編の F13(正例ペア)が到達性の沈黙を破ったのと対で、refine は**設計逸脱**の
  沈黙を破る。妥当性確認の道具立てとして verify / reachable / refine の三層が
  それぞれ別種の「沈黙して verified」を担当する構図が揃った。
- 素朴な定式化 `type Age = 0..WINDOW` + `requires age[o] <= WINDOW` は型境界ゆえ
  **恒真の死にガード**になる(本変種は `< WINDOW` を採用)。issue #4 の
  `always_true_requires` が機械検出する種類の誤りであることを付記する。
- 成果物は `examples/validation/order_refund_{windowed,instant}*.fsl`。
