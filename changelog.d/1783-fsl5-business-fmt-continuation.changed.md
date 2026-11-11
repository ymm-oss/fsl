Changed (#1783): in a document headed `fsl 5`, `fslc fmt` indents the lines a
declaration continues on one level deeper than its first line, as it already
did for the requirements dialect's statements: a business `control`'s
`owner` / `severity` / `applies_to`, a `policy` or `goal`'s `satisfies` and
body (`every …`, `invariant { … }`, `some … can reach …`), a `def` body and a
`map … =` continuation. A brace opened on such a line indents its contents
from there. Before, these lines were flattened to the declaration's own
indentation, so the business example of `skills/fsl-business/SKILL.md` was not
fmt-canonical. `fmt --check` now reports a previously canonical `fsl 5`
document written in the flat layout as `changed`; the formatted document lowers
to the same kernel. A headerless (FSL 4) document's canonical form is
unchanged. The `examples/fsl5/` documents are reformatted, and
`fsl5_fmt.rs` checks the layout, the skill example and those examples.
