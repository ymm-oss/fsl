# FSL — `--strict-tags` lint(トレーサビリティ突合)実装設計

動機: issue #5(ロードマップ #1 の類型7)。宣言タグ `"REQ-n: 原文"` は実装済みだが任意。
AI が要件を取りこぼしても(欠落)、NL にない制約を足しても(捏造)、現状は何も指摘されない。
requirements 方言が強制するのは requirement **ブロック内**の要素のみで、トップレベル要素は
タグなしになりうる(plain `spec` は規律ゼロ)。

## 1. CLI

`fslc check --strict-tags [--requirements ids.txt]`(verify にも同フラグ)。warning は
**ok / verified / proved の成功結果でのみ**付与(violated 時は出さない)。既定(フラグなし)の
出力はバイト単位で従来どおり。

## 2. 2方向の突合(`strict_tag_warnings(spec, requirement_ids=None)`)

1. **`untagged`**: `meta` を持たない user invariant / action / leadsTo / reachable
   (= NL 要件に根拠を持たない捏造候補)。`MODEL: …` / `ASSUME-n: …` を含め、何らかの
   meta があれば tagged(lint はプレフィックス判定をしない)。
2. **`unreferenced_requirement`**: 宣言済み要件 ID のうち、どの宣言タグ・acceptance ID・
   forbidden ID にも現れないもの(= 形式化し忘れの欠落候補)。
   - **宣言済み** = `--requirements ids.txt`(1行1 ID)∪ requirements 方言の requirement
     ブロック ID(`__requirement_ids` で自動収集 — **空の requirement ブロック**「宣言だけ
     して形式化し忘れ」も捕まる。展開後は痕跡が消えるため収集が必須)。
   - **参照済み** = 全要素の `meta.id` ∪ acceptance ID ∪ forbidden ID。

## 3. 生成要素の除外(必須)

方言が自動生成する要素はユーザーがタグを付けようがないため除外する。名前推測でなく
**明示マーカー**: 方言展開が `("__generated", [名前…])` を発行 → `spec["generated_names"]`。
- `tick`(time ブロック、`_expand_time`)、`_kpi_*`(business kpi)。
- `_deadline_*` は requirement meta 継承済み、business transition は自動タグ
  (`{id: 遷移名, text: "by Actor"}`)、branches 分割は requirement meta 継承 ⇒ 除外不要。
- time ブロックの無い仕様でユーザーが `tick` を書いた場合は通常どおり lint 対象
  (time あり時の同名はそもそも type エラー)。

## 4. 波及(検証エンジン・Z3 無改修)

- model.py: `strict_tag_warnings`(~40行、spec dict 走査のみ)+ `generated_names` /
  `requirement_ids` 集約。dialects.py: `__generated` / `__requirement_ids` 発行。
- cli.py: `--strict-tags` / `--requirements` を check / verify に配線。成功結果に
  `{kind:"untagged"|"unreferenced_requirement", element, name(表示名), loc, hint}` を追記。

## 5. テスト(tests/test_strict_tags.py)

種別ごとの untagged(表示名・loc)/ MODEL・ASSUME は対象外 / 生成要素除外
(cancel_flow が `--strict-tags` でクリーン、`tick`/`_kpi_*` 非警告)/ time なし
ユーザー tick は警告 / `--requirements` 未参照 / 空 requirement ブロック → unreferenced /
acceptance・forbidden ID は参照済み算入 / violated では非表示 / 既定は出力不変。

## 6. 将来課題 / 関連

`unknown_requirement_id`(幽霊要件・typo)と `--strict-tags error`(CI ゲート)は v2。
**存在レベル**の突合に限定 — タグだけの空形式化(**意味レベル**)は #6 mutate の要件
ストレスレポート(`empty_formalization`)が受け持つ。ロードマップ #1。
