# skills/

配布用の Agent Skill 置き場。GitHub 上から見つけやすいよう、また
`gh` のスキル拡張や手動コピーで配布できるよう、リポジトリ直下に置いている。

## fsl

[`fsl/`](fsl/) — FSL(本リポジトリの形式仕様言語)を AI エージェントに
教えるスキル。FSL は学習データに存在しない言語なので、エージェントが仕様を
書くにはこのスキルで言語仕様と修復プロトコルを文脈に供給する必要がある。

- [`fsl/SKILL.md`](fsl/SKILL.md) — ワークフロー、結果→次の一手の修復プロトコル表、
  最小構文、構造的に守るべき規則(スキル起動時に読まれる本体)
- [`fsl/reference.md`](fsl/reference.md) — 凝縮版の完全言語リファレンスカード
  (compose / refinement / 全式カタログ / イディオム集)

## fsl-design-review

[`fsl-design-review/`](fsl-design-review/) — FSL を使った設計検討・設計レビューの
手続きスキル。設計案・変種・拡張・変更を「凍結した契約(抽象 spec)への
refinement」として記述し、`fslc refine` の結果を設計原則(SOLID の LSP/OCP、
契約による設計など)の語彙で報告する。手続きが背骨で、原則は各ステップの
判断レンズとして登場する。FSL 構文は fsl スキルに委譲(併用前提)。

- [`fsl-design-review/SKILL.md`](fsl-design-review/SKILL.md) — 5ステップの手続き、
  検査結果→設計判断の翻訳表、原則↔機構の対応表、抽象層の規律

### インストール

**Claude Code(このリポジトリ内)**: `.claude/skills/` 配下に各スキルへの
シンボリックリンクがあるため追加作業は不要。

**他プロジェクトで使う**: `skills/fsl/`(および必要なら `skills/fsl-design-review/`)
を対象プロジェクトの `.claude/skills/`、またはユーザ全体の `~/.claude/skills/` に
コピーする。`gh` のスキル拡張を使う場合は本ディレクトリ(`skills/`)を配布元として
指定する。

検証器 `fslc` 本体は別途必要(リポジトリルートで `pip install -e .`。
依存は lark と z3-solver のみ)。
