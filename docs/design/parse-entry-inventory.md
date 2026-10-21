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
  `lexical_warnings`, `IdentifierRule::for_source`, `edition_header`,
  `document_edition`, `lossless_document_with`, `format_source_with`,
  `canonical_rewrites_with`, and in fsl-core one `_with_options` entry beside
  each function that reads source text (`parse_kernel_source_with_options`,
  `governance_contract_with_options`, ...). The old
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
- The behaviour behind the feature is the identifier rule and the edition
  header below. R0
  (`rust/fslc/tests/unstable_gate_r0.rs`) holds that for the whole
  `specs/` + `examples/` corpus: `fslc kernel` and `fslc check` print the same
  bytes and exit the same with the gate off and on. It is the merge condition
  for every stage behind the gate.

## Identifier rule (S2)

`IdentifierRule::for_source(source, options)` picks the lexer's identifier
rule. It is `Unicode` only when `fsl5` is enabled **and** the first significant
line (after a BOM, whitespace and `//` comments, as the lexer skips them) is
`fsl`, blanks, `5`; otherwise `Ascii`, the published `[A-Za-z_][A-Za-z0-9_]*`.
The rule reads the header through `edition_header` (below). An ASCII-only
document lexes to the same tokens under both rules.

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
emitted by alias resolution (S3, below), not by the lexer. Errors are lexer/parse errors with the code
as `diagnostic_code`; warnings come from `lexical_warnings` and are published by
the language server. Commands that parse through the shared modules (`check`,
`kernel`, the language server's diagnostics) receive the options and see the
rule. Rename accepts a new name only if it is
one identifier under the rule of every document it edits.

## Edition header (S1)

`edition_header(source, options)` reads the header before lexing (J2). With
`fsl5` off it returns nothing, so a header stays the unknown declaration `fsl`.
With it on, the first significant line is a header when it is `fsl`, blanks,
decimal digits, and only blanks to the end of the line; `fsl` followed by
anything else is an identifier, so `const fsl = 1` keeps its meaning.

- `fsl 4` and `fsl 5` are the editions (`Edition::Fsl4` / `Fsl5`); no header
  is `fsl 4`. Any other number is `unsupported_edition` at the header.
- `lex_with` drops the two header tokens, so dispatch and every frontend see
  the document without it; the lossless tree (`lossless_document_with`) keeps
  them, and `fmt` prints the header as written.
- `ParsedDocument::edition` carries the edition. A header on a document whose
  declaration has no syntax edition (`causal`, the ai dialects and `agent`) is
  `edition_header_not_accepted` (decisions/049 D8).
- Literate Markdown blanks every non-fsl line, so a header on the first line
  of the first fsl fence is the first significant line; extraction does not
  parse and needs no options.
- `Edition` is the one edition type: the CLI's `--edition`, `fmt`, `lint` and
  `migrate` all hold `Option<Edition>` (`None` = `current`). `--edition 4|5` is
  accepted only with the gate; `next` is `5`. `document_edition` resolves the
  header against an explicit `--edition`; a mismatch is `edition_conflict`
  (exit 2) at the header.
- A document read in `fsl 5` behaves as `--edition next` does: every migration
  finding is an error (decisions/049 D7 keeps the codes). The language server
  publishes migration diagnostics of such a document as errors.
- `lint` / `migrate` for edition 5 report `missing_edition_header` (severity
  `info`) on a document without a header, and `migrate` inserts `fsl 5` before
  the first significant line. Both stay outside `plan_migration`, which
  `check --edition next` turns into errors.
- `check` / `verify` add `fsl_edition` only when a header was read, so a
  document without one prints the same bytes with the gate on (R0). The
  verify cache key adds the unstable set and the exact `--edition` request
  when the gate is on; the header itself is part of the hashed source.
- The Worker applies the same edition step to a document with a header.

## Glossary and alias resolution (S3)

- `glossary { Canonical = alias  Entity.field = alias ... }` is read only in a
  `requirements` document headed `fsl 5` with the gate on, and only as its
  first item (`misplaced_glossary` otherwise). Elsewhere `glossary` is still
  `expected requirements declaration`. The frontends learn the edition through
  `SourceFile`, which `parse_document_with` stamps after reading the header.
- Resolution starts on the token stream (`fsl-syntax/src/glossary.rs`): the
  block is checked on its own (`canonical_name_not_ascii` for a left side,
  `duplicate_alias`, `alias_collision`, `alias_shadows_keyword` against
  `KEYWORD_SPELLINGS`, the union of every keyword spelling, `alias_shadows_name`
  against the block's canonical names), then every later identifier token
  that is an alias becomes its canonical name (a field alias becomes the
  field name), so every frontend position reads canonical names. The body of
  `implements NAME from "path" { ... }` is not rewritten; an alias there is
  `alias_in_implements`. After the parse, a non-ASCII identifier that is
  neither an alias nor a keyword spelling is `canonical_name_not_ascii`, so
  nothing non-ASCII can reach a Kernel name. `Glossary::uses` records each
  resolved alias and each canonical spelling of an aliased name.
- `fsl_core::lower_requirements` (`resolve_aliases.rs`) lowers the resolved
  document, rejects an alias spelled like a declared name of the lowered spec
  (`alias_shadows_name`, e.g. a generated `claim_stage`), and publishes the
  table as `NameMap::glossary()` (`fsl_core::Glossary`). No Kernel name and no
  `PublicName` is ever an alias.
- Display only: `check` (shared with the Worker through `check_stages`) shows
  `'alias（Canonical）'` for each quoted canonical name and adds `name` /
  `display` to a rejection; `explain` shows aliases by default for a document
  with a glossary and canonical names with `--names canonical`; `lint
  --strict-names` reports `mixed_names` (warning) where a canonical name is
  also written as itself. `--names` and `--strict-names` exist only behind
  the gate.
- Not yet: the language server (hover, completion, rename of aliases; S11),
  `fmt --names` (S10), and the document glossary, whose project parse is
  pinned to the published language until S10 (`DOCUMENT` rows below).

## Fail closed (decisions/049 D2)

A parse that does not receive the process's options reads the published
language. Under the published language a document that starts with an edition
header (`fsl 5`, `fsl 4`) is the unknown declaration `fsl` and is rejected with
`FSL-DIALECT-UNKNOWN`, exactly as today
(`dispatch::tests::edition_header_fails_closed_under_stable_options`). So a
command whose first parse ran behind the gate and whose later re-parse did not
stops with an error; it never reads one file two ways. S1 kept the stable side
of that test and routed the re-parses; the rows below stay on the published
language by design or until the named stage.

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
published-language entry itself, a fixed published-language input, or a parity
tool (stays); `D8` = causal / ai dialects, where decisions/049 D8 makes an
edition header an error, so the published language is the intended reading;
`S10` = the document projection, which takes the options with the document
work of the next edition.

| Reason | Meaning |
|---|---|
| CAUSAL_AI | causal / ai dialects (D8) |
| STANDALONE_FRONTEND | public single-dialect parsers with no product caller; the registry frontends reuse the routed tokens instead; they stay on the published language (fail closed) |
| FRAGMENT | expression / spec fragment parsers without a header; the identifier rule is chosen by the header, so they always read ASCII identifiers |
| PARITY_TOOL | Python-parity debug binaries; the frozen reference reads only the published language |
| REGISTRY | the private `domain` frontend wrapper shares the public entry's name |
| STABLE_WRAPPER | the published-language entry itself (fsl-syntax, and fsl-core's wrapper beside each `_with_options` entry) |
| DIRECT_KERNEL | `parse_direct_kernel_spec`, whose only callers are tests |
| FIXTURE | a fixed in-repository fixture in the published language, never a user document |
| DOCUMENT | document claims / render (#1243), fail closed until S10 |

| File | Entry | Count | Owner | Reason |
|---|---|---|---|---|
| `fsl-core/src/bin/fsl-parse-kernel.rs` | `parse_kernel_source` | 1 | Definition | PARITY_TOOL |
| `fsl-core/src/compose.rs` | `ParseOptions::stable` | 3 | Definition | STABLE_WRAPPER |
| `fsl-core/src/dialect.rs` | `ParseOptions::stable` | 5 | Definition | STABLE_WRAPPER |
| `fsl-core/src/lib.rs` | `ParseOptions::stable` | 4 | Definition | STABLE_WRAPPER |
| `fsl-core/src/lib.rs` | `parse_document` | 1 | Definition | DIRECT_KERNEL |
| `fsl-core/src/refinement.rs` | `ParseOptions::stable` | 2 | Definition | STABLE_WRAPPER |
| `fsl-core/src/refinement.rs` | `requirements_implements_with_bounds` | 1 | Definition | STABLE_WRAPPER |
| `fsl-lsp/src/index.rs` | `is_causal_source` | 1 | D8 | CAUSAL_AI |
| `fsl-runtime/src/bin/fsl-bfs.rs` | `parse_kernel_source` | 1 | Definition | PARITY_TOOL |
| `fsl-runtime/src/bin/fsl-explicit.rs` | `parse_kernel_source` | 1 | Definition | PARITY_TOOL |
| `fsl-runtime/src/bin/fsl-refine.rs` | `parse_kernel_source` | 1 | Definition | PARITY_TOOL |
| `fsl-runtime/src/bin/fsl-refine.rs` | `parse_refinement` | 1 | Definition | PARITY_TOOL |
| `fsl-syntax/src/ai.rs` | `lex` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/ai_project.rs` | `parse_ai_component` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/bin/fsl-parse-surface.rs` | `parse_surface_document` | 1 | Definition | PARITY_TOOL |
| `fsl-syntax/src/causal.rs` | `declaration_keyword` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/causal.rs` | `lex` | 1 | D8 | CAUSAL_AI |
| `fsl-syntax/src/db.rs` | `lex` | 1 | Definition | STANDALONE_FRONTEND |
| `fsl-syntax/src/dispatch.rs` | `ParseOptions::stable` | 2 | Definition | STABLE_WRAPPER |
| `fsl-syntax/src/dispatch.rs` | `parse_domain` | 1 | Definition | REGISTRY |
| `fsl-syntax/src/domain.rs` | `lex` | 1 | Definition | STANDALONE_FRONTEND |
| `fsl-syntax/src/lexer.rs` | `ParseOptions::stable` | 1 | Definition | STABLE_WRAPPER |
| `fsl-syntax/src/lossless.rs` | `ParseOptions::stable` | 3 | Definition | STABLE_WRAPPER |
| `fsl-syntax/src/parser.rs` | `ParseOptions::stable` | 1 | Definition | STABLE_WRAPPER |
| `fsl-syntax/src/parser.rs` | `lex` | 2 | Definition | FRAGMENT |
| `fsl-tools/src/causal.rs` | `ParseOptions::stable` | 1 | Definition | STABLE_WRAPPER |
| `fsl-tools/src/causal.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |
| `fsl-tools/src/document_project.rs` | `parse_document` | 2 | S10 | DOCUMENT |
| `fsl-tools/src/document_project.rs` | `parse_kernel_source` | 1 | S10 | DOCUMENT |
| `fsl-tools/src/document_project.rs` | `requirements_trace_contract` | 1 | S10 | DOCUMENT |
| `fsl-tools/src/document_render.rs` | `requirements_trace_contract` | 1 | S10 | DOCUMENT |
| `fslc/src/bin/fsl-bmc.rs` | `parse_kernel_source` | 1 | Definition | PARITY_TOOL |
| `fslc/src/bin/fsl-replay-actions.rs` | `parse_kernel_source` | 1 | Definition | PARITY_TOOL |
| `fslc/src/causal.rs` | `parse_causal` | 2 | D8 | CAUSAL_AI |
| `fslc/src/coverage.rs` | `parse_kernel_source` | 1 | Definition | FIXTURE |
| `fslc/src/frontend_output.rs` | `declaration_keyword` | 1 | D8 | CAUSAL_AI |
| `fslc/src/frontend_output.rs` | `parse_ai_project` | 1 | D8 | CAUSAL_AI |
| `fslc/src/main.rs` | `is_causal_source` | 2 | D8 | CAUSAL_AI |
| `fslc/src/main.rs` | `parse_ai_project` | 3 | D8 | CAUSAL_AI |
| `fslc/src/main.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |
| `fslc/src/origin_coverage.rs` | `parse_kernel_source_with_file` | 1 | Definition | FIXTURE |
| `fslc/src/source_diagnostic.rs` | `is_causal_source` | 1 | D8 | CAUSAL_AI |
| `fslc/src/source_diagnostic.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |
| `fslc/src/verification.rs` | `is_causal_source` | 1 | D8 | CAUSAL_AI |
| `fslc/src/verification.rs` | `parse_causal` | 1 | D8 | CAUSAL_AI |

Routed at S0 (receive the process's options): every document parse in the
native CLI's own modules (`main.rs` 15 sites, `causal.rs` 1, `verification.rs`
1), `fsl-lsp` `DocumentIndex::build` and its re-lexing (`index.rs`), and the
Worker's surface parse before verification (`fsl-wasm/src/lib.rs`).

Routed at S1: fsl-core's source-reading functions (compose, dialect contracts,
refinement, scope overrides) through their `_with_options` entries; the shared
fslc modules (`check_stages`, `source_diagnostic`, `spec_load`, `reproducer`,
`verification_output`, `frontend_output`, `migration`), which take the options
as a parameter (the CLI passes its process value, the Worker the request's,
the language server its state's); every fsl-core call of the CLI binary; the
causal model's imported specs (`build_causal_model_with_options`,
`compile_expectations`); and the lossless tree, formatter and canonical
rewrites. Literate extraction is no longer listed: it does not parse.

## Known limits of the inventory test

- A `ParseOptions` produced by `Default::default()` through type inference or
  a derived `Default` on a containing struct is not visible to a syntax-tree
  scan. The language server's `ServerState` derives `Default` and its start
  overrides the field; that is the only such holder at S0.
- Calls through trait objects or function values received from elsewhere are
  attributed to the file that names the entry, not to the caller.
