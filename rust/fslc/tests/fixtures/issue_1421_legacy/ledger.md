# 意図ずれ監査台帳: ApprovalReview

- 対象: `./spec.fsl`
- 保証限界: BMC（有界モデル検査）: **深さ 2 までの全実行を網羅**。それ以遠の反例は本台帳の対象外
- 保証クラス（要件ID別）: `proved(induction)` 全深さで証明 / `bounded(BMC depth k)` 深さkまで網羅 / `replay-observed` ログ照合のみ / `statistical` Wilson区間による統計的裏付け / `not_run` 形式的根拠なし。詳細は `docs/design/DESIGN-assurance-classes.md`。
- この台帳が保証するのは **書かれた仕様の内部整合**。仕様が現実の意図に忠実かは各行の **判断** 欄で人間が担保する。

## リスク一覧（要件ID別）

| 要件ID | 業務目的 | 状態 | 保証クラス | 検出種別 | リスク | 判断者 | 次アクション |
|---|---|---|---|---|---|---|---|
| REQ-190 | an approved review remains approved | 🟢 確認済（承認可） | bounded(BMC depth 2) | — | — | — | — |
| （仕様全体） | 要件ID未付与の検出 | 🟡 移行候補 | bounded(BMC depth 2) | migration | — | ____ | 下記詳細 |

## 要件ID別詳細

### （仕様全体）
- **検出**: `migration` — legacy_string_metadata
  - 反例要約: 移行候補（legacy_string_metadata）: legacy string metadata is deprecated; use a typed annotation
  - 業務翻訳: 検出種別 migration（付録の生 JSON を参照）。
  - 次アクション: 責任者が対応方針を決定
- **検出**: `migration` — legacy_string_metadata
  - 反例要約: 移行候補（legacy_string_metadata）: legacy string metadata is deprecated; use a typed annotation
  - 業務翻訳: 検出種別 migration（付録の生 JSON を参照）。
  - 次アクション: 責任者が対応方針を決定
- 判断: ☐ 承認　☐ 差戻し　☐ リスク受容　／　判断者: ____　期限: ____

## 付録: 生 JSON 反例（証跡）

<details><summary>raw findings</summary>

```json
[
  {
    "code": "legacy_string_metadata",
    "kind": "legacy_string_metadata",
    "taxonomy": "deprecated",
    "severity": "warning",
    "edition": "current",
    "message": "legacy string metadata is deprecated; use a typed annotation",
    "loc": {
      "file": "./spec.fsl",
      "line": 12,
      "column": 20,
      "end_line": 12,
      "end_column": 63
    },
    "canonical_replacement": "@requirement(\"REQ-190\", \"a pending review can be approved\")",
    "machine_applicable": true,
    "edits": [
      {
        "start": 228,
        "end": 228,
        "replacement": "@requirement(\"REQ-190\", \"a pending review can be approved\")\n  "
      },
      {
        "start": 245,
        "end": 288,
        "replacement": ""
      }
    ]
  },
  {
    "code": "legacy_string_metadata",
    "kind": "legacy_string_metadata",
    "taxonomy": "deprecated",
    "severity": "warning",
    "edition": "current",
    "message": "legacy string metadata is deprecated; use a typed annotation",
    "loc": {
      "file": "./spec.fsl",
      "line": 18,
      "column": 30,
      "end_line": 18,
      "end_column": 76
    },
    "canonical_replacement": "@requirement(\"REQ-190\", \"an approved review remains approved\")",
    "machine_applicable": true,
    "edits": [
      {
        "start": 369,
        "end": 369,
        "replacement": "@requirement(\"REQ-190\", \"an approved review remains approved\")\n  "
      },
      {
        "start": 396,
        "end": 442,
        "replacement": ""
      }
    ]
  }
]
```

</details>
