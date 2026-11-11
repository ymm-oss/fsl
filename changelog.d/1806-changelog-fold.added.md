Added (#1806): `tools/aggregate_changelog.sh fold --version X.Y.Z` folds
fragments that merged after the release commit into the existing, newest
`## [X.Y.Z]` section, which 5.0.0 did by hand five times. Each bullet is
rendered as `release` renders it and goes before the first bullet whose
declared category and first id sort after it. Existing entries keep their
order, the heading keeps its date, and the folded fragments are deleted. A
conservation check rejects any other change to `CHANGELOG.md`. `fold` refuses,
writing nothing, when the section is missing or not the newest, when
`[Unreleased]` has content, when the tag `vX.Y.Z` exists locally, when the
section already fails `check-section`, or when a fragment's (id, category) is
already an entry. With no fragments it changes nothing. Run on the parent trees
of #1790, #1793, #1796 and #1802, it reproduces their committed `CHANGELOG.md`
byte for byte. `docs/design/RELEASE.md` describes the step. The `selftest` usage
text and success line now name all the fixtures it runs: the six controls,
`check-section` and `fold`.
