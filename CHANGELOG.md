# 変更履歴 (Changelog)

本プロジェクトの変更履歴。形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/)、
バージョニングは [Semantic Versioning](https://semver.org/lang/ja/) に従う。
各バージョンは git のアノテーションタグ(`v1.0.x`)に対応する。

## [Unreleased]

## [1.2.5] - 2026-06-15

テーマ: **監査トリアージ(issue #12) — compose 展開バッチ(Batch D)**。コンポーネントの
`const` をレンジ/binder/param/sync 引数で参照したとき、展開時に const は `alias__` で
プレフィクスされるのに式中の参照が書き換えられず未解決になる問題を修正。

### 修正
- **コンポーネントの `type T = 0..MAX` の domain 境界式が書き換えられず未解決**になる
  問題を修正(`compose.py` `_prefix_component_items`)。展開後 `alias__MAX` と不整合だった。
- **`binder_range`(`forall k in 0..MAX`)・`param_range`(`action f(n in 0..MAX)`)の
  lo/hi 式が書き換えられない**問題を修正。`_rewrite_binder`/`_rewrite_params` が
  コンポーネントの const 集合を受け取り、レンジ境界の const 参照をプレフィクスする。
- **sync 引数式中の `alias.x` 参照が書き換えられず展開後 AST に残る**問題を修正
  (`_expand_sync_action`)。代入前に `_rewrite_expr` で物理名へ解決する。
- 「同期引数の型不一致」の静的検査(DESIGN-compose §2)は compose 層に型推論が無いため
  今回は未実装とし、`build_spec` 後段の型検査に委ねる(arity 検査は従来どおり実施)。

## [1.2.4] - 2026-06-15

テーマ: **監査トリアージ(issue #12) — acceptance/forbidden/mutate バッチ(Batch C)**。
acceptance/forbidden シナリオ再生とミューテーション集計の 6件 + 検証中に発見した
0引数ステップのパースバグ 1件を修正。

### 修正
- **acceptance/forbidden ステップの 0引数呼び出し(`noop()`)が `[None]` とパースされ
  arity mismatch になる**バグを修正(`grammar.py`)。`maybe_placeholders` 由来の None を
  除去(commit cca8627 の refinement 0引数写像と同系統)。既存例は常に引数付き呼び出し
  だったため未発覚だった。Batch C のテスト作成中に発見。
- **acceptance/forbidden の step 引数で `const` 参照が解決されず文字列のまま渡り
  `bad_call` になる**問題を修正(`acceptance.py`)。`_literal_value` が `spec["consts"]`
  を解決するよう変更(未定義 const は構造化エラー)。
- **`expect` 式が非 bool のとき `_EvalError` が伝播し `run_check` が envelope を返さず
  落ちる**問題を修正。expect 評価を捕捉し acceptance 失敗の構造化結果にする。
- **forbidden の setup/final ステップで未知アクション・arity 不一致が構造化失敗 dict を
  返さず例外送出**していた問題を修正。`failed_step` 付き結果を返し、kind も
  `forbidden_setup`/`forbidden` に分離(共有 `_err` の `kind="acceptance"` 固定を解消)。
- **`fslc mutate --by-requirement` の集計に acceptance/forbidden の id と kill が混入**
  していた問題を修正(`mutate.py`)。DESIGN-mutate §4 のとおり requirement ブロックの
  形式化のみを対象とし、AC/FB の id を除外(AC-2 等への `empty_formalization` 誤付与も解消)。

## [1.2.3] - 2026-06-15

テーマ: **監査トリアージ(issue #12) — typestate バッチ(Batch B)**。`fslc typestate` の
from-state 抽出漏れ 2件を修正。どちらも健全な遷移を `relational` と誤判定し、エンティティの
適用可否(applicability)を不当に `none` に落としていた。

### 修正
- **`requires` の合取式から from-state を抽出できず `relational` と誤判定**していた問題を
  修正(`typestate.py`)。`requires e.st == A and q > 0` のような束縛で `and` ノードが
  未処理だったため、`e.st == A` の from-state が拾えなかった。`_enum_guard_states` /
  `_opt_guard_states` が `and` を `or` と同様に再帰処理するよう拡張。
- **`if` 条件中の from-state が抽出されず、分岐遷移が `relational` と誤判定**されていた
  問題を修正。`if light == Red { light = Green }` のような分岐で、囲み条件から各分岐の
  from-state を導出する(status-only な `else` は補集合として扱う)。これにより
  `tiny_traffic_light.tick` が `branching`(from-state 付き)として正しく分類され、
  applicability が `full` になる。`typestate:325`(branching を `_emit_ts` に出す)も
  これに伴い解消。

## [1.2.2] - 2026-06-15

テーマ: **自動コード監査(issue #12)のトリアージ着手 — 健全性バッチ(Batch A)**。
44件(未検証42 + 検証済み未修正2)を 7並列の検証＋実機再現でトリアージし、健全性・
正しさに直結する 5件を修正。いずれも実 CLI 動作で確認済み。

### 修正
- **`Set<有界スカラ>` に暗黙の型境界 invariant が付かず、範囲外要素が見逃される**問題を
  修正(`model.py`/`bmc.py`/`runtime.py`)。`Set<Id>`(Id=0..3)に `s.add(99)` しても
  `verified` のままだった(偽の検証成功)。`set_bounds` AST ノードを導入し、全要素が
  要素型の範囲内であることを Z3 ForAll / 具体評価で検査する(明示初期化された集合に
  対する偽陽性は出ない)。
- **`Map<Int, 有界値>` の値境界 invariant が生成されず、範囲外の値が見逃される**問題を
  修正。`Map<Int, Qty>`(Qty=0..5)に `m[0] = 99` しても `verified` のままだった。
  `map_value_bounds` AST ノードを導入し、Int キー Map の実効ドメイン(`_map_domain` の
  既存規約 `0..max(consts)`)上で値型の境界を検査する。
- **`fslc explain` の `--max-mutants` が弱化探索の前に早期終了**していた問題を修正
  (`explain.py`)。打ち切りを `enumerate_mutants` 全体の index ではなく実際に処理した
  弱化ミュータント数で行うよう変更(反実仮想の取りこぼしを解消)。
- **invariant 評価中の `_PartialOp`(部分演算)が `step()` から例外として漏れる**問題を
  修正(`runtime.py`)。invariant 式が 0 除算・空 Seq の head 等を踏むと例外が伝播し、
  DESIGN-bridge §1.2「step() は常に結果 dict を返す」契約に反していた。`partial_op`
  違反として構造化結果を返す。
- **`fslc testgen` が `-o` 省略時に `NameError`(`parse` 未 import)で全壊**していた問題を
  修正(`testgen.py`)。`default_output_name` が未 import の `parse` を呼んでいた。
  `generate_test_file` と同じく `parse_src(src, base_dir)` を使い、compose 仕様の相対
  パス解決のため spec ファイルの親ディレクトリを base_dir として渡す。

## [1.2.1] - 2026-06-15

テーマ: **自動コード監査(composer-2.5)で検出した検証済みバグの修正**。検証器の健全性に
関わる 1 件を含む 3 件を修正(issue #13/#14/#15)。いずれも実 CLI 動作で確認済み。

### 修正
- **`leadsTo` 束縛の `where` 句が破棄され誤った `violated` を報告**していた問題を修正
  (issue #13)。`expand_leadsto_bindings`(`bmc.py`)が binder の `where` を捨てて全
  ドメイン値を列挙していたため、`forall p: T where p > 0` でも `p = 0` を別束縛として
  検査し、where を満たさない値で偽の反例を出しうる(検証器の健全性)。`where` を具体評価
  して列挙をフィルタするよう修正。あわせて `init_constraints` の `run_collect` が
  ネストした `forall` の `where` を無視していた過剰拘束(unsat)も修正。
- **欠落 spec ファイルが `kind:"internal"`/exit 3 で報告**されていた問題を修正
  (issue #14)。`run_check`/`run_scenarios`(`cli.py`)で `except FileNotFoundError`
  が `except Exception` より後にあり到達不能だった。順序を入れ替え、io エラーは
  LANGUAGE.md §7 どおり `kind:"io"`/exit 2 になるよう修正。
- **コンパイル時整数の除算 `/` が未実装**(`+ - *` のみ)だった問題を修正(issue #15)。
  DESIGN-v1 §3.1 の「四則」に合わせ `eval_const`(`model.py`)に除算を追加。意味論は
  ランタイム(`_euc_div`、ユークリッド除算)と一致させ、0 除算は `kind:"type"` の
  コンパイルエラーにする。`type K = 0..(MAX / 2)` のような range bound が通るように。

## [1.2.0] - 2026-06-14

テーマ: **AI 形式化の妥当性確認(validation)スイート**(roadmap #1 完了)。検査器が
保証する「仕様の内部整合」と「仕様が元の意図に忠実か」のギャップを埋める検出器群を
追加し、AI が書く仕様の誤り(過小制約・空虚・欠落/捏造・取違)に検出網を掛ける。
書く前の規律(形式化メモ・推奨プラクティス)はスキルに、効果は誤り注入ベンチで実測。

### 修正
- **refinement の 0引数 abstract アクション写像**(`action foo() -> bar()`)が
  `expects 0 arguments` の偽エラーで落ちていた問題を修正(`grammar.py` の
  `mapped_action_target`/`req_mapped_action_target` で `maybe_placeholders` 由来の
  None を除去)。既存仕様は 0引数 impl を全て `stutter` に写していたため未発覚。
  fsl-ui スパイク(#9)の副産物。

### 追加
- **fsl-ui スパイク**(#9): 画面遷移方言の検討。返品申請の画面フローを素の fsl で
  手書きし、verified + proved、かつ要件層への refine も成立することを確認
  (`examples/ui_spike/`、所見は `docs/DESIGN-ui.md`)。カーネルの意味論変更なしに
  画面フローを表現でき、方言は AST 糖衣として成立する見込み(go/no-go は DESIGN-ui)。
- **`fslc explain`(issue #7)**を追加。仕様の骨格(state/action/requires/writes/
  properties/暗黙の型境界・partial_op 検査)を loc ベースの原文切り出しと構造走査で
  JSON 化し、user invariant ごとに requires/代入/fair 除去の反実仮想トレースを
  `mutate`/`verify` 機構の再利用で生成する。反実仮想が depth K で見つからない
  invariant はエラーにせず明示し、reachable/scenarios witness も段階的な記述へ整形する。
- **`fslc typestate`(設計 spec → typestate / 幽霊型の適用可否判定 + TS 雛形)**を追加。
  `(エンティティ, action)` ごとに、from-state が**エンティティ自身の状態に対する局所
  ガード**(`requires e.status == S`)なら `derivable`、`if` 内のデータ依存 to-state なら
  `branching`、**状態を代入するのに局所ガードが無い**(前提が queue 等の外部構造に住む)
  なら `relational` と判定する。`relational`/`branching` は型に出さず、理由(diagnostics)
  と action の要件 ID(business 層の `transition ... by <actor>` 等)を添えて runtime/
  検証義務として残す。エンティティ単位の `applicability` は全遷移が `derivable`/
  `branching` のときだけ `full`(理解できなかった遷移を取りこぼして full を名乗らない)。
  対応する状態機械は **enum 値の struct フィールド・enum 値の state 変数(business
  `process`/stages)・`Option<_>` スロット(none/some ≈ Empty/Filled)**の3形。
  `--ts` で導出可能エンティティの TypeScript だけを stdout に出す。出力は他コマンドと
  同じ JSON エンベロープ(`result:"typestate"`、exit 0)。
- **`fslc mutate`**(issue #6)を追加。方言展開後の kernel AST に決定的な単一変異
  (requires 削除/否定、代入削除、enum 入替、整数/型境界 ±1、then/else 交換、
  fair 削除)を加え、mutant ごとに `build_spec` し直して BMC/acceptance/forbidden/
  refinement で殺せるかを JSON 報告する。baseline が clean でない仕様は変異せず
  baseline 結果を返す。`--by-requirement` は殺した性質の requirement tag で集計し、
  ゼロ kill を `empty_formalization` として警告する。survivor はレビュー用データで、
  `mutate` の exit は常に 0。
- **`--strict-tags` lint**(issue #5)を `fslc check` / `fslc verify` に追加。
  ok/verified/proved の成功結果でのみ、タグなし action/invariant/reachable/leadsTo と
  未参照要件 ID(`--requirements ids.txt` および requirements 方言の `requirement`
  ブロック)を warning として出力する。方言生成の `tick` / `_kpi_*` は明示マーカーで
  除外し、既定(フラグなし)の出力は従来どおり。
- **vacuity checks**(issue #4)を `fslc verify` に追加。verified/proved 経路で
  `vacuous_implication`(含意 invariant の不到達前件)、`vacuous_leadsto`
  (leadsTo トリガ不到達)、`always_true_requires`(先行 requires 文脈下で常に真の
  requires 句)を warning として出力する。`--vacuity warn|error|ignore`
  (既定 warn)を追加し、error は `result:"error"` / exit 2 にする。
  coverage false のアクションと compose 同期アクションは `always_true_requires`
  の対象外(同期アクションの句は成分からの継承複製 — 成分間の同一ガードは
  各成分が契約を自衛する設計どおりで、成分 spec 単体の verify で検査される)。
- **`forbidden`(負の受け入れ基準 / must-forbid)**(issue #3)を requirements 方言に追加。
  `forbidden FB-1 "原文" { <手順> expect rejected }` は「拒否されるべき操作列」を書き、
  前提ステップは全て ok・**最後のステップが拒否**される(not-enabled か invariant/
  type_bound/partial_op/ensures 違反)ことを check 時に具象 Monitor で検証する。受理
  されたら `kind: "forbidden"`(安全性 invariant では沈黙する過小制約=ガード漏れの
  検出)、前提が未 enabled なら `kind: "forbidden_setup"`。scenarios に `forbidden_<ID>`
  を出力(`rejected_by` 付き)し testgen のネガティブテストへ流れる。検証エンジン・
  Monitor は無改修。

### ドキュメント / ワークフロー
- **AI形式化の妥当性確認(validation)ワークフロー**(issue #2)をスキルに追加。
  検証器が保証する「内部整合」と、元の意図への忠実性のギャップを埋める規律:
  書く前の**形式化メモ**(チャット出力、仮定のみ `.fsl` の `// ASSUME-n:`
  コメント/タグへ畳む)、**自然言語→構文の逆引き表**、修復時に仮定台帳へ
  追記する規律、**推奨プラクティス**(正例ペア・1要件1宣言・ドメインサイジング・
  高リスク仕様の交差検証 — すべて任意。重い手順は義務化しない)。
- 上記ワークフローの実走記録 `docs/DOGFOOD-9.md` と例
  `examples/validation/order_refund.fsl`(proved)を追加。正例ペア
  `reachable FullyRefunded` が「安全性 invariant は通るのに返金経路が死ぬ」初版を
  `reachable_failed` で検出する様子を実証。
- `docs/README.md` の DOGFOOD 索引を 1-9 に補完(6/7/8 の未掲載も解消)。

## [1.1.0] - 2026-06-12

### 追加
- **整数除算 `/` と剰余 `%`**(算術に追加、`*` と同位)。ゼロ除算は両評価器で
  全域的に 0 と定義(Z3 符号化も明示固定)し、アクション文脈では除数 != 0 を
  暗黙の `partial_op` として検査。負数は Euclidean(`0 <= a%b < |b|`)。
  → 2次元データを単一キーに平坦化したときの軸復元(`c / SLOTS` 等)が書ける。

### ドキュメント / イディオム
- **2次元データの平坦化イディオム**(Map のネスト不可 → 積ドメイン型1本+`/` `%`)を
  LANGUAGE.md・スキルに追記。
- **離散時刻 SLA の明文化**: `time`/`deadline` の配置規則、`age` の意味論、
  `urgent` = 時間凍結という意味。特に「常時 enabled なアクションを urgent に
  すると deadline が空虚に成立する罠」と、正しい **deadline-urgency パターン**
  (期限到達時のみ enabled なガード付きアクションだけを urgent に)を明記。
  公式例 `examples/nfr/support_sla.fsl` を追加(proved)。
- 盲検可記述性テスト(`docs/DOGFOOD-8.md`、n=3): スキル単体で別エージェントが
  新規ドメインを proved にできることを外部検証。上記ドキュメント改善はこの
  テストが surface したギャップに対応するもの。

## [1.0.3] - 2026-06-12

### 追加
- `CHANGELOG.md`(本ファイル)を追加。リリースごとの変更を一望できるようにした。

## [1.0.2] - 2026-06-12

### 修正
- **BUG-020**: `Monitor.enabled()` が、ガード付きの部分操作を含む `let`
  (例: `requires queue.size() > 0` の後の `let j = queue.head()`)で
  `_PartialOp` 例外を送出していた問題を修正。`requires` を先に評価して短絡し、
  ガードを満たさないアクションは単に enabled でないものとして扱う。`step()`
  実行時の `partial_op` 検出は従来どおり維持。`fslc verify` は元々正しく、
  影響は runtime Monitor / replay / testgen。

### 品質保証(テスト)
- Z3 非依存の**総当たり正解オラクル**(`tests/oracle.py`)を追加。Monitor の
  具象意味論で有界到達可能状態を BFS 全探索し、invariant 違反・到達性・
  デッドロックの真値を BMC 判定と照合(偽陰性=見逃しを検出)。
- 反例トレース・witness の**具象再生健全性**、**refinement 独立オラクル**、
  **メタモルフィック**(ガード除去→違反化、リネーム不変、深さ単調性)、
  **ロバストネス**(JSON 直列化・exit code 整合・内部名非漏出)の各テスト群を追加。
- テスト総数 208 → 301(+69 skip、約260秒)。

### ドキュメント
- README をテスト数・docs 一覧・examples ツリー等で現状に更新。

## [1.0.1] - 2026-06-12

### 修正
- **refine のソンドネスバグ**: impl が探索深さの手前でデッドロックすると、
  完全展開が充足不能になり全ての違反検査が見逃され、誤って `refines` を
  返していた問題を修正。到達可能な各プレフィックスを増分検査し、unsat に
  なった深さで打ち切る方式に変更(統制違反の見逃しを解消)。

### 追加
- `fslc version` / `fslc --version` / `-V`(バージョン表示)。

## [1.0.0] - 2026-06-11

実質的な初版。FSL(AI ネイティブ形式仕様言語)と検証器 `fslc`。

### コア検証
- **BMC**(有界モデル検査、最短反例)/ **k 帰納法**(`--engine induction`、
  無限深度 `proved` と `unknown_cti`→補助 invariant ループ)。
- `invariant` / `reachable`(witness)/ `leadsTo` + 弱公平性(`fair`、
  ラッソ反例)。自動チェック: 型境界・部分操作(`partial_op`)・
  action coverage(unsat core 診断)・デッドロック。
- 型システム: ドメイン型・enum・struct(`Option<スカラ>` フィールド可)・
  `Option<T>` / `Map` / `Set` / `Seq<T, N>`。

### 実装橋・合成・詳細化
- `fslc scenarios`(統合テスト雛形)、`fslc replay`(ログ適合性)、
  `fslc testgen`(pytest 適合性雛形)、`fslc.runtime.Monitor`(具象実行)。
- `fslc refine`(refinement mapping による忠実性検査、写像式の条件式対応)。
- `compose`(名前空間付き合成・同期アクション・`internal`)。

### 3層方言とトレーサビリティ
- `business`(コンサル)/ `requirements`(要件、`branches`・`acceptance`・
  `implements`)/ `spec`(設計)を refinement で連鎖。
- 宣言タグ `"ID: 原文"` で要件 ID を全診断(反例・CTI・coverage・scenarios)へ透過。

### 非機能要件
- 権限・監査・容量・信頼性の挙動はイディオムで、SLA/タイムアウトは
  離散時刻(`time` / `urgent` / `age` / `deadline`)で検査。

### 配布・利用
- 事例ギャラリー(正例 / 不正例カタログ / adversarial)、PM・コンサル・3役統合の
  example、素の Python 実装への適合テスト例。
- ワンライナーインストーラ(ZIP ダウンロード対応)、AI エージェント向け Agent Skill。

[Unreleased]: https://github.com/yumemi/fsl/compare/v1.2.5...HEAD
[1.2.5]: https://github.com/yumemi/fsl/compare/v1.2.4...v1.2.5
[1.2.4]: https://github.com/yumemi/fsl/compare/v1.2.3...v1.2.4
[1.2.3]: https://github.com/yumemi/fsl/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/yumemi/fsl/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/yumemi/fsl/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/yumemi/fsl/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/yumemi/fsl/compare/v1.0.3...v1.1.0
[1.0.3]: https://github.com/yumemi/fsl/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/yumemi/fsl/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/yumemi/fsl/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/yumemi/fsl/releases/tag/v1.0.0
