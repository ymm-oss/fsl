# 変更履歴 (Changelog)

本プロジェクトの変更履歴。形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/)、
バージョニングは [Semantic Versioning](https://semver.org/lang/ja/) に従う。
各バージョンは git のアノテーションタグ(`v1.0.x`)に対応する。

## [Unreleased]

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
