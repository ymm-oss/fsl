# FSL — coupled-change metatests (native LSP corpus/index + checklist presence + DESIGN-doc coverage)

Motivation: issue #168. The repository rule that a language feature moves with
its parser, model, runtime, documentation, and tests began as a human checklist.
The original Python metatest caught dialects and grammar productions that the
Python LSP index silently omitted. Issue #310 moved the language server and its
coverage gate to the authoritative Rust implementation.

The coupled-change checks have two owners:

- `rust/fsl-lsp/tests/corpus.rs` owns native LSP corpus and index coverage.
- `tests/test_coupled_change_meta.py` owns two native-to-frozen-Python
  compatibility comparisons, two frozen-Python-sourced DESIGN-map checks, and
  two language-neutral checks.

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

## 2. Language-feature checklist presence

`test_language_feature_checklists_include_coupled_obligations` checks three
authored checklists for the required Japanese language reference and conditional
LSP index plus targeted role/scope-test obligations. It is a fail-closed,
additive documentation-presence control: it reports each exact missing member
and copy, but does not establish LSP semantics, translation freshness, or
product verification.

## 3. DESIGN-doc coverage (dialect/feature ↔ docs/DESIGN-*.md)

The module's other five checks comprise two native-to-frozen-Python
compatibility comparisons, two frozen-Python-sourced DESIGN-map checks, and one
language-neutral check:

1. **`test_retained_python_dialect_registry_matches_native_authority`** — the
   frozen Python dialect registry exactly matches the native dispatch registry.
   This is compatibility evidence, not an evolving language authority.
2. **`test_native_ai_project_block_gate_matches_retained_parser`** — the native
   AI project-block gate exactly matches the retained Python parser's block set.
3. **`test_design_docs_readme_map_bidirectional`** — the `DESIGN-*.md` links in
   `docs/README.md` exactly match the design files on disk.
4. **`test_top_level_dialects_map_to_design_docs`** — the frozen Python
   grammar's `top_def` alternatives exactly match `TOP_DEF_DESIGN_DOCS`, and
   every mapped design document exists.
5. **`test_cli_commands_map_to_design_docs`** — every frozen Python
   compatibility CLI command maps to an existing design document or a reviewed
   waiver reason.

Together, these six Python tests run as required pre-merge repository/compatibility
evidence through `tools/check-merge-readiness.sh automation` and the
`merge readiness / automation contracts` job. They are not product evidence
and are not invoked by the Rust-native `./tools/check-native-integration.sh`
gate. The native LSP corpus/index test remains part of the Rust workspace and
the required product gate.

## Non-goals

- No Python reimplementation of native LSP parsing, indexing, or features.
- No claim that a DESIGN document is semantically current merely because it is
  linked; content freshness remains review territory.
- No fallback from `fslc-lsp` to the frozen Python package.
