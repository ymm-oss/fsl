Fixed: CLI test fixtures now check out with line-feed endings on every runner.
`.gitattributes` pinned `*.fsl` and four named files, so the `.md` and `.json`
fixtures added with the error-envelope parity matrix were checked out CRLF on
`windows-latest`. `error_envelope_document.md`'s leading `---` then stopped
parsing as document frontmatter and three parity cells failed on that runner
alone while Linux and macOS stayed green (run `31759050211`). The directory now
carries the rule, `.z3-trace` stays binary, and
`rust/fslc/tests/fixture_line_endings.rs` fails with the offending path if a
fixture escapes it again.
