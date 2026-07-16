# FSL — coupled-change metatests (native LSP corpus/index + DESIGN-doc coverage)

Motivation: issue #168. The repository rule that a language feature moves with
its parser, model, runtime, documentation, and tests began as a human checklist.
The original Python metatest caught dialects and grammar productions that the
Python LSP index silently omitted. Issue #310 moved the language server and its
coverage gate to the authoritative Rust implementation.

The coupled-change checks now have two owners:

- `rust/fsl-lsp/tests/corpus.rs` owns native LSP corpus and index coverage.
- `tests/test_coupled_change_meta.py` retains compatibility checks for the
  frozen Python reference and the DESIGN-document map.

## 1. Native LSP corpus and index coverage

The Rust test scans valid `.fsl` sources below `specs/` and `examples/`, while
excluding the intentionally invalid `examples/gallery/errors/` corpus. For each
source it builds `fsl_lsp::DocumentIndex`, which first parses through the
authoritative `fsl-syntax` frontend before indexing its token stream.

The test fails if either condition is not met:

1. every valid corpus source can be parsed and indexed by the native LSP; and
2. every identifier token that represents a declaration or reference has a
   corresponding indexed symbol or reference position.

This is intentionally a product test in the Rust workspace. It does not compare
against the removed Python/Lark index and does not permit a Python fallback.
Dialect dispatch, source locations, and semantic acceptance therefore use the
same implementation boundary as the native CLI.

Focused unit tests in `rust/fsl-lsp/src/index.rs` cover role, scope, definition,
reference, rename, and semantic-token behavior. Server unit tests and
`rust/fsl-lsp/tests/stdio.rs` cover request handling, unsaved buffers, workspace
resolution, and the stdio lifecycle.

## 2. DESIGN-doc coverage (dialect/feature ↔ docs/DESIGN-*.md)

`tests/test_coupled_change_meta.py` keeps three assertions whose inspected
surfaces remain Python-owned or language-neutral:

1. **README map is bidirectional** — the `DESIGN-*.md` links in
   `docs/README.md` exactly match the design files on disk.
2. **Frozen Python dialect registry remains explicit** — its registered
   dialects and AI project blocks are compared with the native authority. This
   is compatibility evidence, not an evolving language authority.
3. **Frozen Python CLI map remains explicit** — every compatibility CLI command
   maps to an existing design document or a reviewed waiver reason.

These Python checks remain manual compatibility evidence. The native LSP corpus
test runs as part of the Rust workspace and the required
`./tools/check-native-integration.sh` product gate.

## Non-goals

- No Python reimplementation of native LSP parsing, indexing, or features.
- No claim that a DESIGN document is semantically current merely because it is
  linked; content freshness remains review territory.
- No fallback from `fslc-lsp` to the frozen Python package.
