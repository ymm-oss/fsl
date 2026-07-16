# Native Rust language server

Status: accepted for issue #310.

## 1. Goal and boundary

`fslc-lsp` is a native stdio language server built from the same authoritative Rust syntax,
lowering, typed-model, and analysis crates as `fslc`. The executable name and observable LSP
features remain stable; the Python/Lark/pygls implementation is removed rather than retained as a
fallback.

The server owns only editor concerns: open-buffer state, workspace discovery, source-positioned
document indexes, and LSP protocol translation. It does not own an FSL grammar or semantic rules.
Every standard FSL document is accepted or rejected first by `fsl-syntax`; the existing Rust-native
AI-project compatibility recognizer remains its explicit frontend gate. Semantic diagnostics come
from a shared Rust frontend diagnostic API used by the CLI and LSP.

## 2. Issue contract

### Goal

- A released `fslc-lsp` starts without Python and provides diagnostics, completion, hover,
  definition, references, document symbols, rename, semantic tokens, and code actions.

### Invariants

- `fsl-syntax` remains the only parser and dialect registry used by active product surfaces.
- `fsl-core` remains the only typed lowering/model boundary used for semantic diagnostics.
- CLI JSON envelopes and exit meanings are not replaced by LSP-specific semantics.
- `fsl-runtime` remains independent of LSP, solver, Z3, and JavaScript bridges.
- The server communicates over stdio and never writes protocol-independent output to stdout.
- No Python or compatibility fallback is invoked by the server or native integration gate.

### Acceptance measurements

- Rust unit tests cover every query operation and document-store transition.
- A process test performs initialize, open/change diagnostics, one request, shutdown, and exit.
- Corpus tests parse and index every valid file under `specs/` and `examples/` through the
  authoritative dialect registry and reject missing identifier coverage unless explicitly waived.
- CLI and LSP tests compare diagnostic kind and message for parse, type, and semantic failures;
  shared-frontend tests preserve the compiler range independently where the stable CLI envelope
  intentionally omits it.
- release CI builds and starts both native executables on macOS, Linux, and Windows targets.
- `tools/check-native-integration.sh` covers the LSP without executing Python.

### Non-goals

- New FSL syntax or verification semantics.
- Editor UI changes or editor-specific protocol extensions.
- Python feature backports or a permanent fallback.

## 3. Architecture

```text
VS Code / LSP client
        |
        v
fslc-lsp protocol adapter ── DocumentStore (open text + workspace paths)
        |                              |
        |                              v
        |                       DocumentIndex
        |                       (AST roles + lossless spans)
        v
shared frontend diagnostics
        |
        +── fsl-syntax::parse_document / lossless_document
        +── fsl-core::parse_kernel_source_with_file / build_model
        `── fsl-tools::analyze_model where informational analysis is requested
```

The document index combines two outputs of the same Rust syntax crate:

1. the parsed dialect AST establishes which declarations and relationships are syntactically
   valid; and
2. the lossless token stream supplies exact identifier spans that some surface AST nodes do not
   retain.

The index may classify tokens only after authoritative parsing succeeds. Contextual token walking
is an index projection, not a second parser: it cannot accept a document, lower syntax, or invent
semantic validity.

Open buffers take precedence over files on disk. Imports and workspace references load through a
document resolver that first consults the store and then the filesystem relative to the owning
document. Closing a buffer discards its overlay and republishes diagnostics from disk only after a
subsequent open/save event.

## 4. Protocol contract

The server advertises incremental text synchronization and these standard methods:

- `textDocument/publishDiagnostics`
- `textDocument/completion` (with `.` trigger)
- `textDocument/hover`
- `textDocument/definition`
- `textDocument/references`
- `textDocument/documentSymbol`
- `textDocument/rename`
- `textDocument/semanticTokens/full`
- `textDocument/codeAction`

`rename` is an issue #310 target feature, not parity with the removed Python server: the Python
server never implemented it. Rename edits all references that resolve to the selected declaration
inside the known workspace, including the declaration. An unresolved symbol or invalid replacement
identifier returns no edit.

Diagnostics use the shared compiler classification (`parse`, `type`, or `semantics`) as their code,
the compiler message verbatim, and the compiler source span converted from one-based FSL positions
to zero-based LSP ranges. Migration diagnostics and their machine-applicable edits are derived from
`fsl-syntax::canonical_rewrites`; code actions apply those exact edits.

## 5. Terms, laws, and link map

### TERM-lsp-document-overlay (S1, Boundary)

The latest in-memory text and version for an open document. Observable fields are URI, version, and
text. It enters only through didOpen/didChange and is removed by didClose.

Related laws: `LAW-lsp-authoritative-analysis`, `LAW-lsp-latest-buffer`.

### TERM-lsp-document-index (S1, Value)

A source-positioned projection of declarations, references, imports, and semantic-token roles for
one successfully parsed document. It is recreated from source and never mutates compiler ASTs.

Related laws: `LAW-lsp-authoritative-analysis`, `LAW-lsp-resolved-edit`.

### TERM-lsp-source-diagnostic (S1, Value)

The shared compiler diagnostic identity: kind, message, code, source file, and exact span. The CLI
and LSP render this value into their own transport shapes.

Related law: `LAW-lsp-diagnostic-parity`.

### LAW-lsp-authoritative-analysis (Invariant, S1)

For every indexed or diagnosed standard FSL document `d`, success implies
`fsl_syntax::parse_document(d).is_ok()`. A legacy AI-project document must instead pass the shared
Rust-native compatibility recognizer. Semantic success additionally implies the applicable
`fsl_core` lowering/model gate succeeds. No LSP-local grammar can override either result.

Grounding: parser-rejection unit tests and the valid-corpus index gate.

### LAW-lsp-diagnostic-parity (Invariant, S1)

For every source rejected by the shared frontend diagnostic API, CLI and LSP projections preserve
the same kind and message. The LSP preserves the shared compiler source span; the CLI preserves its
existing envelope and exposes a location only where that established transport already did so.

Grounding: shared diagnostic unit tests plus CLI/LSP identity integration tests.

### LAW-lsp-latest-buffer (Postcondition, S1)

After an accepted didOpen or didChange notification, every subsequent query and published
diagnostic for that URI uses the newest stored version, never stale disk contents.

Grounding: document-store unit tests and stdio process test.

### LAW-lsp-resolved-edit (Precondition, S1)

Rename and code actions emit edits only for source spans whose target is resolved by the document
index or whose rewrite is produced by the authoritative lossless rewrite planner. All edits are
valid UTF-16 LSP ranges and replacement identifiers match FSL identifier syntax.

Grounding: rename resolution tests, Unicode range tests, and migration code-action tests.

### Link map

| Law | Terms | Primary evidence |
|---|---|---|
| `LAW-lsp-authoritative-analysis` | overlay, index | parser/corpus tests |
| `LAW-lsp-diagnostic-parity` | source diagnostic | CLI/LSP diagnostic tests |
| `LAW-lsp-latest-buffer` | overlay | store/process tests |
| `LAW-lsp-resolved-edit` | index | rename/code-action tests |

There are no isolated S1 laws or terms.

## 6. Predict-Light and staged change

Risk is P2 because the change replaces a public executable implementation and changes release
assets. It does not change authentication, persistence, billing, or the FSL language.

1. Add the shared diagnostic contract and prove CLI projection remains stable. Rollback: revert the
   isolated API commit.
2. Add the Rust LSP library/binary and focused query/process tests. Rollback: the new binary is not
   yet selected by distribution.
3. Add the Rust corpus coverage gate and distribution assets. Rollback: revert release/installer
   wiring while keeping the independently testable binary.
4. Delete the Python LSP and update documentation. This step occurs only after the native gate is
   green; rollback is the preceding commit, not a runtime fallback.

Stop and re-scope if three unrelated test failures contradict the model, any required feature lacks
an observable test, or a stage cannot be reverted independently.

## 7. Evidence target

The minimum completion target is L2/E2: unit evidence for all S1 laws, process/corpus integration
evidence across protocol and filesystem boundaries, changed-line coverage or equivalent focused
mutation evidence, and an independent final review. Production telemetry is not part of a local
stdio language server contract.
