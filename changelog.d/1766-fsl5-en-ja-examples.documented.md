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
The same directory adds the business and design layers of that domain, each in
English and Japanese texts: an As-Is and a To-Be `business` flow
(`leave_flow_asis`, `leave_flow_tobe`) and a kernel `spec` design
(`leave_design`), connected by three refinement mappings (To-Be refines As-Is,
the requirements refine the To-Be, the design refines the requirements), each
`refines` at depth 8 for either spelling. FSL 5 has no design dialect of its
own, and its Japanese keywords and glossary are read in `requirements` only,
so the `.ja.fsl` files of these two layers differ from the `.en.fsl` ones only
in comments and quoted texts. `refine_corpus_parity.rs` registers the three
mappings. The corpus helper `top_level_keyword` now skips an `fsl 5` header
line, so a headed refinement file is recognized as a mapping, and the LSP
corpus test expects the next lexicon only for an `fsl 5` requirements
document, which is what the language server indexes with it.
