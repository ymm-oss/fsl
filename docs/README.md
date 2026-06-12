# docs/ 見取り図

## まず読むもの

| 文書 | 内容 |
|---|---|
| [`INTRO-formal-methods-and-fsl.md`](INTRO-formal-methods-and-fsl.md) | **形式手法と FSL の照会文書**。非専門家向けの背景説明、AI 駆動開発での位置づけ、導入 PoC 観点 |
| [`LANGUAGE.md`](LANGUAGE.md) | **言語リファレンス**(全構文・意味論・CLI・イディオム・3層方言・NFR)。仕様を書くならこれ |
| [`DESIGN-v1.md`](DESIGN-v1.md) | 言語設計書(設計原理 G1-G5・型システムの設計判断・修復プロトコル・ロードマップ) |

## アーキテクチャ・機能別の実装設計(DESIGN-*)

| 文書 | 対象 |
|---|---|
| [`DESIGN-layers.md`](DESIGN-layers.md) | **共通カーネル+3方言**(コンサル/要件/設計)の全体構想と実証 |
| [`DESIGN-dialects.md`](DESIGN-dialects.md) | 方言の実装仕様(宣言タグ・fsl-req・fsl-biz) |
| [`DESIGN-nfr.md`](DESIGN-nfr.md) | 非機能要件(対応表・離散時刻 SLA: time/urgent/age/deadline) |
| [`DESIGN-induction.md`](DESIGN-induction.md) | k 帰納法エンジン(proved / unknown_cti / CTI) |
| [`DESIGN-temporal.md`](DESIGN-temporal.md) | leadsTo・弱公平性(ラッソ反例)・respond シナリオ |
| [`DESIGN-refinement.md`](DESIGN-refinement.md) | refinement 検査(写像ファイル・条件式) |
| [`DESIGN-compose.md`](DESIGN-compose.md) | spec 合成(名前空間・同期アクション・internal) |
| [`DESIGN-bridge.md`](DESIGN-bridge.md) | 実装橋(runtime Monitor / replay / testgen) |
| [`DESIGN-scenarios.md`](DESIGN-scenarios.md) | scenarios・coverage の unsat core 診断 |
| [`DESIGN-seq.md`](DESIGN-seq.md) | Seq<T,N>(partial_op・型ホワイトリスト) |
| [`DESIGN-option-struct.md`](DESIGN-option-struct.md) | struct の Option フィールド |

## ドッグフーディング記録(DOGFOOD-*)

各機能を実戦投入した所見とバグ・発見の記録。設計判断の根拠になっている。

1. [`DOGFOOD-1.md`](DOGFOOD-1.md) — v1.0 実地評価(BUG11-14・PERF1 発見)
2. [`DOGFOOD-2.md`](DOGFOOD-2.md) — proved 標準運用・Seq(集約イディオム発見)
3. [`DOGFOOD-3.md`](DOGFOOD-3.md) — フルワークフロー(抽象→refine→compose→実装)
4. [`DOGFOOD-4.md`](DOGFOOD-4.md) — 3層方言の貫通(要件 ID の層横断診断)
5. [`DOGFOOD-5.md`](DOGFOOD-5.md) — NFR / 離散時刻 SLA

実例は [`../specs/`](../specs/)(単体仕様)と [`../examples/`](../examples/)
(bank: 実装適合 / layers: 3層チェーン / nfr: SLA)に。
AI エージェント向けスキルは [`../skills/fsl/`](../skills/fsl/)。
