Documented (#1781, refs #1766): `examples/fsl5/` adds the business and design
layers of the leave-request examples (#1777), each in English and Japanese
texts: an As-Is and a To-Be `business` flow (`leave_flow_asis`,
`leave_flow_tobe`) and a kernel `spec` design (`leave_design`), connected by
three refinement mappings (To-Be refines As-Is, the requirements refine the
To-Be, the design refines the requirements), each `refines` at depth 8 for
either spelling. FSL 5 has no design dialect of its own, and its Japanese
keywords and glossary are read in `requirements` only, so the `.ja.fsl` files of
these two layers differ from the `.en.fsl` ones only in comments and quoted
texts. `refine_corpus_parity.rs` registers the three mappings. The corpus helper
`top_level_keyword` now skips an `fsl 5` header line, so a headed refinement
file is recognized as a mapping, and the LSP corpus test expects the next
lexicon only for an `fsl 5` requirements document, which is what the language
server indexes with it.
