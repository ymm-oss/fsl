# 経費精算 e2e: 3つの役割、1つの真実

このサンプルは、コンサル・PM・エンジニアが同じ経費精算ドメインを別々の粒度で書き、FSL で `business ⊒ requirements ⊒ design ⊒ implementation` をつなぐ旗艦例です。

## 連鎖図

```text
1_business.fsl
  ExpenseToBe           コンサル: To-Be 業務、統制、KPI、goal
      ⊒ implements
2_requirements.fsl
  ExpenseRequirements   PM: 金額、AUTO_LIMIT、要件原文、acceptance
      ⊒ fslc refine
3_design.fsl
  ExpenseDesign         エンジニア: 支払い2段階化、通知 outbox
      ⊒ fslc testgen + Adapter
impl/expense.py         FSL 非依存の Python 実装
```

## 役割別の見え方

コンサルは `1_business.fsl` に、経費精算 To-Be の業務フローを書きます。`Draft -> Submitted -> Approved/Rejected`、`Approved -> Paid`、少額自動承認とマネージャ承認の2レーン、統制 `CTRL-1` / `CTRL-2`、KPI `paid_claims`、完了可能性の goal が対象です。機械保証されるのは、KPI 整合、goal 到達、統制の有界応答性、そして invariant の帰納証明です。PM やエンジニアが下流を変えると、業務層の言葉では `implements` / `refine` の失敗として見えます。

PM は `2_requirements.fsl` に、業務を壊さないシステム要件を書きます。`Amount` と `AUTO_LIMIT` を導入し、`submit` を branches で少額レーンと高額レーンに分け、`REQ-1..4` と acceptance 2本を原文付きで残します。機械保証されるのは、要件単体の矛盾なし、`CountsMatchPaid`、acceptance の再生、業務層 `ExpenseToBe` への `implements: refines` です。コンサルが統制を変えると `implements` が落ち、エンジニアが内部設計を変えると `3_refines_2.fsl` の不対応として見えます。

エンジニアは `3_design.fsl` と `impl/expense.py` に、実装に近い設計と実コードを書きます。設計層では支払いを `pay_submit` / `pay_confirm` に分け、通知 `outbox` を持ちます。`3_refines_2.fsl` は設計の内部アクションを要件層へ対応させます。機械保証されるのは、設計 invariant の帰納証明、要件層への `refines`、そして `fslc testgen` が生成した pytest ハーネスによる実装 conformance です。PM の要件が変わると mapping または Adapter の失敗として見えます。

## コマンド

```bash
# 1. コンサル成果物: 業務層を帰納証明
./.venv/bin/python -m fslc verify examples/e2e/1_business.fsl --engine induction --deadlock ignore

# 2. PM 成果物: 要件層を有界検証し、業務層への implements も確認
./.venv/bin/python -m fslc verify examples/e2e/2_requirements.fsl --deadlock ignore

# 3. PM 成果物: 要件層 invariant を帰納証明し、implements: refines も確認
./.venv/bin/python -m fslc verify examples/e2e/2_requirements.fsl --engine induction --deadlock ignore

# 4. PM 成果物: acceptance が scenarios に出ることを確認
./.venv/bin/python -m fslc scenarios examples/e2e/2_requirements.fsl --deadlock ignore

# 5. エンジニア設計: 設計層を帰納証明
./.venv/bin/python -m fslc verify examples/e2e/3_design.fsl --engine induction --deadlock ignore

# 6. エンジニア設計: 設計層が要件層を refine することを確認
./.venv/bin/python -m fslc refine examples/e2e/3_design.fsl examples/e2e/2_requirements.fsl examples/e2e/3_refines_2.fsl --depth 8

# 7. 実装適合ハーネスを再生成する場合
#    注意: Adapter 骨格も再生成されるため、再生成後は impl/test_conformance.py の結線を戻す
./.venv/bin/python -m fslc testgen examples/e2e/3_design.fsl -o examples/e2e/impl/test_conformance.py

# 8. FSL 非依存の Python 実装を generated harness で検査
(cd examples/e2e/impl && ../../../.venv/bin/python -m pytest -q)

# 9. リポジトリ全体の回帰
./.venv/bin/python -m pytest tests/ -q
```

## 破壊デモ: 承認を飛ばす近道

次の一時改変では、エンジニアが設計層に `pay_without_approval` を追加し、`DesignDraft` から直接 `DesignPaymentSubmitted` に進めます。対応表ではこれを要件層の `pay` に対応させています。要件層の `pay` は `ReqAutoApproved` または `ReqManagerApproved` だけを許すため、`fslc refine` は `abs_requires_failed` を返します。

再現コマンド:

```bash
cp examples/e2e/3_design.fsl /private/tmp/3_design_shortcut.fsl
cp examples/e2e/3_refines_2.fsl /private/tmp/3_refines_shortcut.fsl
perl -0pi -e 's/\n  fair action pay_submit\(c: Claim\) \{/\n  fair action pay_without_approval(c: Claim) {\n    requires design[c].st == DesignDraft\n    requires outbox.size() < OUTBOX_CAP\n    design[c].st = DesignPaymentSubmitted\n    paid_count = paid_count + 1\n    outbox = outbox.push(c)\n  }\n\n  fair action pay_submit(c: Claim) {/s' /private/tmp/3_design_shortcut.fsl
perl -0pi -e 's/\n  action pay_submit\(c\)      -> pay\(c\)/\n  action pay_without_approval(c) -> pay(c)\n  action pay_submit(c)      -> pay(c)/' /private/tmp/3_refines_shortcut.fsl
./.venv/bin/python -m fslc refine /private/tmp/3_design_shortcut.fsl examples/e2e/2_requirements.fsl /private/tmp/3_refines_shortcut.fsl --depth 4
```

実際に取得した出力:

```json
{
  "fsl": "1.0",
  "result": "refinement_failed",
  "impl": "ExpenseDesign",
  "abs": "ExpenseRequirements",
  "at": "step",
  "violated_at_step": 1,
  "impl_action": {
    "name": "pay_without_approval",
    "params": {
      "c": 0
    },
    "loc": {
      "line": 77,
      "column": 3
    }
  },
  "kind": "abs_requires_failed",
  "impl_trace": [
    {
      "step": 0,
      "state": {
        "design": {
          "0": {
            "st": "DesignDraft",
            "amount": 0
          },
          "1": {
            "st": "DesignDraft",
            "amount": 0
          },
          "2": {
            "st": "DesignDraft",
            "amount": 0
          }
        },
        "paid_count": 0,
        "outbox": []
      }
    },
    {
      "step": 1,
      "state": {
        "design": {
          "0": {
            "st": "DesignPaymentSubmitted",
            "amount": 0
          },
          "1": {
            "st": "DesignDraft",
            "amount": 0
          },
          "2": {
            "st": "DesignDraft",
            "amount": 0
          }
        },
        "paid_count": 1,
        "outbox": [
          0
        ]
      },
      "action": {
        "name": "pay_without_approval",
        "params": {
          "c": 0
        },
        "loc": {
          "line": 77,
          "column": 3
        }
      },
      "changes": {
        "paid_count": {
          "from": 0,
          "to": 1
        },
        "design[0][st]": {
          "from": "DesignDraft",
          "to": "DesignPaymentSubmitted"
        },
        "outbox": {
          "from": [],
          "to": [
            0
          ]
        }
      }
    }
  ],
  "abs_before": {
    "req": {
      "0": {
        "st": "ReqDraft",
        "amount": 0
      },
      "1": {
        "st": "ReqDraft",
        "amount": 0
      },
      "2": {
        "st": "ReqDraft",
        "amount": 0
      }
    },
    "paid_count": 0
  },
  "abs_after_expected": {
    "req": {
      "0": {
        "st": "ReqPaid",
        "amount": 0
      },
      "1": {
        "st": "ReqDraft",
        "amount": 0
      },
      "2": {
        "st": "ReqDraft",
        "amount": 0
      }
    },
    "paid_count": 1
  },
  "abs_after_actual": {
    "req": {
      "0": {
        "st": "ReqPaid",
        "amount": 0
      },
      "1": {
        "st": "ReqDraft",
        "amount": 0
      },
      "2": {
        "st": "ReqDraft",
        "amount": 0
      }
    },
    "paid_count": 1
  },
  "mismatch": [],
  "hint": "the impl step does not correspond to the mapped abs action; fix the map expressions, the action correspondence, or guard the impl action"
}
```

この失敗は「状態の見た目は Paid に写せているが、要件層の `pay` を実行する前提条件を満たしていない」ことを示します。統合運用の核心はここで、実装詳細の追加そのものは許しつつ、PM 要件に無い抜け道は対応表の検査で止まります。
