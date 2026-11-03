---
fsl_document_schema: fsl-requirements-document-v2
view: requirements
lang: ja
source: ./spec.fsl
renderer: fslc-document-renderer
renderer_version: 1.3.0
normative_scope: generated-claim-blocks-only
spec_digest: sha256:1d8ad00cf360d39990a8cba12ef5bd10961524e774054fa6dbe3f548a2e44aca
claim_set_digest: sha256:6b11a312eb01a7a8f2c5ac3bdd6c9f4f7d0a116441e963f709c6b90d10dd7afe
glossary_digest: sha256:97f7cdedbd9d145f246108a434919e64981400ae243ae1a8d84a93400a289330
evidence_digest: sha256:7b895b820c91b5537bc5969943722bed752049707a0d3eaba49985aa6a6816b9
---

# 要件仕様書: ApprovalReview

<!-- fsl:slot begin name="background" normative="false" -->
## 背景

（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）
<!-- fsl:slot end -->

## 本書の位置づけ

本書の「形式化された意味」は、検査済みの FSL 仕様から決定論的に生成した規範文である。同じ仕様からは、常にバイト単位で同一の文書が生成される。

本書が保証するのは、FSL が検査した構造 — 実行条件、更新、事後条件、否定、公平性、期限、範囲 — を欠落なく決定論的に表示することである。本書は、日本語の文と FSL の式が意味的に同値であることを証明するものではない。また、FSL が元の業務意図を正しく捉えていることも保証しない。要件原文と形式化された意味との一致の確認は、人間のレビューに委ねられる。

規範文は claim の種類ごとの固定テンプレートで生成しており、一義性を流暢さより優先している。

## 全体の意味規約

本仕様のすべての操作に、次の実行規約が適用される。

- 更新はステップ単位で同時にコミットされる（`updates: simultaneous`）。
- 更新の右辺は遷移前の状態を読む（`reads: pre_state`）。
- 事後条件・状態不変条件・遷移条件のいずれかに違反するステップはコミットされず、状態は遷移前のまま残る（`failed_step: rollback`）。
- 公平性の仮定は弱い公平性である（`fairness: weak`）。公平性は `fair` と宣言された操作にのみ適用される。

## 検証結果の読み方

- `proved(induction)`: k帰納法により全深さで証明済み。
- `bounded(BMC depth k)`: 深さ k までのすべての実行を検査した。それ以遠の実行については何も保証しない。これは証明ではない。
- `replay-observed`: 具体的な実行ログ／トレースを仕様と照合した結果のみ。すべての実行に対する保証ではない。
- `statistical`: 統計的裏付け（Wilson 区間）。個別の実行に対する保証ではない。
- `not_run`: 対応するエビデンスは供給されていない。

- 保証クラスは検証手法の網羅範囲を表し、合否を表さない。反例が存在する場合もクラスは `bounded` のままである。合否は各エビデンスの結果欄に別掲する。
- `fair` は公平性という検証上のスケジューリング仮定であり、即時実行の保証ではない。
- `leadsTo`（進行条件）は将来のある時点での成立を要求する demand であり、それ自体は検証結果ではない。
- refinement は safety の保存を検査するものであり、liveness を自動的には保存しない。
- エビデンスが供給されていない側面は、省略されず `not_run` と明示される。

本書は要求内容（what）を定義する。検証がどこまで実施されたか（how far）は `fslc ledger` が生成する監査台帳が示す。両者は同じ保証クラス語彙を用いる。

## 要件

### REQ-190

**要件原文（意図。形式意味との一致は人間が確認する）**

> a pending review can be approved

（出典: `./spec.fsl:12`）

> an approved review remains approved

（出典: `./spec.fsl:18`）

**形式化された意味（FSLから決定論的に生成）**

<!-- fsl:claim begin id="action:approve#operation" digest="sha256:adfacbf7fe03dc9f7a3fddec77580d35f7f8c1999b5d6150758ff722b03fe000" -->
#### 操作: 承認する（`approve`）

- 識別子: `action:approve#operation`
- 出典: `./spec.fsl:12`
- パラメータ: なし

操作 `approve` を実行できるのは、次の条件を満たす場合に限る。

1. `approved` が `false` である。

操作が成功した場合、次の更新を同一ステップで同時に適用する。更新の右辺は遷移前の状態を読む。

1. `approved` を `true` にする。
2. `ever_approved` を `true` にする。

この操作に公平性の仮定はない。実行可能（enabled）であっても、実行されることは保証されない。
<!-- fsl:claim end -->

<!-- fsl:claim begin id="property:invariant:ApprovalPersists#state_rule" digest="sha256:182ba266fa57e075519e5884bb512c816b711ca7e4dddc427f3e9ace8984ed5c" -->
#### 状態不変条件: `ApprovalPersists`

- 識別子: `property:invariant:ApprovalPersists#state_rule`
- 出典: `./spec.fsl:18`

初期化後、および成功した各操作のコミット後に、次の条件が成立しなければならない。

`ever_approved` が `true` であるならば、`approved` が `true` である。

この条件を満たさない候補遷移はコミットされない。条件が自動的に修復・回復されることを意味しない。
<!-- fsl:claim end -->

**保証クラス**

- 形式検証: `proved(induction)`（出典: `./evidence-b.json`）; `bounded(BMC depth 2)`（出典: `nested/../evidence-a.json`）
- 実装適合: `not_run` — 対応するエビデンスは供給されていない。
- 統計的裏付け: `not_run` — 対応するエビデンスは供給されていない。

## 要件 ID に紐づかない形式要素

該当なし。

## 未決定事項

未決定として宣言された事項はない。

## 解析スコープ

本仕様に解析スコープの宣言（instances / values）はない。

## 用語集

本文中の識別子表示は次の用語集ラベルに基づく。ラベルは表示のみを変更し、検証条件を変更しない。

- `action:approve`: 承認する

## 検証エビデンス出典

本書に指定された外部エビデンスの一覧。要件 ID に対応する行だけが各要件の保証クラス欄に反映される。

- `./evidence-b.json`（結果: ``、対象要件 ID: REQ-190）
- `nested/../evidence-a.json`（結果: ``、対象要件 ID: REQ-190）

## 生成情報

- 生成元仕様: `./spec.fsl`（`ApprovalReview`、dialect: `spec`）
- spec digest: `sha256:1d8ad00cf360d39990a8cba12ef5bd10961524e774054fa6dbe3f548a2e44aca`
- claim set digest: `sha256:6b11a312eb01a7a8f2c5ac3bdd6c9f4f7d0a116441e963f709c6b90d10dd7afe`
- 形式要素の分類: rendered 2 件 / unattributed 0 件 / unsupported 1 件
- 自然言語への言い換えを行わなかった式: 0 箇所
- 次の形式要素は RCIR v1 が対応していないため、本書には規範文として現れない。省略は明示され、黙って落とされることはない。
- `init`: no v1 claim kind projects initial-state definitions
