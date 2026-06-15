# self 仕様 — fslc 自身を FSL でドッグフードする

このディレクトリは self-specs(メタ循環ドッグフーディング)を置く。つまり、
`fslc` 自身の設計契約を FSL の状態機械として書き、検証器を検証器自身の対象にする。

## 登場人物

| ファイル | モデル化している契約 |
|---|---|
| `fslc_session.fsl` | CLI 結果分類と exit-code severity の順序。check 成功後だけ成功結果へ進め、`proved` は `verified` を含み、internal error は修復不能。 |
| `fslc_monitor.fsl` | replay runtime の reject-stickiness。いったん nonconformant になったログは戻らず、conformant は全 step が ok のときだけ。 |
| `refinement_algebra.fsl` | refinement 層を通じて safety は伝播し、liveness は伝播しない。安全を壊す変更は valid refinement link ではない。 |

## 実行

```bash
E=examples/self

# fslc_session: ToolFault や各成功結果など意図した終端状態を terminal { } で宣言
./.venv/bin/python -m fslc check  $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl --engine induction

# fslc_monitor: Conformant / Nonconformant を terminal { } で宣言
./.venv/bin/python -m fslc check  $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl --engine induction

# refinement_algebra: reflexive_refine が常に enabled なので終端状態は無い
./.venv/bin/python -m fslc check  $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl --engine induction
```

結果はいずれも `check` が `ok`、通常 `verify` が `verified`、induction が `proved`。
`fslc_session` / `fslc_monitor` は **意図した終端状態を `terminal { }` ブロックで
宣言**しているため `--deadlock warn`(既定)でもデッドロック警告は出ない。terminal に
**含めない**停止状態(予期せぬデッドロック)があれば従来どおり警告される。
これは DOGFOOD-11 の F23(意図停止を宣言する手段の不在)への対応。

`fslc mutate` の kill-rate は、invariant が死んだ ghost に寄りかかっていないかを
見る非自明性(anti-ghost)指標として使った。
