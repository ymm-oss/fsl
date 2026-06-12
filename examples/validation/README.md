# validation — 妥当性確認ワークフローの実走例

「内部整合(verify)は通るが、元の意図からズレた仕様」を、書く前・書いた後の
規律でどう捕まえるかの実走成果物。経緯と知見は [`../../docs/DOGFOOD-9.md`](../../docs/DOGFOOD-9.md)。

| ファイル | 内容 |
|---|---|
| [`order_refund.fsl`](order_refund.fsl) | 注文の支払い・キャンセル・返金フロー(在庫付き)の設計層 spec。proved |

## このサンプルが示すこと

- **形式化メモ**で要件の「境界の含意」を書く前に洗い出す(出荷"後"はキャンセル不可
  = Shipped を含む、など)。メモはチャットに出しファイルにしない。
- **仮定は `.fsl` に ASSUME タグ/コメントで畳む**(別メモファイルにしない)。
- **正例ペア(`reachable FullyRefunded`)が「沈黙して verified」を可視化する**:
  返金期間を設計層に素朴に持ち込んだ初版は、安全性 invariant は全て成立するのに
  返金経路が丸ごと死んでいた。正例ペアが `reachable_failed` で検出し、coverage が
  `refund` を名指しした(invariant だけなら素通りしていた)。

```bash
fslc verify examples/validation/order_refund.fsl --engine induction   # proved
fslc verify examples/validation/order_refund.fsl --depth 8            # verified + FullyRefunded witness
```
