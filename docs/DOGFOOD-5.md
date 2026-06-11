# ドッグフーディング第5回 — 非機能要件(離散時刻 SLA)(2026-06-12)

「FSL で非機能要件を扱えるか」への回答実装(DESIGN-nfr.md)の検証記録。

## 結果

| 項目 | 結果 |
|---|---|
| カーネル手書き版(examples/nfr/sla_worker_kernel.fsl) | BMC verified + **induction proved**(補助 invariant 6本、CTI 4ラウンド) |
| 方言版(examples/nfr/sla_worker.fsl、`time`+`deadline`) | BMC verified(自動 tick が coverage に出る) |
| urgent を外した変種 | **violated** — `submit → tick×5` の飢餓トレース + `requirement: NFR-1(原文)` |
| 静的検査 | unused age / 未知 urgent / tick 名衝突 / time 重複 / 宣言なし deadline → type エラー |

## 知見

- **SLA は安全性として検査できる**: 「K tick 以内」= age カウンタの上限
  invariant。leadsTo(いつか)より強い「期限付き」が書ける。
- **urgency 規律が本質**: 「緊急アクションが enabled の間は時間が進まない」を
  tick のガードに織り込む。これを忘れた仕様には検証器が飢餓トレースを返す —
  「スケジューリング前提が書かれていない」ことの正しい機械検出であり、
  NFR レビューでそのまま指摘事項になる。
- **証明コストは未時間化仕様より高い**: 時間予算 invariant
  (`age[serving] + busy <= 4`、待機者の予算、サービス開始前 age=0)の階梯が
  必要で、CTI 4ラウンド(従来実績は1ラウンド)。既定運用は BMC 検査、
  証明はオプトインが正しい位置づけ。
- 扱える NFR の線引き(DESIGN-nfr §1)は実装後も変わらず:
  権限・監査・容量・信頼性挙動(今日から)/ SLA・タイムアウト(本機能)/
  確率・パーセンタイル・実時間 ms(対象外 — 文書へ)。
