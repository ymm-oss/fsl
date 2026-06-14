# 変更履歴 (Changelog)

本プロジェクトの変更履歴。形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/)、
バージョニングは [Semantic Versioning](https://semver.org/lang/ja/) に従う。
各バージョンは git のアノテーションタグ(`v1.0.x`)に対応する。

## [Unreleased]

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

[Unreleased]: https://github.com/yumemi/fsl/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/yumemi/fsl/compare/v1.0.3...v1.1.0
[1.0.3]: https://github.com/yumemi/fsl/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/yumemi/fsl/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/yumemi/fsl/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/yumemi/fsl/releases/tag/v1.0.0
