# FSL — 空虚性検査(vacuity)実装設計

動機: issue #4(ロードマップ #1 の類型5)。次の仕様はいずれも verified になるが
何も検査していない: 前件が到達不能な含意 invariant(`P => Q` で P が起きない)、
トリガが到達不能な leadsTo(`P ~> Q` で P が起きない)、到達状態で常に真の requires 句
(死に飾り)。空虚な verified は過小制約と並ぶ最大の見逃し源。従来 `kind:"vacuous"` は
init 充足不能のみを指していた。

## 1. CLI

`fslc verify <f> [--vacuity warn|error|ignore]`(既定 warn、deadlock と同形)。
- `warn`: warnings に載せる(結果は verified / proved のまま)
- `error`: 検出時 `{"result":"error","kind":<検出種>,"findings":[…]}` → **exit 2**
  (反例トレースが無いので violated/exit 1 でなく、init 充足不能の `vacuous` と同族)
- `ignore`: 検査スキップ

## 2. 3つの検査(verified / proved 経路でのみ)

1. **`vacuous_implication`**: `forall*` 直下の単一 `=>` を持つ **user invariant** の
   前件が depth K 内で sat にならない。前件の存在閉包は AST を `("exists", binder, A)` で
   包んで既存 `eval_expr` に渡す(新評価器なし)。暗黙の `_bounds_*` は対象外
   (Seq live-prefix は含意形であり自動生成物への警告はノイズ)。
2. **`vacuous_leadsto`**: leadsTo のトリガ P を同様に存在閉包で検査。
3. **`always_true_requires`**: 各 requires 句 j について **先行句の文脈付き**で
   `sat(句1..j-1 ∧ ¬句j)` が全到達状態×全インスタンスで unsat なら警告。文脈付きに
   する理由は (a) Monitor 短絡(BUG-020)との整合 (b) 冗長句(`st==Paid` の後の
   `st!=Cancelled`)も検出 (c) let 内 partial op の Z3 全域符号化による spurious sat は
   「警告を出さない」方向にしか働かず安全。**coverage false のアクション**(既に
   never-enabled 警告あり)と **compose 同期アクション**は対象外。

### compose 同期アクションを除外する理由(重要)

`deposit_audited = bank.submit_deposit || audit.deposit` は bank と audit の双方から
`requires a > 0` を継承する。これは「各成分が自分の契約を自衛する」設計どおりの複製で、
除去可能な冗長ではない(audit_log 単体では当然必要)。名前推測でなく compose 展開が
立てる sync マーカー(action dict の `sync`)で除外。各句は成分 spec 単体の verify で
正しい文脈の検査を受けるため検出損失はゼロ。

## 3. 波及(検証エンジン本体は無改修、`_bmc_explore` への相乗りのみ)

- bmc.py: `pending_reachables` ループと同型の `pending_vacuity`(含意前件 + leadsTo
  トリガ)を単一展開に相乗り。requires 恒真は coverage ループに「先行句 ∧ ¬句」の
  sat を追加。文脈付き候補は「型空間上で先行句から論理的に含意される句」のみに
  事前フィルタ(`_requires_clause_locally_implied`)し、容量ガード系の有界偽陽性を排除。
- 出力: warnings に `{kind, name(表示名), loc, requirement, message, hint}`。
  prove() は base verify から warning が透過。scenarios は `vacuity_mode="ignore"`。
- hint は修復を誤らせない: 恒真 requires は「句を消せ」でなく「モデルの不足か冗長かを
  判断」(深い depth や induction では効く可能性)。

## 4. テスト(tests/test_vacuity.py)

警告3種(表示名・loc・requirement)/ forall 包み / 文脈付き冗長句 / 抑制2種
(coverage false・sync)/ violated 経路で非表示 / induction 透過 / error(exit 2)・ignore /
**コーパス偽陽性ゼロ門番**(specs/ + examples/ + gallery/valid 一括)。gallery
`vacuous_implication_warning.fsl`(`--vacuity error`)。

## 5. 関連

過小制約・空形式化の検出で #3 forbidden・#6 mutate と相補。有界検査なので警告文言は
「within depth K」を明示。ロードマップ #1。
