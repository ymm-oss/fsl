Documented (#1766): `examples/fsl5/` ships the first `fsl 5`-headed examples: one
leave-request requirements model written with the English spellings
(`leave_request.en.fsl`) and with the Japanese keywords, particles and glossary
aliases (`leave_request.ja.fsl`). Both check, lint with no finding, verify to
depth 8 and prove their invariants by induction; their Kernel JSON is identical
apart from source locations and the quoted requirement texts. LANGUAGE.md,
LANGUAGE.ja.md, the README tree and the site's examples pages link them.
The corpus tests that assumed every corpus document is headerless now read a
headed document in its own edition: `edition_header.rs` copies it unchanged
instead of adding a second header, and the LSP corpus test expects the next
lexicon for an `fsl 5` document. `formatter_cli.rs` formats each corpus document
only under the edition policies `fslc fmt` accepts for it, so an `fsl 5`
document is no longer formatted under the `fsl 4` policy its header refuses.
