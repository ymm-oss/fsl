# コンサルタント向けサンプル — 業務改革の As-Is / To-Be 統制検査

**業務プロセスと社内規程を「機械が検査できる文書」として書く**例です。
題材は経費精算ワークフローの改革提案:

> 現行(As-Is)は全件マネージャ承認で滞留している。
> 改革案(To-Be)では少額レーンを自動承認化して処理を速くしたい。
> **ただし統制(承認を経ない支払いが無い等)は一切弱めない。**

| ファイル | 内容 |
|---|---|
| [`asis_expense.fsl`](asis_expense.fsl) | 現行業務(ヒアリング結果): プロセス+統制規程 CTRL-1/2+KPI |
| [`tobe_expense.fsl`](tobe_expense.fsl) | 改革案: 自動承認レーンを追加、統制は維持 |
| [`tobe_refines_asis.fsl`](tobe_refines_asis.fsl) | **改革の統制検査**: To-Be の業務対応表(自動承認 = 承認行為) |

## 何が嬉しいか(3点)

1. **ヒアリング結果の矛盾が、提案前に機械で見つかる。** 規程を原文ごと書くと、
   「申請が放置される進行順序は無いか」「規程同士が衝突しないか」を全数検査
   できる(As-Is/To-Be とも全規程を無限深度で証明済み)。
2. **「改革しても統制は守られる」を提案書に証拠付きで書ける。** 下のコマンド
   1行が「To-Be のあらゆる業務の流れは As-Is でも許された流れに対応する
   (=新しい抜け道を作っていない)」ことを検査する。
3. **提案がそのまま下流につながる。** この業務層を要件定義
   (`requirements`、`examples/pm/` 参照)→ 設計 → 実装テストまで
   refinement で連鎖でき、コンサル成果物が「死んだ文書」にならない。

## 動かし方

```bash
# 現行・改革案それぞれの規程の証明
fslc verify examples/consulting/asis_expense.fsl --engine induction --deadlock ignore
fslc verify examples/consulting/tobe_expense.fsl --engine induction --deadlock ignore

# 改革の統制検査(To-Be ⊒ As-Is)
fslc refine examples/consulting/tobe_expense.fsl \
            examples/consulting/asis_expense.fsl \
            examples/consulting/tobe_refines_asis.fsl --depth 6
# → {"result": "refines", ...} = 統制は維持されている
```

## 「統制が破られるとどう見えるか」の実例

改革案に「承認を飛ばす即時払いレーン」(`quick_pay: Submitted -> Paid`)が
紛れ込んだ、という想定で検査すると:

```json
{
  "result": "refinement_failed",
  "kind": "abs_requires_failed",
  "impl_action": { "name": "quick_pay" },
  "violated_at_step": 2,
  "abs_before": { "claim_stage": { "0": "Submitted", ... }, "paid_claims": 0 },
  "impl_trace": [ "(init)", "submit", "quick_pay" ]
}
```

読み方: **どの業務(quick_pay)が**、**どの統制を破るか**(As-Is の支払いは
承認済みが前提 = `abs_requires_failed`)、**最短の再現手順(申請→即時払いの
2手)**。「この改革案は現行統制からの逸脱を含む」をレビュー会の前に
機械が指摘してくれる。

## 書き方の要点

- プロセス図の矢印 = `transition`、規程 = `policy ID "原文"`、
  経営指標 = `kpi`、業務ゴール = `goal`。
- As-Is と To-Be は別ファイルに書き、対応表(`refinement`)で結ぶ。
  「To-Be の自動承認は As-Is のマネージャ承認に相当する(承認という
  統制行為に変わりはない)」のような**解釈そのものを対応表に明示**する —
  ここが監査で問われる判断であり、機械はその解釈の下での無矛盾を保証する。
- 検証する世界は小さくてよい(申請3件)。バグは小さな世界でも再現する。
