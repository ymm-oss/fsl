Changed (#1723): the GitHub Release body is now built by
`tools/release-notes.sh`. When `docs/release-notes/vX.Y.Z.md` exists, the body
is that hand-written summary followed by an absolute link to the `## [X.Y.Z]`
section of the tagged `CHANGELOG.md`. Otherwise the body is that section, as
before. The `assemble` job builds and checks the body, so a `workflow_dispatch`
dry run fails on a missing section, an empty body, or a body over GitHub's
125,000-character limit (checked as 125,000 bytes) before any tag exists. The
aggregated 5.0.0 section is 225,360 bytes, so 5.0.0 needs a summary.
