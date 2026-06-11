# ドッグフーディング第2回 — 所見 (2026-06-11)

v1.1 全機能(Seq / k 帰納法 / unsat core 診断 / scenarios)を実戦投入し、
**「proved を標準とする運用」**(BMC verified で止めず、CTI → 補助 invariant で
無限深度証明まで持っていく)を評価した。仕様3本: `specs/mutex_queue.fsl`
(FIFO ミューテックス)、`specs/job_pipeline.fsl`(リトライ付きジョブパイプライン)、
`specs/audit_log.fsl`(追記専用監査ログ)。

## 結果サマリ

| 仕様 | BMC (depth 8) | induction | CTI ラウンド数 |
|---|---|---|---|
| mutex_queue | verified、coverage 全 true | **proved (k=1)** | 0(初版がそのまま帰納的) |
| job_pipeline | verified、coverage 全 true | **proved (k=1)** | 1(NoDupQueue 追加) |
| audit_log | verified | **proved (k=1)** | 0(厳密版 invariant でも) |

第1回の仕様も含め、**リポジトリの正しい仕様 10 本すべてが k=1 で proved**。

## proved 運用の評価

- **job_pipeline の CTI は一読で原因が分かった**: `queue = [0, 0, 0]`(同一ジョブの
  3重エントリ)という幽霊状態。pop が先頭の1個しか除かないため、残った重複の
  状態遷移で `QueuedAreQueued` が破れる。補助 invariant
  `NoDupQueue`(キュー重複なし)1本で proved に変わった。
  第1回の auth_lockout / payment と合わせ、**CTI → 補助 invariant ループは
  3/3 で1ラウンド収束**。CTI の表示品質(論理値・enum 名・changes)が
  この収束速度に直接効いている。
- 補助 invariant はすべて「それ自体がドメインの真実」(キュー重複なし、
  返金は Captured のみ、attempts=3 ならロック)であり、証明のための
  人工物にならなかった。仕様の質が上がる副作用がある。

## 新しい発見

### F5: インデックス・ドメイン型による Seq 集約イディオム(良い驚き)

設計時は「Seq に集約(sum)は書けない」と想定していたが、実際は:

```fsl
type Idx = 0..3   // 容量-1 までを覆うドメイン型
invariant BalanceMatchesLog {
  balance == sum(i: Idx of log.at(i) where i < log.size())
}
```

`at()` が性質文脈で全域(範囲外は don't care)+ `where` ガードの組合せで
**live prefix の畳み込みが書ける**。audit_log の厳密 invariant
(残高 = ログ合計)がこれで書け、しかも k=1 で proved になった。
LANGUAGE ドキュメントに標準イディオムとして載せるべき。

### F6: scenarios の最短トレースは前提条件の連鎖を正しく解く

`cover_finish_fail` が `submit → start → finish_retry → start → finish_fail`
を生成。finish_fail には `tries >= 1` が必要で、そのために retry を先に
経由する5手の最短列を正しく組んでいる。統合テスト雛形として実用品質。

### F7: 「ハンドオフが起きた」を状態だけで言えない(F1 の再確認)

mutex_queue の `HandoffHappened` は `holder == some(1)` で書いたが、
acquire_free(1) でも step 1 で成立してしまい「handoff の結果」を特定できない。
第1回 F1(過去を語る性質にはゴースト変数が必要)と同根。v2.0 `leadsTo` の
動機付け実例として追加。

## バグ

今回の3仕様+プローブでは**新規バグ 0 件**。
(Seq 実装ラウンドのレビューで BUG15(if ガード内 partial_op の誤検出)と
check 素通り2件(容量超過リテラル、`Map<K, Set<K>>`)を事前に検出・修正済み。
詳細は DESIGN-seq.md と commit d8e2ecf)

## 性能

3仕様とも depth 8 の BMC + induction が数秒以内。PERF1 修正後の
エンコーディングは Seq のシフト ite が入っても安定している。
