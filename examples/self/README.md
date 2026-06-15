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

## 実装適合の錨

`fslc_session.fsl` は fslc の CLI 結果分類と exit-code severity を FSL でモデル化した
self-spec だが、モデル単体の `verify` / induction による内部整合の証明だけでは、
**実装 (`src/fslc/cli.py`) がその契約を守っているか**は保証されない。

`tests/test_self_conformance.py` がそのギャップを埋める。多様な outcome を出す spec 群に対し
実 CLI で `check` → (ok なら) `verify` → (verified なら) `verify --engine induction` を走らせ、

1. 各 subcommand の `result` とプロセス exit code が `exit_code()` の severity 表と一致すること
2. `ProvedImpliesVerified` / `SuccessRequiresCheck` などの契約が実結果で成立すること
3. 記録した `(subcommand, result)` 列を `fslc_session` の action 列へ写像し、`fslc replay` が
   `conformant` を返すこと(実 CLI の遷移がモデル状態機械に適合)
4. 契約違反の手書きトレースが `nonconformant` になること(負の対照)

を検査する。これによりメタ循環ドッグフーディングは「モデル検証」から
「実装適合検証」へ引き上げられる。

### fslc_monitor 錨 (replay runtime)

`fslc_monitor.fsl` は `src/fslc/runtime.py` の `Monitor` / `run_replay` が守るべき
契約(最初の reject で停止・全 ok でのみ conformant・reject 後は処理が進まない)を
モデル化する。`tests/test_self_conformance.py` の monitor 節が、ガード付き spec
(`specs/cart_v1.fsl`) に対する実 `fslc replay` の観測結果を `step_ok` / `step_reject` /
`finish` 列へ写像し、`fslc_monitor` への replay が `conformant` になることを検査する。
契約違反トレース(reject 後の step_ok 等)は `nonconformant` になる負の対照も含む。

### subcommand 被覆の拡張

従来の錨は `check` → `verify` → `induction` パイプラインのみ。以下を追加した。

| 実 subcommand | 写像 action | 備考 |
|---|---|---|
| `verify` (semantics error) | `verify_user_error` | `no_actions.fsl` 等、check ok だが verify が semantics エラー |
| `scenarios` | `scenarios_ok` | |
| `explain` | `explained_ok` | |
| `mutate` | `mutated_ok` | |
| `typestate` | `typestate_ok` | |
| `refine` (成功) | `refines_ok` | |
| `refine` (失敗) | `refine_failed` | |
| `replay` (conformant) | `replay_conformant` | |
| `replay` (nonconformant) | `replay_nonconformant` | |

`tool_fault` (exit 3, internal error) はモデルには在るが、内部エラーを意図的に
誘発できないため**実装錨は未整備**。
