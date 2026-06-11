# ドッグフーディング第4回 — 3層方言の貫通 (2026-06-11)

カーネル+3方言(DESIGN-layers.md / DESIGN-dialects.md)の実装後、
返品ドメインを **business → requirements → 設計 fsl → 写像** の4ファイルで
組み上げ、全段を検証した(`examples/layers/`)。

## 結果

| 段 | 結果 |
|---|---|
| business(プロセス3遷移+KPI+policy2本+goal) | proved(自動 `_kpi_refunded` 整合 invariant が帰納前提に入る) |
| requirements(branches・acceptance・implements) | verified + implements: **refines**(verify 1コマンドで上位層検査込み)+ proved |
| 設計層(支払い2段階化+通知キュー) | proved + 要件層へ **refines** |
| 要件を壊した変種 | 反例に `requirement: {REQ-3, 原文}` と `implements: violated` が**同時に**載る |
| acceptance AC-1 | check 時に Monitor 再生で検証され、scenarios に流れる |

## 発見

- **BUG18(修正済み)**: キーワード接頭辞の識別子(`notify` → `not`+`ify`)が
  誤字句化。層スパイク中に発見、段階2で修正。
- **F11: branches 分割アクションの下流参照は内部名。** 設計層の写像から
  要件層の分割アクションを `submit__b1` で参照する必要がある(表示名
  `submit[a <= AUTO_LIMIT]` では書けない)。動くが UX として不細工 —
  表示名/元名+when 条件での参照を将来課題に積む。
- **F12: 層横断診断が1つの JSON に揃う。** requirement(原文付き)と
  implements(上位層への波及)が同一反例に載る — 「どの要件が壊れ、
  業務上の何に違反するか」をエージェントが1往復で読める。設計の狙い
  (透過連携)が診断面で成立した。
- 方言の展開器は compose のパターン3例目でも安定(BMC/帰納法/scenarios/
  Monitor/refine すべて無修正で方言仕様に効いた)。カーネル無変更の
  大原則は4段階を通して守られた。
