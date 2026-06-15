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

# fslc_session: ToolFault など意図した終端状態があるため deadlock は無視する
./.venv/bin/python -m fslc check  $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl --deadlock ignore
./.venv/bin/python -m fslc verify $E/fslc_session.fsl --engine induction --deadlock ignore

# fslc_monitor: Conformant / Nonconformant は意図した終端状態なので deadlock は無視する
./.venv/bin/python -m fslc check  $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl --deadlock ignore
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl --engine induction --deadlock ignore

# refinement_algebra: reflexive_refine が常に enabled なので deadlock ignore は不要
./.venv/bin/python -m fslc check  $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl --engine induction
```

結果はいずれも `check` が `ok`、通常 `verify` が `verified`、induction が `proved`。
`--deadlock error` では `fslc_session` の `ToolFault`、`fslc_monitor` の
`Conformant` など、仕様上そこで停止する状態も deadlock として報告される。

`fslc mutate` の kill-rate は、invariant が死んだ ghost に寄りかかっていないかを
見る非自明性(anti-ghost)指標として使った。
