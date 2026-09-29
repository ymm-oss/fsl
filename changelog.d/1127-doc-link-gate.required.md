Required (#1127): Markdown link targets now resolve or the merge-readiness lane
fails. `tools/check-doc-links.py` runs repository-wide (214 documents, 396
in-repository link targets, about 0.1s, no network and no build) alongside its
slug selftest and the new accepting/rejecting controls in
`tests/test_doc_links.py`, which reproduce issue #1127's measured table in CI --
including a `.fsl#kind:name` fragment naming an element the specification does
not declare (the forward direction of #1124) and a `other.md#anchor` whose
anchor exists only in a different document. What the gate does not check (a bare
path in prose) and does not claim (that every FSL element is discussed by some
document, #1138) is recorded in `docs/design/DESIGN-ci.md`. The one pre-existing
finding, `CONTRIBUTING.md`'s link to a `CODE_OF_CONDUCT.md` that has never
existed in this repository, was removed rather than answered by inventing a
policy document.
