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

### Materialization

The native CLI (`fslc`) materializes the blanked text to a sibling
`.literate.fsl` file next to the original `.md`, passes that path to the
existing check/verify pipeline, and removes the sibling on completion
(via a `Drop` guard).  This design avoids modifying every independent
`read_to_string` call site in the CLI — the blanked file is read from disk
by all existing code paths, and positions are correct because blanking
preserves line structure.

The sibling file requires write access to the source directory and is
visible to concurrent processes during the CLI run.  This is acceptable
for a local developer tool; a future design could use an in-memory buffer
instead of a sibling file, but that would require a shared-source-loader
refactoring across the ~8 independent `read_to_string` call sites in the
CLI.

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

### `use`/compose path resolution

Import paths resolve relative to the Markdown file's parent directory,
matching the existing behavior for `.fsl` files (via `FsResolver`).
No code change was needed.

### Cache key

`collect_fsl_sources` (verify cache-key computation) includes `.md` files
in directory walks so that edits to literate Markdown specs invalidate
the verification cache.

## 3. Scope boundaries

### WASM / browser Worker

The browser Worker receives source text directly from the application, not
a file path.  Literate extraction is the application's responsibility:
call `extract_literate_fsl` (exported by `fsl-syntax`) before passing the
blanked text to the Worker.  The Worker does not inspect `source_file`
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

The current LSP is Python-based (#310 tracks its Rust migration).
Literate Markdown support in the LSP is deferred until the Rust LSP
exists; the `extract_literate_fsl` function in `fsl-syntax` is ready
for it.

## 4. Test evidence

- `rust/fsl-syntax/src/literate.rs`: 6 unit tests (extraction, line-count
  preservation, multi-block, no-fsl-fence rejection, non-fsl fence
  isolation, mixed fences).
- `rust/fslc/tests/literate_markdown.rs`: 8 CLI integration tests (check,
  verify, scenarios, parse-error loc mapping, no-fsl rejection, non-fsl
  rejection, multi-block = single-block verdict, materialized-file
  cleanup).
- `examples/literate/toggle.md`: canonical literate example.
