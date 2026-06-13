# FSL — `fslc mutate`(仕様ミューテーション)実装設計

動機: issue #6(ロードマップ #1 の類型4/7)。invariant が弱すぎる・足りない仕様は
verified のまま沈黙する(過小制約)。「性質群がモデル挙動をどれだけ拘束しているか」を
測る機構がなかった。タグがあっても「その形式化が実際に何かを拘束しているか」(意味的
トレーサビリティ)は #5 の存在チェックでは見えない。DOGFOOD-7 で fslc 自身に手で行った
mutation proof の製品化。

## 1. CLI

`fslc mutate <f> [--depth K=8] [--by-requirement] [--max-mutants N=200]`。
出力 `result:"mutated"`、**exit 0 固定**(scenarios/testgen 同族の生成系。survivor は
失敗でなくレビュー用データ。`--fail-on-survivors` は将来)。

## 2. 変異は spec dict でなく **方言展開後のカーネル AST**

`parse_src` が返すカーネル AST `("spec", name, items)`(compose/requirements/business は
展開済み)を変異し、**ミュータントごとに `build_spec` を再実行**してから検査する。理由:
1. **型境界 ±1 変異は `build_spec` が生成する `_bounds_*` invariant の再生成を要する**
   — spec dict 直接変異では stale になり変異が効かない。
2. `phys_vars` 等の派生整合を build_spec に任せられる。
3. 方言を一様に扱える。文法・検証エンジンには触れない。

### 変異オペレータ(決定的列挙、乱数なし)

| op | 模擬する誤り | AST 操作 |
|---|---|---|
| requires 除去 | ガード漏れ | body から `("requires", …)` 削除 |
| requires 否定 | 条件取り違え | `("not", e)` で包む |
| 代入除去 | 更新漏れ | `("assign", …)` 削除 |
| enum 入替 | 遷移先取り違え | `("var", member)` を同 enum の別メンバへ |
| 整数/境界 ±1 | off-by-one | `("num", n)`±1、`("type", n, lo, hi)` の lo/hi ±1 |
| then/else 交換 | 分岐取り違え | 両分岐非空の `if` を swap |
| fair 除去 | leadsTo 公平性前提の欠落 | action の fair True→False |

## 3. kill オラクルと baseline ゲート

各ミュータント = mutated AST → `build_spec` → **`verify`(BMC, depth K) + acceptance/
forbidden 再生 + implements refine**。いずれかが violated/reachable_failed/error/
refinement_failed を返すか build_spec が FslError → **killed**(killer 記録)。全て clean →
**SURVIVED**。induction は使わない(`unknown_cti` は kill 判定が曖昧かつ遅い)。
**baseline ゲート**: 変異前が verified でなければ refuse(buggy 仕様では全 mutant が自明に
殺され無意味)。

## 4. `--by-requirement`(要件ストレスレポート)— 逆向きの定義

「invariant を外して何が壊れるか」は**安全性では原理的に空回りする**: invariant を
削除すると検査対象が減るだけで違反は生じない(単調性)。invariant は「**挙動の変異を
捕まえる**」ことでしか働きを示せない。したがって正しい機械化は逆向き: kill オラクルが
各ミュータントの killer を記録 → `killed_by` の requirement タグで集計。**どの挙動変異も
殺さなかった requirement = 空形式化**として `empty_formalization` 警告。v1 は first-killer
記録で「観測下限」と明記(sole-killer 冗長分析は将来)。

## 5. 出力 / 波及

```json
{"result":"mutated","spec":"…","depth":8,"baseline":"verified",
 "summary":{"total":N,"killed":K,"survived":S},
 "mutants":[{"op","loc","target","status","killed_by","requirement"}],
 "by_requirement":{"REQ-7":{"kills":0,"warning":"empty_formalization"}},
 "notes":["mutant cap 200 reached: 37 dropped"]}
```

新規 `src/fslc/mutate.py`。決定的列挙 + `--max-mutants` 打ち切りは `notes` に明示
(silent cap 禁止)。coverage-false アクションの survivor は「baseline で死んでいる」と注記、
同値ミュータントはレビューキュー(ハード失敗にしない)。**検証エンジン無改修**。

## 6. テスト / 関連

tests/test_mutate.py: cart_v1 ガード除去 → `_bounds_stock` kill / 型境界+1 kill(AST 変異+
rebuild の証拠)/ 間引き invariant survivor / `empty_formalization` / baseline 拒否 /
coverage-false 注記 / 打ち切り注記 / コーパス安定性 / exit 0。#5 strict-tags の意味レベル
拡張であり、#7 explain の反実仮想はこの kill を invariant 別に物語化したもの。ロードマップ #1。
