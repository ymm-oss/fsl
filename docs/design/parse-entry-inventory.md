# Parse-entry inventory and the unstable gate

Status: internal design note for the next syntax edition, developed on
`main` behind an unstable gate. Not user documentation: the gate is not
advertised, and `rust/fslc/tests/unstable_gate_leak.rs` keeps it out of every
published tree except this directory.

## Gate

- `fsl-syntax` owns `UnstableFeatures` (a bit set; the only feature is
  `fsl5`) and `ParseOptions { unstable }`. Options are obtained only through
  `ParseOptions::stable()` (the published language, also `Default`) and
  `ParseOptions::with_unstable(..)`.
- Entries that take options: `parse_document_with`,
  `parse_surface_document_with`, `dialect_keyword_with`, `lex_with`,
  `lexical_warnings`, `IdentifierRule::for_source`. The old
  names (`parse_document`, ...) stay as the published-language entries and are
  the `_with` entry under `ParseOptions::stable()`. Every registered dialect
  frontend is reached only through `parse_document_with`, on the token stream
  it lexed with the same options.
- The library never reads the environment. Three process entry points decide
  the options, once:
  - native CLI: `main` reads `FSLC_UNSTABLE` (comma-separated feature names)
    through `fslc_rust::unstable_gate::from_environment` into a process
    `OnceLock`; the binary's own modules (`main.rs`, `causal.rs`,
    `verification.rs`) parse with it;
  - language server: `run_stdio` reads the same variable into `ServerState`
    and passes it to `DocumentIndex::build`, which keeps it for its later
    re-lexing;
  - browser Worker: `run(request_json)` accepts `"unstable": ["fsl5"]`, and the
    request's options reach the Worker's own parse.
- Unknown feature names enable nothing and print nothing, so an unset, empty,
  or unknown value is byte-for-byte the published language.
- The only behaviour behind the feature so far is the identifier rule below.
  R0
  (`rust/fslc/tests/unstable_gate_r0.rs`) holds that for the whole
  `specs/` + `examples/` corpus: `fslc kernel` and `fslc check` print the same
  bytes and exit the same with the gate off and on. It is the merge condition
  for every stage behind the gate.

## Identifier rule (S2)

`IdentifierRule::for_source(source, options)` picks the lexer's identifier
rule. It is `Unicode` only when `fsl5` is enabled **and** the first significant
line (after a BOM, whitespace and `//` comments, as the lexer skips them) is
`fsl`, blanks, `5`; otherwise `Ascii`, the published `[A-Za-z_][A-Za-z0-9_]*`.
The header tokens stay in the stream: reading the header as a declaration is
S1's, so until S1 a gated `fsl 5` document lexes with the wider rule and is
then rejected at dispatch (`FSL-DIALECT-UNKNOWN`), as the fail-closed test
expects. An ASCII-only document lexes to the same tokens under both rules.

Under `Unicode`:

- an identifier is `_` or `XID_Start`, then `XID_Continue` (UAX #31), read in
  NFC; a non-NFC spelling is the warning `non_nfc_identifier` and reads as its
  NFC form, so both spellings are one name;
- `・` U+30FB and `･` U+FF65 never join a name (`middle_dot_identifier`),
  although Unicode 15.1 made them `XID_Continue`;
- full-width letters and digits are `fullwidth_ascii_identifier`, other
  full-width ASCII forms `fullwidth_symbol`, each naming the ASCII character;
- an identifier beyond UTS #39 "highly restrictive" on scripts alone (one
  script, Japanese/Korean/Han-with-Bopomofo counted as one, or Latin plus one
  of those) is `mixed_script_identifier`. Identifier status is not consulted,
  so CJK Extension B stays usable;
- U+3000 stays whitespace and is the warning `fullwidth_space`;
- a requirement ID with a non-ASCII character is `non_ascii_requirement_id`
  (the parser; unreachable under `Ascii`).

`IDENTIFIER_DIAGNOSTICS` lists these codes; `canonical_name_not_ascii` is
reserved for alias resolution (S3). Errors are lexer/parse errors with the code
as `diagnostic_code`; warnings come from `lexical_warnings` and are published by
the language server. Commands that parse through the shared modules (`check`,
`kernel`, the language server's diagnostics) see the rule once S1 passes them
the options; `fslc lint` already does. Rename accepts a new name only if it is
one identifier under the rule of every document it edits.

## Fail closed (decisions/049 D2)

A parse that does not receive the process's options reads the published
language. Under the published language a document that starts with an edition
header (`fsl 5`, `fsl 4`) is the unknown declaration `fsl` and is rejected with
`FSL-DIALECT-UNKNOWN`, exactly as today
(`dispatch::tests::edition_header_fails_closed_under_stable_options`). So a
command whose first parse ran behind the gate and whose later re-parse did not
stops with an error; it never reads one file two ways. The stage that makes the
header parse behind the gate (S1) must keep the stable side of that test and
route the re-parse below.

## Inventory

Every reference to a stable-pinned entry outside tests, by file. The
authoritative copy is `INVENTORY` in
`rust/fslc/tests/parse_entry_inventory.rs`; that test reads every non-test
Rust source of the workspace as a syntax tree (paths, `use` aliases, function
pointers, macro bodies; a glob import from `fsl_syntax` is reported) and fails
on any count that differs, so a new parse entry cannot bypass the options
silently. Explicit stable constructions (`ParseOptions::stable`,
`ParseOptions::default`) are counted the same way, so
`parse_document_with(source, &ParseOptions::stable())` is not a routed parse.

`parse_expr` is exempt: it parses one expression with no document header.

Owner: the stage that converts the reference. `Definition` = the
published-language entry itself (stays); `D8` = causal / ai dialects, where
decisions/049 D8 makes an edition header an error, so the published language is
the intended reading.

| Reason | Meaning |
|---|---|
| LIBRARY_API | fsl-core / fsl-tools function taking source text; S1 adds the options to its signature |
| SHARED_MODULE | fslc library module also compiled into fsl-wasm / fsl-lsp; the CLI's process value would be wrong for a Worker request, so S1 passes the options as a parameter |
| FORMAT_MIGRATE | fmt, migrate and migration diagnostics on the lossless tree; S1 owns the header in formatting and header insertion |
| LITERATE | literate Markdown extraction; S1 reads the header in the first fsl fence |
| CAUSAL_AI | causal / ai dialects (D8) |
| STANDALONE_FRONTEND | public single-dialect parsers with no product caller; the registry frontends reuse the routed tokens instead; they stay on the published language (fail closed) |
| FRAGMENT | expression / spec fragment parsers without a header; the identifier rule is chosen by the header, so they always read ASCII identifiers |
| EDITOR_LEX | re-lexing to place a diagnostic of the same module's parse; takes that parse's options once S1 passes them in |
| PARITY_TOOL | the Python-parity debug binary |
| REGISTRY | the private `domain` frontend wrapper shares the public entry's name |
| STABLE_WRAPPER | the published-language entry itself |

| File | Entry | Count | Owner | Reason |
|---|---|---|---|---|
| `fsl-core/src/compose.rs` | `parse_document` | 2 | S1 | LIBRARY_API |
| `fsl-core/src/dialect.rs` | `parse_surface_document` | 7 | S1 | LIBRARY_API |
| `fsl-core/src/lib.rs` | `parse_document` | 4 | S1 | LIBRARY_API |
| `fsl-core/src/refinement.rs` | `parse_surface_document` | 2 | S1 | LIBRARY_API |
| `fsl-lsp/src/index.rs` | `is_causal_source` | 1 | D8 | CAUSAL_AI |
| `fsl-lsp/src/server.rs` | `canonical_rewrites` | 1 | S1 | FORMAT_MIGRATE |
| `fsl-syntax/src/ai.rs` | `lex` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/ai_project.rs` | `parse_ai_component` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/bin/fsl-parse-surface.rs` | `parse_surface_document` | 1 | S1 | PARITY_TOOL |
| `fsl-syntax/src/causal.rs` | `declaration_keyword` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/causal.rs` | `lex` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/db.rs` | `lex` | 1 | Definition | STANDALONE_FRONTEND |
| `fsl-syntax/src/dispatch.rs` | `parse_domain` | 1 | Definition | REGISTRY |
| `fsl-syntax/src/dispatch.rs` | `ParseOptions::stable` | 2 | Definition | STABLE_WRAPPER |
| `fsl-syntax/src/domain.rs` | `lex` | 1 | Definition | STANDALONE_FRONTEND |
| `fsl-syntax/src/lexer.rs` | `ParseOptions::stable` | 1 | Definition | STABLE_WRAPPER |
| `fsl-syntax/src/lossless.rs` | `canonical_rewrites` | 1 | S1 | FORMAT_MIGRATE |
| `fsl-syntax/src/lossless.rs` | `lex` | 1 | S1 | FORMAT_MIGRATE |
| `fsl-syntax/src/lossless.rs` | `lossless_document` | 2 | S1 | FORMAT_MIGRATE |
| `fsl-syntax/src/lossless.rs` | `parse_document` | 3 | S1 | FORMAT_MIGRATE |
| `fsl-syntax/src/parser.rs` | `ParseOptions::stable` | 1 | Definition | STABLE_WRAPPER |
| `fsl-syntax/src/parser.rs` | `lex` | 2 | Definition | FRAGMENT |
| `fsl-tools/src/causal.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |
| `fsl-tools/src/causal_expectation.rs` | `parse_document` | 1 | S1 | LIBRARY_API |
| `fsl-tools/src/document_project.rs` | `parse_document` | 2 | S1 | LIBRARY_API |
| `fslc/src/causal.rs` | `parse_causal` | 2 | D8 | CAUSAL_AI |
| `fslc/src/check_stages.rs` | `parse_document` | 1 | S1 | SHARED_MODULE |
| `fslc/src/check_stages.rs` | `parse_surface_document` | 1 | S1 | SHARED_MODULE |
| `fslc/src/frontend_output.rs` | `declaration_keyword` | 1 | D8 | CAUSAL_AI |
| `fslc/src/frontend_output.rs` | `parse_ai_project` | 1 | D8 | CAUSAL_AI |
| `fslc/src/frontend_output.rs` | `parse_surface_document` | 1 | S1 | SHARED_MODULE |
| `fslc/src/literate_access.rs` | `extract_literate_fsl` | 1 | S1 | LITERATE |
| `fslc/src/main.rs` | `extract_literate_fsl` | 1 | S1 | LITERATE |
| `fslc/src/main.rs` | `format_source` | 1 | S1 | FORMAT_MIGRATE |
| `fslc/src/main.rs` | `is_causal_source` | 2 | D8 | CAUSAL_AI |
| `fslc/src/main.rs` | `parse_ai_project` | 3 | D8 | CAUSAL_AI |
| `fslc/src/main.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |
| `fslc/src/migration.rs` | `canonical_rewrites` | 1 | S1 | FORMAT_MIGRATE |
| `fslc/src/migration.rs` | `dialect_keyword` | 1 | S1 | FORMAT_MIGRATE |
| `fslc/src/migration.rs` | `format_source` | 1 | S1 | FORMAT_MIGRATE |
| `fslc/src/migration.rs` | `lex` | 1 | S1 | FORMAT_MIGRATE |
| `fslc/src/migration.rs` | `lossless_document` | 6 | S1 | FORMAT_MIGRATE |
| `fslc/src/migration.rs` | `parse_surface_document` | 1 | S1 | FORMAT_MIGRATE |
| `fslc/src/reproducer.rs` | `parse_document` | 1 | S1 | SHARED_MODULE |
| `fslc/src/source_diagnostic.rs` | `canonical_rewrites` | 1 | S1 | FORMAT_MIGRATE |
| `fslc/src/source_diagnostic.rs` | `is_causal_source` | 1 | D8 | CAUSAL_AI |
| `fslc/src/source_diagnostic.rs` | `lex` | 1 | S1 | EDITOR_LEX |
| `fslc/src/source_diagnostic.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |
| `fslc/src/source_diagnostic.rs` | `parse_document` | 1 | S1 | SHARED_MODULE |
| `fslc/src/spec_load.rs` | `parse_document` | 1 | S1 | SHARED_MODULE |
| `fslc/src/verification.rs` | `is_causal_source` | 1 | D8 | CAUSAL_AI |
| `fslc/src/verification.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |
| `fslc/src/verification_output.rs` | `parse_surface_document` | 1 | S1 | SHARED_MODULE |

Routed at S0 (receive the process's options): every document parse in the
native CLI's own modules (`main.rs` 15 sites, `causal.rs` 1, `verification.rs`
1), `fsl-lsp` `DocumentIndex::build` and its re-lexing (`index.rs`), and the
Worker's surface parse before verification (`fsl-wasm/src/lib.rs`).

## Known limits of the inventory test

- A `ParseOptions` produced by `Default::default()` through type inference or
  a derived `Default` on a containing struct is not visible to a syntax-tree
  scan. The language server's `ServerState` derives `Default` and its start
  overrides the field; that is the only such holder at S0.
- Calls through trait objects or function values received from elsewhere are
  attributed to the file that names the entry, not to the caller.
