# FSL — `fslc typestate`(状態機械→幽霊型の適用可否判定)実装設計

動機: 設計 spec の状態機械を、ホスト言語(TypeScript 等)の **typestate(幽霊型)** へ
どこまで健全に写せるかを判定し、写せる範囲だけ型雛形を出す。判定そのものが成果物 —
「どこは型で守れて、どこは runtime/検証義務として残るか」を仕様から機械的に切り分ける。

## 1. CLI / 出力

`fslc typestate <f> [--ts]` → `result:"typestate"`、exit 0。`--ts` は導出可能エンティティの
TypeScript だけを stdout に出す。出力は他コマンドと同じ JSON エンベロープ。

## 2. 判定: `(エンティティ, action)` ごとの3分類

- **`derivable`** — from-state が**エンティティ自身の status フィールドに対する局所ガード**
  (`requires e.status == S`)で、to-state が局所代入。runtime ガードが健全に
  コンパイル時の型になる。
- **`branching`** — to-state が `if` 内でのみ代入される(データ依存)。型に出すが、
  実装は網羅性の証明義務を負う(flagged)。
- **`relational`** — status を代入するのに**同一エンティティ上の局所ガードが無い**。
  前提が外部構造(queue・別エンティティ)に住むため幽霊タグでは運べない。型に出さず、
  理由(diagnostics)と action の要件 ID(business 層の `transition … by <actor>` 等)を
  添えて残す。

## 3. 対応する状態機械の3形

1. **enum 値の struct フィールド**(`struct Order { status: St }`)。
2. **enum 値の state 変数**(business `process`/stages 展開後)。
3. **`Option<_>` スロット**(none/some ≈ Empty/Filled)。

## 4. applicability(エンティティ単位)

全遷移が `derivable`(または `branching`)のときだけ `full`。**理解できなかった遷移を
取りこぼして full を名乗らない**(健全側に倒す)。一部のみなら `partial`、皆無なら `none`。

## 5. 波及 / 実装

- 新規 `src/fslc/typestate.py`。spec dict の走査のみで**検証エンジン・Z3 無改修**。
  enum 形は `_enum_guard_states` / `_enum_assignments` / `_enum_is_status_only`、
  Option 形は `_opt_*` の対で判定し、`_classify` が3分類とエンティティ applicability を出す。
  TS 識別子の予約語衝突は `RESERVED_TS` で回避。
- cli.py: `typestate` サブコマンド(`run_typestate`)。`exit_code` の成功集合に
  `"typestate"` を追加。

## 6. テスト / 関連

tests/test_typestate.py。出自は別 PR(#10 phantom-gen-experiment)。形式仕様を実装側の
型システムへ橋渡しする点で DESIGN-bridge(testgen / Monitor)と同系統 — bridge が
「振る舞いの適合テスト」を出すのに対し、typestate は「状態前提の**型**への昇格可否」を
判定する。
