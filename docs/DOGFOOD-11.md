# DOGFOOD-11: メタ循環ドッグフーディング — fslc 自身の設計契約を FSL で検証し、検出器の盲点を炙り出す (2026-06-15)

DOGFOOD 1-10 は banking、reservation、SLA など外部ドメインを対象にしてきた。
今回は初めて fslc 自身の振る舞い契約をモデル化したメタ循環ドッグフーディングである。
成果物は `examples/self/` の3仕様。

## 結果

- `fslc_session` は CLI exit-code severity classification を形式的に証明した。success requires check pass、proved⊒verified、internal errors non-repairable。
- `fslc_monitor` は replay reject-stickiness を証明した。once nonconformant is irreversible、conformant only when all steps ok。
- 修正後の `refinement_algebra` は "safety propagates, liveness does not" を非自明に検査する。

## 知見

- **F22 (最重要・検出器の盲点):** --vacuity も単発 verify も「一度も代入されない state 変数上の恒真 invariant(死んだゴースト)」を検出しない。refinement_algebra 初稿は verified・vacuity警告0なのに mutate kill-rate 6.4%(73/78 survived)。他2本は 71%/67%。mutate の生存率が骨抜きの唯一の指標だった(DOGFOOD-10 F21「invariant弱化は mutate でしか見えない」の延長線)。改善候補: vacuity 検査が「どのアクションからも代入されない変数だけを参照する invariant/後件」を静的に警告できると安価に閉じる。

- **F23 (設計/言語ギャップ):** 意図した終端状態(proved/conformant/tool_fault 等)を宣言する構文が無く、--deadlock ignore を全体にかけるしかない → 意図せぬ deadlock も同時に隠れる。repair_loop.fsl も同理由で --deadlock ignore 必須。per-state/per-action の terminal/final 注釈があれば意図した停止とバグを区別できる。

- **F24 (言語ギャップ):** 「この状態からこのアクションは起動不可/この遷移は禁止」を直接表明する性質構文が無く、ghost+guard で間接表現するしかない。3本すべてで発生(RejectIsSticky / NoStepAfterReject / ToolFaultNotRepairable)。

- **F25 (表現力):** refinement の反射性・推移性のような関係/代数的性質を公理として書けず、状態機械として「過程を模擬」するしかない。これが F22 の死んだゴースト罠を招きやすい(refinement_algebra が実証)。

- **F26 (軽微):** --deadlock=warn の警告メッセージ文字列が deadlock 状態名を欠く("deadlock reachable at step N" のみ)。JSON の deadlock.trace には最終状態まで入っている(bmc.py:2851 が文字列に載せていないだけ)。

- **F27 (テスト容易性):** 単一 invariant だけを狙って検査する手段(--property/--invariant 相当)が無い。verify は全 invariant を一括検査し「最初に見つかった違反」を報告するため、非空虚プローブで特定の invariant(例: SafetyPropagates)の violation を確認したくても、より汎用の invariant(SafetyPreservedAtEveryLayer)が先に報告されてしまい、狙った invariant が報告対象になるよう条件を絞る手間が要る。probe の精度向上のため単一性質指定オプションが候補。

## 改修状況

調査で見つけた所見のうち、コード改修に着手したもの:

| 所見 | 対応 | 状態 |
|---|---|---|
| F23(意図停止の宣言) | `terminal { <述語> }` ブロックを新設(grammar/model/bmc)。述語を満たす停止状態を deadlock 検査から除外。examples/self を terminal 化し `--deadlock ignore` 依存を解消 | **完了**(`94cf68f`) |
| F26(deadlock 状態表示) | warn メッセージに状態を含める。例 `deadlock reachable at step 1 (state: status=ToolFault, ...)` | **完了**(`94cf68f`) |
| F27(単一 invariant 検査) | `verify --property <Name>` を追加。存在しない名前は usage エラー(exit 2) | **完了**(`94cf68f`) |
| F22(死んだゴースト恒真) | `--vacuity` に「どのアクションも代入しない frozen 変数を init 値に固定したとき、動的変数の値によらず恒真になる invariant」を Z3 で静的検出(kind `tautology_over_frozen`)。frozen 変数を全く参照しない/state を参照しない invariant は対象外。refinement_algebra の自明な baseline ゴーストを整理(mutate kill-rate 77.2% 維持)。既存コーパス全体で偽陽性ゼロを確認 | **完了** |
| F24(遷移禁止構文) | 遷移 invariant `trans { old(x) => ... }` を新設(grammar/model/bmc/runtime)。action 横断の2状態安全性を直接宣言でき、self-spec の sticky/不可逆性質を ghost なしで表現。BMC + induction step-case + replay で検査(DESIGN-trans.md) | **完了** |
| F25(代数的性質の表現力) | 言語の本質的限界。改修対象外 | 見送り |

## 実装適合の錨(モデル検証 → 実装検証)

当初 self-spec は **fslc の設計契約を記述したモデル**であり、`verify`/induction が証明したのは
**モデルの内部整合**だけだった。モデルと実コード(`src/fslc/cli.py`)の間にリンクが無く、
「実装がこの契約を守るか」は未検証 — fslc が保証するのは「書かれた仕様の内部整合」であって
「仕様が実態に忠実か」ではない、という本プロジェクトの核心ギャップが self-spec にも当てはまっていた。

`tests/test_self_conformance.py` でこのギャップを埋めた。多様な outcome を出す spec コーパスに
実 CLI のパイプライン(check → verify → induction)を走らせ:
1. 各 result とプロセス exit code が `exit_code()` の severity 表に一致(実 exit code を直接検査)、
2. `ProvedImpliesVerified` / `SuccessRequiresCheck` が実結果で成立、
3. 実結果列を `fslc_session` のアクション列へ写像し `fslc replay` が **conformant**(実 CLI の遷移が
   モデル状態機械に適合)、
4. 契約違反の手書きトレースが **nonconformant**(`verify_ok` 単独は `requires status==CheckOk` で
   reject = 錨に歯がある負の対照)。

これでメタ循環ドッグフーディングは「モデル検証」から「**実装適合検証**」に引き上がった。
範囲は核となる check→verify→induction パイプライン。verify 時の semantics エラーや
tool_fault(内部エラー)・各補助 subcommand は fslc_session に対応アクションが無く未錨(今後の拡張余地)。

## 再現

```bash
E=examples/self

# fslc_session / fslc_monitor は terminal { } 宣言済みなので --deadlock ignore は不要
./.venv/bin/python -m fslc check  $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl --engine induction
./.venv/bin/python -m fslc mutate $E/fslc_session.fsl

./.venv/bin/python -m fslc check  $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl --engine induction
./.venv/bin/python -m fslc mutate $E/fslc_monitor.fsl

./.venv/bin/python -m fslc check  $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl --engine induction
./.venv/bin/python -m fslc mutate $E/refinement_algebra.fsl
```
