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
`.{stem}.literate.fsl` file next to the original `.md` (e.g. `toggle.md`
materializes to `.toggle.literate.fsl`), passes that path to the existing
check/verify pipeline, and removes the sibling on completion (via a `Drop`
guard).  This design avoids modifying every independent `read_to_string`
call site in the CLI — the blanked file is read from disk by all existing
code paths, and positions are correct because blanking preserves line
structure.  The materialized path is used only to read source content;
every user-visible label in the CLI's JSON output (`file` fields, migration
and implicit-initial-value finding locations) is stamped with the original
`.md` path instead, so machine-readable output never names the transient
sibling.

The sibling filename is deterministic (no process ID component) so that the
verify cache key — which is keyed in part on the canonicalized path passed
to the check/verify pipeline — stays stable across repeated runs of the same
document (see "Cache key" below). The sibling file requires write access to
the source directory and is visible to concurrent processes during the CLI
run: two concurrent runs on the same `.md` document write identical bytes
(both blank from the same source), so a race is at worst a redundant write,
never a wrong verdict; the only failure mode is one run's read losing to
another process's deletion of the sibling mid-read, which errors loudly
rather than silently misreading. This is acceptable for a local developer
tool; a future design could use an in-memory buffer instead of a sibling
file, but that would require a shared-source-loader refactoring across the
~8 independent `read_to_string` call sites in the CLI.

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

### `use`/compose path resolution

Import paths resolve relative to the Markdown file's parent directory,
matching the existing behavior for `.fsl` files (via `FsResolver`).
No code change was needed.  A literate `.md` may `use`/compose `.fsl` files
relative to its own directory; using another `.md` file as a compose target
is not supported.

### Cache key

`collect_fsl_sources` (verify cache-key computation) includes `.md` files
in directory walks so that edits to literate Markdown specs invalidate the
verification cache. The transient `.{stem}.literate.fsl` materialization
(see "Materialization" above) is excluded from that walk by filename suffix
— its content is already represented by the `.md` file itself through this
same function's Markdown branch, so including it too would double-count it
and, before the sibling's filename was made deterministic, made every run's
cache key unique (the walk picked up the run-local sibling as if it were an
independent dependency). The cache key's `path` field is the canonicalized
path passed to the check/verify pipeline (the materialized sibling for a
literate `.md`), which is why the sibling's filename must be deterministic
rather than PID-suffixed — otherwise the key would never repeat across runs
of the same document.

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
- `rust/fslc/tests/literate_markdown.rs`: 11 CLI integration tests (check,
  verify, scenarios, parse-error loc mapping, no-fsl rejection, non-fsl
  rejection, multi-block = single-block verdict, materialized-file cleanup,
  verify-cache hit on a second run, edition-finding `file` field naming the
  `.md` document, four-backtick fence repro verifying as `violated`).
- `examples/literate/toggle.md`: canonical literate example.
