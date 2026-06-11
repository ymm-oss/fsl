# 3層連携スパイク(コンサル → 要件 → [設計])

DESIGN-layers.md §2 の実証。**無修正の現行カーネル**で、業務層と要件層を
書き、refinement で連携できることを示す。

| ファイル | 層 | 結果 |
|---|---|---|
| `return_policy.fsl` | コンサル(業務プロセス+ポリシー2本) | proved + leadsTo checked |
| `return_system.fsl` | 要件(金額・自動承認閾値・承認キュー) | proved |
| `return_refines.fsl` | 層間写像(enum→enum のネスト条件写像) | refines |

```bash
fslc verify examples/layers/return_policy.fsl --engine induction --deadlock ignore
fslc verify examples/layers/return_system.fsl --engine induction --deadlock ignore
fslc refine examples/layers/return_system.fsl examples/layers/return_policy.fsl \
            examples/layers/return_refines.fsl --depth 6
```

要件層の submit が「金額次第で業務上の承認 or 何も起きない」になる箇所は
submit_small / submit_large への手動分割で表現している — fsl-req 方言の
`branches` 構文(DESIGN-layers.md §4)はこの分割を自動化する。
