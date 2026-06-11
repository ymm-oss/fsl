# PM / PdM 向けサンプル — 解約フロー(救済オファー付き)

**プロダクト要件を「機械が検査できる文書」として書く**例です。コードは出てきません。
題材は誰でも知っている解約フロー: 解約申請 → 救済オファー提示 → 承諾(継続)or 辞退(解約)。

| ファイル | 何を書くか | 読者 |
|---|---|---|
| [`cancel_flow.fsl`](cancel_flow.fsl) | **業務フロー+業務ルール**(プロセス図・ポリシー・KPI・ゴール) | PM / 事業側 |
| [`cancel_system.fsl`](cancel_system.fsl) | **システム要件**(要件 ID+原文・画面遷移・受け入れ基準) | PdM / 開発との接点 |

## 何が嬉しいか(3点)

1. **規程・要件の矛盾を、実装前に機械が見つける。** 「申請を放置しない」
   「オファーは1回だけ」のようなルールを原文ごと書くと、あらゆる操作順序に
   対して成り立つかを検証器が確認する(下のコマンド1行)。
2. **違反には「壊れた要件の ID と原文」と「再現手順」が付く。** レビュー会で
   議論する材料がそのまま出てくる(下の実例)。
3. **受け入れ基準がそのままテストになる。** `acceptance` に書いた手順は
   検証時に自動再生され、開発チームの統合テスト雛形として出力される。
   要件文書とテストがズレない。

## 動かし方

```bash
# 業務層: 全ルールの証明(proved = どんな操作順序でも成り立つ)
fslc verify examples/pm/cancel_flow.fsl --engine induction --deadlock ignore

# 要件層: 要件の検査 + 業務フローとの整合検査(implements)が1コマンドで同時に
fslc verify examples/pm/cancel_system.fsl --deadlock ignore

# 受け入れ基準・代表シナリオを開発向けテスト雛形として出力
fslc scenarios examples/pm/cancel_system.fsl --deadlock ignore
```

現状はどちらも全ルール成立(業務層は無限深度で証明済み)。

## 「違反するとどう見えるか」の実例

開発が「解約フォームから直接解約完了」のショートカット(オファーを飛ばす)を
追加した、という想定で要件層を改変すると:

```json
{
  "result": "violated",
  "requirement": { "id": "REQ-4", "text": "オファーは提示済みの契約に再提示されない" },
  "last_action": { "name": "quick_churn" },
  "trace": [
    { "step": 0, "state": { "scr[0]": { "st": "Browsing",    "offered": false } } },
    { "step": 1, "state": { "scr[0]": { "st": "CancelForm",  "offered": false } },
      "action": "tap_cancel" },
    { "step": 2, "state": { "scr[0]": { "st": "GoodbyePage", "offered": false } },
      "action": "quick_churn" }
  ],
  "implements": { "abs": "CancelFlow", "result": "violated" }
}
```

読み方: **どの要件が壊れたか(REQ-4 と原文)**、**何をしたら壊れたか
(quick_churn)**、**最短の再現手順(3ステップ)**、さらに**業務フロー
(CancelFlow)からの逸脱**も同時に検出されている。つまり「オファーを
経ない解約は、システム要件にも業務規程にも違反」が1つの出力で分かる。

## 書き方の要点(2ファイルのコメントにも記載)

- 業務ルールは `policy POL-1 "原文" ...`、要件は `requirement REQ-1 "原文" { ... }`
  のように **ID と原文をそのまま**書く。違反時にそのまま表示される。
- 「いつか必ず〜する」(放置しない系)は `responds`、「常に〜である」は
  `invariant`、「〜に到達できるはず」は `goal`。
- 検証する世界は小さくてよい(契約3件)。バグは小さな世界でも再現する。
- さらに踏み込むなら: SLA(「申請から K ステップ以内に提示」)も書ける
  → `docs/DESIGN-nfr.md`、3層構成の全体像 → `examples/layers/`。
