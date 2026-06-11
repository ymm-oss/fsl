# 3層チェーン(コンサル → 要件 → 設計)— 方言による完全版

DESIGN-layers.md の最終形。返品ドメインを3つの方言で書き、refinement で連鎖させる。

| ファイル | 層 / 方言 | 結果 |
|---|---|---|
| `return_policy.fsl` | コンサル / `business`(process・policy・kpi・goal) | proved |
| `return_system.fsl` | 要件 / `requirements`(requirement・branches・acceptance・implements) | verified + **implements: refines** + proved |
| `return_impl.fsl` | 設計 / カーネル fsl(支払い2段階化+通知キュー) | proved |
| `return_impl_refines.fsl` | 設計→要件の写像 | refines |

```bash
fslc verify examples/layers/return_policy.fsl --engine induction --deadlock ignore
fslc verify examples/layers/return_system.fsl --deadlock ignore       # implements が同時検査される
fslc refine examples/layers/return_impl.fsl examples/layers/return_system.fsl \
            examples/layers/return_impl_refines.fsl --depth 5
fslc scenarios examples/layers/return_system.fsl --deadlock ignore    # acceptance_AC-1 が出る
```

見どころ:

- **要件 ID の透過**: 要件層のガードを壊すと、反例 JSON に
  `requirement: {id: REQ-3, text: 支払いは承認後のみ・台帳整合}` と
  上位層への `implements: {result: violated}` が同時に載る — どの要件が
  壊れ、業務層の何に波及するかが1つの診断で分かる。
- **branches**: 「金額次第で業務上の承認 or 保留」というデータ依存の対応を
  `when ... maps ...` で宣言し、自動分割される(coverage 表示は
  `submit[a <= AUTO_LIMIT]`)。下流の refinement からは内部名
  `submit__b1` で参照する(現状の制約)。
- **acceptance**: 要件層の受け入れ基準が check 時に具象 Monitor で再生検証され、
  scenarios → testgen と流れて実装の適合テストになる。
