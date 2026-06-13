# FSL — `forbidden`(負の受け入れ基準 / must-forbid)実装設計

動機: issue #3(妥当性確認ロードマップ #1 の類型4/6)。`acceptance`(must-allow)は
「この操作列は通る」を check 時に再生検証するが、「この操作列は**拒否されるべき**」
(must-forbid)を書く手段がなかった。ガード漏れ等の**過小制約**は、安全性 invariant を
1つも破らずに「禁止すべき操作」を受理してしまうため verify では沈黙する。`forbidden` は
その沈黙を破る独立チャネル(別エージェントが NL とアクション署名だけから正負トレースを
書く交差検証の受け皿)。

## 1. 構文(requirements 方言)

```fsl
forbidden FB-1 "出荷後のキャンセルは拒否される" {
  pay(0)  ship(0)        // 前提(セットアップ): すべて enabled で ok
  cancel(0)              // 最後のステップ: 拒否されることを期待
  expect rejected
}
```

`acceptance_def` の写し。`expect rejected` はインラインマーカー(`acceptance` の
`expect <式>` と違い状態述語を評価しない)。`FB-1` は `REQ_ID` トークンに合致。

## 2. 意味論(具象 Monitor 再生、check 時)

- 前提ステップ `steps[0..n-2]` は全て `ok`(enabled かつ違反なし)であること。
- **最後のステップが拒否**されれば成功。拒否は2系統:
  - (a) **not-enabled**(`requires_failed` / 範囲外 `bad_call`)— **主用途**。
    安全性 invariant に見えない「ガードによる正しい禁止」。
  - (b) **実行すると違反**(`invariant` / `type_bound` / `partial_op` / `ensures`)。
    ただし違反状態が到達可能 ⇒ **spec 自体が verify で violated** を意味する
    (ケースbは「forbidden は満たすが spec はバグ」のシグナル)。出力の
    `rejected_by` がこの区別を担う。
- 最後のステップが `ok`(=受理された)→ `kind: "forbidden"` エラー + `accepted_trace`。
- 前提ステップが `ok` でない → `kind: "forbidden_setup"`(トレース不正、成功扱いにしない)。
- ステップ0個 → `kind: "forbidden"` エラー(最低1ステップ必要)。

## 3. 波及(検証エンジン・Monitor は無改修)

- grammar.py: `forbidden_def`(`expect rejected` インライン)+ トランスフォーマ。
- dialects.py: `("__forbidden", …)` 収集。model.py: `spec["forbidden"]` へ格納。
- acceptance.py: `replay_forbidden` / `validate_forbidden`。`replay_acceptance` の
  写しで、差分は「前提は全 ok / 最後は ok:False を期待 / `expect` 状態評価なし」。
  `Monitor.step()` が requires_failed / invariant / type_bound / partial_op / ensures
  いずれの拒否でも `ok:False` + `kind` を返すため、(a)(b) とも step() の戻りだけで判定。
- cli.py: `_forbidden_error` を check / verify 両経路へ。bmc.py: `scenarios` に
  `forbidden_<ID>`(`rejected_by` 付き)を出力 → testgen のネガティブテストへ。

## 4. テスト(tests/test_forbidden.py)

ケースa満足 + scenario / 受理 → `kind:"forbidden"` + accepted_trace / setup 破損 →
`forbidden_setup` / ケースb(rejected_by=type_bound かつ verify violated)/ 空ステップ /
verify ゲートが BMC 前に発火。gallery 正例(`small_forbidden_guarded_cancel.fsl` → verified)
と誤り例(`forbidden_op_accepted.fsl` → error/forbidden)。

## 5. 関連

`acceptance`(DESIGN-bridge / DESIGN-dialects)の双対。過小制約の検出は #4 vacuity
(`always_true_requires`)・#6 mutate と相補。経緯: 実走 DOGFOOD-9、ロードマップ #1。
