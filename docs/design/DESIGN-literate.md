<!-- SPDX-License-Identifier: Apache-2.0 -->
# Literate Markdown FSL

Issue: #193.

## 1. Motivation

FSL's default stance is spec-as-doc (`.fsl` files are the canonical source).
In settings where natural-language Markdown is the canonical document —
customer contracts, non-engineer-maintained requirements — a `.fsl` file
becomes a secondary derivative and the doc↔spec drift surface grows.
Literate Markdown FSL closes that gap by making the Markdown document itself
the verification target: ` ```fsl ` fenced code blocks are extracted in place
and verified as a single compilation unit.

## 2. Design

### In-place blanking

Lines outside ` ```fsl ` fences are replaced with empty lines.  This
preserves byte offsets, line numbers, and column positions so that every
diagnostic — parse errors, type errors, counterexample locations — points
to the Markdown document's own lines without any position remapping.

The blanking function (`fsl_syntax::extract_literate_fsl`) returns `None`
when no ` ```fsl ` fence is found, so non-literate `.md` files are never
misidentified.

### Captured source and diagnostic context

The native CLI (`fslc`) captures the blanked text in an owned `String` and
passes it to the existing source-based check/verify/scenarios consumers.
Root edition admission, requirement-tag validation (including verify's
`--strict-tags`), and refinement/governance folds consume that extracted
source, not raw Markdown or a second read of an extracted file. Blanking
preserves the source line structure used by diagnostics.

For compatibility, `.{stem}.literate-{pid}.fsl` is still calculated as a
logical path beside the original `.md`. It is never created, read or unlinked
as the extracted root. This preserves the existing scenarios root-admission
labels: edition migration findings and implicit-initial-value warnings may
name that synthetic path. Check and verify pass the original `.md` separately
for their root admission/display labels; the earlier claim that every public
label always named the Markdown file was not true of scenarios.

Concurrent commands do not share a mutable extracted file, and abrupt
termination cannot leave a newly materialized source sibling. Existing
abandoned siblings are not removed or modified. Source-directory write
access is no longer needed for extraction; verifier-cache writes remain
independent. The original Markdown path remains the verify-cache identity
input (see "Cache key" below). The logical path's lexical parent is still
the resolver base, preserving dependencies beside a symlink alias rather
than rebasing to the canonical target. Dependency files retain their
existing independent read semantics; this is not an atomic filesystem snapshot.

### Multiple blocks

Multiple ` ```fsl ` blocks in one document form one compilation unit.
Definitions can be split across sections (e.g. state in one block, actions
in another).  The parser sees the concatenated blanked text and applies
its normal grammar.

### Detection

A `.md` file extension triggers literate mode.  If the file contains no
` ```fsl ` fences, the CLI reports a clear error rather than feeding
empty text to the parser.  Non-fsl fenced blocks (` ```python `, etc.)
are ignored.

Fence recognition follows the CommonMark fenced-code-block grammar rather
than a fixed triple-backtick check: an opening fence is a line whose
trimmed form starts with a run of three or more backticks or tildes, and
the block is an fsl block iff the first whitespace-separated token of the
info string (the text after that run) is exactly `fsl`. A closing fence is
a line whose trimmed form is a run of the *same* character, at least as
long as the opening run, followed by nothing else — so a shorter or
differently-charactered run, or a run followed by trailing text (e.g.
` ``` foo `), does not close the block and is ordinary content instead.
While inside a fence, lines are checked only for closing; a line that looks
like an opening fence is content of the current block, never a nested
fence — this is what lets a non-fsl four-backtick (or `~~~~`) fence safely
contain a literal ` ```fsl ` example without corrupting extraction. An
unterminated fence runs to end of file.

### Marking complete specifications in `docs/` (#1137)

Most `fsl` fences under `docs/` are fragments, so `fslc check` on the whole
document exits 2 — correct for a fragment, but it leaves no way to tell a
fragment from a complete specification that has since broken. Inferring
completeness from "it checks" is rejected for that reason. An author instead
marks a complete specification with a second info-string word:
`fsl check` in place of `fsl`. GitHub renders it as an ordinary `fsl` code
block (verified with `gh api --method POST /markdown -f mode=gfm`: the word
survives only as an undisplayed `data-meta="check"` attribute), and the
extractor above still sees it, because it keys on the *first* token.

`tools/check-doc-fences.py` is the gate. Every marked fence must check; all
fences of one document form one compilation unit, as for any literate
document. Every unmarked `fsl` fence is skipped and counted, and the report
prints both numbers so "the docs are verified" cannot be claimed beyond what
was measured. `fslc` is given the `.md` itself so a failure carries the
document's own line; when a document also holds unmarked fragments, a copy is
given whose unmarked opening fences are renamed on the same line, which cannot
move any line. Marking every remaining fence is deliberately not done here:
each needs a judgement that it is a complete specification, and the gate's
skipped count is the to-do list.

### `use`/compose path resolution

Import paths resolve relative to the Markdown file's parent directory,
matching the existing behavior for `.fsl` files (via `FsResolver`).
No code change was needed.  A literate `.md` may `use`/compose `.fsl` files
relative to its own directory; using another `.md` file as a compose target
is not supported.

### Cache key

Verify hashes the original Markdown bytes, not just the extracted FSL, so
prose-only edits also invalidate the cache. The entry path itself is not
hashed or canonicalized into a key field. Dependency inputs are the paths
relative to the lexical resolver base and contents actually read by the
recording resolver during model preparation and an active inline `implements`
seam. No directory walk collects unrelated siblings. Options, backend/solver,
implementation fingerprint and optional requirements-file bytes remain key
inputs as for ordinary specs.

The synthetic diagnostic path and process ID do not enter the cache key.
Repeated and concurrent invocations can therefore share cached kernel results
without sharing a mutable extracted file. Governance preservations still fold
outside the kernel cache, including on a hit. Dependencies beside a Markdown
symlink alias remain recorded from that alias's lexical parent, not from the
canonical target's directory.

## 3. Scope boundaries

### WASM / browser Worker

The browser Worker receives source text directly from the application, not
a file path, and `fsl-wasm` exports only `run` and `internal_error` via
`wasm_bindgen` — `extract_literate_fsl` is a plain Rust function on
`fsl-syntax`, not reachable from JavaScript.  Literate extraction is
therefore the embedding application's responsibility, performed in JS (or by
any tool that strips non-fsl lines while preserving line positions) before
the blanked text is passed to the Worker.  `extract_literate_fsl` is
available as-is to Rust embedders of `fsl-syntax`; exposing an equivalent
through `fsl-wasm` for JS callers is possible future work, not something
this design relies on today.  The Worker does not inspect `source_file`
extensions.

### CLI contract

The CLI contract's `file` positional argument accepts any file path and
does not constrain extensions.  Literate support is a runtime behavior
triggered by the `.md` extension, not a contract schema change.

### Frozen Python reference

The frozen Python implementation is not changed.  Literate Markdown is
a Rust-only feature (same policy as `--engine explicit`, `--engine auto`,
and `approval`).

### LSP

The native Rust language server (`rust/fsl-lsp`, issue #310) does not yet
handle `.md` documents — its document index and
diagnostics pipeline are wired to `.fsl` sources only. Literate Markdown
support in `fsl-lsp` is deferred follow-up work; `extract_literate_fsl` in
`fsl-syntax` is ready for it (the same blanking function the CLI uses would
let the LSP index a `.md` document's fsl fences with correct line positions
without a separate code path).

## 4. Test evidence

- `rust/fsl-syntax/src/literate.rs`: 10 unit tests (extraction, line-count
  preservation, multi-block, no-fsl-fence rejection, non-fsl fence
  isolation, mixed fences, four-backtick fence containing a literal
  ` ```fsl ` example, tilde-fenced non-fsl block, `~~~fsl` fence, backtick
  run with trailing text staying content inside an fsl block).
- `rust/fslc/tests/literate_markdown.rs`: 12 CLI integration tests (check,
  verify, scenarios, parse-error loc mapping, no-fsl rejection, non-fsl
  rejection, multi-block = single-block verdict, materialized-file cleanup,
  verify-cache hit on a second run, edition-finding `file` field naming the
  `.md` document, four-backtick fence repro verifying as `violated`, and
  concurrent check/verify/scenarios isolation on one Markdown source).
- `rust/fslc/src/main.rs` and `rust/fslc/src/verification.rs` unit tests:
  process IDs produce distinct materialization paths, and symlink-alias cache
  keys invalidate when a dependency beside the alias changes.
- `examples/literate/toggle.md`: canonical literate example.
