# FSL Example Gallery

このギャラリーは、FSL を「小さく読んで、大きく試す」ための教材です。各 `.fsl`
には `expected-command` / `expected-result` / `expected-kind` をコメントで書き、
`tests/test_gallery.py` が実際の `fslc` JSON と照合します。

## valid: 正しい仕様

| サイズ | ファイル | トピック | ひとこと | コマンド |
|---|---|---|---|---|
| tiny | `valid/tiny_turnstile.fsl` | ターンスタイル | `coin` と `push` の最小状態機械 | `fslc verify ... --engine induction` |
| tiny | `valid/tiny_traffic_light.fsl` | 信号機 | `enum` とネストした `if` の基本 | `fslc verify ... --engine induction` |
| tiny | `valid/tiny_bounded_counter.fsl` | 上限付きカウンタ | 有界型と `requires` で境界を守る | `fslc verify ... --engine induction` |
| small | `valid/small_vending_machine.fsl` | 自動販売機 | `Map`、`Option`、在庫減算、`ensures` | `fslc verify --depth 6 --deadlock ignore` |
| small | `valid/small_elevator.fsl` | 1台エレベータ | 階・ドア・目標階の整合 | `fslc verify ... --engine induction` |
| small | `valid/small_tcp_handshake.fsl` | TCP風ハンドシェイク | `fair action` と `leadsTo` の入口 | `fslc verify --depth 6 --deadlock ignore` |
| medium | `valid/medium_dining_philosophers_deadlock_demo.fsl` | 哲学者 | デッドロック形の状態を reachable で観察 | `fslc verify --depth 6 --deadlock warn` |
| medium | `valid/medium_two_phase_commit.fsl` | 2PC | 投票と commit safety | `fslc verify ... --engine induction` |
| large | `valid/large_order_workflow.fsl` | 注文ワークフロー | 注文、配送、返品、台帳 invariant | `fslc verify --depth 8 --deadlock ignore` |

## errors: 壊れた例から学ぶ

壊れた仕様は、FSL の診断を読むための教材です。`result` は大きな分類、
`kind` / `violation_kind` は原因の分類です。最初は JSON の
`message`、`hint`、`trace`、`invariant` だけを読むと十分です。

| kind | ファイル | 出力の見え方 |
|---|---|---|
| `parse` | `errors/parse_missing_expression.fsl` | `{"result":"error","kind":"parse","expected":"one of: ..."}` |
| `type` | `errors/type_option_some_equality.fsl` | `Option == and != are only defined against none` |
| `type` | `errors/type_undeclared_type.fsl` | `unknown type 'UserId'` |
| `type` | `errors/type_struct_set_field.fsl` | `struct field ... has non-scalar type` |
| `semantics` | `errors/semantics_duplicate_assignment.fsl` | `double assignment to 'x' on the same execution path` |
| `vacuous` | `errors/vacuous_contradictory_init.fsl` | `init constraints are unsatisfiable` |
| `invariant` | `errors/violated_invariant_counter.fsl` | `{"result":"violated","violation_kind":"invariant"}` |
| `type_bound` | `errors/violated_type_bound_missing_guard.fsl` | `_bounds_stock` が破れる |
| `ensures` | `errors/violated_ensures_wrong_postcondition.fsl` | action 名が `invariant` 欄に出る |
| `partial_op` | `errors/violated_partial_op_unchecked_pop.fsl` | `guard the action with requires q.size() > 0` |
| `leadsTo` | `errors/violated_leads_to_starvation.fsl` | loop trace と公平性の hint が出る |
| `deadlock` | `errors/violated_deadlock_terminal.fsl` | `--deadlock error` で `violation_kind: deadlock` |
| `refinement_failed` | `errors/refinement_failed_map.fsl` | 期待は `abs_requires_failed`。現状は DOGFOOD-6 の bug candidate |
| `acceptance` | `errors/error_acceptance_false_expect.fsl` | false `expect` の失敗状態が返る |

詳細な実 JSON 抜粋は `errors/README.md` にあります。

## adversarial: 検証器をだます狙いの例

`adversarial/` は、人間には結果が明らかだが検証器の境界を突く例です。深い
`if`、満杯 `Seq.push`、空 `Seq.head`、`Option` + `struct` + `Set` + `Seq`、
量化境界、refinement 写像境界、二重代入の配置、同時点で満たされる `leadsTo`
を置いています。

`adversarial/refine_mapping_boundary_map.fsl` は「完全展開のデッドロック →
空虚 `refines`」バグ(`docs/DOGFOOD-6.md`)の回帰例。増分プレフィックス展開で
解消済みで、現在は `refinement_failed/abs_state_mismatch`(jump 後 bump の
更新結果 n=1 と α(n)=2 の不一致を境界検査より先に検出)を正しく返す。
同根の残存ケース(違反遷移が**一部分岐だけ**終端に至る場合)は、各プレフィックスを
step t までの制約だけで検査する修正で閉じた(`examples/refinement_liveness/`
の `design_bypasses_control` が回帰例)。

## 壊れた例の読み方

1. ファイル先頭の `expected-*` コメントで、何を期待しているかを確認します。
2. コメントのコマンドを実行し、JSON の `result` と `kind` / `violation_kind` を見ます。
3. `trace` がある場合は最後の `action` と `changes` から、何が壊したかを読みます。
4. 期待と実出力が違う場合、仕様を観測結果に合わせて直さず、まず
   `docs/DOGFOOD-6.md` に bug candidate として記録します。
