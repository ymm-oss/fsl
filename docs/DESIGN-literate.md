# Literate FSL in Markdown

Issue #193 adds Markdown as a container format for ordinary FSL compilation
units. A file is treated as literate FSL when it contains at least one
standalone fenced block whose opening line is `````fsl`` and whose closing line
is ``````, with optional horizontal whitespace around either marker.

## Extraction contract

`src/fslc/literate.py` applies an in-place extraction before parsing:

- lines inside every `fsl` fence are retained byte-for-byte;
- prose and fence-marker lines are replaced by blank lines while retaining
  their original newline sequence;
- all `fsl` blocks are concatenated implicitly by their original positions and
  parsed as one compilation unit;
- files with no `fsl` fence, including files containing only another fence
  language, pass through unchanged;
- an unclosed `fsl` fence is a parse error located at its opening line.

Blanking instead of copying blocks into a new buffer preserves the document's
line and column coordinates. Parser diagnostics, declaration locations, LSP
symbols/references, and counterexample action locations therefore point directly
into the Markdown document.

## Integration boundaries

Sniffing happens at each parser entry point, including files loaded through
`compose` and dialect imports. The existing source path remains the base path,
so a relative import in `docs/order.md` resolves relative to `docs/`, exactly as
it would for `docs/order.fsl`.

The LSP selects a line-preserving parser adapter before dialect sniffing. The
adapter exposes the extracted FSL to the existing index visitor without changing
document offsets.

Verification-cache identity continues to include the raw entry-file bytes in
addition to the desugared kernel AST. A prose-only edit can therefore never
reuse a verdict attributed to an older document revision, even when the embedded
FSL is unchanged.

## Semantic boundary

Markdown is only a source container. It introduces no grammar production, AST
node, verifier/runtime behavior, assurance class, or exit-code change. Multiple
blocks have exactly the same meaning as the same FSL declarations in a plain
file. Documentation traceability remains an explicit reporting concern; prose
outside a fence is not assigned formal semantics.
