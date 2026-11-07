Documented (#1440): `docs/manual/LANGUAGE.md` and `LANGUAGE.ja.md` gain
"Upgrading fslc from 4.12 to 5.0", for anyone moving the binary from 4.12
whether or not they adopt FSL 5. It lists the forms 5.0.0 refuses that 4.12
accepted, headerless documents included, with what `check` or `refine` reports
and the repair, and the CLI, JSON and exit-code changes with what to search for
and change. The edition guide no longer implies that a headerless document
keeps every 4.12 result. The breaking removals #1256, #1452 and #1595 are now
recorded with their issue numbers in both language references,
`skills/fsl/references/syntax.md` and their design notes; #1452 is measured
against pre-release builds, since 4.12 refused every `fsl 5` child. Breaking
fragments in `changelog.d/` use one of two labels:
`<Category> (#N, breaking removal):` for a form that was accepted and is now refused, and
`<Category> (#N, breaking):` for other breaking changes such as CLI, JSON,
exit-code, record-format or Rust API changes; and
`docs/release-notes/v5.0.0.md` summarizes the release for a 4.12 user.
