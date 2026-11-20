Required (#1936): `tools/aggregate_changelog.sh check`, `check-pr`,
`release` and `fold` reject a fragment whose lead would fail `check-section`
once rendered (`changelog-fragment-lead-invalid`), or whose lead's category
is not the file name's category or does not name the file name's id among
its own ids, outside a `refs` list (`changelog-fragment-lead-mismatch`).
One lead parser serves the fragment checks and `check-section`. `check-pr`
also runs the command that will consume the pending fragments on a scratch
copy of `CHANGELOG.md` (`fold` into the newest section while it is untagged,
`release` otherwise) and fails with `changelog-fragment-section-invalid` when
the section it writes would break `check-section`, so two fragments naming
one id under one category fail before merge. `release` checks its own
section the way `fold` does (`changelog-release-section-invalid`). At 5.2.0
two merged fragments, one with another issue's id and one with no lead, had
to be corrected by hand in the release commit.
