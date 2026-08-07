Added (#737): checked-in `changelog.d/` fragments for `CHANGELOG.md`'s
`[Unreleased]` entries, closing the shared-edit merge-conflict spike's
GO-C1 decision (`docs/DESIGN-changelog-fragments.md`). `tools/aggregate_changelog.sh`
(stdlib-only bash, matching the `merge readiness / automation contracts`
lane's dependency contract) implements all six fail-closed controls the
record requires, each proven with both an accepting and a calibrated
rejecting fixture executed in `selftest`: nonconforming fragment name (now
also rejecting a conforming-looking name hidden anywhere
`list_fragment_files` will not enumerate it -- a subdirectory or a
top-level dotfile, both of which earlier releases of this fragment left
invisible to every check but control 1's own shallow path match), duplicate (id, category),
nondeterministic or nonconforming order (both the numeric-id and the
category-order sort components, each pinned against a golden a
lexicographic sham fails), unaggregated-at-release plus bidirectional
direct-edit-forbidden (diffed against the merge base, matching this
repository's own three-dot convention, so a release landing on `main` does
not misname every open pull request's untouched `[Unreleased]` body as a
direct edit; the single remaining release exclusion is proven not to let a
bullet silently drop under cover of a release move), missing-or-empty
fragment (with a release exclusion keyed directly to control 4's own
validated "the `[Unreleased]` body was actually emptied under a matching
new version heading" classification, computed once and shared by both
controls so they cannot disagree -- not merely CHANGELOG.md's git status,
which a fragment deletion plus any unrelated CHANGELOG.md edit, down to a
trailing newline, would also have satisfied -- and no longer requiring a
deleted fragment at all, so a version-only release with nothing accumulated
in `changelog.d/` also merges), and aggregation
conservation checked by per-bullet identity, not substring containment
(calibrated against a fragment-dropping sham, a first-line-only-truncating
sham, and a sham that drops one fragment while duplicating another whose
entire rendered block is a byte-for-byte prefix of the dropped one's).
Fragment bodies are also checked for hygiene (a stray carriage-return byte,
a leading list marker, or a leading ATX heading marker -- `#` through
`######` followed by a space, not a bare issue-reference `#`) before
aggregation. The category vocabulary is eleven
words, not ten: `changed` joins the set, measured 12 times across
`CHANGELOG.md`'s full history (more than seven of the other ten words),
where an earlier version of this mechanism's own documentation called it
"unmeasured" and told authors not to add it. The existing 620-line,
27-bullet `[Unreleased]` body is left untouched rather than converted: of
its bullets, 10 are in `- <Lead> (#NNN):` form, 3 more carry an id outside
that position, and 14 carry no id of any kind, so control 6 would reject
every fragment a conversion tried to produce for those 14; the body moves
under a version heading the ordinary way at the next release
(`docs/RELEASE.md` step 7), in the same step that aggregates this
directory's fragments. Every migration site moves together: `AGENTS.md`,
`CONTRIBUTING.md`, `docs/DESIGN-kernel-contract.md`, and
`docs/DESIGN-saga-history.md`'s coupled-change guidance,
`.claude/hooks/changelog_reminder.py`, `.claude/agents/fsl-coupled-change-reviewer.md`,
`.claude/skills/add-language-feature/SKILL.md`, `.claude/skills/release/SKILL.md`,
`tools/check-product-gate-scope.sh`'s `changelog.d/` exemption prefix (with
the `changelog.dx/y` near-miss still classifying as product),
`docs/DESIGN-ci.md`'s exemption amendment, `.github/workflows/release.yml`'s
stale-fragment guard, and `.github/workflows/merge-readiness.yml`'s new
pull-request-level fragment/direct-edit check. This entry is itself the
first fragment.
