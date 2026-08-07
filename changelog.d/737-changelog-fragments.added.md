Added (#737): checked-in `changelog.d/` fragments for `CHANGELOG.md`'s
`[Unreleased]` entries, closing the shared-edit merge-conflict spike's
GO-C1 decision (`docs/DESIGN-changelog-fragments.md`). `tools/aggregate_changelog.sh`
(stdlib-only bash, matching the `merge readiness / automation contracts`
lane's dependency contract) implements all six fail-closed controls the
record requires, each proven with both an accepting and a calibrated
rejecting fixture executed in `selftest`: nonconforming fragment name,
duplicate (id, category), nondeterministic or nonconforming order (both the
numeric-id and the category-order sort components, each pinned against a
golden a lexicographic sham fails), unaggregated-at-release plus
bidirectional direct-edit-forbidden (with the single remaining release
exclusion proven not to let a bullet silently drop under cover of a release
move), and aggregation conservation (calibrated against both a
fragment-dropping sham and a first-line-only-truncating sham). The existing
620-line, 27-bullet `[Unreleased]` body is left untouched rather than
converted: 16 of its bullets carry no `(#NNN)` and therefore no id a
fragment name could carry, so control 6 would reject them; it moves under a
version heading the ordinary way at the next release
(`docs/RELEASE.md` step 7), in the same step that aggregates this
directory's fragments. Every migration site moves together: `AGENTS.md` and
`CONTRIBUTING.md`'s coupled-change and commit guidance,
`.claude/hooks/changelog_reminder.py`, `.claude/agents/fsl-coupled-change-reviewer.md`,
`.claude/skills/add-language-feature/SKILL.md`, `.claude/skills/release/SKILL.md`,
`tools/check-product-gate-scope.sh`'s `changelog.d/` exemption prefix (with
the `changelog.dx/y` near-miss still classifying as product),
`docs/DESIGN-ci.md`'s exemption amendment, `.github/workflows/release.yml`'s
stale-fragment guard, and `.github/workflows/merge-readiness.yml`'s new
pull-request-level fragment/direct-edit check. This entry is itself the
first fragment.
