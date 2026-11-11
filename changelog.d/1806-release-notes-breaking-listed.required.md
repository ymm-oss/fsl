Required (#1806): `tools/aggregate_changelog.sh check-section` fails
`changelog-section-breaking-unlisted` when a release has the summary
`docs/release-notes/vX.Y.Z.md` and an id of a breaking entry of `## [X.Y.Z]`
does not appear as `#N` under the summary's `## Breaking changes` heading. The
summary replaces the section as the GitHub Release body, so an omitted breaking
change was invisible on the release page, and the 5.0.0 review compared the ids
by hand. Ids in a `refs` list are not required. `--summary-dir DIR` overrides
the default `docs/release-notes` beside the changelog. The merge-readiness
automation lane already runs `check-section`. On 5.0.0 it checks 67 ids from 61
breaking entries, and all are listed.
