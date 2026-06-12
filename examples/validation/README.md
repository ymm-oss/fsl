# validation — 妥当性確認ワークフローの実走例

「内部整合(verify)は通るが、元の意図からズレた仕様」を、書く前・書いた後の
規律でどう捕まえるかの実走成果物。経緯と知見は [`../../docs/DOGFOOD-9.md`](../../docs/DOGFOOD-9.md)。

| ファイル | 内容 |
|---|---|
| [`order_refund.fsl`](order_refund.fsl) | 注文の支払い・キャンセル・返金フロー(在庫付き)の設計層 spec = **凍結した契約**。proved |
| [`order_refund_windowed.fsl`](order_refund_windowed.fsl) | 設計変種: **返金期間ウィンドウ付き**(ASSUME-5 で先送りした R5 の設計層実装案)。proved |
| [`order_refund_windowed_refines.fsl`](order_refund_windowed_refines.fsl) | 上の写像(tick は stutter)。**refines** — 契約を壊さず期間制限を追加できる(OCP/LSP) |
| [`order_refund_instant.fsl`](order_refund_instant.fsl) | **負例プローブ**: cancel を飛ばす「即時返金」。単体では verified |
| [`order_refund_instant_refines.fsl`](order_refund_instant_refines.fsl) | 上の写像。**refinement_failed / abs_requires_failed** — `pay → instant_refund` の2手で契約迂回が出る |

## このサンプルが示すこと

- **形式化メモ**で要件の「境界の含意」を書く前に洗い出す(出荷"後"はキャンセル不可
  = Shipped を含む、など)。メモはチャットに出しファイルにしない。
- **仮定は `.fsl` に ASSUME タグ/コメントで畳む**(別メモファイルにしない)。
- **正例ペア(`reachable FullyRefunded`)が「沈黙して verified」を可視化する**:
  返金期間を設計層に素朴に持ち込んだ初版は、安全性 invariant は全て成立するのに
  返金経路が丸ごと死んでいた。正例ペアが `reachable_failed` で検出し、coverage が
  `refund` を名指しした(invariant だけなら素通りしていた)。
- **設計検討は契約適合の検査に翻訳できる**(fsl-design-review スキルの実走):
  窓付き変種は抽象契約を1行も編集せずに refines(先送り判断 ASSUME-5 の機械検証)。
  逆に「即時返金」は**単体 verify では何も破らない**のに refine が契約迂回を
  最短2手で示す — verify では見えない設計逸脱を refinement が捕まえる実例。

```bash
fslc verify examples/validation/order_refund.fsl --engine induction            # proved(契約)
fslc verify examples/validation/order_refund_windowed.fsl --engine induction   # proved(変種)
fslc refine examples/validation/order_refund_windowed.fsl \
            examples/validation/order_refund.fsl \
            examples/validation/order_refund_windowed_refines.fsl --depth 8    # refines
fslc refine examples/validation/order_refund_instant.fsl \
            examples/validation/order_refund.fsl \
            examples/validation/order_refund_instant_refines.fsl --depth 8     # refinement_failed
```
