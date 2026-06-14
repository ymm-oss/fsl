# ui_spike — fsl-ui(画面遷移方言)スパイク資産

issue #9 のスパイク(素の fsl で画面フローを書き、検証・要件層への refine を確認)。
所見・展開規則案・go/no-go は [`../../docs/DESIGN-ui.md`](../../docs/DESIGN-ui.md)。

| ファイル | 内容 |
|---|---|
| [`return_ui.fsl`](return_ui.fsl) | 返品申請の画面フロー(素の fsl)。verified + proved。screen=enum / navigate=action / 袋小路なし=leadsTo / 二重送信防止=invariant / 全画面到達=reachable |
| [`return_req_min.fsl`](return_req_min.fsl) | 要件層のエッセンス(承認後のみ支払い・台帳整合)。refine の abs |
| [`ui_refines_req.fsl`](ui_refines_req.fsl) | UI フロー → 要件への写像。**refines**(UI 専用ステップ=stutter、コミット=要件アクション) |
| [`navstack.fsl`](navstack.fsl) | back stack を `Map<Depth,Screen> + depth`(LIFO)で。Seq は FIFO で不向き |

```bash
fslc verify examples/ui_spike/return_ui.fsl --engine induction         # proved
fslc refine examples/ui_spike/return_ui.fsl examples/ui_spike/return_req_min.fsl \
            examples/ui_spike/ui_refines_req.fsl --depth 8             # refines
fslc verify examples/ui_spike/navstack.fsl --deadlock ignore           # verified
```

スパイクの結論: カーネルの意味論変更なしに画面フローを表現でき、要件層へ refine する。
方言化(`expand_ui`)は AST 糖衣として成立する見込み(DESIGN-ui.md の go/no-go 参照)。
