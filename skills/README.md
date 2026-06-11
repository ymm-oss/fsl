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

### インストール

**Claude Code(このリポジトリ内)**: `.claude/skills/fsl` が `skills/fsl` への
シンボリックリンクになっているため追加作業は不要。

**他プロジェクトで使う**: `skills/fsl/` を対象プロジェクトの `.claude/skills/`、
またはユーザ全体の `~/.claude/skills/` にコピーする。`gh` のスキル拡張を使う
場合は本ディレクトリ(`skills/`)を配布元として指定する。

検証器 `fslc` 本体は別途必要(リポジトリルートで `pip install -e .`。
依存は lark と z3-solver のみ)。
